// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Talking to `coaxd`, which owns the hardware.
//!
//! The daemon listens on a Unix domain socket by default -- on Windows too --
//! and on TCP as well when it was started with `--http`. Both are spoken here
//! on every system, because the socket is what a daemon nobody configured is
//! listening on.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::delivery;
use crate::http::{self, Response};
use crate::json;
use crate::launch::{self, Launcher};

#[cfg(windows)]
pub mod unix_socket;
// The same shape over the standard library's own, which has it everywhere the
// Windows module is not needed.
#[cfg(not(windows))]
#[path = "daemon/unix_socket_std.rs"]
pub mod unix_socket;

/// Where the daemon is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// A path to a Unix domain socket, which is the default.
    Socket(String),
    /// A `host:port`, for a daemon started with `--http`.
    Network(String),
}

impl Endpoint {
    /// Reads what a configuration file says, or falls back to where a daemon
    /// nobody configured is.
    pub fn parse(written: &str) -> Self {
        let written = written.trim();
        if written.is_empty() {
            return Self::default_endpoint();
        }
        Self::written(written)
    }

    /// Reads something that names a daemon: a path has a separator or a drive
    /// letter; an address has a colon and a number after it.
    fn written(written: &str) -> Self {
        match written.rsplit_once(':') {
            Some((host, port))
                if !port.is_empty()
                    && port.chars().all(|digit| digit.is_ascii_digit())
                    && Self::is_a_host(host) =>
            {
                Self::Network(written.to_owned())
            }
            _ => Self::Socket(written.to_owned()),
        }
    }

    /// Where the daemon is when nobody said.
    ///
    /// The environment comes first, in the order `coax` reads them -- see its
    /// `DefaultAddress`. `COAX_HOST` may name either a socket or an address;
    /// `COAXD_SOCKET` is a path by definition. Failing both, it is the socket the
    /// daemon opens by itself, worked out the same way the daemon works it out.
    pub fn default_endpoint() -> Self {
        Self::default_from(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
    }

    /// The rule above, over a given environment, so it can be checked without
    /// setting variables in the running process.
    fn default_from(named: impl Fn(&str) -> Option<String>) -> Self {
        if let Some(host) = named("COAX_HOST") {
            return Self::written(host.trim());
        }
        if let Some(socket) = named("COAXD_SOCKET") {
            return Self::Socket(socket);
        }
        Self::Socket(
            std::path::Path::new(&runtime_directory())
                .join("http.sock")
                .to_string_lossy()
                .into_owned(),
        )
    }

    /// Whether what stands before the last colon can be the host half of an
    /// address.
    ///
    /// It can, unless it holds a colon of its own without brackets around it:
    /// that is a bare IPv6 address, and `::1` split at its last colon is not
    /// the host `::` on port 1 -- which Rust would resolve and connect to. An
    /// address with a port is written `[::1]:40772`.
    fn is_a_host(candidate: &str) -> bool {
        !candidate.contains(':') || (candidate.starts_with('[') && candidate.ends_with(']'))
    }

    /// The name to put in the `Host` header. The daemon does not route on it, but
    /// HTTP/1.1 says a request carries one.
    pub(crate) fn host_header(&self) -> &str {
        match self {
            Self::Socket(_) => "coaxd",
            Self::Network(address) => address,
        }
    }
}

/// The directory the daemon keeps its sockets in. `/run` exists only on Linux
/// and only root may write to it; everywhere else it is the platform's per-user
/// runtime directory, because the daemon's devices are the ones that user may
/// open.
pub(crate) fn runtime_directory() -> String {
    let named = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    if let Some(directory) = named("COAXD_SOCKET_DIR") {
        return directory;
    }
    #[cfg(windows)]
    {
        let base = named("LOCALAPPDATA")
            .or_else(|| named("TEMP"))
            .unwrap_or_default();
        std::path::Path::new(&base)
            .join("coaxd")
            .to_string_lossy()
            .into_owned()
    }
    #[cfg(target_os = "macos")]
    {
        match named("TMPDIR") {
            Some(base) => std::path::Path::new(&base)
                .join("coaxd")
                .to_string_lossy()
                .into_owned(),
            None => format!("/tmp/coaxd-{}", user_id()),
        }
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        if user_id() == 0 {
            // The daemon serves other users' programs, so its sockets live
            // where they can see them.
            return "/run/coaxd".to_owned();
        }
        if let Some(base) = named("XDG_RUNTIME_DIR") {
            return std::path::Path::new(&base)
                .join("coaxd")
                .to_string_lossy()
                .into_owned();
        }
        format!(
            "{}/coaxd-{}",
            std::env::temp_dir().to_string_lossy().trim_end_matches('/'),
            user_id()
        )
    }
}

/// Who is running, for the directory a per-user daemon uses. Read from the
/// environment where the call is not there to make: this only has to agree with
/// the daemon.
#[cfg(not(windows))]
pub(crate) fn user_id() -> u32 {
    // SAFETY: getuid takes nothing and cannot fail.
    unsafe extern "C" {
        safe fn getuid() -> u32;
    }
    getuid()
}

/// How long a read of the daemon may block for when nothing has asked for less.
///
/// Long enough for a tuner that has to build a graph and wait for a lock, and
/// short enough that a daemon which has stopped answering is noticed rather
/// than waited on for ever.
const WAIT: Duration = Duration::from_secs(30);

/// How much a request's work matters, as the daemon weighs it against
/// everything else asking for a tuner.
///
/// When nothing is free, the daemon takes a tuner from work that asked at a
/// lower number; work that asked at the same one it leaves alone.
/// [`DEFAULT_PRIORITY`] is what a request that says nothing is worth, so a
/// driver that never sets one behaves as it always did.
///
/// The field name is this crate's, not HTTP's, and the daemon reads no other
/// spelling.
pub const PRIORITY_HEADER: &str = "X-Coax-Priority";

/// What a request that says nothing is worth.
pub const DEFAULT_PRIORITY: i32 = 0;

/// The header a request carries when it has something to say about its priority.
/// Nothing is sent for the default, so a daemon too old to know the header never
/// sees one.
fn priority_header(priority: i32) -> Option<String> {
    (priority != DEFAULT_PRIORITY).then(|| priority.to_string())
}

/// One connection to the daemon. A connection is also the claim on a tuner --
/// the daemon holds it for as long as the connection is open -- so a stream is
/// kept by keeping this.
#[derive(Debug)]
pub enum Connection {
    Network(TcpStream),
    Socket(unix_socket::Socket),
}

impl Read for Connection {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Network(stream) => stream.read(into),
            Self::Socket(socket) => socket.read(into),
        }
    }
}

impl Write for Connection {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        match self {
            Self::Network(stream) => stream.write(data),
            Self::Socket(socket) => socket.write(data),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Network(stream) => stream.flush(),
            Self::Socket(socket) => socket.flush(),
        }
    }
}

impl Connection {
    /// Opens one to where the daemon is.
    pub fn to(endpoint: &Endpoint) -> io::Result<Self> {
        match endpoint {
            Endpoint::Network(address) => {
                let stream = connect_within(address, CONNECT_PATIENCE).map_err(|reason| {
                    io::Error::new(
                        reason.kind(),
                        format!("reach the daemon at {address}: {reason}"),
                    )
                })?;
                stream.set_nodelay(true)?;
                Ok(Self::Network(stream))
            }
            Endpoint::Socket(path) => Ok(Self::Socket(unix_socket::Socket::connect(path)?)),
        }
    }

    /// How long a single read of this connection may block for.
    pub fn set_read_timeout(&self, patience: Duration) -> io::Result<()> {
        match self {
            Self::Network(stream) => stream.set_read_timeout(Some(patience)),
            Self::Socket(socket) => socket.set_timeout(patience),
        }
    }

    /// A handle that can end a read this connection is waiting in, from another
    /// thread.
    ///
    /// A reader blocked on the daemon has nothing to poll and no deadline worth
    /// waiting out: the way back is to close the connection under it. The
    /// handle is separate because the connection is inside the reader by then.
    pub fn interrupter(&self) -> io::Result<Interrupter> {
        match self {
            Self::Network(stream) => Ok(Interrupter::Network(Arc::new(stream.try_clone()?))),
            Self::Socket(socket) => Ok(Interrupter::Socket(socket.interrupter())),
        }
    }
}

/// How long reaching a daemon over the network may take. A machine that has
/// gone away does not refuse a connection, it just never answers, and the
/// system's own patience for that is the better part of a minute -- spent, in
/// a driver, on the host's thread.
const CONNECT_PATIENCE: Duration = Duration::from_secs(5);

/// Connects to the first of an address's resolutions that answers in time.
fn connect_within(address: &str, patience: Duration) -> io::Result<TcpStream> {
    use std::net::ToSocketAddrs;

    let mut last = None;
    for resolved in address.to_socket_addrs()? {
        match TcpStream::connect_timeout(&resolved, patience) {
            Ok(stream) => return Ok(stream),
            Err(reason) => last = Some(reason),
        }
    }
    Err(last.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "the address resolved to nothing")
    }))
}

/// Ends a read another thread is waiting in.
#[derive(Debug, Clone)]
pub enum Interrupter {
    Network(Arc<TcpStream>),
    Socket(unix_socket::Interrupter),
}

impl Interrupter {
    pub fn interrupt(&self) {
        match self {
            Self::Network(stream) => {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
            Self::Socket(socket) => socket.interrupt(),
        }
    }
}

/// A tuner identifier as a path can carry it. The identifiers hold colons and
/// at signs, which a path may hold as they stand, and little else.
fn escape(tuner: &str) -> String {
    let mut written = String::with_capacity(tuner.len());
    for byte in tuner.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b':' | b'@' => {
                written.push(byte as char);
            }
            _ => written.push_str(&format!("%{byte:02X}")),
        }
    }
    written
}

/// Adds what the settings say a tuner receives to what the tuner said about
/// itself: for every route, its delivery system goes onto every tuner the route
/// names.
///
/// It exists because a tuner cannot always be asked. A Windows BDA tuner is
/// described by what its driver registered, and plenty of cards register one
/// standard of several or none at all. A route is a viewer saying "this tuner
/// receives this", and that is taken over the driver's word; what the tuner
/// said is kept beside it.
pub fn declare(said: &[(String, Vec<String>)], tuners: &mut [Tuner]) {
    for (system, patterns) in said {
        for tuner in tuners.iter_mut() {
            if !patterns.iter().any(|pattern| tuner.matches(pattern)) {
                continue;
            }
            if !tuner
                .deliveries
                .iter()
                .any(|known| delivery::same(known, system))
            {
                tuner.deliveries.push(system.clone());
            }
        }
    }
}

/// The tuners a set of patterns names, the free ones first.
///
/// A pattern is what the settings hold -- `example:*` rather than an identifier --
/// and the daemon takes identifiers. The free ones come first because a tuner
/// someone else holds is refused. Nothing matching gives back the patterns that
/// name a tuner outright, and failing that a single unnamed candidate, leaving
/// the choice to the daemon.
pub fn candidates(tuners: &[Tuner], patterns: &[String], system: &str) -> Vec<Option<String>> {
    if patterns.is_empty() {
        return vec![None];
    }
    let mut free = Vec::new();
    let mut busy = Vec::new();
    for tuner in tuners {
        if !patterns.iter().any(|pattern| tuner.matches(pattern)) || !tuner.receives(system) {
            continue;
        }
        if tuner.leased {
            busy.push(Some(tuner.id.clone()));
        } else {
            free.push(Some(tuner.id.clone()));
        }
    }
    free.extend(busy);
    if !free.is_empty() {
        return free;
    }
    let named: Vec<Option<String>> = patterns
        .iter()
        .filter(|pattern| !pattern.contains('*'))
        .map(|pattern| Some(pattern.clone()))
        .collect();
    if named.is_empty() { vec![None] } else { named }
}

/// What a pattern says when it names a model rather than a tuner, which is what
/// `model:` in front of it means.
fn model_pattern(pattern: &str) -> Option<&str> {
    let pattern = pattern.trim();
    let (front, rest) = pattern.split_at_checked(6)?;
    front.eq_ignore_ascii_case("model:").then_some(rest)
}

/// A tuner held across several channels: one connection, kept open, because the
/// daemon's lease lives and dies with it.
///
/// The connection is put back after every answer that was read to its end. One
/// that was not -- a stream a caller walked away from -- is dropped, and the
/// next request opens another.
#[derive(Debug)]
pub struct Lease {
    endpoint: Endpoint,
    reader: Option<std::io::BufReader<Connection>>,
    /// The tuner the daemon chose, learnt from the first answer and named in
    /// every request after it so that the same one is retuned.
    tuner: Option<String>,
    /// What this work is worth, sent with every request that takes or keeps the
    /// tuner.
    priority: i32,
}

/// What a tune answered: which tuner did it, whether it locked, and what it
/// measured.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tuned {
    pub tuner: String,
    pub locked: bool,
    pub cnr: Option<f32>,
    pub strength: Option<f32>,
    /// The driver names a satellite stream by its place on the transponder,
    /// and has this many places. `None` for the drivers that take the stream's
    /// identifier itself, which is nearly all of them.
    pub relative_streams: Option<u32>,
}

impl Lease {
    /// Which tuner the daemon gave this, once it has said.
    pub fn tuner(&self) -> Option<&str> {
        self.tuner.as_deref()
    }

    /// Sets the tuner to a channel without waiting for the lock, which is the
    /// caller's to wait for: a walk that waits itself can count the seconds up
    /// in front of a viewer and give up on its own terms. `coax` asks the same
    /// way, see `NoLockWait`.
    pub fn tune(&mut self, properties: &[(&str, String)]) -> io::Result<Tuned> {
        let mut asked = self.asking(properties);
        asked.push(("lock_timeout", "0s".to_owned()));
        let mut answer = self.ask("/tune", &asked)?;
        let body = answer.read_to_end()?;
        let status = answer.status;
        self.recover(answer);
        let said = json::parse(&String::from_utf8_lossy(&body)).map_err(io::Error::other)?;
        if status != 200 {
            self.learn_refused(&said);
            return Err(io::Error::other(format!(
                "the daemon refused the channel: {status} {}",
                String::from_utf8_lossy(&body).trim()
            )));
        }
        let number = |name: &str| {
            said.get(name).and_then(|value| match value {
                json::Value::Number(digits) => digits.parse::<f32>().ok(),
                _ => None,
            })
        };
        let tuned = Tuned {
            tuner: said.text("tuner").unwrap_or_default().to_owned(),
            locked: said
                .get("locked")
                .and_then(json::Value::as_bool)
                .unwrap_or(false),
            cnr: number("cnr"),
            strength: number("strength"),
            relative_streams: number("relative_streams")
                .filter(|places| *places >= 1.0)
                .map(|places| places as u32),
        };
        if !tuned.tuner.is_empty() {
            self.tuner = Some(tuned.tuner.clone());
        }
        Ok(tuned)
    }

    /// Opens the stream of the channel this is tuned to, bounded so that the
    /// daemon ends it: a body that ends can be read to its end, and that is
    /// what leaves the connection -- and the tuner on it -- for the next
    /// channel.
    pub fn stream(
        &mut self,
        properties: &[(&str, String)],
        most: Option<usize>,
        longest: Option<Duration>,
        patience: Option<Duration>,
    ) -> io::Result<Response<Connection>> {
        let mut asked = self.asking(properties);
        if let Some(most) = most {
            asked.push(("bytes", most.to_string()));
        }
        if let Some(longest) = longest {
            asked.push(("for", format!("{}ms", longest.as_millis())));
        }
        let answer = self.ask("/stream", &asked)?;
        if answer.status != 200 {
            let mut answer = answer;
            let said = answer
                .read_to_end()
                .map(|body| String::from_utf8_lossy(&body).trim().to_owned())
                .unwrap_or_default();
            let status = answer.status;
            let reason = answer.reason.clone();
            self.recover(answer);
            if let Ok(refused) = json::parse(&said) {
                self.learn_refused(&refused);
            }
            return Err(io::Error::other(format!(
                "the daemon refused the channel: {status} {reason} {said}"
            )));
        }
        // Only once the answer is a stream (see `Daemon::stream_waiting`).
        if let Some(patience) = patience {
            answer.get_ref().set_read_timeout(patience)?;
        }
        if let Some(named) = answer.header("x-coax-tuner") {
            self.tuner = Some(named.to_owned());
        }
        Ok(answer)
    }

    /// Learns the tuner from a refusal that says which one it was.
    ///
    /// **A tune that failed still holds the tuner on this connection.** The
    /// daemon hands a held tuner back only to a request that names it, so a
    /// walk whose first channel was refused (a frequency the tuner cannot
    /// reach) and that went on asking without a name was told the only tuner
    /// was busy -- by itself -- for every channel after.
    fn learn_refused(&mut self, said: &json::Value) {
        if let Some(tuner) = said
            .get("status")
            .and_then(|status| status.text("tuner"))
            .filter(|tuner| !tuner.is_empty())
        {
            self.tuner = Some(tuner.to_owned());
        }
    }

    /// Takes the connection back from an answer that has been read to its end,
    /// so that the tuner stays held for the next channel. One that was not read
    /// to the end is let go of, which gives the tuner back.
    pub fn recover(&mut self, answer: Response<Connection>) {
        if !answer.reusable() || !answer.finished() {
            return;
        }
        let reader = answer.into_reader();
        // The patience a stream was read with was the walk's, and the request
        // after it is a question rather than a stream.
        if reader.get_ref().set_read_timeout(WAIT).is_err() {
            return;
        }
        self.reader = Some(reader);
    }

    /// Gives the tuner back.
    pub fn release(&mut self) {
        self.reader = None;
        self.tuner = None;
    }

    /// The properties of a request, with the tuner named where one has been
    /// chosen: the daemon would otherwise be free to pick another.
    fn asking<'a>(&self, properties: &[(&'a str, String)]) -> Vec<(&'a str, String)> {
        let mut asked: Vec<(&'a str, String)> = properties.to_vec();
        if let Some(tuner) = &self.tuner
            && !asked.iter().any(|(name, _)| *name == "tuner")
        {
            asked.push(("tuner", tuner.clone()));
        }
        asked
    }

    /// What this lease's work is worth, sent on every request it makes.
    pub fn priority(&self) -> i32 {
        self.priority
    }

    fn ask(
        &mut self,
        path: &str,
        properties: &[(&str, String)],
    ) -> io::Result<Response<Connection>> {
        let reader = match self.reader.take() {
            Some(reader) => reader,
            None => std::io::BufReader::new(Connection::to(&self.endpoint)?),
        };
        let priority = priority_header(self.priority);
        let headers: Vec<http::Header<'_>> = priority
            .as_deref()
            .map(|value| (PRIORITY_HEADER, value))
            .into_iter()
            .collect();
        http::kept(
            reader,
            &format!("{path}{}", http::query(properties)),
            self.endpoint.host_header(),
            &headers,
        )
    }
}

/// What a tuner reports about what it is receiving. Both are optional because
/// both are the backend's to measure: one card reports a ratio, another only a
/// level, and a third neither.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seen {
    /// Carrier to noise, in decibels.
    pub cnr: Option<f32>,
    /// Level, in decibel-milliwatts.
    pub strength: Option<f32>,
    /// Whether the demodulator has the carrier. Nothing where the backend does
    /// not say, which is not the same as saying no.
    pub locked: Option<bool>,
    /// What the daemon made of the bytes leaving the tuner: which multiplex it
    /// lined up as -- `ts` or `tlv` -- and how many packets it found. Empty and
    /// zero where nothing lined up, which is what a demodulator locked to noise
    /// produces.
    pub format: String,
    pub packets: u64,
    pub sync_losses: u64,
    /// Everything the tuner produced, what could not be corrected, what went
    /// missing between one packet and the next in its own run, and what was
    /// thrown away because nothing was reading fast enough. Together they say
    /// which of three failures a thin stream is.
    pub bytes: u64,
    pub errors: u64,
    pub discontinuities: u64,
    pub dropped: u64,
}

/// The boards a listing answered. Anything the answer does not carry is absent
/// rather than zero: a tuner nobody holds reports no lock and no figures, and a
/// reading of zero decibels is a reading.
fn boards_of(said: &str) -> io::Result<Vec<Board>> {
    boards_from(&json::parse(said).map_err(io::Error::other)?)
}

/// The same, from the answer already read as JSON.
fn boards_from(boards: &json::Value) -> io::Result<Vec<Board>> {
    let text = |value: &json::Value, name: &str| value.text(name).unwrap_or_default().to_owned();
    let strings = |value: &json::Value, name: &str| -> Vec<String> {
        value
            .get(name)
            .map(|list| {
                list.items()
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let number = |value: &json::Value, name: &str| {
        value.get(name).and_then(|value| match value {
            json::Value::Number(digits) => digits.parse::<f32>().ok(),
            _ => None,
        })
    };
    let whole = |value: &json::Value, name: &str| {
        value.get(name).and_then(|value| match value {
            json::Value::Number(digits) => digits.parse::<i32>().ok(),
            _ => None,
        })
    };
    let pairs = |value: &json::Value, name: &str| -> Vec<(String, String)> {
        value
            .get(name)
            .map(|list| {
                list.items()
                    .iter()
                    .filter_map(|item| {
                        Some((
                            item.text("name")?.to_owned(),
                            item.text("value")?.to_owned(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut listing = Vec::new();
    for board in boards.items() {
        let model = match board.text("description") {
            Some(description) if !description.is_empty() => description.to_owned(),
            _ => text(board, "name"),
        };
        let mut held = Vec::new();
        for tuner in board
            .get("tuners")
            .map(json::Value::items)
            .unwrap_or_default()
        {
            let Some(id) = tuner.text("id") else {
                continue;
            };
            held.push(Held {
                tuner: Tuner {
                    id: id.to_owned(),
                    model: model.clone(),
                    deliveries: strings(tuner, "deliveries"),
                    leased: tuner
                        .get("leased")
                        .and_then(json::Value::as_bool)
                        .unwrap_or(false),
                    name: text(tuner, "name"),
                },
                locked: tuner.get("locked").and_then(json::Value::as_bool),
                cnr: number(tuner, "cnr"),
                strength: number(tuner, "strength"),
                tuned: pairs(tuner, "tuned"),
                priority: whole(tuner, "priority"),
                controls: controls_of(tuner),
            });
        }
        listing.push(Board {
            name: text(board, "name"),
            model,
            backend: text(board, "backend"),
            location: text(board, "location"),
            serial: text(board, "serial"),
            tuners: held,
        });
    }
    Ok(listing)
}

/// One board of a listing: what the daemon calls it, what it is, and the tuners
/// on it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Board {
    /// The name the tuners' identifiers are built on, which is one card's alone.
    pub name: String,
    /// What the card is, which two cards of a model share.
    pub model: String,
    pub backend: String,
    pub location: String,
    pub serial: String,
    pub tuners: Vec<Held>,
}

/// A tuner and what it is doing, as far as the daemon can say without opening
/// anything: everything below the tuner itself is filled in only while somebody
/// holds it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Held {
    pub tuner: Tuner,
    /// Whether the demodulator has the carrier.
    pub locked: Option<bool>,
    /// Carrier to noise, in decibels, and the level in decibel-milliwatts.
    pub cnr: Option<f32>,
    pub strength: Option<f32>,
    /// The channel it is set to, as the properties the tune took. Empty for a
    /// tuner nobody holds, and for one held but not yet tuned.
    pub tuned: Vec<(String, String)>,
    /// What the work holding this tuner said it was worth, and so what it takes
    /// to take the tuner from it. Nothing for a tuner nobody holds, and nothing
    /// from a daemon too old to say: zero is a real priority, so neither can be
    /// spelled as zero.
    pub priority: Option<i32>,
    /// What a channel may set on this tuner beside the channel, as the daemon
    /// knows it from the model. Empty from a tuner with nothing to set, and from
    /// a daemon too old to say.
    pub controls: Controls,
}

/// What a tuner lets a channel set beside where to tune (`/tuners` `controls`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Controls {
    /// The supply on the lead for a dish (`lnb_power`), where it can be set.
    pub lnb: Option<LnbControl>,
    /// The amplifier (`gain`), where it can be set.
    pub gain: Option<GainControl>,
}

/// What `lnb_power` may ask of a tuner.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LnbControl {
    /// What it accepts: `off` where the supply can be switched off, then `low`
    /// and `high`.
    pub levels: Vec<String>,
    /// What the two levels are worth (`11V`, `15V`), empty for a supply that is
    /// only on or off.
    pub low: String,
    pub high: String,
    /// `board` or `card` where one supply feeds more than this tuner.
    pub shared: String,
}

impl LnbControl {
    /// Whether the supply can be switched off.
    pub fn off(&self) -> bool {
        self.levels.iter().any(|level| level == "off")
    }
}

/// What `gain` may ask of a tuner.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GainControl {
    /// The amplifier's steps in dB, least first; empty for one that is only in or
    /// out (`min`, `max`, `auto`).
    pub decibels: Vec<f64>,
    /// The systems it is set for, where it sits on one input only. Empty means
    /// every system the tuner receives.
    pub deliveries: Vec<String>,
}

/// Reads a tuner's `controls`.
fn controls_of(tuner: &json::Value) -> Controls {
    let Some(said) = tuner.get("controls") else {
        return Controls::default();
    };
    let strings = |value: &json::Value, name: &str| -> Vec<String> {
        value
            .get(name)
            .map(|list| {
                list.items()
                    .iter()
                    .filter_map(|item| match item {
                        json::Value::String(text) => Some(text.clone()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let text = |value: &json::Value, name: &str| value.text(name).unwrap_or_default().to_owned();
    Controls {
        lnb: said.get("lnb").map(|lnb| LnbControl {
            levels: strings(lnb, "levels"),
            low: text(lnb, "low"),
            high: text(lnb, "high"),
            shared: text(lnb, "shared"),
        }),
        gain: said.get("gain").map(|gain| GainControl {
            decibels: gain
                .get("decibels")
                .map(|list| {
                    list.items()
                        .iter()
                        .filter_map(|item| match item {
                            json::Value::Number(digits) => digits.parse::<f64>().ok(),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            deliveries: strings(gain, "deliveries"),
        }),
    }
}

/// One tuner as the daemon names it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tuner {
    /// The identifier the daemon takes as `tuner=`.
    pub id: String,
    /// The delivery systems it receives, as far as the backend knows without
    /// opening the card. Empty means it did not say, which is not the same as
    /// "none": a tuner that says nothing is tried rather than passed over.
    pub deliveries: Vec<String>,
    /// Whether someone else has it at the moment.
    pub leased: bool,
    /// What the board calls itself, as the daemon describes it: "Example 6200se
    /// Tuner 2".
    ///
    /// Kept so that a pattern can name a model rather than a tuner. Which
    /// standards a card demodulates is a fact about the model, and the
    /// identifiers are not: on a machine with several cards of one backend they
    /// are opaque and per tuner.
    pub model: String,
    /// What the daemon calls this one tuner of the board -- "satellite tuner
    /// 1" -- where it tells them apart. Empty where it does not.
    pub name: String,
}

impl Tuner {
    /// What to show a person: the model, and the tuner's own name after it
    /// where that says something the model does not.
    pub fn label(&self) -> String {
        if self.name.is_empty() || self.model.contains(&self.name) {
            self.model.clone()
        } else if self.model.is_empty() {
            self.name.clone()
        } else {
            format!("{} {}", self.model, self.name)
        }
    }

    /// Whether a pattern from the settings names this tuner. A trailing `*`
    /// matches the rest, which is how a card whose identifier carries a USB
    /// path is named without writing the path down.
    pub fn matches(&self, pattern: &str) -> bool {
        // A pattern may name the model instead. The names have spaces and
        // capitals in them and are read out of a driver, so the comparison
        // ignores case.
        if let Some(wanted) = model_pattern(pattern) {
            let model = self.model.to_ascii_lowercase();
            let wanted = wanted.trim().to_ascii_lowercase();
            return match wanted.strip_suffix('*') {
                Some(start) => model.starts_with(start),
                None => model == wanted,
            };
        }
        match pattern.strip_suffix('*') {
            Some(start) => self.id.starts_with(start),
            None => self.id == pattern,
        }
    }

    /// Whether it receives a delivery system. A tuner that lists none is taken
    /// at its word rather than ruled out: the list is what a backend knows
    /// before the card is opened, and it is allowed not to know.
    ///
    /// The two sides spell a delivery system differently, so both go through
    /// [`crate::delivery`] before they are compared.
    pub fn receives(&self, system: &str) -> bool {
        self.deliveries.is_empty()
            || self
                .deliveries
                .iter()
                .any(|known| delivery::same(known, system))
    }
}

/// How long to wait for a daemon that has just been started, and how often to
/// ask.
///
/// Half a minute, because the daemon opens its socket only once it has looked
/// at every card on the machine: measured on a machine with six boards, ten
/// seconds was not always enough, and giving up early shows a viewer a tuner
/// that will not open while the daemon behind it is starting normally.
const STARTUP_LIMIT: Duration = Duration::from_secs(30);
const STARTUP_POLL: Duration = Duration::from_millis(50);

/// The daemon, as this driver uses it.
#[derive(Debug, Clone)]
pub struct Daemon {
    endpoint: Endpoint,
    priority: i32,
    /// How long a question may wait for its answer.
    patience: Duration,
}

/// The process serving the daemon's socket ([`Daemon::serving`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Serving {
    pub pid: u32,
    /// Where its program is. `None` when the system would not say, which for a
    /// process of another user it may not.
    pub executable: Option<std::path::PathBuf>,
}

impl Daemon {
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            priority: DEFAULT_PRIORITY,
            patience: WAIT,
        }
    }

    /// How long a question that takes no tuner -- a listing, a version -- may
    /// wait for its answer. Shorter than the default for a caller asking on
    /// the thread that draws a window, where a daemon that has stopped
    /// answering would otherwise hold the window up for the whole default.
    pub fn answering_within(mut self, patience: Duration) -> Self {
        self.patience = patience;
        self
    }

    /// The same, weighing everything it asks for at this priority. One number
    /// for the whole driver rather than one per channel: what a television
    /// program's tuners are worth against a recorder's is a fact about the two
    /// programs.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// What this daemon's requests say they are worth.
    pub fn priority(&self) -> i32 {
        self.priority
    }

    fn connect(&self) -> io::Result<Connection> {
        Connection::to(&self.endpoint)
    }

    /// Makes sure something is answering, starting the daemon if nothing is.
    ///
    /// The wait is for an answer, not for a process: a daemon that has been
    /// started is not yet a daemon that is listening.
    ///
    /// Only a daemon on this machine is started. One named by `host:port` is
    /// somebody else's to run, and a local process started on hearing nothing
    /// from it listens on this machine's own socket and can never answer the
    /// address asked for.
    pub fn ensure_running(&self, launcher: &Launcher) -> io::Result<String> {
        if let Ok(version) = self.version() {
            return Ok(version);
        }
        let Endpoint::Socket(socket) = &self.endpoint else {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                format!(
                    "nothing is answering at {}; a daemon on another machine has to be started there, \
                     and with --http for it to be reachable at all",
                    self.endpoint.host_header()
                ),
            ));
        };
        // Only one starter per socket on this machine. A television host has
        // this library twice over, and all of them arriving at an unstarted
        // daemon at once should produce one daemon, not a race won by the
        // socket bind. See `launch::Claim`.
        let mut claim = None;
        let mut started = None;
        let deadline = Instant::now() + STARTUP_LIMIT;
        // Always set before it is read: the loop asks before it can end.
        let mut refusal;
        loop {
            if claim.is_none()
                && let Some(taken) = launch::Claim::take(socket)
            {
                // Asked again under the claim: whoever held it a moment ago
                // may have just succeeded and let go.
                if let Ok(version) = self.version() {
                    return Ok(version);
                }
                // A daemon that cannot be found is not worth waiting for,
                // and saying so now is better than at the end of the
                // deadline.
                started = Some(launcher.start(socket)?);
                claim = Some(taken);
            }
            match self.version() {
                Ok(version) => return Ok(version),
                Err(reason) => refusal = reason,
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(STARTUP_POLL);
        }
        drop(claim);
        Err(io::Error::other(match started {
            Some(daemon) => format!(
                "started {} but it did not answer within {}s: {refusal}",
                daemon.display(),
                STARTUP_LIMIT.as_secs()
            ),
            // Somebody else was starting one throughout, and it never
            // answered. Naming their daemon is not this side's to do.
            None => format!(
                "another program is starting a daemon on {socket} and it did not answer within {}s: {refusal}",
                STARTUP_LIMIT.as_secs()
            ),
        }))
    }

    /// Asks whether the daemon is there at all, which is what a driver wants to
    /// know before it says its tuner opened.
    pub fn version(&self) -> io::Result<String> {
        let mut answer = self.request("/version", &[])?;
        let body = answer.read_to_end()?;
        if answer.status != 200 {
            return Err(io::Error::other(format!(
                "the daemon answered {}",
                answer.status
            )));
        }
        Ok(String::from_utf8_lossy(&body).trim().to_owned())
    }

    /// Which process is serving the daemon's socket, and where its program is.
    /// Nothing is started: this is only about a daemon already there, which may
    /// be one somebody runs from somewhere else entirely. A daemon reached over
    /// the network is on another machine, so it has no answer here.
    pub fn serving(&self) -> io::Result<Serving> {
        let Endpoint::Socket(path) = &self.endpoint else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "a daemon reached over the network is not a process on this machine",
            ));
        };
        let socket = unix_socket::Socket::connect(path)?;
        let pid = socket.peer_process();
        socket.shutdown();
        let pid = pid?;
        Ok(Serving {
            pid,
            executable: crate::process::executable_of(pid).ok(),
        })
    }

    /// The tuners the daemon has, as far as it knows them without opening a
    /// card. A driver asks so that it can turn the patterns in its settings
    /// into the identifiers the daemon names tuners by.
    pub fn tuners(&self) -> io::Result<Vec<Tuner>> {
        Ok(self
            .listing()?
            .into_iter()
            .flat_map(|board| board.tuners)
            .map(|held| held.tuner)
            .collect())
    }

    /// The same listing, board by board, with what each tuner is doing.
    ///
    /// A board is what a card publishes rather than the card itself -- a
    /// Windows machine lists a four-tuner card as four boards -- so this is the
    /// daemon's own grouping. What a tuner is doing is filled in only where
    /// somebody holds it.
    pub fn listing(&self) -> io::Result<Vec<Board>> {
        let mut answer = self.request("/tuners", &[])?;
        let body = answer.read_to_end()?;
        if answer.status != 200 {
            return Err(io::Error::other(format!(
                "the daemon answered {} to /tuners",
                answer.status
            )));
        }
        boards_of(&String::from_utf8_lossy(&body))
    }

    /// Holds a tuner across several channels, on one connection.
    ///
    /// The daemon gives a tuner to the connection that asked for it and takes
    /// it back when that connection goes, so a walk that opens a connection per
    /// carrier may find the tuner gone when it asks again. `coax` does the
    /// same, see its `Device`.
    pub fn hold(&self, tuner: Option<&str>) -> Lease {
        Lease {
            endpoint: self.endpoint.clone(),
            reader: None,
            tuner: tuner.map(str::to_owned),
            priority: self.priority,
        }
    }

    /// What a tuner is seeing, as far as its backend can measure.
    ///
    /// Asked of a tuner that is streaming: the daemon counts a reader of this
    /// as an observer rather than a lease. Nothing there is not an error -- a
    /// backend that cannot measure says nothing.
    pub fn seen(&self, tuner: &str) -> io::Result<Seen> {
        let path = format!("/tuners/{}", escape(tuner));
        let mut answer = self.request(&path, &[])?;
        let body = answer.read_to_end()?;
        if answer.status != 200 {
            return Ok(Seen::default());
        }
        let said = json::parse(&String::from_utf8_lossy(&body)).map_err(io::Error::other)?;
        let number = |name: &str| {
            said.get(name).and_then(|value| match value {
                json::Value::Number(digits) => digits.parse::<f32>().ok(),
                _ => None,
            })
        };
        // What the daemon's own analyser made of the stream, which is there
        // only while something is being read off the tuner.
        let stream = said.get("stream");
        let counted = |name: &str| {
            stream
                .and_then(|stream| stream.get(name))
                .and_then(|value| match value {
                    json::Value::Number(digits) => digits.parse::<u64>().ok(),
                    _ => None,
                })
                .unwrap_or_default()
        };
        Ok(Seen {
            cnr: number("cnr"),
            strength: number("strength"),
            locked: said.get("locked").and_then(json::Value::as_bool),
            format: stream
                .and_then(|stream| stream.text("format"))
                .unwrap_or_default()
                .to_owned(),
            packets: counted("packets"),
            sync_losses: counted("sync_losses"),
            bytes: counted("bytes"),
            errors: counted("errors"),
            discontinuities: counted("discontinuities"),
            dropped: said
                .get("dropped")
                .and_then(|value| match value {
                    json::Value::Number(digits) => digits.parse::<u64>().ok(),
                    _ => None,
                })
                .unwrap_or_default(),
        })
    }

    /// The carrier-to-noise ratio alone, which is what a signal meter wants.
    pub fn cnr(&self, tuner: &str) -> io::Result<Option<f32>> {
        Ok(self.seen(tuner)?.cnr)
    }

    /// Opens the stream of a channel.
    ///
    /// The connection is inside the answer, and letting go of the answer is
    /// what gives the tuner back. The interrupter comes back beside it because
    /// by then the connection is out of reach.
    pub fn stream(
        &self,
        properties: &[(&str, String)],
    ) -> io::Result<(Response<Connection>, Interrupter)> {
        self.stream_waiting(properties, None)
    }

    /// The same, with how long a single read of the stream may block for.
    ///
    /// A walk holds a carrier for a few seconds and then moves on, so a read
    /// that blocks for the whole of the connection's patience makes an empty
    /// carrier cost six times what it was given. The caller takes a read that
    /// timed out as nothing having arrived rather than as the stream having
    /// broken.
    pub fn stream_waiting(
        &self,
        properties: &[(&str, String)],
        patience: Option<Duration>,
    ) -> io::Result<(Response<Connection>, Interrupter)> {
        // The tune happens before the answer's headers, so a stream waits the
        // whole default for them whatever a question would.
        let connection = self.connect()?;
        connection.set_read_timeout(WAIT)?;
        let interrupter = connection.interrupter()?;
        let priority = priority_header(self.priority);
        let headers: Vec<http::Header<'_>> = priority
            .as_deref()
            .map(|value| (PRIORITY_HEADER, value))
            .into_iter()
            .collect();
        let answer = http::get(
            connection,
            &format!("/stream{}", http::query(properties)),
            self.endpoint.host_header(),
            &headers,
        )?;
        if answer.status != 200 {
            // The refusal says why in its body -- which tuner, which
            // parameter, whether it was busy -- and a driver that dropped that
            // would leave a viewer with nothing but a number.
            let mut answer = answer;
            let said = answer
                .read_to_end()
                .map(|body| String::from_utf8_lossy(&body).trim().to_owned())
                .unwrap_or_default();
            return Err(io::Error::other(format!(
                "the daemon refused the channel: {} {} {said}",
                answer.status, answer.reason
            )));
        }
        // Set after the headers are read: the tune happens before them, and a
        // card that takes seconds to lock must not be cut short by the
        // patience a walk wants for the body. **And only once the answer is a
        // stream**: macOS refuses the option (EINVAL) on a socket the daemon
        // has already closed, which is what a refusal does -- setting it first
        // put "Invalid argument" in place of the daemon's reason.
        if let Some(patience) = patience {
            answer.get_ref().set_read_timeout(patience)?;
        }
        Ok((answer, interrupter))
    }

    /// Opens a connection, set up for the request that is about to go down it.
    fn open(&self) -> io::Result<Connection> {
        let connection = self.connect()?;
        connection.set_read_timeout(self.patience)?;
        Ok(connection)
    }

    fn request(
        &self,
        path: &str,
        parameters: &[(&str, String)],
    ) -> io::Result<Response<Connection>> {
        let connection = self.open()?;
        // No priority: these ask questions rather than take tuners, and the
        // daemon weighs nothing by them.
        http::get(
            connection,
            &format!("{path}{}", http::query(parameters)),
            self.endpoint.host_header(),
            &[],
        )
    }
}

#[cfg(test)]
mod tests {
    /// The recorded answers the daemon and its clients agree on.
    ///
    /// The same file, byte for byte, sits in the daemon's `hostapi` package and
    /// in its command line client, each with this checksum in a test of its
    /// own. The three programs share a protocol, not a build, so this is what
    /// catches the daemon renaming a field or moving one.
    /// A connection says who is at the other end: here, this process itself.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn a_connection_names_the_process_serving_it() {
        let path = std::env::temp_dir().join(format!("coax-peer-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = super::unix_socket::Listener::bind(&path).expect("bind");
        let socket = super::unix_socket::Socket::connect(&path.to_string_lossy()).expect("connect");
        let _accepted = listener.accept().expect("accept");
        assert_eq!(socket.peer_process().expect("named"), std::process::id());
        drop(listener);
        let _ = std::fs::remove_file(&path);
    }

    /// A daemon running on this machine says which program it is. Needs one
    /// running at the default socket, so it is left for when one is.
    #[test]
    #[ignore = "a daemon has to be running at the default socket"]
    fn the_running_daemon_names_its_program() {
        let serving = super::Daemon::new(super::Endpoint::default_endpoint())
            .serving()
            .expect("a daemon is serving the socket");
        println!("pid {} at {:?}", serving.pid, serving.executable);
        assert!(serving.executable.is_some());
    }

    const GOLDEN: &str = include_str!("../testdata/wire.json");
    const GOLDEN_SUM: &str = "5e9c2a94055a7fa98923df9b029ae4d6887bd9aa3493a53d5072998cd9798f3a";

    /// A very small SHA-256, for the one string this has to checksum. Writing
    /// it out is less than taking a dependency for it.
    fn sha256(data: &[u8]) -> String {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut hash: [u32; 8] = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];
        let mut message = data.to_vec();
        let bits = (data.len() as u64) * 8;
        message.push(0x80);
        while message.len() % 64 != 56 {
            message.push(0);
        }
        message.extend_from_slice(&bits.to_be_bytes());
        for block in message.as_chunks::<64>().0 {
            let mut w = [0u32; 64];
            for (at, word) in block.as_chunks::<4>().0.iter().enumerate() {
                w[at] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
            }
            for at in 16..64 {
                let s0 =
                    w[at - 15].rotate_right(7) ^ w[at - 15].rotate_right(18) ^ (w[at - 15] >> 3);
                let s1 =
                    w[at - 2].rotate_right(17) ^ w[at - 2].rotate_right(19) ^ (w[at - 2] >> 10);
                w[at] = w[at - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[at - 7])
                    .wrapping_add(s1);
            }
            let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = hash;
            for at in 0..64 {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let choice = (e & f) ^ ((!e) & g);
                let one = h
                    .wrapping_add(s1)
                    .wrapping_add(choice)
                    .wrapping_add(K[at])
                    .wrapping_add(w[at]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let majority = (a & b) ^ (a & c) ^ (b & c);
                let two = s0.wrapping_add(majority);
                h = g;
                g = f;
                f = e;
                e = d.wrapping_add(one);
                d = c;
                c = b;
                b = a;
                a = one.wrapping_add(two);
            }
            for (into, word) in hash.iter_mut().zip([a, b, c, d, e, f, g, h]) {
                *into = into.wrapping_add(word);
            }
        }
        hash.iter().map(|word| format!("{word:08x}")).collect()
    }

    /// Fails if this copy of the recorded answers has drifted from the
    /// daemon's, which would leave the test below agreeing with nothing.
    #[test]
    fn the_golden_file_is_the_one_the_daemon_has() {
        assert_eq!(
            sha256(GOLDEN.as_bytes()),
            GOLDEN_SUM,
            "testdata/wire.json and the daemon's copy must be the same file"
        );
    }

    /// The recorded listing reads as this side expects it to: what the daemon
    /// says about a tuner in use is read from the answer it records, not from a
    /// sample written here to match the code.
    #[test]
    fn the_recorded_listing_reads_as_a_board() {
        let said = crate::json::parse(GOLDEN).expect("the golden file is JSON");
        let board = said.get("board").expect("the golden file records a board");
        let listing =
            super::boards_from(&crate::json::Value::Array(vec![board.clone()])).expect("one board");
        assert_eq!(listing.len(), 1);
        let board = &listing[0];
        assert_eq!(board.name, "example:ABC12345@1-2.1");
        assert_eq!(board.model, "Example U2 (BR300 + DM100)");
        assert_eq!(board.backend, "example");
        assert_eq!(board.serial, "ABC12345");
        assert_eq!(board.tuners.len(), 2);
        let free = &board.tuners[0];
        assert!(!free.tuner.leased);
        assert_eq!(free.tuner.deliveries, ["ISDBS"]);
        assert!(free.tuned.is_empty());
        let held = &board.tuners[1];
        assert!(held.tuner.leased);
        assert_eq!(held.locked, Some(true));
        assert_eq!(held.cnr, Some(31.25));
        assert_eq!(held.strength, Some(-52.5));
        assert_eq!(
            held.tuned,
            vec![
                ("DELIVERY_SYSTEM".to_owned(), "ISDBT".to_owned()),
                ("FREQUENCY".to_owned(), "551142857".to_owned()),
            ]
        );
    }

    /// A listing carries the boards, the tuners on them, and what the ones
    /// somebody holds are doing. A tuner nobody holds says nothing about a
    /// lock or a channel, which is not the same as saying zero.
    #[test]
    fn a_listing_reads_as_boards_with_what_each_tuner_is_doing() {
        let said = r#"[
          {"name": "example:abc", "description": "Example 1234 Tuner 0",
           "backend": "example", "location": "PCI", "serial": "",
           "tuners": [
             {"id": "example:abc:0", "name": "tuner 0", "deliveries": ["ISDB-T", "DVB-C/ANNEX_A"],
              "leased": true, "priority": 5, "locked": true, "cnr": 29.7, "strength": -42.5,
              "tuned": [{"name": "DELIVERY_SYSTEM", "value": "DVBC/ANNEX_A"},
                        {"name": "FREQUENCY", "value": "471142857"}]},
             {"id": "example:abc:1", "name": "tuner 1", "leased": false}
           ]}
        ]"#;
        let boards = super::boards_of(said).expect("the answer is JSON");
        assert_eq!(boards.len(), 1);
        let board = &boards[0];
        assert_eq!(board.name, "example:abc");
        assert_eq!(board.model, "Example 1234 Tuner 0");
        assert_eq!(board.tuners.len(), 2);
        let held = &board.tuners[0];
        assert_eq!(held.tuner.id, "example:abc:0");
        // The model is the board's, so a pattern naming it names every tuner
        // on the card.
        assert_eq!(held.tuner.model, "Example 1234 Tuner 0");
        assert!(held.tuner.leased);
        assert_eq!(held.locked, Some(true));
        assert_eq!(held.cnr, Some(29.7));
        assert_eq!(held.strength, Some(-42.5));
        assert_eq!(
            held.tuned,
            vec![
                ("DELIVERY_SYSTEM".to_owned(), "DVBC/ANNEX_A".to_owned()),
                ("FREQUENCY".to_owned(), "471142857".to_owned()),
            ]
        );
        // What rank holds it, which is what a viewer refused a channel wants
        // to know: whether they can outrank whoever has it.
        assert_eq!(held.priority, Some(5));
        let free = &board.tuners[1];
        assert!(!free.tuner.leased);
        assert_eq!(free.locked, None);
        assert_eq!(free.cnr, None);
        assert!(free.tuned.is_empty());
        // Zero is a real priority, so a tuner nobody holds cannot report one.
        assert_eq!(free.priority, None);
    }

    /// A daemon that has never heard of priorities says nothing about them,
    /// and a listing from one still reads.
    #[test]
    fn a_tuner_says_what_a_channel_may_set_on_it() {
        let boards = super::boards_of(
            r#"[{"name":"example:A","description":"Example Q4","backend":"example","tuners":[
                {"id":"example:A:0","leased":false,"deliveries":["ISDB-S"],
                 "controls":{"lnb":{"levels":["off","low","high"],"shared":"board"},
                             "gain":{"kind":"steps","decibels":[0,1,2,3]}}},
                {"id":"example:A:2","leased":false,"deliveries":["ISDB-T"]}]}]"#,
        )
        .unwrap();
        let satellite = &boards[0].tuners[0].controls;
        let lnb = satellite.lnb.as_ref().unwrap();
        assert!(lnb.off());
        assert_eq!(lnb.shared, "board");
        assert_eq!(
            satellite.gain.as_ref().unwrap().decibels,
            [0.0, 1.0, 2.0, 3.0]
        );
        assert_eq!(
            boards[0].tuners[1].controls,
            super::Controls::default(),
            "指定しなければ何も無い"
        );
    }

    #[test]
    fn a_listing_from_a_daemon_that_says_no_priority_still_reads() {
        let said = r#"[
          {"name": "example:abc", "backend": "example",
           "tuners": [{"id": "example:abc:0", "leased": true, "locked": true}]}
        ]"#;
        let boards = super::boards_of(said).expect("the answer is JSON");
        let held = &boards[0].tuners[0];
        assert!(held.tuner.leased);
        assert_eq!(held.priority, None);
    }

    /// Which standards a card demodulates is a fact about the model, so a
    /// pattern may name the model. The identifiers cannot say it: they are
    /// opaque and per tuner.
    #[test]
    fn a_pattern_may_name_the_model_instead_of_the_tuner() {
        let card = |id: &str, model: &str| super::Tuner {
            id: id.to_owned(),
            model: model.to_owned(),
            deliveries: Vec::new(),
            leased: false,
            name: String::new(),
        };
        let advanced = card("example:1111222233334444:0", "Example 6100 Tuner 0");
        let older = card("example:aaaabbbbccccdddd:0", "Example 6200se Tuner 2");
        assert!(advanced.matches("model:Example 6100*"));
        assert!(!older.matches("model:Example 6100*"));
        // The names are read out of a driver, so the case is not a viewer's
        // to get right.
        assert!(advanced.matches("model:example 6100 tuner 0"));
        assert!(!advanced.matches("model:example 6100 tuner 1"));
        // And an identifier is still an identifier.
        assert!(advanced.matches("example:1111222233334444:*"));
        assert!(!advanced.matches("model:"));
    }

    /// A tuner's name follows the model only where it adds something: a
    /// board that spells its chips out in the model does not name one twice.
    #[test]
    fn a_label_names_the_tuner_only_where_the_model_does_not() {
        let tuner = |model: &str, name: &str| super::Tuner {
            id: String::new(),
            model: model.to_owned(),
            deliveries: Vec::new(),
            leased: false,
            name: name.to_owned(),
        };
        assert_eq!(
            tuner("Example U1 (BR100 + DM200)", "DM200").label(),
            "Example U1 (BR100 + DM200)"
        );
        assert_eq!(
            tuner("Example Q4", "satellite tuner 1").label(),
            "Example Q4 satellite tuner 1"
        );
        assert_eq!(
            tuner("Example 6200se Tuner 2", "").label(),
            "Example 6200se Tuner 2"
        );
        assert_eq!(tuner("", "DM200").label(), "DM200");
    }

    /// A refusal that names the tuner makes the lease name it from then on:
    /// the connection still holds it, and an unnamed request would be told
    /// that the only tuner is busy.
    #[test]
    fn a_refusal_that_names_the_tuner_is_remembered() {
        let mut lease = Daemon::new(Endpoint::default_endpoint()).hold(None);
        let said = crate::json::parse(
            r#"{"error":{"code":"tune_failed"},"status":{"tuner":"example:p1-13.3:0","locked":false}}"#,
        )
        .unwrap();
        lease.learn_refused(&said);
        assert_eq!(lease.tuner(), Some("example:p1-13.3:0"));
        let silent = crate::json::parse(r#"{"error":{"code":"tuner_busy"}}"#).unwrap();
        lease.learn_refused(&silent);
        assert_eq!(
            lease.tuner(),
            Some("example:p1-13.3:0"),
            "指定しなければそのまま"
        );
    }

    /// A tuner cannot always be asked what it receives, so the routes say
    /// instead. What a route says is added to what the tuner said, for the
    /// tuners the route names and no others.
    #[test]
    fn what_the_routes_say_a_tuner_receives_is_added_to_what_it_said() {
        let mut tuners = vec![
            super::Tuner {
                id: "bda:card0:0".to_owned(),
                model: "Example 6100 Tuner 0".to_owned(),
                // What a Windows driver registered, which is less than the
                // card does.
                deliveries: vec!["ISDB-T".to_owned()],
                leased: false,
                name: String::new(),
            },
            super::Tuner {
                id: "sample:6-17-0-0:0".to_owned(),
                model: "Sample Q2 satellite tuner 1".to_owned(),
                deliveries: vec!["ISDB-S".to_owned()],
                leased: false,
                name: String::new(),
            },
        ];
        let said = vec![
            ("ISDBS3".to_owned(), vec!["bda:card0:*".to_owned()]),
            ("ISDBS".to_owned(), vec!["model:Example 6100*".to_owned()]),
            // Said twice is said once.
            ("ISDB-T".to_owned(), vec!["bda:card0:*".to_owned()]),
        ];
        super::declare(&said, &mut tuners);
        assert!(tuners[0].receives("ISDBS3"), "the routes said it does");
        assert!(tuners[0].receives("ISDBS"), "a route may name the model");
        assert!(
            tuners[0].receives("ISDBT"),
            "and what the driver said stands"
        );
        assert_eq!(tuners[0].deliveries.len(), 3);
        assert!(
            !tuners[1].receives("ISDBS3"),
            "a tuner no route named is left as it was"
        );
    }

    use std::io;

    use super::{Daemon, Endpoint, Tuner};
    use crate::launch::Launcher;

    fn tuner(id: &str, deliveries: &[&str]) -> Tuner {
        Tuner {
            id: id.to_owned(),
            model: String::new(),
            deliveries: deliveries.iter().map(|one| (*one).to_owned()).collect(),
            leased: false,
            name: String::new(),
        }
    }

    /// A pattern ending in `*` is how a card whose identifier carries a USB
    /// path is named without writing the path down.
    #[test]
    fn a_pattern_names_the_tuners_it_starts_with() {
        let one = tuner("example:aaaabbbbccccdddd:0", &["ISDBT"]);
        assert!(one.matches("example:*"));
        assert!(one.matches("example:aaaabbbbccccdddd:0"));
        assert!(!one.matches("example:aaaabbbbccccdddd:1"));
        assert!(!one.matches("sample:*"));
    }

    /// A tuner that lists no delivery systems is tried rather than ruled out:
    /// the list is what a backend knows before the card is opened, and it is
    /// allowed not to know.
    #[test]
    fn a_tuner_that_says_nothing_is_still_worth_asking() {
        assert!(tuner("x:0", &[]).receives("ISDBT"));
        assert!(tuner("x:0", &["ISDBT"]).receives("isdbt"));
        assert!(!tuner("x:0", &["ISDBS"]).receives("ISDBT"));
        // The daemon writes the hyphens a channel file leaves out.
        assert!(tuner("x:0", &["ISDB-T"]).receives("ISDBT"));
        assert!(tuner("x:0", &["DVB-C/ANNEX_A"]).receives("DVBC/ANNEX_A"));
        assert!(!tuner("x:0", &["DVB-C/ANNEX_B"]).receives("DVBC/ANNEX_A"));
    }

    #[test]
    fn an_address_and_a_path_are_told_apart() {
        assert_eq!(
            Endpoint::parse("127.0.0.1:40200"),
            Endpoint::Network("127.0.0.1:40200".to_owned())
        );
        assert_eq!(
            Endpoint::parse(r"C:\Users\me\AppData\Local\coaxd\http.sock"),
            Endpoint::Socket(r"C:\Users\me\AppData\Local\coaxd\http.sock".to_owned())
        );
        // A drive letter is a colon with no number after it, so it stays a path.
        assert_eq!(
            Endpoint::parse("C:/coaxd.sock"),
            Endpoint::Socket("C:/coaxd.sock".to_owned())
        );
    }

    #[test]
    fn saying_nothing_means_the_socket_the_daemon_opens_by_itself() {
        assert!(matches!(Endpoint::parse("   "), Endpoint::Socket(_)));
    }

    /// An IPv6 address carries its port in brackets. Without them there is no
    /// port to read, and the last colon of `::1` must not be taken for one:
    /// `TcpStream` reads that as port 1 on the unspecified address.
    #[test]
    fn an_ipv6_address_needs_its_brackets_to_be_an_address() {
        assert_eq!(
            Endpoint::parse("[::1]:40200"),
            Endpoint::Network("[::1]:40200".to_owned())
        );
        assert_eq!(
            Endpoint::parse("[fd00::10]:40200"),
            Endpoint::Network("[fd00::10]:40200".to_owned())
        );
        assert!(matches!(Endpoint::parse("::1"), Endpoint::Socket(_)));
        assert!(matches!(Endpoint::parse("fd00::10"), Endpoint::Socket(_)));
    }

    /// The environment names the daemon for every program on the machine, and
    /// `COAX_HOST` may name an address -- which is the whole reason it is read
    /// before the socket variable rather than after.
    #[test]
    fn the_environment_names_the_daemon_when_nothing_else_does() {
        let from = |values: &[(&str, &str)]| {
            let owned: Vec<(String, String)> = values
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect();
            Endpoint::default_from(move |wanted| {
                owned
                    .iter()
                    .find(|(name, _)| name == wanted)
                    .map(|(_, value)| value.clone())
            })
        };

        assert_eq!(
            from(&[("COAX_HOST", "192.168.1.10:40772")]),
            Endpoint::Network("192.168.1.10:40772".to_owned())
        );
        assert_eq!(
            from(&[("COAX_HOST", "[fd00::10]:40772")]),
            Endpoint::Network("[fd00::10]:40772".to_owned())
        );
        // It may name a socket just as well, which is what makes it the one
        // variable a machine has to set.
        assert_eq!(
            from(&[("COAX_HOST", "/run/coaxd/http.sock")]),
            Endpoint::Socket("/run/coaxd/http.sock".to_owned())
        );
        // COAXD_SOCKET is a path by definition, so it stays one even where it
        // reads like an address.
        assert_eq!(
            from(&[("COAXD_SOCKET", "/run/coaxd:40772")]),
            Endpoint::Socket("/run/coaxd:40772".to_owned())
        );
        // COAX_HOST first, as `coax` reads them.
        assert_eq!(
            from(&[
                ("COAX_HOST", "192.168.1.10:40772"),
                ("COAXD_SOCKET", "/run/coaxd/http.sock"),
            ]),
            Endpoint::Network("192.168.1.10:40772".to_owned())
        );
        // Neither set is the socket the daemon opens by itself.
        assert!(matches!(from(&[]), Endpoint::Socket(_)));
    }

    /// A daemon named by an address is somebody else's to run: a local one
    /// started on hearing nothing from it listens on this machine's socket and
    /// can never answer. The header spelling is worth pinning: the daemon reads
    /// no other.
    #[test]
    fn the_priority_header_is_the_one_the_daemon_reads() {
        assert_eq!(super::PRIORITY_HEADER, "X-Coax-Priority");
        assert_eq!(super::DEFAULT_PRIORITY, 0);
    }

    /// Nothing is sent for the default, so a daemon too old to know the header
    /// never sees one -- and a driver that was never configured asks exactly
    /// as it always asked.
    #[test]
    fn only_a_priority_worth_saying_is_sent() {
        assert_eq!(super::priority_header(0), None);
        assert_eq!(super::priority_header(5).as_deref(), Some("5"));
        assert_eq!(super::priority_header(-2).as_deref(), Some("-2"));
    }

    /// A lease is opened at the daemon's priority and keeps it across
    /// channels, which is what stops a client losing standing between one
    /// channel and the next.
    #[test]
    fn a_lease_is_held_at_the_priority_the_daemon_was_given() {
        let daemon = Daemon::new(Endpoint::Socket("/nowhere".to_owned()));
        assert_eq!(daemon.priority(), super::DEFAULT_PRIORITY);
        assert_eq!(daemon.hold(None).priority(), super::DEFAULT_PRIORITY);
        let important = Daemon::new(Endpoint::Socket("/nowhere".to_owned())).with_priority(9);
        assert_eq!(important.priority(), 9);
        assert_eq!(important.hold(Some("example:ABC@1-2:0")).priority(), 9);
    }

    #[test]
    fn a_daemon_on_another_machine_is_not_started_here() {
        // Port 1 on the loopback: privileged, so nothing this test can reach
        // is listening there, and the connection is refused rather than hung.
        let daemon = Daemon::new(Endpoint::Network("127.0.0.1:1".to_owned()));
        let refused = daemon
            .ensure_running(&Launcher::default())
            .expect_err("a daemon on another machine is not started here");
        assert_eq!(refused.kind(), io::ErrorKind::ConnectionRefused);
        assert!(
            refused.to_string().contains("127.0.0.1:1"),
            "the refusal names the address: {refused}"
        );
    }
}

// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The daemon's card slots, over the same HTTP everything else uses.
//!
//! The daemon serves the card the way it serves tuners: a card is held by the
//! connection it was opened on and given up when that closes, so there is no
//! session to create, nothing to expire, and nothing left behind by a
//! television that was closed with the card still open. That rule is why an
//! open card here owns a connection of its own.

use std::io::{self, BufReader};

use crate::daemon::{Connection, Endpoint};
use crate::http;
use crate::json;

/// Why a request did not give an answer.
///
/// The two are told apart because they mean different things to a caller
/// standing in for a resource manager. A refusal is about the card or the
/// request and leaves the connection good for the next call; a broken
/// connection has taken every card on it.
#[derive(Debug)]
pub enum Refusal {
    /// Nothing answered, or the answer could not be read.
    Broken(io::Error),
    /// The daemon answered, and said no.
    Refused {
        status: u16,
        /// The daemon's own code, such as `unknown_tuner` or `card_error`.
        code: String,
        message: String,
    },
}

impl Refusal {
    /// The daemon's code, or nothing when there was no answer to carry one.
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Refused { code, .. } => Some(code),
            Self::Broken(_) => None,
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Broken(reason) => write!(out, "{reason}"),
            Self::Refused {
                status,
                code,
                message,
            } => write!(out, "the daemon answered {status} ({code}): {message}"),
        }
    }
}

impl From<io::Error> for Refusal {
    fn from(reason: io::Error) -> Self {
        Self::Broken(reason)
    }
}

/// What a card request gives back.
pub type Answered<T> = Result<T, Refusal>;

/// A slot, as the daemon lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub reader: String,
    pub backend: String,
    pub board: String,
}

/// What a slot looks like from outside.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotState {
    /// How many insertions and removals the slot has seen. A caller passes the
    /// last count it knows and is answered when this leaves it.
    pub events: u32,
    /// Whether the count moved while the request waited, as against the wait
    /// running out.
    pub changed: bool,
    pub present: bool,
    pub atr: Vec<u8>,
    /// How many connections hold the card, or -1 for an exclusive one.
    pub sharing: i32,
}

/// The card side of a daemon.
#[derive(Debug, Clone)]
pub struct Cards {
    endpoint: Endpoint,
}

impl Cards {
    pub fn new(endpoint: Endpoint) -> Self {
        Self { endpoint }
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// The slots this daemon has. Nothing is opened for it, so it costs a
    /// board nothing to be asked.
    pub fn list(&self) -> Answered<Vec<Slot>> {
        let answered = self.once("GET", "/cards", None)?;
        Ok(answered
            .items()
            .iter()
            .map(|one| Slot {
                reader: one.text("reader").unwrap_or_default().to_owned(),
                backend: one.text("backend").unwrap_or_default().to_owned(),
                board: one.text("board").unwrap_or_default().to_owned(),
            })
            .collect())
    }

    /// Waits for a slot to change, or reads it as it stands when `wait` is
    /// zero. The waiting happens at the daemon, which is next to the hardware
    /// and where the polling belongs: a caller that looked on a timer of its
    /// own would put a request on the wire for every tick of a card that has
    /// not moved in hours.
    pub fn events(&self, reader: &str, since: u32, wait_seconds: u32) -> Answered<SlotState> {
        let path = format!(
            "/cards/{}/events{}",
            escape(reader),
            http::query(&[
                ("since", since.to_string()),
                ("wait", wait_seconds.to_string()),
            ])
        );
        let answered = self.once("GET", &path, None)?;
        Ok(SlotState {
            events: answered
                .get("events")
                .and_then(json::Value::as_u64)
                .unwrap_or(0) as u32,
            changed: answered
                .get("changed")
                .and_then(json::Value::as_bool)
                .unwrap_or(false),
            present: answered
                .get("present")
                .and_then(json::Value::as_bool)
                .unwrap_or(false),
            atr: parse_hex(answered.text("atr").unwrap_or_default()),
            sharing: answered
                .get("sharing")
                .and_then(json::Value::as_u64)
                .map_or(-1, |value| value as i32),
        })
    }

    /// Opens a card, on a connection of its own because that is what holds it.
    pub fn open(&self, reader: &str, wait_seconds: u32) -> Answered<Held> {
        let connection = Connection::to(&self.endpoint)?;
        connection.set_read_timeout(std::time::Duration::from_secs(u64::from(wait_seconds)))?;
        let mut held = Held {
            endpoint: self.endpoint.clone(),
            reader: reader.to_owned(),
            link: Some(BufReader::new(connection)),
            atr: Vec::new(),
            max_input: 0,
            transaction: false,
        };
        let opened = held.call("POST", "/connect", Some("{}"))?;
        held.atr = parse_hex(opened.text("atr").unwrap_or_default());
        held.max_input = opened
            .get("max_input")
            .and_then(json::Value::as_u64)
            .unwrap_or(0) as usize;
        Ok(held)
    }

    /// One request on a connection of its own, for the questions that hold
    /// nothing.
    fn once(&self, method: &str, path: &str, body: Option<&str>) -> Answered<json::Value> {
        let connection = Connection::to(&self.endpoint)?;
        connection.set_read_timeout(ASKING)?;
        let mut answer = http::send(
            BufReader::new(connection),
            method,
            path,
            self.endpoint.host_header(),
            body,
        )?;
        read_answer(&mut answer)
    }
}

/// How long a question that is not a wait may sit before it is given up on.
const ASKING: std::time::Duration = std::time::Duration::from_secs(30);

/// One open card, with the connection that owns it.
#[derive(Debug)]
pub struct Held {
    endpoint: Endpoint,
    reader: String,
    /// The connection, taken out while a request is in flight and put back
    /// when its answer has been read to the end. Nothing when the card has
    /// been given up or the connection broke.
    link: Option<BufReader<Connection>>,
    atr: Vec<u8>,
    max_input: usize,
    transaction: bool,
}

impl Held {
    pub fn atr(&self) -> &[u8] {
        &self.atr
    }

    /// The longest command this reader carries, or zero where it did not say.
    pub fn max_input(&self) -> usize {
        self.max_input
    }

    pub fn reader(&self) -> &str {
        &self.reader
    }

    /// Whether the card is still held, which it is not once the connection has
    /// broken under it.
    pub fn open(&self) -> bool {
        self.link.is_some()
    }

    /// Sends one command APDU and returns what the card said, status bytes
    /// included.
    pub fn transmit(&mut self, command: &[u8]) -> Answered<Vec<u8>> {
        let body = format!("{{\"apdu\":\"{}\"}}", format_hex(command));
        let answered = self.call("POST", "/transmit", Some(&body))?;
        Ok(parse_hex(answered.text("response").unwrap_or_default()))
    }

    pub fn begin_transaction(&mut self) -> Answered<()> {
        self.call("POST", "/transaction", Some("{}"))?;
        self.transaction = true;
        Ok(())
    }

    pub fn end_transaction(&mut self, disposition: &str) -> Answered<()> {
        // Recorded as ended before the request, because the claim has to be
        // given up on this side even when the disposition fails: left set, the
        // release below would try to end a transaction the daemon no longer
        // has.
        self.transaction = false;
        self.call(
            "DELETE",
            &format!("/transaction?disposition={disposition}"),
            None,
        )?;
        Ok(())
    }

    /// Gives the card back and drops the connection holding it. The connection
    /// goes either way: it exists to hold this card, and the daemon releases
    /// what a closed connection held, so a failed request here is not a card
    /// left open.
    pub fn close(&mut self, disposition: &str) -> Answered<()> {
        if self.transaction {
            let _ = self.end_transaction("leave");
        }
        let result = self.call(
            "DELETE",
            &format!("/connect?disposition={disposition}"),
            None,
        );
        self.link = None;
        result.map(|_| ())
    }

    /// One request on the connection that owns this card.
    fn call(&mut self, method: &str, path: &str, body: Option<&str>) -> Answered<json::Value> {
        let link = self.link.take().ok_or_else(|| {
            Refusal::Broken(io::Error::new(
                io::ErrorKind::NotConnected,
                "the card is no longer held",
            ))
        })?;
        let target = format!("/cards/{}{}", escape(&self.reader), path);
        let host = self.endpoint.host_header().to_owned();
        // A broken connection has taken the card with it. Not putting the
        // reader back is what makes the next call say so, rather than write
        // into a socket nobody is reading.
        let mut answer = http::send(link, method, &target, &host, body)?;
        let parsed = read_answer(&mut answer);
        // Put back only when the answer left the connection able to carry
        // another request; anything else and the card is gone with it.
        if answer.reusable() && answer.finished() {
            self.link = Some(answer.into_reader());
        }
        parsed
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        if self.link.is_some() {
            // The daemon would release it anyway when the connection goes.
            // Saying so first is what leaves the card powered rather than at
            // the mercy of whatever a dropped connection is taken to mean.
            let _ = self.close("leave");
        }
    }
}

/// Reads an answer to the end and turns a failure into the error a caller can
/// act on, keeping the daemon's own code where it gave one.
fn read_answer<T: std::io::Read>(answer: &mut http::Response<T>) -> Answered<json::Value> {
    let body = answer.read_to_end()?;
    let text = String::from_utf8_lossy(&body);
    if answer.status >= 400 {
        let problem = json::parse(&text).ok().and_then(|parsed| {
            let problem = parsed.get("error")?;
            Some((
                problem.text("code").unwrap_or_default().to_owned(),
                problem.text("message").unwrap_or_default().to_owned(),
            ))
        });
        let (code, message) = problem.unwrap_or_default();
        return Err(Refusal::Refused {
            status: answer.status,
            code,
            message,
        });
    }
    json::parse(&text).map_err(|reason| Refusal::Broken(io::Error::other(reason)))
}

/// Escapes a reader name for a path segment. The names carry spaces and, on
/// some backends, characters a URL cannot hold as they stand.
fn escape(name: &str) -> String {
    let mut escaped = String::with_capacity(name.len());
    for byte in name.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                escaped.push(byte as char);
            }
            _ => escaped.push_str(&format!("%{byte:02X}")),
        }
    }
    escaped
}

/// Writes bytes the way the daemon reads them.
pub fn format_hex(data: &[u8]) -> String {
    let mut written = String::with_capacity(data.len() * 3);
    for byte in data {
        if !written.is_empty() {
            written.push(' ');
        }
        written.push_str(&format!("{byte:02X}"));
    }
    written
}

/// Reads the hexadecimal the daemon writes bytes as, ignoring its separators.
pub fn parse_hex(written: &str) -> Vec<u8> {
    let digits: Vec<u8> = written
        .bytes()
        .filter(|byte| byte.is_ascii_hexdigit())
        .collect();
    digits
        .as_chunks::<2>()
        .0
        .iter()
        .filter_map(|pair| {
            let text = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(text, 16).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{format_hex, parse_hex};

    #[test]
    fn bytes_survive_the_crossing() {
        assert_eq!(format_hex(&[0x90, 0x30, 0x00]), "90 30 00");
        assert_eq!(parse_hex("90 30 00"), vec![0x90, 0x30, 0x00]);
        assert_eq!(parse_hex("903000"), vec![0x90, 0x30, 0x00]);
        assert_eq!(parse_hex(""), Vec::<u8>::new());
    }

    /// A name with a space in it is what these readers are actually called.
    #[test]
    fn a_reader_name_is_escaped_for_the_path() {
        assert_eq!(
            super::escape("PLEX Smart Card Reader"),
            "PLEX%20Smart%20Card%20Reader"
        );
    }
}

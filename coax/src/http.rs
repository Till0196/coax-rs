// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! As much of HTTP/1.1 as talking to the daemon needs.
//!
//! Written out rather than taken from a crate. What is used here is three
//! verbs, one version and two ways of ending a body, and a driver a host loads
//! into its own process is a bad place for a dependency tree.

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::OnceLock;

/// The name every request carries as its `User-Agent`, when the program has not
/// given its own.
const LIBRARY: &str = concat!("coax-rs/", env!("CARGO_PKG_VERSION"));

static AGENT: OnceLock<String> = OnceLock::new();

/// Names the program in the `User-Agent` of every request from here on, so that
/// a server's log tells one client from another. A program calls it once,
/// before its first request; one that never does goes by `coax-rs`. Only the
/// first call counts, and a name that would break the header line is not
/// taken.
pub fn name_agent(name: &str) {
    if !name.is_empty() && !name.contains(['\r', '\n']) {
        let _ = AGENT.set(name.to_owned());
    }
}

fn agent() -> &'static str {
    AGENT.get().map_or(LIBRARY, String::as_str)
}

/// What the daemon answered.
#[derive(Debug)]
pub struct Response<T: Read> {
    pub status: u16,
    pub reason: String,
    pub headers: BTreeMap<String, String>,
    body: Body<T>,
    /// Whether the connection may carry another request once this body has
    /// been read to its end: asked for, and not refused by the answer.
    keeping: bool,
}

#[derive(Debug)]
enum Body<T: Read> {
    /// A body whose length was given, or which runs to the end of the
    /// connection.
    Plain {
        reader: BufReader<T>,
        remaining: Option<u64>,
    },
    /// A body sent in chunks, which is how the daemon streams: it cannot know
    /// how long a recording will be.
    Chunked {
        reader: BufReader<T>,
        left_in_chunk: u64,
        ended: bool,
    },
}

/// A header to send beside the ones every request carries: a field name and its
/// value, written out in the order given. Borrowed rather than owned because
/// every one of them is a constant name and a number a caller has just
/// rendered.
pub type Header<'a> = (&'a str, &'a str);

/// Sends a `GET` and reads the head of the answer, leaving the body to be read
/// as it arrives. The connection carries this one request and is then done
/// with.
pub fn get<T: Read + Write>(
    connection: T,
    path: &str,
    host: &str,
    headers: &[Header<'_>],
) -> io::Result<Response<T>> {
    send_on(
        BufReader::new(connection),
        "GET",
        path,
        host,
        None,
        headers,
        false,
    )
}

/// One request on a connection that is to carry more, with a body when the verb
/// takes one. The card endpoints need this: a card belongs to the connection it
/// was opened on, so every command about it has to travel down that same one.
pub fn send<T: Read + Write>(
    reader: BufReader<T>,
    method: &str,
    path: &str,
    host: &str,
    body: Option<&str>,
) -> io::Result<Response<T>> {
    send_on(reader, method, path, host, body, &[], true)
}

/// The same on a connection that is to carry another request afterwards.
///
/// The daemon ties a tuner to the connection that asked for it, so a caller
/// that wants to keep a tuner between one channel and the next has to keep the
/// connection. The reader is passed in and handed back rather than the
/// connection itself, because what a body did not need may already be in the
/// buffer and belongs to the next answer.
pub fn kept<T: Read + Write>(
    reader: BufReader<T>,
    path: &str,
    host: &str,
    headers: &[Header<'_>],
) -> io::Result<Response<T>> {
    send_on(reader, "GET", path, host, None, headers, true)
}

fn send_on<T: Read + Write>(
    mut reader: BufReader<T>,
    method: &str,
    path: &str,
    host: &str,
    body: Option<&str>,
    headers: &[Header<'_>],
    keep: bool,
) -> io::Result<Response<T>> {
    let connection = reader.get_mut();
    let ending = if keep { "keep-alive" } else { "close" };
    let agent = agent();
    write!(
        connection,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {agent}\r\nAccept: */*\r\nConnection: {ending}\r\n"
    )?;
    for (name, value) in headers {
        // Nothing here escapes: a field value cannot carry a line ending, and
        // every value this crate sends is a number it rendered itself. A
        // caller passing one through from elsewhere would have to check it.
        write!(connection, "{name}: {value}\r\n")?;
    }
    match body {
        // A length is always written, even for the empty body a POST with
        // nothing to say has: without one the daemon waits for a body that is
        // never coming, and the request never gets an answer.
        Some(text) => write!(
            connection,
            "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{text}",
            text.len()
        )?,
        None => write!(connection, "\r\n")?,
    }
    connection.flush()?;

    let (status, reason) = read_status(&mut reader)?;
    let headers = read_headers(&mut reader)?;
    let keeping = keep
        && !headers
            .get("connection")
            .is_some_and(|value| value.to_ascii_lowercase().contains("close"));

    let chunked = headers
        .get("transfer-encoding")
        .is_some_and(|value| value.contains("chunked"));
    let length = headers
        .get("content-length")
        .and_then(|value| value.trim().parse::<u64>().ok());
    let body = if chunked {
        Body::Chunked {
            reader,
            left_in_chunk: 0,
            ended: false,
        }
    } else {
        Body::Plain {
            reader,
            remaining: length,
        }
    };
    Ok(Response {
        status,
        reason,
        headers,
        body,
        keeping,
    })
}

fn read_status<T: Read>(reader: &mut BufReader<T>) -> io::Result<(u16, String)> {
    let line = read_line(reader)?;
    let mut parts = line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("not an HTTP answer: {line}"),
        ));
    }
    let status = parts
        .next()
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, format!("no status in: {line}"))
        })?;
    Ok((status, parts.next().unwrap_or_default().to_owned()))
}

/// Reads the headers, keyed in lower case: the field names are not
/// case-sensitive and this side should not have to remember which spelling the
/// other side chose.
fn read_headers<T: Read>(reader: &mut BufReader<T>) -> io::Result<BTreeMap<String, String>> {
    let mut headers = BTreeMap::new();
    for _ in 0..=MOST_HEADERS {
        let line = read_line(reader)?;
        if line.is_empty() {
            return Ok(headers);
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    Err(too_much("more header lines than an answer has"))
}

/// What an answer may hold before it is taken for something that is not one.
/// The daemon's lines are short, its headers few and its bodies -- the ones
/// read whole -- a listing or a channel table. A host named by the settings may
/// be anywhere, and one that sends without end must not be read without end.
const LONGEST_LINE: usize = 16 * 1024;
const MOST_HEADERS: usize = 128;
const LONGEST_BODY: usize = 32 * 1024 * 1024;

fn too_much(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_owned())
}

fn read_line<T: Read>(reader: &mut BufReader<T>) -> io::Result<String> {
    let mut line = Vec::new();
    let read = read_until_crlf(reader, &mut line)?;
    if read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the answer ended mid-header",
        ));
    }
    Ok(String::from_utf8_lossy(&line).trim_end().to_owned())
}

fn read_until_crlf<T: Read>(reader: &mut BufReader<T>, into: &mut Vec<u8>) -> io::Result<usize> {
    let read = Read::take(&mut *reader, LONGEST_LINE as u64 + 1).read_until(b'\n', into)?;
    if read > LONGEST_LINE {
        return Err(too_much("a header line longer than an answer has"));
    }
    Ok(read)
}

impl<T: Read> Response<T> {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }

    /// The connection the answer is being read from, for a caller that has to
    /// set something on it once the headers are past. The connection the answer
    /// is being read from, for a caller that has to set something on it once
    /// the headers are past.
    pub fn get_ref(&self) -> &T {
        match &self.body {
            Body::Plain { reader, .. } | Body::Chunked { reader, .. } => reader.get_ref(),
        }
    }

    /// Whether this answer's body ends in a way that leaves the connection
    /// able to carry another request.
    pub fn reusable(&self) -> bool {
        self.keeping
            && match &self.body {
                Body::Plain { remaining, .. } => remaining.is_some(),
                Body::Chunked { .. } => true,
            }
    }

    /// Whether the body has been read to its end.
    pub fn finished(&self) -> bool {
        match &self.body {
            Body::Plain { remaining, .. } => *remaining == Some(0),
            Body::Chunked { ended, .. } => *ended,
        }
    }

    /// The reader this answer was read from, for the next request on the same
    /// connection. Only worth taking once the body is finished: anything left
    /// of it would be read as the head of the next answer.
    pub fn into_reader(self) -> BufReader<T> {
        match self.body {
            Body::Plain { reader, .. } | Body::Chunked { reader, .. } => reader,
        }
    }

    /// Reads what has arrived of the body, or nothing at its end. Short reads
    /// are the normal case: the daemon writes a chunk whenever the card has
    /// filled one, and a reader that waited for a full buffer would hold the
    /// picture back.
    pub fn read_body(&mut self, into: &mut [u8]) -> io::Result<usize> {
        match &mut self.body {
            Body::Plain { reader, remaining } => {
                let wanted = match remaining {
                    Some(0) => return Ok(0),
                    Some(left) => into.len().min(usize::try_from(*left).unwrap_or(usize::MAX)),
                    None => into.len(),
                };
                let read = reader.read(&mut into[..wanted])?;
                if let Some(left) = remaining {
                    *left -= read as u64;
                }
                Ok(read)
            }
            Body::Chunked {
                reader,
                left_in_chunk,
                ended,
            } => {
                if *ended {
                    return Ok(0);
                }
                if *left_in_chunk == 0 {
                    let header = read_line(reader)?;
                    // The size is hexadecimal, and may carry extensions after
                    // a semicolon that nothing here needs.
                    let size = header.split(';').next().unwrap_or_default().trim();
                    let size = u64::from_str_radix(size, 16).map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("bad chunk size: {header}"),
                        )
                    })?;
                    if size == 0 {
                        // The trailers, if any, and the blank line that ends
                        // them. Left unread, they would be read as the head of
                        // whatever answer comes next down the connection.
                        while !read_line(reader)?.is_empty() {}
                        *ended = true;
                        return Ok(0);
                    }
                    *left_in_chunk = size;
                }
                let wanted = into
                    .len()
                    .min(usize::try_from(*left_in_chunk).unwrap_or(usize::MAX));
                let read = reader.read(&mut into[..wanted])?;
                *left_in_chunk -= read as u64;
                if *left_in_chunk == 0 {
                    // The two bytes that close a chunk.
                    let _ = read_line(reader)?;
                }
                Ok(read)
            }
        }
    }

    /// Reads the whole body, for the answers that are short by nature.
    pub fn read_to_end(&mut self) -> io::Result<Vec<u8>> {
        let mut all = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            match self.read_body(&mut buffer)? {
                0 => return Ok(all),
                read => all.extend_from_slice(&buffer[..read]),
            }
            if all.len() > LONGEST_BODY {
                return Err(too_much("an answer longer than any the daemon gives"));
            }
        }
    }
}

/// Renders a query string, escaping what a URL cannot carry as it stands.
pub fn query(parameters: &[(&str, String)]) -> String {
    let mut rendered = String::new();
    for (name, value) in parameters {
        rendered.push(if rendered.is_empty() { '?' } else { '&' });
        rendered.push_str(name);
        rendered.push('=');
        for byte in value.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    rendered.push(byte as char);
                }
                _ => rendered.push_str(&format!("%{byte:02X}")),
            }
        }
    }
    rendered
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{get, query};

    /// A connection that answers from a script and remembers what was asked.
    #[derive(Debug)]
    struct Scripted {
        answer: Cursor<Vec<u8>>,
        asked: Vec<u8>,
    }

    impl std::io::Read for Scripted {
        fn read(&mut self, into: &mut [u8]) -> std::io::Result<usize> {
            self.answer.read(into)
        }
    }

    impl std::io::Write for Scripted {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.asked.extend_from_slice(data);
            Ok(data.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn answering(text: &str) -> Scripted {
        Scripted {
            answer: Cursor::new(text.as_bytes().to_vec()),
            asked: Vec::new(),
        }
    }

    #[test]
    fn a_body_sent_in_chunks_is_read_as_one_stream() {
        let mut response = get(
            answering(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nabcd\r\n2\r\nef\r\n0\r\n\r\n",
            ),
            "/stream",
            "coaxd",
            &[],
        )
        .expect("the answer parses");
        assert_eq!(response.status, 200);
        assert_eq!(response.read_to_end().expect("the body reads"), b"abcdef");
    }

    #[test]
    fn a_body_with_a_length_stops_where_the_length_says() {
        let mut response = get(
            answering("HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabcdefgh"),
            "/tune",
            "coaxd",
            &[],
        )
        .expect("the answer parses");
        assert_eq!(response.read_to_end().expect("the body reads"), b"abc");
    }

    #[test]
    fn the_headers_are_found_whatever_case_they_were_sent_in() {
        let response = get(
            answering("HTTP/1.1 409 Conflict\r\nCONTENT-TYPE: application/json\r\n\r\n"),
            "/tune",
            "coaxd",
            &[],
        )
        .expect("the answer parses");
        assert_eq!(response.status, 409);
        assert_eq!(response.reason, "Conflict");
        assert_eq!(response.header("content-type"), Some("application/json"));
    }

    #[test]
    fn a_request_carries_the_headers_it_was_given() {
        let mut connection = answering("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        let response = super::kept(
            std::io::BufReader::new(&mut connection),
            "/tune",
            "coaxd",
            &[("X-Coax-Priority", "5")],
        )
        .expect("the answer parses");
        assert_eq!(response.status, 200);
        drop(response);
        let asked = String::from_utf8_lossy(&connection.asked).into_owned();
        assert!(
            asked.contains("\r\nX-Coax-Priority: 5\r\n"),
            "the header was not sent: {asked:?}"
        );
        // And it goes before the blank line that ends the head, or the daemon
        // would read it as the start of a body.
        let (head, _) = asked
            .split_once("\r\n\r\n")
            .expect("the head ends somewhere");
        assert!(head.contains("X-Coax-Priority: 5"));
    }

    #[test]
    fn a_request_with_no_headers_looks_exactly_as_it_always_did() {
        let mut connection = answering("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        let response = super::kept(
            std::io::BufReader::new(&mut connection),
            "/tune",
            "coaxd",
            &[],
        )
        .expect("the answer parses");
        drop(response);
        assert_eq!(
            String::from_utf8_lossy(&connection.asked),
            format!(
                "GET /tune HTTP/1.1\r\nHost: coaxd\r\nUser-Agent: coax-rs/{}\r\n\
                 Accept: */*\r\nConnection: keep-alive\r\n\r\n",
                env!("CARGO_PKG_VERSION")
            )
        );
    }

    #[test]
    fn a_query_escapes_what_a_url_cannot_carry() {
        let rendered = query(&[
            ("DELIVERY_SYSTEM", "DVBC/ANNEX_A".to_owned()),
            ("FREQUENCY", "473142857".to_owned()),
        ]);
        assert_eq!(
            rendered,
            "?DELIVERY_SYSTEM=DVBC%2FANNEX_A&FREQUENCY=473142857"
        );
    }
}

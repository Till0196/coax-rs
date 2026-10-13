// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! As much of JSON as reading the daemon's answers and a channel file needs.
//!
//! Written out for the same reason the HTTP is: this ends up inside a viewer's
//! television, and a driver a host loads into its own process is a bad place
//! for a dependency tree.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// A value as it was written.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// Numbers are kept as they were written. Nothing here does arithmetic on
    /// one, and a frequency that went out as an integer should come back as
    /// the same integer rather than through a float.
    Number(String),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn get(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Object(members) => members.get(name),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(yes) => Some(*yes),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Number(text) => text.parse().ok(),
            _ => None,
        }
    }

    /// The members of an array, or nothing at all when the value is not one.
    /// A caller walking an answer wants to skip what is not the shape it
    /// expected rather than stop.
    pub fn items(&self) -> &[Value] {
        self.as_array().unwrap_or(&[])
    }

    /// The text of a member, for the many places that want one string out of
    /// an object and nothing else.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.get(name)?.as_str()
    }
}

/// Reads one value, and refuses text with anything but whitespace after it.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut reader = Reader {
        bytes: text.as_bytes(),
        at: 0,
        depth: 0,
    };
    reader.skip_whitespace();
    let value = reader.value()?;
    reader.skip_whitespace();
    if reader.at < reader.bytes.len() {
        return Err(format!("trailing text at byte {}", reader.at));
    }
    Ok(value)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    /// How many arrays and objects the reader is inside.
    depth: usize,
}

/// How deep arrays and objects may nest. The daemon's answers go three or four
/// deep; a text that goes on opening brackets is not an answer, and following
/// it down would run out of stack.
const MOST_DEPTH: usize = 128;

impl Reader<'_> {
    fn skip_whitespace(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.at += 1;
            return Ok(());
        }
        Err(format!("expected {} at byte {}", byte as char, self.at))
    }

    fn value(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some(b'{' | b'[') => {
                if self.depth >= MOST_DEPTH {
                    return Err(format!(
                        "nested deeper than {MOST_DEPTH} at byte {}",
                        self.at
                    ));
                }
                self.depth += 1;
                let value = if self.peek() == Some(b'{') {
                    self.object()
                } else {
                    self.array()
                };
                self.depth -= 1;
                value
            }
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'n') => self.word("null", Value::Null),
            Some(_) => self.number(),
            None => Err("the text ended where a value was expected".to_owned()),
        }
    }

    fn word(&mut self, word: &str, value: Value) -> Result<Value, String> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            return Ok(value);
        }
        Err(format!("not {word} at byte {}", self.at))
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.at;
        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
        ) {
            self.at += 1;
        }
        if self.at == start {
            return Err(format!("not a value at byte {start}"));
        }
        Ok(Value::Number(
            String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned(),
        ))
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut text = String::new();
        loop {
            match self.peek() {
                None => return Err("the text ended inside a string".to_owned()),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(text);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escaped = self
                        .peek()
                        .ok_or_else(|| "the text ended inside an escape".to_owned())?;
                    self.at += 1;
                    match escaped {
                        b'"' => text.push('"'),
                        b'\\' => text.push('\\'),
                        b'/' => text.push('/'),
                        b'b' => text.push('\u{8}'),
                        b'f' => text.push('\u{c}'),
                        b'n' => text.push('\n'),
                        b'r' => text.push('\r'),
                        b't' => text.push('\t'),
                        b'u' => text.push(self.escaped_character()?),
                        other => return Err(format!("unknown escape \\{}", other as char)),
                    }
                }
                Some(_) => {
                    // Everything up to the next escape or the closing quote,
                    // in one go.
                    //
                    // **A character at a time is quadratic here.** What stood
                    // here read one character by turning *the whole rest of
                    // the text* into a `String` and taking its first
                    // character -- so a document of n bytes was copied once
                    // per character. Measured on a Mirakurun programme
                    // listing: 772 kB took longer than a hundred seconds, and
                    // the 29 MB of a week's listing never finished.
                    //
                    // Whole characters still come out whole: the run is
                    // checked as UTF-8 once, and a run that is not valid
                    // falls back to the lossy reading it had before. Station
                    // names are not ASCII, which is what that is for.
                    let start = self.at;
                    while let Some(byte) = self.peek() {
                        if byte == b'"' || byte == b'\\' {
                            break;
                        }
                        self.at += 1;
                    }
                    // **区切りは必ず文字の境界。**区切っているのは `"` と
                    // `\` で、どちらも ASCII ですから、マルチバイト文字の
                    // 途中には来ません。入力そのものも `&str` なので、
                    // ここは借用するだけでコピーしません。
                    text.push_str(&String::from_utf8_lossy(&self.bytes[start..self.at]));
                }
            }
        }
    }

    /// A `\uXXXX` escape, and the pair of them that carries a character
    /// outside the basic plane.
    fn escaped_character(&mut self) -> Result<char, String> {
        let first = self.four_hex_digits()?;
        if !(0xD800..0xDC00).contains(&first) {
            return char::from_u32(u32::from(first))
                .ok_or_else(|| format!("\\u{first:04X} is not a character"));
        }
        if !self.bytes[self.at..].starts_with(b"\\u") {
            return Err("a leading surrogate with nothing after it".to_owned());
        }
        self.at += 2;
        let second = self.four_hex_digits()?;
        if !(0xDC00..0xE000).contains(&second) {
            return Err("a leading surrogate followed by something else".to_owned());
        }
        let combined =
            0x1_0000 + ((u32::from(first) - 0xD800) << 10) + (u32::from(second) - 0xDC00);
        char::from_u32(combined).ok_or_else(|| "a surrogate pair that is no character".to_owned())
    }

    fn four_hex_digits(&mut self) -> Result<u16, String> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .ok_or_else(|| "the text ended inside an escape".to_owned())?;
        let digits = std::str::from_utf8(digits).map_err(|_| "a bad escape".to_owned())?;
        let value = u16::from_str_radix(digits, 16).map_err(|_| format!("bad escape {digits}"))?;
        self.at += 4;
        Ok(value)
    }

    fn array(&mut self) -> Result<Value, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(format!("expected , or ] at byte {}", self.at)),
            }
        }
    }

    fn object(&mut self) -> Result<Value, String> {
        self.expect(b'{')?;
        let mut members = BTreeMap::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.skip_whitespace();
            let name = self.string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            members.insert(name, self.value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Value::Object(members));
                }
                _ => return Err(format!("expected , or }} at byte {}", self.at)),
            }
        }
    }
}

/// Writes a string as JSON, escaping only what has to be escaped. Not the
/// letters above ASCII: a channel file is read by people, and a name written as
/// a row of `\u` escapes is a name nobody can check.
pub fn string(text: &str) -> String {
    let mut written = String::with_capacity(text.len() + 2);
    written.push('"');
    for letter in text.chars() {
        match letter {
            '"' => written.push_str("\\\""),
            '\\' => written.push_str("\\\\"),
            '\n' => written.push_str("\\n"),
            '\r' => written.push_str("\\r"),
            '\t' => written.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                let _ = write!(written, "\\u{:04x}", other as u32);
            }
            other => written.push(other),
        }
    }
    written.push('"');
    written
}

#[cfg(test)]
mod tests {
    use super::{parse, string};

    /// 長い文字列が、長さに比例した時間で読めること。
    ///
    /// 一文字ごとに全体をコピーすると長さの二乗の時間がかかる。長さを十倍にしても
    /// 十倍しか増えないことを、時間ではなく**読めること**で確かめる——時間で
    /// 測ると、遅いマシンで失敗するテストになる。
    #[test]
    fn a_long_string_is_read_in_time_that_grows_with_its_length() {
        let long = "あ".repeat(200_000);
        let text = format!("{{\"name\": {}}}", string(&long));
        let read = parse(&text).expect("読めない");
        assert_eq!(read.text("name"), Some(long.as_str()));
    }

    /// エスケープした文字と、そのままの文字が混じっていても読めること。
    #[test]
    fn escapes_and_plain_characters_come_back_together() {
        let read = parse(r#"{"a": "ＳＡＭＰＬＥ\"サンプル\"1\n地域"}"#).expect("読めない");
        assert_eq!(read.text("a"), Some("ＳＡＭＰＬＥ\"サンプル\"1\n地域"));
    }

    #[test]
    fn a_board_answer_is_read_down_to_the_tuners() {
        let answer = parse(
            r#"[{"name":"px4:ABC@1-2.1","backend":"px4","deliveries":["ISDBT","ISDBS"],
                 "tuners":[{"id":"px4:ABC@1-2.1:0","index":0,"deliveries":["ISDBS"],"leased":false},
                           {"id":"px4:ABC@1-2.1:2","index":2,"deliveries":["ISDBT"],"leased":true}]}]"#,
        )
        .expect("the answer parses");
        let board = &answer.items()[0];
        assert_eq!(board.text("backend"), Some("px4"));
        let tuners = board.get("tuners").expect("the board has tuners").items();
        assert_eq!(tuners.len(), 2);
        assert_eq!(tuners[1].text("id"), Some("px4:ABC@1-2.1:2"));
        assert_eq!(
            tuners[1].get("leased").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(tuners[0].get("index").and_then(|v| v.as_u64()), Some(0));
    }

    /// A name comes back as it was written, whichever way it was written.
    #[test]
    fn a_name_survives_the_round_trip_escaped_or_not() {
        let escaped = parse(r#"{"name":"\u5730\u57df\uff11"}"#).expect("it parses");
        let plain = parse(r#"{"name":"地域１"}"#).expect("it parses");
        assert_eq!(escaped.text("name"), Some("地域１"));
        assert_eq!(plain.text("name"), Some("地域１"));
        assert_eq!(string("地域１"), "\"地域１\"");
    }

    #[test]
    fn a_pair_of_escapes_makes_one_character_above_the_basic_plane() {
        let value = parse(r#""😀""#).expect("it parses");
        assert_eq!(value.as_str(), Some("😀"));
    }

    #[test]
    fn a_number_keeps_the_digits_it_was_written_with() {
        let value = parse(r#"{"frequency":473142857}"#).expect("it parses");
        assert_eq!(
            value.get("frequency").and_then(|v| v.as_u64()),
            Some(473_142_857)
        );
    }

    #[test]
    fn text_after_the_value_is_refused() {
        assert!(parse("{} {}").is_err());
        assert!(parse("[1,2").is_err());
    }

    #[test]
    fn what_is_not_the_expected_shape_reads_as_empty_rather_than_failing() {
        let value = parse(r#"{"tuners":"not a list"}"#).expect("it parses");
        assert!(value.get("tuners").expect("it is there").items().is_empty());
        assert_eq!(value.text("missing"), None);
    }

    /// Brackets opened without end are refused rather than followed down
    /// until the stack runs out.
    #[test]
    fn nesting_without_end_is_refused() {
        let deep = "[".repeat(100_000);
        assert!(super::parse(&deep).is_err());
        let fine = format!("{}{}", "[".repeat(10), "]".repeat(10));
        assert!(super::parse(&fine).is_ok());
    }

    /// A leading surrogate followed by anything but a trailing one is refused,
    /// not subtracted into nonsense.
    #[test]
    fn a_broken_surrogate_pair_is_refused() {
        assert!(super::parse(r#""\ud800\u0041""#).is_err());
        assert_eq!(
            super::parse(r#""\ud83d\ude00""#)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned)),
            Some("😀".to_owned())
        );
    }
}

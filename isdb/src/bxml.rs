// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Data broadcasting on a transport stream: how the BML documents and their
//! files ride the DSM-CC carousel. ARIB STD-B24 Volume 2 (the encoding) and
//! Volume 3 (the transmission), as operated by TR-B14 and TR-B15.
//!
//! The carousel itself is [`mpegts::dsmcc`]: it hands back modules, whole,
//! and says nothing about them. Everything here is what ARIB writes into
//! the bytes around a module: the descriptor in the programme map that says
//! a stream is a data broadcast and which document starts it
//! ([`additional_info`]), the descriptors of a module that say what it is
//! and whether it is compressed ([`describe`]), the multipart body that
//! packs several files into one module ([`files`]), and the events sent in
//! stream descriptors ([`events`]).

use crate::tstables::{Descriptor, be16, be32, descriptors, find};

/// The data component descriptor's tag in a programme map.
pub const DESCRIPTOR_DATA_COMPONENT: u8 = 0xfd;

/// The `data_component_id`s under which BML is broadcast: terrestrial in
/// two profiles, BS and CS. STD-B10 Part 2 Annex J Table J-1.
pub const fn is_bml(data_component_id: u16) -> bool {
    matches!(data_component_id, 0x0007 | 0x000b | 0x000c | 0x000d)
}

/// The `data_component_id` of the C profile: the one-segment service, which
/// starts only when asked.
pub const DATA_COMPONENT_C_PROFILE: u16 = 0x000d;

/// The data component descriptor: what coding the stream is, and the bytes
/// the coding adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataComponent {
    pub id: u16,
    pub additional: Vec<u8>,
}

/// Reads the data component descriptor out of a stream's descriptors.
pub fn data_component(descriptors: &[Descriptor]) -> Option<DataComponent> {
    let found = find(descriptors, DESCRIPTOR_DATA_COMPONENT)?;
    let body = &found.body;
    if body.len() < 2 {
        return None;
    }
    Some(DataComponent {
        id: be16(body),
        additional: body[2..].to_vec(),
    })
}

/// What the additional information of a BML stream says. STD-B24 Volume 2
/// Part 2 clause 9.3, `additional_arib_bxml_info`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdditionalInfo {
    /// 0: the carousel and event messages, which is all that is broadcast.
    pub transmission_format: u8,
    /// Whether this is the component the service starts from. **It is the
    /// one under component tag 0x40**, and its startup document is what a
    /// receiver opens first.
    pub entry_point: bool,
    /// Whether the entry point starts by itself, rather than waiting for the
    /// viewer to ask.
    pub auto_start: bool,
    /// The `document_resolution` field, in the standard's numbering; see
    /// [`AdditionalInfo::resolution`].
    pub document_resolution: u8,
    pub independent: bool,
    pub style_for_tv: bool,
    pub bml_version: (u16, u16),
    /// Of the carousel: which event of the stream the data belongs to, and
    /// whether the stream descriptors carry events.
    pub data_event_id: u8,
    pub event_section: bool,
    pub ondemand_retrieval: bool,
    pub file_storable: bool,
    pub start_priority: u8,
}

impl AdditionalInfo {
    /// The pixels a document of this stream is laid out for, and whether the
    /// picture is wide. `None` where the stream says nothing or leaves it
    /// to the document (the C profile).
    pub fn resolution(&self) -> Option<((u32, u32), bool)> {
        Some(match self.document_resolution {
            0b0001 => ((1920, 1080), true),
            0b0010 => ((1280, 720), true),
            0b0011 => ((960, 540), true),
            0b0100 => ((640, 480), true),
            0b0101 => ((640, 480), false),
            0b0110 => ((320, 240), false),
            _ => return None,
        })
    }
}

/// Reads the additional information of a BML stream.
pub fn additional_info(bytes: &[u8]) -> Option<AdditionalInfo> {
    let first = *bytes.first()?;
    let mut info = AdditionalInfo {
        transmission_format: first >> 6,
        entry_point: first & 0x20 != 0,
        bml_version: (1, 0),
        ..Default::default()
    };
    let mut at = 1;
    if info.entry_point {
        info.auto_start = first & 0x10 != 0;
        info.document_resolution = first & 0x0f;
        let second = *bytes.get(at)?;
        let use_xml = second & 0x80 != 0;
        let default_version = second & 0x40 != 0;
        info.independent = second & 0x20 != 0;
        info.style_for_tv = second & 0x10 != 0;
        at += 1;
        if !default_version {
            let words = bytes.get(at..at + 4)?;
            info.bml_version = (be16(words), be16(&words[2..]));
            at += 4;
            if use_xml {
                at += 4;
            }
        }
    }
    if info.transmission_format == 0 {
        let byte = *bytes.get(at)?;
        info.data_event_id = byte >> 4;
        info.event_section = byte & 0x08 != 0;
        let byte = *bytes.get(at + 1)?;
        info.ondemand_retrieval = byte & 0x80 != 0;
        info.file_storable = byte & 0x40 != 0;
        info.start_priority = (byte >> 5) & 1;
    }
    Some(info)
}

/// The descriptors a DII gives a module. STD-B24 Volume 3 Part 3 clause
/// 6.2.3; only the type and the compression are operated (TR-B14 Volume 3
/// Table 4-4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Described {
    /// The `Type` descriptor: a media type, `multipart/mixed` for a module
    /// of several files.
    pub content_type: Option<String>,
    /// The `Compression Type` descriptor: the method (0 is zlib) and how
    /// long the module is once unpacked.
    pub compression: Option<(u8, u32)>,
}

const MODULE_DESCRIPTOR_TYPE: u8 = 0x01;
const MODULE_DESCRIPTOR_COMPRESSION: u8 = 0xc2;
/// In the private data of the DII, whether the documents go back to the
/// entry point when the data event changes.
const DII_DESCRIPTOR_ARIB_BXML_PRIVATE: u8 = 0xf0;

/// Reads a module's descriptors.
pub fn describe(info: &[u8]) -> Described {
    let mut described = Described::default();
    for descriptor in descriptors(info) {
        match descriptor.tag {
            MODULE_DESCRIPTOR_TYPE => {
                described.content_type =
                    Some(String::from_utf8_lossy(&descriptor.body).into_owned());
            }
            MODULE_DESCRIPTOR_COMPRESSION if descriptor.body.len() >= 5 => {
                described.compression = Some((descriptor.body[0], be32(&descriptor.body[1..])));
            }
            _ => {}
        }
    }
    described
}

/// The `return_to_entry_flag` of the DII's private data, where it was sent.
pub fn return_to_entry(private_data: &[u8]) -> Option<bool> {
    let described = descriptors(private_data);
    let found = find(&described, DII_DESCRIPTOR_ARIB_BXML_PRIVATE)?;
    Some(found.body.first()? & 0x80 != 0)
}

/// The most a module unpacks to.
const MOST_UNPACKED: usize = 16 * 1024 * 1024;

/// A module's bytes as the documents see them: unpacked, where the DII said
/// the module was compressed. `None` where it said so and the bytes do not
/// unpack.
pub fn unpack(described: &Described, data: &[u8]) -> Option<Vec<u8>> {
    match described.compression {
        None => Some(data.to_vec()),
        Some((0, original)) => {
            // No more than the DII said, and never more than a module can be:
            // a few bytes that unpack to gigabytes are not a module.
            let most = match usize::try_from(original) {
                Ok(0) | Err(_) => MOST_UNPACKED,
                Ok(original) => original.min(MOST_UNPACKED),
            };
            let out = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, most).ok()?;
            (original == 0 || out.len() == original as usize).then_some(out)
        }
        Some(_) => None,
    }
}

/// One file of a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// The `Content-Location`, which is the name a document refers to it by
    /// -- empty for a module that is one file with no name.
    pub location: String,
    /// The media type, lower-cased, without its parameters.
    pub content_type: String,
    pub body: Vec<u8>,
}

/// The files of a module: the parts of a `multipart/mixed` body, or the
/// module itself as one file. STD-B24 Volume 2 Part 2 clause 9.
///
/// **A module of several files is an entity**: it begins with its own
/// headers, and the `Content-Type` among them carries the boundary the
/// parts are cut at -- the DII's type descriptor, where it is sent at all,
/// says only `multipart/mixed`. A module of one file is the file's bytes
/// with nothing in front.
pub fn files(content_type: Option<&str>, data: &[u8]) -> Vec<File> {
    let (kind, _) = media_type(content_type.unwrap_or("multipart/mixed"));
    if kind != "multipart/mixed" {
        return vec![File {
            location: String::new(),
            content_type: kind,
            body: data.to_vec(),
        }];
    }
    let Some((headers, body)) = entity_headers(data) else {
        return Vec::new();
    };
    let Some(kind) = headers.iter().find(|(name, _)| name == "content-type") else {
        return Vec::new();
    };
    let (kind, parameters) = media_type(&kind.1);
    if kind != "multipart/mixed" {
        return Vec::new();
    }
    let Some(boundary) = parameters
        .iter()
        .find(|(name, _)| name == "boundary")
        .map(|(_, value)| value.clone())
    else {
        return Vec::new();
    };
    let delimiter = format!("--{boundary}\r\n").into_bytes();
    let next = format!("\r\n--{boundary}\r\n").into_bytes();
    let close = format!("\r\n--{boundary}--\r\n").into_bytes();
    let mut out = Vec::new();
    let mut at = match seek(body, &delimiter, 0) {
        Some(found) => found + delimiter.len(),
        None => return out,
    };
    loop {
        let (end, last) = match seek(body, &next, at) {
            Some(found) => (found, false),
            None => match seek(body, &close, at) {
                Some(found) => (found, true),
                None => return out,
            },
        };
        if let Some(file) = entity(&body[at..end]) {
            out.push(file);
        }
        if last {
            return out;
        }
        at = end + next.len();
    }
}

/// One header line: the name, lower-cased, and the value, trimmed.
type Header = (String, String);

/// The header lines of an entity (names lower-cased, values trimmed) and
/// the bytes after the empty line that ends them.
fn entity_headers(part: &[u8]) -> Option<(Vec<Header>, &[u8])> {
    let mut headers = Vec::new();
    let mut at = 0;
    loop {
        let end = seek(part, b"\r\n", at)?;
        if end == at {
            return Some((headers, &part[at + 2..]));
        }
        let line = &part[at..end];
        at = end + 2;
        let Some(colon) = line.iter().position(|byte| *byte == b':') else {
            continue;
        };
        headers.push((
            String::from_utf8_lossy(&line[..colon])
                .trim()
                .to_ascii_lowercase(),
            String::from_utf8_lossy(&line[colon + 1..])
                .trim()
                .to_owned(),
        ));
    }
}

/// One body part: its headers, then its bytes.
fn entity(part: &[u8]) -> Option<File> {
    let (headers, body) = entity_headers(part)?;
    let mut location = None;
    let mut content_type = None;
    for (name, value) in headers {
        match name.as_str() {
            "content-location" => location = Some(value),
            "content-type" => content_type = Some(media_type(&value).0),
            _ => {}
        }
    }
    Some(File {
        location: location?,
        content_type: content_type?,
        body: body.to_vec(),
    })
}

/// A media type: `type/subtype`, lower-cased, and its parameters with the
/// quotes taken off the values.
fn media_type(text: &str) -> (String, Vec<(String, String)>) {
    let mut pieces = text.split(';');
    let kind = pieces.next().unwrap_or("").trim().to_ascii_lowercase();
    let parameters = pieces
        .filter_map(|piece| {
            let (name, value) = piece.split_once('=')?;
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|inner| inner.strip_suffix('"'))
                .unwrap_or(value);
            Some((name.trim().to_ascii_lowercase(), value.to_owned()))
        })
        .collect();
    (kind, parameters)
}

fn seek(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from > haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|found| from + found)
}

/// An event sent in a stream descriptors section. STD-B24 Volume 3 Part 3
/// clause 8, and ISO/IEC 13818-6 for the NPT reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The NPT reference descriptor: where the stream's clock stands
    /// against the normal play time the events are timed in.
    NptReference {
        post_discontinuity: bool,
        content_id: u8,
        stc: u64,
        npt: i64,
        scale: (i16, i16),
    },
    /// The general event descriptor: a message to the documents, sent now
    /// or at a time.
    General {
        group_id: u16,
        time: Timing,
        kind: u8,
        id: u16,
        private_data: Vec<u8>,
    },
}

/// When a general event is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    /// On arrival.
    Immediate,
    /// At a normal play time.
    Npt(u64),
    /// At a time of day (MJD and JST, as the tables write them), which is
    /// specified and not operated.
    Clock(u64),
    /// After a while, likewise.
    Relative(u64),
}

const STREAM_DESCRIPTOR_NPT_REFERENCE: u8 = 0x17;
const STREAM_DESCRIPTOR_GENERAL_EVENT: u8 = 0x40;

/// The events of one stream descriptors section, with the data event they
/// belong to and the group they are in (the two halves of the section's
/// `table_id_extension`).
pub fn events(section: &mpegts::dsmcc::StreamDescriptors) -> (u8, u16, Vec<Event>) {
    let data_event_id = (section.extension >> 12) as u8;
    let group_id = section.extension & 0x0fff;
    let events = descriptors(&section.descriptors)
        .into_iter()
        .filter_map(|descriptor| event(&descriptor))
        .collect();
    (data_event_id, group_id, events)
}

fn event(descriptor: &Descriptor) -> Option<Event> {
    let body = &descriptor.body;
    match descriptor.tag {
        STREAM_DESCRIPTOR_NPT_REFERENCE if body.len() >= 18 => Some(Event::NptReference {
            post_discontinuity: body[0] & 0x80 != 0,
            content_id: body[0] & 0x7f,
            stc: (u64::from(body[1] & 1) << 32) | u64::from(be32(&body[2..])),
            npt: {
                let low = i64::from(be32(&body[10..]));
                if body[9] & 1 != 0 {
                    low - (1 << 32)
                } else {
                    low
                }
            },
            scale: (be16(&body[14..]) as i16, be16(&body[16..]) as i16),
        }),
        STREAM_DESCRIPTOR_GENERAL_EVENT if body.len() >= 11 => {
            let group_id = be16(body) >> 4;
            let time = match body[2] {
                0x00 => Timing::Immediate,
                0x01 | 0x05 => Timing::Clock(forty(&body[3..])),
                0x02 => Timing::Npt((u64::from(body[3] & 1) << 32) | u64::from(be32(&body[4..]))),
                0x03 => Timing::Relative(
                    (u64::from(body[3] & 0x0f) << 32) | u64::from(be32(&body[4..])),
                ),
                _ => return None,
            };
            Some(Event::General {
                group_id,
                time,
                kind: body[8],
                id: be16(&body[9..]),
                private_data: body[11..].to_vec(),
            })
        }
        _ => None,
    }
}

fn forty(bytes: &[u8]) -> u64 {
    (u64::from(bytes[0]) << 32) | u64::from(be32(&bytes[1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 地上波のエントリーコンポーネント: 運用どおりの値が読めること。
    #[test]
    fn the_entry_component_says_how_the_documents_are_laid_out() {
        // format 0, entry point, auto start, 960x540; no XML, version given,
        // independent, style for TV; BML 3.0; carousel info.
        let bytes = [0x33, 0x30, 0x00, 0x03, 0x00, 0x00, 0xF8, 0xA0];
        let info = additional_info(&bytes).expect("readable");
        assert!(info.entry_point && info.auto_start);
        assert_eq!(info.resolution(), Some(((960, 540), true)));
        assert_eq!(info.bml_version, (3, 0));
        assert_eq!(info.data_event_id, 0xf);
        assert!(info.event_section && info.ondemand_retrieval);
        assert_eq!(info.start_priority, 1);
    }

    #[test]
    fn a_component_that_is_not_the_entry_has_only_the_carousel_info() {
        // As sent on air for non-entry components.
        let bytes = [0x1f, 0xff, 0xbf];
        let info = additional_info(&bytes).expect("readable");
        assert!(!info.entry_point);
        assert_eq!(info.data_event_id, 0xf);
        assert!(info.event_section && info.ondemand_retrieval);
        assert_eq!(info.start_priority, 1);
    }

    #[test]
    fn a_module_says_its_type_and_its_compression() {
        let info = [0x01, 4, b't', b'e', b'x', b't', 0xc2, 5, 0, 0, 0, 1, 0];
        let described = describe(&info);
        assert_eq!(described.content_type.as_deref(), Some("text"));
        assert_eq!(described.compression, Some((0, 256)));
        assert_eq!(return_to_entry(&[0xf0, 1, 0x80]), Some(true));
        assert_eq!(return_to_entry(&[]), None);
    }

    #[test]
    fn a_compressed_module_unpacks_to_the_length_it_said() {
        let original = b"<bml>hello hello hello hello</bml>";
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(original, 6);
        let described = Described {
            content_type: None,
            compression: Some((0, original.len() as u32)),
        };
        assert_eq!(unpack(&described, &packed).as_deref(), Some(&original[..]));
        let wrong = Described {
            compression: Some((0, 3)),
            ..described
        };
        assert_eq!(unpack(&wrong, &packed), None);
    }

    #[test]
    fn a_multipart_module_is_its_files() {
        let body = b"Content-Type: multipart/mixed; boundary=\"BOUNDARY\"\r\n\r\n--BOUNDARY\r\nContent-Type: text/X-arib-bml; charset=\"euc-jp\"\r\nContent-Location: startup.bml\r\n\r\n<bml/>\r\n--BOUNDARY\r\nContent-Location: a.png\r\nContent-Type: image/png\r\n\r\n\x89PNG\r\n--BOUNDARY--\r\n";
        let files = files(None, body);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].location, "startup.bml");
        assert_eq!(files[0].content_type, "text/x-arib-bml");
        assert_eq!(files[0].body, b"<bml/>");
        assert_eq!(files[1].location, "a.png");
        assert_eq!(files[1].body, b"\x89PNG");
    }

    #[test]
    fn a_module_of_one_file_is_that_file() {
        let files = files(Some("image/jpeg"), b"\xff\xd8");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].content_type, "image/jpeg");
        assert!(files[0].location.is_empty());
    }

    #[test]
    fn a_general_event_sent_now_is_read_with_its_message() {
        let section = mpegts::dsmcc::StreamDescriptors {
            extension: 0xf001,
            descriptors: vec![
                0x40, 13, 0x00, 0x10, 0x00, 0, 0, 0, 0, 0, 0x01, 0x00, 0x2a, b'h', b'i',
            ],
            ..Default::default()
        };
        let (data_event, group, events) = events(&section);
        assert_eq!((data_event, group), (0xf, 1));
        assert_eq!(
            events,
            [Event::General {
                group_id: 1,
                time: Timing::Immediate,
                kind: 1,
                id: 0x2a,
                private_data: b"hi".to_vec(),
            }]
        );
    }
}

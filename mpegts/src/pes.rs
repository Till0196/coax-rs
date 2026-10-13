// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The wrapper an elementary stream is carried in.
//!
//! A transport stream carries pictures and sound as packetised elementary
//! streams: a short header saying which stream this is, when it is to be
//! decoded and when shown, and then as many bytes of the coded stream as the
//! multiplexer chose to send at once. Nothing here looks into the bytes after
//! the header.
//!
//! The length a header declares may be nought, which is not a header that says
//! nothing but one that declines to say: a coded picture is larger than sixteen
//! bits can count, so video streams leave the field empty and the packet ends
//! where the next one begins.

/// What every packetised elementary stream packet begins with.
pub const START_CODE: [u8; 3] = [0x00, 0x00, 0x01];

/// The shortest a header can be: the start code, the stream identifier and
/// the declared length.
pub const PREFIX: usize = 6;

/// Whether packets of this stream carry the optional header -- the flags, the
/// timestamps, the rest -- after the prefix. Most do; the handful that do not
/// carry something other than a coded elementary stream, and their payload
/// begins at the sixth byte.
pub const fn has_optional_header(stream_id: u8) -> bool {
    !matches!(
        stream_id,
        // padding, private stream 2, ECM, EMM, DSM-CC, ITU-T H.222.1 type E,
        // programme stream directory.
        0xBE | 0xBF | 0xF0 | 0xF1 | 0xF2 | 0xF8 | 0xFF
    )
}

/// Whether the stream identifier names a video stream, of which there are
/// sixteen, and whose packets are the ones that leave the length unsaid.
pub const fn is_video(stream_id: u8) -> bool {
    stream_id & 0xF0 == 0xE0
}

/// Whether the stream identifier names an audio stream, of which there are
/// thirty-two.
pub const fn is_audio(stream_id: u8) -> bool {
    stream_id & 0xE0 == 0xC0
}

/// The header of one packetised elementary stream packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Which stream this is: what kind, and which of that kind.
    pub stream_id: u8,
    /// How many bytes follow the prefix, or nought where the packet declines
    /// to say and ends where the next one starts.
    pub packet_length: u16,
    /// Whether the payload starts on a boundary the decoder can start at --
    /// an access unit for video, a frame for audio.
    pub data_alignment: bool,
    /// Whether the payload is scrambled at this layer rather than the
    /// transport one, and under which key. Nought is in the clear.
    pub scrambling: u8,
    /// When the access unit is to be shown, in the ninety kilohertz clock the
    /// stream keeps. Thirty-three bits, as sent: the clock rolls over about
    /// once a day and what to do about that is the caller's to decide.
    pub pts: Option<u64>,
    /// When the access unit is to be decoded, which differs from when it is
    /// shown wherever pictures are sent out of order.
    pub dts: Option<u64>,
    /// Where the payload begins, counted from the start of the packet.
    pub payload_at: usize,
}

impl Header {
    /// Reads a header from the start of a packet, or gives nothing where
    /// these bytes are not one: no start code, or too few bytes to hold the
    /// header the packet declares.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < PREFIX || bytes[..3] != START_CODE {
            return None;
        }
        let stream_id = bytes[3];
        let packet_length = (u16::from(bytes[4]) << 8) | u16::from(bytes[5]);
        if !has_optional_header(stream_id) {
            return Some(Self {
                stream_id,
                packet_length,
                data_alignment: false,
                scrambling: 0,
                pts: None,
                dts: None,
                payload_at: PREFIX,
            });
        }
        if bytes.len() < 9 {
            return None;
        }
        let scrambling = (bytes[6] >> 4) & 0x03;
        let data_alignment = bytes[6] & 0x04 != 0;
        let flags = bytes[7] >> 6;
        let header_len = usize::from(bytes[8]);
        let payload_at = 9 + header_len;
        if bytes.len() < payload_at {
            return None;
        }
        // The two flags together: the high one says a presentation stamp is
        // there, and both together say a decoding stamp follows it. The low
        // one on its own is forbidden and read as neither.
        let pts = (flags & 0x02 != 0 && header_len >= 5)
            .then(|| timestamp(&bytes[9..14]))
            .flatten();
        let dts = (flags == 0x03 && header_len >= 10)
            .then(|| timestamp(&bytes[14..19]))
            .flatten();
        Some(Self {
            stream_id,
            packet_length,
            data_alignment,
            scrambling,
            pts,
            dts,
            payload_at,
        })
    }

    /// How many bytes of payload the packet declares, or nothing where it
    /// declared no length at all.
    pub fn payload_len(&self) -> Option<usize> {
        if self.packet_length == 0 {
            return None;
        }
        (PREFIX + usize::from(self.packet_length)).checked_sub(self.payload_at)
    }

    /// How long the whole packet is, header and payload, or nothing where the
    /// packet declared no length.
    pub fn total_len(&self) -> Option<usize> {
        if self.packet_length == 0 {
            return None;
        }
        Some(PREFIX + usize::from(self.packet_length))
    }

    pub fn is_video(&self) -> bool {
        is_video(self.stream_id)
    }

    pub fn is_audio(&self) -> bool {
        is_audio(self.stream_id)
    }
}

/// A whole packet: its header, and the bytes of the elementary stream it
/// carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet<'a> {
    pub header: Header,
    pub payload: &'a [u8],
}

impl<'a> Packet<'a> {
    /// Reads a packet from bytes that begin with one. Where the packet declares
    /// a length the payload is cut to it; where it declares none the payload is
    /// everything that was handed in.
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        let header = Header::parse(bytes)?;
        let rest = &bytes[header.payload_at..];
        let payload = match header.payload_len() {
            Some(len) if len <= rest.len() => &rest[..len],
            _ => rest,
        };
        Some(Self { header, payload })
    }
}

/// One of the thirty-three bit stamps, out of the five bytes it is spread
/// across with markers between. Nothing where the markers are wrong, which is
/// the cheapest sign that these bytes are not a timestamp at all.
pub fn timestamp(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < 5 || bytes[0] & 0x01 == 0 || bytes[2] & 0x01 == 0 || bytes[4] & 0x01 == 0 {
        return None;
    }
    Some(
        (u64::from(bytes[0] & 0x0E) << 29)
            | (u64::from(bytes[1]) << 22)
            | (u64::from(bytes[2] & 0xFE) << 14)
            | (u64::from(bytes[3]) << 7)
            | (u64::from(bytes[4]) >> 1),
    )
}

/// Writes a stamp back out into the five bytes it is sent as, under the four
/// bit prefix the header gives it.
pub fn write_timestamp(prefix: u8, stamp: u64) -> [u8; 5] {
    let stamp = stamp & 0x1_FFFF_FFFF;
    [
        (prefix << 4) | ((((stamp >> 30) as u8) & 0x07) << 1) | 0x01,
        (stamp >> 22) as u8,
        ((((stamp >> 15) as u8) & 0x7F) << 1) | 0x01,
        (stamp >> 7) as u8,
        (((stamp as u8) & 0x7F) << 1) | 0x01,
    ]
}

#[cfg(test)]
mod tests {
    use super::{Header, Packet, timestamp, write_timestamp};

    /// A packet with the optional header on it, the stamps that were asked
    /// for, and the payload given.
    fn packet(stream_id: u8, pts: Option<u64>, dts: Option<u64>, payload: &[u8]) -> Vec<u8> {
        let mut optional = Vec::new();
        let mut flags = 0u8;
        if let Some(pts) = pts {
            flags |= 0x80;
            let prefix = if dts.is_some() { 0x3 } else { 0x2 };
            optional.extend_from_slice(&write_timestamp(prefix, pts));
        }
        if let Some(dts) = dts {
            flags |= 0x40;
            optional.extend_from_slice(&write_timestamp(0x1, dts));
        }
        let mut out = vec![0x00, 0x00, 0x01, stream_id, 0x00, 0x00];
        out.push(0x84);
        out.push(flags);
        out.push(optional.len() as u8);
        out.extend_from_slice(&optional);
        out.extend_from_slice(payload);
        let length = out.len() - 6;
        out[4] = (length >> 8) as u8;
        out[5] = (length & 0xFF) as u8;
        out
    }

    #[test]
    fn a_header_says_which_stream_and_where_the_payload_starts() {
        let bytes = packet(0xE0, None, None, &[1, 2, 3]);
        let header = Header::parse(&bytes).unwrap();
        assert_eq!(header.stream_id, 0xE0);
        assert!(header.is_video());
        assert!(header.data_alignment);
        assert_eq!(header.scrambling, 0);
        assert_eq!(header.payload_at, 9);
        assert_eq!(header.payload_len(), Some(3));
        assert_eq!(header.total_len(), Some(bytes.len()));
    }

    #[test]
    fn the_stamps_come_back_as_they_were_written() {
        let bytes = packet(0xE0, Some(0x1_2345_6789), Some(0x0_1234_5678), &[9]);
        let header = Header::parse(&bytes).unwrap();
        assert_eq!(header.pts, Some(0x1_2345_6789));
        assert_eq!(header.dts, Some(0x0_1234_5678));
    }

    /// The high bit of the pair on its own says a presentation stamp; both
    /// say a decoding stamp follows it.
    #[test]
    fn a_packet_with_one_stamp_has_no_decoding_stamp() {
        let bytes = packet(0xC0, Some(1000), None, &[1]);
        let header = Header::parse(&bytes).unwrap();
        assert!(header.is_audio());
        assert_eq!(header.pts, Some(1000));
        assert_eq!(header.dts, None);
    }

    /// A video access unit is longer than the field can count, so the field
    /// is left empty and the payload runs to whatever arrived.
    #[test]
    fn a_packet_that_declares_no_length_carries_everything_handed_in() {
        let mut bytes = packet(0xE0, Some(42), None, &[7; 40]);
        bytes[4] = 0;
        bytes[5] = 0;
        let parsed = Packet::parse(&bytes).unwrap();
        assert_eq!(parsed.header.packet_length, 0);
        assert_eq!(parsed.header.payload_len(), None);
        assert_eq!(parsed.header.total_len(), None);
        assert_eq!(parsed.payload.len(), 40);
    }

    /// Padding and the like have no optional header, so the payload starts at
    /// the sixth byte.
    #[test]
    fn a_stream_without_the_optional_header_starts_its_payload_at_six() {
        let bytes = [0x00, 0x00, 0x01, 0xBE, 0x00, 0x03, 1, 2, 3];
        let parsed = Packet::parse(&bytes).unwrap();
        assert_eq!(parsed.header.payload_at, 6);
        assert_eq!(parsed.payload, &[1, 2, 3]);
        assert_eq!(parsed.header.pts, None);
    }

    #[test]
    fn bytes_that_are_not_a_packet_are_not_read_as_one() {
        assert!(Header::parse(&[0x00, 0x00, 0x02, 0xE0, 0, 0]).is_none());
        assert!(Header::parse(&[0x00, 0x00, 0x01]).is_none());
        // A header longer than the bytes handed in.
        assert!(Header::parse(&[0x00, 0x00, 0x01, 0xE0, 0, 0, 0x80, 0x00, 0x40]).is_none());
    }

    #[test]
    fn a_stamp_whose_markers_are_wrong_is_not_a_stamp() {
        let mut bytes = write_timestamp(0x2, 90_000);
        assert_eq!(timestamp(&bytes), Some(90_000));
        bytes[2] &= 0xFE;
        assert_eq!(timestamp(&bytes), None);
    }
}

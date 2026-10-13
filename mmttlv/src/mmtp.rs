// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The MMTP packet header, which is what a TLV payload turns out to be once
//! the IP and UDP layers are peeled off.

use crate::{be16, be32};

/// The payload type of a packet carrying media, in media processing units.
pub const MPU: u8 = 0x00;
/// The payload type of a packet carrying signalling.
pub const SIGNALLING: u8 = 0x02;

/// What part of a processing unit a payload carries. Only [`MFU`] is media --
/// the other two are the boxes that describe it.
pub const MPU_METADATA: u8 = 0;
pub const MOVIE_FRAGMENT_METADATA: u8 = 1;
pub const MFU: u8 = 2;

/// Which piece of a data unit a payload is, where the unit did not fit in one
/// packet. A unit that did fit says [`COMPLETE`].
pub const COMPLETE: u8 = 0;
pub const FIRST: u8 = 1;
pub const MIDDLE: u8 = 2;
pub const LAST: u8 = 3;

/// The fixed part of an MPU payload: length, flags, counter, sequence number.
const MPU_HEADER: usize = 8;
/// The header a timed media fragment unit carries in front of its bytes.
const TIMED_HEADER: usize = 14;

/// One MMTP packet, which is what a datagram turns out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mmtp<'a> {
    pub packet_id: u16,
    pub payload_type: u8,
    /// When the sender handed the packet to the delivery layer, in NTP short
    /// format: seconds in the upper half and a fraction of a second in the
    /// lower. It is the only clock this transmission has, so it is what a rate
    /// is worked out from. See [`Clock`].
    pub timestamp: u32,
    /// Counts the packets of this identifier, so a gap in it is a gap in the
    /// stream. It wraps, and the wrap is what the subtraction is done modulo.
    pub sequence_number: u32,
    /// Whether what follows can be decoded without anything before it -- a
    /// random access point.
    pub rap: bool,
    pub payload: &'a [u8],
    /// Whether the payload is under conditional access.
    pub scrambled: bool,
}

/// Reads the header of an MMTP packet.
pub fn read(buffer: &[u8]) -> Option<Mmtp<'_>> {
    if buffer.len() < 12 {
        return None;
    }
    let mut at = 12;
    // A packet counter, where the sender put one in.
    if buffer[0] & 0x20 != 0 {
        at += 4;
    }
    if at > buffer.len() {
        return None;
    }
    let mut scrambled = false;
    // The header extension, which is where the access control says whether the
    // payload was scrambled.
    if buffer[0] & 0x02 != 0 {
        if at + 4 > buffer.len() {
            return None;
        }
        let kind = be16(&buffer[at..]);
        let length = usize::from(be16(&buffer[at + 2..]));
        at += 4;
        if at + length > buffer.len() {
            return None;
        }
        if kind == 0 {
            scrambled = scrambling(&buffer[at..at + length]);
        }
        at += length;
    }
    Some(Mmtp {
        packet_id: be16(&buffer[2..]),
        payload_type: buffer[1] & 0x3f,
        timestamp: be32(&buffer[4..]),
        sequence_number: be32(&buffer[8..]),
        rap: buffer[0] & 0x01 != 0,
        payload: &buffer[at..],
        scrambled,
    })
}

/// Whether the multi-type header extension says the payload is scrambled.
fn scrambling(mut buffer: &[u8]) -> bool {
    while buffer.len() >= 4 {
        let kind = be16(buffer) & 0x7fff;
        let last = buffer[0] & 0x80 != 0;
        let length = usize::from(be16(&buffer[2..]));
        if 4 + length > buffer.len() {
            return false;
        }
        if kind == 1 {
            return length > 0 && buffer[4] & 0x1f != 0;
        }
        buffer = &buffer[4 + length..];
        if last {
            return false;
        }
    }
    false
}

/// One data unit out of an MPU payload. The timed fields are the header a media
/// fragment unit of a timed asset carries; an untimed one names an item
/// instead. What a fragmented payload holds is a piece of the unit's `data`,
/// not a unit of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DataUnit<'a> {
    pub movie_fragment_sequence_number: u32,
    /// Which sample of the movie fragment this is. Broadcasters leave it at
    /// nought, so nothing may be counted from it -- see [`crate::mpu`].
    pub sample_number: u32,
    /// Where the unit sits in its sample, for a sample split into several.
    pub offset: u32,
    pub priority: u8,
    pub dependency_counter: u8,
    /// Which item of an untimed asset this is. Nought for a timed one.
    pub item_id: u32,
    pub data: &'a [u8],
}

/// An MPU payload, which is what an MMTP packet of type [`MPU`] carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpuPayload<'a> {
    /// [`MPU_METADATA`], [`MOVIE_FRAGMENT_METADATA`] or [`MFU`].
    pub fragment_type: u8,
    /// Whether the asset has a timeline. Media does; a file sent alongside it
    /// does not.
    pub timed: bool,
    /// [`COMPLETE`], [`FIRST`], [`MIDDLE`] or [`LAST`].
    pub fragmentation_indicator: u8,
    /// Whether several units share the payload, each behind its own length.
    /// A payload cannot both aggregate and be a fragment.
    pub aggregation_flag: bool,
    /// Counts down the fragments left of the unit being split.
    pub fragment_counter: u8,
    /// Which processing unit this belongs to. It is what tells one group of
    /// samples from the next.
    pub mpu_sequence_number: u32,
    pub units: Vec<DataUnit<'a>>,
}

/// Reads an MPU payload, or nothing where it is not one.
pub fn read_mpu(payload: &[u8]) -> Option<MpuPayload<'_>> {
    if payload.len() < MPU_HEADER {
        return None;
    }
    let length = usize::from(be16(payload));
    if length + 2 > payload.len() || length + 2 < MPU_HEADER {
        return None;
    }
    let payload = &payload[..2 + length];
    let flags = payload[2];
    let mut read = MpuPayload {
        fragment_type: flags >> 4,
        timed: flags & 0x08 != 0,
        fragmentation_indicator: (flags >> 1) & 0x03,
        aggregation_flag: flags & 0x01 != 0,
        fragment_counter: payload[3],
        mpu_sequence_number: be32(&payload[4..]),
        units: Vec::new(),
    };
    let mut body = &payload[MPU_HEADER..];
    if !read.aggregation_flag {
        read.units.push(unit(body, read.timed, read.fragment_type)?);
        return Some(read);
    }
    // Aggregated units are each behind two bytes of their own length. An
    // aggregating payload is never fragmented, so every unit in it is whole.
    while !body.is_empty() {
        if body.len() < 2 {
            return None;
        }
        let length = usize::from(be16(body));
        body = &body[2..];
        if length > body.len() {
            return None;
        }
        read.units
            .push(unit(&body[..length], read.timed, read.fragment_type)?);
        body = &body[length..];
    }
    Some(read)
}

/// One data unit. Only a media fragment unit carries a header in front of its
/// bytes -- the metadata fragments are boxes, all the way down. The header is
/// on every piece of a split unit, not only the first.
fn unit(body: &[u8], timed: bool, fragment_type: u8) -> Option<DataUnit<'_>> {
    if fragment_type != MFU {
        return Some(DataUnit {
            data: body,
            ..DataUnit::default()
        });
    }
    if !timed {
        if body.len() < 4 {
            return None;
        }
        return Some(DataUnit {
            item_id: be32(body),
            data: &body[4..],
            ..DataUnit::default()
        });
    }
    if body.len() < TIMED_HEADER {
        return None;
    }
    Some(DataUnit {
        movie_fragment_sequence_number: be32(body),
        sample_number: be32(&body[4..]),
        offset: be32(&body[8..]),
        priority: body[12],
        dependency_counter: body[13],
        item_id: 0,
        data: &body[TIMED_HEADER..],
    })
}

#[cfg(test)]
mod tests {
    use super::{COMPLETE, MFU, SIGNALLING, read, read_mpu};

    #[test]
    fn an_mmtp_packet_is_read_past_its_extensions() {
        let mut packet = vec![0x00, SIGNALLING, 0x10, 0x00];
        packet.extend_from_slice(&0x1234_5678u32.to_be_bytes());
        packet.extend_from_slice(&[0; 4]);
        packet.extend_from_slice(&[0xaa, 0xbb]);
        let read = read(&packet).expect("the header is whole");
        assert_eq!(read.packet_id, 0x1000);
        assert_eq!(read.payload_type, SIGNALLING);
        assert_eq!(read.timestamp, 0x1234_5678);
        assert_eq!(read.payload, &[0xaa, 0xbb]);
        assert!(!read.scrambled);
    }

    /// An MPU payload around a body, so the length and flags are built the way
    /// the standard says they are.
    fn mpu(flags: u8, mpu_sequence_number: u32, body: &[u8]) -> Vec<u8> {
        let mut payload = vec![0, 0, flags, 0];
        payload.extend_from_slice(&mpu_sequence_number.to_be_bytes());
        payload.extend_from_slice(body);
        let length = (payload.len() - 2) as u16;
        payload[..2].copy_from_slice(&length.to_be_bytes());
        payload
    }

    fn timed(sample_number: u32, data: &[u8]) -> Vec<u8> {
        let mut unit = Vec::new();
        unit.extend_from_slice(&1u32.to_be_bytes());
        unit.extend_from_slice(&sample_number.to_be_bytes());
        unit.extend_from_slice(&2u32.to_be_bytes());
        unit.push(3);
        unit.push(4);
        unit.extend_from_slice(data);
        unit
    }

    #[test]
    fn a_timed_fragment_unit_is_read_with_its_header() {
        let payload = mpu(
            (MFU << 4) | 0x08 | (COMPLETE << 1),
            9,
            &timed(5, b"picture"),
        );
        let read = read_mpu(&payload).expect("it is an MPU payload");
        assert_eq!(read.fragment_type, MFU);
        assert!(read.timed);
        assert!(!read.aggregation_flag);
        assert_eq!(read.fragmentation_indicator, COMPLETE);
        assert_eq!(read.mpu_sequence_number, 9);
        assert_eq!(read.units.len(), 1);
        let unit = read.units[0];
        assert_eq!(unit.movie_fragment_sequence_number, 1);
        assert_eq!(unit.sample_number, 5);
        assert_eq!(unit.offset, 2);
        assert_eq!(unit.priority, 3);
        assert_eq!(unit.dependency_counter, 4);
        assert_eq!(unit.data, b"picture");
    }

    #[test]
    fn an_aggregating_payload_is_read_unit_by_unit() {
        let mut body = Vec::new();
        for data in [&b"a"[..], &b"bb"[..]] {
            let unit = timed(0, data);
            body.extend_from_slice(&(unit.len() as u16).to_be_bytes());
            body.extend_from_slice(&unit);
        }
        let payload = mpu((MFU << 4) | 0x08 | 0x01, 2, &body);
        let read = read_mpu(&payload).expect("it is an MPU payload");
        assert!(read.aggregation_flag);
        let data: Vec<&[u8]> = read.units.iter().map(|unit| unit.data).collect();
        assert_eq!(data, vec![&b"a"[..], &b"bb"[..]]);
    }

    #[test]
    fn a_payload_longer_than_the_bytes_it_is_in_front_of_is_not_a_payload() {
        let mut payload = mpu((MFU << 4) | 0x08, 1, &timed(0, b"x"));
        payload[..2].copy_from_slice(&0xffffu16.to_be_bytes());
        assert!(read_mpu(&payload).is_none());
        assert!(read_mpu(&payload[..4]).is_none());
    }
}

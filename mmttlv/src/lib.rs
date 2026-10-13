// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The MMT/TLV transmission layer: TLV packets, the MMTP headers inside them,
//! and the MMT signalling messages those carry.
//!
//! Nothing here is particular to Japanese broadcasting: the layer says only how
//! bytes are framed and put back together, and what the signalling means
//! belongs to `isdb`.
//!
//! A transport stream is packets of one length carrying one thing. This is not:
//! a TLV packet holds an IP datagram, the datagram holds a UDP one, the UDP
//! payload is an MMTP packet, and that holds a fragment of a signalling message
//! that may have started three packets ago.

pub mod clock;
pub mod mmtp;
pub mod mpu;
pub mod signalling;
pub mod tlv;

/// The two big-endian readings every layer of this is written in.
pub(crate) fn be16(bytes: &[u8]) -> u16 {
    (u16::from(bytes[0]) << 8) | u16::from(bytes[1])
}

pub(crate) fn be32(bytes: &[u8]) -> u32 {
    (u32::from(bytes[0]) << 24)
        | (u32::from(bytes[1]) << 16)
        | (u32::from(bytes[2]) << 8)
        | u32::from(bytes[3])
}

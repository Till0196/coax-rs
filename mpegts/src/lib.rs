// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! MPEG-2 transport streams: the packets, the tables carried in them, and the
//! check that says a table arrived intact.
//!
//! Nothing here is particular to Japanese broadcasting. What the tables of one
//! mean where the broadcaster is Japanese is in `isdb`.

pub mod continuity;
pub mod demux;
pub mod dsmcc;
pub mod pat;
pub mod pes;
pub mod sections;

/// Every packet of a transport stream is this long, and starts with this byte.
pub const PACKET: usize = 188;
pub const SYNC: u8 = 0x47;

/// The check the tables of a transport stream carry, which comes to zero over
/// a table that is intact.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut register: u32 = 0xFFFF_FFFF;
    for byte in bytes {
        register ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            register = if register & 0x8000_0000 != 0 {
                (register << 1) ^ 0x04C1_1DB7
            } else {
                register << 1
            };
        }
    }
    register
}

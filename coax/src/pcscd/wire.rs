// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! pcsc-lite's client-to-daemon protocol: the commands, the structures and
//! their sizes, and the status codes.
//!
//! The wire format is the C structures as the host lays them out, so numbers
//! are in host byte order -- that is what the client library writes. The sizes
//! are pcsc-lite's own and are asserted in the tests.

use std::io::{self, Read};

/// The protocol version this speaks, `PROTOCOL_VERSION_MAJOR` and
/// `PROTOCOL_VERSION_MINOR` in `winscard_msg.h`. A client offering an older
/// minor version down to [`BACKWARD_MINOR`] is accepted and told this one,
/// which is what pcscd itself answers; anything else is refused with
/// `SCARD_E_SERVICE_STOPPED`.
pub const VERSION_MAJOR: u32 = 4;
pub const VERSION_MINOR: u32 = 6;
pub const BACKWARD_MINOR: u32 = 4;
/// Minor versions at or below this use the fixed-size reader array and expect
/// it after registering to wait.
pub const OLD_READER_STATE: u32 = 5;

/// Commands, from `enum pcsc_msg_commands`.
pub mod command {
    pub const ESTABLISH_CONTEXT: u32 = 0x01;
    pub const RELEASE_CONTEXT: u32 = 0x02;
    pub const LIST_READERS: u32 = 0x03;
    pub const CONNECT: u32 = 0x04;
    pub const RECONNECT: u32 = 0x05;
    pub const DISCONNECT: u32 = 0x06;
    pub const BEGIN_TRANSACTION: u32 = 0x07;
    pub const END_TRANSACTION: u32 = 0x08;
    pub const TRANSMIT: u32 = 0x09;
    pub const CONTROL: u32 = 0x0A;
    pub const STATUS: u32 = 0x0B;
    pub const GET_STATUS_CHANGE: u32 = 0x0C;
    pub const CANCEL: u32 = 0x0D;
    pub const CANCEL_TRANSACTION: u32 = 0x0E;
    pub const GET_ATTRIB: u32 = 0x0F;
    pub const SET_ATTRIB: u32 = 0x10;
    pub const VERSION: u32 = 0x11;
    pub const GET_READERS_STATE: u32 = 0x12;
    pub const WAIT_READER_STATE_CHANGE: u32 = 0x13;
    pub const STOP_WAITING_READER_STATE_CHANGE: u32 = 0x14;
    pub const GET_READER_EVENTS: u32 = 0x15;
    pub const GET_STATE_SIZE: u32 = 0x16;
    pub const GET_STATE_ARRAY: u32 = 0x17;
}

/// `PCSCLITE_MAX_READERS_CONTEXTS`, the length of the reader array a client of
/// any supported version expects.
pub const MAX_READERS: usize = 16;
/// `MAX_READERNAME`.
pub const MAX_READER_NAME: usize = 128;
/// `MAX_ATR_SIZE`.
pub const MAX_ATR: usize = 33;
/// `MAX_BUFFER_SIZE`, the room for an attribute value.
pub const MAX_ATTRIBUTE: usize = 264;
/// `MAX_BUFFER_SIZE_EXTENDED`, the longest APDU either way.
pub const MAX_TRANSMIT: usize = 4 + 3 + (1 << 16) + 3 + 2;
/// `READER_STATE`.
pub const READER_STATE_LEN: usize = 184;

/// The structure sizes.
pub const SIZE_VERSION: usize = 12;
pub const SIZE_ESTABLISH: usize = 12;
pub const SIZE_RELEASE: usize = 8;
pub const SIZE_CONNECT: usize = 4 + MAX_READER_NAME + 4 * 5;
pub const SIZE_RECONNECT: usize = 24;
pub const SIZE_DISCONNECT: usize = 12;
pub const SIZE_BEGIN: usize = 8;
pub const SIZE_END: usize = 12;
pub const SIZE_CANCEL: usize = 8;
pub const SIZE_STATUS: usize = 8;
pub const SIZE_TRANSMIT: usize = 32;
pub const SIZE_CONTROL: usize = 24;
pub const SIZE_GET_SET: usize = 4 + 4 + MAX_ATTRIBUTE + 4 + 4;
pub const SIZE_WAIT: usize = 8;
pub const SIZE_EVENTS: usize = 8;

/// A status code, `SCARD_*` in `pcsclite.h`. The numbers are the same as
/// Windows'.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Code(pub u32);

impl Code {
    pub const SUCCESS: Self = Self(0);
    pub const CANCELLED: Self = Self(0x8010_0002);
    pub const INVALID_HANDLE: Self = Self(0x8010_0003);
    pub const INVALID_PARAMETER: Self = Self(0x8010_0004);
    pub const INSUFFICIENT_BUFFER: Self = Self(0x8010_0008);
    pub const UNKNOWN_READER: Self = Self(0x8010_0009);
    pub const TIMEOUT: Self = Self(0x8010_000A);
    pub const NO_SMARTCARD: Self = Self(0x8010_000C);
    pub const PROTO_MISMATCH: Self = Self(0x8010_000F);
    pub const INVALID_VALUE: Self = Self(0x8010_0011);
    pub const NOT_TRANSACTED: Self = Self(0x8010_0016);
    pub const NO_SERVICE: Self = Self(0x8010_001D);
    pub const SERVICE_STOPPED: Self = Self(0x8010_001E);
    pub const UNSUPPORTED_FEATURE: Self = Self(0x8010_0022);
    pub const NO_READERS_AVAILABLE: Self = Self(0x8010_002E);

    /// The code a result travels as.
    pub fn of<T>(result: &Result<T, Code>) -> Self {
        match result {
            Ok(_) => Self::SUCCESS,
            Err(code) => *code,
        }
    }
}

/// `SCARD_PROTOCOL_*` as pcsc-lite numbers them.
pub const PROTOCOL_UNDEFINED: u32 = 0;
pub const PROTOCOL_T1: u32 = 0x0002;

/// `SCARD_SHARE_*`.
pub const SHARE_EXCLUSIVE: u32 = 1;
pub const SHARE_SHARED: u32 = 2;
pub const SHARE_DIRECT: u32 = 3;

/// `SCARD_LEAVE_CARD` to `SCARD_EJECT_CARD`.
pub const LEAVE_CARD: u32 = 0;
pub const EJECT_CARD: u32 = 3;

/// `SCARD_SCOPE_USER` to pcsc-lite's own `SCARD_SCOPE_GLOBAL`.
pub const SCOPE_GLOBAL: u32 = 3;

/// The reader state bits of `READER_STATE`, as pcsc-lite numbers them: flags
/// here and ordinals on Windows.
pub const STATE_UNKNOWN: u32 = 0x0001;
pub const STATE_ABSENT: u32 = 0x0002;
pub const STATE_PRESENT: u32 = 0x0004;
pub const STATE_POWERED: u32 = 0x0010;
pub const STATE_NEGOTIABLE: u32 = 0x0020;
pub const STATE_SPECIFIC: u32 = 0x0040;

/// `PCSCLITE_SHARING_EXCLUSIVE_CONTEXT`.
pub const SHARING_EXCLUSIVE: i32 = -1;

/// `SCARD_ATTR_*` in `reader.h`, the ones these readers answer.
pub mod attribute {
    pub const VENDOR_NAME: u32 = 0x0001_0100;
    pub const VENDOR_IFD_TYPE: u32 = 0x0001_0101;
    pub const MAX_INPUT: u32 = 0x0007_A007;
    pub const CURRENT_PROTOCOL: u32 = 0x0008_0201;
    pub const ATR_STRING: u32 = 0x0009_0303;
    pub const DEVICE_FRIENDLY_NAME_A: u32 = 0x7FFF_0003;
    pub const DEVICE_SYSTEM_NAME_A: u32 = 0x7FFF_0004;
    pub const DEVICE_FRIENDLY_NAME_W: u32 = 0x7FFF_0005;
    pub const DEVICE_SYSTEM_NAME_W: u32 = 0x7FFF_0006;
}

/// `struct rxHeader`, which precedes every message from a client. A reply
/// carries none: the client knows what it asked for and reads exactly the
/// answer's length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub size: u32,
    pub command: u32,
}

pub fn read_header(source: &mut impl Read) -> io::Result<Header> {
    let mut raw = [0u8; 8];
    source.read_exact(&mut raw)?;
    Ok(Header {
        size: field(&raw, 0),
        command: field(&raw, 1),
    })
}

/// A request body of the size the header announced, refusing one that is not
/// the size of the command's structure. A wrong length is not recoverable --
/// the next bytes would be read as the next header -- so pcscd drops such a
/// client, and so does this.
pub fn body(source: &mut impl Read, announced: u32, expected: usize) -> io::Result<Vec<u8>> {
    if announced as usize != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message body is {announced} bytes, want {expected}"),
        ));
    }
    let mut raw = vec![0u8; expected];
    source.read_exact(&mut raw)?;
    Ok(raw)
}

/// The word at a byte offset.
pub fn word(raw: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes(raw[at..at + 4].try_into().expect("four bytes"))
}

pub fn set_word(raw: &mut [u8], at: usize, value: u32) {
    raw[at..at + 4].copy_from_slice(&value.to_ne_bytes());
}

/// The `index`th word of a structure made only of words.
pub fn field(raw: &[u8], index: usize) -> u32 {
    word(raw, index * 4)
}

pub fn put(raw: &mut [u8], index: usize, value: u32) {
    set_word(raw, index * 4, value);
}

pub fn c_string(raw: &[u8]) -> String {
    let end = raw.iter().position(|&byte| byte == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

/// Writes a C string into a fixed-width field, truncating a name that does not
/// fit and leaving room for the terminator.
pub fn put_c_string(raw: &mut [u8], value: &str) {
    raw.fill(0);
    let bytes = value.as_bytes();
    let length = bytes.len().min(raw.len().saturating_sub(1));
    raw[..length].copy_from_slice(&bytes[..length]);
}

/// `READER_STATE` from `readers.h`, the entry a client reads a reader's whole
/// condition from. The client derives the `SCARD_STATE_*` flags of
/// `SCardGetStatusChange` from these fields itself, so what is published here
/// is the hardware's state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReaderState {
    pub name: String,
    /// Card insertions and removals.
    pub events: u32,
    /// The `STATE_*` bits.
    pub state: u32,
    /// -1 for an exclusive connection, 0 for none, or how many share it.
    pub sharing: i32,
    pub atr: Vec<u8>,
    pub protocol: u32,
}

impl ReaderState {
    pub fn encode(&self) -> [u8; READER_STATE_LEN] {
        let mut raw = [0u8; READER_STATE_LEN];
        put_c_string(&mut raw[0..MAX_READER_NAME], &self.name);
        set_word(&mut raw, 128, self.events);
        set_word(&mut raw, 132, self.state);
        set_word(&mut raw, 136, self.sharing as u32);
        let atr = &self.atr[..self.atr.len().min(MAX_ATR)];
        raw[140..140 + atr.len()].copy_from_slice(atr);
        // 173 to 176 is the padding before the next word, which the C
        // structure has and which must be left alone rather than packed out.
        set_word(&mut raw, 176, atr.len() as u32);
        set_word(&mut raw, 180, self.protocol);
        raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structures_are_pcsc_lites_sizes() {
        for (name, got, want) in [
            ("struct version_struct", SIZE_VERSION, 12),
            ("struct establish_struct", SIZE_ESTABLISH, 12),
            ("struct release_struct", SIZE_RELEASE, 8),
            ("struct connect_struct", SIZE_CONNECT, 152),
            ("struct reconnect_struct", SIZE_RECONNECT, 24),
            ("struct disconnect_struct", SIZE_DISCONNECT, 12),
            ("struct begin_struct", SIZE_BEGIN, 8),
            ("struct end_struct", SIZE_END, 12),
            ("struct cancel_struct", SIZE_CANCEL, 8),
            ("struct status_struct", SIZE_STATUS, 8),
            ("struct transmit_struct", SIZE_TRANSMIT, 32),
            ("struct control_struct", SIZE_CONTROL, 24),
            ("struct getset_struct", SIZE_GET_SET, 280),
            ("struct wait_reader_state_change", SIZE_WAIT, 8),
            ("struct get_reader_events", SIZE_EVENTS, 8),
            ("READER_STATE", READER_STATE_LEN, 184),
        ] {
            assert_eq!(got, want, "{name}");
        }
    }

    #[test]
    fn a_reader_state_is_laid_out_as_the_c_structure() {
        let name = "PLEX PX-W3U4 series Smart Card Reader (ID: 0123456789ABCDEF)";
        let encoded = ReaderState {
            name: name.to_owned(),
            events: 7,
            state: STATE_PRESENT,
            sharing: SHARING_EXCLUSIVE,
            atr: vec![0x3B, 0xF0, 0x12],
            protocol: PROTOCOL_T1,
        }
        .encode();
        assert_eq!(c_string(&encoded[0..MAX_READER_NAME]), name);
        assert_eq!(word(&encoded, 128), 7, "eventCounter");
        assert_eq!(word(&encoded, 132), STATE_PRESENT, "readerState");
        assert_eq!(
            word(&encoded, 136) as i32,
            SHARING_EXCLUSIVE,
            "readerSharing"
        );
        assert_eq!(&encoded[140..143], &[0x3B, 0xF0, 0x12], "cardAtr");
        assert_eq!(word(&encoded, 176), 3, "cardAtrLength");
        assert_eq!(word(&encoded, 180), PROTOCOL_T1, "cardProtocol");
    }

    #[test]
    fn a_long_name_keeps_its_terminator() {
        let mut raw = [0xFFu8; 8];
        put_c_string(&mut raw, "0123456789");
        assert_eq!(&raw, b"0123456\0");
    }
}

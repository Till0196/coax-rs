// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Station logos: the pictures a receiver shows beside a service's name.
//!
//! They come three ways. Terrestrial broadcasting sends them in the CDT
//! ([`crate::tstables::common_data`]); the advanced services in the MH-CDT
//! ([`crate::mmttables::common_data`]); and satellite broadcasting, along
//! with cable retransmission, sends them as *common data for all receivers*
//! (ARIB TR-B15, JCTEA STD-007): a software download announced by the SDTT
//! for maker 0xFF, carried in a DSM-CC data carousel whose modules are
//! named `LOGO-00` .. `LOGO-05` (BS), `CS_LOGO-00` .. (CS) or, on cable,
//! `CATV_LOGO` and the like. This module reads such a module
//! ([`logo_module`]) and knows which download and which module names to
//! look at.
//!
//! Whichever way it came, the PNG has **no palette**: it is an 8-bit
//! indexed picture whose colours are the default CLUT of ARIB STD-B24
//! (Table 5-7 of Volume 2), which every receiver already holds. A PNG
//! decoder wants the palette in the file, so [`with_palette`] puts it
//! there.

use crate::tstables::be16;

/// The maker identifier of the common data download.
pub const COMMON_MAKER: u8 = 0xff;

/// Whether an SDTT for this model, under maker 0xFF, announces the common
/// data that logos are part of.
///
/// 0xFE and 0xFD are BS, 0xFC and 0xFB are CS, 0xFA .. 0xF7 terrestrial,
/// which is what a satellite receiver subscribes to; 0xF1 is the cable
/// remultiplexing download that a cable set-top box takes its logos from.
pub const fn is_common_data_model(model: u8) -> bool {
    matches!(model, 0xf1 | 0xf7..=0xfe)
}

/// Whether a module's `moduleInfoByte`s name it as a logo module: a name
/// descriptor (tag 0x02) whose name has `LOGO` in it. That is `LOGO-0n`,
/// `CS_LOGO-0n`, `CATV_LOGO`, `JCHITS_LOGO`; a cable receiver goes by the
/// substring rather than by a list, and so does this.
pub fn is_logo_module(info: &[u8]) -> bool {
    let mut at = 0;
    while at + 2 <= info.len() {
        let tag = info[at];
        let length = usize::from(info[at + 1]);
        let Some(body) = info.get(at + 2..at + 2 + length) else {
            return false;
        };
        if tag == 0x02 && body.windows(4).any(|w| w == b"LOGO") {
            return true;
        }
        at += 2 + length;
    }
    false
}

/// One logo in a logo module, and the services it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogoEntry {
    pub logo_id: u16,
    /// `(original_network_id, transport_stream_id, service_id)`.
    pub services: Vec<(u16, u16, u16)>,
    /// The PNG, without a palette.
    pub png: Vec<u8>,
}

/// A logo module: every logo of one type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogoModule {
    /// 0 .. 5 (ARIB TR-B14: the sizes), the same numbering as the CDT.
    pub logo_type: u8,
    pub entries: Vec<LogoEntry>,
}

/// Reads a logo module of the common data download: `logo_type` (8),
/// `number_of_loop` (16), then per logo `reserved` (7), `logo_id` (9),
/// `number_of_services` (8), the services at six bytes each, `data_size`
/// (16) and the PNG.
pub fn logo_module(bytes: &[u8]) -> Option<LogoModule> {
    let logo_type = *bytes.first()?;
    let count = usize::from(be16(bytes.get(1..3)?));
    let mut at = 3;
    let mut entries = Vec::with_capacity(count.min(64));
    for _ in 0..count {
        let head = bytes.get(at..at + 3)?;
        let logo_id = be16(head) & 0x01ff;
        let services_count = usize::from(head[2]);
        at += 3;
        let mut services = Vec::with_capacity(services_count);
        for _ in 0..services_count {
            let service = bytes.get(at..at + 6)?;
            services.push((be16(service), be16(&service[2..]), be16(&service[4..])));
            at += 6;
        }
        let size = usize::from(be16(bytes.get(at..at + 2)?));
        at += 2;
        let png = bytes.get(at..at + size)?.to_vec();
        at += size;
        entries.push(LogoEntry {
            logo_id,
            services,
            png,
        });
    }
    Some(LogoModule { logo_type, entries })
}

/// The default CLUT of ARIB STD-B24 Volume 2 Table 5-7: 128 colours as
/// R, G, B, A. Index 8 is transparent; 65 .. 127 are 0 .. 7 and 9 .. 63
/// again at half opacity.
pub const DEFAULT_CLUT: [[u8; 4]; 128] = {
    let opaque: [[u8; 3]; 65] = [
        [0, 0, 0],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [0, 0, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
        [0, 0, 0],
        [170, 0, 0],
        [0, 170, 0],
        [170, 170, 0],
        [0, 0, 170],
        [170, 0, 170],
        [0, 170, 170],
        [170, 170, 170],
        [0, 0, 85],
        [0, 85, 0],
        [0, 85, 85],
        [0, 85, 170],
        [0, 85, 255],
        [0, 170, 85],
        [0, 170, 255],
        [0, 255, 85],
        [0, 255, 170],
        [85, 0, 0],
        [85, 0, 85],
        [85, 0, 170],
        [85, 0, 255],
        [85, 85, 0],
        [85, 85, 85],
        [85, 85, 170],
        [85, 85, 255],
        [85, 170, 0],
        [85, 170, 85],
        [85, 170, 170],
        [85, 170, 255],
        [85, 255, 0],
        [85, 255, 85],
        [85, 255, 170],
        [85, 255, 255],
        [170, 0, 85],
        [170, 0, 255],
        [170, 85, 0],
        [170, 85, 85],
        [170, 85, 170],
        [170, 85, 255],
        [170, 170, 85],
        [170, 170, 255],
        [170, 255, 0],
        [170, 255, 85],
        [170, 255, 170],
        [170, 255, 255],
        [255, 0, 85],
        [255, 0, 170],
        [255, 85, 0],
        [255, 85, 85],
        [255, 85, 170],
        [255, 85, 255],
        [255, 170, 0],
        [255, 170, 85],
        [255, 170, 170],
        [255, 170, 255],
        [255, 255, 85],
        [255, 255, 170],
    ];
    let mut table = [[0u8; 4]; 128];
    let mut i = 0;
    while i < 65 {
        let [r, g, b] = opaque[i];
        table[i] = [r, g, b, if i == 8 { 0 } else { 255 }];
        i += 1;
    }
    // 65 .. 72 are 0 .. 7; 73 .. 127 are 9 .. 63.
    let mut i = 65;
    while i < 128 {
        let from = if i < 73 { i - 65 } else { i - 64 };
        let [r, g, b] = opaque[from];
        table[i] = [r, g, b, 128];
        i += 1;
    }
    table
};

/// Puts the default CLUT into an indexed PNG that came without one: a
/// `PLTE` of 256 entries and a `tRNS` of 256, right after the `IHDR`. A PNG
/// that already has a palette, or is not indexed, comes back unchanged.
pub fn with_palette(png: &[u8]) -> Vec<u8> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    // Signature, then IHDR: length 13, "IHDR", 13 bytes, CRC.
    if png.len() < 33 || &png[..8] != SIGNATURE || &png[12..16] != b"IHDR" {
        return png.to_vec();
    }
    let colour_type = png[25];
    if colour_type != 3 || has_chunk(&png[33..], b"PLTE") {
        return png.to_vec();
    }
    let mut out = Vec::with_capacity(png.len() + 8 + 768 + 4 + 8 + 256 + 4);
    out.extend_from_slice(&png[..33]);
    let mut plte = Vec::with_capacity(768);
    let mut trns = Vec::with_capacity(256);
    for i in 0..256 {
        let [r, g, b, a] = DEFAULT_CLUT.get(i).copied().unwrap_or([0, 0, 0, 0]);
        plte.extend_from_slice(&[r, g, b]);
        trns.push(a);
    }
    chunk(&mut out, b"PLTE", &plte);
    chunk(&mut out, b"tRNS", &trns);
    out.extend_from_slice(&png[33..]);
    out
}

fn has_chunk(mut rest: &[u8], name: &[u8; 4]) -> bool {
    while rest.len() >= 12 {
        let length = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        if &rest[4..8] == name {
            return true;
        }
        if &rest[4..8] == b"IDAT" || &rest[4..8] == b"IEND" {
            return false;
        }
        let Some(next) = length.checked_add(12).and_then(|end| rest.get(end..)) else {
            return false;
        };
        rest = next;
    }
    false
}

fn chunk(out: &mut Vec<u8>, name: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(name);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// The CRC of PNG chunks (ISO 3309, the one zlib uses).
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_modules_are_named_by_substring() {
        assert!(is_logo_module(&[
            0x02, 7, b'L', b'O', b'G', b'O', b'-', b'0', b'2'
        ]));
        assert!(is_logo_module(b"\x02\x0aCS_LOGO-00"));
        assert!(is_logo_module(b"\x02\x09CATV_LOGO"));
        assert!(!is_logo_module(b"\x02\x05GENRE"));
        assert!(!is_logo_module(b"\x01\x04LOGO"));
        assert!(!is_logo_module(b"\x02\x09LOG"));
    }

    #[test]
    fn a_logo_module_is_read() {
        let mut bytes = vec![0x02, 0x00, 0x02];
        // logo_id 0x123 with two services and a three-byte "PNG".
        bytes.extend_from_slice(&[0x01, 0x23, 2]);
        bytes.extend_from_slice(&[0x00, 0x04, 0x40, 0xf1, 0x00, 0x65]);
        bytes.extend_from_slice(&[0x00, 0x04, 0x40, 0xf1, 0x00, 0x66]);
        bytes.extend_from_slice(&[0x00, 0x03, 1, 2, 3]);
        // logo_id 5, no services, an empty picture.
        bytes.extend_from_slice(&[0xfe, 0x05, 0, 0x00, 0x00]);
        let module = logo_module(&bytes).unwrap();
        assert_eq!(module.logo_type, 2);
        assert_eq!(module.entries.len(), 2);
        assert_eq!(module.entries[0].logo_id, 0x123);
        assert_eq!(
            module.entries[0].services,
            vec![(4, 0x40f1, 0x65), (4, 0x40f1, 0x66)]
        );
        assert_eq!(module.entries[0].png, vec![1, 2, 3]);
        assert_eq!(module.entries[1].logo_id, 5);
        assert!(module.entries[1].services.is_empty());
        // Short by a byte: nothing.
        assert!(logo_module(&bytes[..bytes.len() - 1]).is_none());
    }

    #[test]
    fn the_clut_is_table_5_7() {
        assert_eq!(DEFAULT_CLUT[0], [0, 0, 0, 255]);
        assert_eq!(DEFAULT_CLUT[7], [255, 255, 255, 255]);
        assert_eq!(DEFAULT_CLUT[8], [0, 0, 0, 0]);
        assert_eq!(DEFAULT_CLUT[9], [170, 0, 0, 255]);
        assert_eq!(DEFAULT_CLUT[64], [255, 255, 170, 255]);
        assert_eq!(DEFAULT_CLUT[65], [0, 0, 0, 128]);
        assert_eq!(DEFAULT_CLUT[72], [255, 255, 255, 128]);
        assert_eq!(DEFAULT_CLUT[73], [170, 0, 0, 128]);
        assert_eq!(DEFAULT_CLUT[86], [0, 170, 255, 128]);
        assert_eq!(DEFAULT_CLUT[107], [170, 85, 0, 128]);
        assert_eq!(DEFAULT_CLUT[127], [255, 255, 85, 128]);
    }

    fn indexed_png(with_plte: bool) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let ihdr = [0, 0, 0, 2, 0, 0, 0, 1, 8, 3, 0, 0, 0];
        chunk(&mut png, b"IHDR", &ihdr);
        if with_plte {
            chunk(&mut png, b"PLTE", &[1, 2, 3]);
        }
        chunk(
            &mut png,
            b"IDAT",
            &[
                0x78, 0x9c, 0x63, 0x60, 0x60, 0x00, 0x00, 0x00, 0x04, 0x00, 0x01,
            ],
        );
        chunk(&mut png, b"IEND", &[]);
        png
    }

    #[test]
    fn a_palette_is_put_after_the_header() {
        let png = indexed_png(false);
        let out = with_palette(&png);
        assert_eq!(&out[..33], &png[..33]);
        assert_eq!(&out[33..41], b"\x00\x00\x03\x00PLTE");
        assert_eq!(&out[41..44], &[0, 0, 0]);
        assert_eq!(&out[44..47], &[255, 0, 0]);
        let trns = 33 + 8 + 768 + 4;
        assert_eq!(&out[trns..trns + 8], b"\x00\x00\x01\x00tRNS");
        assert_eq!(out[trns + 8 + 8], 0);
        assert_eq!(out[trns + 8 + 9], 255);
        assert_eq!(&out[trns + 8 + 256 + 4..], &png[33..]);
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        // Already palettised, or not indexed: untouched.
        assert_eq!(with_palette(&indexed_png(true)), indexed_png(true));
        let mut rgb = indexed_png(false);
        rgb[25] = 2;
        assert_eq!(with_palette(&rgb), rgb);
    }
}

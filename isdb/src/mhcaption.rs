// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! MMT の字幕・文字スーパー（ARIB STD-B60 9 章）。**MMT の字幕は TTML
//! のドキュメントとして、`stpp` のアセットの MPU に載って届く。**
//!
//! - アセットの MH データ符号化方式記述子（0x8020）の追加情報に、字幕の
//!   言語・時刻制御・解像度などが書かれている（表 9-3）。
//! - MPU は MFU の並びで、最初の MFU が TTML のドキュメント、続く MFU がそれの
//!   参照する外部リソース（フォント、画像、音声）。MFU のヘッダーは表 9-1。
//!
//! 受信機のアプリ（ARIB-HTML5）は `addCaptionListener` で MFU ごとに
//! これを受け取る（TR-B39 第 2 編 8.10.1.1）。ここはヘッダーを読むだけ。

/// MH データ符号化方式記述子。字幕なら `data_component_id` が 0x0020。
pub const DESCRIPTOR_DATA_CODING: u16 = 0x8020;
/// 字幕・文字スーパーの `data_component_id`。
pub const DATA_COMPONENT_CAPTION: u16 = 0x0020;

/// MFU の `data_type`（表 9-2）。
pub const DATA_TTML: u8 = 0b0000;
pub const DATA_PNG: u8 = 0b0001;
pub const DATA_SVG: u8 = 0b0010;
pub const DATA_AIFF: u8 = 0b0011;
pub const DATA_MP3: u8 = 0b0100;
pub const DATA_AAC: u8 = 0b0101;
pub const DATA_FONT_SVG: u8 = 0b0110;
pub const DATA_FONT_WOFF: u8 = 0b0111;

/// 追加 ARIB 字幕情報（表 9-3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleInfo {
    pub tag: u8,
    pub version: u8,
    /// ISO 639-2 の三文字。
    pub language: String,
    /// 種類（表 9-4）: 0 字幕、1 文字スーパー。
    pub kind: u8,
    pub format: u8,
    pub opm: u8,
    /// 時刻制御モード（表 9-7）。0b0010 なら `reference_start` を起点に
    /// TTML の時刻を読む。0b1111 は時刻制御なし。
    pub tmd: u8,
    /// 表示モード（表 9-8）。
    pub dmf: u8,
    /// 表示解像度（表 9-9）: 0 は 1920×1080、1 は 3840×2160、2 は 7680×4320。
    pub resolution: u8,
    /// 圧縮（表 9-10）: 0 なし、1 zlib。TTML のドキュメントだけが対象。
    pub compression: u8,
    pub start_mpu_sequence: Option<u32>,
    /// 起点の時刻（NTP の 64 ビット）。`tmd` が 0b0010 のとき。
    pub reference_start: Option<u64>,
}

/// MH データ符号化方式記述子の本文から、字幕の追加情報を読む。
/// 字幕でなければ（`data_component_id` が違えば）何も返さない。
pub fn subtitle_info(body: &[u8]) -> Option<SubtitleInfo> {
    let id = u16::from(*body.first()?) << 8 | u16::from(*body.get(1)?);
    if id != DATA_COMPONENT_CAPTION {
        return None;
    }
    additional_subtitle_info(body.get(2..)?)
}

/// `additional_data_component_info` そのものを読む（表 9-3）。
pub fn additional_subtitle_info(info: &[u8]) -> Option<SubtitleInfo> {
    let tag = *info.first()?;
    let flags = *info.get(1)?;
    let language = String::from_utf8_lossy(info.get(2..5)?).into_owned();
    let b5 = *info.get(5)?;
    let b6 = *info.get(6)?;
    let b7 = *info.get(7)?;
    let mut at = 8;
    let mut read = SubtitleInfo {
        tag,
        version: flags >> 4,
        language,
        kind: b5 >> 6,
        format: (b5 >> 2) & 0x0f,
        opm: b5 & 0x03,
        tmd: b6 >> 4,
        dmf: b6 & 0x0f,
        resolution: b7 >> 4,
        compression: b7 & 0x0f,
        start_mpu_sequence: None,
        reference_start: None,
    };
    if flags & 0x08 != 0 {
        let bytes = info.get(at..at + 4)?;
        read.start_mpu_sequence = Some(u32::from_be_bytes(bytes.try_into().ok()?));
        at += 4;
    }
    if read.tmd == 0b0010 {
        let bytes = info.get(at..at + 8)?;
        read.reference_start = Some(u64::from_be_bytes(bytes.try_into().ok()?));
    }
    Some(read)
}

/// 字幕の MFU のヘッダー（表 9-1）と本文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mfu<'a> {
    pub tag: u8,
    pub sequence: u8,
    pub subsample: u8,
    pub last_subsample: u8,
    /// `DATA_*`。
    pub data_type: u8,
    /// ヘッダーが示す本文のサイズ。届いた本文と一致しないこともある。
    pub size: u32,
    pub data: &'a [u8],
}

/// 字幕の MFU を読む。
pub fn mfu(bytes: &[u8]) -> Option<Mfu<'_>> {
    let tag = *bytes.first()?;
    let sequence = *bytes.get(1)?;
    let subsample = *bytes.get(2)?;
    let last_subsample = *bytes.get(3)?;
    let b4 = *bytes.get(4)?;
    let data_type = b4 >> 4;
    let long = b4 & 0x08 != 0;
    let listed = b4 & 0x04 != 0;
    let mut at = 5;
    let size = if long {
        let v = u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?);
        at += 4;
        v
    } else {
        let v = u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?);
        at += 2;
        u32::from(v)
    };
    // 最初の MFU は、続く MFU の種類とサイズの一覧を持つことがある。
    if subsample == 0 && last_subsample > 0 && listed {
        at += usize::from(last_subsample) * if long { 5 } else { 3 };
    }
    Some(Mfu {
        tag,
        sequence,
        subsample,
        last_subsample,
        data_type,
        size,
        data: bytes.get(at..)?,
    })
}

/// 展開した TTML のドキュメントのサイズの上限。
const MOST_TTML: usize = 4 * 1024 * 1024;

/// TTML のドキュメントの内容。圧縮（追加情報の `compression` が 1、zlib）なら
/// デコードして。ドキュメントでなければ（画像や音声なら）None。
pub fn ttml_text(mfu: &Mfu<'_>, compressed: bool) -> Option<String> {
    if mfu.data_type != DATA_TTML {
        return None;
    }
    let body = if compressed {
        // 字幕のドキュメントはせいぜい数十キロバイト。数バイトが何ギガにも膨らむものは
        // ドキュメントではない。
        miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(mfu.data, MOST_TTML).ok()?
    } else {
        mfu.data.to_vec()
    };
    String::from_utf8(body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitle_info_is_read() {
        // 字幕：時刻制御 0b0010 なので起点の時刻（NTP）が続く。
        let mut body = vec![0x00, 0x20, 0x30, 0x07];
        body.extend_from_slice(b"jpn");
        body.extend_from_slice(&[0x01, 0x2a, 0x10]);
        body.extend_from_slice(&0xe000_0000_8000_0000u64.to_be_bytes());
        let info = subtitle_info(&body).expect("字幕");
        assert_eq!(info.tag, 0x30);
        assert_eq!(info.language, "jpn");
        assert_eq!(info.kind, 0);
        assert_eq!(info.opm, 1);
        assert_eq!(info.tmd, 0b0010);
        assert_eq!(info.dmf, 0b1010);
        assert_eq!(info.resolution, 1);
        assert_eq!(info.compression, 0);
        assert_eq!(info.start_mpu_sequence, None);
        assert_eq!(info.reference_start, Some(0xe000_0000_8000_0000));
        // 文字スーパー: 時刻制御なし、起点の時刻は無い。
        let body = [0x00, 0x20, 0x38, 0x07, 0x6a, 0x70, 0x6e, 0x41, 0xf2, 0x10];
        let info = subtitle_info(&body).expect("文字スーパー");
        assert_eq!(info.kind, 1);
        assert_eq!(info.tmd, 0b1111);
        assert_eq!(info.reference_start, None);
        assert!(subtitle_info(&[0x00, 0x21, 0x30]).is_none());
    }

    #[test]
    fn a_ttml_mfu_is_read() {
        let mut bytes = vec![0x30, 0xa9, 0x00, 0x00, 0x03, 0x05, 0x63];
        bytes.extend_from_slice(b"<tt/>");
        let read = mfu(&bytes).expect("読める");
        assert_eq!(read.tag, 0x30);
        assert_eq!(read.sequence, 0xa9);
        assert_eq!(read.data_type, DATA_TTML);
        assert_eq!(read.size, 0x0563);
        assert_eq!(read.data, b"<tt/>");
        // 長いサイズと、続く MFU の一覧を持つ最初の MFU。
        let bytes = [
            0x30, 0x01, 0x00, 0x02, 0x0c, 0x00, 0x00, 0x00, 0x03, 0x60, 0x00, 0x00, 0x00, 0x10,
            0x10, 0x00, 0x00, 0x00, 0x20, b'a', b'b', b'c',
        ];
        let read = mfu(&bytes).expect("読める");
        assert_eq!(read.size, 3);
        assert_eq!(read.last_subsample, 2);
        assert_eq!(read.data, b"abc");
    }
}

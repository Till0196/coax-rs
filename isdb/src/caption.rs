// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! 字幕と文字スーパー（ARIB STD-B24）。
//!
//! PES の私用データ（`data_identifier` 0x80 が字幕、0x81 が文字スーパー）
//! に**データグループ**が乗る。管理データ（言語と表示書式）と文データ
//! （本文と DRCS）があり、本文は第二編の八単位符号で、文字の間に位置・
//! サイズ・色の制御符号が混じる。ここでは、
//!
//! - [`pes`]：PES のペイロード → [`Group`]（管理か文か、言語、時刻、データユニット）
//! - [`Presenter`]：管理データで画面を設定し、文データを**画面に配置する文字の
//!   並び**（[`Layout`]）にする。位置は画面のピクセルで、レンダリング側は拡大縮小するだけ。
//!
//! 文字スーパーは時刻制御なしで届いた端から出し、消去符号か三分で消す。

use crate::arib::{Piece, pieces, pieces_c_profile};

/// RPT で繰り返す字数の上限。最も広い画面に最も小さい文字を並べた数。
const MOST_REPEAT: u32 = 1920;

/// 字幕か文字スーパーか（PES の `data_identifier`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Caption,
    Superimpose,
}

/// 一つのデータグループ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub kind: Kind,
    /// `data_group_id`（0x0/0x20 管理、0x1〜0x8/0x21〜0x28 文。0x20 の
    /// ビットは B 組）。
    pub id: u8,
    pub body: Body,
}

impl Group {
    /// 管理データか。
    pub fn is_management(&self) -> bool {
        self.id & 0x0f == 0
    }
    /// 文データなら、その言語の番号（1〜8）。
    pub fn language(&self) -> Option<u8> {
        match self.id & 0x0f {
            0 => None,
            n => Some(n),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Management(Management),
    Statement(Statement),
}

/// 字幕管理データ。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Management {
    /// 時刻制御モード（0 自由、1 リアルタイム、2 オフセット）。
    pub tmd: u8,
    pub languages: Vec<Language>,
    pub units: Vec<Unit>,
}

/// 管理データの言語ごとのエントリー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Language {
    /// 言語の番号（0〜7、文データの `data_group_id` の下位 − 1）。
    pub tag: u8,
    /// 受信時の表示（DMF）。上位二ビットが受信時、下位が録画再生時。
    pub dmf: u8,
    pub iso: [u8; 3],
    /// 表示書式（表 8-4）: 0 = 1920×1080 横、1 = 縦、7 = 960×540 横、
    /// 8 = 縦、9 = 720×480 横、10 = 縦、11 = 720×480 横…など。
    pub format: u8,
    /// 文字符号化（0 = 八単位符号）。
    pub tcs: u8,
    pub rollup: u8,
}

/// 字幕文データ。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Statement {
    pub tmd: u8,
    /// 提示の時刻（STM、ミリ秒）。TMD が 1 か 2 のときだけ。
    pub stm: Option<u64>,
    pub units: Vec<Unit>,
}

/// データユニット。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unit {
    /// 本文（八単位符号）。
    Text(Vec<u8>),
    /// DRCS の字形（表 D-1 の並びのまま）。`two_byte` は 0x31（二バイト
    /// 符号の DRCS-0）か、0x30（一バイト符号の DRCS-1〜15）か。
    Drcs { two_byte: bool, data: Vec<u8> },
    /// ほか（ビットマップ、ジオメトリック…）。
    Other(u8, Vec<u8>),
}

/// PES のペイロード（PES ヘッダーを除いた、`data_identifier` から）を読む。
/// 一つの PES に一つのデータグループ。CRC は見ない。
pub fn pes(payload: &[u8]) -> Option<Group> {
    let kind = match *payload.first()? {
        0x80 => Kind::Caption,
        0x81 => Kind::Superimpose,
        _ => return None,
    };
    if *payload.get(1)? != 0xff {
        return None;
    }
    let header = usize::from(payload.get(2)? & 0x0f);
    let group = payload.get(3 + header..)?;
    let id = group.first()? >> 2;
    let size = usize::from(u16::from_be_bytes([*group.get(3)?, *group.get(4)?]));
    let data = group.get(5..5 + size)?;
    let body = if id & 0x0f == 0 {
        Body::Management(management(data)?)
    } else {
        Body::Statement(statement(data)?)
    };
    Some(Group { kind, id, body })
}

fn management(data: &[u8]) -> Option<Management> {
    let tmd = data.first()? >> 6;
    let mut at = 1;
    if tmd == 2 {
        // OTM（オフセット時刻）五バイト。
        at += 5;
    }
    let count = usize::from(*data.get(at)?);
    at += 1;
    let mut languages = Vec::with_capacity(count);
    for _ in 0..count {
        let first = *data.get(at)?;
        let tag = first >> 5;
        let dmf = first & 0x0f;
        at += 1;
        if dmf == 0b1100 || dmf == 0b1101 || dmf == 0b1110 {
            // DC（表示条件指定）一バイト。
            at += 1;
        }
        let iso = [*data.get(at)?, *data.get(at + 1)?, *data.get(at + 2)?];
        at += 3;
        let last = *data.get(at)?;
        at += 1;
        languages.push(Language {
            tag,
            dmf,
            iso,
            format: last >> 4,
            tcs: (last >> 2) & 3,
            rollup: last & 3,
        });
    }
    let (units, _) = units(data, at)?;
    Some(Management {
        tmd,
        languages,
        units,
    })
}

fn statement(data: &[u8]) -> Option<Statement> {
    let tmd = data.first()? >> 6;
    let mut at = 1;
    let mut stm = None;
    if tmd == 1 || tmd == 2 {
        let t = data.get(at..at + 5)?;
        at += 5;
        // HHMMSS.mmm の BCD。
        let bcd = |b: u8| u64::from(b >> 4) * 10 + u64::from(b & 0x0f);
        let hours = bcd(t[0]);
        let minutes = bcd(t[1]);
        let seconds = bcd(t[2]);
        let millis = bcd(t[3]) * 10 + u64::from(t[4] >> 4);
        stm = Some(((hours * 60 + minutes) * 60 + seconds) * 1000 + millis);
    }
    let (units, _) = units(data, at)?;
    Some(Statement { tmd, stm, units })
}

/// データユニットの並び（三バイトの長さの後に）。
fn units(data: &[u8], at: usize) -> Option<(Vec<Unit>, usize)> {
    let total = data.get(at..at + 3)?;
    let total = usize::from(total[0]) << 16 | usize::from(total[1]) << 8 | usize::from(total[2]);
    let mut at = at + 3;
    let end = (at + total).min(data.len());
    let mut out = Vec::new();
    while at + 5 <= end {
        if data[at] != 0x1f {
            break;
        }
        let parameter = data[at + 1];
        let size = usize::from(data[at + 2]) << 16
            | usize::from(data[at + 3]) << 8
            | usize::from(data[at + 4]);
        let body = data.get(at + 5..(at + 5 + size).min(end))?;
        out.push(match parameter {
            0x20 => Unit::Text(body.to_vec()),
            0x30 | 0x31 => Unit::Drcs {
                two_byte: parameter == 0x31,
                data: body.to_vec(),
            },
            other => Unit::Other(other, body.to_vec()),
        });
        at += 5 + size;
    }
    Some((out, at))
}

// ---- 画面への配置 ----

/// 色（表 8-8 の CLUT の既定）。`a` は 0 で透明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    /// CLUT の番号（0〜127）から（表 8-8 の既定。値は受信機のもの）。
    pub fn indexed(index: u8) -> Color {
        let (r, g, b, a) = CLUT[usize::from(index & 0x7f)];
        Color { r, g, b, a }
    }
}

/// CLUT の既定（ARIB STD-B24 第三編 表 8-8）。0〜7 が基本色、8 が透明、
/// 9〜15 が半輝度、16〜64 が 85 刻みの組み合わせ、65〜127 がその半透明
/// （0 番から順に）。
const CLUT: [(u8, u8, u8, u8); 128] = [
    (0, 0, 0, 255),
    (255, 0, 0, 255),
    (0, 255, 0, 255),
    (255, 255, 0, 255),
    (0, 0, 255, 255),
    (255, 0, 255, 255),
    (0, 255, 255, 255),
    (255, 255, 255, 255),
    (0, 0, 0, 0),
    (170, 0, 0, 255),
    (0, 170, 0, 255),
    (170, 170, 0, 255),
    (0, 0, 170, 255),
    (170, 0, 170, 255),
    (0, 170, 170, 255),
    (170, 170, 170, 255),
    (0, 0, 85, 255),
    (0, 85, 0, 255),
    (0, 85, 85, 255),
    (0, 85, 170, 255),
    (0, 85, 255, 255),
    (0, 170, 85, 255),
    (0, 170, 255, 255),
    (0, 255, 85, 255),
    (0, 255, 170, 255),
    (85, 0, 0, 255),
    (85, 0, 85, 255),
    (85, 0, 170, 255),
    (85, 0, 255, 255),
    (85, 85, 0, 255),
    (85, 85, 85, 255),
    (85, 85, 170, 255),
    (85, 85, 255, 255),
    (85, 170, 0, 255),
    (85, 170, 85, 255),
    (85, 170, 170, 255),
    (85, 170, 255, 255),
    (85, 255, 0, 255),
    (85, 255, 85, 255),
    (85, 255, 170, 255),
    (85, 255, 255, 255),
    (170, 0, 85, 255),
    (170, 0, 255, 255),
    (170, 85, 0, 255),
    (170, 85, 85, 255),
    (170, 85, 170, 255),
    (170, 85, 255, 255),
    (170, 170, 85, 255),
    (170, 170, 255, 255),
    (170, 255, 0, 255),
    (170, 255, 85, 255),
    (170, 255, 170, 255),
    (170, 255, 255, 255),
    (255, 0, 85, 255),
    (255, 0, 170, 255),
    (255, 85, 0, 255),
    (255, 85, 85, 255),
    (255, 85, 170, 255),
    (255, 85, 255, 255),
    (255, 170, 0, 255),
    (255, 170, 85, 255),
    (255, 170, 170, 255),
    (255, 170, 255, 255),
    (255, 255, 85, 255),
    (255, 255, 170, 255),
    (0, 0, 0, 128),
    (255, 0, 0, 128),
    (0, 255, 0, 128),
    (255, 255, 0, 128),
    (0, 0, 255, 128),
    (255, 0, 255, 128),
    (0, 255, 255, 128),
    (255, 255, 255, 128),
    (170, 0, 0, 128),
    (0, 170, 0, 128),
    (170, 170, 0, 128),
    (0, 0, 170, 128),
    (170, 0, 170, 128),
    (0, 170, 170, 128),
    (170, 170, 170, 128),
    (0, 0, 85, 128),
    (0, 85, 0, 128),
    (0, 85, 85, 128),
    (0, 85, 170, 128),
    (0, 85, 255, 128),
    (0, 170, 85, 128),
    (0, 170, 255, 128),
    (0, 255, 85, 128),
    (0, 255, 170, 128),
    (85, 0, 0, 128),
    (85, 0, 85, 128),
    (85, 0, 170, 128),
    (85, 0, 255, 128),
    (85, 85, 0, 128),
    (85, 85, 85, 128),
    (85, 85, 170, 128),
    (85, 85, 255, 128),
    (85, 170, 0, 128),
    (85, 170, 85, 128),
    (85, 170, 170, 128),
    (85, 170, 255, 128),
    (85, 255, 0, 128),
    (85, 255, 85, 128),
    (85, 255, 170, 128),
    (85, 255, 255, 128),
    (170, 0, 85, 128),
    (170, 0, 255, 128),
    (170, 85, 0, 128),
    (170, 85, 85, 128),
    (170, 85, 170, 128),
    (170, 85, 255, 128),
    (170, 170, 85, 128),
    (170, 170, 255, 128),
    (170, 255, 0, 128),
    (170, 255, 85, 128),
    (170, 255, 170, 128),
    (170, 255, 255, 128),
    (255, 0, 85, 128),
    (255, 0, 170, 128),
    (255, 85, 0, 128),
    (255, 85, 85, 128),
    (255, 85, 170, 128),
    (255, 85, 255, 128),
    (255, 170, 0, 128),
    (255, 170, 85, 128),
    (255, 170, 170, 128),
    (255, 170, 255, 128),
    (255, 255, 85, 128),
];

/// DRCS（放送局が送ってくる字形）の一つ。文字は私用領域の文字（[`drcs_char`]）に
/// 割り当てて [`Text::text`] に入れ、レンダリング側が [`Layout::drcs`] から参照する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrcsGlyph {
    pub width: u8,
    pub height: u8,
    /// 階調の数（depth + 2）。
    pub levels: u8,
    /// ピクセルの階調、一行ずつ。0 は透明、levels−1 が文字色。
    pub bitmap: Vec<u8>,
}

/// DRCS の文字を割り当てる私用領域の文字。二バイト符号（DRCS-0）は区点で、
/// 一バイト符号（DRCS-1〜15）は集合と符号で。
pub fn drcs_char(set: u8, code: u16, width: usize) -> char {
    let index = if width == 2 || set == 0 {
        let row = u32::from((code >> 8) as u8).saturating_sub(0x21);
        let cell = u32::from(code as u8).saturating_sub(0x21);
        row * 94 + cell
    } else {
        0x3000 + u32::from(set.min(15)) * 0x60 + u32::from(code as u8).saturating_sub(0x21)
    };
    char::from_u32(0xf0000 + index).unwrap_or('〓')
}

/// 私用領域の文字が DRCS か。
pub fn is_drcs(letter: char) -> bool {
    (0xf0000..0xf4000).contains(&u32::from(letter))
}

/// DRCS のデータユニット（表 D-1）を読む。文字ごとに字形は複数来る
/// （フォント・サイズ）ので、全部返す。
pub fn drcs(two_byte: bool, bytes: &[u8]) -> Vec<(char, DrcsGlyph)> {
    let mut out = Vec::new();
    let mut at = 0usize;
    let Some(&count) = bytes.first() else {
        return out;
    };
    at += 1;
    for _ in 0..count {
        let Some(&[first, second, fonts]) = bytes.get(at..at + 3) else {
            break;
        };
        at += 3;
        let letter = if two_byte {
            drcs_char(0, u16::from(first) << 8 | u16::from(second), 2)
        } else {
            drcs_char(first & 0x0f, u16::from(second), 1)
        };
        for _ in 0..fonts {
            let Some(&[mode, depth, width, height]) = bytes.get(at..at + 4) else {
                return out;
            };
            at += 4;
            if mode & 0x0f > 1 {
                // 幾何は運用されない。
                return out;
            }
            let levels = u16::from(depth) + 2;
            let bits = (16 - (levels - 1).leading_zeros()) as usize;
            let pixels = usize::from(width) * usize::from(height);
            let length = (pixels * bits).div_ceil(8);
            let Some(data) = bytes.get(at..at + length) else {
                return out;
            };
            at += length;
            let mut bitmap = Vec::with_capacity(pixels);
            let mut bit = 0usize;
            for _ in 0..pixels {
                let mut value = 0u8;
                for _ in 0..bits {
                    let byte = data[bit >> 3];
                    value = value << 1 | (byte >> (7 - (bit & 7))) & 1;
                    bit += 1;
                }
                bitmap.push(value);
            }
            out.push((
                letter,
                DrcsGlyph {
                    width,
                    height,
                    levels: levels as u8,
                    bitmap,
                },
            ));
        }
    }
    out
}

/// 画面に配置した連続する文字列。位置とサイズは画面のピクセル（[`Layout::screen`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub x: i32,
    pub y: i32,
    /// 一文字の文字枠（横、縦）。半角なら横が半分。
    pub cell: (u32, u32),
    /// 文字サイズ（文字枠から文字間・行間を引いたもの）。
    pub size: (u32, u32),
    pub text: String,
    pub fg: Color,
    pub bg: Color,
    /// 縁取りの色（ORN、TTML の textOutline）。無ければ None。
    pub outline: Option<Color>,
    /// 点滅（FLC）：0 なし、1 正相、2 逆相。
    pub flash: u8,
    /// 囲み線（HLC）：ビットで 1 右、2 下、4 左、8 上。
    pub enclosure: u8,
    /// 表示までの待ち時間（TIME、ミリ秒）。文の始まりから。
    pub delay_ms: u32,
    /// 消す時刻（TIME のあとの CS、ミリ秒）。文の始まりから。無ければ
    /// 次の文まで。
    pub until_ms: Option<u32>,
}

impl Text {
    /// 装飾なしの既定。
    pub fn plain(
        x: i32,
        y: i32,
        cell: (u32, u32),
        size: (u32, u32),
        text: String,
        fg: Color,
        bg: Color,
    ) -> Self {
        Self {
            x,
            y,
            cell,
            size,
            text,
            fg,
            bg,
            outline: None,
            flash: 0,
            enclosure: 0,
            delay_ms: 0,
            until_ms: None,
        }
    }
}

/// アニメーション（TTML の keyframe による座標の変化、字幕の SCR）。文の始まりを
/// 0 として、`delay_ms` のあと `duration_ms` かけて `from` から `to` へ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Motion {
    pub from: (i32, i32),
    pub to: (i32, i32),
    pub delay_ms: u32,
    pub duration_ms: u32,
    /// 滑らかにではなく、終わりで一度に（`step-end`）。
    pub step_end: bool,
}

/// 一つの文データを配置した結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// 画面のサイズ（SWF）。
    pub screen: (u32, u32),
    /// 表示領域（SDF、SDP）。ここを背景色で塗る受信機もある。
    pub area: (i32, i32, u32, u32),
    /// 先に画面を消去してから配置する（CS があった）。
    pub clear: bool,
    /// TIME で待ってから消す（`… TIME CS` の文、ミリ秒）。それまで
    /// `texts` は表示しておく。
    pub clear_after_ms: Option<u32>,
    pub texts: Vec<Text>,
    /// 文字スーパーなら真。
    pub superimpose: bool,
    /// この文で使う DRCS の字形（[`drcs_char`] の文字 → 字形の候補）。
    pub drcs: std::collections::BTreeMap<char, Vec<DrcsGlyph>>,
    /// ラスタ色（RCS）：表示領域を塗る色。透明なら塗らない。
    pub raster: Color,
    /// 鳴らす内蔵音（PRA、1〜13）。緊急地震速報の報知音はこれで来る。
    pub sound: Option<u8>,
    /// 全体のアニメーション（スクロール）。
    pub motion: Option<Motion>,
}

/// 表示領域の位置とサイズ、文字サイズ、字間、行間。
type Setup = ((i32, i32, u32, u32), (u32, u32), u32, u32);

/// 管理データで設定し、文データを配置する。言語一つ分。
#[derive(Debug, Clone)]
pub struct Presenter {
    pub kind: Kind,
    /// どの言語のレイアウトか（1 が第 1 言語、2 が第 2 言語）。管理データの
    /// その言語のエントリーで設定する。
    pub language: u8,
    screen: (u32, u32),
    /// 表示領域の位置とサイズ（SDP、SDF）。
    area: (i32, i32, u32, u32),
    /// 文字サイズ（SSM）、文字間（SHS）、行間（SVS）。
    ssm: (u32, u32),
    shs: u32,
    svs: u32,
    /// CS で戻す設定（表示領域、文字サイズ、字間、行間）。管理データと
    /// SWF で決まり、管理データの中の SDF/SDP/SSM/SHS/SVS でも変わる。
    init: Setup,
    /// 管理データの文字列を読んでいる間は真。
    in_management: bool,
    /// CS（SWF）のあと、作用位置が移動したか文字を配置したか。移動したあとの
    /// SWF・SDF・SDP・RCS は受信機が無視する。
    moved: bool,
    /// サイズの指定（SZX）: 1 標準、2 中（半角）、3 小、4 縦倍、5 横倍、6 縦横倍。
    size_mode: u8,
    fg: Color,
    bg: Color,
    outline: Option<Color>,
    /// 色の組（COL のパレット指定、0〜7）。番号に 16 倍を足す。
    palette: u8,
    /// 極性が反転している（POL）。
    inverted: bool,
    /// 作用位置（ピクセル）。`x` は文字の左、`y` は文字の**下**（規格の作用位置は
    /// 文字の左下。上端は配置するときの文字の高さで決まる。
    x: i32,
    y: i32,
    /// ACPS が表示領域の外を指した。
    hidden: bool,
    /// 最後に配置した文字の文字枠の高さ。APD/APR は現在の文字サイズではなく
    /// **これ**で下がる。行末で SSZ/MSZ に変えてから改行しても行が重ならない。
    last_height: Option<u32>,
    /// 縦書きか。
    vertical: bool,
    /// 届いた DRCS の字形。文をまたいで使われるので保持しておく。
    drcs: std::collections::BTreeMap<char, Vec<DrcsGlyph>>,
    /// ラスタ色（RCS）。
    raster: Color,
    /// RPC：次の文字を何回繰り返すか（0 は行末まで）。
    repeat: Option<u8>,
    /// FLC の点滅、HLC の囲み線、TIME で累積した待ち時間（ミリ秒）。
    flash: u8,
    enclosure: u8,
    delay_ms: u32,
    /// ワンセグ（C プロファイル）の字幕か。**文字集合が固定**（GL が
    /// DRCS、GR が漢字。TR-B14 第三分冊 6.4.1）で、表示領域は受信機の
    /// もの（16 字 × 3 行。6.3.1）。
    pub c_profile: bool,
}

impl Presenter {
    pub fn new(kind: Kind) -> Self {
        Self::for_language(kind, 1)
    }

    /// `language` 番目（1 から）の言語のレイアウト。
    pub fn for_language(kind: Kind, language: u8) -> Self {
        let mut p = Self {
            kind,
            language: language.max(1),
            screen: (960, 540),
            area: (0, 0, 960, 540),
            ssm: (36, 36),
            shs: 4,
            svs: 24,
            init: ((0, 0, 960, 540), (36, 36), 4, 24),
            in_management: false,
            moved: false,
            size_mode: 1,
            fg: Color::WHITE,
            bg: Color::TRANSPARENT,
            outline: None,
            palette: 0,
            inverted: false,
            x: 0,
            y: 0,
            hidden: false,
            last_height: None,
            vertical: false,
            drcs: Default::default(),
            raster: Color::TRANSPARENT,
            repeat: None,
            flash: 0,
            enclosure: 0,
            delay_ms: 0,
            c_profile: false,
        };
        p.reset((960, 540), false);
        p
    }

    /// ワンセグ（C プロファイル）の字幕のレイアウト。
    ///
    /// 放送は横 16 字 × 3 行（か縦 12 字 × 4 行）の領域に収まる文を作り、
    /// 改行は APR で、右端では受信機が折り返す（TR-B14 第三分冊 6.3.1）。
    /// 960×540 の画面に、その 16 字 × 3 行を**画面下部**に配置する：文字は
    /// 標準の 36 ピクセルで、一行 16 字 × 40 = 640 ピクセル、左右に余白。
    pub fn for_one_seg(kind: Kind) -> Self {
        let mut p = Self::new(kind);
        p.set_one_seg();
        p
    }

    /// ワンセグのレイアウトに切り替える（[`Presenter::for_one_seg`]）。
    pub fn set_one_seg(&mut self) {
        self.c_profile = true;
        self.reset((960, 540), false);
    }

    /// 管理データ（言語のエントリーの `format`、表 8-4）で画面を設定する。
    pub fn manage(&mut self, management: &Management) {
        self.take_drcs(&management.units);
        let language = management
            .languages
            .iter()
            .find(|l| l.tag + 1 == self.language)
            .or_else(|| management.languages.first());
        if let Some(language) = language {
            // 8 が 960×540 横、9 が縦、10 が 720×480 横、11 が縦、
            // ほかは 960×540 横。
            let (screen, vertical) = match language.format {
                9 => ((960, 540), true),
                10 => ((720, 480), false),
                11 => ((720, 480), true),
                _ => ((960, 540), false),
            };
            self.reset(screen, vertical);
        }
        // 管理データの中の制御符号は設定を決める（レンダリングするものは無い）。
        let mut scratch = self.layout();
        self.in_management = true;
        for unit in &management.units {
            if let Unit::Text(bytes) = unit {
                self.text(bytes, &mut scratch);
            }
        }
        self.in_management = false;
    }

    /// 空のレイアウト。
    fn layout(&self) -> Layout {
        Layout {
            screen: self.screen,
            area: self.area,
            clear: false,
            clear_after_ms: None,
            texts: Vec::new(),
            superimpose: self.kind == Kind::Superimpose,
            drcs: Default::default(),
            raster: self.raster,
            sound: None,
            motion: None,
        }
    }

    /// 画面のサイズと向きから、既定（表 8-2、8-3）。
    fn reset(&mut self, screen: (u32, u32), vertical: bool) {
        let (ssm, shs, svs) = match (screen, vertical) {
            ((720, 480), false) => ((36, 36), 4, 16),
            ((720, 480), true) => ((36, 36), 8, 24),
            (_, false) => ((36, 36), 4, 24),
            (_, true) => ((36, 36), 12, 24),
        };
        self.screen = screen;
        self.area = (0, 0, screen.0, screen.1);
        if self.c_profile {
            // 16 字 × 3 行を、画面の下部に。行の高さ 60 × 3 = 180。
            self.area = (160, 540 - 180, 640, 180);
        }
        self.ssm = ssm;
        self.shs = shs;
        self.svs = svs;
        self.init = (self.area, ssm, shs, svs);
        self.moved = false;
        self.vertical = vertical;
        self.size_mode = 1;
        self.home();
    }

    /// 作用位置を表示領域の一行目の先頭へ。
    fn home(&mut self) {
        self.x = self.area.0;
        self.y = self.area.1 + self.cell().1 as i32;
        self.hidden = false;
        self.last_height = None;
    }

    /// 一文字の文字枠（横、縦）。サイズの指定を含む。
    fn cell(&self) -> (u32, u32) {
        let (w, h) = (self.ssm.0 + self.shs, self.ssm.1 + self.svs);
        match self.size_mode {
            2 => (w / 2, h),
            3 => (w / 2, h / 2),
            4 => (w, h * 2),
            5 => (w * 2, h),
            6 => (w * 2, h * 2),
            _ => (w, h),
        }
    }

    fn glyph(&self) -> (u32, u32) {
        let (w, h) = self.ssm;
        match self.size_mode {
            2 => (w / 2, h),
            3 => (w / 2, h / 2),
            4 => (w, h * 2),
            5 => (w * 2, h),
            6 => (w * 2, h * 2),
            _ => (w, h),
        }
    }

    /// 作用位置のピクセル（文字の左上）。
    fn position(&self) -> (i32, i32) {
        (self.x, self.y - self.cell().1 as i32)
    }

    fn color(&self, n: u8) -> Color {
        Color::indexed(self.palette * 16 + n)
    }

    /// 前景色を設定する。極性が反転していれば背景へ。
    fn set_fg(&mut self, c: Color) {
        if self.inverted {
            self.bg = c;
        } else {
            self.fg = c;
        }
    }

    fn set_bg(&mut self, c: Color) {
        if self.inverted {
            self.fg = c;
        } else {
            self.bg = c;
        }
    }

    /// 文データを配置する。`clear` は CS が来たら真。
    pub fn present(&mut self, statement: &Statement) -> Layout {
        let mut layout = self.layout();
        self.delay_ms = 0;
        // 字形は本文より先に来るとは限らない（同じ文の中なら順は問わない）。
        self.take_drcs(&statement.units);
        for unit in &statement.units {
            if let Unit::Text(bytes) = unit {
                self.text(bytes, &mut layout);
            }
        }
        // 使った DRCS の字形だけを添える。
        for text in &layout.texts {
            for letter in text.text.chars().filter(|c| is_drcs(*c)) {
                if let Some(glyphs) = self.drcs.get(&letter) {
                    layout.drcs.entry(letter).or_insert_with(|| glyphs.clone());
                }
            }
        }
        layout
    }

    /// データユニットの DRCS を保持する。同じ文字の同じサイズが来たら差し替え。
    fn take_drcs(&mut self, units: &[Unit]) {
        for unit in units {
            if let Unit::Drcs { two_byte, data } = unit {
                for (letter, glyph) in drcs(*two_byte, data) {
                    let had = self.drcs.entry(letter).or_default();
                    had.retain(|g| (g.width, g.height) != (glyph.width, glyph.height));
                    had.push(glyph);
                }
            }
        }
    }

    fn text(&mut self, bytes: &[u8], layout: &mut Layout) {
        let mut run: Option<Text> = None;
        let flush = |run: &mut Option<Text>, layout: &mut Layout| {
            if let Some(text) = run.take()
                && !text.text.is_empty()
            {
                layout.texts.push(text);
            }
        };
        let pieces = if self.c_profile {
            pieces_c_profile(bytes)
        } else {
            pieces(bytes)
        };
        for piece in pieces {
            match piece {
                Piece::Char(letter, width) => {
                    self.put(&mut run, layout, letter, width);
                }
                Piece::Space => {
                    self.put(&mut run, layout, ' ', 2);
                }
                Piece::Drcs { set, code, width } => {
                    let letter = drcs_char(set, code, width);
                    self.put(&mut run, layout, letter, width);
                }
                Piece::Lost => {}
                Piece::Control(code) => {
                    flush(&mut run, layout);
                    self.control(code, layout);
                }
            }
        }
        flush(&mut run, layout);
    }

    /// 一文字配置して、作用位置を進める。
    fn put(&mut self, run: &mut Option<Text>, layout: &mut Layout, letter: char, width: usize) {
        if self.hidden {
            return;
        }
        let cell = self.cell();
        let glyph = self.glyph();
        // 一バイト系（半角）の文字は、標準サイズなら全角の文字枠に配置する
        // （表 7-14: 英数字も一枠）。中（MSZ）なら半分。
        let _ = width;
        let (x, y) = self.position();
        let same = run.as_ref().is_some_and(|r| {
            r.y == y
                && r.cell == cell
                && r.fg == self.fg
                && r.bg == self.bg
                && r.outline == self.outline
                && r.flash == self.flash
                && r.enclosure == self.enclosure
                && r.delay_ms == self.delay_ms
                && r.x + (r.text.chars().count() as i32) * cell.0 as i32 == x
        });
        if !same {
            if let Some(text) = run.take()
                && !text.text.is_empty()
            {
                layout.texts.push(text);
            }
            let mut text = Text::plain(x, y, cell, glyph, String::new(), self.fg, self.bg);
            text.outline = self.outline;
            text.flash = self.flash;
            text.enclosure = self.enclosure;
            text.delay_ms = self.delay_ms;
            *run = Some(text);
        }
        let times = match self.repeat.take() {
            None => 1,
            Some(0) => {
                // 行末まで。
                let (cw, _) = self.cell();
                let left = (self.area.0 + self.area.2 as i32 - self.x).max(0) as u32;
                (left / cw.max(1)).max(1)
            }
            Some(n) => u32::from(n),
        };
        // 表示領域は CSI でいくらでも広く指定できるので、繰り返しは画面の幅の
        // 字数まで。
        let times = times.min(MOST_REPEAT);
        if let Some(text) = run.as_mut() {
            for _ in 0..times {
                text.text.push(letter);
            }
        }
        self.advance(times);
        self.moved = true;
        self.last_height = Some(cell.1);
    }

    /// 作用位置を一行下へ（APD、APR）。表示領域の下端を越えるなら一番上の
    /// 行へ。
    fn down(&mut self) {
        let (_, ch) = self.cell();
        let by = self.last_height.unwrap_or(ch);
        self.y += by as i32;
        if self.y > self.area.1 + self.area.3 as i32 {
            self.y = self.area.1 + ch as i32;
        }
    }

    /// 作用位置を右へ。表示領域の右端に達したら次の行の先頭へ。一文字ずつ進めて、そのたびに確認する。
    fn advance(&mut self, by: u32) {
        let (cw, ch) = self.cell();
        for _ in 0..by {
            self.x += cw as i32;
            if self.x >= self.area.0 + self.area.2 as i32 {
                self.x = self.area.0;
                self.y += ch as i32;
            }
        }
    }

    fn control(&mut self, code: &[u8], layout: &mut Layout) {
        let Some(&first) = code.first() else {
            return;
        };
        let (cw, ch) = self.cell();
        if matches!(first, 0x08..=0x0b | 0x0d | 0x16 | 0x1c) {
            self.moved = true;
        }
        match first {
            // 作用位置。
            0x08 => self.x -= cw as i32,
            0x09 => self.advance(1),
            0x0a => self.down(),
            0x0b => self.y -= ch as i32,
            0x0d => {
                // ワンセグ: 右端で折り返した直後の APR は改行しない
                // （TR-B14 第三分冊 6.3.1「重なるときは一回だけ」）。
                if self.c_profile && self.x == self.area.0 && self.moved {
                    return;
                }
                self.x = self.area.0;
                self.down();
            }
            // CS: 画面を消し、表示領域・文字サイズ・字間・行間を設定値に戻し、
            // 色・極性・点滅・囲み・縁取り・ラスタを既定に戻す。
            0x0c => {
                (self.area, self.ssm, self.shs, self.svs) = self.init;
                self.moved = false;
                if self.delay_ms > 0 {
                    // TIME で待ったあとの CS。**先に配置した文字はそれまで表示しておく**。
                    for text in layout.texts.iter_mut().filter(|t| t.until_ms.is_none()) {
                        text.until_ms = Some(self.delay_ms);
                    }
                    layout.clear_after_ms = Some(self.delay_ms);
                } else {
                    layout.clear = true;
                    layout.texts.clear();
                }
                self.size_mode = 1;
                self.home();
                self.fg = Color::WHITE;
                self.bg = Color::TRANSPARENT;
                self.palette = 0;
                self.inverted = false;
                self.outline = None;
                self.flash = 0;
                self.enclosure = 0;
                self.raster = Color::TRANSPARENT;
                layout.raster = Color::TRANSPARENT;
            }
            // FLC: 点滅（0x40 正相、0x47 逆相、0x4F 止める）。
            0x91 => {
                if let Some(&p) = code.get(1) {
                    self.flash = match p {
                        0x40 => 1,
                        0x47 => 2,
                        _ => 0,
                    };
                }
            }
            // HLC: 囲み線（下位四ビットが右・下・左・上）。
            0x97 => {
                if let Some(&p) = code.get(1) {
                    self.enclosure = p & 0x0f;
                }
            }
            // TIME: 待ち（`9D 20 P` で (P−0x40)/10 秒）。以後の文字を遅らせる。
            0x9d => {
                if code.get(1) == Some(&0x20)
                    && let Some(&p) = code.get(2)
                {
                    self.delay_ms += u32::from(p.saturating_sub(0x40)) * 100;
                }
            }
            // RPC: 次の文字を繰り返す（0x40 なら行末まで）。
            0x98 => {
                if let Some(&n) = code.get(1) {
                    self.repeat = Some(n & 0x3f);
                }
            }
            0x16 => {
                // PAPF: 指定した数だけ前進。
                if let Some(&n) = code.get(1) {
                    self.advance(u32::from(n & 0x3f));
                }
            }
            0x1c => {
                // APS: 行、列（それぞれ 0x40 を足してある）。現在の文字の
                // 大きさで数える（表 7-9）。
                if let (Some(&row), Some(&col)) = (code.get(1), code.get(2)) {
                    self.x = self.area.0 + (u32::from(col & 0x3f) * cw) as i32;
                    self.y = self.area.1 + ((u32::from(row & 0x3f) + 1) * ch) as i32;
                    self.hidden = false;
                }
            }
            // 色。
            0x80..=0x87 => self.set_fg(self.color(first - 0x80)),
            0x90 => {
                // COL：`90 20 4X` はパレット、
                // `4X` 前景、`5X` 背景（下位四ビットが番号、0〜15）、
                // `6X`/`7X` は半前景・半背景（文字の中間調。レンダリングしない）。
                if let Some(&p) = code.get(1) {
                    match p {
                        0x20 => {
                            if let Some(&q) = code.get(2)
                                && q & 0xf0 == 0x40
                            {
                                self.palette = q & 0x07;
                            }
                        }
                        0x40..=0x4f => self.set_fg(self.color(p & 0x0f)),
                        0x50..=0x5f => self.set_bg(self.color(p & 0x0f)),
                        _ => {}
                    }
                }
            }
            // POL: 極性（0x40 正、0x41 反転＝前景と背景を入れ替える）。
            0x93 => {
                if let Some(&p) = code.get(1) {
                    let inverted = p == 0x41;
                    if inverted != self.inverted {
                        std::mem::swap(&mut self.fg, &mut self.bg);
                        self.inverted = inverted;
                    }
                }
            }
            // サイズ。
            0x88 => self.size_mode = 3,
            0x89 => self.size_mode = 2,
            0x8a => self.size_mode = 1,
            0x8b => {
                if let Some(&p) = code.get(1) {
                    self.size_mode = match p {
                        0x60 => 3,
                        0x41 => 4,
                        0x44 => 5,
                        0x45 => 6,
                        _ => 1,
                    };
                }
            }
            0x9b => self.csi(code, layout),
            _ => {}
        }
    }

    /// CSI：`9B P1 3B P2 … Fn`。
    fn csi(&mut self, code: &[u8], layout: &mut Layout) {
        // 少なくとも 9B と終端の二バイト。それより短いのは本文の終わりで
        // 切れた列。
        if code.len() < 2 {
            return;
        }
        let Some(&final_byte) = code.last() else {
            return;
        };
        let params: Vec<u32> = code[1..code.len() - 1]
            .split(|b| *b == 0x3b)
            .map(|p| {
                p.iter()
                    .filter(|b| (0x30..=0x39).contains(*b))
                    .fold(0u32, |n, b| {
                        n.saturating_mul(10).saturating_add(u32::from(b - 0x30))
                    })
            })
            .collect();
        match final_byte {
            // SWF: 表示書式（表 7-x の番号。管理データの Format とは
            // 番号が違う）。
            0x53 => {
                if let Some(&p) = params.first()
                    && !self.moved
                {
                    let (screen, vertical) = match p {
                        5 => ((1920, 1080), false),
                        6 => ((1920, 1080), true),
                        7 => ((960, 540), false),
                        8 => ((960, 540), true),
                        9 => ((720, 480), false),
                        10 => ((720, 480), true),
                        11 => ((1280, 720), false),
                        12 => ((1280, 720), true),
                        _ => return,
                    };
                    self.reset(screen, vertical);
                }
            }
            // SDP: 表示領域の位置。
            0x5f => {
                if let (Some(&x), Some(&y)) = (params.first(), params.get(1))
                    && !self.moved
                {
                    self.area.0 = x as i32;
                    self.area.1 = y as i32;
                    self.home();
                    if self.in_management {
                        self.init.0 = self.area;
                    }
                }
            }
            // SDF: 表示領域のサイズ。
            0x56 => {
                if let (Some(&w), Some(&h)) = (params.first(), params.get(1))
                    && !self.moved
                {
                    self.area.2 = w;
                    self.area.3 = h;
                    if self.in_management {
                        self.init.0 = self.area;
                    }
                }
            }
            // SSM: 文字サイズ。
            0x57 => {
                if let (Some(&w), Some(&h)) = (params.first(), params.get(1))
                    && w == h
                    && matches!(w, 16 | 20 | 24 | 30 | 36)
                {
                    self.ssm = (w, h);
                    if self.in_management {
                        self.init.1 = self.ssm;
                    }
                }
            }
            // SHS: 文字間。
            0x58 => {
                if let Some(&v) = params.first() {
                    self.shs = v;
                    if self.in_management {
                        self.init.2 = v;
                    }
                }
            }
            // SVS: 行間。
            0x59 => {
                if let Some(&v) = params.first() {
                    self.svs = v;
                    if self.in_management {
                        self.init.3 = v;
                    }
                }
            }
            // ORN: 縁取り（1 なら二番目が色。色は「組×100 + 番号」の書き方）。
            0x63 => match params.first() {
                Some(1) => {
                    let p = params.get(1).copied().unwrap_or(0);
                    self.outline = Some(Color::indexed(((p / 100) * 16 + p % 100) as u8));
                }
                _ => self.outline = None,
            },
            // ACPS: 作用位置をピクセルで指定。
            0x61 => {
                if let (Some(&x), Some(&y)) = (params.first(), params.get(1)) {
                    self.x = x as i32;
                    self.y = y as i32;
                    self.moved = true;
                    self.hidden = self.x > self.area.0 + self.area.2 as i32
                        || self.y > self.area.1 + self.area.3 as i32;
                }
            }
            // RCS: ラスタ色（表示領域の塗り）。
            0x6e => {
                if let Some(&c) = params.first()
                    && c < 128
                    && !self.moved
                {
                    self.raster = Color::indexed(c as u8);
                    layout.raster = self.raster;
                }
            }
            // SCR: スクロール（P1 1・2 行、3・4 画面。偶数はロールアウト
            // あり。P2 は速さ）。文字を右から左へ流す。
            0x67 => {
                if let (Some(&mode), Some(&speed)) = (params.first(), params.get(1))
                    && (1..=4).contains(&mode)
                    && speed > 0
                {
                    // 速さは一秒あたりのピクセルとみなす。表示領域の幅の分スクロールする。
                    let width = self.area.2 as i32;
                    layout.motion = Some(Motion {
                        from: (0, 0),
                        to: (-width, 0),
                        delay_ms: 0,
                        duration_ms: (width as u32 * 1000) / speed.max(1),
                        step_end: false,
                    });
                }
            }
            // PRA: 内蔵音。
            0x68 => {
                if let Some(&n) = params.first()
                    && (1..=13).contains(&n)
                {
                    layout.sound = Some(n as u8);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(kind: u8, id: u8, data: &[u8]) -> Vec<u8> {
        let mut out = vec![kind, 0xff, 0x00, id << 2, 0x00, 0x00];
        out.extend_from_slice(&(data.len() as u16).to_be_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(&[0, 0]);
        out
    }

    fn statement_bytes(text: &[u8]) -> Vec<u8> {
        let mut data = vec![0x00]; // TMD 自由
        let mut unit = vec![0x1f, 0x20];
        unit.extend_from_slice(&[
            (text.len() >> 16) as u8,
            (text.len() >> 8) as u8,
            text.len() as u8,
        ]);
        unit.extend_from_slice(text);
        data.extend_from_slice(&[
            (unit.len() >> 16) as u8,
            (unit.len() >> 8) as u8,
            unit.len() as u8,
        ]);
        data.extend_from_slice(&unit);
        data
    }

    #[test]
    fn a_management_group_names_the_language_and_the_format() {
        // TMD 自由、一言語、tag 0 DMF 0、jpn、format 7（960×540 横）。
        let data = [0x00, 0x01, 0x00, b'j', b'p', b'n', 0x70, 0x00, 0x00, 0x00];
        let pes = group(0x80, 0x00, &data);
        let g = pes_to_group(&pes);
        assert!(g.is_management());
        let Body::Management(m) = g.body else {
            panic!()
        };
        assert_eq!(m.languages[0].iso, *b"jpn");
        assert_eq!(m.languages[0].format, 7);
    }

    fn pes_to_group(pes_bytes: &[u8]) -> Group {
        pes(pes_bytes).expect("解析できる")
    }

    #[test]
    fn a_statement_is_laid_out_where_aps_says() {
        // APS 行 2 列 3 → 「あい」、CS、白の既定。
        let mut text = vec![0x1c, 0x40 + 2, 0x40 + 3];
        text.extend_from_slice(&[0x24, 0x22, 0x24, 0x24]); // あ い（漢字集合の既定、JIS）
        let data = statement_bytes(&text);
        let pes_bytes = group(0x81, 0x01, &data);
        let g = pes_to_group(&pes_bytes);
        assert_eq!(g.kind, Kind::Superimpose);
        let Body::Statement(s) = g.body else { panic!() };
        let mut p = Presenter::new(Kind::Superimpose);
        let layout = p.present(&s);
        assert_eq!(layout.screen, (960, 540));
        assert_eq!(layout.texts.len(), 1);
        let t = &layout.texts[0];
        assert_eq!(t.text, "あい");
        assert_eq!((t.x, t.y), (3 * 40, 2 * 60));
        assert_eq!(t.cell, (40, 60));
        assert_eq!(t.fg, Color::WHITE);
        assert!(layout.superimpose);
    }

    #[test]
    fn drcs_glyphs_are_read_and_put_where_the_text_uses_them() {
        // DRCS-0（二バイト）: 区点 0x21 0x21、字形一つ、角ゴシック(2)・
        // mode 1、depth 2（四階調 → 二ビット）、2×2。
        let drcs_unit = [
            0x01,
            0x21,
            0x21,
            0x01,
            0x21,
            0x02,
            0x02,
            0x02,
            0b11_00_00_11,
        ];
        let glyphs = drcs(true, &drcs_unit);
        assert_eq!(glyphs.len(), 1);
        assert_eq!(glyphs[0].1.bitmap, vec![3, 0, 0, 3]);
        assert_eq!(glyphs[0].0, drcs_char(0, 0x2121, 2));
        // 本文で DRCS-0 を G0 に指示して（ESC 0x24 0x28 0x20 0x40）0x21 0x21。
        let mut data = vec![0x00];
        let mut unit_d = vec![0x1f, 0x31, 0, 0, drcs_unit.len() as u8];
        unit_d.extend_from_slice(&drcs_unit);
        let text = [0x1b, 0x24, 0x28, 0x20, 0x40, 0x21, 0x21];
        let mut unit_t = vec![0x1f, 0x20, 0, 0, text.len() as u8];
        unit_t.extend_from_slice(&text);
        let total = unit_d.len() + unit_t.len();
        data.extend_from_slice(&[0, (total >> 8) as u8, total as u8]);
        data.extend_from_slice(&unit_d);
        data.extend_from_slice(&unit_t);
        let g = pes_to_group(&group(0x81, 0x01, &data));
        let Body::Statement(s) = g.body else { panic!() };
        let mut p = Presenter::new(Kind::Superimpose);
        let layout = p.present(&s);
        assert_eq!(layout.texts.len(), 1);
        let letter = layout.texts[0].text.chars().next().unwrap();
        assert!(is_drcs(letter));
        assert_eq!(layout.drcs.get(&letter).map(|g| g.len()), Some(1));
    }

    #[test]
    fn colours_and_sizes_split_the_runs() {
        // 赤 (0x81)、「あ」、MSZ (0x89)、「A」。
        let text = [0x81, 0x24, 0x22, 0x89, 0x0e, 0x41]; // LS1 で G1（英数）に
        let data = statement_bytes(&text);
        let pes_bytes = group(0x80, 0x01, &data);
        let g = pes_to_group(&pes_bytes);
        let Body::Statement(s) = g.body else { panic!() };
        let mut p = Presenter::new(Kind::Caption);
        let layout = p.present(&s);
        assert_eq!(layout.texts.len(), 2);
        assert_eq!(layout.texts[0].fg, Color::indexed(1));
        assert_eq!(layout.texts[1].text, "A");
        assert_eq!(layout.texts[1].cell, (20, 60));
    }

    #[test]
    fn a_csi_cut_off_at_the_end_of_the_text_is_passed_over() {
        // 「あ」のあとに 9B だけで本文が終わる。
        let text = [0x24, 0x22, 0x9b];
        let data = statement_bytes(&text);
        let pes_bytes = group(0x80, 0x01, &data);
        let g = pes_to_group(&pes_bytes);
        let Body::Statement(s) = g.body else { panic!() };
        let mut p = Presenter::new(Kind::Caption);
        let layout = p.present(&s);
        assert_eq!(layout.texts.len(), 1);
    }
}

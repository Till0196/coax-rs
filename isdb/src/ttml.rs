// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! MMT の字幕（ARIB-TTML、STD-B62 第一編第三部 3 章と TR-B39 第二編
//! 8.5）のドキュメント。**TTML のドキュメントとして解析してレイアウトする**——TS の放送の字幕
//! （[`crate::caption`]）とは別のもの。
//!
//! 放送局のドキュメントは決まった構造をしている：`head/styling/style`（`xml:id` ごとに
//! `tts:fontSize="幅 高さ"`、`tts:lineHeight`、`arib-tt:letter-spacing`、
//! `tts:color`、`tts:backgroundColor`、縁取り・影・ボーダー・アニメーション）、
//! `head/layout/region`（`tts:origin` と `tts:extent`、ピクセル）、
//! `body/div/p`（`region`、`begin`/`end`）、`span`（`style` に id の並び、
//! `region` で起点を指定し直す）と `br`。座標は表示解像度のピクセル
//! （3840×2160 が普通）。
//!
//! ここで決めるのは**どこに何を配置するか**（[`Document`]）まで。レンダリングするのは
//! 受信機（deck）で、いつ出すかは [`Paragraph::begin`] / [`Paragraph::end`]
//! とドキュメントの時刻（MPU の提示時刻）から。

use std::collections::BTreeMap;

use crate::xml::{self, Element, Node};

/// 色（`#rrggbb` / `#rrggbbaa`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    pub const WHITE: Rgba = Rgba {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const BLACK: Rgba = Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    /// `#rrggbb` か `#rrggbbaa`（TR-B39 8.5.6.4）、または名前（STD-B62
    /// 3.5.5 は名前も許す）。
    pub fn parse(value: &str) -> Option<Rgba> {
        let v = value.trim();
        if let Some(hex) = v.strip_prefix('#') {
            let n = u32::from_str_radix(hex, 16).ok()?;
            return Some(match hex.len() {
                6 => Rgba {
                    r: (n >> 16) as u8,
                    g: (n >> 8) as u8,
                    b: n as u8,
                    a: 255,
                },
                8 => Rgba {
                    r: (n >> 24) as u8,
                    g: (n >> 16) as u8,
                    b: (n >> 8) as u8,
                    a: n as u8,
                },
                _ => return None,
            });
        }
        let rgb = |r, g, b| Rgba { r, g, b, a: 255 };
        Some(match v.to_ascii_lowercase().as_str() {
            "white" => Rgba::WHITE,
            "black" => Rgba::BLACK,
            "transparent" => Rgba::TRANSPARENT,
            "red" => rgb(255, 0, 0),
            "green" => rgb(0, 128, 0),
            "lime" => rgb(0, 255, 0),
            "yellow" => rgb(255, 255, 0),
            "blue" => rgb(0, 0, 255),
            "magenta" | "fuchsia" => rgb(255, 0, 255),
            "cyan" | "aqua" => rgb(0, 255, 255),
            "gray" | "grey" => rgb(128, 128, 128),
            "silver" => rgb(192, 192, 192),
            _ => return None,
        })
    }

    /// 二色の間（`t` は 0〜1）。
    pub fn mix(self, other: Rgba, t: f32) -> Rgba {
        let t = t.clamp(0.0, 1.0);
        let c = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Rgba {
            r: c(self.r, other.r),
            g: c(self.g, other.g),
            b: c(self.b, other.b),
            a: c(self.a, other.a),
        }
    }

    /// 不透明度を掛けた色。
    pub fn faded(self, opacity: f32) -> Rgba {
        Rgba {
            a: (f32::from(self.a) * opacity.clamp(0.0, 1.0)).round() as u8,
            ..self
        }
    }
}

/// 矩形（左上とサイズ、ピクセル）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// 縁取り（`tts:textOutline="色 太さ ぼかし"`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outline {
    pub color: Rgba,
    pub width: u32,
    pub blur: u32,
}

/// 影（`arib-tt:text-shadow="x y ぼかし 色"`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shadow {
    pub dx: i32,
    pub dy: i32,
    pub blur: u32,
    pub color: Rgba,
}

/// ボーダーの一辺（`arib-tt:border="solid 太さ 色"`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub width: u32,
    pub color: Rgba,
}

/// 文字のスタイル。確定した値だけ持ち、継承は [`Style::apply`] で。
#[derive(Debug, Clone, Default, PartialEq)]
struct Style {
    font: Option<(u32, u32)>,
    /// 行間（ピクセル）。`normal` は None のまま。
    line_height: Option<u32>,
    spacing: Option<u32>,
    fg: Option<Rgba>,
    bg: Option<Rgba>,
    /// 縁取り：（色（無ければ文字色）、太さ、ぼかし）。
    outline: Option<Option<(Option<Rgba>, u32, u32)>>,
    vertical: Option<bool>,
    opacity: Option<f32>,
    underline: Option<bool>,
    bold: Option<bool>,
    italic: Option<bool>,
    shadow: Option<Option<Shadow>>,
    animation: Option<Option<AnimationRef>>,
    /// ボーダー（上・右・下・左）。`border` は四辺まとめて。
    border: [Option<Option<Edge>>; 4],
}

/// `arib-tt:animation` の値（名前で keyframes を参照する）。
#[derive(Debug, Clone, PartialEq)]
struct AnimationRef {
    name: String,
    duration_ms: u32,
    step_end: bool,
    delay_ms: u32,
    /// 回数。無限は None。
    iterations: Option<u32>,
}

impl Style {
    fn apply(&mut self, other: &Style) {
        macro_rules! take {
            ($($field:ident),*) => {
                $(if other.$field.is_some() { self.$field = other.$field.clone(); })*
            };
        }
        take!(
            font,
            line_height,
            spacing,
            fg,
            bg,
            outline,
            vertical,
            opacity,
            underline,
            bold,
            italic,
            shadow,
            animation
        );
        for (mine, theirs) in self.border.iter_mut().zip(&other.border) {
            if theirs.is_some() {
                *mine = *theirs;
            }
        }
    }
}

/// 文字列のスタイル（確定した値）。
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub fg: Rgba,
    pub bg: Rgba,
    /// 文字サイズ（幅、高さ）。
    pub font: (u32, u32),
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub outline: Option<Outline>,
    pub shadow: Option<Shadow>,
    /// 文字表示枠のボーダー（上・右・下・左）。span のスタイルで指定されたものだけ
    /// （領域に指定された `border` は表示領域のボーダー——[`Region::border`]）。
    /// **枠の外側に付く**。[`Run::x`] はボーダーの内側の文字表示枠。
    pub border: [Option<Edge>; 4],
    /// 下線の位置（行の先頭からのピクセル）と太さ。
    /// 横書きは行の上端から下へ、縦書きは行の右端から左へ。
    pub underline_at: (u32, u32),
}

/// 連続する文字列。**一文字一枠**（TR-B39 8.3.4 の文字表示枠）で、横書きなら
/// 右へ、縦書きなら下へ並ぶ。
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// 最初の枠の左上（字幕プレーンの座標）。
    pub x: i32,
    pub y: i32,
    /// 枠の大きさ。横書きは（字幅＋字間、行間）、縦書きは（行間、字高＋字間）。
    pub cell: (u32, u32),
    pub vertical: bool,
    pub text: String,
    pub style: TextStyle,
    /// この span の時刻（ドキュメントの時刻、ミリ秒）。無ければ段落と同じ。
    pub begin: Option<u64>,
    pub end: Option<u64>,
    /// この span だけのアニメーション（span のスタイルで指定されたもの）。
    pub animation: Option<Animation>,
}

impl Run {
    /// ボーダーを含めた矩形（ボーダーは枠の外側）。
    pub fn framed(&self) -> Rect {
        let b = self.bounds();
        let w = |e: Option<Edge>| e.map_or(0, |e| e.width);
        let [top, right, bottom, left] = self.style.border;
        Rect {
            x: b.x - w(left) as i32,
            y: b.y - w(top) as i32,
            w: b.w + w(left) + w(right),
            h: b.h + w(top) + w(bottom),
        }
    }

    /// 枠を並べた矩形。
    pub fn bounds(&self) -> Rect {
        let n = self.text.chars().count() as u32;
        if self.vertical {
            Rect {
                x: self.x,
                y: self.y,
                w: self.cell.0,
                h: self.cell.1 * n,
            }
        } else {
            Rect {
                x: self.x,
                y: self.y,
                w: self.cell.0 * n,
                h: self.cell.1,
            }
        }
    }

    /// 文字表示枠（左上）を順に。
    pub fn cells(&self) -> impl Iterator<Item = (char, i32, i32)> + '_ {
        self.text.chars().enumerate().map(move |(i, c)| {
            if self.vertical {
                (c, self.x, self.y + (self.cell.1 * i as u32) as i32)
            } else {
                (c, self.x + (self.cell.0 * i as u32) as i32, self.y)
            }
        })
    }
}

/// 表示領域（`region`）。
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub id: String,
    pub rect: Rect,
    /// 領域の背景色（領域のスタイルの `tts:backgroundColor`）。**領域の矩形
    /// 全体を塗る**。
    pub bg: Rgba,
    /// 表示領域のボーダー（領域のスタイルの `arib-tt:border`）。矩形の外側に付く。
    pub border: [Option<Edge>; 4],
    /// 領域ごとのアニメーション（領域のスタイルの `arib-tt:animation`）。
    pub animation: Option<Animation>,
}

/// 一つの `p`。
#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    /// 参照している領域（[`Document::regions`] の添字）。
    pub region: Option<usize>,
    /// はみ出しを隠す矩形（`tts:overflow` は hidden で動く、TR-B39 8.5.5.20）。
    pub clip: Option<Rect>,
    /// 表示する時刻と消す時刻（ドキュメントの時刻、ミリ秒）。`end` が無ければ次の
    /// ドキュメントまで。
    pub begin: Option<u64>,
    pub end: Option<u64>,
    pub runs: Vec<Run>,
    /// 段落全体のアニメーション（領域のスタイルで指定されたもの）。
    pub animation: Option<Animation>,
}

/// 静止画（`div` の `smpte:backgroundImage`）。
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub rect: Rect,
    /// `subt://N`（同じ MPU の副サンプル）か `#id`（ドキュメントに埋め込んだ画像）。
    pub src: String,
    pub begin: Option<u64>,
    pub end: Option<u64>,
}

/// 音声（`arib-tt:audio`）。
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    /// `romsound://N`（内蔵音）か `subt://N`（AIFF-C）。
    pub src: String,
    pub looped: bool,
    pub begin: Option<u64>,
    pub end: Option<u64>,
}

/// 外字のフォント（`arib-tt:font-face`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFace {
    pub family: String,
    /// `unicode-range`（始まり、終わり）の並び。
    pub ranges: Vec<(u32, u32)>,
    /// `subt://N`。
    pub url: String,
}

impl FontFace {
    pub fn covers(&self, letter: char) -> bool {
        let code = letter as u32;
        self.ranges
            .iter()
            .any(|(lo, hi)| (*lo..=*hi).contains(&code))
    }
}

/// アニメーションのキーフレーム一つ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// 0〜1（`position="50%"`）。
    pub at: f32,
    pub origin: Option<(i32, i32)>,
    pub color: Option<Rgba>,
    pub bg: Option<Rgba>,
    pub opacity: Option<f32>,
}

/// アニメーション（`arib-tt:keyframes` を `arib-tt:animation` で参照したもの）。
///
/// キーフレームで指定されていない値は前のキーフレームを引き継ぎ、最初から無ければ要素の値
/// （TR-B39 8.5.7）。開始前は 0% のキーフレーム、終了後は 100% のキーフレーム
/// （fill-mode both）。
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub frames: Vec<Frame>,
    pub duration_ms: u32,
    /// `step-end`（偽は `linear`）。
    pub step_end: bool,
    pub delay_ms: u32,
    /// 回数。無限は None。
    pub iterations: Option<u32>,
    /// アニメーションを付けた要素の位置。キーフレームの `origin` との差がオフセットになる。
    pub base: (i32, i32),
    /// 縦書き：x のオフセットは逆向き。
    pub mirror_x: bool,
}

/// ある時刻のアニメーションの値。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub dx: i32,
    pub dy: i32,
    pub color: Option<Rgba>,
    pub bg: Option<Rgba>,
    pub opacity: f32,
}

impl Animation {
    /// アニメーション開始からの進捗（0〜1）。`elapsed_ms` は要素を表示してから。
    pub fn progress(&self, elapsed_ms: u64) -> f32 {
        let t = elapsed_ms.saturating_sub(u64::from(self.delay_ms));
        if elapsed_ms < u64::from(self.delay_ms) || self.duration_ms == 0 {
            return if self.duration_ms == 0 && elapsed_ms >= u64::from(self.delay_ms) {
                1.0
            } else {
                0.0
            };
        }
        let duration = u64::from(self.duration_ms);
        let iteration = t / duration;
        if let Some(count) = self.iterations
            && iteration >= u64::from(count.max(1))
        {
            return 1.0;
        }
        (t % duration) as f32 / duration as f32
    }

    /// アニメーション中か（終わったアニメーションは再レンダリングしなくてよい）。
    pub fn running(&self, elapsed_ms: u64) -> bool {
        match self.iterations {
            None => true,
            Some(count) => {
                elapsed_ms
                    < u64::from(self.delay_ms)
                        + u64::from(self.duration_ms) * u64::from(count.max(1))
            }
        }
    }

    /// 進捗 `p` のときの値。
    pub fn at(&self, p: f32) -> Motion {
        let origin = self.value(
            p,
            |f| f.origin,
            |a, b, t| {
                (
                    (a.0 as f32 + (b.0 - a.0) as f32 * t).round() as i32,
                    (a.1 as f32 + (b.1 - a.1) as f32 * t).round() as i32,
                )
            },
        );
        let color = self.value(p, |f| f.color, |a, b, t| a.mix(b, t));
        let bg = self.value(p, |f| f.bg, |a, b, t| a.mix(b, t));
        let opacity = self
            .value(p, |f| f.opacity, |a, b, t| a + (b - a) * t)
            .unwrap_or(1.0);
        let dx = origin.map_or(0, |o| o.0 - self.base.0);
        Motion {
            dx: if self.mirror_x { -dx } else { dx },
            dy: origin.map_or(0, |o| o.1 - self.base.1),
            color,
            bg,
            opacity,
        }
    }

    /// 進捗 `p` の時刻の値を求める（`elapsed_ms` から）。
    pub fn motion(&self, elapsed_ms: u64) -> Motion {
        self.at(self.progress(elapsed_ms))
    }

    /// 一つの値を、それを指定しているキーフレームの間で補間する。指定しているキーフレームが無ければ None
    /// （要素の値）。最初に指定したキーフレームより前も要素の値。
    fn value<T: Copy>(
        &self,
        p: f32,
        pick: impl Fn(&Frame) -> Option<T>,
        lerp: impl Fn(T, T, f32) -> T,
    ) -> Option<T> {
        let said: Vec<(f32, T)> = self
            .frames
            .iter()
            .filter_map(|f| pick(f).map(|v| (f.at, v)))
            .collect();
        let first = said.first()?;
        if p <= first.0 {
            return Some(first.1);
        }
        for pair in said.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if p >= a.0 && p < b.0 {
                if self.step_end || b.0 <= a.0 {
                    return Some(a.1);
                }
                return Some(lerp(a.1, b.1, (p - a.0) / (b.0 - a.0)));
            }
        }
        said.last().map(|l| l.1)
    }
}

/// 一つの ARIB-TTML のドキュメント。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    /// 字幕プレーンのサイズ（追加情報の `resolution`）。
    pub screen: (u32, u32),
    pub regions: Vec<Region>,
    pub paragraphs: Vec<Paragraph>,
    pub images: Vec<Image>,
    pub audio: Vec<Audio>,
    pub fonts: Vec<FontFace>,
    /// ドキュメントに埋め込んだ画像（`smpte:image`、id → PNG のバイト列）。
    pub embedded: BTreeMap<String, Vec<u8>>,
}

impl Document {
    /// 消去パケット（TR-B39 8.11.5：`<tt></tt>` か、配置するものの無いドキュメント）。
    pub fn is_clear(&self) -> bool {
        self.paragraphs.iter().all(|p| p.runs.is_empty()) && self.images.is_empty()
    }

    /// 最初に表示するものの時刻（ドキュメントの時刻、ミリ秒）。時刻の無いドキュメントは 0。
    pub fn begin(&self) -> u64 {
        self.paragraphs
            .iter()
            .filter_map(|p| p.begin)
            .chain(self.images.iter().filter_map(|i| i.begin))
            .chain(self.audio.iter().filter_map(|a| a.begin))
            .min()
            .unwrap_or(0)
    }

    /// 最後のものが消える時刻。消えないもの（`end` 無し）があれば None。
    pub fn end(&self) -> Option<u64> {
        let mut latest = None;
        for end in self
            .paragraphs
            .iter()
            .filter(|p| !p.runs.is_empty())
            .map(|p| p.end)
            .chain(self.images.iter().map(|i| i.end))
        {
            let end = end?;
            latest = Some(latest.map_or(end, |l: u64| l.max(end)));
        }
        latest
    }

    /// 時刻を捨てる（時刻制御なし、TMD=1111：`begin`/`end` は書いてあっても
    /// 無視する、TR-B39 8.5.9.2）。届いたら表示し、次のドキュメントまで。
    pub fn ignore_times(&mut self) {
        for p in &mut self.paragraphs {
            p.begin = None;
            p.end = None;
            for r in &mut p.runs {
                r.begin = None;
                r.end = None;
            }
        }
        for i in &mut self.images {
            i.begin = None;
            i.end = None;
        }
        for a in &mut self.audio {
            a.begin = None;
            a.end = None;
        }
    }

    /// アニメーションするものがあるか（あればレンダリング側は再レンダリングし続ける）。
    pub fn animates(&self) -> bool {
        self.paragraphs
            .iter()
            .any(|p| p.animation.is_some() || p.begin.is_some() || p.end.is_some())
            || self
                .images
                .iter()
                .any(|i| i.begin.is_some() || i.end.is_some())
    }

    /// 時刻 `at`（ドキュメントの時刻、ミリ秒）に表示されている段落。
    pub fn shown_at(&self, at: u64) -> impl Iterator<Item = &Paragraph> {
        self.paragraphs
            .iter()
            .filter(move |p| !p.runs.is_empty() && between(at, p.begin, p.end))
    }

    /// 時刻 `at` に表示されている画像。
    pub fn images_at(&self, at: u64) -> impl Iterator<Item = &Image> {
        self.images
            .iter()
            .filter(move |i| between(at, i.begin, i.end))
    }
}

/// `begin <= at < end`（無いほうは制限しない）。
pub fn between(at: u64, begin: Option<u64>, end: Option<u64>) -> bool {
    begin.is_none_or(|b| at >= b) && end.is_none_or(|e| at < e)
}

fn px(value: &str) -> Option<u32> {
    let v = value.trim().trim_end_matches("px").trim();
    v.parse::<f32>().ok().map(|n| n.max(0.0).round() as u32)
}

fn px_i(value: &str) -> Option<i32> {
    let v = value.trim().trim_end_matches("px").trim();
    v.parse::<f32>().ok().map(|n| n.round() as i32)
}

fn two(value: &str) -> Option<(i32, i32)> {
    let mut parts = value.split_whitespace();
    Some((px_i(parts.next()?)?, px_i(parts.next()?)?))
}

/// 「種類 太さ 色」。三つ揃わなければ解析できない。none/hidden は無し。
/// 返り値：Some(Some) が線、Some(None) が「無し」と指定された、None が解析できない。
fn edge(value: &str) -> Option<Option<Edge>> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }
    if parts[0] == "none" || parts[0] == "hidden" {
        return Some(None);
    }
    let width = px(parts[1])?;
    let color = Rgba::parse(parts[2])?;
    if width == 0 {
        return Some(None);
    }
    Some(Some(Edge { width, color }))
}

fn ms(value: &str) -> Option<u32> {
    let v = value.trim();
    if let Some(s) = v.strip_suffix("ms") {
        s.parse::<f32>().ok().map(|n| n.max(0.0) as u32)
    } else if let Some(s) = v.strip_suffix('s') {
        s.parse::<f32>().ok().map(|n| (n.max(0.0) * 1000.0) as u32)
    } else {
        v.parse::<f32>().ok().map(|n| n.max(0.0) as u32)
    }
}

/// 要素の属性からスタイルを作る（ローカル名で見る——名前空間の接頭辞は除かれている）。
fn style_of(node: &Element) -> Style {
    let mut style = Style::default();
    for (name, value) in &node.attrs {
        let value = value.as_str();
        match name.as_str() {
            "fontSize" => {
                let mut parts = value.split_whitespace().filter(|p| p.contains("px"));
                let w = parts.next().and_then(px);
                let h = parts.next().and_then(px).or(w);
                if let (Some(w), Some(h)) = (w, h) {
                    style.font = Some((w, h));
                }
            }
            "lineHeight" => {
                // px で、負でないものだけ（`normal` は既定のまま）。
                if value.contains("px") && !value.contains('-') {
                    style.line_height = px(value);
                }
            }
            "letter-spacing" => {
                if value.contains("px") && !value.contains('-') {
                    style.spacing = px(value);
                }
            }
            "color" => style.fg = Rgba::parse(value),
            "backgroundColor" => style.bg = Rgba::parse(value),
            "fontWeight" => style.bold = Some(value.trim() == "bold"),
            "fontStyle" => style.italic = Some(value.trim() == "italic"),
            "textOutline" => {
                // 「[色] 太さ [ぼかし]」か「none」。色だけのものは無し。
                // 色は先頭にしか置けない。**太さが 0px なら縁取りは無い**——放送局は
                // `#000000ff 0px 0px` で「縁取り無し」と書く。
                let parts: Vec<&str> = value.split_whitespace().take(3).collect();
                if parts.first() == Some(&"none") {
                    style.outline = Some(None);
                    continue;
                }
                if parts.len() == 1 && parts[0].starts_with('#') {
                    continue;
                }
                if parts.iter().skip(1).any(|p| p.starts_with('#')) {
                    continue;
                }
                let named = parts
                    .first()
                    .filter(|p| p.starts_with('#'))
                    .and_then(|p| Rgba::parse(p));
                let lengths: Vec<u32> = parts.iter().filter_map(|p| px(p)).collect();
                let width = lengths.first().copied().unwrap_or(0);
                style.outline = Some(if width == 0 {
                    None
                } else {
                    Some((named, width, lengths.get(1).copied().unwrap_or(0)))
                });
            }
            "writingMode" => match value.trim() {
                "lrtb" => style.vertical = Some(false),
                "tbrl" => style.vertical = Some(true),
                _ => {}
            },
            "opacity" => style.opacity = value.trim().parse().ok(),
            "textDecoration" => style.underline = Some(value.trim() == "underline"),
            "text-shadow" => {
                // 「x y ぼかし 色」。四つ揃わなければ無し。
                let parts: Vec<&str> = value.split_whitespace().collect();
                if parts.len() >= 4
                    && let (Some(dx), Some(dy), Some(blur), Some(color)) = (
                        px_i(parts[0]),
                        px_i(parts[1]),
                        px(parts[2]),
                        Rgba::parse(parts[3]),
                    )
                {
                    style.shadow = Some(Some(Shadow {
                        dx,
                        dy,
                        blur,
                        color,
                    }));
                }
            }
            "animation" => {
                // 名前 長さms 補間 遅延ms 回数 向き。**六つ揃い、長さと遅延は
                // ms、補間は linear か step-end、向きは normal**でなければ
                // 無視する。
                let parts: Vec<&str> = value.split_whitespace().collect();
                if parts.len() >= 6
                    && parts[1].ends_with("ms")
                    && parts[3].ends_with("ms")
                    && matches!(parts[2], "linear" | "step-end")
                    && parts[5] == "normal"
                    && let (Some(duration_ms), Some(delay_ms)) = (ms(parts[1]), ms(parts[3]))
                {
                    style.animation = Some(Some(AnimationRef {
                        name: parts[0].to_owned(),
                        duration_ms,
                        step_end: parts[2] == "step-end",
                        delay_ms,
                        iterations: match parts[4] {
                            "infinite" => None,
                            n => Some(n.parse::<u32>().unwrap_or(1)),
                        },
                    }));
                }
            }
            "border" => {
                if let Some(e) = edge(value) {
                    style.border = [Some(e); 4];
                }
            }
            "border-top" => style.border[0] = edge(value).or(style.border[0]),
            "border-right" => style.border[1] = edge(value).or(style.border[1]),
            "border-bottom" => style.border[2] = edge(value).or(style.border[2]),
            "border-left" => style.border[3] = edge(value).or(style.border[3]),
            _ => {}
        }
    }
    style
}

/// `normal` の行間。
fn normal_line_height(font_size: u32) -> u32 {
    let base = (font_size as f32 * 1.2).ceil() as u32;
    base + base / 50
}

/// 下線の位置と太さ：（行の先頭からのピクセル、太さ）。横書きは行の上端から、縦書きは行の右端から。
fn underline_at(vertical: bool, font: (u32, u32), line_height: Option<u32>) -> (u32, u32) {
    let size = if vertical { font.0 } else { font.1 } as f32;
    let (line, offset) = match line_height {
        Some(l) => (
            l as f32,
            if vertical {
                -(size * 0.6).ceil()
            } else {
                (size * 0.45).ceil()
            },
        ),
        None => (
            normal_line_height(size.ceil() as u32) as f32,
            if vertical {
                -(size * 0.6).ceil()
            } else {
                (size * 0.6).ceil()
            },
        ),
    };
    let width = (size / 8.0).ceil() as u32;
    let at = (line / 2.0 + offset).ceil().clamp(0.0, line) as u32;
    (at, width)
}

/// `hh:mm:ss.fff`（TR-B39 8.5.9）をミリ秒に。`12.5s`・`300ms` も読む。
pub fn clock(value: &str) -> Option<u64> {
    let v = value.trim();
    if v.contains(':') {
        let mut parts = v.splitn(3, ':');
        let h: u64 = parts.next()?.trim().parse().ok()?;
        let m: u64 = parts.next()?.trim().parse().ok()?;
        let s = parts.next()?.trim();
        let (whole, frac_ms) = match s.find(['.', ':']) {
            Some(dot) if s.as_bytes()[dot] == b':' => {
                let frames: u64 = s[dot + 1..].parse().ok()?;
                (&s[..dot], frames * 1000 / 30)
            }
            Some(dot) => {
                let digits = &s[dot + 1..];
                let mut millis = 0u64;
                for (i, c) in digits.chars().take(3).enumerate() {
                    millis += u64::from(c.to_digit(10)?) * 10u64.pow(2 - i as u32);
                }
                (&s[..dot], millis)
            }
            None => (s, 0),
        };
        let seconds: u64 = whole.parse().ok()?;
        return Some(((h * 60 + m) * 60 + seconds) * 1000 + frac_ms);
    }
    if let Some(n) = v.strip_suffix("ms") {
        return n.trim().parse::<f64>().ok().map(|n| n.max(0.0) as u64);
    }
    if let Some(n) = v.strip_suffix('s') {
        return n
            .trim()
            .parse::<f64>()
            .ok()
            .map(|n| (n.max(0.0) * 1000.0) as u64);
    }
    if let Some(n) = v.strip_suffix('m') {
        return n
            .trim()
            .parse::<f64>()
            .ok()
            .map(|n| (n.max(0.0) * 60_000.0) as u64);
    }
    if let Some(n) = v.strip_suffix('h') {
        return n
            .trim()
            .parse::<f64>()
            .ok()
            .map(|n| (n.max(0.0) * 3_600_000.0) as u64);
    }
    None
}

/// 親の時刻から、この要素の時刻（TR-B39 8.5.9.1：`begin`/`end` は親の
/// 始まりからの相対。無ければ親を引き継ぐ）。
fn timed(node: &Element, parent: (Option<u64>, Option<u64>)) -> (Option<u64>, Option<u64>) {
    let base = parent.0.unwrap_or(0);
    let begin = match node.attr("begin").and_then(clock) {
        Some(b) => Some(base + b),
        None => parent.0,
    };
    let end = match node.attr("end").and_then(clock) {
        Some(e) => Some(base + e),
        None => parent.1,
    };
    (begin, end)
}

/// スタイルの名前を参照して重ねる（`style="a b"` は a、b の順）。
fn named(styles: &BTreeMap<String, Style>, ids: Option<&str>, into: &mut Style) {
    if let Some(ids) = ids {
        for id in ids.split_whitespace() {
            if let Some(found) = styles.get(id) {
                into.apply(found);
            }
        }
    }
}

/// 文字のスタイルだけを継承する：色・背景色・ボーダー・アニメーションは継承しない。
fn apply_text_styles(into: &mut Style, from: &Style) {
    let mut only = from.clone();
    only.fg = None;
    only.bg = None;
    only.animation = None;
    only.border = [None; 4];
    into.apply(&only);
}

/// 領域の解析結果（位置と、重ねたスタイル）。
#[derive(Debug, Clone)]
struct RegionDef {
    /// `tts:origin` と `tts:extent`（原文のまま：縦書きでも origin は右上）。
    origin: Option<(i32, i32)>,
    extent: Option<(u32, u32)>,
    style: Style,
    index: usize,
}

impl RegionDef {
    /// 左上で持つ矩形。縦書きは origin が右上（TR-B39 8.5.5.9）。
    fn rect(&self, vertical: bool) -> Option<Rect> {
        let (o, e) = (self.origin?, self.extent?);
        Some(Rect {
            x: if vertical { o.0 - e.0 as i32 } else { o.0 },
            y: o.1,
            w: e.0,
            h: e.1,
        })
    }
}

/// 一つの表示領域に同じ時刻で配置する文字。
struct Flow {
    region: Option<String>,
    time: (Option<u64>, Option<u64>),
    pieces: Vec<Piece>,
}

/// ドキュメントのパーサー。
struct Reader {
    styles: BTreeMap<String, Style>,
    regions: BTreeMap<String, RegionDef>,
    keyframes: BTreeMap<String, Vec<Frame>>,
    flows: Vec<Flow>,
    out: Document,
}

impl Reader {
    fn animation(
        &self,
        at: Option<&AnimationRef>,
        base: (i32, i32),
        mirror_x: bool,
    ) -> Option<Animation> {
        let at = at?;
        let frames = self.keyframes.get(&at.name)?;
        Some(Animation {
            frames: frames.clone(),
            duration_ms: at.duration_ms,
            step_end: at.step_end,
            delay_ms: at.delay_ms,
            iterations: at.iterations,
            base,
            mirror_x,
        })
    }

    /// `style` 要素を、参照先から順に解決する（先に書いてあるスタイルを
    /// 後のスタイルが参照できるし、逆も。循環していれば止まる）。
    fn resolve_styles(&mut self, elements: &[&Element]) {
        fn resolve(
            id: &str,
            elements: &[&Element],
            done: &mut BTreeMap<String, Style>,
            visiting: &mut Vec<String>,
        ) {
            if done.contains_key(id) || visiting.iter().any(|v| v == id) {
                return;
            }
            let Some(el) = elements.iter().find(|e| e.attr("id") == Some(id)) else {
                return;
            };
            visiting.push(id.to_owned());
            let mut style = Style::default();
            if let Some(ids) = el.attr("style") {
                for reference in ids.split_whitespace() {
                    resolve(reference, elements, done, visiting);
                    if let Some(found) = done.get(reference) {
                        style.apply(found);
                    }
                }
            }
            style.apply(&style_of(el));
            visiting.pop();
            done.insert(id.to_owned(), style);
        }
        let mut visiting = Vec::new();
        for el in elements {
            if let Some(id) = el.attr("id") {
                resolve(id, elements, &mut self.styles, &mut visiting);
            }
        }
    }

    fn read_head(&mut self, head: &Element) {
        let descendants = head.descendants();
        let styles: Vec<&Element> = descendants
            .iter()
            .copied()
            .filter(|e| e.name == "style")
            .collect();
        self.resolve_styles(&styles);
        for node in &descendants {
            match node.name.as_str() {
                "region" => {
                    let Some(id) = node.attr("id") else {
                        continue;
                    };
                    let mut style = Style::default();
                    named(&self.styles, node.attr("style"), &mut style);
                    style.apply(&style_of(node));
                    let origin = node.attr("origin").and_then(two);
                    let extent = node
                        .attr("extent")
                        .and_then(two)
                        .map(|(w, h)| (w.max(0) as u32, h.max(0) as u32));
                    let def = RegionDef {
                        origin,
                        extent,
                        style,
                        index: self.out.regions.len(),
                    };
                    let vertical = def.style.vertical.unwrap_or(false);
                    let base = origin.unwrap_or((0, 0));
                    let animation = self.animation(
                        def.style.animation.as_ref().and_then(|a| a.as_ref()),
                        base,
                        vertical,
                    );
                    self.out.regions.push(Region {
                        id: id.to_owned(),
                        rect: def.rect(vertical).unwrap_or(Rect {
                            x: 0,
                            y: 0,
                            w: self.out.screen.0,
                            h: self.out.screen.1,
                        }),
                        bg: def.style.bg.unwrap_or(Rgba::TRANSPARENT),
                        border: def.style.border.map(|e| e.flatten()),
                        animation,
                    });
                    self.regions.insert(id.to_owned(), def);
                }
                "keyframes" => {
                    let Some(name) = node.attr("animationName") else {
                        continue;
                    };
                    let mut frames: Vec<Frame> = node
                        .elements()
                        .filter(|n| n.name == "keyframe")
                        .map(|frame| Frame {
                            at: frame
                                .attr("position")
                                .and_then(|p| p.trim().trim_end_matches('%').parse::<f32>().ok())
                                .map_or(0.0, |p| (p / 100.0).clamp(0.0, 1.0)),
                            origin: frame.attr("origin").and_then(two),
                            color: frame.attr("color").and_then(Rgba::parse),
                            bg: frame.attr("backgroundColor").and_then(Rgba::parse),
                            opacity: frame.attr("opacity").and_then(|o| o.trim().parse().ok()),
                        })
                        .collect();
                    frames.sort_by(|a, b| a.at.total_cmp(&b.at));
                    if frames.len() >= 2 {
                        self.keyframes.insert(name.to_owned(), frames);
                    }
                }
                "font-face" => {
                    // `unicode-range` の無いものは使わない。
                    let Some(ranges) = node.attr("unicode-range").map(unicode_ranges) else {
                        continue;
                    };
                    let family = node.attr("font-family").unwrap_or("").to_owned();
                    let url = node
                        .elements()
                        .find(|n| n.name == "src")
                        .and_then(|s| s.attr("url"))
                        .unwrap_or("")
                        .to_owned();
                    self.out.fonts.push(FontFace {
                        family,
                        ranges,
                        url,
                    });
                }
                "image" => {
                    // `smpte:image`（Base64 の PNG）。
                    if let Some(id) = node.attr("id") {
                        let text: String = node
                            .children
                            .iter()
                            .filter_map(|c| match c {
                                Node::Text(t) => Some(t.as_str()),
                                Node::Element(_) => None,
                            })
                            .collect();
                        if let Some(bytes) = base64(&text) {
                            self.out.embedded.insert(id.to_owned(), bytes);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// `body` の下の最初の `div`（TR-B39 8.5.4：body の下の div は一つ）。
    /// その中の `p` は文字、`div` は画像と音声。
    fn read_body(&mut self, body: &Element) {
        let time = timed(body, (None, None));
        let Some(div) = body.elements().find(|e| e.name == "div") else {
            return;
        };
        let time = timed(div, time);
        for child in div.elements() {
            match child.name.as_str() {
                "p" => {
                    let time = timed(child, time);
                    self.paragraph(child, time);
                }
                "div" => {
                    let time = timed(child, time);
                    if let Some(src) = child.attr("backgroundImage") {
                        // 画像の領域は横書き扱い（左上が origin）。
                        let rect = child
                            .attr("region")
                            .and_then(|id| self.regions.get(id))
                            .and_then(|r| r.rect(false));
                        if let Some(rect) = rect {
                            self.out.images.push(Image {
                                rect,
                                src: src.trim().to_owned(),
                                begin: time.0,
                                end: time.1,
                            });
                        }
                    }
                    for audio in child.elements().filter(|n| n.name == "audio") {
                        self.audio(audio, time);
                    }
                }
                _ => {}
            }
        }
        let flows = std::mem::take(&mut self.flows);
        for flow in flows {
            self.lay_out(flow);
        }
    }

    fn audio(&mut self, node: &Element, time: (Option<u64>, Option<u64>)) {
        if let Some(src) = node.attr("src") {
            self.out.audio.push(Audio {
                src: src.trim().to_owned(),
                looped: node.attr("loop").is_some_and(|l| l.trim() == "true"),
                begin: time.0,
                end: time.1,
            });
        }
    }

    /// `p` の span を表示領域ごとのフローに積む。**自分の領域を参照する span は
    /// その領域のフローへ**。同じ領域に同じ時刻で配置するものは一つのフロー。
    fn paragraph(&mut self, p: &Element, time: (Option<u64>, Option<u64>)) {
        let p_region = p.attr("region").map(str::to_owned);
        let mut items = Vec::new();
        inlines(
            p,
            &Inline {
                time,
                ..Inline::default()
            },
            &mut items,
        );
        for item in items {
            let (region, span_time) = match &item {
                Piece::Text(piece) => (
                    piece.region.clone().or_else(|| p_region.clone()),
                    if piece.timed { piece.time } else { time },
                ),
                Piece::Break => (
                    self.flows
                        .last()
                        .map(|f| f.region.clone())
                        .unwrap_or_else(|| p_region.clone()),
                    self.flows.last().map_or(time, |f| f.time),
                ),
            };
            match self
                .flows
                .iter_mut()
                .find(|f| f.region == region && f.time == span_time)
            {
                Some(flow) => flow.pieces.push(item),
                None => self.flows.push(Flow {
                    region,
                    time: span_time,
                    pieces: vec![item],
                }),
            }
        }
    }

    /// 一つのフローを配置する。
    fn lay_out(&mut self, flow: Flow) {
        let region = flow
            .region
            .as_deref()
            .and_then(|id| self.regions.get(id))
            .cloned();
        let Some(region) = region else {
            // 領域の無い文字は配置できない。
            return;
        };
        let Some(origin_raw) = region.origin else {
            return;
        };
        let Some(extent) = region.extent else {
            return;
        };
        // span のスタイル（領域の文字のスタイルを継承し、自分のスタイルを重ねる）。span が
        // 縦書きを指定すれば領域ごと縦書き。
        struct Said {
            text: String,
            style: Style,
        }
        let mut said: Vec<Option<Said>> = Vec::new();
        let mut vertical = region.style.vertical.unwrap_or(false);
        for piece in &flow.pieces {
            match piece {
                Piece::Break => said.push(None),
                Piece::Text(piece) => {
                    let mut style = Style::default();
                    apply_text_styles(&mut style, &region.style);
                    named(&self.styles, piece.styles.as_deref(), &mut style);
                    style.apply(&piece.own);
                    if let Some(v) = style.vertical {
                        vertical = v;
                    }
                    said.push(Some(Said {
                        text: piece.text.clone(),
                        style,
                    }));
                }
            }
        }
        let area = Rect {
            x: if vertical {
                origin_raw.0 - extent.0 as i32
            } else {
                origin_raw.0
            },
            y: origin_raw.1,
            w: extent.0,
            h: extent.1,
        };
        let animation = self.animation(
            region.style.animation.as_ref().and_then(|a| a.as_ref()),
            origin_raw,
            vertical,
        );
        let mut paragraph = Paragraph {
            region: Some(region.index),
            clip: Some(area),
            begin: flow.time.0,
            end: flow.time.1,
            runs: Vec::new(),
            animation,
        };
        // 行に分ける。**入らない文字は次の行へ**（自動折り返し）。連続する空白は一つにまとめ、
        // 折り返した直後の先頭の空白と折り返す直前の末尾の空白は削る
        // （`<br/>` の前後の空白は残る）。
        let area_size = if vertical { extent.1 } else { extent.0 } as i32;
        struct Cut {
            text: String,
            style: Style,
        }
        let mut lines: Vec<Vec<Cut>> = vec![Vec::new()];
        let mut remaining = area_size;
        let mut after_wrap = false;
        let last_is_space = |line: &Vec<Cut>| line.last().is_some_and(|c| c.text.ends_with(' '));
        let line_empty = |line: &Vec<Cut>| line.iter().all(|c| c.text.is_empty());
        for item in said {
            let Some(item) = item else {
                lines.push(Vec::new());
                remaining = area_size;
                after_wrap = false;
                continue;
            };
            let Some(font) = item.style.font else {
                // 文字サイズが無ければ表示できない。
                continue;
            };
            let spacing = item.style.spacing.unwrap_or(0);
            let step = if vertical { font.1 } else { font.0 } as i32 + spacing as i32;
            let mut chars = item.text.chars().peekable();
            let push_cut = |lines: &mut Vec<Vec<Cut>>, style: &Style| {
                lines.last_mut().expect("一行はある").push(Cut {
                    text: String::new(),
                    style: style.clone(),
                });
            };
            push_cut(&mut lines, &item.style);
            while let Some(c) = chars.peek().copied() {
                let c = if matches!(c, '\t' | '\n' | '\r') {
                    ' '
                } else {
                    c
                };
                let line = lines.last_mut().expect("一行はある");
                let ignore = c == ' ' && ((line_empty(line) && after_wrap) || last_is_space(line));
                if step <= remaining {
                    if !ignore {
                        line.last_mut().expect("区切りはある").text.push(c);
                        remaining -= step;
                    }
                    chars.next();
                } else if line_empty(line) {
                    // 領域より大きい文字：一文字だけの行。
                    if !ignore {
                        line.last_mut().expect("区切りはある").text.push(c);
                        lines.push(Vec::new());
                        push_cut(&mut lines, &item.style);
                        remaining = area_size;
                        after_wrap = true;
                    }
                    chars.next();
                } else {
                    // 入らない：折り返す（末尾の空白は削る）。同じ文字を次の行で。
                    if last_is_space(line) {
                        for cut in line.iter_mut().rev() {
                            if cut.text.ends_with(' ') {
                                cut.text.pop();
                                break;
                            }
                        }
                    }
                    lines.push(Vec::new());
                    push_cut(&mut lines, &item.style);
                    remaining = area_size;
                    after_wrap = true;
                }
            }
        }
        // 行を配置する：横書きは上から下へ、縦書きは右から左へ。行の高さは
        // その行の最大の行間で、文字はその中央。
        let mut along = if vertical { origin_raw.0 } else { area.y };
        for line in lines {
            let cuts: Vec<&Cut> = line.iter().filter(|c| !c.text.is_empty()).collect();
            if cuts.is_empty() {
                continue;
            }
            let line_size = |style: &Style| -> u32 {
                let font = style.font.unwrap_or((0, 0));
                style
                    .line_height
                    .unwrap_or_else(|| normal_line_height(if vertical { font.0 } else { font.1 }))
            };
            let thick = cuts.iter().map(|c| line_size(&c.style)).max().unwrap_or(0);
            let mut across = if vertical { area.y } else { area.x };
            for cut in cuts {
                let style = &cut.style;
                let font = style.font.expect("サイズの無い文字は除いてある");
                let spacing = style.spacing.unwrap_or(0);
                let opacity = style.opacity.unwrap_or(1.0);
                // 色：span のスタイル、無ければ領域の色、無ければ白。
                let fg = style
                    .fg
                    .or(region.style.fg)
                    .unwrap_or(Rgba::WHITE)
                    .faded(opacity);
                let outline = style.outline.flatten().map(|(color, width, blur)| Outline {
                    color: color.map_or(fg, |c| c.faded(opacity)),
                    width,
                    blur,
                });
                let border = style.border.map(|e| e.flatten());
                let w = |e: Option<Edge>| e.map_or(0, |e| e.width) as i32;
                let (before, after) = if vertical {
                    (w(border[0]), w(border[2]))
                } else {
                    (w(border[3]), w(border[1]))
                };
                let cell = if vertical {
                    (thick, font.1 + spacing)
                } else {
                    (font.0 + spacing, thick)
                };
                let count = cut.text.chars().count() as i32;
                let (x, y) = if vertical {
                    (along - thick as i32, across + before)
                } else {
                    (across + before, along)
                };
                let animation = self.animation(
                    style.animation.as_ref().and_then(|a| a.as_ref()),
                    origin_raw,
                    vertical,
                );
                paragraph.runs.push(Run {
                    x,
                    y,
                    cell,
                    vertical,
                    text: cut.text.clone(),
                    style: TextStyle {
                        fg,
                        bg: style.bg.unwrap_or(Rgba::TRANSPARENT).faded(opacity),
                        font,
                        bold: style.bold.unwrap_or(false),
                        italic: style.italic.unwrap_or(false),
                        underline: style.underline.unwrap_or(false),
                        outline,
                        shadow: style.shadow.flatten(),
                        border,
                        underline_at: underline_at(vertical, font, style.line_height),
                    },
                    begin: flow.time.0,
                    end: flow.time.1,
                    animation,
                });
                across += before + count * if vertical { cell.1 } else { cell.0 } as i32 + after;
            }
            along += if vertical {
                -(thick as i32)
            } else {
                thick as i32
            };
        }
        self.out.paragraphs.push(paragraph);
    }
}

/// `p` の内容をドキュメントの順に：改行か、文字列と最も近い `span` のスタイル。
#[derive(Debug, Clone, Default)]
struct Inline {
    styles: Option<String>,
    region: Option<String>,
    /// span の時刻（親の p からの相対、解決したあと）。
    time: (Option<u64>, Option<u64>),
    /// span 自身の属性のスタイル。
    own: Style,
    /// 時刻を持つ span の下か。
    timed: bool,
}

enum Piece {
    Break,
    Text(Box<TextPiece>),
}

struct TextPiece {
    text: String,
    styles: Option<String>,
    region: Option<String>,
    time: (Option<u64>, Option<u64>),
    timed: bool,
    own: Style,
}

/// `p` の直下の `span` の内容を順に。
fn inlines(el: &Element, inherited: &Inline, out: &mut Vec<Piece>) {
    for child in &el.children {
        match child {
            Node::Text(text) => {
                // p の直下の文字は無い（span の中だけ、TR-B39 8.5.4）。
                if inherited.styles.is_none()
                    && inherited.region.is_none()
                    && !inherited.timed
                    && el.name == "p"
                {
                    continue;
                }
                if text.is_empty() {
                    continue;
                }
                out.push(Piece::Text(Box::new(TextPiece {
                    text: text.clone(),
                    styles: inherited.styles.clone(),
                    region: inherited.region.clone(),
                    time: inherited.time,
                    timed: inherited.timed,
                    own: inherited.own.clone(),
                })));
            }
            Node::Element(e) if e.name == "br" => out.push(Piece::Break),
            Node::Element(e) if e.name == "span" => {
                let mut own = inherited.own.clone();
                own.apply(&style_of(e));
                let has_time = e.attr("begin").is_some() || e.attr("end").is_some();
                let time = if has_time {
                    timed(e, inherited.time)
                } else {
                    inherited.time
                };
                let next = Inline {
                    styles: match (inherited.styles.as_deref(), e.attr("style")) {
                        (Some(outer), Some(inner)) => Some(format!("{outer} {inner}")),
                        (None, Some(inner)) => Some(inner.to_owned()),
                        (outer, None) => outer.map(str::to_owned),
                    }
                    .or_else(|| Some(String::new())),
                    region: e
                        .attr("region")
                        .map(str::to_owned)
                        .or_else(|| inherited.region.clone()),
                    time,
                    own,
                    timed: inherited.timed || has_time,
                };
                inlines(e, &next, out);
            }
            Node::Element(e) if e.name == "audio" => {}
            Node::Element(e) => inlines(e, inherited, out),
        }
    }
}

/// `U+E000`、`U+0-7F`、`U+A5, U+0-7F` を範囲の並びに。
fn unicode_ranges(value: &str) -> Vec<(u32, u32)> {
    value
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            let body = part
                .strip_prefix("U+")
                .or_else(|| part.strip_prefix("u+"))?;
            let (lo, hi) = match body.split_once('-') {
                Some((lo, hi)) => (lo, hi),
                None => (body, body),
            };
            Some((
                u32::from_str_radix(lo, 16).ok()?,
                u32::from_str_radix(hi, 16).ok()?,
            ))
        })
        .collect()
}

/// Base64（`smpte:image` の内容）。空白は読み飛ばす。
fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in text.chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            '=' => break,
            c if c.is_whitespace() => continue,
            _ => return None,
        };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// 一つの TTML のドキュメントを解析してレイアウトする。`screen` は表示解像度（追加情報の
/// `resolution`）。解析できなければ None。
pub fn parse(text: &str, screen: (u32, u32)) -> Option<Document> {
    let doc = xml::parse(text)?;
    let mut reader = Reader {
        styles: BTreeMap::new(),
        regions: BTreeMap::new(),
        keyframes: BTreeMap::new(),
        flows: Vec::new(),
        out: Document {
            screen,
            ..Default::default()
        },
    };
    if let Some(head) = doc.elements().find(|e| e.name == "head") {
        reader.read_head(head);
    }
    if let Some(body) = doc.elements().find(|e| e.name == "body") {
        reader.read_body(body);
    }
    Some(reader.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_text(doc: &Document) -> Vec<String> {
        doc.paragraphs
            .iter()
            .flat_map(|p| p.runs.iter().map(|r| r.text.clone()))
            .collect()
    }

    #[test]
    fn keyframes_borders_outlines_and_audio_are_read() {
        let xml = r##"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:tts="http://www.w3.org/ns/ttml#styling" xmlns:arib-tt="http://www.arib.or.jp/ns/arib-tt"><head><styling><style xml:id="s1" tts:fontSize="144px 144px" tts:lineHeight="240px" tts:textOutline="#000000ff 4px 0px" tts:textDecoration="underline" arib-tt:text-shadow="4px 4px 0px #ff0000ff"/><arib-tt:keyframes animationName="slide"><keyframe position="0%" tts:origin="3840px 1800px"/><keyframe position="100%" tts:origin="0px 1800px"/></arib-tt:keyframes></styling><layout><region xml:id="r" tts:origin="100px 1800px" tts:extent="3640px 240px" arib-tt:border="solid 8px #ffffffff" arib-tt:animation="slide 20000ms linear 0ms 1 normal"/></layout></head><body><div begin="00:00:01.000" end="00:00:21.000"><p region="r"><span style="s1">流れる</span></p><div><arib-tt:audio arib-tt:src="chime.aiff"/></div></div></body></tt>"##;
        let doc = parse(xml, (3840, 2160)).expect("解析できる");
        let p = &doc.paragraphs[0];
        let r = &p.runs[0];
        assert_eq!(
            r.style.outline.map(|o| (o.color, o.width)),
            Some((Rgba::BLACK, 4))
        );
        assert!(r.style.underline);
        assert_eq!(r.style.shadow.map(|s| (s.dx, s.dy)), Some((4, 4)));
        let m = p.animation.as_ref().expect("アニメーション");
        assert_eq!(m.duration_ms, 20000);
        assert_eq!((m.at(0.0).dx, m.at(0.0).dy), (3740, 0));
        assert_eq!((m.at(1.0).dx, m.at(1.0).dy), (-100, 0));
        assert_eq!(m.at(0.5).dx, 1820);
        assert_eq!(doc.regions[0].border[0].map(|b| b.width), Some(8));
        assert_eq!(doc.audio.len(), 1);
        assert_eq!(doc.audio[0].src, "chime.aiff");
        assert_eq!(doc.audio[0].begin, Some(1000));
        assert_eq!((p.begin, p.end), (Some(1000), Some(21000)));
    }

    #[test]
    fn a_broadcast_document_is_laid_out_in_its_region() {
        let xml = r##"<?xml version="1.0" encoding="utf-8"?><tt xmlns="http://www.w3.org/ns/ttml" xmlns:tts="http://www.w3.org/ns/ttml#styling" xmlns:arib-tt="http://www.arib.or.jp/ns/arib-tt" xml:lang="ja"><head><styling><style xml:id="middleSize1" tts:fontSize="72px 144px" tts:lineHeight="240px" arib-tt:letter-spacing="8px"/><style xml:id="foreColor1" tts:color="#ffffffff"/><style xml:id="backColor1" tts:backgroundColor="#00000080"/><style xml:id="normalSize1" tts:fontSize="144px 144px" tts:lineHeight="240px" arib-tt:letter-spacing="16px"/></styling><layout><region xml:id="r690" tts:origin="1592px 1796px" tts:extent="640px 240px"/></layout></head><body><div begin="00:12:32.000" end="00:12:35.000"><p xml:id="P1880-1" region="r690"><span style="middleSize1 foreColor1 backColor1">（</span><span style="normalSize1 foreColor1 backColor1">落石音</span><span style="middleSize1 foreColor1 backColor1">）</span></p></div></body></tt>"##;
        let doc = parse(xml, (3840, 2160)).expect("解析できる");
        let p = &doc.paragraphs[0];
        assert_eq!(p.runs.len(), 3);
        assert_eq!((p.runs[0].x, p.runs[0].y), (1592, 1796));
        assert_eq!(p.runs[0].cell, (80, 240));
        assert_eq!(p.runs[1].x, 1592 + 80);
        assert_eq!(p.runs[1].text, "落石音");
        assert_eq!(p.runs[1].cell, (160, 240));
        assert_eq!(p.runs[1].style.bg.a, 0x80);
        assert_eq!(
            p.clip,
            Some(Rect {
                x: 1592,
                y: 1796,
                w: 640,
                h: 240
            })
        );
        assert_eq!(doc.begin(), 752_000);
        assert_eq!(doc.end(), Some(755_000));
        assert!(doc.shown_at(752_000).next().is_some());
        assert!(doc.shown_at(755_000).next().is_none());
    }

    /// 消去パケット（TR-B39 8.11.5）：`<tt></tt>` か、head だけのドキュメント。
    /// **解析でき、配置するものが無い**——解析できないと前の字幕が残る。
    #[test]
    fn a_clear_packet_is_read_as_a_document_with_nothing_to_show() {
        for xml in [
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<tt></tt>",
            "<tt/>",
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<tt xmlns=\"http://www.w3.org/ns/ttml\" xmlns:tts=\"http://www.w3.org/ns/ttml#styling\">\n<head>\n</head>\n</tt>",
            "<tt xmlns=\"http://www.w3.org/ns/ttml\"><head><styling/></head><body/></tt>",
            "<?xml version=\"1.0\" encoding=\"utf-8\"?><tt xmlns=\"http://www.w3.org/ns/ttml\" xmlns:tts=\"http://www.w3.org/ns/ttml#styling\" xmlns:arib-tt=\"http://www.arib.or.jp/ns/arib-tt\" ><body><div><p><span/></p></div></body></tt>",
        ] {
            let doc = parse(xml, (3840, 2160)).expect("解析できる");
            assert!(doc.is_clear(), "{xml}");
        }
    }

    /// **段落ごとの時刻**：一つのドキュメントに時刻の違う p が並ぶ。p の時刻は
    /// 親（div）の始まりからの相対（TR-B39 8.5.9.1）。
    #[test]
    fn paragraphs_keep_their_own_times_relative_to_the_parent() {
        let xml = r##"<tt><head><styling><style xml:id="s" tts:fontSize="64px"/></styling><layout><region xml:id="r" tts:origin="0px 0px" tts:extent="640px 64px"/></layout></head><body><div begin="00:00:10.000"><p region="r" begin="00:00:01.000" end="00:00:03.000"><span style="s">a</span></p><p region="r" begin="00:00:02.000"><span style="s">b</span></p></div></body></tt>"##;
        let doc = parse(xml, (1920, 1080)).expect("解析できる");
        assert_eq!(doc.paragraphs[0].begin, Some(11_000));
        assert_eq!(doc.paragraphs[0].end, Some(13_000));
        assert_eq!(doc.paragraphs[1].begin, Some(12_000));
        assert_eq!(doc.paragraphs[1].end, None);
        assert_eq!(doc.begin(), 11_000);
        assert_eq!(doc.end(), None, "消えない段落がある");
        let at = |t| {
            doc.shown_at(t)
                .map(|p| p.runs[0].text.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(at(10_500), Vec::<String>::new());
        assert_eq!(at(11_000), vec!["a"]);
        assert_eq!(at(12_500), vec!["a", "b"]);
        assert_eq!(at(13_000), vec!["b"]);
    }

    /// **文字表示枠**（TR-B39 8.3.4）：字幅＋字間、行間。**自分の領域を
    /// 参照する span は別の表示領域**で、その origin から配置し、p のスタイルは継承しない。
    #[test]
    fn a_span_with_its_own_region_is_its_own_area() {
        let xml = r##"<tt><head><styling><style xml:id="s" tts:fontSize="60px" arib-tt:letter-spacing="20px" tts:lineHeight="80px"/></styling><layout><region xml:id="p1" tts:origin="100px 100px" tts:extent="490px 170px"/><region xml:id="p2" tts:origin="185px 180px" tts:extent="490px 90px"/></layout></head><body><div><p region="p1"><span style="s">あい</span><span style="s" region="p2">うえ</span><span style="s">お</span></p></div></body></tt>"##;
        let doc = parse(xml, (1920, 1080)).expect("解析できる");
        assert_eq!(doc.paragraphs.len(), 2);
        let first = &doc.paragraphs[0].runs;
        assert_eq!(
            (first[0].x, first[0].y, first[0].cell),
            (100, 100, (80, 80))
        );
        assert_eq!(
            first[0].bounds(),
            Rect {
                x: 100,
                y: 100,
                w: 160,
                h: 80
            }
        );
        assert_eq!(
            (first[1].text.as_str(), first[1].x),
            ("お", 260),
            "p の領域は続きから"
        );
        let second = &doc.paragraphs[1];
        assert_eq!(
            second.clip,
            Some(Rect {
                x: 185,
                y: 180,
                w: 490,
                h: 90
            })
        );
        assert_eq!((second.runs[0].x, second.runs[0].y), (185, 180));
    }

    /// 行の高さはその行の最大の行間。
    #[test]
    fn a_line_is_as_thick_as_its_tallest_run() {
        let xml = r##"<tt><head><styling><style xml:id="a" tts:fontSize="60px" tts:lineHeight="80px"/><style xml:id="b" tts:fontSize="40px" tts:lineHeight="50px"/></styling><layout><region xml:id="r" tts:origin="0px 0px" tts:extent="600px 300px"/></layout></head><body><div><p region="r"><span style="b">小</span><span style="a">大</span><br/><span style="b">次</span></p></div></body></tt>"##;
        let doc = parse(xml, (1920, 1080)).expect("解析できる");
        let runs = &doc.paragraphs[0].runs;
        assert_eq!(runs[0].cell, (40, 80));
        assert_eq!(runs[1].cell, (60, 80));
        assert_eq!((runs[2].y, runs[2].cell), (80, (40, 50)));
    }

    /// **ボーダー**：領域の `border` は表示領域、span の `border-*` は文字表示枠。
    #[test]
    fn borders_on_the_region_and_on_the_span_are_told_apart() {
        let xml = r##"<tt><head><styling><style xml:id="ss" tts:fontSize="60px" arib-tt:border="solid 5px #0000ff"/><style xml:id="s1" arib-tt:border-top="solid 5px #0000ff" arib-tt:border-left="solid 5px #0000ff"/></styling><layout><region xml:id="p1" style="ss" tts:origin="100px 100px" tts:extent="490px 90px"/></layout></head><body><div><p region="p1"><span>あ</span><span style="s1">い</span></p></div></body></tt>"##;
        let doc = parse(xml, (1920, 1080)).expect("解析できる");
        assert_eq!(doc.regions[0].border[0].map(|b| b.width), Some(5));
        let runs = &doc.paragraphs[0].runs;
        assert_eq!(
            runs[0].style.border, [None; 4],
            "領域のボーダーは文字には付かない"
        );
        assert!(runs[1].style.border[0].is_some() && runs[1].style.border[3].is_some());
        assert!(runs[1].style.border[1].is_none());
    }

    /// **点滅**（TR-B39 8.5.7.1）：opacity のキーフレームを step-end で繰り返す。
    #[test]
    fn blinking_is_an_opacity_animation_stepping_between_frames() {
        let xml = r##"<tt><head><styling><arib-tt:keyframes animationName="blink"><arib-tt:keyframe position="0%" tts:opacity="1"/><arib-tt:keyframe position="50%" tts:opacity="0"/><arib-tt:keyframe position="100%" tts:opacity="1"/></arib-tt:keyframes><style xml:id="s001" tts:fontSize="46px"/><style xml:id="s003" arib-tt:animation="blink 2000ms step-end 0ms 100 normal"/></styling><layout><region xml:id="r001" style="s001 s003" tts:origin="0px 40px" tts:extent="300px 90px"/></layout></head><body><div><p region="r001"><span>x</span></p></div></body></tt>"##;
        let doc = parse(xml, (1920, 1080)).expect("解析できる");
        let a = doc.paragraphs[0]
            .animation
            .as_ref()
            .expect("アニメーション");
        assert_eq!(a.iterations, Some(100));
        assert_eq!(a.motion(0).opacity, 1.0);
        assert_eq!(a.motion(500).opacity, 1.0);
        assert_eq!(a.motion(1000).opacity, 0.0);
        assert_eq!(a.motion(1999).opacity, 0.0);
        assert_eq!(a.motion(2000).opacity, 1.0, "二周目");
        assert_eq!(
            a.motion(200_000 + 1000).opacity,
            1.0,
            "百回で終わり、100% のキーフレーム"
        );
        assert!(a.running(1000) && !a.running(200_000));
    }

    /// 縦書きの `tts:origin` は表示領域の**右上**（TR-B39 8.5.5.9）。列は右から左へ、文字は下へ。
    #[test]
    fn vertical_writing_hangs_from_the_top_right_origin() {
        let xml = r##"<tt><head><styling><style xml:id="s" tts:fontSize="60px" tts:lineHeight="80px" tts:writingMode="tbrl"/></styling><layout><region xml:id="r" style="s" tts:origin="1160px 100px" tts:extent="160px 600px"/></layout></head><body><div><p region="r"><span>あい</span><br/><span>う</span></p></div></body></tt>"##;
        let doc = parse(xml, (1920, 1080)).expect("解析できる");
        let p = &doc.paragraphs[0];
        assert_eq!(
            p.clip,
            Some(Rect {
                x: 1000,
                y: 100,
                w: 160,
                h: 600
            })
        );
        let runs = &p.runs;
        assert!(runs[0].vertical);
        assert_eq!((runs[0].x, runs[0].y, runs[0].cell), (1080, 100, (80, 60)));
        assert_eq!((runs[1].x, runs[1].y), (1000, 100));
        let cells: Vec<_> = runs[0].cells().collect();
        assert_eq!(cells[1], ('い', 1080, 160));
    }

    /// 静止画（`smpte:backgroundImage`）と外字のフォントの宣言と埋め込んだ画像。
    #[test]
    fn images_fonts_and_embedded_pictures_are_read() {
        let xml = r##"<tt xmlns:smpte="http://www.smpte-ra.org/schemas/2052-1/2013/smpte-tt"><head><metadata><smpte:image xml:id="Img1" imageType="PNG" encoding="Base64">AQID
BA==</smpte:image></metadata><styling><arib-tt:font-face font-family="Round gothic" unicode-range="U+F000-F005, U+E000"><arib-tt:src url="subt://1" format="svg"/></arib-tt:font-face></styling><layout><region xml:id="r" tts:origin="10px 20px" tts:extent="300px 200px"/></layout></head><body><div><div region="r" begin="00:00:05" end="00:00:10" smpte:backgroundImage="subt://2"/><div region="r" smpte:backgroundImage="#Img1"/></div></body></tt>"##;
        let doc = parse(xml, (1920, 1080)).expect("解析できる");
        assert_eq!(doc.images.len(), 2);
        assert_eq!(doc.images[0].src, "subt://2");
        assert_eq!(
            (doc.images[0].begin, doc.images[0].end),
            (Some(5000), Some(10_000))
        );
        assert_eq!(
            doc.images[0].rect,
            Rect {
                x: 10,
                y: 20,
                w: 300,
                h: 200
            }
        );
        assert_eq!(
            doc.embedded.get("Img1").map(Vec::as_slice),
            Some(&[1u8, 2, 3, 4][..])
        );
        assert_eq!(
            doc.fonts[0].ranges,
            vec![(0xF000, 0xF005), (0xE000, 0xE000)]
        );
        assert!(doc.fonts[0].covers('\u{F003}') && !doc.fonts[0].covers('a'));
        assert!(!doc.is_clear());
    }

    /// 連続する空白は一つにまとめるが、`<br/>` の前後の空白は残る。改行・タブは空白。
    #[test]
    fn spaces_collapse_but_survive_around_a_br() {
        let xml = "<tt><head><styling><style xml:id=\"s\" tts:fontSize=\"60px\"/></styling><layout><region xml:id=\"r\" tts:origin=\"0px 0px\" tts:extent=\"600px 200px\"/></layout></head><body><div><p region=\"r\"><span style=\"s\">あいうえお <br/>\n   かきくけこ</span></p></div></body></tt>";
        let doc = parse(xml, (1920, 1080)).expect("解析できる");
        assert_eq!(run_text(&doc), vec!["あいうえお ", " かきくけこ"]);
        assert_eq!(doc.paragraphs[0].runs[1].y, normal_line_height(60) as i32);
    }

    /// **入らない文字は次の行へ**：領域の幅で
    /// 折り返し、折り返す直前の空白と直後の先頭の空白は削る。
    #[test]
    fn text_wraps_at_the_area_width() {
        let xml = r##"<tt><head><styling><style xml:id="s" tts:fontSize="60px" tts:lineHeight="80px"/></styling><layout><region xml:id="r" tts:origin="0px 0px" tts:extent="200px 400px"/></layout></head><body><div><p region="r"><span style="s">あいう えおか</span></p></div></body></tt>"##;
        let doc = parse(xml, (1920, 1080)).expect("解析できる");
        assert_eq!(run_text(&doc), vec!["あいう", "えおか"]);
        let runs = &doc.paragraphs[0].runs;
        assert_eq!((runs[0].y, runs[1].y), (0, 80));
        // 領域より大きい文字は一文字だけの行。
        let xml = r##"<tt><head><styling><style xml:id="s" tts:fontSize="300px"/></styling><layout><region xml:id="r" tts:origin="0px 0px" tts:extent="200px 900px"/></layout></head><body><div><p region="r"><span style="s">あい</span></p></div></body></tt>"##;
        let doc = parse(xml, (1920, 1080)).expect("解析できる");
        assert_eq!(run_text(&doc), vec!["あ", "い"]);
    }

    /// 同じ領域に同じ時刻で配置する p は一つのフロー。
    /// 色は span のスタイル、無ければ領域の色。領域の背景は領域全体。
    #[test]
    fn paragraphs_in_one_region_at_one_time_flow_together() {
        let xml = r##"<tt><head><styling><style xml:id="s" tts:fontSize="60px" tts:color="#ff0000" tts:backgroundColor="#00000080"/><style xml:id="g" tts:color="#00ff00"/></styling><layout><region xml:id="r" style="s" tts:origin="0px 0px" tts:extent="600px 200px"/></layout></head><body><div><p region="r"><span>あ</span></p><p region="r"><span style="g">い</span></p></div></body></tt>"##;
        let doc = parse(xml, (1920, 1080)).expect("解析できる");
        assert_eq!(doc.paragraphs.len(), 1);
        let runs = &doc.paragraphs[0].runs;
        assert_eq!((runs[0].text.as_str(), runs[0].x), ("あ", 0));
        assert_eq!((runs[1].text.as_str(), runs[1].x), ("い", 60));
        assert_eq!(
            runs[0].style.fg,
            Rgba {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            }
        );
        assert_eq!(
            runs[1].style.fg,
            Rgba {
                r: 0,
                g: 255,
                b: 0,
                a: 255
            }
        );
        assert_eq!(
            runs[0].style.bg,
            Rgba::TRANSPARENT,
            "文字の背景は span のスタイルだけ"
        );
        assert_eq!(doc.regions[0].bg.a, 0x80);
    }

    /// span のボーダーは枠の外側に付き、次の span はその太さの分ずれる。
    #[test]
    fn a_span_border_sits_outside_and_pushes_the_next_span() {
        let xml = r##"<tt><head><styling><style xml:id="s" tts:fontSize="60px"/><style xml:id="b" arib-tt:border="solid 5px #0000ff"/></styling><layout><region xml:id="r" tts:origin="100px 100px" tts:extent="600px 200px"/></layout></head><body><div><p region="r"><span style="s b">あ</span><span style="s">い</span></p></div></body></tt>"##;
        let doc = parse(xml, (1920, 1080)).expect("解析できる");
        let runs = &doc.paragraphs[0].runs;
        assert_eq!(runs[0].x, 105);
        assert_eq!(
            runs[0].framed(),
            Rect {
                x: 100,
                y: 95,
                w: 70,
                h: normal_line_height(60) + 10
            }
        );
        assert_eq!(runs[1].x, 170);
    }

    /// 下線の位置と太さ。
    #[test]
    fn underline_position() {
        assert_eq!(underline_at(false, (60, 60), Some(80)), (67, 8));
        assert_eq!(underline_at(false, (144, 144), Some(240)), (185, 18));
        assert_eq!(underline_at(true, (60, 60), Some(80)), (4, 8));
        assert_eq!(normal_line_height(60), 73);
    }

    #[test]
    fn clock_forms() {
        assert_eq!(clock("00:12:32.000"), Some(752_000));
        assert_eq!(clock("01:00:00.5"), Some(3_600_500));
        assert_eq!(clock("00:00:01:15"), Some(1_500));
        assert_eq!(clock("2.5s"), Some(2_500));
        assert_eq!(clock("300ms"), Some(300));
        assert_eq!(clock("nonsense"), None);
    }
}

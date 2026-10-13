// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! 小さな SVG。**輪郭をベクターのまま持ち、レンダリングするサイズでラスタライズする。**
//!
//! 読むのは放送で使われる範囲（STD-B62 第一編第二部 5.5 のフォント、3.5 の
//! 画像、受信機のボタンのアイコン）：`<path d>`・`<rect>`・`<circle>`・`<ellipse>`・
//! `<polygon>`・`<polyline>`・`<line>` の塗り、`fill`／`fill-opacity`／
//! `opacity`（`style` の中も）、`transform`（matrix・translate・scale・
//! rotate・skewX・skewY）と `<g>` の継承、`viewBox`。線（stroke）は
//! 読みません。SVG フォント（`<font><glyph unicode d>`）は文字ごとの輪郭に。
//!
//! ラスタライズは非ゼロ回転数規則のスキャンラインで行い、曲線はラスタライズするサイズに応じて
//! 分割する——グリッドのビットマップに一度ラスタライズしてから拡大縮小する（DRCS の方法）と、文字の輪郭が
//! ぼやける。

use crate::xml::{self, Element};

/// 色（RGBA）。
pub type Rgba = [u8; 4];

/// 輪郭のコマンド一つ。座標は絶対。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    Move((f32, f32)),
    Line((f32, f32)),
    Quad((f32, f32), (f32, f32)),
    Cubic((f32, f32), (f32, f32), (f32, f32)),
    Close,
}

/// 二次元のアフィン変換（`a c e / b d f`）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform(pub [f32; 6]);

impl Transform {
    pub const IDENTITY: Transform = Transform([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub fn apply(&self, p: (f32, f32)) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (a * p.0 + c * p.1 + e, b * p.0 + d * p.1 + f)
    }

    /// `self` のあとに `other`（`other × self`）。
    pub fn then(&self, other: &Transform) -> Transform {
        let [a1, b1, c1, d1, e1, f1] = self.0;
        let [a2, b2, c2, d2, e2, f2] = other.0;
        Transform([
            a2 * a1 + c2 * b1,
            b2 * a1 + d2 * b1,
            a2 * c1 + c2 * d1,
            b2 * c1 + d2 * d1,
            a2 * e1 + c2 * f1 + e2,
            b2 * e1 + d2 * f1 + f2,
        ])
    }

    /// `transform="translate(…) rotate(…) …"` を読む。左から順に掛ける
    /// （SVG は右のものを先に効かせる）。
    pub fn parse(text: &str) -> Transform {
        let mut out = Transform::IDENTITY;
        let mut rest = text;
        while let Some(open) = rest.find('(') {
            let name = rest[..open].trim().trim_start_matches(',').trim();
            let Some(close) = rest[open..].find(')') else {
                break;
            };
            let args = numbers(&rest[open + 1..open + close]);
            rest = &rest[open + close + 1..];
            let t = match (name, args.as_slice()) {
                ("matrix", [a, b, c, d, e, f]) => Transform([*a, *b, *c, *d, *e, *f]),
                ("translate", [x]) => Transform([1.0, 0.0, 0.0, 1.0, *x, 0.0]),
                ("translate", [x, y]) => Transform([1.0, 0.0, 0.0, 1.0, *x, *y]),
                ("scale", [s]) => Transform([*s, 0.0, 0.0, *s, 0.0, 0.0]),
                ("scale", [x, y]) => Transform([*x, 0.0, 0.0, *y, 0.0, 0.0]),
                ("rotate", [deg]) => rotation(*deg),
                ("rotate", [deg, cx, cy]) => Transform([1.0, 0.0, 0.0, 1.0, -cx, -cy])
                    .then(&rotation(*deg))
                    .then(&Transform([1.0, 0.0, 0.0, 1.0, *cx, *cy])),
                ("skewX", [deg]) => Transform([1.0, 0.0, deg.to_radians().tan(), 1.0, 0.0, 0.0]),
                ("skewY", [deg]) => Transform([1.0, deg.to_radians().tan(), 0.0, 1.0, 0.0, 0.0]),
                _ => continue,
            };
            // 並びは左が外側：`translate rotate` は rotate してから translate。
            out = t.then(&out);
        }
        out
    }
}

fn rotation(deg: f32) -> Transform {
    let (s, c) = deg.to_radians().sin_cos();
    Transform([c, s, -s, c, 0.0, 0.0])
}

/// 輪郭（塗る領域）。座標は `viewbox` の座標系で、y は下向き。
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    pub steps: Vec<Step>,
    /// この座標系を、レンダリングする矩形いっぱいに対応させる（x, y, 幅, 高さ）。
    pub viewbox: (f32, f32, f32, f32),
}

impl Outline {
    /// 変換を掛けた輪郭。
    pub fn transformed(&self, t: &Transform) -> Outline {
        Outline {
            steps: self
                .steps
                .iter()
                .map(|s| match *s {
                    Step::Move(p) => Step::Move(t.apply(p)),
                    Step::Line(p) => Step::Line(t.apply(p)),
                    Step::Quad(c, p) => Step::Quad(t.apply(c), t.apply(p)),
                    Step::Cubic(c1, c2, p) => Step::Cubic(t.apply(c1), t.apply(c2), t.apply(p)),
                    Step::Close => Step::Close,
                })
                .collect(),
            viewbox: self.viewbox,
        }
    }

    /// `width × height` のピクセルにラスタライズする（カバレッジ、0〜255）。座標系を矩形
    /// いっぱいに対応させる（縦横比は保たない：文字の幅と高さは別に指定される）。
    pub fn rasterise(&self, width: usize, height: usize) -> Vec<u8> {
        if width == 0 || height == 0 {
            return Vec::new();
        }
        let segments = self.segments(width, height);
        coverage(&segments, width, height)
    }

    /// 座標系を `width × height` に対応させた線分。
    fn segments(&self, width: usize, height: usize) -> Vec<[(f32, f32); 2]> {
        let (vx, vy, vw, vh) = self.viewbox;
        let sx = width as f32 / vw.max(1e-6);
        let sy = height as f32 / vh.max(1e-6);
        let to_pixel = Transform([sx, 0.0, 0.0, sy, -vx * sx, -vy * sy]);
        flatten(&self.transformed(&to_pixel).steps)
    }
}

/// 一つの塗り。
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub outline: Outline,
    pub fill: Rgba,
}

/// 一枚の画像。
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    /// (x, y, 幅, 高さ)。
    pub viewbox: (f32, f32, f32, f32),
    pub shapes: Vec<Shape>,
}

impl Drawing {
    /// SVG のドキュメントを読む。`<svg>` がルートでなければ最初の `<svg>`。
    pub fn parse(text: &str) -> Option<Drawing> {
        let root = xml::parse(text)?;
        let root = if root.name == "svg" {
            root
        } else {
            root.find_all("svg").into_iter().next()?.clone()
        };
        let viewbox = match root.attr("viewBox").map(numbers) {
            Some(v) if v.len() == 4 && v[2] > 0.0 && v[3] > 0.0 => (v[0], v[1], v[2], v[3]),
            _ => {
                let size = |name: &str, or: f32| {
                    root.attr(name)
                        .and_then(|v| v.trim().trim_end_matches("px").parse().ok())
                        .unwrap_or(or)
                };
                (0.0, 0.0, size("width", 24.0), size("height", 24.0))
            }
        };
        let mut drawing = Drawing {
            viewbox,
            shapes: Vec::new(),
        };
        let paint = Paint {
            fill: Some([0, 0, 0, 255]),
            opacity: 1.0,
            transform: Transform::IDENTITY,
        };
        collect(&root, &paint, viewbox, &mut drawing.shapes);
        Some(drawing)
    }

    /// 座標系のサイズ（ピクセルとして使うとき）。
    pub fn size(&self) -> (f32, f32) {
        (self.viewbox.2, self.viewbox.3)
    }

    /// `width × height` の RGBA にラスタライズする（上から一行ずつ、乗算していない α）。
    pub fn rasterise(&self, width: usize, height: usize) -> Vec<u8> {
        let mut out = vec![0u8; width * height * 4];
        for shape in &self.shapes {
            let cover = shape.outline.rasterise(width, height);
            let [r, g, b, a] = shape.fill;
            for (pixel, c) in out.as_chunks_mut::<4>().0.iter_mut().zip(cover) {
                if c == 0 {
                    continue;
                }
                let alpha = f32::from(c) / 255.0 * f32::from(a) / 255.0;
                blend(pixel, [r, g, b], alpha);
            }
        }
        out
    }

    /// 塗りを単色のカバレッジにラスタライズする（ボタンのアイコンなど、色はレンダリング側が掛ける）。
    pub fn rasterise_alpha(&self, width: usize, height: usize) -> Vec<u8> {
        let mut out = vec![0u8; width * height];
        for shape in &self.shapes {
            let cover = shape.outline.rasterise(width, height);
            let a = f32::from(shape.fill[3]) / 255.0;
            for (dst, c) in out.iter_mut().zip(cover) {
                let v = (f32::from(c) * a) as u32 + u32::from(*dst);
                *dst = v.min(255) as u8;
            }
        }
        out
    }
}

/// 「上に重ねる」（source over）。
fn blend(pixel: &mut [u8; 4], [r, g, b]: [u8; 3], alpha: f32) {
    let dst_a = f32::from(pixel[3]) / 255.0;
    let out_a = alpha + dst_a * (1.0 - alpha);
    if out_a <= 0.0 {
        return;
    }
    for (k, src) in [r, g, b].into_iter().enumerate() {
        let dst = f32::from(pixel[k]) * dst_a;
        let v = (f32::from(src) * alpha + dst * (1.0 - alpha)) / out_a;
        pixel[k] = v.round().clamp(0.0, 255.0) as u8;
    }
    pixel[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// 継承される塗りの状態。
#[derive(Debug, Clone, Copy)]
struct Paint {
    /// None は `fill="none"`。
    fill: Option<Rgba>,
    opacity: f32,
    transform: Transform,
}

fn collect(el: &Element, inherited: &Paint, viewbox: (f32, f32, f32, f32), out: &mut Vec<Shape>) {
    let mut paint = *inherited;
    // `style="fill:…; opacity:…"` と属性。属性は style より弱い（CSS の
    // 規則）が、放送局の画像はどちらか片方なので同じに扱う。
    let mut fill_attr = el.attr("fill").map(str::to_owned);
    let mut fill_opacity: Option<f32> = el.attr("fill-opacity").and_then(|v| v.trim().parse().ok());
    let mut opacity: Option<f32> = el.attr("opacity").and_then(|v| v.trim().parse().ok());
    if let Some(style) = el.attr("style") {
        for decl in style.split(';') {
            let Some((name, value)) = decl.split_once(':') else {
                continue;
            };
            match name.trim() {
                "fill" => fill_attr = Some(value.trim().to_owned()),
                "fill-opacity" => fill_opacity = value.trim().parse().ok(),
                "opacity" => opacity = value.trim().parse().ok(),
                _ => {}
            }
        }
    }
    if let Some(fill) = fill_attr {
        let fill = fill.trim();
        paint.fill = if fill == "none" {
            None
        } else {
            colour(fill).or(paint.fill)
        };
    }
    if let Some(o) = opacity {
        paint.opacity *= o.clamp(0.0, 1.0);
    }
    if let Some(t) = el.attr("transform") {
        paint.transform = Transform::parse(t).then(&paint.transform);
    }
    match el.name.as_str() {
        "svg" | "g" | "defs" | "symbol" => {
            for child in el.elements() {
                collect(child, &paint, viewbox, out);
            }
            return;
        }
        _ => {}
    }
    let steps = match shape_steps(el) {
        Some(steps) if !steps.is_empty() => steps,
        _ => return,
    };
    let Some(fill) = paint.fill else {
        return;
    };
    let alpha =
        f32::from(fill[3]) / 255.0 * paint.opacity * fill_opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    let outline = Outline { steps, viewbox }.transformed(&paint.transform);
    out.push(Shape {
        outline,
        fill: [fill[0], fill[1], fill[2], (alpha * 255.0).round() as u8],
    });
}

/// 図形の輪郭。
fn shape_steps(el: &Element) -> Option<Vec<Step>> {
    let get = |name: &str| el.attr(name).and_then(|v| v.trim().parse::<f32>().ok());
    Some(match el.name.as_str() {
        "path" => path(el.attr("d")?),
        "rect" => {
            let (x, y) = (get("x").unwrap_or(0.0), get("y").unwrap_or(0.0));
            let (w, h) = (get("width")?, get("height")?);
            let rx = get("rx").or_else(|| get("ry")).unwrap_or(0.0).min(w / 2.0);
            let ry = get("ry").or_else(|| get("rx")).unwrap_or(0.0).min(h / 2.0);
            if rx > 0.0 && ry > 0.0 {
                // 角の丸い矩形：角を四分の円（三次曲線の近似）で。
                const K: f32 = 0.5523;
                vec![
                    Step::Move((x + rx, y)),
                    Step::Line((x + w - rx, y)),
                    Step::Cubic(
                        (x + w - rx + rx * K, y),
                        (x + w, y + ry - ry * K),
                        (x + w, y + ry),
                    ),
                    Step::Line((x + w, y + h - ry)),
                    Step::Cubic(
                        (x + w, y + h - ry + ry * K),
                        (x + w - rx + rx * K, y + h),
                        (x + w - rx, y + h),
                    ),
                    Step::Line((x + rx, y + h)),
                    Step::Cubic(
                        (x + rx - rx * K, y + h),
                        (x, y + h - ry + ry * K),
                        (x, y + h - ry),
                    ),
                    Step::Line((x, y + ry)),
                    Step::Cubic((x, y + ry - ry * K), (x + rx - rx * K, y), (x + rx, y)),
                    Step::Close,
                ]
            } else {
                vec![
                    Step::Move((x, y)),
                    Step::Line((x + w, y)),
                    Step::Line((x + w, y + h)),
                    Step::Line((x, y + h)),
                    Step::Close,
                ]
            }
        }
        "circle" => ellipse(
            get("cx").unwrap_or(0.0),
            get("cy").unwrap_or(0.0),
            get("r")?,
            get("r")?,
        ),
        "ellipse" => ellipse(
            get("cx").unwrap_or(0.0),
            get("cy").unwrap_or(0.0),
            get("rx")?,
            get("ry")?,
        ),
        "polygon" | "polyline" => {
            let points = el.attr("points").map(numbers).unwrap_or_default();
            let mut out = Vec::new();
            for (i, pair) in points.as_chunks::<2>().0.iter().enumerate() {
                let p = (pair[0], pair[1]);
                out.push(if i == 0 { Step::Move(p) } else { Step::Line(p) });
            }
            out.push(Step::Close);
            out
        }
        _ => return None,
    })
}

/// 楕円を四つの三次曲線で。
fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<Step> {
    const K: f32 = 0.5523;
    vec![
        Step::Move((cx + rx, cy)),
        Step::Cubic(
            (cx + rx, cy + ry * K),
            (cx + rx * K, cy + ry),
            (cx, cy + ry),
        ),
        Step::Cubic(
            (cx - rx * K, cy + ry),
            (cx - rx, cy + ry * K),
            (cx - rx, cy),
        ),
        Step::Cubic(
            (cx - rx, cy - ry * K),
            (cx - rx * K, cy - ry),
            (cx, cy - ry),
        ),
        Step::Cubic(
            (cx + rx * K, cy - ry),
            (cx + rx, cy - ry * K),
            (cx + rx, cy),
        ),
        Step::Close,
    ]
}

/// SVG フォント（`<font><font-face/><glyph unicode d/></font>`）の文字ごとの
/// 輪郭。座標系は（送り幅 × (ascent + descent)）で、y は下向きに直してある。
pub fn font_glyphs(svg: &str) -> Vec<(char, Outline)> {
    let Some(root) = xml::parse(svg) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for font in root.find_all("font") {
        let face = font.find_all("font-face").into_iter().next();
        let number = |name: &str, or: f32| -> f32 {
            face.and_then(|f| f.attr(name))
                .and_then(|v| v.trim().parse::<f32>().ok())
                .unwrap_or(or)
        };
        let em = number("units-per-em", 1000.0).max(1.0);
        let ascent = number("ascent", em * 0.8);
        let descent = number("descent", em - ascent).abs();
        let advance = font
            .attr("horiz-adv-x")
            .and_then(|v| v.trim().parse::<f32>().ok())
            .unwrap_or(em);
        for glyph in font.find_all("glyph") {
            let Some(letter) = glyph
                .attr("unicode")
                .map(xml::decode)
                .and_then(|s| s.chars().next())
            else {
                continue;
            };
            let width = glyph
                .attr("horiz-adv-x")
                .and_then(|v| v.trim().parse::<f32>().ok())
                .unwrap_or(advance)
                .max(1.0);
            let Some(d) = glyph.attr("d") else {
                continue;
            };
            let steps = path(d);
            if steps.is_empty() {
                continue;
            }
            // フォントの y は上向き（ベースラインが 0、上が正）。下向きに。
            let flip = Transform([1.0, 0.0, 0.0, -1.0, 0.0, ascent]);
            let outline = Outline {
                steps,
                viewbox: (0.0, 0.0, width, (ascent + descent).max(1.0)),
            }
            .transformed(&flip);
            out.push((letter, outline));
        }
    }
    out
}

/// 小さな SVG の画像（受信機のボタンのアイコン）を、一辺 `side` のカバレッジにラスタライズする。
/// 中央に配置する（縦横が違えば短いほうを中央に寄せる）。
pub fn icon(svg: &str, side: usize) -> Option<Vec<u8>> {
    let drawing = Drawing::parse(svg)?;
    if drawing.shapes.is_empty() {
        return None;
    }
    let (ox, oy, w, h) = drawing.viewbox;
    let long = w.max(h);
    let dx = (long - w) / 2.0;
    let dy = (long - h) / 2.0;
    let centred = Drawing {
        viewbox: (ox - dx, oy - dy, long, long),
        shapes: drawing.shapes,
    };
    Some(centred.rasterise_alpha(side, side))
}

/// `#rgb`・`#rrggbb`・`#rrggbbaa`・`rgb(r,g,b)`・名前。
pub fn colour(text: &str) -> Option<Rgba> {
    let v = text.trim();
    if let Some(hex) = v.strip_prefix('#') {
        let n = u32::from_str_radix(hex, 16).ok()?;
        return Some(match hex.len() {
            3 => {
                let r = ((n >> 8) & 0xf) as u8;
                let g = ((n >> 4) & 0xf) as u8;
                let b = (n & 0xf) as u8;
                [r * 17, g * 17, b * 17, 255]
            }
            6 => [(n >> 16) as u8, (n >> 8) as u8, n as u8, 255],
            8 => [(n >> 24) as u8, (n >> 16) as u8, (n >> 8) as u8, n as u8],
            _ => return None,
        });
    }
    if let Some(inner) = v.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let n = numbers(inner);
        if n.len() >= 3 {
            return Some([n[0] as u8, n[1] as u8, n[2] as u8, 255]);
        }
    }
    Some(match v.to_ascii_lowercase().as_str() {
        "black" => [0, 0, 0, 255],
        "white" => [255, 255, 255, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 128, 0, 255],
        "lime" => [0, 255, 0, 255],
        "blue" => [0, 0, 255, 255],
        "yellow" => [255, 255, 0, 255],
        "cyan" | "aqua" => [0, 255, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255, 255],
        "gray" | "grey" => [128, 128, 128, 255],
        "silver" => [192, 192, 192, 255],
        "orange" => [255, 165, 0, 255],
        "transparent" => [0, 0, 0, 0],
        _ => return None,
    })
}

fn numbers(text: &str) -> Vec<f32> {
    text.split(|c: char| c.is_whitespace() || c == ',')
        .filter_map(|v| v.trim().parse::<f32>().ok())
        .collect()
}

/// SVG のパス（`d`）を読む。M・L・H・V・Q・T・C・S・A・Z と、その相対形。
/// A（円弧）は端点までの直線で近似する（フォントにも放送局の画像にも出ない）。
pub fn path(d: &str) -> Vec<Step> {
    let mut out = Vec::new();
    let mut numbers: Vec<f32> = Vec::new();
    let mut command = None::<char>;
    let mut current = (0.0f32, 0.0f32);
    let mut start = current;
    // 直前の制御点（T と S の反転に使う）。
    let mut last_quad = None::<(f32, f32)>;
    let mut last_cubic = None::<(f32, f32)>;
    let mut flush = |command: char, numbers: &mut Vec<f32>, out: &mut Vec<Step>| {
        let relative = command.is_ascii_lowercase();
        let upper = command.to_ascii_uppercase();
        let mut at = 0usize;
        let take = |at: &mut usize, n: usize| -> Option<Vec<f32>> {
            if *at + n > numbers.len() {
                return None;
            }
            let v = numbers[*at..*at + n].to_vec();
            *at += n;
            Some(v)
        };
        let abs = |current: (f32, f32), x: f32, y: f32| -> (f32, f32) {
            if relative {
                (current.0 + x, current.1 + y)
            } else {
                (x, y)
            }
        };
        let mut first = true;
        loop {
            match upper {
                'M' => {
                    let Some(v) = take(&mut at, 2) else { break };
                    let p = abs(current, v[0], v[1]);
                    if first {
                        out.push(Step::Move(p));
                        start = p;
                    } else {
                        out.push(Step::Line(p));
                    }
                    current = p;
                    last_quad = None;
                    last_cubic = None;
                }
                'L' => {
                    let Some(v) = take(&mut at, 2) else { break };
                    current = abs(current, v[0], v[1]);
                    out.push(Step::Line(current));
                    last_quad = None;
                    last_cubic = None;
                }
                'H' => {
                    let Some(v) = take(&mut at, 1) else { break };
                    current = (if relative { current.0 + v[0] } else { v[0] }, current.1);
                    out.push(Step::Line(current));
                    last_quad = None;
                    last_cubic = None;
                }
                'V' => {
                    let Some(v) = take(&mut at, 1) else { break };
                    current = (current.0, if relative { current.1 + v[0] } else { v[0] });
                    out.push(Step::Line(current));
                    last_quad = None;
                    last_cubic = None;
                }
                'Q' => {
                    let Some(v) = take(&mut at, 4) else { break };
                    let c = abs(current, v[0], v[1]);
                    let p = abs(current, v[2], v[3]);
                    out.push(Step::Quad(c, p));
                    current = p;
                    last_quad = Some(c);
                    last_cubic = None;
                }
                'T' => {
                    let Some(v) = take(&mut at, 2) else { break };
                    let c = last_quad
                        .map_or(current, |q| (2.0 * current.0 - q.0, 2.0 * current.1 - q.1));
                    let p = abs(current, v[0], v[1]);
                    out.push(Step::Quad(c, p));
                    current = p;
                    last_quad = Some(c);
                    last_cubic = None;
                }
                'C' => {
                    let Some(v) = take(&mut at, 6) else { break };
                    let c1 = abs(current, v[0], v[1]);
                    let c2 = abs(current, v[2], v[3]);
                    let p = abs(current, v[4], v[5]);
                    out.push(Step::Cubic(c1, c2, p));
                    current = p;
                    last_cubic = Some(c2);
                    last_quad = None;
                }
                'S' => {
                    let Some(v) = take(&mut at, 4) else { break };
                    let c1 = last_cubic
                        .map_or(current, |c| (2.0 * current.0 - c.0, 2.0 * current.1 - c.1));
                    let c2 = abs(current, v[0], v[1]);
                    let p = abs(current, v[2], v[3]);
                    out.push(Step::Cubic(c1, c2, p));
                    current = p;
                    last_cubic = Some(c2);
                    last_quad = None;
                }
                'A' => {
                    let Some(v) = take(&mut at, 7) else { break };
                    current = abs(current, v[5], v[6]);
                    out.push(Step::Line(current));
                    last_quad = None;
                    last_cubic = None;
                }
                'Z' => {
                    out.push(Step::Close);
                    current = start;
                    last_quad = None;
                    last_cubic = None;
                    break;
                }
                _ => break,
            }
            first = false;
            if at >= numbers.len() {
                break;
            }
        }
        numbers.clear();
    };
    let mut number = String::new();
    let push_number = |number: &mut String, numbers: &mut Vec<f32>| {
        if !number.is_empty() {
            if let Ok(v) = number.parse::<f32>() {
                numbers.push(v);
            }
            number.clear();
        }
    };
    for c in d.chars() {
        match c {
            'A'..='Z' | 'a'..='z' if c != 'e' && c != 'E' => {
                push_number(&mut number, &mut numbers);
                if let Some(previous) = command {
                    flush(previous, &mut numbers, &mut out);
                }
                command = Some(c);
                if c == 'Z' || c == 'z' {
                    flush(c, &mut numbers, &mut out);
                    command = None;
                }
            }
            '0'..='9' | 'e' | 'E' => number.push(c),
            '.' => {
                // `1.5.3` は `1.5` と `.3`。
                if number.contains('.') && !number.ends_with(['e', 'E']) {
                    push_number(&mut number, &mut numbers);
                }
                number.push(c);
            }
            '-' | '+' => {
                // 符号は数の先頭。指数の符号は続きのまま。
                if number.ends_with(['e', 'E']) {
                    number.push(c);
                } else {
                    push_number(&mut number, &mut numbers);
                    number.push(c);
                }
            }
            _ => push_number(&mut number, &mut numbers),
        }
    }
    push_number(&mut number, &mut numbers);
    if let Some(previous) = command {
        flush(previous, &mut numbers, &mut out);
    }
    out
}

/// 輪郭を線分にする。**曲線はピクセル単位の長さに応じて分割する**（三ピクセルに
/// 一つ、四〜六十四）。開いたパスも閉じる。座標はピクセル。
pub fn flatten(steps: &[Step]) -> Vec<[(f32, f32); 2]> {
    let mut out = Vec::new();
    let mut current = (0.0f32, 0.0f32);
    let mut start = current;
    let mut open = false;
    let close = |out: &mut Vec<[(f32, f32); 2]>, current: (f32, f32), start: (f32, f32)| {
        if current != start {
            out.push([current, start]);
        }
    };
    let pieces = |points: &[(f32, f32)]| -> usize {
        let length: f32 = points
            .windows(2)
            .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
            .sum();
        ((length / 3.0).ceil() as usize).clamp(4, 64)
    };
    for step in steps {
        match *step {
            Step::Move(p) => {
                if open {
                    close(&mut out, current, start);
                }
                current = p;
                start = p;
                open = true;
            }
            Step::Line(p) => {
                out.push([current, p]);
                current = p;
            }
            Step::Quad(c, p) => {
                let from = current;
                let n = pieces(&[from, c, p]);
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    let x = u * u * from.0 + 2.0 * u * t * c.0 + t * t * p.0;
                    let y = u * u * from.1 + 2.0 * u * t * c.1 + t * t * p.1;
                    out.push([current, (x, y)]);
                    current = (x, y);
                }
            }
            Step::Cubic(c1, c2, p) => {
                let from = current;
                let n = pieces(&[from, c1, c2, p]);
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    let x = u * u * u * from.0
                        + 3.0 * u * u * t * c1.0
                        + 3.0 * u * t * t * c2.0
                        + t * t * t * p.0;
                    let y = u * u * u * from.1
                        + 3.0 * u * u * t * c1.1
                        + 3.0 * u * t * t * c2.1
                        + t * t * t * p.1;
                    out.push([current, (x, y)]);
                    current = (x, y);
                }
            }
            Step::Close => {
                close(&mut out, current, start);
                current = start;
                open = false;
            }
        }
    }
    if open {
        close(&mut out, current, start);
    }
    out
}

/// 線分の閉路をラスタライズする（非ゼロ回転数規則）。
///
/// 行を `SUB` 本のスキャンラインに分け、各スキャンラインで閉路との交点を回転数の向き
/// 付きで集め、回転数がゼロでない区間をピクセルの幅で按分して足す。一ピクセルの
/// 値は 0〜255。
pub fn coverage(segments: &[[(f32, f32); 2]], width: usize, height: usize) -> Vec<u8> {
    const SUB: usize = 4;
    let mut cover = vec![0.0f32; width * height];
    let mut crossings: Vec<(f32, i32)> = Vec::new();
    // 行ごとに見る線分を絞る（文字は数百本、画像は数千本）。
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); height];
    for (i, [a, b]) in segments.iter().enumerate() {
        let (top, bottom) = (a.1.min(b.1), a.1.max(b.1));
        if top == bottom {
            continue;
        }
        let first = top.floor().max(0.0) as usize;
        let last = (bottom.ceil().max(0.0) as usize).min(height);
        for row in rows.iter_mut().take(last).skip(first) {
            row.push(i);
        }
    }
    for (row, listed) in rows.iter().enumerate() {
        if listed.is_empty() {
            continue;
        }
        for sub in 0..SUB {
            let y = row as f32 + (sub as f32 + 0.5) / SUB as f32;
            crossings.clear();
            for &i in listed {
                let [a, b] = segments[i];
                let (top, bottom, direction) = if a.1 < b.1 { (a, b, 1) } else { (b, a, -1) };
                // 上端は含み下端は含まない（頂点を二度数えない）。
                if y < top.1 || y >= bottom.1 {
                    continue;
                }
                let t = (y - top.1) / (bottom.1 - top.1);
                let x = top.0 + (bottom.0 - top.0) * t;
                crossings.push((x, direction));
            }
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut winding = 0i32;
            let mut from = 0.0f32;
            for &(x, direction) in &crossings {
                let was = winding;
                winding += direction;
                if was == 0 && winding != 0 {
                    from = x;
                } else if was != 0 && winding == 0 {
                    // [from, x) をピクセルに按分。
                    let start = from.max(0.0).min(width as f32);
                    let end = x.max(0.0).min(width as f32);
                    if end <= start {
                        continue;
                    }
                    let first = start.floor() as usize;
                    let last = (end.ceil() as usize).min(width);
                    for col in first..last {
                        let left = start.max(col as f32);
                        let right = end.min(col as f32 + 1.0);
                        if right > left {
                            cover[row * width + col] += (right - left) / SUB as f32;
                        }
                    }
                }
            }
        }
    }
    cover
        .into_iter()
        .map(|c| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 座標系いっぱいの四角は、どのサイズにラスタライズしても全部が塗られる。
    #[test]
    fn a_square_glyph_fills_its_box_at_any_size() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><font><font-face units-per-em="1024" ascent="880" descent="144"/><glyph unicode="&#xe054;" horiz-adv-x="1024" d="M0,-144 L1024,-144 L1024,880 L0,880 Z"/></font></defs></svg>"#;
        let glyphs = font_glyphs(svg);
        assert_eq!(glyphs.len(), 1);
        let (letter, outline) = &glyphs[0];
        assert_eq!(*letter, '\u{e054}');
        for (w, h) in [(96, 96), (144, 144), (72, 144)] {
            let bitmap = outline.rasterise(w, h);
            assert_eq!(
                bitmap.iter().filter(|&&v| v == 255).count(),
                w * h,
                "{w}x{h}"
            );
        }
    }

    /// 上半分だけの四角（曲線で書いたもの）は、上が塗られて下が透明。
    /// フォントの y は上向きなので、`ascent` に近いほうが上の行。
    #[test]
    fn the_upper_half_lands_in_the_upper_rows_and_curves_are_read() {
        let svg = r#"<svg><font><font-face units-per-em="1024" ascent="880" descent="144"/><glyph unicode="A" d="M0,368 Q512,368 1024,368 L1024,880 Q512,880 0,880 Z"/></font></svg>"#;
        let outline = &font_glyphs(svg)[0].1;
        let side = 96;
        let bitmap = outline.rasterise(side, side);
        let at = |x: usize, y: usize| bitmap[y * side + x];
        assert_eq!(at(side / 2, 10), 255);
        assert_eq!(at(side / 2, side - 10), 0);
    }

    /// 穴のある字形（外側の閉路と逆向きの内側の閉路）は、穴が透ける。
    #[test]
    fn a_hole_wound_the_other_way_is_left_open() {
        let svg = r#"<svg><font><font-face units-per-em="1024" ascent="880" descent="144"/><glyph unicode="O" d="M0,-144 L1024,-144 L1024,880 L0,880 Z M256,112 L256,624 L768,624 L768,112 Z"/></font></svg>"#;
        let outline = &font_glyphs(svg)[0].1;
        let side = 96;
        let bitmap = outline.rasterise(side, side);
        let at = |x: usize, y: usize| bitmap[y * side + x];
        assert_eq!(at(side / 2, side / 2), 0, "the hole");
        assert_eq!(at(4, 4), 255, "the ring");
    }

    /// ボタンのアイコン：`viewBox` の座標系を一辺に対応させ、y は下向き。上半分の矩形は
    /// 上の行に出る。
    #[test]
    fn an_icon_is_laid_in_its_box_top_down() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="0" y="0" width="24" height="12"/><path fill="none" d="M0,0 L24,24"/></svg>"#;
        let side = 32;
        let bitmap = icon(svg, side).expect("読める");
        assert_eq!(bitmap.len(), side * side);
        assert_eq!(bitmap[4 * side + 16], 255, "上は塗られる");
        assert_eq!(bitmap[28 * side + 16], 0, "下は透ける");
        let svg = r#"<svg viewBox="0 0 10 10"><circle cx="5" cy="5" r="4"/><polygon points="0,0 2,0 0,2"/></svg>"#;
        let bitmap = icon(svg, 20).expect("読める");
        assert_eq!(bitmap[10 * 20 + 10], 255);
        assert_eq!(bitmap[19 * 20 + 19], 0);
    }

    /// 色と変換と入れ子：赤い四角を `g` で右へ動かし、半透明の青を重ねる。
    #[test]
    fn colours_transforms_and_groups_make_a_picture() {
        let svg = r##"<svg viewBox="0 0 20 10"><g transform="translate(10,0)" fill="#ff0000"><rect x="0" y="0" width="10" height="10"/></g><rect x="0" y="0" width="5" height="10" style="fill:blue;fill-opacity:0.5"/><rect x="0" y="5" width="5" height="5" fill="none"/></svg>"##;
        let drawing = Drawing::parse(svg).expect("読める");
        assert_eq!(drawing.shapes.len(), 2);
        let rgba = drawing.rasterise(20, 10);
        let px = |x: usize, y: usize| &rgba[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4];
        assert_eq!(px(15, 5), &[255, 0, 0, 255]);
        assert_eq!(px(2, 5), &[0, 0, 255, 128]);
        assert_eq!(px(7, 5)[3], 0, "何も無いところは透明");
    }

    #[test]
    fn transforms_compose_like_svg() {
        let t = Transform::parse("translate(10,0) scale(2)");
        assert_eq!(t.apply((1.0, 1.0)), (12.0, 2.0));
        let r = Transform::parse("rotate(90)");
        let p = r.apply((1.0, 0.0));
        assert!((p.0).abs() < 1e-5 && (p.1 - 1.0).abs() < 1e-5);
        let m = Transform::parse("matrix(1 0 0 1 5 6)");
        assert_eq!(m.apply((0.0, 0.0)), (5.0, 6.0));
    }

    /// 相対コマンドと H・V・T・S も読める。
    #[test]
    fn relative_and_shorthand_steps_are_read() {
        let steps = path("m10,20 h30 v-5 l-10,10 q5,5 10,0 t10,0 c1,1 2,2 3,3 s1,1 2,2 z");
        assert_eq!(steps[0], Step::Move((10.0, 20.0)));
        assert_eq!(steps[1], Step::Line((40.0, 20.0)));
        assert_eq!(steps[2], Step::Line((40.0, 15.0)));
        assert_eq!(steps[3], Step::Line((30.0, 25.0)));
        assert_eq!(steps[4], Step::Quad((35.0, 30.0), (40.0, 25.0)));
        assert_eq!(steps[5], Step::Quad((45.0, 20.0), (50.0, 25.0)));
        assert_eq!(
            steps[6],
            Step::Cubic((51.0, 26.0), (52.0, 27.0), (53.0, 28.0))
        );
        assert_eq!(
            steps[7],
            Step::Cubic((54.0, 29.0), (54.0, 29.0), (55.0, 30.0))
        );
        assert_eq!(steps[8], Step::Close);
        // `1.5.3` は二つの数。
        let steps = path("M1.5.3L2 2");
        assert_eq!(steps[0], Step::Move((1.5, 0.3)));
    }
}

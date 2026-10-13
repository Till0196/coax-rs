// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The character encoding the broadcast tables spell names in.
//!
//! A name in a service table is not text in any encoding a computer already
//! had. It is a stream of graphic characters and the codes that say which of
//! four character sets each is to be read out of, which set is currently on
//! which half of the byte range, and how the result is to be drawn -- ARIB
//! STD-B24 part 2. So a service name arrives as bytes that mean nothing until
//! the sets they were sent under have been followed.
//!
//! What the sets hold is mostly not particular to broadcasting: the two-byte
//! set is JIS X 0208, so it is read out of an encoding that carries it -- see
//! [`kanji`]. The rows the broadcast added past the end of that standard are in
//! [`symbols`].
//!
//! Only the decoding is here. Nothing in this program writes a name back out.

mod symbols;

use encoding_rs::EUC_JP;

/// The character sets, by the number that designates them.
const ASCII: u8 = 0x40;
const ASCII_X: u8 = 0x4a;
const ASCII_PROP: u8 = 0x36;
const HIRAGANA: u8 = 0x30;
const HIRAGANA_PROP: u8 = 0x37;
const KATAKANA: u8 = 0x31;
const KATAKANA_PROP: u8 = 0x38;
const JISX0201_KATA: u8 = 0x49;
const KANJI: u8 = 0x42;
const JISX0213_1: u8 = 0x39;
const JISX0213_2: u8 = 0x3a;
const ADDITIONAL: u8 = 0x3b;
/// The sets a broadcast defines the shapes of itself, which arrive as pictures
/// elsewhere in the stream. Nothing here draws them.
const DRCS0: u8 = 0xc0;
const DRCS15: u8 = 0xcf;
/// The macro set (designated as `ESC 2B 20 70`), whose sixteen codes stand
/// for sequences of designations and shifts. A caption starts with it in
/// the fourth place (STD-B24 part 3, table 7-?); a name never uses it.
const MACRO: u8 = 0xf0;

/// The default macros (STD-B24 part 2, table 7-13): what code 0x60 to 0x6F
/// of the macro set expand to. Each designates the first three places and
/// the macro set into the fourth, then `LS0` and `LS2R`.
const MACROS: [&[u8]; 16] = [
    &[
        0x1b, 0x24, 0x42, 0x1b, 0x29, 0x4a, 0x1b, 0x2a, 0x30, 0x1b, 0x2b, 0x20, 0x70, 0x0f, 0x1b,
        0x7d,
    ],
    &[
        0x1b, 0x24, 0x42, 0x1b, 0x29, 0x31, 0x1b, 0x2a, 0x30, 0x1b, 0x2b, 0x20, 0x70, 0x0f, 0x1b,
        0x7d,
    ],
    &[
        0x1b, 0x24, 0x42, 0x1b, 0x29, 0x20, 0x41, 0x1b, 0x2a, 0x30, 0x1b, 0x2b, 0x20, 0x70, 0x0f,
        0x1b, 0x7d,
    ],
    &[
        0x1b, 0x28, 0x32, 0x1b, 0x29, 0x34, 0x1b, 0x2a, 0x35, 0x1b, 0x2b, 0x20, 0x70, 0x0f, 0x1b,
        0x7d,
    ],
    &[
        0x1b, 0x28, 0x32, 0x1b, 0x29, 0x33, 0x1b, 0x2a, 0x35, 0x1b, 0x2b, 0x20, 0x70, 0x0f, 0x1b,
        0x7d,
    ],
    &[
        0x1b, 0x28, 0x32, 0x1b, 0x29, 0x20, 0x41, 0x1b, 0x2a, 0x35, 0x1b, 0x2b, 0x20, 0x70, 0x0f,
        0x1b, 0x7d,
    ],
    &[
        0x1b, 0x28, 0x20, 0x41, 0x1b, 0x29, 0x20, 0x42, 0x1b, 0x2a, 0x20, 0x43, 0x1b, 0x2b, 0x20,
        0x70, 0x0f, 0x1b, 0x7d,
    ],
    &[
        0x1b, 0x28, 0x20, 0x44, 0x1b, 0x29, 0x20, 0x45, 0x1b, 0x2a, 0x20, 0x46, 0x1b, 0x2b, 0x20,
        0x70, 0x0f, 0x1b, 0x7d,
    ],
    &[
        0x1b, 0x28, 0x20, 0x47, 0x1b, 0x29, 0x20, 0x48, 0x1b, 0x2a, 0x20, 0x49, 0x1b, 0x2b, 0x20,
        0x70, 0x0f, 0x1b, 0x7d,
    ],
    &[
        0x1b, 0x28, 0x20, 0x4a, 0x1b, 0x29, 0x20, 0x4b, 0x1b, 0x2a, 0x20, 0x4c, 0x1b, 0x2b, 0x20,
        0x70, 0x0f, 0x1b, 0x7d,
    ],
    &[
        0x1b, 0x28, 0x20, 0x4d, 0x1b, 0x29, 0x20, 0x4e, 0x1b, 0x2a, 0x20, 0x4f, 0x1b, 0x2b, 0x20,
        0x70, 0x0f, 0x1b, 0x7d,
    ],
    &[
        0x1b, 0x24, 0x42, 0x1b, 0x29, 0x20, 0x42, 0x1b, 0x2a, 0x30, 0x1b, 0x2b, 0x20, 0x70, 0x0f,
        0x1b, 0x7d,
    ],
    &[
        0x1b, 0x24, 0x42, 0x1b, 0x29, 0x20, 0x43, 0x1b, 0x2a, 0x30, 0x1b, 0x2b, 0x20, 0x70, 0x0f,
        0x1b, 0x7d,
    ],
    &[
        0x1b, 0x24, 0x42, 0x1b, 0x29, 0x20, 0x44, 0x1b, 0x2a, 0x30, 0x1b, 0x2b, 0x20, 0x70, 0x0f,
        0x1b, 0x7d,
    ],
    &[
        0x1b, 0x28, 0x31, 0x1b, 0x29, 0x30, 0x1b, 0x2a, 0x4a, 0x1b, 0x2b, 0x20, 0x70, 0x0f, 0x1b,
        0x7d,
    ],
    &[
        0x1b, 0x28, 0x4a, 0x1b, 0x29, 0x32, 0x1b, 0x2a, 0x20, 0x41, 0x1b, 0x2b, 0x20, 0x70, 0x0f,
        0x1b, 0x7d,
    ],
];

/// The control codes that are acted on rather than skipped.
const LS0: u8 = 0x0f;
const LS1: u8 = 0x0e;
const SS2: u8 = 0x19;
const SS3: u8 = 0x1d;
const ESC: u8 = 0x1b;
/// A line break and a carriage return, which a two-line name is written with.
const APD: u8 = 0x0a;
const APR: u8 = 0x0d;

/// The escapes that move a set onto one half of the byte range.
const LS2_ESC: u8 = 0x6e;
const LS3_ESC: u8 = 0x6f;
const LS1R_ESC: u8 = 0x7e;
const LS2R_ESC: u8 = 0x7d;
const LS3R_ESC: u8 = 0x7c;

/// The punctuation both kana sets end with, which is not where the kana are.
const KANA_MARKS: [char; 8] = ['ヽ', 'ヾ', 'ー', '。', '「', '」', '、', '・'];
const HIRAGANA_MARKS: [char; 2] = ['ゝ', 'ゞ'];

/// A decoded name, and whether it is all of what was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub text: String,
    /// Set where something was dropped: a character of a set nothing here
    /// draws, or bytes that were not a character at all. A name that lost
    /// something is still worth having, but a caller recording it may want to
    /// keep the bytes as well.
    pub lossy: bool,
}

/// The text of a name, as far as it can be read.
pub fn decode(bytes: &[u8]) -> String {
    read(bytes).text
}

/// The same, with whether anything was lost along the way.
pub fn read(bytes: &[u8]) -> Decoded {
    let mut text = String::new();
    let mut lossy = false;
    // **A space is a full-width space unless the size has been halved.** A
    // string starts at the normal size, and a space there occupies a
    // full-width cell; after MSZ or SSZ it is a half-width one. Mirakurun
    // (aribts's `TsChar`) reads it the same way, and a title read here has
    // to be the same string as the one it hands over, or the two keep
    // replacing each other.
    let mut normal = true;
    for piece in Decoder::new(true).run(bytes) {
        match piece {
            // **The alphanumerics are full-width at the normal size too**,
            // for the same reason as the space: aribts puts its ASCII set
            // through a full-width table unless MSZ has halved it, so a
            // "(解)" read here has to come out "（解）" or it is a different
            // string from Mirakurun's and the titles keep flickering between
            // the two spellings.
            Piece::Char(letter, width) => text.push(if normal && width == 1 {
                full_width(letter)
            } else {
                letter
            }),
            Piece::Space => text.push(if normal { '\u{3000}' } else { ' ' }),
            Piece::Control(code) => match code[0] {
                APD => text.push('\n'),
                APR => text.push('\r'),
                // SSZ, MSZ: smaller; NSZ: normal; SZX: normal unless its
                // parameter says tiny.
                0x88 | 0x89 => normal = false,
                0x8a => normal = true,
                0x8b => normal = code.get(1) != Some(&0x60),
                _ => {}
            },
            Piece::Drcs { .. } | Piece::Lost => lossy = true,
        }
    }
    Decoded { text, lossy }
}

/// The full-width form of a printable ASCII character, as aribts's table
/// (Shift-JIS `0x81xx` / `0x82xx`) reads it. Anything else is itself.
fn full_width(letter: char) -> char {
    match letter {
        '"' => '\u{201d}',
        '\'' => '\u{2019}',
        // The set's own yen sign and overline (ARIB's alphanumerics put
        // them where ASCII has the backslash and the tilde).
        '\\' | '\u{a5}' => '\u{ffe5}',
        '`' => '\u{2018}',
        '~' | '\u{203e}' => '\u{ffe3}',
        '!'..='}' => char::from_u32(letter as u32 - 0x21 + 0xff01).unwrap_or(letter),
        _ => letter,
    }
}

/// One thing the eight-unit coding said: a character, or a control code
/// with its parameters. What a caption decoder reads the stream as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece<'a> {
    /// A character, and whether it came from a two-byte set (a full-width
    /// cell) or a one-byte one (half).
    Char(char, usize),
    Space,
    /// A character of a set of shapes the broadcast sent itself: the set's
    /// number (0 to 15) and the code within it (one or two bytes).
    Drcs {
        set: u8,
        code: u16,
        width: usize,
    },
    /// A control code, with the bytes it takes with it (the code first).
    Control(&'a [u8]),
    /// Something that was not a character.
    Lost,
}

/// The stream broken into what it says, in order. **The sets and the shifts
/// are followed here**; what the control codes mean for the layout is the
/// reader's.
pub fn pieces(bytes: &[u8]) -> Vec<Piece<'_>> {
    let mut decoder = Decoder::new(false);
    // A caption starts with the macro set in the fourth place, and the
    // documents lean on it: `SS3 61` is the usual way of switching the
    // second place to katakana.
    decoder.sets[3] = MACRO;
    decoder.run(bytes)
}

/// The stream broken into what it says, as a one-segment (C profile)
/// caption is coded: **the low half is fixed to the first one-byte DRCS set
/// and the high half to the kanji set, and nothing in the stream changes
/// them** (ARIB TR-B14 Part 3, 6.4.1). Read as an ordinary caption, where
/// the high half starts as hiragana, every kanji came out as two hiragana.
pub fn pieces_c_profile(bytes: &[u8]) -> Vec<Piece<'_>> {
    let mut decoder = Decoder::new(false);
    decoder.initial = ([DRCS0 + 1, ASCII, KANJI, MACRO], 0, 2);
    (decoder.sets, decoder.low, decoder.high) = decoder.initial;
    decoder.run(bytes)
}

/// Puts words where the symbols that cannot be drawn were. The symbols a
/// broadcast marks a programme with are characters like any other, and where
/// the font has them they should stay characters; where it does not, a box is
/// worse than the word.
pub fn substitute(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for letter in text.chars() {
        match symbols::SUBSTITUTES
            .iter()
            .find(|(symbol, _)| *symbol == letter)
        {
            Some((_, said)) => out.push_str(said),
            None => out.push(letter),
        }
    }
    out
}

/// Which set is on which half of the byte range, as the string has said so far.
/// The four sets a string can reach are held together: the codes that switch
/// them name a place in this table rather than a set, so a designation part way
/// through a name changes what the bytes after it mean without the bytes
/// themselves changing.
#[derive(Debug)]
struct Decoder {
    sets: [u8; 4],
    /// Which of them the low half of the byte range reads out of, and which
    /// the high half.
    low: usize,
    high: usize,
    /// The set the next character alone is to be read out of, where one was
    /// called for. Zero for none: the table's first entry is never single
    /// shifted to.
    once: usize,
    /// Whether a new line (APD) puts the sets back to what a string starts
    /// in. A name in a table is written that way; a caption is not (APD
    /// only moves the cursor there).
    reset_on_newline: bool,
    /// What a clear screen (CS) puts the sets back to. A caption's is the
    /// macro set in the fourth place; a one-segment caption's is fixed and
    /// never changes (TR-B14 Part 3, 6.4.1).
    initial: ([u8; 4], usize, usize),
}

impl Decoder {
    fn new(reset_on_newline: bool) -> Self {
        Self {
            reset_on_newline,
            // What a string starts in, which a string that says nothing else
            // stays in.
            sets: [KANJI, ASCII, HIRAGANA, KATAKANA],
            low: 0,
            high: 2,
            once: 0,
            initial: ([KANJI, ASCII, HIRAGANA, MACRO], 0, 2),
        }
    }

    fn run<'a>(mut self, bytes: &'a [u8]) -> Vec<Piece<'a>> {
        let mut out = Vec::new();
        self.run_into(bytes, &mut out);
        out
    }

    fn run_into<'a>(&mut self, bytes: &'a [u8], out: &mut Vec<Piece<'a>>) {
        let mut at = 0;
        while at < bytes.len() {
            at += self.step(bytes, at, out);
        }
    }

    /// A code of the macro set: run what it stands for. Only the default
    /// macros are known (a broadcast can define its own with MACRO 0x95;
    /// those are skipped as a control code).
    fn macro_code<'a>(&mut self, code: u8, out: &mut Vec<Piece<'a>>) {
        if let Some(body) = MACROS.get(usize::from(code.wrapping_sub(0x60))) {
            // The body is static, so its pieces (all controls, no text) can
            // be dropped: what matters is the state they leave behind.
            let mut scratch: Vec<Piece<'static>> = Vec::new();
            self.run_into(body, &mut scratch);
            let _ = out;
        }
    }

    fn step<'a>(&mut self, bytes: &'a [u8], at: usize, out: &mut Vec<Piece<'a>>) -> usize {
        let byte = bytes[at];
        match byte {
            // Padding rather than characters: a table's fields are a fixed
            // length and the name in one is not.
            0x00 | 0x7f | 0xff => 1,
            ESC => self.escape(bytes, at),
            0x20 | 0xa0 => {
                out.push(Piece::Space);
                self.once = 0;
                1
            }
            _ if byte < 0x20 || (0x80..=0x9f).contains(&byte) => self.control(bytes, at, out),
            _ => self.graphic(bytes, at, out),
        }
    }

    fn graphic<'a>(&mut self, bytes: &'a [u8], at: usize, out: &mut Vec<Piece<'a>>) -> usize {
        let byte = bytes[at];
        let which = match (self.once, byte & 0x80) {
            (0, 0) => self.low,
            (0, _) => self.high,
            (once, _) => once,
        };
        self.once = 0;
        let set = self.sets[which];
        if set == MACRO {
            self.macro_code(byte & 0x7f, out);
            return 1;
        }
        if !two_byte(set) {
            out.push(Self::character(set, u16::from(byte & 0x7f), 1));
            return 1;
        }
        let Some(second) = bytes.get(at + 1) else {
            // The last byte of the field is the first half of a character:
            // the name was cut short.
            out.push(Piece::Lost);
            return 1;
        };
        // Both halves come from the same side of the byte range. One from each
        // is not a character but the string having gone wrong.
        if (second ^ byte) & 0x80 != 0 {
            out.push(Piece::Lost);
            return 1;
        }
        let cell = u16::from(byte & 0x7f) << 8 | u16::from(second & 0x7f);
        out.push(Self::character(set, cell, 2));
        2
    }

    fn character(set: u8, cell: u16, width: usize) -> Piece<'static> {
        if (DRCS0..=DRCS15).contains(&set) {
            // Drawn from shapes sent elsewhere in the stream. A caption
            // decoder gathers those; a name reader cannot draw them.
            return Piece::Drcs {
                set: set & 0x0f,
                code: cell,
                width,
            };
        }
        let (first, second) = if width == 1 {
            (cell as u8, 0)
        } else {
            ((cell >> 8) as u8, cell as u8)
        };
        match character(set, first, second) {
            Some(letter) => Piece::Char(letter, width),
            None => Piece::Lost,
        }
    }

    fn escape(&mut self, bytes: &[u8], at: usize) -> usize {
        let Some(&code) = bytes.get(at + 1) else {
            return 1;
        };
        match code {
            LS2_ESC | LS3_ESC => {
                self.low = if code == LS2_ESC { 2 } else { 3 };
                self.once = 0;
                2
            }
            LS1R_ESC | LS2R_ESC | LS3R_ESC => {
                self.high = match code {
                    LS1R_ESC => 1,
                    LS2R_ESC => 2,
                    _ => 3,
                };
                self.once = 0;
                2
            }
            // A two-byte set, either straight into the first place or into a
            // named one.
            0x24 => self.designate_wide(bytes, at),
            0x28..=0x2b => self.designate(bytes, at, usize::from(code - 0x28), 2),
            _ => 2,
        }
    }

    /// A single-byte set into one of the four places.
    fn designate(&mut self, bytes: &[u8], at: usize, place: usize, offset: usize) -> usize {
        let Some(&code) = bytes.get(at + offset) else {
            return bytes.len() - at;
        };
        // The one that says a set of shapes the broadcast defines itself
        // follows, which is two bytes rather than one.
        if code == 0x20 {
            let Some(&which) = bytes.get(at + offset + 1) else {
                return bytes.len() - at;
            };
            if which == 0x70 || (0x41..=0x4f).contains(&which) {
                self.sets[place] = which | 0x80;
            }
            return offset + 2;
        }
        if single_byte_set(code) {
            self.sets[place] = code;
        }
        offset + 1
    }

    /// A two-byte set, which is designated with a code of its own in front.
    fn designate_wide(&mut self, bytes: &[u8], at: usize) -> usize {
        let Some(&code) = bytes.get(at + 2) else {
            return bytes.len() - at;
        };
        if two_byte_set(code) {
            self.sets[0] = code;
            return 3;
        }
        if !(0x28..=0x2b).contains(&code) {
            return 3;
        }
        let place = usize::from(code - 0x28);
        let Some(&which) = bytes.get(at + 3) else {
            return bytes.len() - at;
        };
        if which == 0x20 {
            if bytes.get(at + 4) == Some(&0x40) {
                self.sets[place] = DRCS0;
            }
            return 5;
        }
        if two_byte_set(which) {
            self.sets[place] = which;
        }
        4
    }

    /// A control code: acted on where it says something a name keeps, skipped
    /// over where it says how to draw. Skipped over rather than skipped: a
    /// control code takes its parameters with it, and a reader that stepped one
    /// byte would read those as characters.
    fn control<'a>(&mut self, bytes: &'a [u8], at: usize, out: &mut Vec<Piece<'a>>) -> usize {
        match bytes[at] {
            LS0 => {
                self.low = 0;
                self.once = 0;
                return 1;
            }
            LS1 => {
                self.low = 1;
                self.once = 0;
                return 1;
            }
            SS2 => {
                self.once = 2;
                return 1;
            }
            SS3 => {
                self.once = 3;
                return 1;
            }
            APD if self.reset_on_newline => {
                // A name written on two lines. The state goes back to what a
                // string starts in, as a new line does.
                self.sets = [KANJI, ASCII, HIRAGANA, KATAKANA];
                self.low = 0;
                self.high = 2;
            }
            // CS (clear screen) in a caption puts the sets back as well.
            0x0c if !self.reset_on_newline => {
                (self.sets, self.low, self.high) = self.initial;
            }
            _ => {}
        }
        self.once = 0;
        let length = control_length(bytes, at);
        out.push(Piece::Control(&bytes[at..at + length]));
        length
    }
}

/// How many bytes a control code takes with it.
fn control_length(bytes: &[u8], at: usize) -> usize {
    let rest = bytes.len() - at;
    let clamp = |how_many: usize| how_many.min(rest);
    match bytes[at] {
        // One parameter each: where to move to, how large to draw, which
        // colour, whether to flash, and how many times to repeat.
        0x16 | 0x8b | 0x91 | 0x93 | 0x94 | 0x97 | 0x98 => clamp(2),
        // A position, which is two.
        0x1c => clamp(3),
        // A colour, which takes a second byte only when it names a palette.
        0x90 | 0x92 => {
            if rest > 1 && bytes[at + 1] == 0x20 {
                clamp(3)
            } else {
                clamp(2)
            }
        }
        // A macro, which runs to the code that closes it.
        0x95 => {
            for step in 1..rest {
                if bytes[at + step] == 0x95 {
                    return clamp(step + 2);
                }
            }
            rest
        }
        // A sequence with as many parameters as it likes, ended by the letter
        // that says which sequence it was.
        0x9b => {
            let mut length = 1;
            while length < rest && (0x20..=0x3f).contains(&bytes[at + length]) {
                length += 1;
            }
            if length < rest && (0x40..=0x7e).contains(&bytes[at + length]) {
                length += 1;
            }
            length
        }
        // A time, which is either a wait of one byte or a clock reading that
        // runs to its own closing byte.
        0x9d => {
            if rest > 1 && bytes[at + 1] == 0x20 {
                return clamp(3);
            }
            let mut length = 1;
            while length < rest && (0x28..=0x3b).contains(&bytes[at + length]) {
                let ends = bytes[at + length] == 0x29;
                length += 1;
                if ends {
                    break;
                }
            }
            length
        }
        _ => 1,
    }
}

/// Whether a set is read two bytes at a time.
fn two_byte(set: u8) -> bool {
    matches!(set, KANJI | JISX0213_1 | JISX0213_2 | ADDITIONAL | DRCS0)
}

/// Whether a code names a set that may be designated as a single-byte one. A
/// code that names nothing leaves the place as it was: a set nobody knows is
/// not a reason to read the rest of the name out of the wrong one.
fn single_byte_set(code: u8) -> bool {
    matches!(code, ASCII | ASCII_X | JISX0201_KATA) || (HIRAGANA..=KATAKANA_PROP).contains(&code)
}

fn two_byte_set(code: u8) -> bool {
    matches!(code, KANJI | JISX0213_1 | JISX0213_2 | ADDITIONAL)
}

/// One character of one set.
fn character(set: u8, first: u8, second: u8) -> Option<char> {
    match set {
        ASCII | ASCII_X | ASCII_PROP => match first {
            // Where the standard's own alphabet differs from the one it was
            // taken from.
            0x5c => Some('¥'),
            0x7e => Some('‾'),
            0x21..=0x7d => Some(char::from(first)),
            _ => None,
        },
        KATAKANA | KATAKANA_PROP => match first {
            0x21..=0x76 => char::from_u32(0x3080 + u32::from(first)),
            0x77..=0x7e => Some(KANA_MARKS[usize::from(first - 0x77)]),
            _ => None,
        },
        HIRAGANA | HIRAGANA_PROP => match first {
            0x21..=0x73 => char::from_u32(0x3020 + u32::from(first)),
            0x77 | 0x78 => Some(HIRAGANA_MARKS[usize::from(first - 0x77)]),
            0x79..=0x7e => Some(KANA_MARKS[usize::from(first - 0x77)]),
            _ => None,
        },
        // The half-width kana, which are where they are in every encoding
        // that has them.
        JISX0201_KATA => match first {
            0x21..=0x5f => char::from_u32(0xff40 + u32::from(first)),
            _ => None,
        },
        KANJI | ADDITIONAL | JISX0213_1 | JISX0213_2 => wide(first, second),
        _ => None,
    }
}

/// One character of a two-byte set, by cell: the two bytes on the low half
/// of the range, as JIS numbers them. **EUC-JP is the same cells with the
/// high bit set**, so a data broadcast's documents (STD-B24 Volume 2, which
/// writes them in EUC-JP with the added rows) are read through here.
pub fn wide(first: u8, second: u8) -> Option<char> {
    if !(0x21..=0x7e).contains(&first) || !(0x21..=0x7e).contains(&second) {
        return None;
    }
    let cell = u16::from(first) << 8 | u16::from(second);
    if let Some(letter) = added(cell) {
        return Some(letter);
    }
    if let Some((_, letter)) = symbols::ALIASES.iter().find(|(at, _)| *at == cell) {
        return Some(*letter);
    }
    if let Some((_, mark)) = symbols::COMBINING.iter().find(|(at, _)| *at == cell) {
        return Some(*mark);
    }
    kanji(first, second)
}

/// One of the rows the broadcast added past the end of the standard's own.
fn added(cell: u16) -> Option<char> {
    for (first, run) in symbols::ADDED {
        let Some(step) = cell.checked_sub(*first) else {
            continue;
        };
        // A run is written as consecutive cells, and a cell of a row is a byte
        // of the low half: a run never crosses into the next row, so counting
        // along it is counting cells.
        if let Some(letter) = run.chars().nth(usize::from(step)) {
            // A hole in the run: a cell nothing was assigned to.
            return (letter != '\u{0}').then_some(letter);
        }
    }
    None
}

/// One character of JIS X 0208, read out of an encoding that carries it. The
/// cell numbering here and EUC-JP's are the same table with a different bias,
/// so the one is the other with the high bit set on both bytes. The rows past
/// the standard's own are what [`added`] is for.
fn kanji(first: u8, second: u8) -> Option<char> {
    if !(0x21..=0x74).contains(&first) {
        return None;
    }
    let euc = [first | 0x80, second | 0x80];
    let (text, wrong) = EUC_JP.decode_without_bom_handling(&euc);
    if wrong {
        return None;
    }
    let mut letters = text.chars();
    let letter = letters.next()?;
    letters.next().is_none().then_some(letter)
}

#[cfg(test)]
mod tests {
    use super::{decode, read, substitute};

    /// A string starts with kanji on the low half of the byte range and
    /// hiragana on the high, and stays there until it says otherwise. So the
    /// bytes of a name are not its letters: the same byte is a different
    /// character depending on what came before it.
    #[test]
    fn a_name_is_read_in_the_sets_a_string_starts_in() {
        // 衛星, whose cells are read out of the two-byte set the string starts
        // with, and then one byte of the single-byte set on the high half,
        // which is あ.
        let bytes = [0x31, 0x52, 0x40, 0x31, 0xa2];
        assert_eq!(decode(&bytes), "衛星あ");
    }

    /// The alphabet is on a set of its own, which a name reaches by moving it
    /// onto the low half of the byte range and moving it off again.
    #[test]
    fn the_alphabet_is_a_set_the_name_moves_to() {
        // LS1 for the alphabet, "SAMPLE", LS0 back, then 衛星.
        let bytes = [
            0x0e, b'S', b'A', b'M', b'P', b'L', b'E', 0x0f, 0x31, 0x52, 0x40, 0x31,
        ];
        assert_eq!(decode(&bytes), "ＳＡＭＰＬＥ衛星");
    }

    /// A set designated part way through a name changes what the bytes after
    /// it mean, without those bytes changing.
    #[test]
    fn a_set_designated_part_way_through_changes_what_follows() {
        // The same two bytes, read as kanji and then as hiragana: ESC ( 0
        // designates hiragana into the place the low half is reading out of.
        assert_eq!(decode(&[0x24, 0x22]), "あ");
        let bytes = [0x1b, 0x28, 0x30, 0x24, 0x22];
        assert_eq!(decode(&bytes), "いあ");
    }

    /// The single shift reaches a set for one character and then goes back,
    /// which is how one kana sits in a name that is otherwise kanji.
    #[test]
    fn a_single_shift_lasts_one_character() {
        // SS2 reaches the third place, which a string starts with hiragana in;
        // the two bytes after it are a kanji cell again.
        let bytes = [0x19, 0x22, 0x31, 0x52];
        assert_eq!(decode(&bytes), "あ衛");
    }

    /// The rows the broadcast added past the end of the standard are read as
    /// the characters they were assigned, which no general table has.
    #[test]
    fn the_added_rows_are_read() {
        assert_eq!(decode(&[0x7c, 0x21]), "➡");
        assert_eq!(decode(&[0x7a, 0x4d]), "⒑");
        // A cell of those rows that stands for a character the standard
        // already has.
        assert_eq!(decode(&[0x7c, 0x27]), "年");
    }

    /// A control code takes its parameters with it. A reader that stepped one
    /// byte would read the parameters as characters and put them in the name.
    #[test]
    fn a_control_code_takes_its_parameters_with_it() {
        // The alphabet, a colour change with one parameter, and then "AB".
        let bytes = [0x0e, 0x90, 0x40, b'A', b'B'];
        assert_eq!(decode(&bytes), "ＡＢ");
    }

    /// Padding is not a character. A table's field is a fixed length and the
    /// name in it is not.
    #[test]
    fn padding_is_not_a_character() {
        assert_eq!(decode(&[0x0e, b'A', 0x00, 0xff, b'B']), "ＡＢ");
    }

    /// A name that lost something says so, so that a caller can keep the bytes
    /// as well -- and a viewer still gets the rest of the name.
    #[test]
    fn a_name_that_lost_something_says_so() {
        assert!(!read(&[0x0e, b'A']).lossy);
        // A set of shapes the broadcast defines itself, which nothing here
        // draws: ESC $ ( SP @ designates it into the place the low half reads
        // out of.
        let bytes = [0x1b, 0x24, 0x28, 0x20, 0x40, 0x21, 0x21];
        let read = read(&bytes);
        assert!(read.lossy);
        assert!(read.text.is_empty());
    }

    /// Where a symbol cannot be drawn, the word it is read as is better than
    /// the box a font puts there.
    #[test]
    fn a_symbol_that_cannot_be_drawn_is_written_out() {
        assert_eq!(substitute("\u{1f201}です"), "[ココ]です");
        assert_eq!(substitute("SAMPLE"), "SAMPLE");
    }
}

#[cfg(test)]
mod c_profile_tests {
    use super::*;

    /// A one-segment caption reads its high half as kanji from the start.
    #[test]
    fn a_c_profile_caption_reads_kanji_without_a_designation() {
        let pieces = pieces_c_profile(&[0x0c, 0x87, 0x8a, 0xa4, 0xb7, 0xa4, 0xe3, 0xa4, 0xea]);
        let text: String = pieces
            .iter()
            .filter_map(|piece| match piece {
                Piece::Char(letter, _) => Some(*letter),
                _ => None,
            })
            .collect();
        assert_eq!(text, "しゃり");
    }
}

#[cfg(test)]
mod space_tests {
    use super::*;

    /// A space is full-width at the normal size and half-width after MSZ,
    /// which is how Mirakurun (aribts) reads it too.
    #[test]
    fn a_space_is_full_width_unless_the_size_was_halved() {
        // LS1 puts the alphanumerics on the low half.
        assert_eq!(read(b"\x0eA B").text, "Ａ\u{3000}Ｂ");
        assert_eq!(read(b"\x0e\x89A B\x8aC D").text, "A BＣ\u{3000}Ｄ");
    }

    /// The alphanumerics go the same way as the space, with the five
    /// characters whose full-width form is not at `U+FF01 + n`.
    #[test]
    fn the_alphanumerics_are_full_width_at_the_normal_size() {
        assert_eq!(read(b"\x0e(A)").text, "（Ａ）");
        assert_eq!(read(b"\x0e\x89(A)").text, "(A)");
        assert_eq!(
            read(b"\x0e\x22\x27\x5c\x60\x7e").text,
            "\u{201d}\u{2019}\u{ffe5}\u{2018}\u{ffe3}"
        );
    }
}

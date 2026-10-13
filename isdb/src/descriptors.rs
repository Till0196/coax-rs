// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The descriptors a receiver reads to know what a stream is and what a
//! programme is, in both the transport stream (ARIB STD-B10) and MMT-SI
//! (ARIB STD-B60) forms.
//!
//! The two transmissions say the same things in different shapes -- a wider
//! tag, a wider length, UTF-8 where the older one has the eight-unit code.
//! Each reader here comes in a pair (`x` for the transport stream, `mh_x` for
//! MMT) that gives back the same struct, so that whoever uses the answer
//! never has to know which transmission it came from.

use crate::mmttables;
use crate::tstables;

/// The text of a transport stream table, read out of ARIB's eight-unit code.
///
/// **It is neither ASCII nor UTF-8**: 「日本語」 arrives as `467c4b5c386c`,
/// three two-byte characters of the kanji set, and handing those bytes on as
/// they are gives a viewer four Latin letters and a shrug. The marks ARIB
/// puts in the extra symbol set (`[4K]` and the like) come out as
/// [`crate::arib::substitute`] writes them. Empty where there was nothing.
pub fn text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    crate::arib::substitute(&crate::arib::decode(bytes))
        .trim()
        .to_owned()
}

/// The text of an MMT-SI table, which is UTF-8 by definition. Bytes that
/// are not are replaced rather than failing the whole name.
pub fn utf8(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_owned()
}

/// What the transmission calls a stream, out of the text a descriptor ends
/// with. Nothing where the text was left out.
fn named(bytes: &[u8]) -> Option<String> {
    let text = text(bytes);
    (!text.is_empty()).then_some(text)
}

fn mh_named(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_matches(|letter: char| letter.is_control() || letter == '\u{fffd}');
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Three letters, as the standard writes a language.
fn language(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| char::from(*byte))
        .filter(|letter| letter.is_ascii_alphabetic())
        .collect()
}

/// The transport stream's tags (ARIB STD-B10 Part 2, Table 6-1).
pub mod tag {
    /// The conditional access descriptor.
    pub const CONDITIONAL_ACCESS: u8 = 0x09;
    /// The service descriptor: kind, provider and name.
    /// Service list (STD-B10 6.2.14): identifier and type, three bytes each.
    pub const SERVICE_LIST: u8 = 0x41;
    pub const SERVICE: u8 = 0x48;
    /// The short event descriptor: title and a line of text.
    pub const SHORT_EVENT: u8 = 0x4d;
    /// The extended event descriptor: headed items, over several descriptors.
    pub const EXTENDED_EVENT: u8 = 0x4e;
    /// The component descriptor: what a picture is.
    pub const COMPONENT: u8 = 0x50;
    /// The stream identifier descriptor: the component tag.
    pub const STREAM_IDENTIFIER: u8 = 0x52;
    /// The content descriptor: genres.
    pub const CONTENT: u8 = 0x54;
    /// Parental rating (STD-B10 6.2.11).
    pub const PARENTAL_RATING: u8 = 0x55;
    /// ARIB's hierarchical transmission descriptor: the rain-fade pair.
    /// Network identification (STD-B10 6.2.25), in the SIT of a partial
    /// transport stream.
    pub const NETWORK_IDENTIFICATION: u8 = 0xc2;
    /// Partial transport stream time (STD-B10 6.2.26), in the SIT.
    pub const PARTIAL_TS_TIME: u8 = 0xc3;
    /// Partial transport stream (STD-B10 6.2.20, DVB), in the SIT.
    pub const PARTIAL_TS: u8 = 0x63;
    /// Video decode control (STD-B10 6.2.30), in the PMT.
    pub const VIDEO_DECODE_CONTROL: u8 = 0xc8;
    /// Download content (STD-B21), in the SDTT.
    pub const DOWNLOAD_CONTENT: u8 = 0xc9;
    /// Access control (STD-B61): the ECM/EMM of the newer conditional
    /// access, in the PMT and CAT beside the CA descriptor.
    pub const ACCESS_CONTROL: u8 = 0xf6;
    /// Hyperlink (STD-B10 6.2.29).
    pub const HYPERLINK: u8 = 0xc5;
    /// Target region (STD-B10 6.2.27).
    pub const TARGET_REGION: u8 = 0xc6;
    pub const HIERARCHICAL_TRANSMISSION: u8 = 0xc0;
    /// Digital copy control (STD-B10 6.2.23).
    pub const DIGITAL_COPY_CONTROL: u8 = 0xc1;
    /// ARIB's audio component descriptor: what a sound is.
    pub const AUDIO_COMPONENT: u8 = 0xc4;
    /// ARIB's data content descriptor: what data goes with the event.
    pub const DATA_CONTENT: u8 = 0xc7;
    /// CA_EMM_TS (STD-B25).
    pub const CA_EMM_TS: u8 = 0xca;
    /// CA contract information (STD-B25).
    pub const CA_CONTRACT_INFO: u8 = 0xcb;
    /// Extended broadcaster (STD-B10 6.2.43), in the BIT.
    pub const EXTENDED_BROADCASTER: u8 = 0xce;
    /// Logo transmission (STD-B10 6.2.44), in the SDT.
    pub const LOGO_TRANSMISSION: u8 = 0xcf;
    /// Series (STD-B10 6.2.33).
    pub const SERIES: u8 = 0xd5;
    /// ARIB's event group descriptor: the same event on other services.
    pub const EVENT_GROUP: u8 = 0xd6;
    /// Broadcaster name (STD-B10 6.2.36), in the BIT.
    pub const BROADCASTER_NAME: u8 = 0xd8;
    /// Component group (STD-B10 6.2.37): a multi-view programme.
    pub const COMPONENT_GROUP: u8 = 0xd9;
    /// Content availability (STD-B10 6.2.45).
    pub const CONTENT_AVAILABILITY: u8 = 0xde;
    /// ARIB's data component descriptor: what a private stream holds.
    pub const DATA_COMPONENT: u8 = 0xfd;
}

/// MMT-SI's tags (ARIB STD-B60 Table 10-1). **Another space from the
/// transport stream's**, two bytes wide.
pub mod mh_tag {
    /// The asset group descriptor: the rain-fade pair.
    pub const ASSET_GROUP: u16 = 0x8000;
    /// Access control (STD-B60 7.4.3.5): the same as the transport
    /// stream's, with a packet identifier.
    pub const ACCESS_CONTROL: u16 = 0x8004;
    /// MH-service list (STD-B60): the same three bytes per service.
    pub const SERVICE_LIST: u16 = 0x800d;
    /// The event group descriptor.
    pub const EVENT_GROUP: u16 = 0x800c;
    /// The video component descriptor.
    pub const VIDEO_COMPONENT: u16 = 0x8010;
    /// The stream identifier descriptor.
    pub const STREAM_IDENTIFIER: u16 = 0x8011;
    /// The content descriptor.
    pub const CONTENT: u16 = 0x8012;
    /// MH-parental rating (STD-B60 7.4.3.22).
    pub const PARENTAL_RATING: u16 = 0x8013;
    /// The audio component descriptor.
    pub const AUDIO_COMPONENT: u16 = 0x8014;
    /// MH-target region (STD-B60 7.4.3.24).
    pub const TARGET_REGION: u16 = 0x8015;
    /// The service descriptor.
    /// MH-series (STD-B60 7.4.3.25).
    pub const SERIES: u16 = 0x8016;
    /// MH-broadcaster name (STD-B60 7.4.3.27), in the MH-BIT.
    pub const BROADCASTER_NAME: u16 = 0x8018;
    pub const SERVICE: u16 = 0x8019;
    /// MH-component group (STD-B60 7.4.3.33).
    pub const COMPONENT_GROUP: u16 = 0x8024;
    /// MH-logo transmission (STD-B60 7.4.3.34).
    pub const LOGO_TRANSMISSION: u16 = 0x8025;
    /// Content copy control (STD-B60 7.4.3.41): the digital copy control
    /// descriptor with wider tags.
    pub const CONTENT_COPY_CONTROL: u16 = 0x8038;
    /// Content usage control (STD-B60 7.4.3.42): the content availability
    /// descriptor, plus remote viewing.
    pub const CONTENT_USAGE_CONTROL: u16 = 0x8039;
    /// Related broadcaster (STD-B60 7.4.3.46), in the MH-BIT.
    pub const RELATED_BROADCASTER: u16 = 0x803e;
    /// Multimedia service information (STD-B60 7.4.3.47).
    pub const MULTIMEDIA_SERVICE_INFO: u16 = 0x803f;
    /// The emergency information descriptor (MH) (STD-B60 7.4.3.10), in the
    /// MPT's own loop (TR-B39 Part 2 4.8.2). Its body is laid out as the TS
    /// one's, so it is read by `crate::nitdescriptors::emergency_information`.
    pub const EMERGENCY_INFORMATION: u16 = 0x8007;
    /// Emergency news (STD-B60 7.4.3.43).
    pub const EMERGENCY_NEWS: u16 = 0x8040;
    /// MH-CA contract information (STD-B60 7.4.3.44).
    pub const CA_CONTRACT_INFO: u16 = 0x8041;
    /// The data coding descriptor: how a caption asset is coded.
    pub const DATA_CODING: u16 = 0x8020;
    /// The application service descriptor (ARIB STD-B60 Table 7-87).
    pub const APPLICATION_SERVICE: u16 = 0x8034;
    /// The short event descriptor. **0xf000 and up carry a two-byte length.**
    pub const SHORT_EVENT: u16 = 0xf001;
    /// The extended event descriptor.
    pub const EXTENDED_EVENT: u16 = 0xf002;
}

/// The `data_component_id` of the subtitle and superimposed text coding
/// (ARIB STD-B24), as the data component and data content descriptors write
/// it.
pub const DATA_COMPONENT_CAPTION: u16 = 0x0008;
/// The same coding as the one-segment (C profile) broadcast carries it,
/// which ARIB gives its own identifier (TR-B14 Part 3). The stream is
/// otherwise the same data groups, so it is read by the same reader.
pub const DATA_COMPONENT_CAPTION_ONE_SEG: u16 = 0x0012;

/// What a transmission puts in the simulcast group of a component that is
/// not one of a simulcast pair.
const NO_SIMULCAST: u8 = 0xff;
/// The identifier a hierarchical transmission descriptor uses for a
/// counterpart the stream has not got.
const NO_REFERENCE: u16 = 0x1fff;

/// Who a sound is for, where the transmission says it is for somebody in
/// particular.
///
/// ARIB STD-B10 Table 6-43 puts it in two bits of the component type
/// (b6-b5), beside the arrangement rather than instead of it: a commentary
/// for a viewer who cannot see is an ordinary stereo stream that happens to
/// be described.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Assist {
    /// The transmission said nothing, which is every ordinary sound.
    #[default]
    None,
    /// The commentary for a viewer who cannot see what is happening.
    Visual,
    /// The sound for a viewer who cannot hear it as it is sent.
    Hearing,
}

impl Assist {
    /// What the two bits of a component type say (b6-b5).
    ///
    /// **The whole byte, not the mode**: the bits sit above the arrangement,
    /// so this takes the type as the descriptor sent it. The fourth value is
    /// reserved, and is read as nothing said rather than as a fourth kind of
    /// viewer.
    pub fn of(component_type: u8) -> Self {
        match (component_type >> 5) & 0x03 {
            0x01 => Self::Visual,
            0x02 => Self::Hearing,
            _ => Self::None,
        }
    }

    /// What to call it, for a viewer choosing between sounds. STD-B10
    /// Table 6-43 says 視覚障害者用音声解説 and 聴覚障害者用音声; the first
    /// is what a broadcast calls 解説放送, and is shortened to that. The
    /// second has no everyday name (nothing on air sends it), so the
    /// standard's own word is kept. Nothing where
    /// the transmission said nothing -- a list in which every ordinary sound
    /// carries a word is a list in which the word means nothing.
    pub fn name(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Visual => Some("解説"),
            Self::Hearing => Some("聴覚障害者用音声"),
        }
    }
}

/// What the transmission says a sound stream is.
///
/// It comes from one descriptor -- ARIB's audio component, which a transport
/// stream tags 0xC4 and MMT tags 0x8014 -- and it is what a viewer is
/// choosing between when a programme carries more than one sound. Without it
/// the streams are a list of numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    /// ARIB's component type, as the byte arrived.
    ///
    /// **Only the low five bits are the arrangement.** ARIB STD-B10 Table
    /// 6-43 divides the eight: the top bit says whether the stream carries
    /// dialogue control information, the two below it whether this is the
    /// commentary for a viewer who cannot see or the sound for one who
    /// cannot hear, and b4-b0 the audio mode. So the value is masked
    /// wherever it is read -- [`Component::dual_mono`],
    /// [`Component::channels`], `audio_name` in the player -- and what is
    /// kept here is what was sent, because the three upper bits say
    /// something a viewer would want and nothing here shows it yet.
    pub kind: u8,
    /// The component tag, where the descriptor carried one. It is what a
    /// stream of the map and a description in the event table are matched
    /// by: a one-segment map describes no sound at all, and the event
    /// table's audio component descriptors are the only word on it.
    pub tag: Option<u16>,
    /// The language, as the three letters the standard uses.
    pub language: String,
    /// The language of the second half of a dual mono pair, where the
    /// transmission said there was one.
    pub second: Option<String>,
    /// Whether the transmission calls this the main sound of the programme.
    pub main: bool,
    /// What the transmission calls it, where it said anything.
    ///
    /// **This is the line a viewer reads**: 日本語, 英語, 日本語（解説）. A
    /// list of identifiers and three-letter codes tells a viewer which sound
    /// is the commentary and which is the audio description only if they
    /// already know. Nothing where the transmission left the text out, which
    /// it may: an MMT package table can name both sounds with no text at
    /// all while the event table names them.
    pub name: Option<String>,
    /// ARIB's quality indicator: mode one, two or three of STD-B32 Part 2,
    /// or nought where the transmission did not say.
    pub quality: u8,
    /// The sampling frequency as the descriptor codes it (STD-B10 Table
    /// 6-45). See [`Component::sampling_hz`].
    ///
    /// **It is what the broadcaster declared, not what arrived**: what the
    /// samples are actually at is in the stream, and that is what the sound
    /// path runs on.
    pub sampling: u8,
    /// The simulcast group, where this sound is one of a pair sent twice in
    /// different codings. 0xFF means it is not, and that is what a
    /// transmission sends when there is no pair, so it is kept as nothing.
    pub simulcast: Option<u8>,
}

/// Whether a text is a listing mark rather than a name: `[S]`, `［Ｓ］`,
/// `[SS]` -- brackets of either width around Latin letters of either width,
/// and nothing that reads as a word.
fn is_mark(text: &str) -> bool {
    text.chars().all(|letter| {
        // Full-width ASCII folds onto ASCII.
        let folded = match letter as u32 {
            0xff01..=0xff5e => char::from_u32(letter as u32 - 0xff01 + 0x21).unwrap_or(letter),
            0x3000 => ' ',
            _ => letter,
        };
        folded.is_ascii() || matches!(letter, '【' | '】' | '〔' | '〕' | '「' | '」')
    })
}

impl Component {
    /// Whether the two channels are two programmes rather than two sides of
    /// one.
    ///
    /// **This is not a stereo stream.** ARIB calls it 1/0 + 1/0 mode: two
    /// mono programmes sent as the two channels of one stream, which is how a
    /// Japanese broadcast carries a film in Japanese and in the language it
    /// was made in. Playing it as stereo puts one language in each ear.
    ///
    /// **The whole byte is not the mode** (see [`Component::kind`]): a dual
    /// mono commentary for a viewer who cannot see arrives as 0x22, and
    /// tested against 0x02 it reads as an ordinary stream -- which is the one
    /// case where getting this wrong is heard rather than merely displayed.
    pub fn dual_mono(&self) -> bool {
        self.mode() == 0x02
    }

    /// The audio mode: the five bits of [`Component::kind`] that say how the
    /// channels are arranged.
    fn mode(&self) -> u8 {
        self.kind & 0x1f
    }

    /// Who this sound is for, where the transmission said it was for somebody
    /// in particular (STD-B10 Table 6-43, b6-b5).
    ///
    /// What the transmission calls the sound, where that is worth reading:
    /// 「球場音声」, 「日本語（解説）」. **A mark such as `[S]` is not a
    /// name** -- one BS station sends that over MMT as the text of a stereo sound, and a
    /// viewer reads the language and the arrangement better. Anything
    /// without a character beyond ASCII is taken as a mark.
    pub fn called(&self) -> Option<&str> {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty() && !is_mark(name))
    }

    /// Takes what a description in the event table says about the same
    /// sound and this one does not: the text, and who the sound is for.
    /// **An MMT package names its sounds with no text and no assist bits
    /// where the event table has both**, and the two arrive apart.
    pub fn enrich(&mut self, described: &Component) {
        if self.called().is_none()
            && let Some(name) = described.called()
        {
            self.name = Some(name.to_owned());
        }
        if self.kind & 0xe0 == 0 && described.kind & 0x1f == self.kind & 0x1f {
            self.kind |= described.kind & 0xe0;
        }
        if self.second.is_none() {
            self.second = described.second.clone();
        }
    }

    /// The fourth value of the two bits is reserved, and is read as nothing
    /// said rather than as a fourth kind of viewer.
    pub fn assist(&self) -> Assist {
        Assist::of(self.kind)
    }

    /// Whether the stream carries dialogue control information: the data a
    /// receiver needs to raise the voices against the rest, which 22.2
    /// channel sound is sent with (STD-B10 Table 6-43, b7; STD-B32 Part 2).
    ///
    /// **Nothing here acts on it yet.** It is read because it is one of the
    /// three bits that are not the arrangement, and a value half read is
    /// worse than one not read at all.
    pub fn dialogue_control(&self) -> bool {
        self.kind & 0x80 != 0
    }

    /// The sampling frequency the descriptor declared, in hertz, or nothing
    /// where it is one of the two values the standard reserves.
    pub fn sampling_hz(&self) -> Option<u32> {
        Some(match self.sampling & 0x07 {
            0x01 => 16_000,
            0x02 => 22_050,
            0x03 => 24_000,
            0x05 => 32_000,
            0x06 => 44_100,
            0x07 => 48_000,
            // 0x00 and 0x04 are reserved.
            _ => return None,
        })
    }

    /// How many channels the arrangement has, or nought where this build does
    /// not know the arrangement.
    ///
    /// It is what the descriptor says rather than what the frames turn out to
    /// hold, so it is for showing a viewer and not for laying out samples.
    ///
    /// **The number is counted out of the name ARIB gives the arrangement**,
    /// which `audio_name` in the player writes out in full. STD-B10 Table
    /// 6-43 says how to read those names: `upper layer (front/side/rear) -
    /// middle layer (front/side/rear) - lower layer (front/side/rear) . LFE`,
    /// a layer with nothing in it written 0, and a mode with only a middle
    /// layer written as that layer alone. So 3/2+LFE is six and
    /// 3/3/3-5/2/3-3/0/0.2 is nine and ten and three and two -- twenty-four,
    /// which is what 22.2 is a count of. Counting them is the whole of the
    /// rule, and it is why every mode in that table has an answer here.
    ///
    /// **0x11 is the 22.2 channel sound**, and it was missing: only 0x10 was
    /// here, and the one thing this table is asked about in practice is the
    /// 22.2 channel sound, which is sent as 0x11. Without it the
    /// 22.2 channel asset showed in the player as `？` -- the value for
    /// "the transmission did not say" -- while the transmission had said
    /// exactly what it was. Both the standard and the stream say which of the
    /// two it is: Table 6-43 gives 0x11 the name `3/3/3-5/2/3-3/0/0.2`, which
    /// counts to twenty-four, and ARIB STD-B32 Part 2 calls that same name
    /// `22.2ch (3/3/3-5/2/3-3/0/0+2)` in its channel configuration 13. 0x10's
    /// name counts to twelve.
    pub fn channels(&self) -> u8 {
        match self.mode() {
            // 1/0
            0x01 => 1,
            // 1/0+1/0, 2/0
            0x02 | 0x03 => 2,
            // 2/1, 3/0
            0x04 | 0x05 => 3,
            // 2/2, 3/1
            0x06 | 0x07 => 4,
            // 3/2
            0x08 => 5,
            // 3/2+LFE
            0x09 => 6,
            // 3/3.1, 2/0/0-2/0/2-0.1
            0x0a | 0x0b => 7,
            // 5/2.1, 3/2/2.1, 2/0/0-3/0/2-0.1, 0/2/0-3/0/2-0.1
            0x0c..=0x0f => 8,
            // 2/0/0-3/2/3-0.2
            0x10 => 12,
            // 3/3/3-5/2/3-3/0/0.2, which is 22.2
            0x11 => 24,
            _ => 0,
        }
    }
}

/// Where a stream sits in a rain-fade pair.
///
/// A satellite broadcast sends the same programme twice: the picture a
/// receiver shows, and a smaller one coded to survive the carrier-to-noise
/// ratio left in heavy rain. Both are in the map, both are on the air the
/// whole time, and nothing in the pictures themselves says which is which.
///
/// **The two transmissions say it differently.** A transport stream gives
/// each of the pair a hierarchical transmission descriptor naming the other's
/// identifier and which of the two it is; MMT puts both in one asset group
/// and numbers them, nought first. What is kept here is the number, because
/// that is the part a player acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hierarchy {
    /// Nought for the picture the transmission leads with, and higher for one
    /// that stands in for it in worse conditions.
    pub level: u8,
    /// The other stream of the pair, where the transmission named it. A
    /// transport stream does; MMT leaves it to be worked out from the group.
    pub reference: Option<u16>,
    /// MMT's group, which is what pairs its streams. Nothing for a transport
    /// stream, which pairs by the identifier above.
    pub group: Option<u8>,
}

/// Reads an audio component descriptor (transport stream, 0xC4).
///
/// The layout is ARIB STD-B10 Table 6-42: the stream content and the
/// component type, the tag, the stream type and the simulcast group, a byte
/// of flags, and the language. A second language follows where the flags say
/// so, which is how the two halves of a dual mono pair are named separately.
///
/// ```text
///   0  reserved 4 | stream_content 4
///   1  component_type
///   2  component_tag
///   3  stream_type
///   4  simulcast_group_tag
///   5  multi_lingual 1 | main 1 | quality 2 | sampling_rate 3 | reserved 1
///   6  ISO_639_language_code（3）
///   9  ISO_639_language_code_2（3、multi_lingual のとき）
///      text_char
/// ```
///
/// **The stream content and the stream type are not kept.** They say which
/// coding this is, and the programme map has already said it -- reading it
/// twice gives a receiver two answers to one question and no rule for which
/// one wins.
pub fn audio_component(body: &[u8]) -> Option<Component> {
    if body.len() < 9 {
        return None;
    }
    let multi = body[5] & 0x80 != 0;
    let second = (multi && body.len() >= 12).then(|| language(&body[9..12]));
    // The text runs to the end, after however many languages were named.
    let at = if multi { 12 } else { 9 };
    Some(Component {
        kind: body[1],
        tag: Some(u16::from(body[2])),
        language: language(&body[6..9]),
        second,
        main: body[5] & 0x40 != 0,
        name: body.get(at..).and_then(named),
        quality: (body[5] >> 4) & 0x03,
        sampling: (body[5] >> 1) & 0x07,
        simulcast: (body[4] != NO_SIMULCAST).then_some(body[4]),
    })
}

/// Reads an MMT audio component descriptor (0x8014).
///
/// It is ARIB STD-B60's, which is the transport stream's with a wider tag:
/// two bytes where the other has one, so everything after it sits a byte
/// further along. **The text is UTF-8**, where a transport stream writes
/// the eight-unit code.
pub fn mh_audio_component(body: &[u8]) -> Option<Component> {
    if body.len() < 10 {
        return None;
    }
    let multi = body[6] & 0x80 != 0;
    let second = (multi && body.len() >= 13).then(|| language(&body[10..13]));
    let at = if multi { 13 } else { 10 };
    Some(Component {
        kind: body[1],
        tag: Some(u16::from_be_bytes([body[2], body[3]])),
        language: language(&body[7..10]),
        second,
        main: body[6] & 0x40 != 0,
        name: body.get(at..).and_then(mh_named),
        quality: (body[6] >> 4) & 0x03,
        sampling: (body[6] >> 1) & 0x07,
        simulcast: (body[5] != NO_SIMULCAST).then_some(body[5]),
    })
}

/// The component tag of a stream, out of whichever descriptor of the loop
/// named it.
///
/// **A stream is named twice and either will do.** The stream identifier
/// descriptor carries the tag for every kind of stream, and the audio
/// component descriptor carries it again for sound. Asking for the general
/// one first means a picture is tagged as well as a sound; falling back
/// means a sound whose map left the stream identifier out is still
/// remembered across a map update.
pub fn component_tag(descriptors: &[tstables::Descriptor]) -> Option<u16> {
    tstables::find(descriptors, tag::STREAM_IDENTIFIER)
        .and_then(|found| found.body.first())
        .map(|tag| u16::from(*tag))
        .or_else(|| {
            tstables::find(descriptors, tag::AUDIO_COMPONENT)
                .and_then(|found| found.body.get(2))
                .map(|tag| u16::from(*tag))
        })
}

/// The same for MMT, whose tags are sixteen bits.
pub fn mh_component_tag(descriptors: &[mmttables::Descriptor]) -> Option<u16> {
    let wide = |bytes: &[u8]| bytes.get(..2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    mmttables::find(descriptors, mh_tag::STREAM_IDENTIFIER)
        .and_then(|found| wide(&found.body))
        .or_else(|| {
            mmttables::find(descriptors, mh_tag::AUDIO_COMPONENT)
                .and_then(|found| found.body.get(2..4))
                .and_then(wide)
        })
}

/// What a component descriptor (0x50) says of a picture: the stream content
/// (which coding) and the component type (format and aspect), then the tag
/// and a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureComponent {
    pub stream_content: u8,
    pub component_type: u8,
    pub tag: u16,
    /// What the transmission calls it, where it said anything. TR-B14 lets a
    /// broadcaster leave it out for the default name.
    pub name: Option<String>,
}

/// The values of `stream_content` that mean a picture (ARIB STD-B10 Table
/// 6-5): one for each coding, with the same `component_type` layout.
pub const STREAM_CONTENT_VIDEO: [u8; 3] = [
    // MPEG-2 Video
    0x01, // H.264 | MPEG-4 AVC（ワンセグ）
    0x05, // H.265 | MPEG-H HEVC
    0x09,
];

/// Reads a component descriptor.
pub fn picture_component(body: &[u8]) -> Option<PictureComponent> {
    if body.len() < 6 {
        return None;
    }
    Some(PictureComponent {
        stream_content: body[0] & 0x0f,
        component_type: body[1],
        tag: u16::from(body[2]),
        name: named(&body[6..]),
    })
}

/// What an MH video component descriptor (ARIB STD-B60 Table 7-47) says
/// of a picture: resolution 4, aspect 4, progressive 1, reserved 2, frame
/// rate 5, `component_tag` 16, transfer characteristics 4, reserved 4,
/// then the language and a text. **Whether it is HDR is said here and
/// nowhere else** -- a transport stream broadcast has nothing of the kind in the
/// programme's description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VideoComponent {
    /// The vertical resolution as coded (Table 7-48). Nought means unsaid.
    pub resolution: u8,
    /// The aspect ratio as coded (Table 7-49).
    pub aspect: u8,
    pub progressive: bool,
    /// The frame rate as coded (Table 7-50).
    pub frame_rate: u8,
    pub tag: u16,
    /// The transfer characteristics as coded (Table 7-51): **4 is PQ and 5
    /// is HLG, both HDR.** Nought means unsaid.
    pub transfer: u8,
}

impl VideoComponent {
    /// Whether the picture is HDR. **Only where the transmission said** --
    /// unsaid is not SDR, it is unknown.
    pub fn hdr(&self) -> Option<bool> {
        match self.transfer {
            0 => None,
            4 | 5 => Some(true),
            _ => Some(false),
        }
    }
}

/// Reads an MH video component descriptor.
pub fn mh_video_component(body: &[u8]) -> Option<VideoComponent> {
    let bytes = body.get(..5)?;
    Some(VideoComponent {
        resolution: bytes[0] >> 4,
        aspect: bytes[0] & 0x0f,
        progressive: bytes[1] & 0x80 != 0,
        frame_rate: bytes[1] & 0x1f,
        tag: u16::from_be_bytes([bytes[2], bytes[3]]),
        transfer: bytes[4] >> 4,
    })
}

/// Reads a hierarchical transmission descriptor (0xC0).
///
/// Three bytes: seven reserved bits and the quality level in the lowest bit
/// of the first, then three reserved bits and the thirteen of the other
/// stream's identifier.
///
/// **The quality level is one for the picture to show and nought for the
/// stand-in**, which is the way round ARIB STD-B10 puts it. It is turned
/// over here so that nought means the one to play in both transmissions --
/// MMT numbers its group the other way, and a player that had to remember
/// which broadcast it was watching to know what nought meant would get it
/// wrong once.
pub fn hierarchical_transmission(body: &[u8]) -> Option<Hierarchy> {
    if body.len() < 3 {
        return None;
    }
    let high = body[0] & 0x01 != 0;
    Some(Hierarchy {
        level: u8::from(!high),
        // The null identifier is what the standard uses for a counterpart
        // that does not exist, and a satellite broadcast really sends it: the
        // sound of a BS service is marked low quality with no other half.
        reference: match u16::from(body[1] & 0x1f) << 8 | u16::from(body[2]) {
            NO_REFERENCE => None,
            pid => Some(pid),
        },
        group: None,
    })
}

/// Reads an asset group descriptor (MMT, 0x8000).
///
/// Two bytes: which group the asset belongs to, and where it comes in it.
/// Nought is the asset a receiver shows and higher stands in for it, which is
/// already the way round [`Hierarchy::level`] wants.
pub fn asset_group(body: &[u8]) -> Option<Hierarchy> {
    let bytes = body.get(..2)?;
    Some(Hierarchy {
        level: bytes[1],
        reference: None,
        group: Some(bytes[0]),
    })
}

/// What a conditional access descriptor (0x09) names: the system and the
/// identifier its messages ride on.
pub fn conditional_access(body: &[u8]) -> Option<(u16, u16)> {
    let bytes = body.get(..4)?;
    Some((
        u16::from_be_bytes([bytes[0], bytes[1]]),
        u16::from_be_bytes([bytes[2], bytes[3]]) & 0x1fff,
    ))
}

/// What the data component descriptor (0xFD) says the stream is coded as:
/// the `data_component_id`.
pub fn data_component_id(body: &[u8]) -> Option<u16> {
    let bytes = body.get(..2)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

/// What the data content descriptor (0xC7) says goes with the event: the
/// `data_component_id` in its first two bytes. **A programme's caption mark
/// comes from here**, 0x0008 (ARIB STD-B10 Part 2 6.2.28).
pub fn data_content_id(body: &[u8]) -> Option<u16> {
    data_component_id(body)
}

/// The application service descriptor (ARIB STD-B60 Table 7-87): where the
/// data broadcast's entry (MH-AIT), its files and its event messages ride.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationService {
    /// `document_resolution` (Table 7-88).
    pub resolution: u8,
    /// The packet identifier the MH-AIT rides on.
    pub ait: u16,
    /// The packet identifier the data transmission messages ride on, where
    /// the flag says there are any.
    pub data: Option<u16>,
    /// (EMT_tag, packet_id) for each event message table.
    pub emts: Vec<(u8, u16)>,
}

impl ApplicationService {
    /// The tag of the event message table on this identifier, if any.
    pub fn emt_tag(&self, packet_id: u16) -> Option<u8> {
        self.emts
            .iter()
            .find(|(_, id)| *id == packet_id)
            .map(|(tag, _)| *tag)
    }
}

/// Reads an application service descriptor: `application_format` 4 and
/// reserved 4, `document_resolution` 4 and reserved 4, `default_AIT_flag` 1,
/// `DT_message_flag` 1, reserved 2 and `EMT_num` 4, then the location of the
/// AIT (`MMT_general_location_info`), of the data transmission messages
/// where the flag says so, and of each EMT. **Only location type 0 (a
/// packet identifier in this stream) is operated** (TR-B39 Table 6-3); any
/// other ends the reading, since its length depends on its type.
pub fn application_service(body: &[u8]) -> Option<ApplicationService> {
    if body.len() < 3 {
        return None;
    }
    let resolution = body[1] >> 4;
    let dt_message = body[2] & 0x40 != 0;
    let emt_num = body[2] & 0x0f;
    let mut at = 3;
    let location = |at: &mut usize| -> Option<u16> {
        if body.get(*at) != Some(&0x00) {
            return None;
        }
        let id = u16::from_be_bytes([*body.get(*at + 1)?, *body.get(*at + 2)?]);
        *at += 3;
        Some(id)
    };
    let ait = location(&mut at)?;
    let data = if dt_message { location(&mut at) } else { None };
    let mut emts = Vec::new();
    for _ in 0..emt_num {
        let Some(&tag) = body.get(at) else {
            break;
        };
        at += 1;
        let Some(packet_id) = location(&mut at) else {
            break;
        };
        emts.push((tag, packet_id));
    }
    Some(ApplicationService {
        resolution,
        ait,
        data,
        emts,
    })
}

/// What a service descriptor says: the kind of service, and who provides it
/// and what it is called.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServiceNaming {
    pub kind: u8,
    pub provider: String,
    pub name: String,
}

/// Reads a service descriptor (0x48): kind, provider name, service name. The
/// names are written one after the other behind their lengths, so **the
/// second cannot be found without reading the first.**
pub fn service(body: &[u8]) -> Option<ServiceNaming> {
    let (kind, provider, name) = service_fields(body)?;
    Some(ServiceNaming {
        kind,
        provider: text(provider),
        name: text(name),
    })
}

/// Reads an MH service descriptor (0x8019): the same layout with the text
/// in UTF-8.
pub fn mh_service(body: &[u8]) -> Option<ServiceNaming> {
    let (kind, provider, name) = service_fields(body)?;
    Some(ServiceNaming {
        kind,
        provider: utf8(provider),
        name: utf8(name),
    })
}

fn service_fields(body: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    if body.len() < 3 {
        return None;
    }
    let provider = usize::from(body[1]);
    if 2 + provider >= body.len() {
        return None;
    }
    let name = usize::from(body[2 + provider]);
    let from = 3 + provider;
    if from + name > body.len() {
        return None;
    }
    Some((body[0], &body[2..2 + provider], &body[from..from + name]))
}

/// What a short event descriptor says: the title and a line of text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShortEvent {
    pub language: String,
    pub title: String,
    pub text: String,
}

/// Reads a short event descriptor (0x4D): three letters of language, the
/// title, then the text, each behind a one-byte length.
pub fn short_event(body: &[u8]) -> Option<ShortEvent> {
    if body.len() < 5 {
        return None;
    }
    let title = usize::from(body[3]);
    let from = 4 + title;
    if from >= body.len() {
        return None;
    }
    let detail = usize::from(body[from]);
    let at = from + 1;
    if at + detail > body.len() {
        return None;
    }
    Some(ShortEvent {
        language: language(&body[..3]),
        title: text(&body[4..4 + title]),
        text: text(&body[at..at + detail]),
    })
}

/// Reads an MH short event descriptor (0xF001). **The text's length is two
/// bytes**; the transport stream's is one, and that is the only difference
/// besides the encoding.
pub fn mh_short_event(body: &[u8]) -> Option<ShortEvent> {
    if body.len() < 4 {
        return None;
    }
    let title = usize::from(body[3]);
    let from = 4 + title;
    if from + 2 > body.len() {
        return None;
    }
    let detail = usize::from(u16::from_be_bytes([body[from], body[from + 1]]));
    let at = from + 2;
    if at + detail > body.len() {
        return None;
    }
    Some(ShortEvent {
        language: language(&body[..3]),
        title: utf8(&body[4..4 + title]),
        text: utf8(&body[at..at + detail]),
    })
}

/// Reads the items of an extended event descriptor (0x4E): after which
/// descriptor of how many and the language, headed items, each a heading and
/// a value behind one-byte lengths.
///
/// **A descriptor holds only some 220 bytes of text**, so one item may be
/// split over several descriptors: an item whose heading is empty is the
/// continuation of the last one, and the caller joins them. **The pieces
/// are joined as bytes, before decoding** ([`extended_event_bytes`]): the
/// cut falls anywhere in the eight-bit code, and the continuation goes on
/// in whatever character sets the first piece had designated -- read on
/// its own it comes out as kana (seen on a BS service whose text shifts
/// digits and brackets into a set designated in the first descriptor).
pub fn extended_event(body: &[u8]) -> Vec<(String, String)> {
    extended_event_bytes(body)
        .into_iter()
        .map(|(heading, value)| (text(heading), text(value)))
        .collect()
}

/// The items of an extended event descriptor as sent, before decoding.
pub fn extended_event_bytes(body: &[u8]) -> Vec<(&[u8], &[u8])> {
    let Some(&items) = body.get(4) else {
        return Vec::new();
    };
    let end = 5 + usize::from(items);
    if end > body.len() {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut at = 5;
    while at + 2 <= end {
        let heading = usize::from(body[at]);
        let next = at + 1 + heading;
        if next >= end {
            break;
        }
        let length = usize::from(body[next]);
        let from = next + 1;
        if from + length > end {
            break;
        }
        found.push((&body[at + 1..next], &body[from..from + length]));
        at = from + length;
    }
    found
}

/// Reads the items of an MH extended event descriptor (0xF002): **the length
/// of all the items is two bytes, and so is each value's.**
pub fn mh_extended_event(body: &[u8]) -> Vec<(String, String)> {
    if body.len() < 6 {
        return Vec::new();
    }
    let items = usize::from(u16::from_be_bytes([body[4], body[5]]));
    let end = 6 + items;
    if end > body.len() {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut at = 6;
    while at + 3 <= end {
        let heading = usize::from(body[at]);
        let next = at + 1 + heading;
        if next + 2 > end {
            break;
        }
        let length = usize::from(u16::from_be_bytes([body[next], body[next + 1]]));
        let from = next + 2;
        if from + length > end {
            break;
        }
        found.push((utf8(&body[at + 1..next]), utf8(&body[from..from + length])));
        at = from + length;
    }
    found
}

/// One entry of a content descriptor: the genre in two nibbles, and the
/// byte the standard leaves to the operating rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Content {
    /// `content_nibble_level_1`: the large genre (ARIB STD-B10 Annex H).
    pub large: u8,
    /// `content_nibble_level_2`: the middle genre.
    pub middle: u8,
    /// The two `user_nibble`s as one byte. **Meaningful only under large
    /// genre 0xE**: 0xE0 makes it the programme attachment information of
    /// TR-B14 Appendix B (may be cancelled, extended, interrupted...), 0xE1
    /// the extended genres of wideband CS (sports, foreign film, Japanese
    /// film, by TR-B15). Under any other large genre the rules say to send
    /// 0xFF and ignore it.
    pub user: u8,
}

/// Reads a content descriptor (0x54, and MMT's 0x8012, which is the same
/// bytes): pairs of a large and a middle genre nibble, and two nibbles for
/// the broadcaster's own use, two bytes each.
pub fn content(body: &[u8]) -> Vec<Content> {
    body.as_chunks::<2>()
        .0
        .iter()
        .map(|pair| Content {
            large: pair[0] >> 4,
            middle: pair[0] & 0x0f,
            user: pair[1],
        })
        .collect()
}

/// The `group_type` of an event group descriptor that means the same event
/// is carried on other services of the network (ARIB STD-B10 Part 2
/// 6.2.34: 事象共有).
pub const EVENT_GROUP_COMMON: u8 = 1;

/// Reads an event group descriptor (0xD6, and MMT's 0x800C, which is laid
/// out the same): the group type in four bits, a count in four, then
/// (service_id, event_id) pairs.
pub fn event_group(body: &[u8]) -> Option<(u8, Vec<(u16, u16)>)> {
    let head = *body.first()?;
    let count = usize::from(head & 0x0f);
    let pairs = body[1..]
        .as_chunks::<4>()
        .0
        .iter()
        .take(count)
        .map(|pair| {
            (
                u16::from_be_bytes([pair[0], pair[1]]),
                u16::from_be_bytes([pair[2], pair[3]]),
            )
        })
        .collect();
    Some((head >> 4, pairs))
}

/// What a parental rating descriptor (0x55, MH 0x8013) says: one entry
/// per country, the rating as coded. [`rating_age`] turns it into years.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParentalRating {
    pub country: String,
    pub rating: u8,
}

/// The minimum age a rating means, where it means one: 0x01..=0x0F is the
/// age less three, 0x00 is unsaid and 0x10.. is the broadcaster's own.
pub fn rating_age(rating: u8) -> Option<u8> {
    (1..=0x0f).contains(&rating).then(|| rating + 3)
}

/// Reads a parental rating descriptor. The MH form is the same bytes.
pub fn parental_rating(body: &[u8]) -> Vec<ParentalRating> {
    body.as_chunks::<4>()
        .0
        .iter()
        .map(|entry| ParentalRating {
            country: language(&entry[..3]),
            rating: entry[3],
        })
        .collect()
}

/// One component's copy control inside a [`CopyControl`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ComponentCopyControl {
    pub tag: u16,
    /// `digital_recording_control_data` (Table 6-39): 0 free, 1 by the
    /// provider, 2 one generation, 3 prohibited.
    pub control: u8,
    /// `maximum_bitrate`, where given, in units of a quarter megabit.
    pub maximum_bitrate: Option<u8>,
}

/// What a digital copy control descriptor (0xC1, MMT's content copy control
/// 0x8038) says of the programme or service as a whole and of each
/// component.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CopyControl {
    /// `digital_recording_control_data` for the whole (Table 6-39).
    pub control: u8,
    pub maximum_bitrate: Option<u8>,
    pub components: Vec<ComponentCopyControl>,
}

impl CopyControl {
    /// What the control means, in the standard's words.
    pub fn control_name(control: u8) -> &'static str {
        match control & 0x03 {
            0 => "制約条件なしにコピー可",
            1 => "事業者定義",
            2 => "一世代のみコピー可",
            _ => "コピー禁止",
        }
    }
}

/// Reads a digital copy control descriptor (STD-B10 6.2.23).
pub fn digital_copy_control(body: &[u8]) -> Option<CopyControl> {
    let head = *body.first()?;
    let mut at = 1;
    let mut read = CopyControl {
        control: head >> 6,
        maximum_bitrate: None,
        components: Vec::new(),
    };
    if head & 0x20 != 0 {
        read.maximum_bitrate = Some(*body.get(at)?);
        at += 1;
    }
    if head & 0x10 != 0 {
        let length = usize::from(*body.get(at)?);
        at += 1;
        let end = (at + length).min(body.len());
        while at + 2 <= end {
            let flags = body[at + 1];
            let mut component = ComponentCopyControl {
                tag: u16::from(body[at]),
                control: flags >> 6,
                maximum_bitrate: None,
            };
            at += 2;
            if flags & 0x20 != 0 {
                component.maximum_bitrate = Some(*body.get(at)?);
                at += 1;
            }
            read.components.push(component);
        }
    }
    Some(read)
}

/// Reads a content copy control descriptor (STD-B60 7.4.3.41): the same,
/// with a reserved byte after the flags and sixteen-bit tags.
pub fn mh_content_copy_control(body: &[u8]) -> Option<CopyControl> {
    let head = *body.first()?;
    let mut at = 2;
    let mut read = CopyControl {
        control: head >> 6,
        maximum_bitrate: None,
        components: Vec::new(),
    };
    if head & 0x20 != 0 {
        read.maximum_bitrate = Some(*body.get(at)?);
        at += 1;
    }
    if head & 0x10 != 0 {
        let length = usize::from(*body.get(at)?);
        at += 1;
        let end = (at + length).min(body.len());
        while at + 4 <= end {
            let flags = body[at + 2];
            let mut component = ComponentCopyControl {
                tag: u16::from_be_bytes([body[at], body[at + 1]]),
                control: flags >> 6,
                maximum_bitrate: None,
            };
            at += 4;
            if flags & 0x20 != 0 {
                component.maximum_bitrate = Some(*body.get(at)?);
                at += 1;
            }
            read.components.push(component);
        }
    }
    Some(read)
}

/// What a content availability descriptor (0xDE, MMT's content usage
/// control 0x8039) says about keeping and copying the programme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContentAvailability {
    /// Whether a limited number of copies may be made (ダビング10).
    pub copy_restriction: bool,
    /// Whether the picture may go out at full resolution over an analogue
    /// link (`image_constraint_token` set means unconstrained).
    pub image_unconstrained: bool,
    /// Whether temporary storage is forbidden even where copying is.
    pub no_retention: bool,
    /// `retention_state` (Table 6-88): how long temporary storage may keep
    /// it. [`ContentAvailability::retention_name`] says it in words.
    pub retention_state: u8,
    /// Whether the output must be protected.
    pub encryption: bool,
    /// MMT only: whether remote viewing is allowed.
    pub remote_view: Option<bool>,
}

impl ContentAvailability {
    /// How long temporary storage may keep it (Table 6-88).
    pub fn retention_name(&self) -> &'static str {
        match self.retention_state & 0x07 {
            0 => "無制限",
            1 => "1週間",
            2 => "1日",
            3 => "12時間",
            4 => "6時間",
            5 => "3時間",
            6 => "1時間30分",
            _ => "予備",
        }
    }
}

/// Reads a content availability descriptor (STD-B10 6.2.45).
pub fn content_availability(body: &[u8]) -> Option<ContentAvailability> {
    let flags = *body.first()?;
    Some(ContentAvailability {
        copy_restriction: flags & 0x40 != 0,
        image_unconstrained: flags & 0x20 != 0,
        no_retention: flags & 0x10 != 0,
        retention_state: (flags >> 1) & 0x07,
        encryption: flags & 0x01 != 0,
        remote_view: None,
    })
}

/// Reads a content usage control descriptor (STD-B60 7.4.3.42).
pub fn mh_content_usage_control(body: &[u8]) -> Option<ContentAvailability> {
    let [first, second, ..] = body else {
        return None;
    };
    Some(ContentAvailability {
        remote_view: Some(first & 0x80 != 0),
        copy_restriction: first & 0x40 != 0,
        image_unconstrained: first & 0x20 != 0,
        no_retention: second & 0x10 != 0,
        retention_state: (second >> 1) & 0x07,
        encryption: second & 0x01 != 0,
    })
}

/// What a CA contract information descriptor (0xCB, MH 0x8041; ARIB
/// STD-B25) says: which components form a charge unit and what the fee is
/// called.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CaContract {
    pub ca_system_id: u16,
    /// `CA_unit_id`: 0 no charge, 1 the unit holding the programme's main
    /// components, 2.. other units.
    pub unit: u8,
    pub components: Vec<u16>,
    /// `contract_verification_info`, for the card. Kept as sent.
    pub verification: Vec<u8>,
    pub fee_name: String,
}

fn ca_contract_read(body: &[u8], wide: bool, text: fn(&[u8]) -> String) -> Option<CaContract> {
    let ca_system_id = u16::from_be_bytes([*body.first()?, *body.get(1)?]);
    let head = *body.get(2)?;
    let count = usize::from(head & 0x0f);
    let mut at = 3;
    let mut components = Vec::with_capacity(count);
    for _ in 0..count {
        components.push(if wide {
            let tag = u16::from_be_bytes([*body.get(at)?, *body.get(at + 1)?]);
            at += 2;
            tag
        } else {
            let tag = u16::from(*body.get(at)?);
            at += 1;
            tag
        });
    }
    let verification_length = usize::from(*body.get(at)?);
    at += 1;
    let verification = body.get(at..at + verification_length)?.to_vec();
    at += verification_length;
    let fee_length = usize::from(*body.get(at)?);
    at += 1;
    let fee_name = text(body.get(at..at + fee_length)?);
    Some(CaContract {
        ca_system_id,
        unit: head >> 4,
        components,
        verification,
        fee_name,
    })
}

/// Reads a CA contract information descriptor (STD-B25).
pub fn ca_contract_info(body: &[u8]) -> Option<CaContract> {
    ca_contract_read(body, false, text)
}

/// Reads an MH-CA contract information descriptor (STD-B60 7.4.3.44).
pub fn mh_ca_contract_info(body: &[u8]) -> Option<CaContract> {
    ca_contract_read(body, true, utf8)
}

/// What a series descriptor (0xD5, MH 0x8016) says: which series the
/// programme belongs to and where in it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Series {
    pub id: u16,
    /// Tells a rerun's series from the original's where the two overlap; 0
    /// is the original.
    pub repeat_label: u8,
    /// `program_pattern` (Table 6-66). [`Series::pattern_name`] says it.
    pub pattern: u8,
    /// When the series ends, as a modified Julian date, where the
    /// transmission said.
    pub expire_mjd: Option<u16>,
    /// This episode's number and the last, where the transmission counts
    /// them (0 means not counted).
    pub episode: u16,
    pub last_episode: u16,
    pub name: String,
}

impl Series {
    /// The programme pattern in words (Table 6-66).
    pub fn pattern_name(&self) -> &'static str {
        match self.pattern & 0x07 {
            0 => "不定期",
            1 => "帯番組",
            2 => "週一回",
            3 => "月一回",
            4 => "一日に複数回",
            5 => "長時間番組の分割",
            6 => "定期・不定期の蓄積用",
            _ => "予備",
        }
    }
}

fn series_read(body: &[u8], text: fn(&[u8]) -> String) -> Option<Series> {
    let head = body.get(..8)?;
    Some(Series {
        id: u16::from_be_bytes([head[0], head[1]]),
        repeat_label: head[2] >> 4,
        pattern: (head[2] >> 1) & 0x07,
        expire_mjd: (head[2] & 0x01 != 0).then(|| u16::from_be_bytes([head[3], head[4]])),
        episode: (u16::from(head[5]) << 4) | u16::from(head[6] >> 4),
        last_episode: (u16::from(head[6] & 0x0f) << 8) | u16::from(head[7]),
        name: text(&body[8..]),
    })
}

/// Reads a series descriptor (STD-B10 6.2.33).
pub fn series(body: &[u8]) -> Option<Series> {
    series_read(body, text)
}

/// Reads an MH-series descriptor (STD-B60 7.4.3.25).
pub fn mh_series(body: &[u8]) -> Option<Series> {
    series_read(body, utf8)
}

/// One charge unit of a [`ComponentGroup`]: which components it holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComponentGroupUnit {
    pub ca_unit: u8,
    pub components: Vec<u16>,
}

/// One group of a component group descriptor: one view of a multi-view
/// programme.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComponentGroup {
    pub id: u8,
    pub units: Vec<ComponentGroupUnit>,
    /// The group's bit rate, in units of a quarter megabit, where given.
    pub total_bitrate: Option<u8>,
    pub name: String,
}

/// What a component group descriptor (0xD9, MH 0x8024) says: the groups of
/// components an event is split into (`group_type` 0 is multi-view TV).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComponentGroups {
    pub group_type: u8,
    pub groups: Vec<ComponentGroup>,
}

fn component_groups_read(
    body: &[u8],
    wide: bool,
    text: fn(&[u8]) -> String,
) -> Option<ComponentGroups> {
    let head = *body.first()?;
    let with_bitrate = head & 0x10 != 0;
    let count = usize::from(head & 0x0f);
    let mut at = 1;
    let mut groups = Vec::with_capacity(count);
    for _ in 0..count {
        let byte = *body.get(at)?;
        at += 1;
        let mut group = ComponentGroup {
            id: byte >> 4,
            ..ComponentGroup::default()
        };
        for _ in 0..usize::from(byte & 0x0f) {
            let byte = *body.get(at)?;
            at += 1;
            let mut unit = ComponentGroupUnit {
                ca_unit: byte >> 4,
                components: Vec::new(),
            };
            for _ in 0..usize::from(byte & 0x0f) {
                unit.components.push(if wide {
                    let tag = u16::from_be_bytes([*body.get(at)?, *body.get(at + 1)?]);
                    at += 2;
                    tag
                } else {
                    let tag = u16::from(*body.get(at)?);
                    at += 1;
                    tag
                });
            }
            group.units.push(unit);
        }
        if with_bitrate {
            group.total_bitrate = Some(*body.get(at)?);
            at += 1;
        }
        let length = usize::from(*body.get(at)?);
        at += 1;
        group.name = text(body.get(at..at + length)?);
        at += length;
        groups.push(group);
    }
    Some(ComponentGroups {
        group_type: head >> 5,
        groups,
    })
}

/// Reads a component group descriptor (STD-B10 6.2.37).
pub fn component_group(body: &[u8]) -> Option<ComponentGroups> {
    component_groups_read(body, false, text)
}

/// Reads an MH-component group descriptor (STD-B60 7.4.3.33).
pub fn mh_component_group(body: &[u8]) -> Option<ComponentGroups> {
    component_groups_read(body, true, utf8)
}

/// What a logo transmission descriptor (0xCF, MH 0x8025) says: where the
/// service's logo is, or what to write in its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Logo {
    /// Type 1: in the CDT, found by download data identifier.
    Downloaded {
        logo_id: u16,
        version: u16,
        download_data_id: u16,
    },
    /// Type 2: in the CDT, by logo identifier alone.
    Referenced { logo_id: u16 },
    /// Type 3: a short text instead of a picture.
    Text(String),
    /// A type this reader does not know.
    Other(u8),
}

fn logo_read(body: &[u8], text: fn(&[u8]) -> String) -> Option<Logo> {
    let kind = *body.first()?;
    let rest = &body[1..];
    Some(match kind {
        0x01 => {
            let bytes = rest.get(..6)?;
            Logo::Downloaded {
                logo_id: u16::from_be_bytes([bytes[0] & 0x01, bytes[1]]),
                version: u16::from_be_bytes([bytes[2] & 0x0f, bytes[3]]),
                download_data_id: u16::from_be_bytes([bytes[4], bytes[5]]),
            }
        }
        0x02 => {
            let bytes = rest.get(..2)?;
            Logo::Referenced {
                logo_id: u16::from_be_bytes([bytes[0] & 0x01, bytes[1]]),
            }
        }
        0x03 => Logo::Text(text(rest)),
        other => Logo::Other(other),
    })
}

/// Reads a logo transmission descriptor (STD-B10 6.2.44).
pub fn logo_transmission(body: &[u8]) -> Option<Logo> {
    logo_read(body, text)
}

/// Reads an MH-logo transmission descriptor (STD-B60 7.4.3.34).
pub fn mh_logo_transmission(body: &[u8]) -> Option<Logo> {
    logo_read(body, utf8)
}

/// Reads a broadcaster name descriptor (STD-B10 6.2.36, MH 0x8018).
pub fn broadcaster_name(body: &[u8]) -> String {
    text(body)
}

pub fn mh_broadcaster_name(body: &[u8]) -> String {
    utf8(body)
}

/// What an extended broadcaster descriptor (0xCE, STD-B10 6.2.43) says of
/// a terrestrial broadcaster: its identifier, the affiliations (networks
/// of stations) it belongs to, and the same broadcaster on other
/// networks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtendedBroadcaster {
    /// 1 television, 2 sound; anything else is reserved and carries
    /// nothing readable.
    pub kind: u8,
    pub terrestrial_broadcaster_id: u16,
    pub affiliations: Vec<u8>,
    /// `(original_network_id, broadcaster_id)` of the same broadcaster
    /// elsewhere.
    pub related: Vec<(u16, u8)>,
}

/// Reads an extended broadcaster descriptor.
pub fn extended_broadcaster(body: &[u8]) -> Option<ExtendedBroadcaster> {
    let kind = *body.first()? >> 4;
    let mut read = ExtendedBroadcaster {
        kind,
        ..ExtendedBroadcaster::default()
    };
    if !matches!(kind, 1 | 2) {
        return Some(read);
    }
    let head = body.get(1..4)?;
    read.terrestrial_broadcaster_id = u16::from_be_bytes([head[0], head[1]]);
    let affiliations = usize::from(head[2] >> 4);
    let others = usize::from(head[2] & 0x0f);
    let mut at = 4;
    read.affiliations = body.get(at..at + affiliations)?.to_vec();
    at += affiliations;
    for _ in 0..others {
        let entry = body.get(at..at + 3)?;
        read.related
            .push((u16::from_be_bytes([entry[0], entry[1]]), entry[2]));
        at += 3;
    }
    Some(read)
}

/// What a related broadcaster descriptor (0x803E, STD-B60 7.4.3.46) says:
/// the broadcasters on other networks that share this one's NVRAM.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RelatedBroadcaster {
    /// `(network_id, broadcaster_id)`.
    pub broadcasters: Vec<(u16, u8)>,
    pub affiliations: Vec<u8>,
    pub original_networks: Vec<u16>,
}

/// Reads a related broadcaster descriptor.
pub fn related_broadcaster(body: &[u8]) -> Option<RelatedBroadcaster> {
    let head = body.get(..2)?;
    let (broadcasters, affiliations, networks) = (
        usize::from(head[0] >> 4),
        usize::from(head[0] & 0x0f),
        usize::from(head[1] >> 4),
    );
    let mut read = RelatedBroadcaster::default();
    let mut at = 2;
    for _ in 0..broadcasters {
        let entry = body.get(at..at + 3)?;
        read.broadcasters
            .push((u16::from_be_bytes([entry[0], entry[1]]), entry[2]));
        at += 3;
    }
    read.affiliations = body.get(at..at + affiliations)?.to_vec();
    at += affiliations;
    for _ in 0..networks {
        let entry = body.get(at..at + 2)?;
        read.original_networks
            .push(u16::from_be_bytes([entry[0], entry[1]]));
        at += 2;
    }
    Some(read)
}

/// What a CA_EMM_TS descriptor (0xCA, STD-B25) says: on which stream the
/// EMMs of a CA system ride, and how long a receiver should stay powered
/// to collect them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaEmmTs {
    pub ca_system_id: u16,
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    /// In minutes.
    pub power_supply_period: u8,
}

/// Reads a CA_EMM_TS descriptor.
pub fn ca_emm_ts(body: &[u8]) -> Option<CaEmmTs> {
    let bytes = body.get(..7)?;
    Some(CaEmmTs {
        ca_system_id: u16::from_be_bytes([bytes[0], bytes[1]]),
        transport_stream_id: u16::from_be_bytes([bytes[2], bytes[3]]),
        original_network_id: u16::from_be_bytes([bytes[4], bytes[5]]),
        power_supply_period: bytes[6],
    })
}

/// What a video decode control descriptor (0xC8, STD-B10 6.2.30) says of
/// a video stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VideoDecodeControl {
    /// The stream is still pictures.
    pub still_picture: bool,
    /// Every sequence ends with a sequence end code (so a decoder may be
    /// reset there).
    pub sequence_end_code: bool,
    /// `video_encode_format` (Table 6-64): 0 1080p, 1 1080i, 2 720p, 3
    /// 480p, 4 480i, 5 240p, 6 120p, 7 2160p, 8 180p, 9 4320p.
    pub encode_format: u8,
    /// `transfer_characteristics` (v5.x): 0 unsaid, 1 SDR BT.709, 2 IEC
    /// 61966-2-4, 3 HLG.
    pub transfer: u8,
}

impl VideoDecodeControl {
    pub fn format_name(&self) -> Option<&'static str> {
        Some(match self.encode_format {
            0 => "1080p",
            1 => "1080i",
            2 => "720p",
            3 => "480p",
            4 => "480i",
            5 => "240p",
            6 => "120p",
            7 => "2160p",
            8 => "180p",
            9 => "4320p",
            _ => return None,
        })
    }
}

/// Reads a video decode control descriptor.
pub fn video_decode_control(body: &[u8]) -> Option<VideoDecodeControl> {
    let byte = *body.first()?;
    Some(VideoDecodeControl {
        still_picture: byte & 0x80 != 0,
        sequence_end_code: byte & 0x40 != 0,
        encode_format: (byte >> 2) & 0x0f,
        transfer: byte & 0x03,
    })
}

/// Reads an access control descriptor (0xF6, MH 0x8004): the CA system and
/// the identifier its messages ride on, like the CA descriptor. Returns
/// `(system, pid, transmission_type)`.
pub fn access_control(body: &[u8]) -> Option<(u16, u16, u8)> {
    let bytes = body.get(..4)?;
    Some((
        u16::from_be_bytes([bytes[0], bytes[1]]),
        u16::from_be_bytes([bytes[2] & 0x1f, bytes[3]]),
        bytes[2] >> 5,
    ))
}

/// What a network identification descriptor (0xC2, in a SIT) says: which
/// network the partial transport stream was recorded from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NetworkIdentification {
    pub country: String,
    /// 0x0001 terrestrial, 0x0002 BS/CS ... as the standard numbers them.
    pub media_type: u16,
    pub network_id: u16,
    pub private_data: Vec<u8>,
}

/// Reads a network identification descriptor.
pub fn network_identification(body: &[u8]) -> Option<NetworkIdentification> {
    let head = body.get(..7)?;
    Some(NetworkIdentification {
        country: language(&head[..3]),
        media_type: u16::from_be_bytes([head[3], head[4]]),
        network_id: u16::from_be_bytes([head[5], head[6]]),
        private_data: body[7..].to_vec(),
    })
}

/// What a partial transport stream time descriptor (0xC3, in a SIT) says:
/// when the recorded event was, and where in it the recording starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PartialTsTime {
    pub event_version: u8,
    pub event_start: Option<tstables::BroadcastTime>,
    pub duration: Option<u32>,
    /// How far from the event's start the stream begins, in seconds.
    pub offset: Option<u32>,
    /// `offset_flag`: which way the offset counts, as the standard codes it.
    pub offset_flag: bool,
    /// Whether the other descriptors describe the event as it was
    /// (`other_descriptor_status`).
    pub others_valid: bool,
    /// The wall clock at the start of the recording, where said.
    pub jst_time: Option<tstables::BroadcastTime>,
}

/// Reads a partial transport stream time descriptor.
pub fn partial_ts_time(body: &[u8]) -> Option<PartialTsTime> {
    let head = body.get(..13)?;
    let flags = head[12];
    Some(PartialTsTime {
        event_version: head[0],
        event_start: tstables::broadcast_time(&head[1..6]),
        duration: tstables::duration(&head[6..9]),
        offset: tstables::duration(&head[9..12]),
        offset_flag: flags & 0x04 != 0,
        others_valid: flags & 0x02 != 0,
        jst_time: (flags & 0x01 != 0)
            .then(|| body.get(13..18).and_then(tstables::broadcast_time))
            .flatten(),
    })
}

/// What a partial transport stream descriptor (0x63, in a SIT) says of the
/// stream's rate, in units of 400 bit/s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PartialTs {
    pub peak_rate: u32,
    pub minimum_smoothing_rate: u32,
    pub maximum_smoothing_buffer: u16,
}

/// Reads a partial transport stream descriptor.
pub fn partial_ts(body: &[u8]) -> Option<PartialTs> {
    let bytes = body.get(..8)?;
    Some(PartialTs {
        peak_rate: u32::from_be_bytes([bytes[0] & 0x3f, bytes[1], bytes[2], 0]) >> 8,
        minimum_smoothing_rate: u32::from_be_bytes([bytes[3] & 0x3f, bytes[4], bytes[5], 0]) >> 8,
        maximum_smoothing_buffer: u16::from_be_bytes([bytes[6] & 0x3f, bytes[7]]),
    })
}

/// What a download content descriptor (0xC9, STD-B21) says of one piece
/// of software: enough to show, not to fetch it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DownloadContentInfo {
    pub reboot: bool,
    pub add_on: bool,
    pub component_size: u32,
    pub download_id: u32,
    pub timeout_dii: u32,
    /// In units of 50 bytes/s.
    pub leak_rate: u32,
    pub component_tag: u8,
    /// The text, where the descriptor carried one.
    pub text: Option<(String, String)>,
}

/// Reads a download content descriptor, skipping the compatibility and
/// module loops.
pub fn download_content(body: &[u8]) -> Option<DownloadContentInfo> {
    let head = body.get(..17)?;
    let flags = head[0];
    let mut read = DownloadContentInfo {
        reboot: flags & 0x80 != 0,
        add_on: flags & 0x40 != 0,
        component_size: u32::from_be_bytes([head[1], head[2], head[3], head[4]]),
        download_id: u32::from_be_bytes([head[5], head[6], head[7], head[8]]),
        timeout_dii: u32::from_be_bytes([head[9], head[10], head[11], head[12]]),
        leak_rate: u32::from_be_bytes([head[13] & 0x3f, head[14], head[15], 0]) >> 8,
        component_tag: head[16],
        text: None,
    };
    let mut at = 17;
    if flags & 0x20 != 0 {
        let length = usize::from(u16::from_be_bytes([*body.get(at)?, *body.get(at + 1)?]));
        at += 2 + length;
    }
    if flags & 0x10 != 0 {
        let count = usize::from(u16::from_be_bytes([*body.get(at)?, *body.get(at + 1)?]));
        at += 2;
        for _ in 0..count {
            let length = usize::from(*body.get(at + 6)?);
            at += 7 + length;
        }
    }
    let private = usize::from(*body.get(at)?);
    at += 1 + private;
    if flags & 0x08 != 0 {
        let language = language(body.get(at..at + 3)?);
        let length = usize::from(*body.get(at + 3)?);
        let text = download_text(body.get(at + 4..at + 4 + length)?);
        read.text = Some((language, text));
    }
    Some(read)
}

/// The text of a download content descriptor. **Broadcasters send it as
/// EUC-JP**, not as the eight-bit code the other descriptors use: two
/// bytes per character with both high bits set, and CR LF between lines
/// (seen on the BS stations' announcements). Read as
/// the eight-bit code, every pair comes out as two kana. So it is taken as
/// EUC-JP when it decodes cleanly as that, and as the eight-bit code
/// otherwise. Some makers' notices come in UTF-8 instead; read as the
/// eight-bit code those turn into rows of kana from the lead bytes, the
/// continuation bytes dropped as C1 controls. Neither EUC-JP nor the
/// eight-bit code decodes cleanly as UTF-8 once it has a byte above 0x7f
/// (both leave their 0xa1 .. 0xfe bytes without a lead), so UTF-8 goes
/// first.
fn download_text(bytes: &[u8]) -> String {
    if !bytes.is_ascii()
        && let Ok(utf8) = std::str::from_utf8(bytes)
    {
        return utf8.to_owned();
    }
    // The eight-bit code shows itself by its escapes and C1 controls;
    // EUC-JP has neither.
    let eight_bit = bytes
        .iter()
        .any(|&b| b == 0x1b || (0x80..=0xa0).contains(&b));
    // Text with nothing above 0x7f is either plain ASCII (a version, a
    // file name) or the eight-bit code in its initial state -- kanji in
    // two seven-bit bytes, as one maker's notices come. A Japanese notice
    // has kana and punctuation, whose first bytes are 0x21 .. 0x25; ASCII
    // tokens have letters and digits.
    let seven_bit_kanji = bytes.iter().all(|&b| b < 0x80)
        && bytes
            .split(|&b| b < 0x21)
            .filter(|run| !run.is_empty())
            .all(|run| {
                run.len() % 2 == 0 && run.iter().step_by(2).any(|b| (0x21..=0x25).contains(b))
            });
    if !eight_bit && !seven_bit_kanji {
        let (decoded, wrong) = encoding_rs::EUC_JP.decode_without_bom_handling(bytes);
        if !wrong {
            return decoded.into_owned();
        }
    }
    text(bytes)
}

/// The prefectures a target region descriptor (0xC6, MH 0x8015) names, in
/// the order of STD-B10 Annex G Table G-2 (1 East Hokkaido .. 47 Kagoshima,
/// 48 Okinawa, 49 the islands of Tokyo, 50 the islands of Kagoshima).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TargetRegion {
    /// `region_spec_type`: 1 is the prefecture bitmap of BS digital.
    pub kind: u8,
    /// The bits that were set, numbered from one.
    pub prefectures: Vec<u8>,
    /// The specifier as sent, for a kind this reader does not know.
    pub raw: Vec<u8>,
}

/// The names behind Table G-2, numbered from one.
pub fn prefecture_name(bit: u8) -> Option<&'static str> {
    const NAMES: [&str; 50] = [
        "北海道（東）",
        "北海道（西）",
        "青森",
        "岩手",
        "宮城",
        "秋田",
        "山形",
        "福島",
        "茨城",
        "栃木",
        "群馬",
        "埼玉",
        "千葉",
        "東京（島部を除く）",
        "神奈川",
        "新潟",
        "富山",
        "石川",
        "福井",
        "山梨",
        "長野",
        "岐阜",
        "静岡",
        "愛知",
        "三重",
        "滋賀",
        "京都",
        "大阪",
        "兵庫",
        "奈良",
        "和歌山",
        "鳥取",
        "島根",
        "岡山",
        "広島",
        "山口",
        "徳島",
        "香川",
        "愛媛",
        "高知",
        "福岡",
        "佐賀",
        "長崎",
        "熊本",
        "大分",
        "宮崎",
        "鹿児島（南西諸島を除く）",
        "沖縄",
        "東京（島部）",
        "鹿児島（南西諸島）",
    ];
    NAMES.get(usize::from(bit.checked_sub(1)?)).copied()
}

/// Reads a target region descriptor. The MH form is the same bytes.
pub fn target_region(body: &[u8]) -> Option<TargetRegion> {
    let kind = *body.first()?;
    let spec = &body[1..];
    let mut read = TargetRegion {
        kind,
        prefectures: Vec::new(),
        raw: spec.to_vec(),
    };
    if kind == 0x01 {
        for (index, byte) in spec.iter().take(7).enumerate() {
            for bit in 0..8 {
                if byte & (0x80 >> bit) != 0 {
                    read.prefectures.push((index * 8 + bit + 1) as u8);
                }
            }
        }
    }
    Some(read)
}

/// What a hyperlink descriptor (0xC5, STD-B10 6.2.29) points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkDestination {
    /// 0x01: a service.
    Service {
        network: u16,
        transport: u16,
        service: u16,
    },
    /// 0x02: an event.
    Event {
        network: u16,
        transport: u16,
        service: u16,
        event: u16,
    },
    /// 0x03: a module of an event's data broadcast.
    Module {
        network: u16,
        transport: u16,
        service: u16,
        event: u16,
        component: u8,
        module: u16,
    },
    /// 0x04: content.
    Content {
        network: u16,
        transport: u16,
        service: u16,
        content: u32,
    },
    /// 0x05: a module of content.
    ContentModule {
        network: u16,
        transport: u16,
        service: u16,
        content: u32,
        component: u8,
        module: u16,
    },
    /// 0x06: an event's ERT node.
    ErtNode {
        information_provider: u16,
        event_relation: u16,
        node: u16,
    },
    /// 0x07: a stored content (a URI).
    Stored(String),
    /// A kind this reader does not know: the selector as sent.
    Other(u8, Vec<u8>),
}

/// A hyperlink descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hyperlink {
    /// `hyper_linkage_type` (Table 6-50): 1 combined data, 2 combined
    /// stream, 3 content to index, 4 index to content, 5 guide to content,
    /// 6 guide to content module, 7 guide to content index, 8 portal.
    pub kind: u8,
    pub destination: LinkDestination,
    pub private_data: Vec<u8>,
}

/// Reads a hyperlink descriptor.
pub fn hyperlink(body: &[u8]) -> Option<Hyperlink> {
    let head = body.get(..3)?;
    let length = usize::from(head[2]);
    let selector = body.get(3..3 + length)?;
    let be16 = |at: usize| {
        selector
            .get(at..at + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
    };
    let be32 = |at: usize| {
        selector
            .get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let destination = match head[1] {
        0x01 => LinkDestination::Service {
            network: be16(0)?,
            transport: be16(2)?,
            service: be16(4)?,
        },
        0x02 => LinkDestination::Event {
            network: be16(0)?,
            transport: be16(2)?,
            service: be16(4)?,
            event: be16(6)?,
        },
        0x03 => LinkDestination::Module {
            network: be16(0)?,
            transport: be16(2)?,
            service: be16(4)?,
            event: be16(6)?,
            component: *selector.get(8)?,
            module: be16(9)?,
        },
        0x04 => LinkDestination::Content {
            network: be16(0)?,
            transport: be16(2)?,
            service: be16(4)?,
            content: be32(6)?,
        },
        0x05 => LinkDestination::ContentModule {
            network: be16(0)?,
            transport: be16(2)?,
            service: be16(4)?,
            content: be32(6)?,
            component: *selector.get(10)?,
            module: be16(11)?,
        },
        0x06 => LinkDestination::ErtNode {
            information_provider: be16(0)?,
            event_relation: be16(2)?,
            node: be16(4)?,
        },
        0x07 => LinkDestination::Stored(String::from_utf8_lossy(selector).into_owned()),
        other => LinkDestination::Other(other, selector.to_vec()),
    };
    Some(Hyperlink {
        kind: head[0],
        destination,
        private_data: body[3 + length..].to_vec(),
    })
}

/// What a multimedia service information descriptor (0x803F, STD-B60
/// 7.4.3.47) says of an HTML5 data service.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MultimediaService {
    /// The data coding system (0x0020 the application, 0x0021 the
    /// additional data of a programme).
    pub data_component_id: u16,
    /// For 0x0020: the component and a description.
    pub component_tag: Option<u16>,
    pub language: String,
    pub text: String,
    /// For 0x0021: whether the service has content tied to the programme.
    pub associated_contents: Option<bool>,
    pub selector: Vec<u8>,
}

/// Reads a multimedia service information descriptor.
pub fn multimedia_service_info(body: &[u8]) -> Option<MultimediaService> {
    let id = u16::from_be_bytes([*body.first()?, *body.get(1)?]);
    let mut read = MultimediaService {
        data_component_id: id,
        ..MultimediaService::default()
    };
    let mut at = 2;
    match id {
        0x0020 => {
            read.component_tag = Some(u16::from_be_bytes([*body.get(at)?, *body.get(at + 1)?]));
            read.language = language(body.get(at + 2..at + 5)?);
            let length = usize::from(*body.get(at + 5)?);
            read.text = utf8(body.get(at + 6..at + 6 + length)?);
            at += 6 + length;
        }
        0x0021 => {
            read.associated_contents = Some(*body.get(at)? & 0x80 != 0);
            at += 1;
        }
        _ => {}
    }
    let length = usize::from(*body.get(at)?);
    read.selector = body.get(at + 1..at + 1 + length)?.to_vec();
    Some(read)
}

/// Reads an emergency news descriptor (0x8040, STD-B60 7.4.3.43): when the
/// bulletin was first sent, as an NTP timestamp. The same value for as long
/// as the same bulletin runs.
pub fn emergency_news(body: &[u8]) -> Option<u64> {
    let bytes = body.get(..8)?;
    Some(u64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

/// Reads an MH access control descriptor (0x8004, STD-B60 7.4.3.7): the CA
/// system and where its messages are (the EMM in the MH-CAT, the ECM in
/// the MPT).
pub fn mh_access_control(body: &[u8]) -> Option<(u16, mmttables::PackageLocation)> {
    let system = u16::from_be_bytes([*body.first()?, *body.get(1)?]);
    let (location, _) = mmttables::general_location(&body[2..])?;
    Some((system, location))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_audio_component_reads_the_same_from_both_transmissions() {
        // stream_content/type, tag, stream_type, simulcast, flags(main, 48 kHz),
        // language "jpn".
        let ts = [0x02, 0x03, 0x11, 0x0f, 0xff, 0x4e, b'j', b'p', b'n'];
        let read = audio_component(&ts).expect("reads");
        assert_eq!(read.tag, Some(0x11));
        assert_eq!(read.language, "jpn");
        assert!(read.main);
        assert_eq!(read.sampling_hz(), Some(48_000));
        assert_eq!(read.channels(), 2);
        assert_eq!(read.simulcast, None);
        let mh = [0x02, 0x03, 0x00, 0x11, 0x0f, 0xff, 0x4e, b'j', b'p', b'n'];
        let read_mh = mh_audio_component(&mh).expect("reads");
        assert_eq!(read_mh.tag, Some(0x11));
        assert_eq!(read_mh.channels(), read.channels());
    }

    #[test]
    fn a_dual_mono_commentary_is_still_dual_mono() {
        let ts = [
            0x02, 0x22, 0x11, 0x0f, 0xff, 0xc0, b'j', b'p', b'n', b'e', b'n', b'g',
        ];
        let read = audio_component(&ts).expect("reads");
        assert!(read.dual_mono());
        assert_eq!(read.assist(), Assist::Visual);
        assert_eq!(read.second.as_deref(), Some("eng"));
    }

    #[test]
    fn the_hierarchy_levels_agree_between_transmissions() {
        // TS: quality_level 1 (the one to show), reference 0x0111.
        assert_eq!(
            hierarchical_transmission(&[0x01, 0x01, 0x11]),
            Some(Hierarchy {
                level: 0,
                reference: Some(0x0111),
                group: None
            })
        );
        assert_eq!(
            hierarchical_transmission(&[0x00, 0x1f, 0xff]).map(|h| (h.level, h.reference)),
            Some((1, None))
        );
        assert_eq!(
            asset_group(&[0x01, 0x00]),
            Some(Hierarchy {
                level: 0,
                reference: None,
                group: Some(1)
            })
        );
    }

    #[test]
    fn events_read_from_both_transmissions() {
        // The transport stream's text is the eight-unit code, in which two
        // ASCII bytes are one kanji: what is checked here is the framing.
        let mut ts = vec![b'j', b'p', b'n', 2, 0x46, 0x7c, 2, 0x4b, 0x5c];
        let read = short_event(&ts).expect("reads");
        assert_eq!((read.title.as_str(), read.text.as_str()), ("日", "本"));
        assert_eq!(read.language, "jpn");
        ts[3] = 9;
        assert!(short_event(&ts).is_none());
        let mh = [b'j', b'p', b'n', 2, b'A', b'B', 0, 1, b'C'];
        let read = mh_short_event(&mh).expect("reads");
        assert_eq!((read.title.as_str(), read.text.as_str()), ("AB", "C"));
        let read = content(&[0x12, 0x00, 0x34, 0xff, 0xe1, 0x10]);
        assert_eq!(
            read.iter().map(|c| (c.large, c.middle)).collect::<Vec<_>>(),
            [(1, 2), (3, 4), (0xe, 1)]
        );
        // 拡張（0xE1）の user_nibble：洋画（CS）の最初の中分類。
        assert_eq!(read[2].user, 0x10);
        assert_eq!(
            event_group(&[0x12, 0x04, 0x00, 0x00, 0x10, 0x04, 0x01, 0x00, 0x11]),
            Some((1, vec![(0x0400, 0x0010), (0x0401, 0x0011)]))
        );
    }

    #[test]
    fn an_application_service_reads_its_locations() {
        let body = [
            0x00, 0x10, 0x42, 0x00, 0x1f, 0x00, 0x00, 0x1f, 0x01, 0x07, 0x00, 0x1f, 0x02,
        ];
        let read = application_service(&body).expect("reads");
        assert_eq!(read.resolution, 1);
        assert_eq!(read.ait, 0x1f00);
        assert_eq!(read.data, Some(0x1f01));
        assert_eq!(read.emts, vec![(7, 0x1f02)]);
        assert_eq!(read.emt_tag(0x1f02), Some(7));
    }

    /// 視聴年齢は rating + 3、0 は未指定。
    #[test]
    fn a_parental_rating_is_an_age() {
        let read = parental_rating(b"JPN\x04");
        assert_eq!(read[0].country, "JPN");
        assert_eq!(rating_age(read[0].rating), Some(7));
        assert_eq!(rating_age(0), None);
        assert_eq!(rating_age(0x10), None);
    }

    /// デジタルコピー制御：全体と成分ごと、最大ビットレートはフラグが立った
    /// ときだけ。MH バージョンは予備の一バイトが挟まり、タグが二バイト。
    // ビットフィールドごとに書く。`| 0x00` は「そのフィールドが 0」の意味。
    #[allow(clippy::identity_op)]
    #[test]
    fn copy_control_is_read_for_the_whole_and_each_component() {
        // 一世代のみ、最大ビットレートあり（0x1c）、成分二つ。
        let read =
            digital_copy_control(&[0xa0 | 0x10, 0x1c, 0x05, 0x10, 0xc0, 0x11, 0x20 | 0x00, 0x08])
                .expect("read");
        assert_eq!(read.control, 2);
        assert_eq!(read.maximum_bitrate, Some(0x1c));
        assert_eq!(read.components.len(), 2);
        assert_eq!(
            (read.components[0].tag, read.components[0].control),
            (0x10, 3)
        );
        assert_eq!(read.components[1].maximum_bitrate, Some(8));
        let mh = mh_content_copy_control(&[0x80 | 0x10, 0x00, 0x04, 0x00, 0x10, 0xc0, 0x00])
            .expect("read");
        assert_eq!(mh.control, 2);
        assert_eq!(mh.components[0].tag, 0x0010);
        assert_eq!(mh.components[0].control, 3);
        assert_eq!(CopyControl::control_name(2), "一世代のみコピー可");
    }

    /// コンテント利用記述子：ダビング10 と保持時間。
    #[test]
    fn content_availability_says_how_long_it_may_be_kept() {
        let read = content_availability(&[0x40 | 0x20 | 0x0c]).expect("read");
        assert!(read.copy_restriction && read.image_unconstrained && !read.no_retention);
        assert_eq!(read.retention_state, 6);
        assert_eq!(read.retention_name(), "1時間30分");
        let mh = mh_content_usage_control(&[0x80, 0x0e]).expect("read");
        assert_eq!(mh.remote_view, Some(true));
        assert_eq!(mh.retention_name(), "予備");
    }

    /// シリーズ記述子：番号は 12 ビット二つが三バイトに詰まる。
    #[test]
    fn a_series_counts_its_episodes() {
        let mut body = vec![0x12, 0x34, 0x02 | 0x01, 0xe5, 0x2c, 0x00, 0x51, 0x0c];
        body.extend_from_slice(&[0x46, 0x7c]);
        let read = series(&body).expect("read");
        assert_eq!(read.id, 0x1234);
        assert_eq!(read.pattern, 1);
        assert_eq!(read.pattern_name(), "帯番組");
        assert_eq!(read.expire_mjd, Some(0xe52c));
        assert_eq!((read.episode, read.last_episode), (5, 268));
        assert_eq!(read.name, "日");
    }

    /// コンポーネントグループ：マルチビューの各グループと、その成分。
    #[test]
    fn component_groups_are_read_with_their_names() {
        let body = [
            0x10 | 0x02,
            0x01,
            0x12,
            0x00,
            0x10,
            0x20,
            0x00,
            0x11,
            0x01,
            0x11,
            0x00,
            0x00,
        ];
        let read = component_group(&body).expect("read");
        assert_eq!(read.group_type, 0);
        assert_eq!(read.groups.len(), 2);
        assert_eq!(read.groups[0].units[0].components, [0x00, 0x10]);
        assert_eq!(read.groups[0].total_bitrate, Some(0x20));
        assert_eq!(read.groups[1].id, 1);
        assert_eq!(read.groups[1].units[0].components, [0x11]);
    }

    /// ロゴ伝送記述子の三つの型。
    #[test]
    fn a_logo_is_downloaded_referenced_or_spelled() {
        assert_eq!(
            logo_transmission(&[0x01, 0xfe, 0x21, 0xf0, 0x02, 0x12, 0x34]),
            Some(Logo::Downloaded {
                logo_id: 0x21,
                version: 2,
                download_data_id: 0x1234
            })
        );
        assert_eq!(
            logo_transmission(&[0x02, 0x01, 0x05]),
            Some(Logo::Referenced { logo_id: 0x105 })
        );
        assert_eq!(
            logo_transmission(&[0x03, 0x0e, b'S', b'M', b'P']),
            Some(Logo::Text("ＳＭＰ".into()))
        );
    }

    /// 拡張ブロードキャスタ記述子と関連ブロードキャスタ記述子。
    #[test]
    fn broadcasters_name_their_affiliations() {
        let read = extended_broadcaster(&[0x10, 0x00, 0x2a, 0x21, 0x05, 0x50, 0x01, 0x03, 0x00])
            .expect("read");
        assert_eq!(read.kind, 1);
        assert_eq!(read.terrestrial_broadcaster_id, 0x2a);
        assert_eq!(read.affiliations, [0x05, 0x50]);
        assert_eq!(read.related, [(0x0103, 0)]);
        let related =
            related_broadcaster(&[0x11, 0x10, 0x00, 0x04, 0x02, 0x05, 0x7f, 0xe1]).expect("read");
        assert_eq!(related.broadcasters, [(4, 2)]);
        assert_eq!(related.affiliations, [5]);
        assert_eq!(related.original_networks, [0x7fe1]);
    }

    /// CA 契約情報：料金の名前まで。
    #[test]
    fn a_ca_contract_names_its_fee() {
        let body = [
            0x00, 0x05, 0x12, 0x10, 0x11, 0x02, 0xaa, 0xbb, 0x02, 0x46, 0x7c,
        ];
        let read = ca_contract_info(&body).expect("read");
        assert_eq!((read.ca_system_id, read.unit), (5, 1));
        assert_eq!(read.components, [0x10, 0x11]);
        assert_eq!(read.verification, [0xaa, 0xbb]);
        assert_eq!(read.fee_name, "日");
        let emm = ca_emm_ts(&[0x00, 0x05, 0x40, 0x10, 0x00, 0x04, 0x1e]).expect("read");
        assert_eq!(emm.transport_stream_id, 0x4010);
        assert_eq!(emm.power_supply_period, 30);
    }
}

#[cfg(test)]
mod download_text_tests {
    use super::download_text;

    /// 放送の告知の文字列は EUC-JP。八単位符号として読むと仮名の並びになる。
    #[test]
    fn announcement_text_is_euc_jp() {
        let bytes = [
            0xa4, 0xb3, 0xa4, 0xb3, 0xa4, 0xcf, 0xc9, 0xbd, 0xbc, 0xa8, 0x0d, 0x0a, 0x30, 0x31,
        ];
        assert_eq!(download_text(&bytes), "ここは表示\r\n01");
        assert_eq!(download_text(b"000276"), "000276");
        assert_eq!(download_text(b"SharpOAD.bin"), "SharpOAD.bin");
        // 七ビットの漢字（別のメーカーの告知）。
        assert_eq!(download_text(b"%=%U%H%&%'%\"!#"), "ソフトウェア。");
        // UTF-8 の告知（メーカー 0x28 の機種）。
        assert_eq!(
            download_text("ソフトウェア更新\r\n　お知らせ".as_bytes()),
            "ソフトウェア更新\r\n　お知らせ"
        );
    }
}

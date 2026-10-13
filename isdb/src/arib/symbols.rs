// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The tables of the broadcast character encoding that are the broadcast's own.
//!
//! What the standard shares with every other Japanese encoding -- the 6879
//! characters of JIS X 0208 -- is not here: [`super`] reads it out of a general
//! table. What is here is what only broadcasting has: the rows added past the
//! end of the standard's own, the cells that stand for something the standard
//! already holds, the combining marks, and the words to put where a symbol
//! cannot be drawn.

/// The symbols the broadcast adds to the standard's own rows: rows 0x75 and up
/// of the two-byte set. Written as the runs they were assigned in, a run's
/// first cell and then one character per cell. A hole in a run is a cell
/// nothing was assigned to.
pub(super) static ADDED: &[(u16, &str)] = &[
    (
        0x7521,
        "\u{3402}\u{20158}\u{4efd}\u{4eff}\u{4f9a}\u{4fc9}\u{509c}\u{511e}\u{51bc}\u{351f}\u{5307}\u{5361}",
    ),
    (
        0x752d,
        "\u{536c}\u{8a79}\u{20bb7}\u{544d}\u{5496}\u{549c}\u{54a9}\u{550e}\u{554a}\u{5672}\u{56e4}\u{5733}",
    ),
    (
        0x7539,
        "\u{5734}\u{fa10}\u{5880}\u{59e4}\u{5a23}\u{5a55}\u{5bec}\u{fa11}\u{37e2}\u{5eac}\u{5f34}\u{5f45}\u{5fb7}\u{6017}",
    ),
    (
        0x7547,
        "\u{fa6b}\u{6130}\u{6624}\u{66c8}\u{0}\u{66fa}\u{66fb}\u{6852}\u{9fc4}\u{6911}\u{693b}\u{6a45}\u{6a91}\u{0}\u{233cc}",
    ),
    (
        0x7556,
        "\u{233fe}\u{235c4}\u{6bf1}\u{6ce0}\u{6d2e}\u{fa45}\u{6dbf}\u{6dca}\u{6df8}\u{fa46}\u{6f5e}",
    ),
    (
        0x7561,
        "\u{6ff9}\u{7064}\u{fa6c}\u{242ee}\u{7147}\u{71c1}\u{7200}\u{739f}\u{73a8}\u{73c9}\u{73d6}\u{741b}\u{7421}",
    ),
    (
        0x756e,
        "\u{fa4a}\u{7426}\u{742a}\u{742c}\u{7439}\u{744b}\u{3eda}\u{7575}\u{7581}\u{7772}\u{4093}\u{78c8}",
    ),
    (0x757a, "\u{78e0}\u{0}\u{0}\u{9fc6}\u{4103}"),
    (
        0x7621,
        "\u{9fc5}\u{79da}\u{7a1e}\u{7b7f}\u{7c31}\u{4264}\u{7d8b}\u{7fa1}\u{8118}\u{813a}\u{fa6d}\u{82ae}",
    ),
    (
        0x762e,
        "\u{84dc}\u{0}\u{8559}\u{85ce}\u{0}\u{87ec}\u{880b}\u{88f5}\u{0}\u{8af6}\u{8dce}\u{0}\u{8ff6}\u{90dd}\u{9127}",
    ),
    (
        0x763e,
        "\u{91b2}\u{9233}\u{9288}\u{9321}\u{9348}\u{9592}\u{96de}\u{0}\u{9940}\u{9ad9}\u{0}\u{9dd7}\u{9eb4}\u{9eb5}",
    ),
    (
        0x7a21,
        "\u{26cc}\u{26cd}\u{2757}\u{26cf}\u{26d0}\u{26d1}\u{0}\u{26d2}\u{26d5}\u{26d3}\u{26d4}",
    ),
    (
        0x7a30,
        "\u{1f17f}\u{1f18a}\u{0}\u{0}\u{26d6}\u{26d7}\u{26d8}\u{26d9}\u{26da}\u{26db}\u{26dc}",
    ),
    (
        0x7a3b,
        "\u{26dd}\u{26de}\u{26df}\u{26e0}\u{26e1}\u{2b55}\u{3248}\u{3249}\u{324a}\u{324b}\u{324c}",
    ),
    (0x7a46, "\u{324d}\u{324e}\u{324f}"),
    (
        0x7a4d,
        "\u{2491}\u{2492}\u{2493}\u{1f14a}\u{1f14c}\u{1f13f}\u{1f146}\u{1f14b}",
    ),
    (
        0x7a55,
        "\u{1f210}\u{1f211}\u{1f212}\u{1f213}\u{1f142}\u{1f214}\u{1f215}",
    ),
    (
        0x7a5c,
        "\u{1f216}\u{1f14d}\u{1f131}\u{1f13d}\u{2b1b}\u{2b24}\u{1f217}\u{1f218}",
    ),
    (
        0x7a64,
        "\u{1f219}\u{1f21a}\u{1f21b}\u{26bf}\u{1f21c}\u{1f21d}\u{1f21e}",
    ),
    (
        0x7a6b,
        "\u{1f21f}\u{1f220}\u{1f221}\u{1f222}\u{1f223}\u{1f224}\u{1f225}",
    ),
    (0x7a72, "\u{1f14e}\u{3299}\u{1f200}"),
    (
        0x7b21,
        "\u{26e3}\u{2b56}\u{2b57}\u{2b58}\u{2b59}\u{2613}\u{328b}\u{0}\u{26e8}\u{3246}\u{3245}\u{26e9}",
    ),
    (
        0x7b2d,
        "\u{fd6}\u{26ea}\u{26eb}\u{26ec}\u{2668}\u{26ed}\u{26ee}\u{26ef}\u{2693}\u{2708}\u{26f0}",
    ),
    (
        0x7b38,
        "\u{26f1}\u{26f2}\u{26f3}\u{26f4}\u{26f5}\u{1f157}\u{24b9}\u{24c8}\u{26f6}\u{1f15f}",
    ),
    (
        0x7b42,
        "\u{1f18b}\u{1f18d}\u{1f18c}\u{1f179}\u{26f7}\u{26f8}\u{26f9}\u{26fa}\u{1f17b}",
    ),
    (
        0x7b4b,
        "\u{260e}\u{26fb}\u{26fc}\u{26fd}\u{26fe}\u{1f17c}\u{26ff}",
    ),
    (0x7c21, "\u{27a1}\u{2b05}\u{2b06}\u{2b07}\u{2b2f}\u{2b2e}"),
    (
        0x7c2b,
        "\u{33a1}\u{33a5}\u{339d}\u{33a0}\u{33a4}\u{1f100}\u{2488}\u{2489}\u{248a}\u{248b}\u{248c}\u{248d}",
    ),
    (0x7c37, "\u{248e}\u{248f}\u{2490}"),
    (
        0x7c40,
        "\u{1f101}\u{1f102}\u{1f103}\u{1f104}\u{1f105}\u{1f106}\u{1f107}",
    ),
    (
        0x7c47,
        "\u{1f108}\u{1f109}\u{1f10a}\u{3233}\u{3236}\u{3232}\u{3231}\u{3239}\u{3244}\u{25b6}\u{25c0}",
    ),
    (0x7c52, "\u{3016}\u{3017}\u{27d0}\u{b2}\u{b3}\u{1f12d}"),
    (
        0x7c76,
        "\u{1f12c}\u{1f12b}\u{3247}\u{1f190}\u{1f226}\u{213b}",
    ),
    (
        0x7d21,
        "\u{322a}\u{322b}\u{322c}\u{322d}\u{322e}\u{322f}\u{3230}\u{3237}\u{337e}\u{337d}\u{337c}\u{337b}\u{2116}\u{2121}",
    ),
    (
        0x7d2f,
        "\u{3036}\u{26be}\u{1f240}\u{1f241}\u{1f242}\u{1f243}\u{1f244}\u{1f245}",
    ),
    (
        0x7d37,
        "\u{1f246}\u{1f247}\u{1f248}\u{1f12a}\u{1f227}\u{1f228}\u{1f229}",
    ),
    (
        0x7d3f,
        "\u{1f22a}\u{1f22b}\u{1f22c}\u{1f22d}\u{1f22e}\u{1f22f}\u{1f230}",
    ),
    (
        0x7d46,
        "\u{1f231}\u{2113}\u{338f}\u{3390}\u{33ca}\u{339e}\u{33a2}\u{3371}\u{0}\u{0}\u{bd}\u{2189}",
    ),
    (
        0x7d52,
        "\u{2153}\u{2154}\u{bc}\u{be}\u{2155}\u{2156}\u{2157}\u{2158}\u{2159}\u{215a}\u{2150}",
    ),
    (
        0x7d5d,
        "\u{215b}\u{2151}\u{2152}\u{2600}\u{2601}\u{2602}\u{26c4}\u{2616}\u{2617}\u{26c9}\u{26ca}",
    ),
    (
        0x7d68,
        "\u{2666}\u{2665}\u{2663}\u{2660}\u{26cb}\u{2a00}\u{203c}\u{2049}\u{26c5}\u{2614}\u{26c6}",
    ),
    (
        0x7d73,
        "\u{2603}\u{26c7}\u{26a1}\u{26c8}\u{0}\u{269e}\u{269f}\u{266c}",
    ),
    (
        0x7e21,
        "\u{2160}\u{2161}\u{2162}\u{2163}\u{2164}\u{2165}\u{2166}\u{2167}\u{2168}\u{2169}\u{216a}\u{216b}\u{2470}\u{2471}\u{2472}\u{2473}\u{2474}\u{2475}",
    ),
    (
        0x7e33,
        "\u{2476}\u{2477}\u{2478}\u{2479}\u{247a}\u{247b}\u{247c}\u{247d}\u{247e}\u{247f}\u{3251}",
    ),
    (
        0x7e3e,
        "\u{3252}\u{3253}\u{3254}\u{1f110}\u{1f111}\u{1f112}\u{1f113}\u{1f114}",
    ),
    (
        0x7e46,
        "\u{1f115}\u{1f116}\u{1f117}\u{1f118}\u{1f119}\u{1f11a}\u{1f11b}",
    ),
    (
        0x7e4d,
        "\u{1f11c}\u{1f11d}\u{1f11e}\u{1f11f}\u{1f120}\u{1f121}\u{1f122}",
    ),
    (
        0x7e54,
        "\u{1f123}\u{1f124}\u{1f125}\u{1f126}\u{1f127}\u{1f128}\u{1f129}",
    ),
    (
        0x7e5b,
        "\u{3255}\u{3256}\u{3257}\u{3258}\u{3259}\u{325a}\u{2460}\u{2461}\u{2462}\u{2463}\u{2464}\u{2465}\u{2466}\u{2467}\u{2468}\u{2469}",
    ),
    (
        0x7e6b,
        "\u{246a}\u{246b}\u{246c}\u{246d}\u{246e}\u{246f}\u{2776}\u{2777}\u{2778}\u{2779}\u{277a}\u{277b}\u{277c}\u{277d}",
    ),
    (0x7e79, "\u{277e}\u{277f}\u{24eb}\u{24ec}\u{325b}"),
];

/// Cells of those rows that stand for a character the standard already
/// has, or for one of the private-use marks a receiver draws itself.
pub(super) static ALIASES: &[(u16, char)] = &[
    (0x754b, '\u{66d9}'), // 曙
    (0x7554, '\u{6adb}'), // 櫛
    (0x757b, '\u{7947}'), // 祇
    (0x757c, '\u{79ae}'), // 禮
    (0x762d, '\u{845b}'), // 葛
    (0x762f, '\u{84ec}'), // 蓬
    (0x7632, '\u{8755}'), // 蝕
    (0x7636, '\u{89d2}'), // 角
    (0x7639, '\u{8fbb}'), // 辻
    (0x763d, '\u{912d}'), // 鄭
    (0x7645, '\u{9903}'), // 餃
    (0x7648, '\u{9bd6}'), // 鯖
    (0x7b28, '\u{3012}'), // 〒
    (0x7c27, '\u{5e74}'), // 年
    (0x7c28, '\u{6708}'), // 月
    (0x7c29, '\u{65e5}'), // 日
    (0x7c2a, '\u{5186}'), // 円
    (0x7c3a, '\u{e290}'),
    (0x7c3b, '\u{e291}'),
    (0x7c3c, '\u{e292}'),
    (0x7c3d, '\u{e293}'),
    (0x7c3e, '\u{e294}'),
    (0x7c3f, '\u{e295}'),
    (0x7c58, '\u{e2a5}'),
    (0x7c59, '\u{e2a6}'),
    (0x7c5a, '\u{e2a7}'),
    (0x7c5b, '\u{e2a8}'),
    (0x7c5c, '\u{e2a9}'),
    (0x7c5d, '\u{e2aa}'),
    (0x7c5e, '\u{e2ab}'),
    (0x7c5f, '\u{e2ac}'),
    (0x7c60, '\u{e2ad}'),
    (0x7c61, '\u{e2ae}'),
    (0x7c62, '\u{e2af}'),
    (0x7c63, '\u{e2b0}'),
    (0x7c64, '\u{e2b1}'),
    (0x7c65, '\u{e2b2}'),
    (0x7c66, '\u{e2b3}'),
    (0x7c67, '\u{e2b4}'),
    (0x7c68, '\u{e2b5}'),
    (0x7c69, '\u{e2b6}'),
    (0x7c6a, '\u{e2b7}'),
    (0x7c6b, '\u{e2b8}'),
    (0x7c6c, '\u{e2b9}'),
    (0x7c6d, '\u{e2ba}'),
    (0x7c6e, '\u{e2bb}'),
    (0x7c6f, '\u{e2bc}'),
    (0x7c70, '\u{e2bd}'),
    (0x7c71, '\u{e2be}'),
    (0x7c72, '\u{e2bf}'),
    (0x7c73, '\u{e2c0}'),
    (0x7c74, '\u{e2c1}'),
    (0x7c75, '\u{e2c2}'),
    (0x7d3e, '\u{1f214}'), // 🈔
    (0x7d7b, '\u{260e}'),  // ☎
];

/// The marks that are combined with the character before them. They sit in
/// the standard's own rows but are not characters of it.
pub(super) static COMBINING: &[(u16, char)] = &[
    (0x212d, '\u{301}'),
    (0x212e, '\u{300}'),
    (0x212f, '\u{308}'),
    (0x2130, '\u{302}'),
    (0x2131, '\u{304}'),
    (0x2132, '\u{332}'),
    (0x227e, '\u{20dd}'),
];

/// What a symbol is written as where the symbol itself cannot be drawn. The
/// replacements are the broadcast's own words rather than this program's, which
/// is why some are Japanese: the mark that stands for 得 is read as 得 by
/// everyone who sees it.
pub(super) static SUBSTITUTES: &[(char, &str)] = &[
    ('🅏', "[WC]"),
    ('🆒', "[COOL]"),
    ('🆓', "[FREE]"),
    ('🆕', "[NEW]"),
    ('🆖', "[NG]"),
    ('🆗', "[OK]"),
    ('🆙', "[UP!]"),
    ('🆚', "[VS]"),
    ('🆛', "[3D]"),
    ('🆜', "[2ndScr]"),
    ('🆝', "[2K]"),
    ('🆞', "[4K]"),
    ('🆟', "[8K]"),
    ('🆠', "[5.1]"),
    ('🆡', "[7.1]"),
    ('🆢', "[22.2]"),
    ('🆣', "[60P]"),
    ('🆤', "[120P]"),
    ('🆥', "[d]"),
    ('🆦', "[HC]"),
    ('🆧', "[HDR]"),
    ('🆨', "[Hi-Res]"),
    ('🆩', "[Lossless]"),
    ('🆪', "[SHV]"),
    ('🆫', "[UHD]"),
    ('🆬', "[VOD]"),
    ('🈁', "[ココ]"),
    ('🈂', "[サ]"),
    ('🉐', "[得]"),
    ('🉑', "[可]"),
];

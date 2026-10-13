// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The delivery systems, by the names the settings use for them.
//!
//! The settings name what is received the way this country names it: ISDB-T,
//! ISDB-S, ISDB-C, and the optical distribution of the 124/128 degree east
//! service. The daemon, and the tuners behind it, name some of them after the
//! DVB API instead, which knows Japanese cable only as `DVBC/ANNEX_A` although
//! the two share nothing a demodulator would care about. So the file says
//! `ISDBC`, and the name the daemon takes is put on only at the point of asking
//! it.
//!
//! [`named`] takes the daemon's and the tuners' spellings to the settings'
//! name, [`same`] compares two, and [`wire`] gives what the daemon is told.

/// The settings' name for each delivery system this understands.
pub const ISDBT: &str = "ISDBT";
pub const ISDBS: &str = "ISDBS";
pub const ISDBS3: &str = "ISDBS3";
/// Japanese cable: ITU-T J.83 Annex C at 5.274 Msym/s, carrying the aerial
/// and satellite platforms transmodulated beside the operator's own channels.
pub const ISDBC: &str = "ISDBC";
/// The optical distribution the 124/128 degree east service is remodulated
/// onto: J.83 Annex B at 5.6 Msym/s, a rate North American cable never uses.
pub const J83B_5_6: &str = "J83B_5_6";
pub const DVBS: &str = "DVBS";
pub const DVBS2: &str = "DVBS2";

/// What the daemon calls the two systems whose settings name is not its own.
const WIRE_ISDBC: &str = "DVBC/ANNEX_A";
const WIRE_J83B_5_6: &str = "DVBC/ANNEX_B";

/// The systems a viewer is offered to choose from, with what to show for
/// each. Anything else is a name typed in by someone who knows what it is.
pub const OFFERED: [(&str, &str); 6] = [
    (ISDBT, "地上デジタル"),
    (ISDBS, "BS・110°CS"),
    (ISDBS3, "BS4K・CS4K"),
    (ISDBC, "ケーブルテレビ"),
    (J83B_5_6, "124,128°CS 光"),
    (DVBS2, "124,128°CS"),
];

/// The family a delivery system belongs to, which is what decides what a tune
/// of it takes and how its carriers are found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// One stream per carrier on a grid of channels.
    Terrestrial,
    /// The 110 degree east satellites: asked, not walked, because every
    /// transponder names its neighbours.
    Satellite,
    /// Japanese cable, walked on the cable grid at the cable's one rate.
    Cable,
    /// The optical distribution of the 124/128 degree east service: a grid of
    /// its own, at a rate and modulation the cable's figures would not lock.
    Optical,
    /// The 124/128 degree east service off a dish, which is walked rather than
    /// asked: its carriers say nothing about their neighbours, and everything
    /// a tune needs is in the published table.
    Sky,
    Elsewhere,
}

/// The settings' name for a system, whatever spelling was used: hyphens
/// dropped, case raised, and the daemon's names for Japanese cable and the
/// optical distribution taken to the settings' own. A name this does not know
/// comes back as written, less the hyphens.
pub fn named(system: &str) -> String {
    let plain: String = system
        .trim()
        .chars()
        .filter(|letter| *letter != '-')
        .flat_map(char::to_uppercase)
        .collect();
    match plain.as_str() {
        "DVBC" | "DVBC/ANNEXA" | "DVBC/ANNEX_A" | "DVBC/ANNEXC" | "DVBC/ANNEX_C" | "ANNEX_C"
        | "ISDBC/ANNEXC" | "J.83C" | "J83C" => ISDBC.to_owned(),
        "DVBC/ANNEXB" | "DVBC/ANNEX_B" | "ANNEX_B" | "J.83B" | "J83B" | "HIKARI" => {
            J83B_5_6.to_owned()
        }
        _ => plain,
    }
}

/// Whether two spellings name the same delivery system.
pub fn same(one: &str, other: &str) -> bool {
    named(one) == named(other)
}

/// What the daemon is told, for a system the settings name. The daemon knows
/// Japanese cable and the optical distribution by the DVB API's names.
pub fn wire(system: &str) -> String {
    match named(system).as_str() {
        ISDBC => WIRE_ISDBC.to_owned(),
        J83B_5_6 => WIRE_J83B_5_6.to_owned(),
        other => other.to_owned(),
    }
}

/// Which family a system belongs to.
pub fn family(system: &str) -> Family {
    let system = named(system);
    if system == ISDBT || system.starts_with("DVBT") {
        return Family::Terrestrial;
    }
    if system == ISDBS || system == ISDBS3 {
        return Family::Satellite;
    }
    if system == ISDBC {
        return Family::Cable;
    }
    if system == J83B_5_6 {
        return Family::Optical;
    }
    if system == DVBS || system == DVBS2 {
        return Family::Sky;
    }
    Family::Elsewhere
}

/// What a scan's account and a tuner's state write a system as, the way the
/// system's own standard spells it: `ISDB-T`, `ISDB-S3`, `J.83B (5.6 MBaud)`,
/// `DVB-S2`. The settings' name where it is none of these.
pub fn written(system: &str) -> String {
    let system = named(system);
    let spelled = match system.as_str() {
        ISDBT => "ISDB-T",
        ISDBS => "ISDB-S",
        ISDBS3 => "ISDB-S3",
        ISDBC => "ISDB-C",
        J83B_5_6 => "J.83B (5.6 MBaud)",
        DVBS => "DVB-S",
        DVBS2 => "DVB-S2",
        _ => return system,
    };
    spelled.to_owned()
}

/// What to show a viewer for a system: the name it is offered under, or the
/// settings' name where it is not one of those.
pub fn shown(system: &str) -> String {
    let system = named(system);
    OFFERED
        .iter()
        .find(|(name, _)| *name == system)
        .map_or(system.clone(), |(_, shown)| (*shown).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_system_is_written_as_its_standard_spells_it() {
        assert_eq!(written("isdbt"), "ISDB-T");
        assert_eq!(written("ISDBS3"), "ISDB-S3");
        assert_eq!(written("DVBC/ANNEX_A"), "ISDB-C");
        assert_eq!(written("DVBC/ANNEX_B"), "J.83B (5.6 MBaud)");
        assert_eq!(written("DVB-S2"), "DVB-S2");
        assert_eq!(written("ATSC"), "ATSC");
    }

    #[test]
    fn every_spelling_of_the_cable_is_the_cable() {
        for spelling in [
            "DVBC/ANNEX_A",
            "DVB-C/ANNEX_A",
            "DVBC/ANNEX_C",
            "isdb-c",
            "ISDBC",
            "J83C",
        ] {
            assert_eq!(named(spelling), ISDBC, "{spelling}");
        }
        assert_eq!(named("DVBC/ANNEX_B"), J83B_5_6);
        assert_eq!(named("J.83B"), J83B_5_6);
        assert_eq!(named("isdb-t"), ISDBT);
        assert_eq!(named("ISDB-S3"), ISDBS3);
    }

    #[test]
    fn a_name_this_does_not_know_is_kept() {
        assert_eq!(named("DVBT2"), "DVBT2");
        assert_eq!(named("dvb-c2"), "DVBC2");
    }

    #[test]
    fn the_daemon_is_told_the_names_it_knows() {
        assert_eq!(wire("ISDBC"), "DVBC/ANNEX_A");
        assert_eq!(wire("DVBC/ANNEX_A"), "DVBC/ANNEX_A");
        assert_eq!(wire("J83B_5_6"), "DVBC/ANNEX_B");
        assert_eq!(wire("ISDBT"), "ISDBT");
        assert_eq!(wire("isdb-s"), "ISDBS");
    }

    #[test]
    fn spellings_compare_as_what_they_name() {
        assert!(same("ISDBC", "DVB-C/ANNEX_A"));
        assert!(same("isdbs", "ISDB-S"));
        assert!(!same("ISDBS", "ISDBS3"));
        assert!(!same("ISDBC", "J83B_5_6"));
    }

    #[test]
    fn families() {
        assert_eq!(family("ISDBT"), Family::Terrestrial);
        assert_eq!(family("DVBT2"), Family::Terrestrial);
        assert_eq!(family("ISDBS"), Family::Satellite);
        assert_eq!(family("ISDBS3"), Family::Satellite);
        assert_eq!(family("DVBC/ANNEX_A"), Family::Cable);
        assert_eq!(family("ISDBC"), Family::Cable);
        assert_eq!(family("DVBC/ANNEX_B"), Family::Optical);
        assert_eq!(family("DVBS2"), Family::Sky);
        assert_eq!(family("ATSC"), Family::Elsewhere);
    }

    #[test]
    fn what_is_shown() {
        assert_eq!(shown("DVBC/ANNEX_A"), "ケーブルテレビ");
        assert_eq!(shown("J83B_5_6"), "124,128°CS 光");
        assert_eq!(shown("DVBT2"), "DVBT2");
    }
}

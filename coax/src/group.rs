// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Which platform a stream belongs to, as a viewer would name it.
//!
//! A tuning space is a platform: BS, CS, 地デジ. The signal says which one a
//! stream is on -- the network identifier is handed out by platform -- so
//! nothing has to be configured for a stream to end up in the right space. Two
//! networks share the 110 degree east CS band, and one lock finds both.

use crate::delivery::{self, Family};

/// The platforms this tells apart. The names are the ones a viewer sees and
/// the ones the settings file writes.
pub const GROUPS: [&str; 8] = ["BS", "CS", "BS4K", "CS4K", "SKY", "GR", "CATV", "OTHER"];

/// The platform a network identifier belongs to, or nothing where it says
/// nothing.
pub fn of_network(original_network_id: u16) -> Option<&'static str> {
    match original_network_id {
        4 => Some("BS"),
        6 | 7 => Some("CS"),
        0x0001 | 0x0003 | 0x000a => Some("SKY"),
        0x000b => Some("BS4K"),
        0x000c => Some("CS4K"),
        // A cable operator's own carriage, under an identifier from the block
        // they name themselves from.
        0xfff7..=0xffff => Some("CATV"),
        0x7880..=0x7fe8 => Some("GR"),
        0 => None,
        _ => Some("OTHER"),
    }
}

/// The same, for a stream that reached the receiver by transmodulation.
///
/// A cable carries the other platforms by taking their streams and modulating
/// them again: the stream is the platform's, the modulation is the operator's.
/// So what a stream declares is still what it is, and the only ones that are
/// the operator's own are the ones no platform claims -- which arrive under an
/// identifier outside the block a cable operator names themselves from.
///
/// This deliberately leaves the 124/128 degree east platform alone: that
/// platform transmodulated is still that platform.
pub fn of_network_transmodulated(original_network_id: u16) -> Option<&'static str> {
    match of_network(original_network_id) {
        Some("OTHER") => Some("CATV"),
        other => other,
    }
}

/// The platform whose channels a delivery system's plan is: the terrestrial
/// plan is 地デジ, the cable plan the operator's own carriage, the 124/128 degree
/// east table that service. Nothing for the 110 degree east satellites, whose
/// plan is transponders rather than channels, each holding several platforms'
/// streams.
pub fn of_plan(system: &str) -> Option<&'static str> {
    match delivery::family(system) {
        Family::Terrestrial => Some("GR"),
        Family::Optical | Family::Sky => Some("SKY"),
        Family::Cable => Some("CATV"),
        Family::Satellite | Family::Elsewhere => None,
    }
}

/// Whether a name a viewer gave a space names a platform. The names a viewer
/// writes are not always the ones written down here: the 124/128 degree east
/// platform is often named after its delivery system rather than the orbit,
/// and a file that spells it that way should not need a `group=` to say what
/// it plainly says.
pub fn named(name: &str) -> Option<&'static str> {
    let name = name.trim().to_ascii_uppercase();
    if let Some(group) = GROUPS.iter().copied().find(|group| *group == name) {
        return Some(group);
    }
    ALSO_CALLED
        .iter()
        .find(|(spelling, _)| *spelling == name)
        .map(|(_, group)| *group)
}

/// The other spellings of a platform, and which of GROUPS each one is. One
/// platform, whatever it is called: the 124/128 degree east platform reaches a
/// receiver off a dish and, transmodulated, down an optical distribution, and
/// both declare identifiers of the same platform.
const ALSO_CALLED: [(&str, &str); 2] = [("SKP", "SKY"), ("地デジ", "GR")];

#[cfg(test)]
mod tests {
    /// The 124/128 degree east platform is called SKP as often as SKY, and a
    /// space named either way is that platform: 0x000A comes off a dish and off
    /// an optical distribution alike.
    #[test]
    fn the_sky_platform_is_known_by_the_name_people_give_it() {
        assert_eq!(super::named("SKP"), Some("SKY"));
        assert_eq!(super::named("skp"), Some("SKY"));
        assert_eq!(super::named("SKY"), Some("SKY"));
        assert_eq!(super::of_network(0x000a), Some("SKY"));
        // Not everything is a platform.
        assert_eq!(super::named("光"), None);
    }

    use super::{named, of_network};

    /// The two networks of the 110 degree east CS band are one platform to a
    /// viewer, and are found by one lock, so they have to be told apart from
    /// BS afterwards rather than by where they were found.
    #[test]
    fn a_network_says_which_platform_it_is() {
        assert_eq!(of_network(4), Some("BS"));
        assert_eq!(of_network(6), Some("CS"));
        assert_eq!(of_network(7), Some("CS"));
        assert_eq!(of_network(0x000b), Some("BS4K"));
        assert_eq!(of_network(0xfffe), Some("CATV"));
        assert_eq!(of_network(0x7fe0), Some("GR"));
        assert_eq!(of_network(0), None);
        assert_eq!(of_network(0x1234), Some("OTHER"));
    }

    /// What a cable carries under an identifier nobody recognises is that
    /// operator's own. What it carries under a platform's identifier is that
    /// platform, whatever it was modulated as to get there.
    #[test]
    fn a_transmodulated_stream_nobody_recognises_is_the_operators_own() {
        use super::of_network_transmodulated as transmodulated;
        assert_eq!(transmodulated(0x1234), Some("CATV"));
        assert_eq!(transmodulated(4), Some("BS"));
        assert_eq!(transmodulated(0x000b), Some("BS4K"));
        // The 124/128 degree east platform stays itself.
        assert_eq!(transmodulated(0x000a), Some("SKY"));
        assert_eq!(transmodulated(0), None);
    }

    #[test]
    fn a_space_named_after_a_platform_is_recognised() {
        assert_eq!(named("BS"), Some("BS"));
        assert_eq!(named(" cs "), Some("CS"));
        // Terrestrial is named in Japanese more often than by its letters.
        assert_eq!(named("地デジ"), Some("GR"));
        assert_eq!(named("地デジ(CATV)"), None);
    }
}

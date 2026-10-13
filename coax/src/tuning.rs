// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! What a tune is told: the words that ask the daemon for a carrier of the
//! plan, and what a tuner's own settings add to them.
//!
//! The words are names and values in the daemon's vocabulary --
//! `DELIVERY_SYSTEM`, `FREQUENCY`, `SYMBOL_RATE` and the rest -- in the order
//! they are said. A carrier of the plan is a frequency and a name; everything
//! else a tune needs is a fact about the system it is in, and is here.

use crate::delivery::{self, Family};
use crate::plan::{self, Carrier, SkyChannel, Transponder};

fn word(name: &str, value: impl ToString) -> (String, String) {
    (name.to_owned(), value.to_string())
}

/// The name a tune of a system is asked under. Japanese cable keeps its own
/// name, as `coax scan` asks for it: the daemon takes it for J.83 Annex A
/// itself, and a channel file written from a scan then says what the band is.
/// The optical distribution has no name the daemon knows but its annex's.
pub fn told(system: &str) -> String {
    match delivery::family(system) {
        Family::Optical => delivery::wire(system),
        _ => delivery::named(system),
    }
}

/// The words for a carrier of the terrestrial, cable or optical plan, in any
/// spelling of its system. A carrier is in hertz.
///
/// The cable's figures have to be said: a tuner will not find them. A system
/// whose carriers are not of these plans is told only where to look.
pub fn carrier_words(system: &str, carrier: &Carrier) -> Vec<(String, String)> {
    let mut words = vec![
        word("DELIVERY_SYSTEM", told(system)),
        word("FREQUENCY", carrier.frequency_hz),
    ];
    match delivery::family(system) {
        Family::Terrestrial => {
            words.push(word("BANDWIDTH_HZ", plan::TERRESTRIAL_BANDWIDTH_HZ));
        }
        Family::Cable => {
            words.push(word("SYMBOL_RATE", plan::CABLE_SYMBOL_RATE));
            words.push(word("MODULATION", plan::CABLE_MODULATION));
        }
        Family::Optical => {
            words.push(word("SYMBOL_RATE", plan::OPTICAL_SYMBOL_RATE));
            words.push(word("MODULATION", plan::OPTICAL_MODULATION));
        }
        Family::Satellite | Family::Sky | Family::Elsewhere => {}
    }
    words
}

/// The words for a transponder of the 110 degree east plan, as ISDB-S or as
/// ISDB-S3. A transponder is in kilohertz, as an intermediate frequency. Which
/// stream on it is the network's to say, so no `STREAM_ID` is told here.
pub fn transponder_words(s3: bool, transponder: &Transponder) -> Vec<(String, String)> {
    let system = if s3 {
        delivery::ISDBS3
    } else {
        delivery::ISDBS
    };
    vec![
        word("DELIVERY_SYSTEM", system),
        word("FREQUENCY", transponder.frequency_khz),
    ]
}

/// The words for a carrier of the 124/128 degree east plan. Everything a tune
/// needs is in the published table: the downlink, the block on the dish --
/// which is also what puts the tone on the lead that tells the two satellites
/// apart -- the polarisation, and the modulation and rate of the generation it
/// is in.
pub fn sky_words(channel: &SkyChannel) -> Vec<(String, String)> {
    vec![
        word("DELIVERY_SYSTEM", channel.system),
        word("FREQUENCY", channel.downlink_khz),
        word("LNB", channel.satellite),
        word("POLARIZATION", channel.polarization()),
        word("SYMBOL_RATE", channel.symbol_rate),
        word("MODULATION", channel.modulation),
    ]
}

/// What a person set for one tuner, added to every tune of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TunerSetting {
    /// The systems to use it for. For a tuner that names its systems, the ones
    /// of those to use; for one that names none (BDA), the ones a person says
    /// it receives (see [`systems_used`]). Empty is every one it names.
    pub systems: Vec<String>,
    /// The LNB supply of a satellite tune: `off`, `low` or `high`. Empty says
    /// nothing, which is no supply.
    pub lnb: String,
    /// The front end's gain: `auto`, `min`, `max`, `+13dB`. Empty says nothing.
    pub gain: String,
}

impl TunerSetting {
    /// Whether a system is one to use the tuner for.
    pub fn allows(&self, system: &str) -> bool {
        self.systems.is_empty()
            || self
                .systems
                .iter()
                .any(|allowed| delivery::same(allowed, system))
    }

    /// What it adds to a tune of a system: `lnb_power` on a satellite, and
    /// `gain`.
    pub fn words(&self, system: &str) -> Vec<(&'static str, String)> {
        let mut words = Vec::new();
        let lnb = self.lnb.trim();
        if !lnb.is_empty() && is_satellite(system) {
            words.push(("lnb_power", lnb.to_owned()));
        }
        let gain = self.gain.trim();
        if !gain.is_empty() {
            words.push(("gain", gain.to_owned()));
        }
        words
    }
}

fn is_satellite(system: &str) -> bool {
    let named = system.to_ascii_uppercase();
    named.starts_with("ISDBS") || named.starts_with("DVBS")
}

/// Whether a person may add a system to what a tuner names: only ISDB-S3, to a
/// tuner that names ISDB-S and not ISDB-S3. A DVB driver names ISDB-S for a
/// tuner that receives both.
pub fn addable(told: &[String], system: &str) -> bool {
    delivery::same(system, delivery::ISDBS3)
        && told.iter().any(|one| delivery::same(one, delivery::ISDBS))
        && !told.iter().any(|one| delivery::same(one, delivery::ISDBS3))
}

/// The systems a tuner is used for. For one that names its systems, those the
/// setting allows and ISDB-S3 where a person added it; for one that names
/// none, what a person said.
pub fn systems_used(told: &[String], setting: &TunerSetting) -> Vec<String> {
    if told.is_empty() {
        return setting.systems.clone();
    }
    let mut used: Vec<String> = told
        .iter()
        .filter(|system| setting.allows(system))
        .cloned()
        .collect();
    used.extend(
        setting
            .systems
            .iter()
            .filter(|system| addable(told, system))
            .cloned(),
    );
    used
}

/// The symbol rate of ISDB-S3 (ARIB STD-B44). A tuner asked for ISDB-S3 under
/// ISDB-S's name tells the two apart by it.
pub const S3_SYMBOL_RATE: &str = "33756000";

/// Whether a tuner is asked for ISDB-S3 under ISDB-S's name: it is used for
/// ISDB-S3 without naming it itself, so the daemon would refuse the name.
pub fn s3_as_s(told: &[String], used: &[String]) -> bool {
    !told.iter().any(|one| delivery::same(one, delivery::ISDBS3))
        && used.iter().any(|one| delivery::same(one, delivery::ISDBS3))
}

/// Turns an ISDB-S3 tune into one such a tuner takes: ISDB-S, at ISDB-S3's
/// symbol rate. The system is changed in place; the rate is given back for the
/// caller to add, unless the tune already said one.
pub fn ask_s3_as_s<K: AsRef<str>>(words: &mut [(K, String)]) -> Option<(&'static str, String)> {
    for (name, value) in words.iter_mut() {
        if name.as_ref() == "DELIVERY_SYSTEM" {
            *value = delivery::ISDBS.to_owned();
        }
    }
    (!words.iter().any(|(name, _)| name.as_ref() == "SYMBOL_RATE"))
        .then(|| ("SYMBOL_RATE", S3_SYMBOL_RATE.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said<'a>(words: &'a [(String, String)], name: &str) -> Option<&'a str> {
        words
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn a_terrestrial_tune_says_how_wide_the_channel_is() {
        let carrier = &plan::terrestrial(&[plan::Band::Uhf])[0];
        let words = carrier_words("ISDB-T", carrier);
        assert_eq!(said(&words, "DELIVERY_SYSTEM"), Some("ISDBT"));
        assert_eq!(said(&words, "FREQUENCY"), Some("473142857"));
        assert_eq!(said(&words, "BANDWIDTH_HZ"), Some("6000000"));
    }

    /// The cable is asked for under its own name, at the cable's one rate.
    #[test]
    fn a_cable_tune_says_the_cables_figures() {
        let carrier = &plan::cable(&[plan::Band::Uhf])[0];
        for spelling in ["ISDBC", "DVBC/ANNEX_A"] {
            let words = carrier_words(spelling, carrier);
            assert_eq!(said(&words, "DELIVERY_SYSTEM"), Some("ISDBC"));
            assert_eq!(said(&words, "FREQUENCY"), Some("473000000"));
            assert_eq!(said(&words, "SYMBOL_RATE"), Some("5274000"));
            assert_eq!(said(&words, "MODULATION"), Some("QAM/AUTO"));
        }
    }

    #[test]
    fn an_optical_tune_is_asked_for_by_its_annex() {
        let carrier = &plan::optical()[0];
        let words = carrier_words("J83B_5_6", carrier);
        assert_eq!(said(&words, "DELIVERY_SYSTEM"), Some("DVBC/ANNEX_B"));
        assert_eq!(said(&words, "FREQUENCY"), Some("73750000"));
        assert_eq!(said(&words, "SYMBOL_RATE"), Some("5600000"));
        assert_eq!(said(&words, "MODULATION"), Some("QAM/256"));
    }

    #[test]
    fn a_transponder_is_asked_for_in_kilohertz() {
        let transponder = &plan::satellite()[0];
        let words = transponder_words(false, transponder);
        assert_eq!(said(&words, "DELIVERY_SYSTEM"), Some("ISDBS"));
        assert_eq!(said(&words, "FREQUENCY"), Some("1049480"));
        let words = transponder_words(true, transponder);
        assert_eq!(said(&words, "DELIVERY_SYSTEM"), Some("ISDBS3"));
    }

    /// The daemon knows the polarisation as `POLARIZATION`, and the block on
    /// the dish has to be named for the tone and the oscillator.
    #[test]
    fn a_sky_tune_names_the_block_and_the_polarisation() {
        let channels = plan::sky();
        let words = sky_words(&channels[0]);
        assert_eq!(
            words,
            [
                word("DELIVERY_SYSTEM", "DVBS2"),
                word("FREQUENCY", "12508000"),
                word("LNB", "JCSAT3A"),
                word("POLARIZATION", "VERTICAL"),
                word("SYMBOL_RATE", "23303000"),
                word("MODULATION", "PSK/8"),
            ]
        );
        let older = sky_words(&channels[1]);
        assert_eq!(said(&older, "DELIVERY_SYSTEM"), Some("DVBS"));
        assert_eq!(said(&older, "MODULATION"), Some("QPSK"));
        let second = channels
            .iter()
            .find(|one| one.name == "JCSAT4B/JD2")
            .expect("JD2 is in the plan");
        let words = sky_words(second);
        assert_eq!(said(&words, "LNB"), Some("JCSAT4B"));
        assert_eq!(said(&words, "POLARIZATION"), Some("HORIZONTAL"));
    }

    #[test]
    fn a_tuners_setting_adds_the_supply_only_to_a_satellite_tune() {
        let setting = TunerSetting {
            lnb: String::from("low"),
            gain: String::from("+13dB"),
            ..TunerSetting::default()
        };
        assert_eq!(
            setting.words("ISDBS"),
            [
                ("lnb_power", String::from("low")),
                ("gain", String::from("+13dB"))
            ]
        );
        assert_eq!(setting.words("ISDBT"), [("gain", String::from("+13dB"))]);
        assert!(TunerSetting::default().words("DVBS2").is_empty());
    }

    /// A tuner that names ISDB-S may be given ISDB-S3 by a person, and is then
    /// asked for it under ISDB-S's name.
    #[test]
    fn isdb_s3_is_added_only_to_a_tuner_that_names_isdb_s() {
        let names =
            |list: &[&str]| -> Vec<String> { list.iter().map(|one| (*one).to_owned()).collect() };
        let both = TunerSetting {
            systems: names(&["ISDB-S", "ISDB-S3"]),
            ..TunerSetting::default()
        };
        let dvb = names(&["ISDB-T", "ISDB-S"]);
        let used = systems_used(&dvb, &both);
        assert_eq!(used, names(&["ISDB-S", "ISDB-S3"]));
        assert!(!both.allows("ISDBT"));
        assert!(s3_as_s(&dvb, &used));
        // One that names nothing receives what a person said.
        assert_eq!(systems_used(&[], &both), both.systems);
        assert!(s3_as_s(&[], &both.systems));
        // One that names ISDB-S3 is asked for it by its name.
        let native = names(&["ISDB-S", "ISDB-S3"]);
        assert!(!s3_as_s(&native, &systems_used(&native, &both)));
        assert!(!addable(&names(&["ISDB-T"]), "ISDB-S3"));
        assert!(!addable(&dvb, "ISDB-T"));
    }

    #[test]
    fn an_isdb_s3_tune_is_turned_into_isdb_s_at_its_rate() {
        let mut asking = vec![
            ("DELIVERY_SYSTEM", String::from("ISDBS3")),
            ("FREQUENCY", String::from("2241660")),
        ];
        let rate = ask_s3_as_s(&mut asking);
        assert_eq!(asking[0], ("DELIVERY_SYSTEM", String::from("ISDBS")));
        assert_eq!(rate, Some(("SYMBOL_RATE", String::from(S3_SYMBOL_RATE))));
        let mut said = vec![
            (String::from("DELIVERY_SYSTEM"), String::from("ISDBS3")),
            (String::from("SYMBOL_RATE"), String::from("1")),
        ];
        assert_eq!(ask_s3_as_s(&mut said), None, "a rate already said is kept");
        assert_eq!(said[0].1, "ISDBS");
    }
}

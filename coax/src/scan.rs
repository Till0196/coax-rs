// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Which carriers a scan tries ([`targets`]): one line of the settings, as it
//! was written, made into a list of tunes in the order they are tried, and
//! narrowed by channel number.
//!
//! Each target has a name a person reads in the scan's account -- `ISDB-T
//! 13ch`, `ISDB-C C13ch`, `J.83B (5.6 MBaud) 73.75MHz`, `ISDB-S BS01`, `ISDB-S3
//! BS01`, `JCSAT3A/JD1` -- and the words to ask the daemon with (see
//! [`crate::tuning`]).

use std::fmt;

use crate::plan::{self, Band, Transponder};
use crate::tuning;

/// How one line is scanned, as the settings write it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Spec {
    /// The delivery system the line receives, in the settings' spelling:
    /// `isdbt`, `isdbs`, `isdbs3`, `isdbc`, `j83b_5_6` or `dvbs2`.
    pub plan: String,
    /// For terrestrial and cable, `uhf`, `catv`, `exuhf` and `vhf`; for the
    /// satellites, `bs` and `cs` (right-hand) and `bs-left` and `cs-left`
    /// (left-hand, ISDB-S3 only). Empty is the plan's default.
    pub bands: Vec<String>,
    /// The channel numbers to try (`only = [23, 24]`). Empty is all of them.
    pub only: Vec<u32>,
    /// For `sky`, the satellites to try (`JCSAT3A`). Empty is both.
    pub satellites: Vec<String>,
    /// For `sky`, the generations to try, by system or by modulation (`DVBS2`,
    /// `PSK/8`). Empty is both.
    pub modulations: Vec<String>,
}

/// One carrier to try. `words` are what the daemon is asked with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    /// Only the 4K satellites are TLV; everything else is a transport stream.
    pub tlv: bool,
    pub words: Vec<(String, String)>,
}

impl Target {
    /// The words with the names borrowed, as a tune takes them.
    pub fn asked(&self) -> Vec<(&str, String)> {
        self.words
            .iter()
            .map(|(name, value)| (name.as_str(), value.clone()))
            .collect()
    }
}

/// Why a line cannot be scanned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A plan this does not know.
    Plan(String),
    /// A band this does not know.
    Band(String),
    /// A satellite band the system is not carried on: `s3` is whether the
    /// line was ISDB-S3.
    NotCarried { band: String, s3: bool },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(plan) => write!(f, "plan「{plan}」は不明です"),
            Self::Band(band) => write!(f, "bands「{band}」は不明です"),
            Self::NotCarried { band, s3: true } => write!(
                f,
                "ISDB-S3 は 110°CS の右旋をスキャンできません（bands の「{band}」）"
            ),
            Self::NotCarried { band, s3: false } => {
                write!(f, "ISDB-S は左旋をスキャンできません（bands の「{band}」）")
            }
        }
    }
}

impl std::error::Error for Error {}

/// The targets of a line, in the order they are tried. A plan this does not
/// know is an error: an empty list would read as "nothing was found".
pub fn targets(spec: &Spec) -> Result<Vec<Target>, Error> {
    let targets = match spec.plan.as_str() {
        "isdbt" => plan::terrestrial(&bands_of(&spec.bands, &spec.plan)?)
            .into_iter()
            .map(|carrier| Target {
                name: format!("ISDB-T {}ch", carrier.name),
                tlv: false,
                words: tuning::carrier_words(crate::delivery::ISDBT, &carrier),
            })
            .collect(),
        // A cable is not found without its figures; the words carry them.
        "isdbc" => plan::cable(&bands_of(&spec.bands, &spec.plan)?)
            .into_iter()
            .map(|carrier| Target {
                name: format!("ISDB-C {}ch", carrier.name),
                tlv: false,
                words: tuning::carrier_words(crate::delivery::ISDBC, &carrier),
            })
            .collect(),
        // BS and the 110 degree east CS are one plan, and the network's own
        // table says what is on which transponder.
        "isdbs" => satellite_transponders(&spec.bands, false)?
            .into_iter()
            .map(|transponder| Target {
                words: tuning::transponder_words(false, &transponder),
                name: format!("ISDB-S {}", transponder.name),
                tlv: false,
            })
            .collect(),
        // BS4K and 8K, and CS4K, carried in TLV.
        "isdbs3" => satellite_transponders(&spec.bands, true)?
            .into_iter()
            .map(|transponder| Target {
                words: tuning::transponder_words(true, &transponder),
                name: format!("ISDB-S3 {}", transponder.name),
                tlv: true,
            })
            .collect(),
        // The optical distribution has no channel numbers of its own, so a
        // carrier is called by its annex and its frequency.
        "j83b_5_6" => plan::optical()
            .into_iter()
            .map(|carrier| Target {
                name: format!("J.83B (5.6 MBaud) {}MHz", carrier.name),
                tlv: false,
                words: tuning::carrier_words(crate::delivery::J83B_5_6, &carrier),
            })
            .collect(),
        "dvbs2" => plan::sky()
            .into_iter()
            .filter(|channel| named_in(&spec.satellites, channel.satellite))
            .filter(|channel| {
                named_in(&spec.modulations, channel.modulation)
                    || named_in(&spec.modulations, channel.system)
            })
            .map(|channel| Target {
                words: tuning::sky_words(&channel),
                name: channel.name,
                tlv: false,
            })
            .collect(),
        other => return Err(Error::Plan(other.to_owned())),
    };
    Ok(only(targets, &spec.only))
}

/// Whether a list of names says something, in any case. Empty says all.
fn named_in(names: &[String], one: &str) -> bool {
    names.is_empty() || names.iter().any(|named| named.eq_ignore_ascii_case(one))
}

/// The terrestrial or cable bands a line names. Empty is the plan's default:
/// UHF and the cable's bands for terrestrial, everything for a cable.
pub fn bands_of(named: &[String], plan: &str) -> Result<Vec<Band>, Error> {
    if named.is_empty() {
        return Ok(match plan {
            "isdbc" => plan::CABLE_BANDS.to_vec(),
            _ => plan::DEFAULT_BANDS.to_vec(),
        });
    }
    named
        .iter()
        .map(|name| Band::parse(name).ok_or_else(|| Error::Band(name.clone())))
        .collect()
}

/// The transponders of the 110 degree east plan in the bands a line names
/// (`bs`, `bs-left`, `cs`, `cs-left`), for ISDB-S or ISDB-S3. Empty is every
/// band the system is carried on. A band it is not carried on is refused
/// rather than walked: ISDB-S has nothing left-hand, and the 4K services on CS
/// were left-hand only (see [`plan::carries`]).
pub fn satellite_transponders(named: &[String], s3: bool) -> Result<Vec<Transponder>, Error> {
    let wanted: Vec<(bool, bool)> = if named.is_empty() {
        plan::SATELLITE_QUARTERS
            .iter()
            .filter(|(_, bs, right)| plan::carries(s3, *bs, *right))
            .map(|(_, bs, right)| (*bs, *right))
            .collect()
    } else {
        named
            .iter()
            .map(|name| {
                let lower = name.trim().to_ascii_lowercase();
                match plan::SATELLITE_QUARTERS
                    .iter()
                    .find(|(spelling, _, _)| *spelling == lower)
                {
                    Some((_, bs, right)) if plan::carries(s3, *bs, *right) => Ok((*bs, *right)),
                    Some(_) => Err(Error::NotCarried {
                        band: name.clone(),
                        s3,
                    }),
                    None => Err(Error::Band(name.clone())),
                }
            })
            .collect::<Result<_, _>>()?
    };
    Ok(plan::satellite()
        .into_iter()
        .filter(|transponder| wanted.contains(&(transponder.is_bs(), transponder.right_hand)))
        .collect())
}

/// Narrows targets to the channel numbers asked for, read from the end of
/// their names (see [`number_in`]): the number a person sees is the one the
/// settings write.
pub fn only(targets: Vec<Target>, wanted: &[u32]) -> Vec<Target> {
    if wanted.is_empty() {
        return targets;
    }
    targets
        .into_iter()
        .filter(|target| number_in(&target.name).is_some_and(|number| wanted.contains(&number)))
        .collect()
}

/// The number at the head of a name's last word: 23 in `ISDB-T 23ch`, 13 in
/// `ISDB-C C13ch`, 73 in `J.83B (5.6 MBaud) 73.75MHz`. The digits in the system's own name
/// (`J.83B`) are not it.
pub fn number_in(name: &str) -> Option<u32> {
    let digits: String = name
        .rsplit(' ')
        .next()?
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::targets as targets_of;

    fn walk(plan: &str, bands: &[&str]) -> Result<Vec<Target>, Error> {
        targets(&Spec {
            plan: plan.to_owned(),
            bands: bands.iter().map(|one| (*one).to_owned()).collect(),
            ..Spec::default()
        })
    }

    fn frequency(target: &Target) -> u32 {
        target
            .words
            .iter()
            .find(|(name, _)| name == "FREQUENCY")
            .expect("a frequency")
            .1
            .parse()
            .expect("a number")
    }

    #[test]
    fn a_terrestrial_line_walks_uhf_first() {
        let targets = walk("isdbt", &["uhf"]).expect("walks");
        assert_eq!(targets.len(), 40, "13 to 52");
        assert_eq!(targets[0].name, "ISDB-T 13ch");
        assert_eq!(
            targets[0].words[0],
            (String::from("DELIVERY_SYSTEM"), String::from("ISDBT"))
        );
    }

    #[test]
    fn only_narrows_the_walk_by_channel_number() {
        let targets = targets(&Spec {
            plan: String::from("isdbt"),
            bands: vec![String::from("uhf")],
            only: vec![23, 24],
            ..Spec::default()
        })
        .expect("walks");
        assert_eq!(targets.len(), 2);
        assert_eq!(number_in(&targets[0].name), Some(23));
        assert_eq!(targets[0].name, "ISDB-T 23ch");
    }

    /// A cable channel is named with the cable's prefix, and narrowed by the
    /// number after it.
    #[test]
    fn a_cable_channel_is_named_as_the_cable_numbers_it() {
        let targets = targets(&Spec {
            plan: String::from("isdbc"),
            bands: vec![String::from("catv")],
            only: vec![13],
            ..Spec::default()
        })
        .expect("walks");
        let names: Vec<&str> = targets.iter().map(|target| target.name.as_str()).collect();
        assert_eq!(names, ["ISDB-C C13ch"]);
    }

    /// The optical distribution is called by its annex and its frequency, and
    /// narrowed by the whole megahertz.
    #[test]
    fn an_optical_carrier_is_named_by_its_frequency() {
        let targets = targets(&Spec {
            plan: String::from("j83b_5_6"),
            only: vec![73, 86],
            ..Spec::default()
        })
        .expect("walks");
        let names: Vec<&str> = targets.iter().map(|target| target.name.as_str()).collect();
        assert_eq!(
            names,
            ["J.83B (5.6 MBaud) 73.75MHz", "J.83B (5.6 MBaud) 86.75MHz"]
        );
    }

    /// A cable's own broadcasts sit a seventh of a megahertz below the air's.
    #[test]
    fn a_cable_carrier_sits_a_seventh_of_a_megahertz_lower() {
        let line = |plan: &str| {
            targets(&Spec {
                plan: plan.to_owned(),
                bands: vec![String::from("uhf")],
                only: vec![23],
                ..Spec::default()
            })
            .expect("walks")
        };
        assert_eq!(
            frequency(&line("isdbt")[0]) - frequency(&line("isdbc")[0]),
            1_000_000 / 7
        );
    }

    #[test]
    fn a_satellite_line_walks_the_right_hand_plan() {
        let targets = walk("isdbs", &[]).expect("walks");
        assert_eq!(targets.len(), 24);
        assert!(
            targets[0].name.starts_with("ISDB-S BS"),
            "{}",
            targets[0].name
        );
        // Right-hand BS is the odd numbers, right-hand CS the even.
        assert!(targets.iter().all(|target| {
            let number = number_in(&target.name).expect("a number");
            if target.name.starts_with("ISDB-S BS") {
                number % 2 == 1
            } else {
                number.is_multiple_of(2)
            }
        }));
    }

    #[test]
    fn the_satellite_bands_choose_the_transponders() {
        let bs = walk("isdbs", &["bs"]).expect("walks");
        assert_eq!(bs.len(), 12);
        assert!(bs.iter().all(|target| target.name.starts_with("ISDB-S BS")));
        let cs = walk("isdbs", &["CS"]).expect("walks");
        assert_eq!(cs.len(), 12);
        assert!(cs.iter().all(|target| target.name.starts_with("ISDB-S ND")));
        assert_eq!(
            walk("isdbs", &["bs-left"]),
            Err(Error::NotCarried {
                band: String::from("bs-left"),
                s3: false
            }),
            "nothing is left-hand in ISDB-S"
        );
        let s3 = walk("isdbs3", &[]).expect("walks");
        assert_eq!(s3.len(), 36, "both hands of BS, and the left hand of CS");
        // Right-hand CS is the even numbers (ND02 up), left-hand the odd.
        assert!(
            s3.iter()
                .filter(|target| target.name.starts_with("ISDB-S3 ND"))
                .all(|target| number_in(&target.name).is_some_and(|number| number % 2 == 1)),
            "only left-hand CS"
        );
        assert!(s3.iter().all(|target| target.tlv));
        assert!(
            s3[0]
                .words
                .contains(&(String::from("DELIVERY_SYSTEM"), String::from("ISDBS3")))
        );
        let left = walk("isdbs3", &["bs-left"]).expect("walks");
        assert_eq!(left.len(), 12);
        assert!(left.iter().any(|target| target.name == "ISDB-S3 BS14"));
        assert_eq!(
            walk("isdbs3", &["uhf"]),
            Err(Error::Band(String::from("uhf")))
        );
        assert_eq!(
            walk("isdbs3", &["cs"]),
            Err(Error::NotCarried {
                band: String::from("cs"),
                s3: true
            }),
            "the 4K services on CS were never right-hand"
        );
    }

    /// The sky line is asked with the block and the polarisation, and narrowed
    /// by satellite and generation.
    #[test]
    fn a_sky_line_is_narrowed_by_satellite_and_modulation() {
        let targets = targets(&Spec {
            plan: String::from("dvbs2"),
            satellites: vec![String::from("jcsat4b")],
            modulations: vec![String::from("PSK/8")],
            ..Spec::default()
        })
        .expect("walks");
        assert_eq!(targets.len(), 32);
        assert!(
            targets
                .iter()
                .all(|target| target.name.starts_with("JCSAT4B/"))
        );
        assert!(
            targets[0]
                .words
                .contains(&(String::from("LNB"), String::from("JCSAT4B")))
        );
        assert!(
            targets[0]
                .words
                .contains(&(String::from("POLARIZATION"), String::from("VERTICAL")))
        );
        let older = |named: &str| {
            targets_of(&Spec {
                plan: String::from("dvbs2"),
                modulations: vec![named.to_owned()],
                ..Spec::default()
            })
            .expect("walks")
        };
        assert_eq!(older("DVBS"), older("qpsk"), "by system or by modulation");
        assert_eq!(older("DVBS").len(), 60);
    }

    #[test]
    fn a_plan_that_is_not_known_is_an_error() {
        assert_eq!(walk("dvbt2", &[]), Err(Error::Plan(String::from("dvbt2"))));
        assert_eq!(
            walk("isdbt", &["shf"]),
            Err(Error::Band(String::from("shf")))
        );
    }
}

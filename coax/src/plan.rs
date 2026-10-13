// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The Japanese television channel plan: where a carrier sits when a channel is
//! named rather than measured.
//!
//! It is one plan, not a terrestrial one and a cable one. VHF and UHF go
//! through the air, the mid band and the super high band only over a cable, and
//! a cable carries all of it on the same numbers -- channel 13 over the air and
//! channel 13 on the cable are the same channel, a seventh of a megahertz
//! apart.
//!
//! Every channel is 6 MHz wide and every stretch is packed from its low edge,
//! so a band is a first channel, a first frequency and a count. What looks
//! irregular in one flat list is only the joins between bands.

/// One carrier of the plan: what it is called and where it sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carrier {
    pub name: String,
    pub frequency_hz: u32,
}

/// A stretch of the plan: a run of channels evenly spaced from a first
/// frequency, named with that band's prefix. `C` marks the stretches only a
/// cable has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stretch {
    pub prefix: &'static str,
    pub first: u32,
    pub last: u32,
    pub first_hz: u32,
}

/// How wide a channel is, in every band of the plan.
const CHANNEL_SPACING_HZ: u32 = 6_000_000;

/// A terrestrial carrier sits a seventh of a megahertz above a cable one --
/// 473142857 against 473000000 -- for every channel of the plan. Mixing the two
/// up is a lock that never comes, which reads as a broken tuner.
pub const TERRESTRIAL_OFFSET_HZ: u32 = 142_857;
pub const CABLE_OFFSET_HZ: u32 = 0;

/// How wide a terrestrial channel is, as a tune is told it. Some boards work
/// out a cable's width from its symbol rate, but a terrestrial channel has no
/// rate to work it out from.
pub const TERRESTRIAL_BANDWIDTH_HZ: &str = "6000000";

/// What a cable's own broadcasts take, beyond a frequency. Japanese cable
/// carries one symbol rate across a network, and the modulation is left to the
/// demodulator: a network runs 64QAM and 256QAM side by side, and naming one is
/// a lock lost on the other.
pub const CABLE_SYMBOL_RATE: &str = "5274000";
pub const CABLE_MODULATION: &str = "QAM/AUTO";

/// UHF, which is the whole of Japanese terrestrial television.
///
/// It stops at 52. Channels 53 to 62 were terrestrial television until the band
/// above 710 MHz was given to mobile telephony; over the air they are ten
/// carriers of certain silence, so they are a band of their own -- see EX_UHF
/// -- walked by a cable receiver.
pub const UHF: Stretch = Stretch {
    prefix: "",
    first: 13,
    last: 52,
    first_hz: 473_000_000,
};

/// The channels the broadcast plan gave back: 53 to 62. Not over the air. On a
/// cable they are ordinary channels, so a carrier that answers here answers
/// from a cable.
pub const EX_UHF: Stretch = Stretch {
    prefix: "",
    first: 53,
    last: 62,
    first_hz: 713_000_000,
};

/// The mid band and the super high band, which only a cable has, on the
/// numbering a cable gives them. The mid band runs 108 to 170 MHz, two
/// megahertz more than its ten channels need: C13 to C21 are packed from the
/// bottom and C22 put against the top edge, so the spare falls between them.
pub const CATV: [Stretch; 3] = [
    Stretch {
        prefix: "C",
        first: 13,
        last: 21,
        first_hz: 111_000_000,
    },
    Stretch {
        prefix: "C",
        first: 22,
        last: 22,
        first_hz: 167_000_000,
    },
    Stretch {
        prefix: "C",
        first: 23,
        last: 63,
        first_hz: 225_000_000,
    },
];

/// The VHF channels, which have carried no terrestrial television since the
/// analogue switch-off and are here only because a cable may put something on
/// them. VHF high is two stretches that overlap by two megahertz, which is the
/// terrestrial allocation's own doing and why 7 and 8 are four megahertz apart
/// rather than six.
pub const VHF: [Stretch; 3] = [
    Stretch {
        prefix: "",
        first: 1,
        last: 3,
        first_hz: 93_000_000,
    },
    Stretch {
        prefix: "",
        first: 4,
        last: 7,
        first_hz: 173_000_000,
    },
    Stretch {
        prefix: "",
        first: 8,
        last: 12,
        first_hz: 195_000_000,
    },
];

/// Which stretch of the plan a carrier belongs to, told from the name the plan
/// gave it. It is what a viewer picks from when they ask for part of a sweep:
/// the terrestrial plan is a hundred and thirteen carriers and the satellite
/// plan another forty-eight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    /// 1 to 12.
    Vhf,
    /// 13 to 52, which is what is broadcast.
    Uhf,
    /// 53 to 62, which is not broadcast any more and only a cable carries.
    ExUhf,
    /// C13 to C63.
    Catv,
    /// The 110 degree east broadcasting satellites, BS01 upwards.
    Bs,
    /// The 110 degree east communication satellites, ND02 upwards.
    Cs,
    /// The 124 and 128 degree east transponders, named after the satellite
    /// they are on.
    Sky,
    /// Anything the plan names another way, which is walked whatever was
    /// asked for: a viewer choosing stretches has said nothing about it.
    Elsewhere,
}

/// Reads a carrier's name as one of those.
pub fn range_of(name: &str) -> Range {
    let name = name.trim();
    if let Some(number) = name.strip_prefix('C')
        && !number.is_empty()
        && number.chars().all(|digit| digit.is_ascii_digit())
    {
        return Range::Catv;
    }
    if name.starts_with("BS") {
        return Range::Bs;
    }
    if name.starts_with("ND") {
        return Range::Cs;
    }
    // A transponder of the 124/128 degree east plan is named for its satellite
    // and its series, which nothing else in the plan is.
    if name.contains('/') {
        return Range::Sky;
    }
    match name.parse::<u32>() {
        Ok(1..=12) => Range::Vhf,
        Ok(13..=52) => Range::Uhf,
        Ok(53..=62) => Range::ExUhf,
        _ => Range::Elsewhere,
    }
}

/// A band as the settings file names one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    /// Channels 13 to 52, which is the whole of what is broadcast.
    Uhf,
    /// Channels 53 to 62, which are broadcast nowhere and carried by cables
    /// that have run out of room lower down.
    ExUhf,
    /// C13 to C63, where a cable may carry the same broadcasts moved down.
    Catv,
    /// 1 to 12, which carry nothing over the air any more.
    Vhf,
}

/// What a terrestrial space walks when its settings say nothing. UHF because
/// that is where the broadcasts are, and the cable band because a cable may
/// have moved them; not VHF, which would only add twelve carriers of silence.
pub const DEFAULT_BANDS: [Band; 3] = [Band::Uhf, Band::Catv, Band::ExUhf];

/// What a cable space walks: all of it. A cable numbers its own channels over
/// the whole plan, VHF included, and those numbers are in use.
pub const CABLE_BANDS: [Band; 4] = [Band::Uhf, Band::Catv, Band::ExUhf, Band::Vhf];

impl Band {
    /// The bands in the order the plan walks them, which is the order the
    /// settings write them.
    pub const ALL: [Self; 4] = [Self::Uhf, Self::Catv, Self::ExUhf, Self::Vhf];

    /// The band as the settings file spells it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Uhf => "UHF",
            Self::ExUhf => "EXUHF",
            Self::Catv => "CATV",
            Self::Vhf => "VHF",
        }
    }

    /// Reads a band as the settings file spells it, in whatever case.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_uppercase().as_str() {
            "UHF" => Some(Self::Uhf),
            "EXUHF" => Some(Self::ExUhf),
            "CATV" | "CABLE" => Some(Self::Catv),
            "VHF" => Some(Self::Vhf),
            _ => None,
        }
    }

    fn stretches(self) -> &'static [Stretch] {
        match self {
            Self::Uhf => std::slice::from_ref(&UHF),
            Self::ExUhf => std::slice::from_ref(&EX_UHF),
            Self::Catv => &CATV,
            Self::Vhf => &VHF,
        }
    }
}

impl Stretch {
    /// The carriers of this stretch, at the given offset.
    pub fn carriers(&self, offset_hz: u32) -> Vec<Carrier> {
        (self.first..=self.last)
            .map(|number| Carrier {
                name: format!("{}{}", self.prefix, number),
                frequency_hz: self.first_hz
                    + (number - self.first) * CHANNEL_SPACING_HZ
                    + offset_hz,
            })
            .collect()
    }
}

/// The terrestrial walk over the given bands, always in the same order however
/// they were asked for: UHF, then the cable band, then VHF.
///
/// The order is the point. Terrestrial television in Japan is all in UHF, so
/// putting anything else first means sixty carriers of nothing before the first
/// that could answer. And the bands a viewer is most likely to turn off sit at
/// the end, so turning one off leaves the numbering of everything before it
/// alone.
pub fn terrestrial(bands: &[Band]) -> Vec<Carrier> {
    let mut carriers = Vec::new();
    for band in [Band::Uhf, Band::Catv, Band::ExUhf, Band::Vhf] {
        if !bands.contains(&band) {
            continue;
        }
        for stretch in band.stretches() {
            carriers.extend(stretch.carriers(TERRESTRIAL_OFFSET_HZ));
        }
    }
    carriers
}

/// The same walk for a cable's own broadcasts, which sit a seventh of a
/// megahertz lower.
pub fn cable(bands: &[Band]) -> Vec<Carrier> {
    terrestrial(bands)
        .into_iter()
        .map(|carrier| Carrier {
            frequency_hz: carrier.frequency_hz - TERRESTRIAL_OFFSET_HZ,
            ..carrier
        })
        .collect()
}

/// A run of transponders of the 110 degree east plan, evenly spaced in
/// intermediate frequency. The numbering is the broadcasters': right-hand BS
/// transponders are the odd numbers and left-hand ones the even; right-hand CS
/// the even ND numbers and left-hand CS the odd. The two hands come down from
/// different oscillators, which is the 1173 MHz between a transponder's two
/// bands.
#[derive(Debug, Clone, Copy)]
pub struct SatelliteBand {
    pub prefix: &'static str,
    pub first_number: u32,
    pub first_khz: u32,
    pub spacing_khz: u32,
    pub count: u32,
}

/// The four runs, in the order their physical channel numbers are given out.
/// Left-hand CS is here for the numbering rather than for anything on it: those
/// transmissions ended in 2024, but a feed that still carries something must
/// not be refused.
pub const SATELLITE_BANDS: [SatelliteBand; 4] = [
    SatelliteBand {
        prefix: "BS",
        first_number: 1,
        first_khz: 1_049_480,
        spacing_khz: 38_360,
        count: 12,
    },
    SatelliteBand {
        prefix: "ND",
        first_number: 2,
        first_khz: 1_613_000,
        spacing_khz: 40_000,
        count: 12,
    },
    SatelliteBand {
        prefix: "BS",
        first_number: 2,
        first_khz: 2_241_660,
        spacing_khz: 38_360,
        count: 12,
    },
    SatelliteBand {
        prefix: "ND",
        first_number: 1,
        first_khz: 2_766_000,
        spacing_khz: 40_000,
        count: 12,
    },
];

/// How many of the bands above are right-hand circular, which is the
/// polarisation every installation can receive: left-hand needs a block and a
/// distribution that pass a band the older ones stop short of.
const RIGHT_HAND_BANDS: usize = 2;

/// Where a receiver tunes a downlink frequency of the 110 degree east
/// satellites, or nothing when it lands outside what the block converts.
///
/// The block on every Japanese installation has two oscillators and picks
/// between them by the hand of the circular polarisation, so a downlink alone
/// does not say where it arrives. The range is checked rather than assumed: a
/// downlink outside the stretch a hand lands on gives a plausible-looking
/// number against the other oscillator, and a plausible number names a
/// transponder that is not there.
///
/// Only this plan. Everywhere else the block is named in the channel and the
/// daemon converts; see the `LNB` property.
#[must_use]
pub fn intermediate_frequency_khz(downlink_khz: u32, left_hand: bool) -> Option<u32> {
    /// The two oscillators of a 110 degree east block, in kilohertz.
    const RIGHT_HAND_OSCILLATOR_KHZ: u32 = 10_678_000;
    const LEFT_HAND_OSCILLATOR_KHZ: u32 = 9_505_000;
    /// What each hand lands on once its oscillator is off.
    const RIGHT_HAND_BAND_KHZ: (u32, u32) = (1_032_000, 2_072_000);
    const LEFT_HAND_BAND_KHZ: (u32, u32) = (2_224_000, 3_224_000);

    let (oscillator, band) = if left_hand {
        (LEFT_HAND_OSCILLATOR_KHZ, LEFT_HAND_BAND_KHZ)
    } else {
        (RIGHT_HAND_OSCILLATOR_KHZ, RIGHT_HAND_BAND_KHZ)
    };
    let intermediate = downlink_khz.checked_sub(oscillator)?;
    (intermediate >= band.0 && intermediate <= band.1).then_some(intermediate)
}

/// One transponder of the satellite plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transponder {
    /// What it is called: BS01, ND02.
    pub name: String,
    /// Where it is, as an intermediate frequency in kilohertz. A satellite
    /// tune takes kilohertz where a terrestrial one takes hertz.
    pub frequency_khz: u32,
    pub right_hand: bool,
}

impl Transponder {
    /// Whether it is on the broadcasting satellites rather than the
    /// communication ones.
    pub fn is_bs(&self) -> bool {
        self.name.starts_with("BS")
    }
}

/// The four quarters of the 110 degree east plan as the settings spell them:
/// the spelling, whether it is BS, and whether it is right-hand.
pub const SATELLITE_QUARTERS: [(&str, bool, bool); 4] = [
    ("bs", true, true),
    ("bs-left", true, false),
    ("cs", false, true),
    ("cs-left", false, false),
];

/// Whether a quarter of the plan carries a system. ISDB-S is right-hand only,
/// on BS and CS alike: nothing is sent left-hand in it, and a tuner whose
/// intermediate frequency stops short of the left-hand band only refuses.
/// ISDB-S3 is both hands of BS and the left hand of CS -- the 4K services on
/// the 110 degree east CS were left-hand only.
pub fn carries(s3: bool, bs: bool, right_hand: bool) -> bool {
    if s3 { bs || !right_hand } else { right_hand }
}

/// Whether a network is on the broadcasting satellites: ISDB-S's 4 and the 4K
/// and 8K services' 11. The 110 degree east CS is 6 and 7, and 12 for 4K.
pub fn is_bs(network: u16) -> bool {
    matches!(network, 4 | 11)
}

/// The transponder a stream is on, which its transport stream identifier
/// carries in bits 8 to 4 (ARIB TR-B15). Nothing for a network that is not on
/// the satellites.
pub fn transponder_of(network: u16, transport: u16) -> Option<u16> {
    matches!(network, 4 | 6 | 7 | 11 | 12).then_some((transport >> 4) & 0x1f)
}

/// Where a satellite stream is tuned: the intermediate frequency of its
/// transponder, in kilohertz (see [`transponder_of`]). Nothing for a network
/// that is not on the satellites.
pub fn satellite_frequency(network: u16, transport: u16) -> Option<u32> {
    let transponder = transponder_of(network, transport)?;
    let name = if is_bs(network) {
        format!("BS{transponder:02}")
    } else {
        format!("ND{transponder:02}")
    };
    satellite()
        .into_iter()
        .find(|known| known.name == name)
        .map(|known| known.frequency_khz)
}

/// Every transponder of the 110 degree east plan, in the order the plan numbers
/// them. The plan is only where to look: a satellite carrier holds several
/// transport streams, so a transponder alone is not a channel. Which stream is
/// which is in the network's own table, and that is what discovery reads.
pub fn satellite() -> Vec<Transponder> {
    let mut transponders = Vec::new();
    for (which, band) in SATELLITE_BANDS.iter().enumerate() {
        for index in 0..band.count {
            transponders.push(Transponder {
                name: format!("{}{:02}", band.prefix, band.first_number + index * 2),
                frequency_khz: band.first_khz + index * band.spacing_khz,
                right_hand: which < RIGHT_HAND_BANDS,
            });
        }
    }
    transponders
}

/// The optical distribution the 124/128 degree east service is transmodulated
/// onto: it arrives over a fibre and comes out of the wall on a coaxial lead as
/// J.83 Annex B. A hundred and eight carriers, evenly spaced, on a grid that is
/// neither the cable plan nor the terrestrial one. They have no published
/// channel numbers, so they are named by their frequency in megahertz, to as
/// many places as it takes: `73.75`.
const OPTICAL_FIRST_KHZ: u32 = 73_750;
const OPTICAL_SPACING_KHZ: u32 = 6_500;
const OPTICAL_COUNT: u32 = 108;

/// What a carrier of that distribution takes beyond its frequency. Not the
/// cable's figures: this is a different modulation at a different rate.
pub const OPTICAL_SYMBOL_RATE: &str = "5600000";
pub const OPTICAL_MODULATION: &str = "QAM/256";

/// Every carrier of the optical distribution, in the order they sit in the
/// spectrum.
pub fn optical() -> Vec<Carrier> {
    (0..OPTICAL_COUNT)
        .map(|index| {
            let khz = OPTICAL_FIRST_KHZ + index * OPTICAL_SPACING_KHZ;
            Carrier {
                name: megahertz(khz * 1000),
                frequency_hz: khz * 1000,
            }
        })
        .collect()
}

/// A frequency in megahertz, to as many places as it takes and no more:
/// 73750000 is `73.75`, 80250000 `80.25`, 473000000 `473`.
pub fn megahertz(hz: u32) -> String {
    let whole = hz / 1_000_000;
    let rest = hz % 1_000_000;
    if rest == 0 {
        return whole.to_string();
    }
    let places = format!("{rest:06}");
    format!("{whole}.{}", places.trim_end_matches('0'))
}

/// A run of transponders of the 124/128 degree east service, which is the one this
/// country receives with DVB-S2 rather than ISDB-S.
///
/// The numbers are the published frequency table's:
///
/// ```text
/// JCSAT-3A  JD1..JD16    12508 MHz + 15 MHz apiece,    27 MHz wide
/// JCSAT-3A  JD17..JD28   12268 MHz + 20 MHz apiece,    36 MHz wide
/// JCSAT-4B  K1..K16      12268.25 MHz + 15 MHz apiece, 27 MHz wide
/// JCSAT-4B  JD1..JD16    12508.25 MHz + 15 MHz apiece, 27 MHz wide
/// ```
///
/// The quarter megahertz is JCSAT-4B's own offset, which is what keeps the two apart in
/// a plan that numbers them alike. They share one lead, dish and oscillator and are
/// told apart only by the 22 kHz tone, so a carrier here names its satellite.
#[derive(Debug, Clone, Copy)]
pub struct SkyBand {
    pub satellite: &'static str,
    /// The letter this run's transponders are numbered under. The two
    /// satellites number theirs alike, hence the satellite in every name.
    pub series: &'static str,
    pub first_number: u32,
    pub count: u32,
    /// The centre of the first transponder at the dish. A downlink, not an
    /// intermediate frequency: the block is named anyway for the tone, and a
    /// receiver told the name subtracts the oscillator itself.
    pub downlink_khz: u32,
    pub spacing_khz: u32,
}

pub const SKY_BANDS: [SkyBand; 4] = [
    SkyBand {
        satellite: "JCSAT3A",
        series: "JD",
        first_number: 1,
        count: 16,
        downlink_khz: 12_508_000,
        spacing_khz: 15_000,
    },
    SkyBand {
        satellite: "JCSAT3A",
        series: "JD",
        first_number: 17,
        count: 12,
        downlink_khz: 12_268_000,
        spacing_khz: 20_000,
    },
    SkyBand {
        satellite: "JCSAT4B",
        series: "K",
        first_number: 1,
        count: 16,
        downlink_khz: 12_268_250,
        spacing_khz: 15_000,
    },
    SkyBand {
        satellite: "JCSAT4B",
        series: "JD",
        first_number: 1,
        count: 16,
        downlink_khz: 12_508_250,
        spacing_khz: 15_000,
    },
];

/// The ways a carrier of that grid is modulated: it carries two generations.
///
/// The service moved to DVB-S2 with 8PSK at 23.3037 Msym/s while a handful
/// still run DVB-S with QPSK at 21.096 Msym/s on the same frequencies, so a
/// walk assuming one would find the other's carriers empty. The symbol rate is
/// the one thing a demodulator cannot work out for itself.
const SKY_PROFILES: [(&str, &str, &str, u32); 2] = [
    ("", "DVBS2", "PSK/8", 23_303_000),
    ("QPSK", "DVBS", "QPSK", 21_096_000),
];

/// One carrier of the 124/128 degree east plan, in one of its modulations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkyChannel {
    /// What the published table calls it, with the satellite in front because
    /// the two number theirs the same way: `JCSAT3A/JD5`, `JCSAT4B/K12`.
    pub name: String,
    /// The block on the dish to look through, which is also what puts the tone
    /// on the lead.
    pub satellite: &'static str,
    pub downlink_khz: u32,
    /// The table gives the polarisation by parity: odd vertical, even
    /// horizontal, on both satellites and in both series.
    pub vertical: bool,
    pub system: &'static str,
    pub modulation: &'static str,
    pub symbol_rate: u32,
}

impl SkyChannel {
    pub fn polarization(&self) -> &'static str {
        if self.vertical {
            "VERTICAL"
        } else {
            "HORIZONTAL"
        }
    }
}

/// Every transponder of the 124/128 degree east plan in every modulation, for a
/// walk to try. Nothing here can be cut short the way a 110 degree east walk
/// is: that plan is one network describing itself, and this one is a grid whose
/// carriers say nothing about their neighbours.
pub fn sky() -> Vec<SkyChannel> {
    let mut channels = Vec::new();
    for band in SKY_BANDS {
        for index in 0..band.count {
            let number = band.first_number + index;
            for (suffix, system, modulation, symbol_rate) in SKY_PROFILES {
                let mut name = format!("{}/{}{number}", band.satellite, band.series);
                if !suffix.is_empty() {
                    name.push('/');
                    name.push_str(suffix);
                }
                channels.push(SkyChannel {
                    name,
                    satellite: band.satellite,
                    downlink_khz: band.downlink_khz + index * band.spacing_khz,
                    vertical: number % 2 == 1,
                    system,
                    modulation,
                    symbol_rate,
                });
            }
        }
    }
    channels
}

#[cfg(test)]
mod tests {
    use super::{
        Band, DEFAULT_BANDS, cable, is_bs, megahertz, optical, satellite, satellite_frequency, sky,
        terrestrial, transponder_of,
    };

    #[test]
    fn the_terrestrial_walk_starts_at_uhf_where_the_broadcasts_are() {
        let carriers = terrestrial(&[Band::Uhf, Band::Catv, Band::ExUhf, Band::Vhf]);
        assert_eq!(carriers[0].name, "13");
        assert_eq!(carriers[0].frequency_hz, 473_142_857);
        // What is broadcast ends at 52.
        assert_eq!(carriers[39].name, "52");
        // The cable band follows, then the channels the plan gave back, then
        // VHF, so trimming any of them shortens the list from the end.
        assert_eq!(carriers[40].name, "C13");
        assert_eq!(carriers[91].name, "53");
        assert_eq!(carriers[91].frequency_hz, 713_142_857);
        assert_eq!(carriers[100].name, "62");
        assert_eq!(carriers[101].name, "1");
        assert_eq!(carriers.len(), 113);
    }

    #[test]
    fn a_band_that_is_left_out_takes_only_the_tail_with_it() {
        let all = terrestrial(&[Band::Uhf, Band::Catv, Band::ExUhf, Band::Vhf]);
        let default = terrestrial(&DEFAULT_BANDS);
        let uhf = terrestrial(&[Band::Uhf]);
        assert_eq!(uhf.len(), 40);
        assert_eq!(default.len(), 101);
        assert_eq!(uhf, all[..40]);
        assert_eq!(default, all[..101]);
    }

    /// However the bands were written down, the order is the plan's own.
    #[test]
    fn the_order_is_the_plans_and_not_the_order_they_were_asked_for() {
        assert_eq!(
            terrestrial(&[Band::Vhf, Band::Catv, Band::Uhf]),
            terrestrial(&[Band::Uhf, Band::Catv, Band::Vhf])
        );
    }

    #[test]
    fn a_cable_carrier_sits_a_seventh_of_a_megahertz_below_a_terrestrial_one() {
        let cable = cable(&[Band::Uhf]);
        assert_eq!(cable[0].name, "13");
        assert_eq!(cable[0].frequency_hz, 473_000_000);
        assert_eq!(
            terrestrial(&[Band::Uhf])[0].frequency_hz - cable[0].frequency_hz,
            142_857
        );
    }

    #[test]
    fn the_joins_between_the_stretches_are_where_the_plan_puts_them() {
        let carriers = terrestrial(&[Band::Uhf, Band::Catv, Band::Vhf]);
        let at = |name: &str| {
            carriers
                .iter()
                .find(|one| one.name == name)
                .unwrap_or_else(|| panic!("{name} is in the plan"))
                .frequency_hz
        };
        // C22 sits against the top of the mid band rather than beside C21.
        assert_eq!(at("C22") - at("C21"), 8_000_000);
        // 7 and 8 are four megahertz apart because the two VHF stretches
        // overlap by two.
        assert_eq!(at("8") - at("7"), 4_000_000);
    }

    /// The plan is the broadcasters' numbering, and the two hands of
    /// polarisation are interleaved: the right-hand transponders come first.
    #[test]
    fn the_satellite_plan_is_the_broadcasters_numbering() {
        let transponders = satellite();
        assert_eq!(transponders.len(), 48);
        assert_eq!(transponders[0].name, "BS01");
        assert_eq!(transponders[0].frequency_khz, 1_049_480);
        assert!(transponders[0].right_hand);
        assert_eq!(transponders[1].name, "BS03");
        assert_eq!(transponders[12].name, "ND02");
        assert_eq!(transponders[12].frequency_khz, 1_613_000);
        // The left-hand bands follow, and are not what an ordinary
        // installation receives.
        assert_eq!(transponders[24].name, "BS02");
        assert!(!transponders[24].right_hand);
        assert_eq!(transponders[36].name, "ND01");
    }

    /// The 124/128 degree east grid, in both of the generations it carries.
    #[test]
    fn the_sky_plan_carries_both_generations_on_one_grid() {
        let channels = sky();
        assert_eq!(channels.len(), 60 * 2);
        assert_eq!(channels[0].name, "JCSAT3A/JD1");
        assert_eq!(channels[0].downlink_khz, 12_508_000);
        assert_eq!(channels[0].system, "DVBS2");
        assert_eq!(channels[0].modulation, "PSK/8");
        assert_eq!(channels[0].polarization(), "VERTICAL");
        // The same carrier as the older generation, named apart.
        assert_eq!(channels[1].name, "JCSAT3A/JD1/QPSK");
        assert_eq!(channels[1].system, "DVBS");
        assert_eq!(channels[1].symbol_rate, 21_096_000);
        assert_eq!(channels[1].downlink_khz, channels[0].downlink_khz);
        // Even numbers are horizontal, and the second satellite sits a quarter
        // of a megahertz off the first.
        let second = channels
            .iter()
            .find(|one| one.name == "JCSAT3A/JD2")
            .expect("JD2 is in the plan");
        assert_eq!(second.polarization(), "HORIZONTAL");
        let other = channels
            .iter()
            .find(|one| one.name == "JCSAT4B/JD1")
            .expect("JCSAT4B numbers its own");
        assert_eq!(other.downlink_khz, 12_508_250);
        assert_eq!(other.satellite, "JCSAT4B");
    }

    /// The optical distribution is its own grid: evenly spaced, named by
    /// frequency, and neither the cable plan nor the terrestrial one.
    #[test]
    fn the_optical_distribution_is_a_grid_of_its_own() {
        let carriers = optical();
        assert_eq!(carriers.len(), 108);
        assert_eq!(carriers[0].name, "73.75");
        assert_eq!(carriers[0].frequency_hz, 73_750_000);
        assert_eq!(carriers[1].name, "80.25");
        assert_eq!(carriers[1].frequency_hz, 80_250_000);
        // One of the carriers measured on a feed that carries this service.
        assert!(
            carriers.iter().any(|one| one.frequency_hz == 112_750_000),
            "the grid holds the carriers this service is received on"
        );
        assert_eq!(carriers[107].frequency_hz, 769_250_000);
    }

    #[test]
    fn a_frequency_in_megahertz_takes_only_the_places_it_needs() {
        assert_eq!(megahertz(73_750_000), "73.75");
        assert_eq!(megahertz(86_750_000), "86.75");
        assert_eq!(megahertz(473_000_000), "473");
        assert_eq!(megahertz(473_142_857), "473.142857");
        assert_eq!(megahertz(100_500_000), "100.5");
    }

    /// A stream's identifier names its transponder, on ISDB-S and on the 4K
    /// services alike.
    #[test]
    fn a_satellite_stream_names_its_transponder() {
        let bs15 = satellite()
            .into_iter()
            .find(|known| known.name == "BS15")
            .expect("BS15 is in the plan");
        assert_eq!(satellite_frequency(4, 0x48f3), Some(bs15.frequency_khz));
        assert!(satellite_frequency(6, 0x6020).is_some());
        assert_eq!(satellite_frequency(0x7fe0, 0x7fe0), None);
        assert_eq!(transponder_of(11, 0xb0e0), Some(14));
        assert!(satellite_frequency(11, 0xb0e0).is_some());
        assert!(is_bs(11) && !is_bs(12));
    }

    #[test]
    fn a_band_is_read_however_it_was_written() {
        assert_eq!(Band::parse("uhf"), Some(Band::Uhf));
        assert_eq!(Band::parse(" CATV "), Some(Band::Catv));
        assert_eq!(Band::parse("cable"), Some(Band::Catv));
        assert_eq!(Band::parse("VHF"), Some(Band::Vhf));
        assert_eq!(Band::parse("BS"), None);
    }
}

#[cfg(test)]
mod range_tests {
    use super::{Range, range_of, satellite, sky};

    #[test]
    fn every_name_the_plan_makes_reads_as_the_stretch_it_is_in() {
        assert_eq!(range_of("1"), Range::Vhf);
        assert_eq!(range_of("12"), Range::Vhf);
        assert_eq!(range_of("13"), Range::Uhf);
        // Where the broadcast plan stops and the cable's own use begins.
        assert_eq!(range_of("52"), Range::Uhf);
        assert_eq!(range_of("53"), Range::ExUhf);
        assert_eq!(range_of("62"), Range::ExUhf);
        assert_eq!(range_of("C13"), Range::Catv);
        assert_eq!(range_of("C63"), Range::Catv);
        for transponder in satellite() {
            let range = range_of(&transponder.name);
            assert!(
                range == Range::Bs || range == Range::Cs,
                "{} read as {range:?}",
                transponder.name
            );
        }
        for transponder in sky() {
            assert_eq!(
                range_of(&transponder.name),
                Range::Sky,
                "{}",
                transponder.name
            );
        }
    }

    /// A name the plan did not make is walked rather than filtered out.
    #[test]
    fn a_name_from_somewhere_else_is_left_alone() {
        assert_eq!(range_of("111"), Range::Elsewhere);
        assert_eq!(range_of(""), Range::Elsewhere);
    }
}

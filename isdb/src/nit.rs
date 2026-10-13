// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The table a network uses to describe itself.
//!
//! One lock on a satellite is enough to learn the whole of it: the network
//! information table lists every transport stream of the network with the
//! frequency, polarisation and symbol rate it is on, and the services it
//! carries -- and, more to the point, which identifier each answers to. Nothing
//! else does: a transponder cannot be tuned by frequency alone on a satellite
//! that carries several streams on one carrier.
//!
//! The 110 degree east CS band has two networks sharing it, so the tables of
//! other networks are read as well as the tuner's own.

use mpegts::sections::Sections;

/// The table rides on a packet identifier of its own.
const NIT_PID: u16 = 0x10;
/// The service tables ride on another. They are read here as well as in
/// [`crate::sdt`]: what is wanted here is the names of the other multiplexes'
/// services, which only the network table lists and only those tables name.
const SDT_PID: u16 = 0x11;

/// This network, and another one on the same feed.
const TABLE_OWN: u8 = 0x40;
const TABLE_OTHER: u8 = 0x41;

/// The descriptors read here.
const SATELLITE_DELIVERY: u8 = 0x43;
const CABLE_DELIVERY: u8 = 0x44;
const SERVICE_LIST: u8 = 0x41;
const NETWORK_NAME: u8 = 0x40;
/// Which button a multiplex sits on, and what the multiplex is called.
///
/// **The same tag means something else in a TLV-NIT.** There it is the remote
/// control key descriptor of ARIB STD-B60, it sits among the network's own
/// descriptors rather than a multiplex's, and it names a button per *service*
/// -- which is the only place an MMT/TLV service's button is transmitted at
/// all. See [`described_tlv`].
const TS_INFORMATION: u8 = 0xcd;
/// The same tag, read the other way: the remote control key descriptor of a
/// TLV-NIT.
const REMOTE_CONTROL_KEY: u8 = 0xcd;
/// Which of a terrestrial carrier's services ride in the layer a one-segment
/// receiver reads.
const PARTIAL_RECEPTION: u8 = 0xfb;
/// Where a terrestrial multiplex is transmitted from, over the air.
const TERRESTRIAL_DELIVERY: u8 = 0xfa;
/// The cable multiple carrier transmission delivery system descriptor. Not
/// 0xF4, which is the advanced cable one ([`ADVANCED_CABLE_DELIVERY`]).
const CHANNEL_BONDING_CABLE: u8 = 0xf3;
/// The advanced cable delivery system descriptor: where a multiplex is on
/// a C2 (ITU-T J.382 based) cable. Its shape is the cable UHDTV working
/// party's report, Table 7.4-41 (`C2_delivery_system_descriptor`).
const ADVANCED_CABLE_DELIVERY: u8 = 0xf4;
/// The cable TS division system descriptor: a multiplex split over several
/// cable carriers by the older (J.183) scheme, each carrier naming the
/// services it holds.
const CABLE_TS_DIVISION: u8 = 0xf9;
/// The SI parameter descriptor (ARIB STD-B10 6.2.35): how often each table
/// is sent. In the NIT and the BIT.
const SI_PARAMETER: u8 = 0xd7;
const SYSTEM_MANAGEMENT: u8 = 0xfe;
const STUFFING: u8 = 0x42;
const LINKAGE: u8 = 0x4a;
const SERVICE_GROUP: u8 = 0xe0;
const AREA_BROADCASTING: u8 = 0xe1;
const EMERGENCY_INFORMATION: u8 = 0xfc;
const CONNECTED_TRANSMISSION: u8 = 0xdd;
const CA_EMM_TS: u8 = 0xca;

/// One service a network table lists for a multiplex.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListedService {
    pub id: u16,
    /// What kind of service it is, as ARIB STD-B10 numbers them.
    pub kind: u8,
    /// What a service table called it, or empty where none has.
    pub name: String,
    /// Set where something was dropped in reading the name.
    pub lossy: bool,
}

/// One transport stream of a network.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transport {
    /// What the stream answers to, which is what a satellite tune asks for.
    pub stream_id: u16,
    pub original_network_id: u16,
    /// Where the multiplex is carried on a satellite, where the network said
    /// so. It is what lets one lock anywhere on the network yield the whole of
    /// it.
    ///
    /// The descriptor as it was written, not as a tune takes it: the frequency
    /// in it is the downlink, and turning that into something a tuner accepts
    /// needs to know which block is on the dish, which is not broadcast
    /// knowledge.
    pub satellite: Option<Satellite>,
    /// The services it carries. The network table gives identifiers and types
    /// only; the name is what a service table called the service, and is empty
    /// where none has yet.
    pub services: Vec<ListedService>,
    /// What the network calls this multiplex, and the button a receiver puts it
    /// on. Both come from one ARIB descriptor, and both are empty where it was
    /// not sent.
    pub name: String,
    pub remote_control_key_id: u8,
    /// The TS information descriptor's transmission types: which services
    /// ride in which layer, as `(transmission_type_info, service_ids)`. The
    /// byte is defined by the operating rules rather than the standard (ARIB
    /// STD-B10 6.2.42), so it is kept as sent; TR-B14 uses it for the
    /// hierarchy of a terrestrial carrier.
    pub transmission_types: Vec<(u8, Vec<u16>)>,
    /// Where each CA system's EMMs ride, where the network said (ARIB
    /// STD-B25's CA_EMM_TS descriptor).
    pub emm: Vec<crate::descriptors::CaEmmTs>,
    /// Where the multiplex is on an advanced (C2) cable, where the network
    /// said so.
    pub advanced_cable: Option<AdvancedCable>,
    /// The carriers a multiplex was divided over by the older cable scheme,
    /// each with the services it holds.
    pub cable_division: Vec<CableDivisionCarrier>,
    /// How often the tables are sent, where the network said.
    pub si_parameters: Option<SiParameters>,
    /// The services the network says ride in the partial reception layer of
    /// this carrier, which is one-segment. Their service type says only "data",
    /// so the layer is the one thing that tells them from a genuine data
    /// service. ARIB TR-B14 30.4.3.3 puts this here whenever the carrier uses
    /// the layer.
    pub partial_reception: Vec<u16>,
    /// Where the multiplex is on a cable, where the network said so. Unlike the
    /// satellite descriptor, its frequency is the one to tune.
    pub cable: Option<Cable>,
    /// The carriers a multiplex too wide for one was split over. A cable and a
    /// transport stream are both split this way, so it is read in both tables.
    /// See [`channel_bonding_cable`].
    pub channel_bonding: Vec<ChannelBondingCarrier>,
    /// Whether the multiplex says it is a broadcast at all, where it said.
    pub system: Option<SystemManagement>,
    /// The services this multiplex points at.
    pub links: Vec<crate::nitdescriptors::Linkage>,
    /// The emergency warning signal, sent only while such a broadcast is being
    /// made: its presence is the signal.
    pub emergency: Vec<crate::nitdescriptors::EmergencyService>,
    /// Whether this multiplex is one of a group sent together, which is how
    /// two multiplexes are halves of one thing.
    pub connected: Option<crate::nitdescriptors::ConnectedTransmission>,
    /// Services paired as belonging together.
    pub service_groups: Vec<crate::nitdescriptors::ServiceGroup>,
    /// The transmitting stations of an area broadcast.
    pub area_broadcasting: Vec<crate::nitdescriptors::AreaBroadcastingStation>,
    /// Where the multiplex goes out over the air, where the network said so. It
    /// is the broadcaster's word: a cable passing the broadcast through does
    /// not rewrite it, so a carrier received somewhere this does not name is
    /// one something moved.
    pub terrestrial: Option<Terrestrial>,
}

/// Where a terrestrial multiplex is transmitted, as its own network says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terrestrial {
    /// The broadcast area the transmitters serve, as ARIB numbers them. Two
    /// area codes on one lead are two areas' broadcasts.
    pub area_code: u16,
    /// Every transmitter of this multiplex, as a frequency in hertz on the same
    /// reckoning as a tune takes -- the plan's seventh of a megahertz included.
    pub carriers_hz: Vec<u32>,
}

/// Where a multiplex is on a satellite, as the descriptor wrote it. The
/// frequency is the downlink one, which no tuner takes: a receiver subtracts
/// its low-noise block's oscillator, and which oscillator that was is a fact
/// about the dish.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Satellite {
    /// The downlink frequency, in kilohertz.
    pub downlink_khz: u32,
    /// Where the satellite sits, in tenths of a degree, with `east` for the
    /// hemisphere. 1100 east is the 110 degrees the BS satellites are at.
    pub orbital_position: u16,
    pub east: bool,
    /// `H`, `V`, `L` or `R`. Japanese satellite broadcasting is circular, and
    /// the hand decides which oscillator the block used, and so which
    /// intermediate frequency the multiplex arrives on.
    pub polarisation: char,
    /// The descriptor's modulation field, read ARIB's way: the low five bits as
    /// one number. DVB divides those same bits into three fields and nothing
    /// says which was meant, so the reading belongs to whoever knows what they
    /// tuned. See the `dvb_` methods, which offer the other one.
    pub modulation: u8,
    /// Symbols per second.
    pub symbol_rate: u32,
    pub inner_correction: u8,
}

impl Satellite {
    /// Whether it is right-hand circular, which is what every Japanese
    /// installation can receive; left-hand needs a block and a distribution
    /// that pass the higher band.
    #[must_use]
    pub fn right_hand(&self) -> bool {
        self.polarisation != 'L'
    }

    /// Whether the descriptor's modulation_system bit says DVB-S2 rather than
    /// DVB-S. The DVB reading; see [`Satellite::modulation`].
    #[must_use]
    pub fn dvb_second_generation(&self) -> bool {
        self.modulation & 0x04 != 0
    }

    /// The roll-off as a channel file writes it, and nothing for the reserved
    /// value or for DVB-S, where the field means nothing.
    #[must_use]
    pub fn dvb_roll_off(&self) -> Option<&'static str> {
        if !self.dvb_second_generation() {
            return None;
        }
        match (self.modulation >> 3) & 0x03 {
            0 => Some("35"),
            1 => Some("25"),
            2 => Some("20"),
            _ => None,
        }
    }

    /// The modulation as a channel file writes it, and nothing where the
    /// descriptor said to work it out.
    #[must_use]
    pub fn dvb_modulation(&self) -> Option<&'static str> {
        match self.modulation & 0x03 {
            1 => Some("QPSK"),
            2 => Some("PSK/8"),
            3 => Some("QAM/16"),
            // Zero is "auto", which is the descriptor declining to say.
            // Passing that on as a modulation would be inventing one.
            _ => None,
        }
    }

    /// The inner error-correction rate as a channel file writes it, and
    /// nothing where the descriptor did not say.
    #[must_use]
    pub fn inner_fec(&self) -> Option<&'static str> {
        inner_fec_name(self.inner_correction)
    }
}

/// The system management descriptor, which is how a table says whether what it
/// describes is a broadcast at all. ARIB STD-B10 6.2.21 makes it mandatory in
/// either the PMT or the network table, and the identifier's top two bits are
/// the answer: a multiplex an operator carries for its own purposes says
/// non-broadcasting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemManagement {
    /// What the top two bits say. False is the descriptor saying so outright,
    /// not the descriptor being absent.
    pub broadcasting: bool,
    /// The standard, in six bits.
    pub broadcasting_identifier: u8,
    /// The low byte of the identifier, and whatever followed it.
    pub additional_identification: u8,
    pub extra: Vec<u8>,
}

/// Reads the system management descriptor.
///
/// ```text
/// system_management_id            16  broadcasting_flag(2) identifier(6) additional(8)
/// additional_identification_info   8  each, to the end
/// ```
#[must_use]
pub fn system_management(body: &[u8]) -> Option<SystemManagement> {
    if body.len() < 2 {
        return None;
    }
    Some(SystemManagement {
        // Zero is broadcasting; one and two are not, and three is undefined,
        // which is not a promise that it is a broadcast either.
        broadcasting: body[0] >> 6 == 0,
        broadcasting_identifier: body[0] & 0x3f,
        additional_identification: body[1],
        extra: body[2..].to_vec(),
    })
}

/// One carrier of the cable multiple carrier transmission delivery system
/// descriptor, which is how a multiplex too wide for one cable carrier says
/// which carriers it was split over.
///
/// ARIB STD-B10 gives it tag 0xF3 and leaves the shape to the Ministry's 2015
/// notification No. 92 and JCTEA STD-003; the cable UHDTV working party's
/// report sets it out in section 6.4. One record per carrier, in
/// `carrier_sequence` order. It is not the advanced cable delivery system
/// descriptor, which is tag 0xF4.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChannelBondingCarrier {
    /// Where the carrier is on the cable, which is the frequency a tune takes:
    /// there is no oscillator between the carrier and the receiver.
    pub frequency_hz: u32,
    /// Which shape of multiplexing frame this is, by the descriptor's own
    /// numbering.
    pub frame_type: u8,
    pub outer_correction: u8,
    /// The descriptor's raw modulation field, numbered as the cable descriptor
    /// numbers it.
    pub modulation: u8,
    pub symbol_rate: u32,
    /// 0xF by the specification, there being no inner code on a cable. Kept as
    /// sent rather than assumed.
    pub inner_correction: u8,
    /// The group of carriers this one belongs to. Every carrier of one split
    /// multiplex carries the same group, which is what says they are to be put
    /// back together.
    pub group_id: u8,
}

impl ChannelBondingCarrier {
    /// The modulation as a channel file writes it.
    #[must_use]
    pub fn modulation_name(&self) -> Option<&'static str> {
        Cable {
            modulation: self.modulation,
            ..Cable::default()
        }
        .modulation_name()
    }

    /// The inner error-correction rate as a channel file writes it.
    #[must_use]
    pub fn inner_fec(&self) -> Option<&'static str> {
        inner_fec_name(self.inner_correction)
    }
}

/// One carrier's record. See the syntax in [`channel_bonding_cable`].
const CHANNEL_BONDING_CARRIER_BYTES: usize = 12;

/// Reads the cable multiple carrier transmission delivery system descriptor, which the
/// TLV network table carries where the advanced services have been put on a cable.
///
/// It is neither the satellite descriptor nor the ordinary cable one, and a reader
/// looking for those finds an operator's table saying nothing about where anything is.
/// On one feed, every stream of the 高度BS network carried this and no other.
///
/// ```text
/// frequency               32  BCD, MHz with the point after four digits
/// reserved_for_future_use  8
/// frame_type               4
/// FEC_outer                4
/// modulation               8
/// symbol_rate             28  BCD, Msymbol/s with the point after three
/// FEC_inner                4
/// group_id                 8
/// ```
#[must_use]
pub fn channel_bonding_cable(body: &[u8]) -> Vec<ChannelBondingCarrier> {
    let mut carriers = Vec::new();
    for record in body.as_chunks::<CHANNEL_BONDING_CARRIER_BYTES>().0 {
        let (Some(frequency), Some(symbol_rate)) =
            (decimal(&record[0..4], 8), decimal(&record[7..11], 7))
        else {
            return carriers;
        };
        carriers.push(ChannelBondingCarrier {
            // Megahertz with the point after four digits: hundreds of hertz.
            frequency_hz: frequency.saturating_mul(100),
            frame_type: record[5] >> 4,
            outer_correction: record[5] & 0x0f,
            modulation: record[6],
            symbol_rate: symbol_rate.saturating_mul(100),
            inner_correction: record[10] & 0x0f,
            group_id: record[11],
        });
    }
    carriers
}

/// One data slice of an advanced cable (C2) multiplex: where to tune for it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AdvancedCableSlice {
    pub data_slice_id: u8,
    /// `C2_tuning_frequency`, in hertz, read the way `frequency_type` says.
    pub frequency_hz: u32,
    /// 0 the data slice's centre, 1 the C2 system's centre, 2 the initial
    /// centre for a data slice L1 cannot be read from.
    pub frequency_type: u8,
    /// 1 none, 2 RS(204,188), 3 advanced cable (see L1).
    pub outer_code: u8,
    /// As the cable descriptor numbers it (1 16QAM .. 7 1024QAM), or 0x80
    /// for the advanced scheme (see L1).
    pub modulation: u8,
    /// 1..=5 the code rates 1/2 .. 7/8, 8 ISDB-S (TMCC), 9 advanced cable
    /// (L1), 15 none.
    pub inner_code: u8,
}

/// The advanced cable delivery system descriptor (tag 0xF4): a C2 system
/// and the data slices carrying the multiplex.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdvancedCable {
    pub tag_extension: u8,
    pub plp_id: u8,
    /// 0 is 448 µs, 1 is 597.33 µs.
    pub ofdm_symbol_duration: u8,
    /// 0 is 1/128, 1 is 1/64.
    pub guard_interval: u8,
    pub bundled_channel: u8,
    pub slices: Vec<AdvancedCableSlice>,
}

impl AdvancedCable {
    /// The modulation of a slice in words.
    pub fn modulation_name(modulation: u8) -> Option<&'static str> {
        Some(match modulation {
            0x80 => "高度ケーブル（L1）",
            other => {
                return Cable {
                    modulation: other,
                    ..Cable::default()
                }
                .modulation_name();
            }
        })
    }

    /// The inner code of a slice in words.
    pub fn inner_code_name(code: u8) -> Option<&'static str> {
        Some(match code {
            8 => "ISDB-S（TMCC）",
            9 => "高度ケーブル（L1）",
            other => return inner_fec_name(other),
        })
    }
}

/// Reads the advanced cable delivery system descriptor.
///
/// ```text
/// descriptor_tag_extension     8
/// plp_id                       8
/// active_OFDM_symbol_duration  3
/// guard_interval               3
/// bundled_channel              8
/// reserved                     2
/// for each data slice:
///   data_slice_id              8
///   C2_tuning_frequency       32
///   C2_tuning_frequency_type   2
///   outer_code                 4
///   modulation                 8
///   inner_code                 4
///   reserved                   6
/// ```
#[must_use]
pub fn advanced_cable_delivery(body: &[u8]) -> Option<AdvancedCable> {
    let head = body.get(..4)?;
    let mut read = AdvancedCable {
        tag_extension: head[0],
        plp_id: head[1],
        ofdm_symbol_duration: head[2] >> 5,
        guard_interval: (head[2] >> 2) & 0x07,
        bundled_channel: (head[2] << 6) | (head[3] >> 2),
        slices: Vec::new(),
    };
    for record in body[4..].as_chunks::<8>().0 {
        read.slices.push(AdvancedCableSlice {
            data_slice_id: record[0],
            frequency_hz: u32::from_be_bytes([record[1], record[2], record[3], record[4]]),
            frequency_type: record[5] >> 6,
            outer_code: (record[5] >> 2) & 0x0f,
            modulation: (record[5] << 6) | (record[6] >> 2),
            inner_code: ((record[6] & 0x03) << 2) | (record[7] >> 6),
        });
    }
    Some(read)
}

/// One carrier of a cable TS division system descriptor (tag 0xF9): where
/// part of a divided multiplex is, and which services that part holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CableDivisionCarrier {
    /// The carrier's frequency, in hertz (the descriptor writes megahertz
    /// with four digits after the point).
    pub frequency_hz: u32,
    /// As the cable descriptor numbers it (JCTEA).
    pub frame_type: u8,
    pub outer_correction: u8,
    pub modulation: u8,
    pub symbol_rate: u32,
    pub inner_correction: u8,
    /// `future_use_data`, kept as sent where the descriptor carried any.
    pub extra: Vec<u8>,
    /// The services carried on this carrier.
    pub services: Vec<u16>,
}

impl CableDivisionCarrier {
    pub fn modulation_name(&self) -> Option<&'static str> {
        Cable {
            modulation: self.modulation,
            ..Cable::default()
        }
        .modulation_name()
    }

    pub fn inner_fec(&self) -> Option<&'static str> {
        inner_fec_name(self.inner_correction)
    }
}

/// Reads the cable TS division system descriptor.
///
/// ```text
/// per carrier:
///   frequency               32  BCD, MHz with four decimals
///   reserved                 7
///   future_use_data_flag     1  nought means the data follows
///   frame_type               4
///   FEC_outer                4
///   modulation               8
///   symbol_rate             28  BCD, Msym/s with four decimals
///   FEC_inner                4
///   if flag == 0: length 8, then that many bytes
///   number_of_service        8
///   service_id              16 × number_of_service
/// ```
#[must_use]
pub fn cable_ts_division(body: &[u8]) -> Vec<CableDivisionCarrier> {
    let mut carriers = Vec::new();
    let mut at = 0;
    while let Some(head) = body.get(at..at + 11) {
        let (Some(frequency), Some(symbol_rate)) =
            (decimal(&head[0..4], 8), decimal(&head[7..11], 7))
        else {
            break;
        };
        let mut carrier = CableDivisionCarrier {
            frequency_hz: frequency.saturating_mul(100),
            frame_type: head[5] >> 4,
            outer_correction: head[5] & 0x0f,
            modulation: head[6],
            symbol_rate: symbol_rate.saturating_mul(100),
            inner_correction: head[10] & 0x0f,
            extra: Vec::new(),
            services: Vec::new(),
        };
        at += 11;
        if head[4] & 0x01 == 0 {
            let Some(&length) = body.get(at) else {
                break;
            };
            at += 1;
            let Some(extra) = body.get(at..at + usize::from(length)) else {
                break;
            };
            carrier.extra = extra.to_vec();
            at += usize::from(length);
        }
        let Some(&count) = body.get(at) else {
            break;
        };
        at += 1;
        for _ in 0..count {
            let Some(pair) = body.get(at..at + 2) else {
                break;
            };
            carrier
                .services
                .push(u16::from_be_bytes([pair[0], pair[1]]));
            at += 2;
        }
        carriers.push(carrier);
    }
    carriers
}

/// What an SI parameter descriptor (0xD7, MH 0x8017) says: a version, when
/// it was last changed (MJD), and per table the operator's own description
/// bytes (ARIB TR-B14/B15 lay them out per table: cycle times and the like).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SiParameters {
    pub version: u8,
    pub update_mjd: u16,
    /// `(table_id, table_description_byte)`.
    pub tables: Vec<(u8, Vec<u8>)>,
}

/// Reads an SI parameter descriptor.
#[must_use]
pub fn si_parameter(body: &[u8]) -> Option<SiParameters> {
    let head = body.get(..3)?;
    let mut read = SiParameters {
        version: head[0],
        update_mjd: u16::from_be_bytes([head[1], head[2]]),
        tables: Vec::new(),
    };
    let mut at = 3;
    while let Some(entry) = body.get(at..at + 2) {
        let length = usize::from(entry[1]);
        let Some(description) = body.get(at + 2..at + 2 + length) else {
            break;
        };
        read.tables.push((entry[0], description.to_vec()));
        at += 2 + length;
    }
    Some(read)
}

/// The inner error-correction rates as a channel file writes them. Zero is the
/// descriptor declining to say and fifteen is none at all.
fn inner_fec_name(correction: u8) -> Option<&'static str> {
    Some(match correction {
        1 => "1/2",
        2 => "2/3",
        3 => "3/4",
        4 => "5/6",
        5 => "7/8",
        6 => "8/9",
        7 => "3/5",
        8 => "4/5",
        9 => "9/10",
        15 => "NONE",
        _ => return None,
    })
}

/// Where a multiplex is on a cable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cable {
    pub frequency_hz: u32,
    /// Which shape of multiplexing frame the carrier uses (JCTEA: 1 and 2
    /// are TSMF [53,15], 0xF a single transport stream with no frame).
    pub frame_type: u8,
    /// The descriptor's own modulation number (JCTEA table 6.4-4): 1 is
    /// 16QAM, 2 32QAM, 3 64QAM, 4 128QAM, 5 256QAM, 7 1024QAM.
    pub modulation: u8,
    pub symbol_rate: u32,
    pub outer_correction: u8,
    pub inner_correction: u8,
}

impl Cable {
    /// The modulation as a channel file writes it. The descriptor numbers them
    /// from 16-QAM upwards, doubling at each step, zero being "not defined".
    /// One reading rather than two: the cable descriptor gives its modulation a
    /// byte of its own.
    #[must_use]
    pub fn modulation_name(&self) -> Option<&'static str> {
        Some(match self.modulation {
            1 => "QAM/16",
            2 => "QAM/32",
            3 => "QAM/64",
            4 => "QAM/128",
            5 => "QAM/256",
            7 => "QAM/1024",
            _ => return None,
        })
    }

    /// The inner error-correction rate as a channel file writes it.
    #[must_use]
    pub fn inner_fec(&self) -> Option<&'static str> {
        inner_fec_name(self.inner_correction)
    }
}

/// One network a table described, as it named itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Network {
    pub id: u16,
    pub name: String,
    /// Whether this network says it is a broadcast at all, where it said.
    pub system: Option<SystemManagement>,
    /// The services the network points at: information about itself, a
    /// programme guide, a stream carrying the whole of its own information.
    pub links: Vec<crate::nitdescriptors::Linkage>,
    /// Whether this is the network the stream belongs to, as opposed to one it
    /// merely described.
    pub actual: bool,
}

/// Reads the table out of a stream as it arrives.
#[derive(Debug)]
pub struct Reader {
    sections: Sections,
    /// The service tables, for the names the listing has none of.
    service_sections: Sections,
    /// What those tables have named so far. A table and the listing it names
    /// arrive in no order against each other, so each side fills in what the
    /// other has.
    namings: Vec<crate::sdt::Naming>,
    found: Vec<Transport>,
    networks: Vec<u16>,
    /// The networks whose own table was read here, as opposed to the ones a
    /// table described in passing. A network describes its neighbours, and
    /// being described is not being received.
    actual: Vec<u16>,
    /// The networks as they named themselves.
    described: Vec<Network>,
    /// Which sections of this stream's own table have been seen, for telling a
    /// table that has arrived whole from one that has only begun.
    whole: SubTable,
}

/// Which sections of one sub-table have gone past. A sub-table is split into
/// numbered sections and they arrive one at a time, so a reader that stopped at
/// the first would list three transponders of a satellite that carries twelve.
/// A version change starts the count again.
#[derive(Debug, Default)]
struct SubTable {
    version: Option<u8>,
    last: u8,
    seen: Vec<u8>,
}

impl SubTable {
    fn note(&mut self, version: u8, number: u8, last: u8) {
        if self.version != Some(version) {
            self.version = Some(version);
            self.seen.clear();
        }
        self.last = last;
        if !self.seen.contains(&number) {
            self.seen.push(number);
        }
    }

    /// Whether every section from the first to the last has gone past. A table
    /// nothing has been seen of is not whole: there is no table.
    fn whole(&self) -> bool {
        self.version.is_some() && (0..=self.last).all(|number| self.seen.contains(&number))
    }
}

impl Default for Reader {
    fn default() -> Self {
        Self::new()
    }
}

impl Reader {
    pub fn new() -> Self {
        Self {
            sections: Sections::new(NIT_PID),
            service_sections: Sections::new(SDT_PID),
            namings: Vec::new(),
            found: Vec::new(),
            networks: Vec::new(),
            actual: Vec::new(),
            described: Vec::new(),
            whole: SubTable::default(),
        }
    }

    /// Takes what has arrived and reads what tables are in it.
    pub fn feed(&mut self, arrived: &[u8]) {
        for section in self.sections.feed(arrived) {
            self.section(&section);
        }
        for section in self.service_sections.feed(arrived) {
            for naming in crate::sdt::namings(&section) {
                self.name(naming);
            }
        }
    }

    /// Records what a service table called a service and puts the name onto
    /// whatever the network table has already listed.
    fn name(&mut self, naming: crate::sdt::Naming) {
        // A table is repeated for as long as the stream runs, and every repeat
        // says the same thing, so the first is kept.
        if self.namings.iter().any(|known| {
            known.original_network_id == naming.original_network_id
                && known.stream_id == naming.stream_id
                && known.service_id == naming.service_id
        }) {
            return;
        }
        for transport in &mut self.found {
            if transport.stream_id != naming.stream_id
                || transport.original_network_id != naming.original_network_id
            {
                continue;
            }
            for service in &mut transport.services {
                if service.id == naming.service_id {
                    service.name = naming.name.clone();
                    service.lossy = naming.lossy;
                }
            }
        }
        self.namings.push(naming);
    }

    /// How many services the tables have named so far. A count that has stopped
    /// rising is what says the naming is done: there is no marker for having
    /// heard them all.
    pub fn namings(&self) -> usize {
        self.namings.len()
    }

    /// Puts the names read so far onto a multiplex the table has just listed.
    fn name_transport(&self, transport: &mut Transport) {
        for service in &mut transport.services {
            let found = self.namings.iter().find(|naming| {
                naming.original_network_id == transport.original_network_id
                    && naming.stream_id == transport.stream_id
                    && naming.service_id == service.id
            });
            if let Some(naming) = found {
                service.name = naming.name.clone();
                service.lossy = naming.lossy;
            }
        }
    }

    fn section(&mut self, section: &[u8]) {
        let Some(read) = described(section) else {
            return;
        };
        let network = read.network.id;
        if !self.networks.contains(&network) {
            self.networks.push(network);
        }
        if read.network.actual {
            if !self.actual.contains(&network) {
                self.actual.push(network);
            }
            // Only this stream's own table is counted towards being whole. The
            // neighbours' arrive as they please.
            self.whole
                .note((section[5] >> 1) & 0x1f, section[6], section[7]);
        }
        match self.described.iter_mut().find(|known| known.id == network) {
            Some(known) => {
                if known.name.is_empty() && !read.network.name.is_empty() {
                    known.name = read.network.name;
                }
                if known.system.is_none() {
                    known.system = read.network.system;
                }
                if known.links.is_empty() {
                    known.links = read.network.links;
                }
            }
            None => self.described.push(read.network),
        }
        for mut transport in read.transports {
            // Kept apart by network as well as by stream. A cable carries
            // several networks' tables at once, and each numbers its streams
            // for itself: keying on the stream alone lets one table stand for
            // another network's stream.
            if self.found.iter().any(|seen| {
                seen.stream_id == transport.stream_id
                    && seen.original_network_id == transport.original_network_id
            }) {
                continue;
            }
            self.name_transport(&mut transport);
            self.found.push(transport);
        }
    }

    /// The transport streams that can be tuned from what the table said, which
    /// is the ones it gave a place on the satellite, in the order the
    /// identifiers run.
    pub fn transports(&self) -> Vec<Transport> {
        let mut found: Vec<Transport> = self
            .found
            .iter()
            .filter(|transport| transport.satellite.is_some())
            .cloned()
            .collect();
        found.sort_by_key(|transport| transport.stream_id);
        found
    }

    /// Everything the tables described, including the multiplexes they gave no
    /// place to: a terrestrial or cable network names its carriers without
    /// saying where they are.
    pub fn described(&self) -> &[Transport] {
        &self.found
    }

    /// The networks as they named themselves.
    pub fn named(&self) -> &[Network] {
        &self.described
    }

    /// Whether this stream's own network table has arrived whole, so that what
    /// it lists is all of it rather than the first few.
    pub fn complete(&self) -> bool {
        self.whole.whole()
    }

    /// Which networks were named at all, their own tables and their
    /// neighbours' alike.
    pub fn networks(&self) -> &[u16] {
        &self.networks
    }

    /// Which networks this carrier actually belongs to, which is what says
    /// whether a band has been heard from twice. A table describing a neighbour
    /// is not the neighbour answering, and a walk that counted it as one would
    /// never tune its transponders.
    pub fn actual(&self) -> &[u16] {
        &self.actual
    }
}

/// A button on the receiver's remote, and the service it selects.
///
/// **Only the advanced broadcasts say this.** A transport stream puts the
/// button on the multiplex (the TS information descriptor, one button for
/// however many services ride together), and a receiver works out which
/// service a press means from the multiplex it lands on. TLV has no such
/// descriptor: ARIB TR-B39 36.1.2.3 requires the remote control key
/// descriptor wherever a TLV-NIT for the network itself is transmitted, and
/// it names the service directly. So for MMT/TLV this is the button, and
/// there is nothing else to guess from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RemoteKey {
    pub remote_control_key_id: u8,
    pub service_id: u16,
}

/// What one section of the table said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Described {
    pub network: Network,
    pub transports: Vec<Transport>,
    /// The buttons the network put its services on. **Empty for a transport
    /// stream's table**, which says it per multiplex instead
    /// ([`Transport::remote_control_key_id`]).
    pub remote_keys: Vec<RemoteKey>,
}

/// Reads one section of the network table.
///
/// **This is the whole of reading it.** [`Reader`] is the same reading with
/// the sections put back together and the names of a service table folded in;
/// a caller that already has whole sections -- a player demultiplexing the
/// stream for other reasons -- wants this instead, and both go through here so
/// that there is one place the table is read.
///
/// Nothing for a section of another table, or one too short to hold a header.
pub fn described(section: &[u8]) -> Option<Described> {
    read_section(section, false)
}

/// Reads one section of a TLV-NIT, which is the same table for an advanced
/// broadcast.
///
/// **It is not [`described`] with another argument's worth of difference.**
/// The section has the same shape and the same table identifiers, and the
/// delivery system descriptors inside it are the same ones -- but two things
/// are not, and neither shows up as an error:
///
/// * **The text is UTF-8** (ARIB STD-B44), where a transport stream's tables
///   are written in the eight-unit code of STD-B24. Running 高度ＢＳデジタル
///   放送 through the ARIB decoder yields plausible-looking kana that is
///   entirely wrong.
/// * **Tag 0xCD is a different descriptor.** In a transport stream it is the
///   TS information descriptor -- a button for the multiplex, then a name. In
///   a TLV-NIT it is the remote control key descriptor, it sits among the
///   network's descriptors rather than a multiplex's, and it lists a button
///   per service. Read the wrong way round it puts a service identifier's high
///   byte on the remote and a name made of the rest.
///
/// So the two are separate calls, and the caller says which transmission the
/// section came out of. Nothing in the bytes does.
pub fn described_tlv(section: &[u8]) -> Option<Described> {
    read_section(section, true)
}

fn read_section(section: &[u8], tlv: bool) -> Option<Described> {
    if section.len() < 12 || !matches!(section[0], TABLE_OWN | TABLE_OTHER) {
        return None;
    }
    let network = Network {
        id: (u16::from(section[3]) << 8) | u16::from(section[4]),
        name: String::new(),
        system: None,
        links: Vec::new(),
        actual: section[0] == TABLE_OWN,
    };
    let mut read = Described {
        network,
        transports: Vec::new(),
        remote_keys: Vec::new(),
    };
    // The network's own descriptors, which is where its name is, and then the
    // loop of transport streams.
    let network_descriptors = ((usize::from(section[8]) & 0x0f) << 8) | usize::from(section[9]);
    if 10 + network_descriptors <= section.len() {
        let about = &section[10..10 + network_descriptors];
        if let Some(name) = named(about, tlv) {
            read.network.name = name;
        }
        read.network.system = describes_system(about);
        read.network.links = describes_links(about);
        if tlv {
            read.remote_keys = remote_keys(about);
        }
    }
    let mut at = 10 + network_descriptors;
    if at + 2 > section.len() {
        return Some(read);
    }
    let loop_length = ((usize::from(section[at]) & 0x0f) << 8) | usize::from(section[at + 1]);
    at += 2;
    let end = (at + loop_length).min(section.len().saturating_sub(4));
    while at + 6 <= end {
        let stream_id = (u16::from(section[at]) << 8) | u16::from(section[at + 1]);
        let original_network_id = (u16::from(section[at + 2]) << 8) | u16::from(section[at + 3]);
        let length = ((usize::from(section[at + 4]) & 0x0f) << 8) | usize::from(section[at + 5]);
        at += 6;
        let stop = (at + length).min(end);
        let mut transport = Transport {
            stream_id,
            original_network_id,
            ..Transport::default()
        };
        let mut which = at;
        while which + 2 <= stop {
            let tag = section[which];
            let size = usize::from(section[which + 1]);
            let body = &section[which + 2..(which + 2 + size).min(stop)];
            match tag {
                SATELLITE_DELIVERY => transport.satellite = satellite(body),
                CABLE_DELIVERY => transport.cable = cable(body),
                CHANNEL_BONDING_CABLE => {
                    transport.channel_bonding = channel_bonding_cable(body);
                }
                ADVANCED_CABLE_DELIVERY => {
                    transport.advanced_cable = advanced_cable_delivery(body);
                }
                CABLE_TS_DIVISION => transport.cable_division = cable_ts_division(body),
                SI_PARAMETER => transport.si_parameters = si_parameter(body),
                SYSTEM_MANAGEMENT => transport.system = system_management(body),
                LINKAGE => {
                    if let Some(link) = crate::nitdescriptors::linkage(body) {
                        transport.links.push(link);
                    }
                }
                CA_EMM_TS => {
                    if let Some(emm) = crate::descriptors::ca_emm_ts(body) {
                        transport.emm.push(emm);
                    }
                }
                EMERGENCY_INFORMATION => {
                    transport.emergency = crate::nitdescriptors::emergency_information(body);
                }
                CONNECTED_TRANSMISSION => {
                    transport.connected = crate::nitdescriptors::connected_transmission(body);
                }
                SERVICE_GROUP => {
                    if let Some(group) = crate::nitdescriptors::service_group(body) {
                        transport.service_groups.push(group);
                    }
                }
                AREA_BROADCASTING => {
                    transport.area_broadcasting = crate::nitdescriptors::area_broadcasting(body);
                }
                // Padding, and the standard's own way of striking out a
                // descriptor that came before it. Nothing to read.
                STUFFING => {}
                TERRESTRIAL_DELIVERY => transport.terrestrial = terrestrial(body),
                SERVICE_LIST => {
                    for service in body.as_chunks::<3>().0 {
                        transport.services.push(ListedService {
                            id: (u16::from(service[0]) << 8) | u16::from(service[1]),
                            kind: service[2],
                            ..ListedService::default()
                        });
                    }
                }
                PARTIAL_RECEPTION => {
                    for service in body.as_chunks::<2>().0 {
                        transport
                            .partial_reception
                            .push((u16::from(service[0]) << 8) | u16::from(service[1]));
                    }
                }
                // The button, then how long the name is in the top six bits of
                // the byte after it.
                // A TLV-NIT has no TS information descriptor, and the tag
                // means the other thing there.
                TS_INFORMATION if !tlv && body.len() >= 2 => {
                    let length = usize::from(body[1] >> 2);
                    if 2 + length <= body.len() {
                        transport.remote_control_key_id = body[0];
                        transport.name =
                            crate::arib::decode(&body[2..2 + length]).trim().to_owned();
                        transport.transmission_types =
                            transmission_types(usize::from(body[1] & 0x03), &body[2 + length..]);
                    }
                }
                _ => {}
            }
            which += 2 + size;
        }
        read.transports.push(transport);
        at = stop;
    }
    Some(read)
}

/// The transmission type loops of a TS information descriptor: a type byte, a
/// count, and that many service identifiers, `count` times. Read as far as
/// the bytes go rather than as far as the count says.
fn transmission_types(count: usize, mut bytes: &[u8]) -> Vec<(u8, Vec<u16>)> {
    let mut found = Vec::with_capacity(count);
    for _ in 0..count {
        let [kind, services, rest @ ..] = bytes else {
            break;
        };
        let services = rest
            .as_chunks::<2>()
            .0
            .iter()
            .take(usize::from(*services))
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<u16>>();
        bytes = &rest[(services.len() * 2).min(rest.len())..];
        found.push((*kind, services));
    }
    found
}

/// The name a network gives itself, out of the descriptors describing it.
fn named(mut buffer: &[u8], tlv: bool) -> Option<String> {
    while buffer.len() >= 2 {
        let length = usize::from(buffer[1]);
        if 2 + length > buffer.len() {
            return None;
        }
        if buffer[0] == NETWORK_NAME {
            let body = &buffer[2..2 + length];
            let name = if tlv {
                String::from_utf8_lossy(body).trim().to_owned()
            } else {
                crate::arib::decode(body).trim().to_owned()
            };
            return Some(name);
        }
        buffer = &buffer[2 + length..];
    }
    None
}

/// The buttons a TLV-NIT put its services on.
///
/// A count, and then that many entries of a button, the service it selects,
/// and two bytes the standard reserved (ARIB STD-B60 5.3.1.5). **The count is
/// read as a bound rather than trusted**: a descriptor cut short by a section
/// boundary would otherwise be read past its end.
///
/// A button of nought means the service is on no button, which is how the
/// standard says "not on the remote" -- data services and the like -- and it
/// is left out rather than recorded as button nought.
fn remote_keys(mut buffer: &[u8]) -> Vec<RemoteKey> {
    const ENTRY: usize = 5;
    let mut found = Vec::new();
    while buffer.len() >= 2 {
        let length = usize::from(buffer[1]);
        if 2 + length > buffer.len() {
            break;
        }
        if buffer[0] == REMOTE_CONTROL_KEY && length >= 1 {
            let body = &buffer[2..2 + length];
            let entries = &body[1..];
            for index in 0..usize::from(body[0]) {
                let Some(entry) = entries.get(index * ENTRY..index * ENTRY + ENTRY) else {
                    break;
                };
                let service_id = (u16::from(entry[1]) << 8) | u16::from(entry[2]);
                if entry[0] == 0 || service_id == 0 {
                    continue;
                }
                found.push(RemoteKey {
                    remote_control_key_id: entry[0],
                    service_id,
                });
            }
        }
        buffer = &buffer[2 + length..];
    }
    found
}

/// Whether the network's own descriptors say it is a broadcast at all.
fn describes_system(mut buffer: &[u8]) -> Option<SystemManagement> {
    while buffer.len() >= 2 {
        let length = usize::from(buffer[1]);
        if 2 + length > buffer.len() {
            return None;
        }
        if buffer[0] == SYSTEM_MANAGEMENT {
            return system_management(&buffer[2..2 + length]);
        }
        buffer = &buffer[2 + length..];
    }
    None
}

/// The services the network's own descriptors point at.
fn describes_links(mut buffer: &[u8]) -> Vec<crate::nitdescriptors::Linkage> {
    let mut links = Vec::new();
    while buffer.len() >= 2 {
        let length = usize::from(buffer[1]);
        if 2 + length > buffer.len() {
            break;
        }
        if buffer[0] == LINKAGE
            && let Some(link) = crate::nitdescriptors::linkage(&buffer[2..2 + length])
        {
            links.push(link);
        }
        buffer = &buffer[2 + length..];
    }
    links
}

/// The descriptor that says where a transport stream is on a cable. Its
/// frequency is megahertz with the point after four digits, which works out as
/// hundreds of hertz.
pub(crate) fn cable(body: &[u8]) -> Option<Cable> {
    if body.len() < 11 {
        return None;
    }
    Some(Cable {
        // Checked, because what is being read may not be this descriptor at
        // all: a length that let something else through should give nothing
        // rather than a wrapped number.
        frequency_hz: decimal(&body[0..4], 8)?.checked_mul(100)?,
        frame_type: body[5] >> 4,
        outer_correction: body[5] & 0x0f,
        modulation: body[6],
        symbol_rate: decimal(&body[7..11], 7)?.checked_mul(100)?,
        inner_correction: body[10] & 0x0f,
    })
}

/// The descriptor that says where a terrestrial multiplex is transmitted: the
/// area in twelve bits, then four of guard interval and transmission mode, then
/// a frequency per transmitter -- two bytes each, counting sevenths of a
/// megahertz.
fn terrestrial(body: &[u8]) -> Option<Terrestrial> {
    if body.len() < 2 {
        return None;
    }
    let area_code = (u16::from(body[0]) << 4) | u16::from(body[1] >> 4);
    let carriers_hz = body[2..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|carrier| {
            let sevenths = u32::from(carrier[0]) << 8 | u32::from(carrier[1]);
            // A seventh of a megahertz, rounded the way the plan writes it:
            // 3312 sevenths is channel 13 at 473142857.
            (u64::from(sevenths) * 1_000_000 / 7) as u32
        })
        .collect();
    Some(Terrestrial {
        area_code,
        carriers_hz,
    })
}

/// The descriptor that says where a transport stream is. The frequency is
/// written as the downlink in binary-coded decimal, and a tune takes the
/// intermediate frequency, so the block's oscillator is subtracted here -- a
/// different one for each hand of polarisation, which is what puts the two
/// hands into different bands.
pub(crate) fn satellite(body: &[u8]) -> Option<Satellite> {
    if body.len() < 11 {
        return None;
    }
    let (downlink, symbol_rate) = (decimal(&body[0..4], 8)?, decimal(&body[7..11], 7)?);
    let downlink_khz = downlink.checked_mul(10)?;
    let position = decimal(&body[4..6], 4)?;
    // The polarisation is the two bits under the top one of the byte after the
    // orbital position: 0 horizontal, 1 vertical, 2 left-hand, 3 right-hand.
    let polarisation = ['H', 'V', 'L', 'R'][usize::from((body[6] >> 5) & 0x03)];
    Some(Satellite {
        downlink_khz,
        orbital_position: position as u16,
        east: body[6] & 0x80 != 0,
        polarisation,
        // The low five bits as one number, which is ARIB's reading. DVB
        // divides them differently; see Satellite::modulation.
        modulation: body[6] & 0x1f,
        // Seven digits of it, the last in the top half of the byte it shares
        // with the error correction, counting hundreds of symbols a second.
        symbol_rate: symbol_rate.saturating_mul(100),
        inner_correction: body[10] & 0x0f,
    })
}

/// A run of binary-coded decimal digits as the number it spells.
///
/// Nothing where a nibble is not a digit, which catches a descriptor that is
/// not the one it claims to be. The count of digits is given rather than taken
/// from the length: one field ends half way through its last byte, with error
/// correction in the other.
pub(crate) fn decimal(bytes: &[u8], digits: usize) -> Option<u32> {
    if digits > bytes.len() * 2 {
        return None;
    }
    let mut value = 0u32;
    for place in 0..digits {
        let byte = bytes[place / 2];
        let nibble = if place % 2 == 0 {
            byte >> 4
        } else {
            byte & 0x0f
        };
        if nibble > 9 {
            return None;
        }
        value = value * 10 + u32::from(nibble);
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    /// The descriptor a terrestrial network describes its own transmitters
    /// with. Read for what a cable cannot rewrite: a carrier received somewhere
    /// this does not name is one an operator moved.
    #[test]
    fn a_terrestrial_network_says_where_its_transmitters_are() {
        // Area 1581, a mode and guard interval, then three sites: 551.142857,
        // 557.142857 and 497.142857 MHz, each in sevenths of a megahertz.
        let body = [0x62, 0xDA, 0x0F, 0x12, 0x0F, 0x3C, 0x0D, 0x98];
        let said = super::terrestrial(&body).expect("the descriptor reads");
        assert_eq!(said.area_code, 1581);
        assert_eq!(
            said.carriers_hz,
            vec![551_142_857, 557_142_857, 497_142_857]
        );
    }

    use super::{
        AdvancedCable, Reader, advanced_cable_delivery, cable_ts_division, decimal, si_parameter,
    };
    use mpegts::crc32;
    use mpegts::{PACKET, SYNC};

    fn digits(value: u32, bytes: usize) -> Vec<u8> {
        let text = format!("{value:0width$}", width = bytes * 2);
        text.as_bytes()
            .chunks(2)
            .map(|pair| ((pair[0] - b'0') << 4) | (pair[1] - b'0'))
            .collect()
    }

    /// A table naming transport streams of one network. Two of them fit in one
    /// packet; more do not, which is what the reassembly is for.
    fn nit() -> Vec<u8> {
        nit_of(&[
            (0x4010u16, 1_172_748u32, vec![0x0065u16]),
            (0x4031, 1_176_584, vec![0x00d1, 0x00d2]),
        ])
    }

    fn nit_of(streams: &[(u16, u32, Vec<u16>)]) -> Vec<u8> {
        let mut transports = Vec::new();
        for (stream_id, downlink, services) in streams {
            let (stream_id, downlink) = (*stream_id, *downlink);
            let mut descriptors = Vec::new();
            // The satellite delivery descriptor.
            let mut body = digits(downlink, 4);
            body.extend_from_slice(&[0x01, 0x00]); // orbital position
            body.push(0b0110_0000); // right-hand circular
            body.extend_from_slice(&[0x02, 0x88, 0x60]); // 28.86 Msym/s
            body.push(0x03); // its last digit, and the coding
            descriptors.push(0x43);
            descriptors.push(body.len() as u8);
            descriptors.extend(body);
            // The service list.
            let mut list = Vec::new();
            for service in services {
                list.extend_from_slice(&service.to_be_bytes());
                list.push(0x01);
            }
            descriptors.push(0x41);
            descriptors.push(list.len() as u8);
            descriptors.extend(list);

            transports.extend_from_slice(&stream_id.to_be_bytes());
            transports.extend_from_slice(&4u16.to_be_bytes());
            transports.extend_from_slice(&(0xf000 | descriptors.len() as u16).to_be_bytes());
            transports.extend(descriptors);
        }

        let mut section = vec![0x40, 0x00, 0x00];
        section.extend_from_slice(&4u16.to_be_bytes()); // network id
        section.extend_from_slice(&[0xc1, 0x00, 0x00]);
        section.extend_from_slice(&0xf000u16.to_be_bytes()); // no network descriptors
        section.extend_from_slice(&(0xf000 | transports.len() as u16).to_be_bytes());
        section.extend(transports);
        let length = section.len() + 4 - 3;
        section[1] = 0xb0 | ((length >> 8) as u8 & 0x0f);
        section[2] = (length & 0xff) as u8;
        let check = crc32(&section);
        section.extend_from_slice(&check.to_be_bytes());
        section
    }

    /// The section, cut into packets the way a stream carries it.
    fn packets(section: &[u8]) -> Vec<u8> {
        let mut stream = Vec::new();
        let mut left = section.to_vec();
        let mut first = true;
        let mut counter = 0u8;
        while !left.is_empty() {
            let mut packet = vec![0xffu8; PACKET];
            packet[0] = SYNC;
            packet[1] = if first { 0x40 } else { 0x00 };
            packet[2] = 0x10;
            packet[3] = 0x10 | (counter & 0x0f);
            counter += 1;
            let mut at = 4;
            if first {
                packet[at] = 0;
                at += 1;
                first = false;
            }
            let room = PACKET - at;
            let take = room.min(left.len());
            packet[at..at + take].copy_from_slice(&left[..take]);
            left.drain(..take);
            stream.extend(packet);
        }
        stream
    }

    /// A service table for a multiplex, of either kind: 0x42 for the one the
    /// stream is, 0x46 for a neighbour. The name is in the broadcast character
    /// encoding, which starts in kanji, so the escape designating the
    /// alphanumeric set comes first.
    fn sdt_of(table: u8, stream_id: u16, service_id: u16, name: &str) -> Vec<u8> {
        let mut written = vec![0x1b, 0x28, 0x4a];
        written.extend_from_slice(name.as_bytes());
        let mut descriptor = vec![0x48, (3 + written.len()) as u8, 0x01, 0x00];
        descriptor.push(written.len() as u8);
        descriptor.extend(written);

        let mut services = Vec::new();
        services.extend_from_slice(&service_id.to_be_bytes());
        services.push(0xff);
        services.extend_from_slice(&(0x8000 | descriptor.len() as u16).to_be_bytes());
        services.extend(descriptor);

        let mut section = vec![table, 0x00, 0x00];
        section.extend_from_slice(&stream_id.to_be_bytes());
        section.extend_from_slice(&[0xc1, 0x00, 0x00]);
        section.extend_from_slice(&4u16.to_be_bytes()); // original network
        section.push(0xff);
        section.extend(services);
        let length = section.len() + 4 - 3;
        section[1] = 0xb0 | ((length >> 8) as u8 & 0x0f);
        section[2] = (length & 0xff) as u8;
        let check = crc32(&section);
        section.extend_from_slice(&check.to_be_bytes());
        section
    }

    /// The same cutting, on the packet identifier the service tables ride on.
    fn service_packets(section: &[u8]) -> Vec<u8> {
        let mut stream = packets(section);
        for packet in stream.as_chunks_mut::<PACKET>().0 {
            packet[2] = 0x11;
        }
        stream
    }

    /// A satellite sends a service table for every multiplex of its network, so
    /// one lock can name the whole network rather than the transponder in front
    /// of the tuner.
    #[test]
    fn the_listing_is_named_by_another_multiplexs_service_table() {
        // Either order: the two tables arrive in none in particular.
        for table_first in [true, false] {
            let mut reader = Reader::new();
            let listing = packets(&nit());
            let naming = service_packets(&sdt_of(0x46, 0x4010, 0x0065, "BS Sample 1"));
            if table_first {
                reader.feed(&listing);
                reader.feed(&naming);
            } else {
                reader.feed(&naming);
                reader.feed(&listing);
            }
            let found = reader.transports();
            assert_eq!(
                found[0].services[0].name,
                "ＢＳ\u{3000}Ｓａｍｐｌｅ\u{3000}１"
            );
            // The service the neighbour's table said nothing about stays
            // unnamed: saying nothing is right where nothing was said.
            assert_eq!(found[1].services[0].name, "");
        }
    }

    /// The stream's own table names its own multiplex the same way.
    #[test]
    fn the_listing_is_named_by_this_streams_own_service_table() {
        let mut reader = Reader::new();
        reader.feed(&packets(&nit()));
        reader.feed(&service_packets(&sdt_of(0x42, 0x4031, 0x00d1, "BS 1")));
        let found = reader.transports();
        assert_eq!(found[1].services[0].name, "ＢＳ\u{3000}１");
    }

    #[test]
    fn a_table_names_every_transport_of_the_network() {
        let mut reader = Reader::new();
        reader.feed(&packets(&nit()));
        let found = reader.transports();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].stream_id, 0x4010);
        assert_eq!(found[0].original_network_id, 4);
        // The descriptor as it was written: the downlink frequency, which no
        // tuner takes. See Transport::satellite.
        let satellite = found[0].satellite.expect("the table placed the stream");
        assert_eq!(satellite.downlink_khz, 11_727_480);
        assert!(satellite.right_hand());
        assert_eq!(satellite.symbol_rate, 28_860_000);
        assert_eq!(found[0].services.len(), 1);
        assert_eq!(found[0].services[0].id, 0x0065);
        assert_eq!(found[0].services[0].kind, 1);
        assert_eq!(found[1].services.len(), 2);
        assert_eq!(reader.networks(), &[4]);
        assert_eq!(reader.actual(), &[4]);
    }

    /// The table arrives split across packets, and a reader that only looked
    /// at whole packets would never see the second half.
    #[test]
    fn a_table_split_across_packets_is_put_back_together() {
        let many: Vec<(u16, u32, Vec<u16>)> = (0..8)
            .map(|which| {
                (
                    0x4010 + which as u16,
                    1_172_748 + which as u32 * 40,
                    vec![0x0065 + which as u16],
                )
            })
            .collect();
        let stream = packets(&nit_of(&many));
        assert!(stream.len() > PACKET, "the sample is worth splitting");
        let mut reader = Reader::new();
        for piece in stream.chunks(37) {
            reader.feed(piece);
        }
        assert_eq!(reader.transports().len(), 8);
    }

    /// A table describing a neighbour names that neighbour's transponders,
    /// which is worth having -- but the neighbour has not answered.
    #[test]
    fn a_neighbours_table_is_read_but_is_not_the_neighbour_answering() {
        let mut about_another = nit();
        about_another[0] = 0x41;
        let check = crc32(&about_another[..about_another.len() - 4]);
        let end = about_another.len() - 4;
        about_another[end..].copy_from_slice(&check.to_be_bytes());

        let mut reader = Reader::new();
        reader.feed(&packets(&about_another));
        assert_eq!(reader.transports().len(), 2, "the transponders are named");
        assert_eq!(reader.networks(), &[4]);
        assert!(reader.actual().is_empty(), "nobody answered");
    }

    /// A table that arrived damaged says nothing rather than something wrong.
    #[test]
    fn a_table_that_does_not_check_out_is_left_alone() {
        let mut section = nit();
        let last = section.len() - 1;
        section[last] ^= 0xff;
        let mut reader = Reader::new();
        reader.feed(&packets(&section));
        assert!(reader.transports().is_empty());
    }

    /// What a terrestrial or cable network says about a multiplex it does not
    /// have to say where to find: the name, the button, and which services ride
    /// in the one-segment layer.
    #[test]
    fn a_network_names_its_multiplexes_and_the_buttons_they_are_on() {
        // The name, spelled in the broadcast character encoding: the alphabet
        // moved onto the low half of the byte range, and then the letters.
        let name: &[u8] = &[0x0e, b'S', b'A', b'M', b'P', b'L', b'E'];
        // Two transmission types: the full-segment layer with two services
        // and the one-segment layer with one (TR-B14).
        let mut information = vec![0x04, (name.len() as u8) << 2 | 0x02];
        information.extend_from_slice(name);
        information
            .extend_from_slice(&[0x0f, 0x02, 0x04, 0x00, 0x04, 0x01, 0xaf, 0x01, 0x04, 0x08]);
        let mut descriptors = vec![0xcd, information.len() as u8];
        descriptors.extend_from_slice(&information);
        // One service in the partial reception layer.
        descriptors.extend_from_slice(&[0xfb, 0x02, 0x04, 0x08]);
        // Where it is on a cable: 471.1428 MHz, 5.274 Msym/s, 256QAM.
        let mut cable = digits(471, 2);
        cable.extend(digits(1428, 2));
        cable.extend_from_slice(&[0x00, 0x00, 0x05]);
        cable.extend(digits(52, 2));
        cable.extend_from_slice(&[0x74, 0x02]);
        descriptors.push(0x44);
        descriptors.push(cable.len() as u8);
        descriptors.extend(cable);

        let mut transports = Vec::new();
        transports.extend_from_slice(&0x4010u16.to_be_bytes());
        transports.extend_from_slice(&0x7fe0u16.to_be_bytes());
        transports.extend_from_slice(&(0xf000 | descriptors.len() as u16).to_be_bytes());
        transports.extend(descriptors);

        // The network's own name.
        let network_name: &[u8] = &[0x0e, b'N', b'E', b'T'];
        let mut described = vec![0x40, network_name.len() as u8];
        described.extend_from_slice(network_name);

        let mut section = vec![0x40, 0x00, 0x00];
        section.extend_from_slice(&0x7fe0u16.to_be_bytes());
        section.extend_from_slice(&[0xc1, 0x00, 0x00]);
        section.extend_from_slice(&(0xf000 | described.len() as u16).to_be_bytes());
        section.extend(described);
        section.extend_from_slice(&(0xf000 | transports.len() as u16).to_be_bytes());
        section.extend(transports);
        let length = section.len() + 4 - 3;
        section[1] = 0xb0 | ((length >> 8) as u8 & 0x0f);
        section[2] = (length & 0xff) as u8;
        let check = crc32(&section);
        section.extend_from_slice(&check.to_be_bytes());

        let mut reader = Reader::new();
        reader.feed(&packets(&section));
        // Nowhere on a satellite, so it is not something to tune from this.
        assert!(reader.transports().is_empty());
        let described = reader.described();
        assert_eq!(described.len(), 1);
        assert_eq!(described[0].name, "ＳＡＭＰＬＥ");
        assert_eq!(described[0].remote_control_key_id, 4);
        assert_eq!(
            described[0].transmission_types,
            vec![(0x0f, vec![0x0400, 0x0401]), (0xaf, vec![0x0408])]
        );
        assert_eq!(described[0].partial_reception, vec![0x0408]);
        let cable = described[0].cable.expect("the cable descriptor was read");
        assert_eq!(cable.frequency_hz, 471_142_800);
        assert_eq!(cable.symbol_rate, 5_274_000);
        assert_eq!(cable.modulation, 0x05);
        assert_eq!(reader.named().len(), 1);
        assert_eq!(reader.named()[0].name, "ＮＥＴ");
        assert!(reader.named()[0].actual);
        // One section, and it says it is the last.
        assert!(reader.complete());
    }

    #[test]
    fn binary_coded_decimal_reads_as_the_digits_it_spells() {
        assert_eq!(decimal(&[0x11, 0x72, 0x74, 0x80], 8), Some(11_727_480));
        assert_eq!(decimal(&[0x00, 0x00], 4), Some(0));
        // Seven digits of a field whose last nibble is not one.
        assert_eq!(decimal(&[0x02, 0x84, 0x41, 0x0f], 7), Some(284_410));
        // A nibble that is not a digit is a descriptor that is not this one.
        assert_eq!(decimal(&[0x1a, 0x00], 4), None);
        assert_eq!(decimal(&[0x11], 4), None);
    }

    /// The MMT/TLV services on a cable are placed by the cable multiple
    /// carrier transmission delivery system descriptor rather than by the
    /// satellite one or the ordinary cable one. Each record is eleven bytes:
    /// the frequency in BCD (MHz, four digits and four after the point), a
    /// reserved byte, frame type and outer code, modulation, the symbol rate
    /// in BCD with the inner code in its last nibble, and the group.
    #[test]
    fn channel_bonding_says_where_each_carrier_is() {
        // One carrier at 201 MHz.
        let one = super::channel_bonding_cable(&[
            0x02, 0x01, 0x00, 0x00, 0xff, 0x22, 0x05, 0x00, 0x52, 0x74, 0x0f, 0x01,
        ]);
        assert_eq!(one.len(), 1);
        let carrier = one[0];
        assert_eq!(carrier.frequency_hz, 201_000_000);
        assert_eq!(carrier.symbol_rate, 5_274_000);
        assert_eq!(carrier.modulation_name(), Some("QAM/256"));
        assert_eq!(carrier.inner_fec(), Some("NONE"));
        // The byte the frame type and the outer code share: 0x22 is frame
        // type 2 and RS(204,188), not twelve bits of reserved and a code.
        assert_eq!(carrier.frame_type, 2);
        assert_eq!(carrier.outer_correction, 2);
        assert_eq!(carrier.group_id, 1);

        // A multiplex too wide for one carrier is sent over several, and the
        // descriptor lists all of them -- with the same group on each.
        let split = super::channel_bonding_cable(&[
            0x02, 0x13, 0x00, 0x00, 0xff, 0x22, 0x05, 0x00, 0x52, 0x74, 0x0f, 0x02, 0x02, 0x19,
            0x00, 0x00, 0xff, 0x22, 0x05, 0x00, 0x52, 0x74, 0x0f, 0x02, 0x02, 0x25, 0x00, 0x00,
            0xff, 0x22, 0x05, 0x00, 0x52, 0x74, 0x0f, 0x02,
        ]);
        assert_eq!(
            split.iter().map(|c| c.frequency_hz).collect::<Vec<_>>(),
            vec![213_000_000, 219_000_000, 225_000_000]
        );
        assert!(split.iter().all(|carrier| carrier.group_id == 2));

        // A record that did not arrive whole is left off rather than
        // half-read, and one whose digits are not digits is not believed.
        assert!(super::channel_bonding_cable(&[0x02, 0x01, 0x00, 0x00, 0xff]).is_empty());
        assert!(
            super::channel_bonding_cable(&[
                0xab, 0xcd, 0x00, 0x00, 0xff, 0x22, 0x05, 0x00, 0x52, 0x74, 0x0f, 0x01,
            ])
            .is_empty()
        );
    }

    /// ARIB STD-B10 6.2.21 makes this descriptor mandatory in either the PMT or
    /// the network table, and its top two bits are the whole answer: a
    /// multiplex an operator carries for its own purposes says
    /// non-broadcasting.
    #[test]
    fn system_management_tells_broadcast_from_not() {
        let broadcast = super::system_management(&[0x03, 0x01]).expect("a broadcast descriptor");
        assert!(broadcast.broadcasting);
        assert_eq!(broadcast.broadcasting_identifier, 3);
        assert_eq!(broadcast.additional_identification, 1);

        // The two non-broadcasting values, and the undefined one -- which is
        // not a promise that it is a broadcast either.
        for flag in [0x40u8, 0x80, 0xc0] {
            let read = super::system_management(&[flag, 0x00]).expect("a descriptor");
            assert!(!read.broadcasting, "{flag:#04x} was read as a broadcast");
        }

        // Whatever followed the identifier is kept rather than dropped.
        let extra = super::system_management(&[0x03, 0x01, 0xaa, 0xbb]).expect("a descriptor");
        assert_eq!(extra.extra, vec![0xaa, 0xbb]);

        // Too short to hold the identifier at all.
        assert!(super::system_management(&[0x03]).is_none());
    }

    /// A TLV-NIT laid out the way an MMT/TLV transmission writes it.
    #[test]
    fn a_tlv_network_table_is_read_as_utf8_with_a_button_for_each_service() {
        let mut network = Vec::new();
        // The network name, which is 高度ＢＳデジタル放送 in UTF-8.
        let name = "高度ＢＳデジタル放送".as_bytes();
        network.push(super::NETWORK_NAME);
        network.push(name.len() as u8);
        network.extend_from_slice(name);
        // It says it is a broadcast.
        network.extend_from_slice(&[super::SYSTEM_MANAGEMENT, 0x02, 0x08, 0x01]);
        // Two buttons
        network.extend_from_slice(&[
            super::REMOTE_CONTROL_KEY,
            0x0b,
            0x02,
            0x01,
            0x00,
            0x11,
            0xff,
            0xff,
            0x03,
            0x00,
            0x22,
            0xff,
            0xff,
        ]);

        let mut streams = Vec::new();
        for (stream_id, service) in [(0xb010u16, 0x0011u16), (0xb020, 0x0022)] {
            let mut about = vec![super::SERVICE_LIST, 0x03];
            about.extend_from_slice(&service.to_be_bytes());
            about.push(0x01);
            about.extend_from_slice(&[
                super::SATELLITE_DELIVERY,
                0x0b,
                0x01,
                0x20,
                0x34,
                0x36,
                0x11,
                0x00,
                0xeb,
                0x03,
                0x37,
                0x56,
                0x1b,
            ]);
            streams.extend_from_slice(&stream_id.to_be_bytes());
            streams.extend_from_slice(&0x000bu16.to_be_bytes());
            streams.push(0xf0 | (about.len() >> 8) as u8);
            streams.push(about.len() as u8);
            streams.extend(about);
        }

        let mut section = vec![super::TABLE_OWN, 0x00, 0x00];
        section.extend_from_slice(&0x000bu16.to_be_bytes());
        section.extend_from_slice(&[0xcf, 0x00, 0x00]);
        section.push(0xf0 | (network.len() >> 8) as u8);
        section.push(network.len() as u8);
        section.extend(network);
        section.push(0xf0 | (streams.len() >> 8) as u8);
        section.push(streams.len() as u8);
        section.extend(streams);
        section.extend_from_slice(&[0, 0, 0, 0]);

        let read = super::described_tlv(&section).expect("a TLV network table");
        assert_eq!(read.network.id, 0x000b);
        assert!(read.network.actual);
        assert_eq!(
            read.network.name, "高度ＢＳデジタル放送",
            "the name was not read as UTF-8"
        );
        assert!(
            read.network
                .system
                .expect("a system descriptor")
                .broadcasting
        );
        assert_eq!(
            read.remote_keys,
            vec![
                super::RemoteKey {
                    remote_control_key_id: 1,
                    service_id: 0x0011
                },
                super::RemoteKey {
                    remote_control_key_id: 3,
                    service_id: 0x0022
                },
            ]
        );
        assert_eq!(read.transports.len(), 2);
        assert_eq!(read.transports[0].stream_id, 0xb010);
        assert_eq!(read.transports[0].services[0].id, 0x0011);
        let satellite = read.transports[0]
            .satellite
            .as_ref()
            .expect("a satellite descriptor");
        assert_eq!(satellite.downlink_khz, 12_034_360);
        assert_eq!(satellite.orbital_position, 1100);
        // **The other reading finds nothing here.** Tag 0xCD is the TS
        // information descriptor in a transport stream, and it is not in the
        // multiplex loop at all in this table, so no button comes out.
        let wrong = super::described(&section).expect("the same section read the other way");
        assert!(wrong.remote_keys.is_empty());
        assert!(
            wrong
                .transports
                .iter()
                .all(|transport| transport.remote_control_key_id == 0)
        );
        assert_ne!(
            wrong.network.name, "高度ＢＳデジタル放送",
            "the ARIB decoder is not what wrote this name"
        );
    }

    /// 高度ケーブル分配システム記述子。
    // ビットフィールドごとに書く。`| 0x00` は「そのフィールドが 0」の意味。
    #[allow(clippy::identity_op)]
    #[test]
    fn an_advanced_cable_descriptor_names_its_slices() {
        // 拡張タグ 0、PLP 1、448us、1/64、束ね 5、スライス一つ：
        // 中心 771 MHz、type 1、外符号 RS、256QAM、内符号 3/4。
        let body = [
            0x00,
            0x01,
            0x20 | 0x00,
            0x14,
            0x07,
            0x2d,
            0xf4,
            0x86,
            0xc0,
            0x48,
            0x14,
            0xc0,
        ];
        let read = advanced_cable_delivery(&body).expect("read");
        assert_eq!(read.plp_id, 1);
        assert_eq!(read.ofdm_symbol_duration, 1);
        assert_eq!(read.guard_interval, 0);
        assert_eq!(read.bundled_channel, 5);
        assert_eq!(read.slices.len(), 1);
        let slice = read.slices[0];
        assert_eq!(slice.data_slice_id, 7);
        assert_eq!(slice.frequency_hz, 771_000_000);
        assert_eq!(slice.frequency_type, 1);
        assert_eq!(slice.outer_code, 2);
        assert_eq!(slice.modulation, 5);
        assert_eq!(slice.inner_code, 3);
        assert_eq!(AdvancedCable::modulation_name(5), Some("QAM/256"));
        assert_eq!(
            AdvancedCable::inner_code_name(9),
            Some("高度ケーブル（L1）")
        );
    }

    /// ケーブル TS 分割システム記述子。
    #[test]
    fn a_cable_division_descriptor_names_each_carriers_services() {
        let mut body = Vec::new();
        // 471.1428 MHz、フラグ 1（付加データ無し）、フレーム 1、外符号 0、256QAM、
        // 5.274 Msym/s、内符号 15、サービス二つ。
        body.extend_from_slice(&[
            0x04, 0x71, 0x14, 0x28, 0x01, 0x10, 0x05, 0x00, 0x52, 0x74, 0x0f,
        ]);
        body.extend_from_slice(&[0x02, 0x04, 0x00, 0x04, 0x01]);
        // 二つ目：フラグ 0 で付加データ 2 バイト、サービス無し。
        body.extend_from_slice(&[
            0x04, 0x77, 0x14, 0x28, 0x00, 0x10, 0x05, 0x00, 0x52, 0x74, 0x0f,
        ]);
        body.extend_from_slice(&[0x02, 0xaa, 0xbb, 0x00]);
        let read = cable_ts_division(&body);
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].frequency_hz, 471_142_800);
        assert_eq!(read[0].symbol_rate, 5_274_000);
        assert_eq!(read[0].modulation_name(), Some("QAM/256"));
        assert_eq!(read[0].services, [0x0400, 0x0401]);
        assert_eq!(read[1].frequency_hz, 477_142_800);
        assert_eq!(read[1].extra, [0xaa, 0xbb]);
        assert!(read[1].services.is_empty());
    }

    /// SI 伝送パラメータ記述子：テーブルごとの記述をそのまま。
    #[test]
    fn si_parameters_are_kept_per_table() {
        let read = si_parameter(&[0x01, 0xe5, 0x2c, 0x4e, 0x02, 0x00, 0x0a, 0x50, 0x01, 0x03])
            .expect("read");
        assert_eq!(read.version, 1);
        assert_eq!(read.tables, [(0x4e, vec![0x00, 0x0a]), (0x50, vec![0x03])]);
    }
}

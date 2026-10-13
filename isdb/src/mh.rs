// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The tables the advanced services describe themselves with.
//!
//! They answer the same questions as the older ones -- which streams a network
//! carries, what a stream calls its services -- and are not the same tables.
//! They ride in MMT signalling rather than in transport-stream sections (see
//! `mmttlv` for the layers under them), their descriptors are tagged with two
//! bytes rather than one, and they spell text in UTF-8: putting one of these
//! names through the ARIB decoder produces plausible kana that is entirely
//! wrong.
//!
//! Three of them are read here: the network table says where the network's
//! streams are and which button each service is on, the service table names the
//! services, and the package list says which of those services this stream
//! actually carries.

use mmttlv::clock::Clock;
use mmttlv::mmtp;
use mmttlv::signalling::Signalling;
use mmttlv::tlv::{Feed, Piece};
use mpegts::sections::check;

/// The message that carries the tables about the package, the package list
/// among them.
pub const MESSAGE_PA: u16 = 0x0000;
/// The CA message, which carries the MH-CAT (STD-B60 7.2.3.3, Table 4-6).
pub use crate::mmttables::MESSAGE_CA;
/// The packet identifier the CA message travels on (STD-B60 Table 4-11).
pub const PACKET_CA: u16 = 0x0001;
/// The messages that carry a table in a transport-stream-shaped section.
pub const MESSAGE_M2_SECTION: u16 = 0x8000;
pub const MESSAGE_M2_SHORT_SECTION: u16 = 0x8002;

/// The package list table, which says what this stream holds.
const TABLE_PLT: u8 = 0x80;

/// The sections laid end to end in an M2 section message, an M2 short
/// section message or a data transmission message: each one three bytes of
/// header and then what its length field says, the message padded to its
/// length with bytes that cannot begin a section (0xFF).
///
/// **Every section is checked here**, and one that does not check out ends
/// the reading: a transport stream's sections are checked while they are
/// being assembled out of packets, but an MMT section arrives whole inside a
/// message and nothing before this point looks at its CRC. What a corrupt
/// section would do further on is put a programme with a corrupt name into
/// a guide that keeps it until the broadcast repeats.
pub fn sections(payload: &[u8]) -> Vec<&[u8]> {
    let mut found = Vec::new();
    let mut at = 0;
    while at + 3 <= payload.len() {
        // Padding. Nothing is coming after it.
        if payload[at] == 0xff {
            break;
        }
        let length = usize::from(be16(&payload[at + 1..])) & 0x0fff;
        let next = at + 3 + length;
        if next > payload.len() {
            break;
        }
        let section = &payload[at..next];
        if check(section) {
            found.push(section);
        }
        at = next;
    }
    found
}

/// The one section of a TLV-SI control packet, checked.
///
/// **The packet is not necessarily the section.** The payload holds one --
/// a section is at most 4096 bytes and a TLV packet's length field is
/// sixteen bits wide, so it never has to be split -- but the standard lets a
/// sender pad after it, and a checksum taken over the padding as well fails
/// on a section that is perfectly good. So the length the section itself
/// gives is what is read, and the rest is left.
pub fn control_section(payload: &[u8]) -> Option<&[u8]> {
    if payload.len() < 3 {
        return None;
    }
    let length = usize::from(be16(&payload[1..])) & 0x0fff;
    let section = payload.get(..3 + length)?;
    check(section).then_some(section)
}

/// The tables of a package access message, as (table_id, bytes).
///
/// The message begins with an index of the tables it carries, and the tables
/// themselves follow it, each behind the same three fields the index gave.
pub fn package_message_tables(payload: &[u8]) -> Vec<(u8, &[u8])> {
    let Some(&tables) = payload.first() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut at = 1 + 4 * usize::from(tables);
    while at + 4 <= payload.len() {
        let length = usize::from(be16(&payload[at + 2..]));
        if at + 4 + length > payload.len() {
            break;
        }
        found.push((payload[at], &payload[at..at + 4 + length]));
        at += 4 + length;
    }
    found
}
/// The network table of the network this stream belongs to.
const TABLE_TLV_NIT_OWN: u8 = 0x40;
/// The same table describing a network this stream does not belong to. A cable
/// carrying both generations sends one of these for the older services beside
/// its own, which is what lets one lock on a TLV carrier account for the
/// transport stream multiplexes on the lead.
const TABLE_TLV_NIT_OTHER: u8 = 0x41;
/// The service table of the network this stream belongs to.
const TABLE_MH_SDT_OWN: u8 = 0x9f;
/// The same table describing a neighbour's services. The network table lists
/// those streams' services without their names, and this is where the names
/// are.
const TABLE_MH_SDT_OTHER: u8 = 0xa0;

/// The descriptor that names a service, whose tag is two bytes here.
const MH_SERVICE: u16 = 0x8019;
/// The descriptor that says which button each service is on. ARIB TR-B39
/// 36.1.2.3 requires it whenever the network table is transmitted, which makes
/// it the one place an MMT/TLV service's button can be read rather than guessed at.
const REMOTE_CONTROL_KEY: u8 = 0xcd;
const NETWORK_NAME: u8 = 0x40;
const SERVICE_LIST: u8 = 0x41;
const SATELLITE_DELIVERY: u8 = 0x43;
const CABLE_DELIVERY: u8 = 0x44;
const SYSTEM_MANAGEMENT: u8 = 0xfe;
/// The cable multiple carrier transmission delivery system descriptor, which is
/// what a Japanese operator sends for the MMT/TLV services it has
/// remodulated. Not 0xF4, which is the advanced cable one and something else.
const CHANNEL_BONDING_CABLE: u8 = 0xf3;

/// One service of the network.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Service {
    pub id: u16,
    pub kind: u8,
    pub name: String,
    pub provider: String,
    /// Set where the service is under conditional access, which the field
    /// says the way the standard writes it: set means a card is needed.
    pub scrambled: bool,
}

/// One network the table describes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Network {
    pub id: u16,
    pub name: String,
    pub streams: Vec<Stream>,
    /// Which button each service is on, as identifier and key.
    pub remote_keys: Vec<(u16, u8)>,
}

/// One TLV stream of a network, which is how a receiver learns of the streams
/// it has not tuned yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stream {
    pub tlv_stream_id: u16,
    pub original_network_id: u16,
    /// The services the network says the stream carries. The network table
    /// gives identifiers and kinds only; the name is what a service table
    /// called the service, and is empty where none has yet.
    pub services: Vec<ListedService>,
    /// Where the network said the stream is carried.
    pub satellite: Option<Satellite>,
    /// Where the network said the stream is carried on a cable, using the
    /// ordinary cable descriptor.
    pub cable: Option<crate::nit::Cable>,
    /// What the Japanese operators actually send: one record per carrier, so
    /// that a multiplex split across several names all of them and says which
    /// group they belong to. See `crate::nit::channel_bonding_cable`.
    pub channel_bonding: Vec<crate::nit::ChannelBondingCarrier>,
    /// Whether the stream says it is a broadcast at all.
    pub system: Option<crate::nit::SystemManagement>,
}

/// Where on the satellite a stream is, as the network describes it. One type
/// for one descriptor: the advanced services carry the same satellite delivery
/// system descriptor the older tables do, so this is [`crate::nit::Satellite`]
/// under another name and read by the same parser.
pub use crate::nit::{ListedService, Satellite};

/// Reads the tables out of a TLV stream as it arrives.
///
/// The layers under them are peeled by [`crate::mmt`]; what this adds is
/// knowing which of the messages that come out are tables worth reading, and
/// holding what they said. The same shape as [`crate::sdt::Reader`] and
/// [`crate::nit::Reader`].
#[derive(Debug, Default)]
pub struct Reader {
    stream: Feed,
    signalling: Signalling,
    clock: Clock,
    state: State,
}

impl Reader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes what has arrived and reads what tables are in it.
    pub fn feed(&mut self, arrived: &[u8]) {
        for piece in self.stream.feed(arrived) {
            match piece {
                Piece::Control(section) => self.state.push_control(&section),
                // The transmission's clock says when a packet was sent, not
                // what the broadcast is made of.
                Piece::Ntp(_) => {}
                Piece::Mmtp {
                    packet_id,
                    payload_type,
                    timestamp,
                    at,
                    payload,
                    ..
                } => {
                    self.clock.add(packet_id, at, timestamp);
                    if payload_type != mmtp::SIGNALLING {
                        continue;
                    }
                    // A message may be spread over several packets, so what is
                    // read is what the packet completed rather than what it
                    // carried.
                    for message in self.signalling.push(packet_id, &payload) {
                        self.state.push_message(message.id, &message.payload);
                    }
                }
            }
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// How fast the stream is running, from the times its packets were sent
    /// at. Nothing until enough of them have gone past to say.
    pub fn bitrate(&self) -> Option<f64> {
        self.clock.bitrate()
    }

    /// What the stream says it is, where the tables have said.
    pub fn identity(&self) -> Option<(u16, u16)> {
        (self.state.tlv_stream_id != 0)
            .then_some((self.state.tlv_stream_id, self.state.original_network_id))
    }

    /// Every stream the network's own table named, in the shape a transport
    /// stream's network table names one.
    ///
    /// The advanced services describe their streams in a table of their own,
    /// but it answers the same question: which streams the network has and
    /// where each is. So what comes back is [`crate::nit::Transport`] and
    /// everything downstream treats a TLV network like any other.
    ///
    /// Only the streams the table placed on a satellite: one it did not say
    /// where to find cannot be tuned.
    pub fn transports(&self) -> Vec<crate::nit::Transport> {
        let mut found = Vec::new();
        for network in &self.state.networks {
            for stream in &network.streams {
                let Some(satellite) = stream.satellite else {
                    continue;
                };
                found.push(crate::nit::Transport {
                    stream_id: stream.tlv_stream_id,
                    original_network_id: stream.original_network_id,
                    // As the network wrote it. What a tune takes is worked out
                    // where the dish is known; see crate::nit::Satellite.
                    satellite: Some(satellite),
                    channel_bonding: stream.channel_bonding.clone(),
                    system: stream.system.clone(),
                    // The TLV network table carries none of these: they belong
                    // to the older tables' own vocabulary. The emergency
                    // signal of the advanced services is in the MPT instead
                    // (crate::descriptors::mh_tag::EMERGENCY_INFORMATION).
                    links: Vec::new(),
                    emergency: Vec::new(),
                    connected: None,
                    service_groups: Vec::new(),
                    area_broadcasting: Vec::new(),
                    services: stream.services.clone(),
                    name: String::new(),
                    remote_control_key_id: 0,
                    transmission_types: Vec::new(),
                    emm: Vec::new(),
                    advanced_cable: None,
                    cable_division: Vec::new(),
                    si_parameters: None,
                    partial_reception: Vec::new(),
                    cable: stream.cable,
                    terrestrial: None,
                });
            }
        }
        found
    }

    /// The services this stream carries, which is not every service the network
    /// defines: see [`State::carried`]. Where no package list has arrived, all
    /// of them are given rather than none, and the caller is told which
    /// question was answered by [`State::carried`] being empty.
    pub fn carried(&self) -> Vec<&Service> {
        if self.state.carried.is_empty() {
            return self.state.services.iter().collect();
        }
        self.state
            .services
            .iter()
            .filter(|service| self.state.carried.contains(&service.id))
            .collect()
    }
}

/// What the tables have said so far.
#[derive(Debug, Default)]
pub struct State {
    /// The network this stream belongs to, and the stream's own identifiers.
    pub network_id: u16,
    pub tlv_stream_id: u16,
    pub original_network_id: u16,
    /// The services the service table named, which is every service channel
    /// of the network rather than of this stream.
    pub services: Vec<Service>,
    /// What the package list says this stream carries, which is the answer the
    /// service table cannot give: its actual table describes the whole network
    /// -- ARIB TR-B39 34.16 says so, and a TLV carrier measured here declared
    /// seven services while carrying one. TR-B39 34.3 has the package list as
    /// the services transmitted in the concerned TLV stream, and table 5.5-1
    /// makes a service identifier and a package identifier one number. Empty
    /// where no package list arrived.
    pub carried: Vec<u16>,
    pub networks: Vec<Network>,
    /// What the service tables have called the services of every stream, this
    /// one's and its neighbours'.
    names: Vec<Naming>,
    /// Sections that did not read as tables, which is what says a sample is
    /// too damaged to believe rather than empty.
    pub invalid: u64,
}

/// What a service table called one service of some stream. A service identifier
/// is unique only within its stream, so all three numbers are the key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Naming {
    original_network_id: u16,
    stream_id: u16,
    service_id: u16,
    name: String,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the payload of a TLV control packet, which is where the network
    /// table is.
    pub fn push_control(&mut self, section: &[u8]) {
        if section.len() < 3 {
            self.invalid += 1;
            return;
        }
        let length = usize::from(be16(&section[1..])) & 0x0fff;
        if 3 + length > section.len() || !check(&section[..3 + length]) {
            self.invalid += 1;
            return;
        }
        if section[0] == TABLE_TLV_NIT_OWN || section[0] == TABLE_TLV_NIT_OTHER {
            self.network_table(&section[..3 + length]);
        }
    }

    /// Takes one signalling message, by the identifier it was sent under.
    pub fn push_message(&mut self, id: u16, payload: &[u8]) {
        if id == MESSAGE_PA {
            self.package_message(payload);
            return;
        }
        if id != MESSAGE_M2_SECTION && id != MESSAGE_M2_SHORT_SECTION {
            return;
        }
        let mut payload = payload;
        while payload.len() >= 3 {
            let length = usize::from(be16(&payload[1..])) & 0x0fff;
            let whole = 3 + length;
            if whole > payload.len() {
                self.invalid += 1;
                return;
            }
            let section = &payload[..whole];
            if !check(section) {
                self.invalid += 1;
            } else if section[0] == TABLE_MH_SDT_OWN || section[0] == TABLE_MH_SDT_OTHER {
                self.service_table(section);
            }
            payload = &payload[whole..];
        }
        if !payload.is_empty() {
            self.invalid += 1;
        }
    }

    /// Walks the tables of a package message, for the package list among them.
    /// An index of the tables may come first; it carries nothing the tables do
    /// not state again, so the tables themselves are what is walked.
    fn package_message(&mut self, payload: &[u8]) {
        if payload.is_empty() {
            self.invalid += 1;
            return;
        }
        let mut at = 1 + 4 * usize::from(payload[0]);
        while at + 4 <= payload.len() {
            let length = usize::from(be16(&payload[at + 2..]));
            if at + 4 + length > payload.len() {
                self.invalid += 1;
                return;
            }
            if payload[at] == TABLE_PLT {
                self.package_list(&payload[at..at + 4 + length]);
            }
            at += 4 + length;
        }
    }

    /// Which services this stream carries.
    fn package_list(&mut self, table: &[u8]) {
        // The table identifier, the version, the length, and how many packages
        // follow.
        if table.len() < 5 {
            self.invalid += 1;
            return;
        }
        let count = usize::from(table[4]);
        let mut at = 5;
        let mut carried = Vec::with_capacity(count);
        for _ in 0..count {
            let Some(&length) = table.get(at) else {
                self.invalid += 1;
                return;
            };
            let length = usize::from(length);
            at += 1;
            if at + length > table.len() {
                self.invalid += 1;
                return;
            }
            if length == 2 {
                carried.push(be16(&table[at..]));
            }
            at += length;
            // Where the package is. Everything a broadcast puts here is a
            // packet of this same flow; the rest of the list cannot be walked
            // past anything else.
            if table.get(at) != Some(&0x00) {
                self.invalid += 1;
                return;
            }
            at += 3;
        }
        self.carried = carried;
    }

    /// The service table, which names the network's services.
    fn service_table(&mut self, section: &[u8]) {
        // The section syntax bit is set on this one, unlike the short sections
        // the same message may carry.
        if section.len() < 15 || section[1] & 0x80 == 0 {
            self.invalid += 1;
            return;
        }
        // Only the stream's own table says what this stream is. A table
        // describing a neighbour carries that neighbour's identifiers.
        let own = section[0] == TABLE_MH_SDT_OWN;
        let stream_id = be16(&section[3..]);
        let network_id = be16(&section[8..]);
        if own {
            self.tlv_stream_id = stream_id;
            self.original_network_id = network_id;
        }
        let end = section.len() - 4;
        let mut at = 11;
        while at + 5 <= end {
            let id = be16(&section[at..]);
            let length = usize::from(be16(&section[at + 3..])) & 0x0fff;
            let next = at + 5 + length;
            if next > end {
                self.invalid += 1;
                return;
            }
            let mut service = match own {
                true => self.take(id),
                false => Service {
                    id,
                    ..Service::default()
                },
            };
            service.scrambled = section[at + 3] & 0x10 != 0;
            describe(&section[at + 5..next], &mut service);
            let name = service.name.clone();
            if own {
                self.put(service);
            }
            self.name_service(network_id, stream_id, id, name);
            at = next;
        }
    }

    /// How many services the tables have named so far, which is what says
    /// whether the naming is still going. See `nit::Reader::named`.
    pub fn namings(&self) -> usize {
        self.names.len()
    }

    /// Records what a service table called a service and puts the name onto
    /// whatever the network table has already listed. The two arrive in no
    /// order against each other.
    fn name_service(&mut self, network_id: u16, stream_id: u16, service_id: u16, name: String) {
        if name.is_empty() {
            return;
        }
        if self.names.iter().any(|known| {
            known.original_network_id == network_id
                && known.stream_id == stream_id
                && known.service_id == service_id
        }) {
            return;
        }
        for network in &mut self.networks {
            for stream in &mut network.streams {
                if stream.tlv_stream_id != stream_id || stream.original_network_id != network_id {
                    continue;
                }
                for service in &mut stream.services {
                    if service.id == service_id {
                        service.name = name.clone();
                    }
                }
            }
        }
        self.names.push(Naming {
            original_network_id: network_id,
            stream_id,
            service_id,
            name,
        });
    }

    /// Puts the names read so far onto a stream the network table has just
    /// listed.
    fn name_stream(&self, stream: &mut Stream) {
        for service in &mut stream.services {
            let found = self.names.iter().find(|naming| {
                naming.original_network_id == stream.original_network_id
                    && naming.stream_id == stream.tlv_stream_id
                    && naming.service_id == service.id
            });
            if let Some(naming) = found {
                service.name = naming.name.clone();
            }
        }
    }

    /// The network table, which says where the network's streams are.
    fn network_table(&mut self, section: &[u8]) {
        if section.len() < 16 {
            self.invalid += 1;
            return;
        }
        let id = be16(&section[3..]);
        if section[0] == TABLE_TLV_NIT_OWN {
            // Only the stream's own table says which network the stream belongs
            // to. The other-network table describes a neighbour.
            self.network_id = id;
        }
        let mut network = match self.networks.iter().position(|known| known.id == id) {
            Some(at) => self.networks.remove(at),
            None => Network {
                id,
                ..Network::default()
            },
        };
        let end = section.len() - 4;
        let mut at = 8;
        let described = usize::from(be16(&section[at..])) & 0x0fff;
        if at + 2 + described <= end {
            self.about_network(&section[at + 2..at + 2 + described], &mut network);
        }
        at += 2 + described;
        if at + 2 > end {
            self.invalid += 1;
            self.networks.push(network);
            return;
        }
        let loop_length = usize::from(be16(&section[at..])) & 0x0fff;
        at += 2;
        let stop = (at + loop_length).min(end);
        while at + 6 <= stop {
            let tlv_stream_id = be16(&section[at..]);
            let original_network_id = be16(&section[at + 2..]);
            let length = usize::from(be16(&section[at + 4..])) & 0x0fff;
            let next = at + 6 + length;
            if next > stop {
                self.invalid += 1;
                break;
            }
            // Which of these is this stream, the table does not say: a cable
            // that carries them one to a carrier sends the same table on every
            // one. Only the service table names the stream it came in on.
            let mut stream = Stream {
                tlv_stream_id,
                original_network_id,
                ..Stream::default()
            };
            self.about_stream(&section[at + 6..next], &mut stream);
            self.name_stream(&mut stream);
            // A table is sent again for as long as the stream runs, so a
            // stream already described is left as it was.
            if !network
                .streams
                .iter()
                .any(|known| known.tlv_stream_id == stream.tlv_stream_id)
            {
                network.streams.push(stream);
            }
            at = next;
        }
        self.networks.push(network);
    }

    /// The descriptors describing the network itself.
    fn about_network(&mut self, mut buffer: &[u8], network: &mut Network) {
        while buffer.len() >= 2 {
            let length = usize::from(buffer[1]);
            if 2 + length > buffer.len() {
                return;
            }
            let body = &buffer[2..2 + length];
            match buffer[0] {
                REMOTE_CONTROL_KEY => remote_keys(body, network),
                NETWORK_NAME if network.name.is_empty() => {
                    // UTF-8, not the broadcast character encoding: see the
                    // module's own note.
                    if let Ok(name) = std::str::from_utf8(body) {
                        network.name = name.trim().to_owned();
                    }
                }
                _ => {}
            }
            buffer = &buffer[2 + length..];
        }
    }

    /// The descriptors describing one stream of the network.
    fn about_stream(&mut self, mut buffer: &[u8], stream: &mut Stream) {
        while buffer.len() >= 2 {
            let length = usize::from(buffer[1]);
            if 2 + length > buffer.len() {
                return;
            }
            let body = &buffer[2..2 + length];
            match buffer[0] {
                SERVICE_LIST => {
                    for listed in body.as_chunks::<3>().0 {
                        let id = be16(listed);
                        stream.services.push(ListedService {
                            id,
                            kind: listed[2],
                            ..ListedService::default()
                        });
                        // The network's word about a service the service table
                        // may not have got to yet.
                        let mut service = self.take(id);
                        service.kind = listed[2];
                        self.put(service);
                    }
                }
                SATELLITE_DELIVERY => stream.satellite = crate::nit::satellite(body),
                CABLE_DELIVERY => stream.cable = crate::nit::cable(body),
                CHANNEL_BONDING_CABLE => {
                    stream.channel_bonding = crate::nit::channel_bonding_cable(body);
                }
                SYSTEM_MANAGEMENT => stream.system = crate::nit::system_management(body),
                _ => {}
            }
            buffer = &buffer[2 + length..];
        }
    }

    /// The service of this identifier as it stands, or a new one.
    fn take(&mut self, id: u16) -> Service {
        match self.services.iter().position(|known| known.id == id) {
            Some(at) => self.services.remove(at),
            None => Service {
                id,
                ..Service::default()
            },
        }
    }

    fn put(&mut self, service: Service) {
        let at = self
            .services
            .iter()
            .position(|known| known.id > service.id)
            .unwrap_or(self.services.len());
        self.services.insert(at, service);
    }
}

/// The descriptors of one service, for the one that names it. A tag here is two
/// bytes, and how many bytes the length takes depends on which range the tag is
/// in: a reader that assumed one of each would walk into the middle of a
/// descriptor.
fn describe(mut buffer: &[u8], service: &mut Service) {
    while buffer.len() >= 3 {
        let tag = be16(buffer);
        let width = length_width(tag);
        if buffer.len() < 2 + width {
            return;
        }
        let mut length = 0usize;
        for byte in &buffer[2..2 + width] {
            length = (length << 8) | usize::from(*byte);
        }
        let start = 2 + width;
        // Checked: a four-byte length on a 32-bit system can carry the sum
        // round to something small, and the walk would never end.
        let Some(next) = start
            .checked_add(length)
            .filter(|next| *next <= buffer.len())
        else {
            return;
        };
        if tag == MH_SERVICE {
            names(&buffer[start..next], service);
        }
        buffer = &buffer[next..];
    }
}

/// How many bytes a descriptor of this tag writes its length in.
fn length_width(tag: u16) -> usize {
    match tag {
        0x4000..=0x6fff => 2,
        0x7000..=0x7fff => 4,
        0xf000.. => 2,
        _ => 1,
    }
}

/// What a service is and what it is called.
fn names(body: &[u8], service: &mut Service) {
    if body.len() < 3 {
        return;
    }
    service.kind = body[0];
    let provider = usize::from(body[1]);
    if 2 + provider >= body.len() {
        return;
    }
    let name = usize::from(body[2 + provider]);
    let from = 3 + provider;
    if from + name > body.len() {
        return;
    }
    // UTF-8, as everything in these tables is.
    if let Ok(provider) = std::str::from_utf8(&body[2..2 + provider]) {
        service.provider = provider.trim().to_owned();
    }
    if let Ok(name) = std::str::from_utf8(&body[from..from + name]) {
        service.name = name.trim().to_owned();
    }
}

/// Which button each service is on: a count, and then that many of a key, the
/// service it belongs to, and two bytes kept for later -- ARIB STD-B60 5.3.1.5.
/// The count is read as a bound rather than trusted: what is there is what the
/// length says.
fn remote_keys(body: &[u8], network: &mut Network) {
    const ENTRY: usize = 5;
    let Some((count, entries)) = body.split_first() else {
        return;
    };
    for index in 0..usize::from(*count) {
        let Some(entry) = entries.get(index * ENTRY..index * ENTRY + 3) else {
            return;
        };
        let (key, id) = (entry[0], be16(&entry[1..]));
        if key == 0 || id == 0 {
            continue;
        }
        if !network.remote_keys.iter().any(|(known, _)| *known == id) {
            network.remote_keys.push((id, key));
        }
    }
}

fn be16(bytes: &[u8]) -> u16 {
    (u16::from(bytes[0]) << 8) | u16::from(bytes[1])
}

#[cfg(test)]
mod tests {
    use super::{MESSAGE_M2_SECTION, MESSAGE_PA, Reader, State};
    use mpegts::crc32;

    /// The CA message is told apart by its message identifier, 0x8001
    /// (STD-B60 Table 4-6); 0x0001 is the packet identifier it travels on
    /// (Table 4-11), and as a message identifier an MPI message.
    #[test]
    fn the_ca_message_is_named_by_its_message_identifier() {
        assert_eq!(super::MESSAGE_CA, 0x8001);
        assert_eq!(super::PACKET_CA, 0x0001);
    }

    /// A section with its length filled in and its check appended, which is
    /// what these tables arrive as.
    fn section(table: u8, body: &[u8]) -> Vec<u8> {
        let mut section = vec![table, 0x00, 0x00];
        section.extend_from_slice(body);
        let length = section.len() + 4 - 3;
        section[1] = 0xb0 | ((length >> 8) as u8 & 0x0f);
        section[2] = (length & 0xff) as u8;
        let check = crc32(&section);
        section.extend_from_slice(&check.to_be_bytes());
        section
    }

    /// The service table names the services and says whether a card is needed
    /// for them. The names are UTF-8 here rather than the older encoding.
    #[test]
    fn the_service_table_names_the_services() {
        let name = "サンプル4K".as_bytes();
        let provider = "サンプル放送".as_bytes();
        let mut descriptor = vec![0x80, 0x19];
        descriptor.push((3 + provider.len() + name.len()) as u8);
        descriptor.extend_from_slice(&[0x01, provider.len() as u8]);
        descriptor.extend_from_slice(provider);
        descriptor.push(name.len() as u8);
        descriptor.extend_from_slice(name);
        let mut body = vec![0x40, 0x01, 0xc1, 0x00, 0x00, 0x00, 0x0b, 0xff];
        body.extend_from_slice(&[0x40, 0x02, 0x00]);
        body.push(0x90 | ((descriptor.len() >> 8) as u8 & 0x0f));
        body.push((descriptor.len() & 0xff) as u8);
        body.extend_from_slice(&descriptor);
        let mut state = State::new();
        state.push_message(MESSAGE_M2_SECTION, &section(0x9f, &body));
        assert_eq!(state.invalid, 0);
        assert_eq!(state.tlv_stream_id, 0x4001);
        assert_eq!(state.original_network_id, 0x000b);
        assert_eq!(state.services.len(), 1);
        assert_eq!(state.services[0].id, 0x4002);
        assert_eq!(state.services[0].name, "サンプル4K");
        assert_eq!(state.services[0].provider, "サンプル放送");
        assert_eq!(state.services[0].kind, 0x01);
        assert!(state.services[0].scrambled);
    }

    /// The network table says which streams the network has and which button
    /// each of its services is on.
    #[test]
    fn the_network_table_says_where_the_streams_are_and_what_button_they_are_on() {
        let name = "サンプルネット".as_bytes();
        let mut described = vec![0x40, name.len() as u8];
        described.extend_from_slice(name);
        // One button: the count, the key, the service, and two bytes kept for
        // later.
        described.extend_from_slice(&[0xcd, 0x06, 0x01, 0x05, 0x40, 0x02, 0xff, 0xff]);
        let mut streams = vec![0x40, 0x01, 0x00, 0x0b];
        let listed = [0x41u8, 0x03, 0x40, 0x02, 0x01];
        streams.push(0xf0 | ((listed.len() >> 8) as u8 & 0x0f));
        streams.push((listed.len() & 0xff) as u8);
        streams.extend_from_slice(&listed);
        let mut body = vec![0x00, 0x0b, 0xc1, 0x00, 0x00];
        body.push(0xf0 | ((described.len() >> 8) as u8 & 0x0f));
        body.push((described.len() & 0xff) as u8);
        body.extend_from_slice(&described);
        body.push(0xf0 | ((streams.len() >> 8) as u8 & 0x0f));
        body.push((streams.len() & 0xff) as u8);
        body.extend_from_slice(&streams);
        let mut state = State::new();
        state.push_control(&section(0x40, &body));
        assert_eq!(state.invalid, 0);
        assert_eq!(state.network_id, 0x000b);
        assert_eq!(state.networks.len(), 1);
        assert_eq!(state.networks[0].name, "サンプルネット");
        assert_eq!(state.networks[0].remote_keys, vec![(0x4002, 5)]);
        assert_eq!(state.networks[0].streams.len(), 1);
        assert_eq!(state.networks[0].streams[0].tlv_stream_id, 0x4001);
        assert_eq!(state.networks[0].streams[0].services.len(), 1);
        assert_eq!(state.networks[0].streams[0].services[0].id, 0x4002);
        assert_eq!(state.networks[0].streams[0].services[0].kind, 0x01);
        // The table says nothing about which of its streams this is: a walk
        // that took the first as its own would call every stream by that one's
        // identifier.
        assert_eq!(state.tlv_stream_id, 0);
    }

    /// A cable carrying both generations sends the network table twice: once
    /// for the network the stream belongs to and once, under the other table
    /// identifier, for a neighbour. Reading only the first loses the older
    /// multiplexes sharing the lead.
    #[test]
    fn the_other_networks_table_is_read_without_joining_that_network() {
        // The older network's entry as one feed writes it: the service list
        // and the ordinary cable descriptor at 279 MHz, 64-QAM.
        let described = [
            0x41u8, 0x03, 0x03, 0xa1, 0xa4, 0x44, 0x0b, 0x02, 0x79, 0x00, 0x00, 0xff, 0x12, 0x03,
            0x00, 0x52, 0x74, 0x0f,
        ];
        let mut streams = vec![0xb0, 0xf1, 0x00, 0x04];
        streams.push(0xf0 | ((described.len() >> 8) as u8 & 0x0f));
        streams.push((described.len() & 0xff) as u8);
        streams.extend_from_slice(&described);
        let mut body = vec![0x00, 0x04, 0xc1, 0x00, 0x00, 0xf0, 0x00];
        body.push(0xf0 | ((streams.len() >> 8) as u8 & 0x0f));
        body.push((streams.len() & 0xff) as u8);
        body.extend_from_slice(&streams);
        let mut state = State::new();
        state.push_control(&section(0x41, &body));

        assert_eq!(state.invalid, 0);
        assert_eq!(state.networks.len(), 1);
        let network = &state.networks[0];
        assert_eq!(network.id, 4);
        assert_eq!(network.streams.len(), 1);
        let stream = &network.streams[0];
        assert_eq!(stream.tlv_stream_id, 0xb0f1);
        let cable = stream.cable.expect("the other network placed its stream");
        assert_eq!(cable.frequency_hz, 279_000_000);
        assert_eq!(cable.modulation_name(), Some("QAM/64"));
        // The stream belongs to its own network, not to the one it was merely
        // told about.
        assert_ne!(state.network_id, 4);
    }

    /// The advanced services send a service table for every stream of the
    /// network, marked as another stream's. Reading those is what lets one lock
    /// name the others.
    #[test]
    fn the_listing_is_named_by_another_streams_service_table() {
        let mut streams = vec![0xb0, 0x50, 0x00, 0x0b];
        let listed = [0x41u8, 0x03, 0x00, 0x97, 0x01];
        streams.push(0xf0 | ((listed.len() >> 8) as u8 & 0x0f));
        streams.push((listed.len() & 0xff) as u8);
        streams.extend_from_slice(&listed);
        let mut body = vec![0x00, 0x0b, 0xc1, 0x00, 0x00, 0xf0, 0x00];
        body.push(0xf0 | ((streams.len() >> 8) as u8 & 0x0f));
        body.push((streams.len() & 0xff) as u8);
        body.extend_from_slice(&streams);
        let listing = section(0x40, &body);

        // The service table for that stream, which this one is not. TLV-SI
        // writes its names as UTF-8.
        let name = "サンプル　４Ｋ".as_bytes();
        let mut descriptor = vec![0x80, 0x19];
        descriptor.push((3 + name.len()) as u8);
        descriptor.extend_from_slice(&[0x01, 0x00]);
        descriptor.push(name.len() as u8);
        descriptor.extend_from_slice(name);
        let mut services = vec![0x00, 0x97, 0xfc];
        services.push(0x80 | ((descriptor.len() >> 8) as u8 & 0x0f));
        services.push((descriptor.len() & 0xff) as u8);
        services.extend_from_slice(&descriptor);
        let mut body = vec![0xb0, 0x50, 0xc1, 0x00, 0x00, 0x00, 0x0b, 0xff];
        body.extend_from_slice(&services);
        let naming = section(0xa0, &body);

        for listing_first in [true, false] {
            let mut state = State::new();
            if listing_first {
                state.push_control(&listing);
                state.push_message(MESSAGE_M2_SECTION, &naming);
            } else {
                state.push_message(MESSAGE_M2_SECTION, &naming);
                state.push_control(&listing);
            }
            assert_eq!(state.invalid, 0);
            let stream = &state.networks[0].streams[0];
            assert_eq!(stream.services[0].name, "サンプル　４Ｋ");
            // The stream is not the one the neighbour's table described, and
            // the neighbour's services are not this stream's.
            assert_ne!(state.tlv_stream_id, 0xb050);
            assert!(state.services.iter().all(|service| service.name.is_empty()));
        }
    }

    /// A network that placed its streams on a satellite hands them over as
    /// tunable transponders, which is what a walk of the satellite plan takes
    /// away from one lock.
    #[test]
    fn the_streams_a_network_placed_are_transponders_a_receiver_can_tune() {
        // The satellite descriptor as the tables write it: the downlink in tens
        // of kilohertz as binary-coded decimal (11.84256 GHz), the orbital
        // position (110.0 east), the hemisphere and hand sharing a byte with
        // the modulation, and the symbol rate with the error correction in its
        // last nibble (33.7561 Msymbols).
        let satellite = [
            0x43u8, 0x0b, 0x01, 0x18, 0x42, 0x56, 0x11, 0x00, 0xeb, 0x03, 0x37, 0x56, 0x13,
        ];
        let mut streams = vec![0x40, 0x01, 0x00, 0x0b];
        streams.push(0xf0 | ((satellite.len() >> 8) as u8 & 0x0f));
        streams.push((satellite.len() & 0xff) as u8);
        streams.extend_from_slice(&satellite);
        let mut body = vec![0x00, 0x0b, 0xc1, 0x00, 0x00, 0xf0, 0x00];
        body.push(0xf0 | ((streams.len() >> 8) as u8 & 0x0f));
        body.push((streams.len() & 0xff) as u8);
        body.extend_from_slice(&streams);
        let mut reader = Reader::new();
        reader.state.push_control(&section(0x40, &body));

        let transports = reader.transports();
        assert_eq!(transports.len(), 1);
        let stream = &transports[0];
        assert_eq!(stream.stream_id, 0x4001);
        assert_eq!(stream.original_network_id, 0x000b);
        // The descriptor as it was written: the downlink, not what a tune is
        // given. Which oscillator comes off it is a fact about the dish.
        let satellite = stream.satellite.expect("the table placed the stream");
        assert_eq!(satellite.downlink_khz, 11_842_560);
        assert!(satellite.right_hand());
        assert_eq!(satellite.symbol_rate, 33_756_100);
    }

    /// The package list is the one table that says what this stream carries,
    /// as against what the network defines.
    #[test]
    fn the_package_list_says_what_this_stream_carries() {
        // No index of tables, then the package list: table, version, length,
        // one package of two bytes and where to find it.
        let table = [
            0x80u8, 0x01, 0x00, 0x08, 0x01, 0x02, 0x40, 0x02, 0x00, 0x00, 0x00, 0x00,
        ];
        let mut payload = vec![0x00];
        payload.extend_from_slice(&table);
        let mut state = State::new();
        state.push_message(MESSAGE_PA, &payload);
        assert_eq!(state.invalid, 0);
        assert_eq!(state.carried, vec![0x4002]);
    }

    /// A TLV packet: the sync byte, what it carries, and the length of what
    /// follows.
    fn tlv(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0x7f, kind];
        packet.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        packet.extend_from_slice(payload);
        packet
    }

    /// The tables arrive as a stream, a chunk at a time, and a chunk does not
    /// end where a packet does.
    #[test]
    fn the_tables_are_read_out_of_a_stream_that_arrives_in_chunks() {
        // The network table, in a control packet.
        let mut network = vec![0x00, 0x0b, 0xc1, 0x00, 0x00];
        network.extend_from_slice(&[0xf0, 0x00, 0xf0, 0x00]);
        let mut bytes = tlv(0xfe, &section(0x40, &network));
        // The package list, in a signalling message inside an MMTP packet
        // inside a UDP datagram inside an IPv4 packet.
        let table = [
            0x80u8, 0x01, 0x00, 0x08, 0x01, 0x02, 0x40, 0x02, 0x00, 0x00, 0x00, 0x00,
        ];
        let mut package = vec![0x00];
        package.extend_from_slice(&table);
        let mut message = vec![0x00, 0x00, 0x01];
        message.extend_from_slice(&(package.len() as u32).to_be_bytes());
        message.extend_from_slice(&package);
        let mut signalling = vec![0x00, 0x00];
        signalling.extend_from_slice(&message);
        let mut mmtp = vec![0x00, 0x02, 0x00, 0x11];
        mmtp.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        mmtp.extend_from_slice(&[0; 4]);
        mmtp.extend_from_slice(&signalling);
        let mut udp = vec![0x12, 0x34, 0x30, 0x39];
        udp.extend_from_slice(&((8 + mmtp.len()) as u16).to_be_bytes());
        udp.extend_from_slice(&[0x00, 0x00]);
        udp.extend_from_slice(&mmtp);
        let mut ip = vec![0x45, 0x00];
        ip.extend_from_slice(&((20 + udp.len()) as u16).to_be_bytes());
        ip.extend_from_slice(&[0, 0, 0, 0, 64, 17, 0, 0]);
        ip.extend_from_slice(&[10, 0, 0, 1]);
        ip.extend_from_slice(&[239, 0, 0, 1]);
        ip.extend_from_slice(&udp);
        bytes.extend(tlv(0x01, &ip));

        // Fed in pieces that end nowhere in particular.
        let mut reader = Reader::new();
        for chunk in bytes.chunks(7) {
            reader.feed(chunk);
        }
        assert_eq!(reader.state().invalid, 0);
        assert_eq!(reader.state().network_id, 0x000b);
        assert_eq!(reader.state().carried, vec![0x4002]);
    }

    /// A section that does not check out is not read. A table taken from
    /// damaged bytes says something that was never sent.
    #[test]
    fn a_section_that_does_not_check_out_is_left_alone() {
        let mut damaged = section(
            0x40,
            &[0x00, 0x0b, 0xc1, 0x00, 0x00, 0xf0, 0x00, 0xf0, 0x00],
        );
        let last = damaged.len() - 1;
        damaged[last] ^= 0xff;
        let mut state = State::new();
        state.push_control(&damaged);
        assert_eq!(state.invalid, 1);
        assert!(state.networks.is_empty());
    }
}

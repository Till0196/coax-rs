// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The MMT-SI counterparts of the tables in [`crate::tstables`], and the tables
//! that exist only here.
//!
//! The package table says what a broadcast is made of, and the address map says
//! which datagrams belong to which service. Neither is acted on: they are
//! parsed so that a caller holding a capture can say what is in one. STD-B60
//! Table 4-8 assigns the identifiers.

use crate::tstables::{BroadcastTime, be16, be32, broadcast_time, duration, long_section};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub const TABLE_MPT_SUBSET_FIRST: u8 = 0x11;
pub const TABLE_MPT_COMPLETE: u8 = 0x20;
pub const TABLE_MH_ECM_FIRST: u8 = 0x82;
pub const TABLE_MH_ECM_LAST: u8 = 0x83;
pub const TABLE_MH_EMM_FIRST: u8 = 0x84;
pub const TABLE_MH_EMM_LAST: u8 = 0x85;
pub const TABLE_MH_CAT: u8 = 0x86;
/// PLT: the package list, in the PA message beside the MPT.
pub const TABLE_PLT: u8 = 0x80;
/// LCT: the layout configuration (multi-view regions).
pub const TABLE_LCT: u8 = 0x81;
/// MH-SDTT: software download announcements.
pub const TABLE_MH_SDTT: u8 = 0x9e;
/// MH-CDT: common data (logos).
pub const TABLE_MH_CDT: u8 = 0xa2;
pub const TABLE_MH_EIT_PRESENT: u8 = 0x8b;
pub const TABLE_MH_EIT_SCHEDULE_FIRST: u8 = 0x8c;
pub const TABLE_MH_EIT_SCHEDULE_LAST: u8 = 0x9b;
pub const TABLE_MH_AIT: u8 = 0x9c;
/// The event message table (ARIB STD-B60 11.2.1.1): how a broadcast tells a
/// running application that something happened, or tells it the NPT.
pub const TABLE_EMT: u8 = 0xa6;
/// The event message descriptor (11.2.2.2), which has a 16 bit length.
pub const DESCRIPTOR_EVENT_MESSAGE: u16 = 0xf003;
/// The UTC-NPT reference descriptor (11.2.2.1).
pub const DESCRIPTOR_UTC_NPT_REFERENCE: u16 = 0x8021;
pub const TABLE_MH_BIT: u8 = 0x9d;
pub const TABLE_MH_SDT_OWN: u8 = 0x9f;
pub const TABLE_MH_SDT_OTHER: u8 = 0xa0;
pub const TABLE_MH_TOT: u8 = 0xa1;
pub const TABLE_AMT: u8 = 0xfe;

/// The message that carries the data transmission tables.
pub const MESSAGE_DATA: u16 = 0x8003;
/// The message that carries the entitlement messages.
pub const MESSAGE_CA: u16 = 0x8001;

/// One descriptor as MMT-SI writes it: a two-byte tag, and a length whose own
/// width depends on the tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Descriptor {
    pub tag: u16,
    pub body: Vec<u8>,
}

/// How many bytes the length field of a descriptor with this tag takes.
fn length_bytes(tag: u16) -> usize {
    match tag {
        0x4000..=0x6fff => 2,
        0x7000..=0x7fff => 4,
        tag if tag >= 0xf000 => 2,
        _ => 1,
    }
}

/// Reads an MMT-SI descriptor loop.
pub fn descriptors(buffer: &[u8]) -> Vec<Descriptor> {
    let mut described = Vec::new();
    let mut at = 0;
    while at + 3 <= buffer.len() {
        let tag = be16(&buffer[at..]);
        let width = length_bytes(tag);
        if at + 2 + width > buffer.len() {
            break;
        }
        let mut length = 0usize;
        for value in &buffer[at + 2..at + 2 + width] {
            length = length << 8 | usize::from(*value);
        }
        let start = at + 2 + width;
        // Checked: a four-byte length on a 32-bit system can carry the sum
        // round to before the start.
        let Some(next) = start
            .checked_add(length)
            .filter(|next| *next <= buffer.len())
        else {
            break;
        };
        described.push(Descriptor {
            tag,
            body: buffer[start..next].to_vec(),
        });
        at = next;
    }
    described
}

/// The descriptor with this tag, or nothing.
pub fn find(described: &[Descriptor], tag: u16) -> Option<&Descriptor> {
    described.iter().find(|one| one.tag == tag)
}

/// The MPT: the assets a package is made of and where each of them is carried.
/// It arrives inside a PA message rather than as a section, so it has no check
/// of its own and is parsed from the table bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageTable {
    pub table_id: u8,
    pub version: u8,
    pub mode: u8,
    pub package_id: Vec<u8>,
    pub descriptors: Vec<Descriptor>,
    pub assets: Vec<Asset>,
}

impl PackageTable {
    /// The package identifier's last two bytes, which is where a broadcast
    /// writes the service the package is.
    pub fn service_id(&self) -> u16 {
        if self.package_id.len() < 2 {
            return 0;
        }
        be16(&self.package_id[self.package_id.len() - 2..])
    }
}

/// One component of a package: a video stream, an audio stream, a subtitle
/// track or a data carousel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Asset {
    pub identifier_type: u8,
    pub id_scheme: u32,
    pub id: Vec<u8>,
    /// The four characters that say what the asset is: `hev1`, `mp4a`, `stpp`
    /// and the rest.
    pub kind: String,
    pub clock_relation: bool,
    pub locations: Vec<AssetLocation>,
    pub descriptors: Vec<Descriptor>,
}

impl Asset {
    /// Where the asset is in this stream, when it is in this stream at all:
    /// location type 0 is the one that means here.
    pub fn packet_id(&self) -> Option<u16> {
        self.locations
            .iter()
            .find(|location| location.kind == 0x00)
            .map(|location| location.packet_id)
    }
}

/// One place an asset is carried, which need not be this stream: MMT can point
/// at another packet identifier, another broadcast, or a URL. The bytes are
/// kept whole because only some kinds have a packet identifier to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetLocation {
    pub kind: u8,
    pub packet_id: u16,
    pub raw: Vec<u8>,
}

/// Reads an MP table, complete or subset.
pub fn package_table(table: &[u8]) -> Option<PackageTable> {
    if table.len() < 6 || !(TABLE_MPT_SUBSET_FIRST..=TABLE_MPT_COMPLETE).contains(&table[0]) {
        return None;
    }
    let length = usize::from(be16(&table[2..]));
    if 4 + length > table.len() {
        return None;
    }
    let body = &table[4..4 + length];
    if body.len() < 2 {
        return None;
    }
    let mut described = PackageTable {
        table_id: table[0],
        version: table[1],
        mode: body[0] & 0x03,
        ..PackageTable::default()
    };
    let mut at = 1;
    let identifier = usize::from(body[at]);
    at += 1;
    if at + identifier + 2 > body.len() {
        return None;
    }
    described.package_id = body[at..at + identifier].to_vec();
    at += identifier;
    let descriptors_length = usize::from(be16(&body[at..]));
    at += 2;
    if at + descriptors_length + 1 > body.len() {
        return None;
    }
    described.descriptors = descriptors(&body[at..at + descriptors_length]);
    at += descriptors_length;
    let count = usize::from(body[at]);
    at += 1;
    for _ in 0..count {
        let Some((asset, read)) = asset(&body[at..]) else {
            return Some(described);
        };
        described.assets.push(asset);
        at += read;
    }
    Some(described)
}

fn asset(buffer: &[u8]) -> Option<(Asset, usize)> {
    if buffer.len() < 11 {
        return None;
    }
    let mut asset = Asset {
        identifier_type: buffer[0],
        id_scheme: be32(&buffer[1..]),
        ..Asset::default()
    };
    let mut at = 5;
    let identifier = usize::from(buffer[at]);
    at += 1;
    if at + identifier + 6 > buffer.len() {
        return None;
    }
    asset.id = buffer[at..at + identifier].to_vec();
    at += identifier;
    asset.kind = String::from_utf8_lossy(&buffer[at..at + 4]).into_owned();
    at += 4;
    asset.clock_relation = buffer[at] & 0x01 != 0;
    at += 1;
    if asset.clock_relation {
        // The timescale follows, and a flag saying whether an offset follows
        // it. Neither says where the asset is, so both are stepped over.
        if at + 2 > buffer.len() {
            return None;
        }
        let extended = buffer[at + 1] & 0x01 != 0;
        at += 2;
        if extended {
            if at + 4 > buffer.len() {
                return None;
            }
            at += 4;
        }
    }
    if at >= buffer.len() {
        return None;
    }
    let locations = usize::from(buffer[at]);
    at += 1;
    for _ in 0..locations {
        let (location, read) = asset_location(&buffer[at..])?;
        asset.locations.push(location);
        at += read;
    }
    if at + 2 > buffer.len() {
        return None;
    }
    let descriptors_length = usize::from(be16(&buffer[at..]));
    at += 2;
    if at + descriptors_length > buffer.len() {
        return None;
    }
    asset.descriptors = descriptors(&buffer[at..at + descriptors_length]);
    Some((asset, at + descriptors_length))
}

fn asset_location(buffer: &[u8]) -> Option<(AssetLocation, usize)> {
    let size = match *buffer.first()? {
        0x00 => 1 + 2,               // this stream
        0x01 => 1 + 4 + 4 + 2 + 2,   // another broadcast, over IPv4
        0x02 => 1 + 16 + 16 + 2 + 2, // another broadcast, over IPv6
        0x03 => 1 + 2 + 2 + 2,       // another network, by its identifiers
        0x04 => 1 + 16 + 16 + 2 + 2, // another broadcast, with a flow label
        // A URL, whose length is the byte after the type.
        0x05 => 2 + usize::from(*buffer.get(1)?),
        _ => return None,
    };
    if buffer.len() < size {
        return None;
    }
    let mut location = AssetLocation {
        kind: buffer[0],
        packet_id: 0,
        raw: buffer[..size].to_vec(),
    };
    match buffer[0] {
        0x00 => location.packet_id = be16(&buffer[1..]),
        0x01 | 0x02 | 0x04 => location.packet_id = be16(&buffer[size - 2..]),
        _ => {}
    }
    Some((location, size))
}

/// An MH-SDT, the TLV counterpart of the SDT. Like it, the table comes in a
/// kind for this stream and a kind for a neighbour.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceDescription {
    pub table_id: u8,
    pub tlv_stream_id: u16,
    pub original_network_id: u16,
    pub version: u8,
    pub services: Vec<DescribedService>,
}

impl ServiceDescription {
    /// Whether the table describes the stream that carried it.
    pub fn own(&self) -> bool {
        self.table_id == TABLE_MH_SDT_OWN
    }
}

/// One service of an MH-SDT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescribedService {
    pub service_id: u16,
    pub eit_user_defined: u8,
    pub eit_schedule: bool,
    pub eit_present_following: bool,
    pub running_status: u8,
    pub scrambled: bool,
    pub descriptors: Vec<Descriptor>,
}

/// Reads an MH-SDT section of either kind.
pub fn service_description(section: &[u8]) -> Option<ServiceDescription> {
    let header = long_section(section)?;
    if (header.table_id != TABLE_MH_SDT_OWN && header.table_id != TABLE_MH_SDT_OTHER)
        || header.body.len() < 3
    {
        return None;
    }
    let body = header.body;
    let mut table = ServiceDescription {
        table_id: header.table_id,
        tlv_stream_id: header.extension,
        original_network_id: be16(body),
        version: header.version,
        services: Vec::new(),
    };
    let mut at = 3;
    while at + 5 <= body.len() {
        let length = usize::from(be16(&body[at + 3..])) & 0x0fff;
        let next = at + 5 + length;
        if next > body.len() {
            break;
        }
        table.services.push(DescribedService {
            service_id: be16(&body[at..]),
            eit_user_defined: body[at + 2] >> 2 & 0x07,
            eit_schedule: body[at + 2] & 0x02 != 0,
            eit_present_following: body[at + 2] & 0x01 != 0,
            running_status: body[at + 3] >> 5,
            scrambled: body[at + 3] & 0x10 != 0,
            descriptors: descriptors(&body[at + 5..next]),
        });
        at = next;
    }
    Some(table)
}

/// An MH-EIT, whose shape is the EIT's with MMT-SI descriptors in place of the
/// transport stream's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventTable {
    pub table_id: u8,
    pub service_id: u16,
    pub tlv_stream_id: u16,
    pub original_network_id: u16,
    pub version: u8,
    pub number: u8,
    pub last_number: u8,
    pub segment_last_number: u8,
    pub last_table_id: u8,
    pub events: Vec<Event>,
}

impl EventTable {
    /// Whether the table is a day of the schedule rather than the present and
    /// following pair.
    pub fn schedule(&self) -> bool {
        (TABLE_MH_EIT_SCHEDULE_FIRST..=TABLE_MH_EIT_SCHEDULE_LAST).contains(&self.table_id)
    }
}

/// One program of an MH-EIT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub event_id: u16,
    pub start: Option<BroadcastTime>,
    /// How long the event runs, in seconds.
    pub duration: Option<u32>,
    pub running_status: u8,
    pub scrambled: bool,
    pub descriptors: Vec<Descriptor>,
}

/// Reads an MH-EIT section.
pub fn event_table(section: &[u8]) -> Option<EventTable> {
    let header = long_section(section)?;
    if !(TABLE_MH_EIT_PRESENT..=TABLE_MH_EIT_SCHEDULE_LAST).contains(&header.table_id)
        || header.body.len() < 6
    {
        return None;
    }
    let body = header.body;
    let mut table = EventTable {
        table_id: header.table_id,
        service_id: header.extension,
        tlv_stream_id: be16(body),
        original_network_id: be16(&body[2..]),
        version: header.version,
        number: header.number,
        last_number: header.last_number,
        segment_last_number: body[4],
        last_table_id: body[5],
        events: Vec::new(),
    };
    let mut at = 6;
    while at + 12 <= body.len() {
        let length = usize::from(be16(&body[at + 10..])) & 0x0fff;
        let next = at + 12 + length;
        if next > body.len() {
            break;
        }
        table.events.push(Event {
            event_id: be16(&body[at..]),
            start: broadcast_time(&body[at + 2..at + 7]),
            duration: duration(&body[at + 7..at + 10]),
            running_status: body[at + 10] >> 5,
            scrambled: body[at + 10] & 0x10 != 0,
            descriptors: descriptors(&body[at + 12..next]),
        });
        at = next;
    }
    Some(table)
}

/// The MH-TOT: the time the broadcast says it is, with the offsets that turn
/// it into local time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeTable {
    pub time: BroadcastTime,
    pub descriptors: Vec<Descriptor>,
}

/// Reads an MH-TOT section, which like the TOT is written in the short form
/// with a check after the descriptors.
pub fn time_table(section: &[u8]) -> Option<TimeTable> {
    if section.len() < 14 || section[0] != TABLE_MH_TOT {
        return None;
    }
    let length = usize::from(be16(&section[1..])) & 0x0fff;
    if 3 + length > section.len() {
        return None;
    }
    let time = broadcast_time(&section[3..8])?;
    let descriptors_length = usize::from(be16(&section[8..])) & 0x0fff;
    let mut table = TimeTable {
        time,
        descriptors: Vec::new(),
    };
    if 10 + descriptors_length <= 3 + length {
        table.descriptors = descriptors(&section[10..10 + descriptors_length]);
    }
    Some(table)
}

/// The MH-BIT.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BroadcasterInformation {
    pub original_network_id: u16,
    pub version: u8,
    pub view_propriety: bool,
    pub descriptors: Vec<Descriptor>,
    pub broadcasters: Vec<Broadcaster>,
}

/// One broadcaster of the MH-BIT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broadcaster {
    pub broadcaster_id: u8,
    pub descriptors: Vec<Descriptor>,
}

/// Reads an MH-BIT section.
pub fn broadcaster_information(section: &[u8]) -> Option<BroadcasterInformation> {
    let header = long_section(section)?;
    if header.table_id != TABLE_MH_BIT || header.body.len() < 2 {
        return None;
    }
    let body = header.body;
    let length = usize::from(be16(body)) & 0x0fff;
    if 2 + length > body.len() {
        return None;
    }
    let mut table = BroadcasterInformation {
        original_network_id: header.extension,
        version: header.version,
        view_propriety: body[0] & 0x10 != 0,
        descriptors: descriptors(&body[2..2 + length]),
        broadcasters: Vec::new(),
    };
    let mut at = 2 + length;
    while at + 3 <= body.len() {
        let loop_length = usize::from(be16(&body[at + 1..])) & 0x0fff;
        let next = at + 3 + loop_length;
        if next > body.len() {
            break;
        }
        table.broadcasters.push(Broadcaster {
            broadcaster_id: body[at],
            descriptors: descriptors(&body[at + 3..next]),
        });
        at = next;
    }
    Some(table)
}

/// The MH-AIT, which starts and stops the applications a broadcast carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplicationInformation {
    pub application_type: u16,
    pub version: u8,
    pub descriptors: Vec<Descriptor>,
    pub applications: Vec<Application>,
}

/// One application of the MH-AIT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Application {
    pub organization_id: u32,
    pub application_id: u16,
    pub control_code: u8,
    pub descriptors: Vec<Descriptor>,
}

/// Reads an MH-AIT section.
pub fn application_information(section: &[u8]) -> Option<ApplicationInformation> {
    let header = long_section(section)?;
    if header.table_id != TABLE_MH_AIT || header.body.len() < 2 {
        return None;
    }
    let body = header.body;
    let length = usize::from(be16(body)) & 0x0fff;
    if 2 + length + 2 > body.len() {
        return None;
    }
    let mut table = ApplicationInformation {
        application_type: header.extension,
        version: header.version,
        descriptors: descriptors(&body[2..2 + length]),
        applications: Vec::new(),
    };
    let mut at = 2 + length;
    let loop_length = usize::from(be16(&body[at..])) & 0x0fff;
    at += 2;
    let end = (at + loop_length).min(body.len());
    while at + 9 <= end {
        let descriptors_length = usize::from(be16(&body[at + 7..])) & 0x0fff;
        let next = at + 9 + descriptors_length;
        if next > end {
            break;
        }
        table.applications.push(Application {
            organization_id: be32(&body[at..]),
            application_id: be16(&body[at + 4..]),
            control_code: body[at + 6],
            descriptors: descriptors(&body[at + 9..next]),
        });
        at = next;
    }
    Some(table)
}

/// One event message table (ARIB STD-B60 表 11-1): a version, a group, and
/// the messages in it. TR-B39 6.2.4 operates it as one section per table
/// with one descriptor: an event message descriptor (group 0x001) or a
/// UTC-NPT reference descriptor (group 0xfff).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventMessageTable {
    pub data_event_id: u8,
    pub group_id: u16,
    pub version: u8,
    pub descriptors: Vec<Descriptor>,
}

/// Reads an EMT section.
pub fn event_message_table(section: &[u8]) -> Option<EventMessageTable> {
    let header = long_section(section)?;
    if header.table_id != TABLE_EMT {
        return None;
    }
    Some(EventMessageTable {
        data_event_id: (header.extension >> 12) as u8,
        group_id: header.extension & 0x0fff,
        version: header.version,
        descriptors: descriptors(header.body),
    })
}

/// One event message (表 11-3): when it fires, what it is called, and what
/// it carries. `time` is the field the mode names (nothing for immediate,
/// UTC as NTP for 0x01/0x05, NPT for 0x02, a relative time for 0x03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventMessage {
    pub group_id: u16,
    pub time_mode: u8,
    pub time: u64,
    pub kind: u8,
    pub id: u16,
    pub private: Vec<u8>,
}

impl EventMessage {
    /// Fires as soon as it arrives (time mode 0x00).
    pub fn immediate(&self) -> bool {
        self.time_mode == 0x00
    }

    /// What an application calls the message: the upper eight bits of
    /// `event_msg_id` (ARIB STD-B62 第二編 3.3.12, `message_id`).
    pub fn message_id(&self) -> u8 {
        (self.id >> 8) as u8
    }

    /// The message's own version: the lower eight bits of `event_msg_id`
    /// (`message_version`). An application fires once per version.
    pub fn message_version(&self) -> u8 {
        (self.id & 0xff) as u8
    }
}

/// Reads the body of an event message descriptor.
pub fn event_message(body: &[u8]) -> Option<EventMessage> {
    if body.len() < 3 + 8 + 3 {
        return None;
    }
    let group_id = be16(body) >> 4;
    let time_mode = body[2];
    let mut time = 0u64;
    for value in &body[3..11] {
        time = time << 8 | u64::from(*value);
    }
    Some(EventMessage {
        group_id,
        time_mode,
        time,
        kind: body[11],
        id: be16(&body[12..]),
        private: body[14..].to_vec(),
    })
}

/// The AMT: which multicast group each service is carried on. A receiver that
/// wants one service and not the rest reads this to know which datagrams to
/// keep. STD-B60 Table 5-2.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddressMap {
    pub extension: u16,
    pub version: u8,
    pub services: Vec<AddressMapEntry>,
}

/// One service's group. The masks are how many leading bits of each address
/// count, so a mask shorter than the address matches a range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressMapEntry {
    pub service_id: u16,
    pub source: IpAddr,
    pub source_mask: u8,
    pub destination: IpAddr,
    pub destination_mask: u8,
    pub private: Vec<u8>,
}

/// Reads an AMT section.
pub fn address_map(section: &[u8]) -> Option<AddressMap> {
    let header = long_section(section)?;
    if header.table_id != TABLE_AMT || header.body.len() < 2 {
        return None;
    }
    let body = header.body;
    let mut table = AddressMap {
        extension: header.extension,
        version: header.version,
        services: Vec::new(),
    };
    let count = usize::from(be16(body) >> 6);
    let mut at = 2;
    for _ in 0..count {
        if at + 4 > body.len() {
            break;
        }
        let service_id = be16(&body[at..]);
        let sixth = body[at + 2] & 0x80 != 0;
        let loop_length = usize::from(be16(&body[at + 2..])) & 0x03ff;
        let next = at + 4 + loop_length;
        if next > body.len() {
            break;
        }
        let addresses = &body[at + 4..next];
        let width = if sixth { 16 } else { 4 };
        if addresses.len() < 2 * width + 2 {
            break;
        }
        table.services.push(AddressMapEntry {
            service_id,
            source: address(&addresses[..width]),
            source_mask: addresses[width],
            destination: address(&addresses[width + 1..2 * width + 1]),
            destination_mask: addresses[2 * width + 1],
            private: addresses[2 * width + 2..].to_vec(),
        });
        at = next;
    }
    Some(table)
}

fn address(bytes: &[u8]) -> IpAddr {
    if bytes.len() == 16 {
        let mut octets = [0u8; 16];
        octets.copy_from_slice(bytes);
        return IpAddr::V6(Ipv6Addr::from(octets));
    }
    let mut octets = [0u8; 4];
    octets.copy_from_slice(&bytes[..4]);
    IpAddr::V4(Ipv4Addr::from(octets))
}

/// The MPU timestamp descriptor, which gives the presentation time of the
/// first access unit of an MPU as a network time.
pub const DESCRIPTOR_MPU_TIMESTAMP: u16 = 0x0001;
/// The MPU extended timestamp descriptor, which gives the decoding times of
/// every access unit of an MPU relative to that one presentation time.
pub const DESCRIPTOR_MPU_EXTENDED_TIMESTAMP: u16 = 0x8026;

/// The clock a transport stream counts presentation times in.
pub const CLOCK_HZ: i64 = 90_000;

/// One entry of an MPU timestamp descriptor: the presentation time of an
/// MPU's first access unit, written as a 64-bit network time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpuTimestamp {
    pub sequence: u32,
    /// A network time: seconds in the high 32 bits, fraction in the low 32.
    pub ntp: u64,
}

/// Reads an MPU timestamp descriptor, which is nothing but a run of
/// twelve-byte pairs. A trailing part-pair is ignored.
pub fn mpu_timestamps(body: &[u8]) -> Vec<MpuTimestamp> {
    let mut times = Vec::new();
    let mut at = 0;
    while at + 12 <= body.len() {
        times.push(MpuTimestamp {
            sequence: be32(&body[at..]),
            ntp: be64(&body[at + 4..]),
        });
        at += 12;
    }
    times
}

/// One access unit of an MPU extended timestamp descriptor. Both offsets are
/// counted in the descriptor's timescale, not in the transport stream's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccessUnitTiming {
    /// How far the presentation time of this access unit is after its
    /// decoding time.
    pub dts_pts_offset: u16,
    /// How long this access unit lasts, which is what separates its decoding
    /// time from the next one's. When the descriptor does not write one per
    /// access unit, this is the descriptor's default.
    pub pts_offset: u16,
}

/// One MPU of an MPU extended timestamp descriptor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtendedTimestampEntry {
    pub sequence: u32,
    /// The leap second indicator, which says whether a leap second falls
    /// inside this MPU.
    pub leap: u8,
    /// How far the decoding time of the first access unit is *before* the
    /// presentation time the MPU timestamp descriptor gave it.
    pub decoding_time_offset: u16,
    pub access_units: Vec<AccessUnitTiming>,
}

/// An MPU extended timestamp descriptor. The timescale and the default offset
/// are written once for the asset and need not be repeated, so a descriptor
/// that omits them means the ones an earlier descriptor gave still stand.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtendedTimestamp {
    /// How the per-access-unit presentation offsets are written: 1 for a
    /// single default for all of them, 2 for one written per access unit.
    pub pts_offset_type: u8,
    /// The unit the offsets are counted in, when this descriptor writes one.
    pub timescale: Option<u32>,
    /// The offset every access unit takes when `pts_offset_type` is 1.
    pub default_pts_offset: u16,
    pub entries: Vec<ExtendedTimestampEntry>,
    /// Whether the descriptor ran out before its fields did, or gave a
    /// timescale of zero. The entries read before that are still returned,
    /// but a caller that wants times should not trust any of it.
    pub truncated: bool,
}

impl ExtendedTimestamp {
    /// The entry for an MPU, or nothing.
    pub fn entry(&self, sequence: u32) -> Option<&ExtendedTimestampEntry> {
        self.entries.iter().find(|entry| entry.sequence == sequence)
    }
}

/// Reads an MPU extended timestamp descriptor. The entries after the header are
/// a variable-length run whose stride depends on `pts_offset_type`: two bytes
/// per access unit when the presentation offsets are a single default, four
/// when each writes its own.
pub fn extended_timestamp(body: &[u8]) -> ExtendedTimestamp {
    let Some(first) = body.first() else {
        return ExtendedTimestamp {
            truncated: true,
            ..ExtendedTimestamp::default()
        };
    };
    let mut read = ExtendedTimestamp {
        pts_offset_type: first >> 1 & 0x03,
        ..ExtendedTimestamp::default()
    };
    let mut at = 1;
    if first & 0x01 != 0 {
        if body.len() - at < 4 {
            read.truncated = true;
            return read;
        }
        let timescale = be32(&body[at..]);
        if timescale == 0 {
            read.truncated = true;
            return read;
        }
        read.timescale = Some(timescale);
        at += 4;
    }
    if read.pts_offset_type == 1 {
        if body.len() - at < 2 {
            read.truncated = true;
            return read;
        }
        read.default_pts_offset = be16(&body[at..]);
        at += 2;
    }
    let per_unit = if read.pts_offset_type == 2 { 4 } else { 2 };
    while at < body.len() {
        if body.len() - at < 8 {
            read.truncated = true;
            return read;
        }
        let mut entry = ExtendedTimestampEntry {
            sequence: be32(&body[at..]),
            leap: body[at + 4] >> 6,
            decoding_time_offset: be16(&body[at + 5..]),
            access_units: Vec::new(),
        };
        let count = usize::from(body[at + 7]);
        at += 8;
        if body.len() - at < count * per_unit {
            read.truncated = true;
            return read;
        }
        entry.access_units.reserve(count);
        for _ in 0..count {
            let mut unit = AccessUnitTiming {
                dts_pts_offset: be16(&body[at..]),
                pts_offset: read.default_pts_offset,
            };
            at += 2;
            if read.pts_offset_type == 2 {
                unit.pts_offset = be16(&body[at..]);
                at += 2;
            }
            entry.access_units.push(unit);
        }
        read.entries.push(entry);
    }
    read
}

/// A network time as a count of the transport stream's clock, taken from a
/// base that the same stream's other times are taken from.
pub fn ntp_to_90k(base: u64, ntp: u64) -> i64 {
    let delta = ntp.wrapping_sub(base) as i64;
    let seconds = delta >> 32;
    let fraction = u64::from(delta as u32);
    seconds
        .wrapping_mul(CLOCK_HZ)
        .wrapping_add(((fraction * CLOCK_HZ as u64) >> 32) as i64)
}

/// A count of a descriptor's own timescale as a count of the transport
/// stream's clock, rounded to the nearest tick and rounded the same way on
/// either side of zero.
pub fn ticks_to_90k(ticks: i64, timescale: u32) -> i64 {
    if timescale == 0 {
        return 0;
    }
    let scale = i128::from(timescale);
    let magnitude = i128::from(ticks).abs();
    let rounded = (magnitude * i128::from(CLOCK_HZ) + scale / 2) / scale;
    let rounded = rounded.min(i128::from(i64::MAX)) as i64;
    if ticks < 0 { -rounded } else { rounded }
}

/// The one network time every other time of a stream is counted from. Which
/// time it is does not matter, only that it is the same one throughout: the
/// first that arrives is taken and kept, so that the times a caller hands on
/// are a timeline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClockBase {
    ntp: Option<u64>,
}

impl ClockBase {
    /// A base that has not been taken yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes this time as the base, if none has been taken.
    pub fn set(&mut self, ntp: u64) {
        self.ntp.get_or_insert(ntp);
    }

    /// The time the base was taken from, once one has been.
    pub fn ntp(&self) -> Option<u64> {
        self.ntp
    }

    /// A network time as a count of the transport stream's clock from this
    /// base, or nothing while no base has been taken.
    pub fn to_90k(&self, ntp: u64) -> Option<i64> {
        self.ntp.map(|base| ntp_to_90k(base, ntp))
    }
}

/// When one access unit is decoded and when it is presented, both counted in
/// the transport stream's clock from a [`ClockBase`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessUnitTimes {
    pub dts: i64,
    pub pts: i64,
}

/// The decoding and presentation times of one access unit of an MPU.
///
/// `ntp` is the MPU's presentation time from its MPU timestamp descriptor,
/// `entry` the same MPU's entry in an extended timestamp descriptor, and
/// `sample_number` counts the access units from one. The timescale and the
/// presentation offsets are passed rather than read from the entry because a
/// descriptor need not repeat them.
///
/// The decoding time of the first access unit is `decoding_time_offset` before
/// the MPU's presentation time, each access unit after it is one presentation
/// offset later, and the presentation time of any of them is its own
/// `dts_pts_offset` after its decoding time. Nothing comes back if the
/// timescale is zero or the sample number is not one of the entry's.
pub fn access_unit_times(
    base: &mut ClockBase,
    entry: &ExtendedTimestampEntry,
    timescale: u32,
    pts_offset_type: u8,
    default_pts_offset: u16,
    ntp: u64,
    sample_number: u32,
) -> Option<AccessUnitTimes> {
    if timescale == 0 || sample_number == 0 {
        return None;
    }
    let index = sample_number as usize - 1;
    let units = entry.access_units.get(..=index)?;
    let mut offset = -i64::from(entry.decoding_time_offset);
    if pts_offset_type == 2 {
        for unit in &units[..index] {
            offset += i64::from(unit.pts_offset);
        }
    } else {
        offset += index as i64 * i64::from(default_pts_offset);
    }
    base.set(ntp);
    let origin = base.to_90k(ntp)?;
    Some(AccessUnitTimes {
        dts: origin + ticks_to_90k(offset, timescale),
        pts: origin + ticks_to_90k(offset + i64::from(units[index].dts_pts_offset), timescale),
    })
}

fn be64(bytes: &[u8]) -> u64 {
    if bytes.len() < 8 {
        return 0;
    }
    u64::from(be32(bytes)) << 32 | u64::from(be32(&bytes[4..]))
}

/// Where a package's PA message is, as a PLT names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageLocation {
    /// In this stream, on this packet identifier.
    Here { packet_id: u16 },
    /// In an IP flow (IPv4 or IPv6): the destination port and packet
    /// identifier.
    Ip { port: u16, packet_id: u16 },
    /// In an MPEG-2 transport stream.
    Ts {
        network_id: u16,
        transport_stream_id: u16,
        pid: u16,
    },
    /// At a URL.
    Url(String),
    /// A location type this reader does not know.
    Other(u8),
}

/// One package a PLT lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedPackage {
    pub package_id: Vec<u8>,
    pub location: PackageLocation,
}

/// A package list table (STD-B60 7.3.3.2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageList {
    pub version: u8,
    pub packages: Vec<ListedPackage>,
}

/// Reads an `MMT_general_location_info`: where something is, and how many
/// bytes the field took.
pub fn general_location(bytes: &[u8]) -> Option<(PackageLocation, usize)> {
    let kind = *bytes.first()?;
    let rest = &bytes[1..];
    Some(match kind {
        0x00 => (
            PackageLocation::Here {
                packet_id: be16(rest.get(..2)?),
            },
            3,
        ),
        0x01 => (
            PackageLocation::Ip {
                port: be16(rest.get(8..10)?),
                packet_id: be16(rest.get(10..12)?),
            },
            13,
        ),
        0x02 => (
            PackageLocation::Ip {
                port: be16(rest.get(32..34)?),
                packet_id: be16(rest.get(34..36)?),
            },
            37,
        ),
        0x03 => (
            PackageLocation::Ts {
                network_id: be16(rest.get(..2)?),
                transport_stream_id: be16(rest.get(2..4)?),
                pid: be16(rest.get(4..6)?) & 0x1fff,
            },
            7,
        ),
        0x04 => (
            PackageLocation::Ts {
                network_id: 0,
                transport_stream_id: 0,
                pid: be16(rest.get(34..36)?) & 0x1fff,
            },
            37,
        ),
        0x05 => {
            let length = usize::from(*rest.first()?);
            (
                PackageLocation::Url(
                    String::from_utf8_lossy(rest.get(1..1 + length)?).into_owned(),
                ),
                2 + length,
            )
        }
        other => (PackageLocation::Other(other), 1),
    })
}

/// Reads a PLT.
pub fn package_list(table: &[u8]) -> Option<PackageList> {
    if table.len() < 5 || table[0] != TABLE_PLT {
        return None;
    }
    let mut read = PackageList {
        version: table[1],
        packages: Vec::new(),
    };
    let count = usize::from(table[4]);
    let mut at = 5;
    for _ in 0..count {
        let length = usize::from(*table.get(at)?);
        let package_id = table.get(at + 1..at + 1 + length)?.to_vec();
        at += 1 + length;
        let (location, used) = general_location(table.get(at..)?)?;
        at += used;
        read.packages.push(ListedPackage {
            package_id,
            location,
        });
    }
    Some(read)
}

/// One region of a layout (STD-B60 7.3.3.3): the corners as percentages of
/// the screen, and the stacking order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayoutRegion {
    pub number: u8,
    pub left: u8,
    pub top: u8,
    pub right: u8,
    pub bottom: u8,
    pub layer: u8,
}

/// One layout of one device.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    pub number: u8,
    /// 0 is the main device (the television itself).
    pub device: u8,
    pub regions: Vec<LayoutRegion>,
}

/// Reads an LCT: the layouts a multi-view service defines.
pub fn layout_configuration(table: &[u8]) -> Option<Vec<Layout>> {
    if table.len() < 5 || table[0] != TABLE_LCT {
        return None;
    }
    let count = usize::from(table[4]);
    let mut at = 5;
    let mut layouts = Vec::with_capacity(count);
    for _ in 0..count {
        let head = table.get(at..at + 3)?;
        let mut layout = Layout {
            number: head[0],
            device: head[1],
            regions: Vec::new(),
        };
        at += 3;
        for _ in 0..usize::from(head[2]) {
            let region = table.get(at..at + 6)?;
            layout.regions.push(LayoutRegion {
                number: region[0],
                left: region[1],
                top: region[2],
                right: region[3],
                bottom: region[4],
                layer: region[5],
            });
            at += 6;
        }
        layouts.push(layout);
    }
    Some(layouts)
}

/// Reads an MH-SDTT section: the SDTT's shape with a TLV stream identifier
/// (STD-B60 7.3.3.20). The transport stream identifier of the answer holds
/// the TLV stream identifier.
pub fn software_download(section: &[u8]) -> Option<crate::tstables::SoftwareDownload> {
    // The descriptors are MMT-SI ones (STD-B60 Table 7-22 and 7.4): two-byte
    // tags, in `mh_descriptors`.
    crate::tstables::download_table(section, TABLE_MH_SDTT, true)
}

/// An MH-CDT section: common data, which is a logo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommonData {
    pub download_data_id: u16,
    pub version: u8,
    pub number: u8,
    pub last_number: u8,
    pub original_network_id: u16,
    pub data_type: u8,
    pub descriptors: Vec<Descriptor>,
    /// The data module as sent (for a logo: STD-B60 6.7.1 lays it out as
    /// logo_type, logo_id, logo_version and the PNG).
    pub data: Vec<u8>,
}

/// Reads an MH-CDT section (STD-B60 7.3.3.24).
pub fn common_data(section: &[u8]) -> Option<CommonData> {
    let header = long_section(section)?;
    if header.table_id != TABLE_MH_CDT || header.body.len() < 5 {
        return None;
    }
    let body = header.body;
    let described = usize::from(be16(&body[3..]) & 0x0fff);
    Some(CommonData {
        download_data_id: header.extension,
        version: header.version,
        number: header.number,
        last_number: header.last_number,
        original_network_id: be16(body),
        data_type: body[2],
        descriptors: descriptors(body.get(5..5 + described)?),
        data: body.get(5 + described..)?.to_vec(),
    })
}

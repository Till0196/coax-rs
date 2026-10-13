// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The tables of a transport stream that are read but not acted on.
//!
//! Tuning and scanning need the network table and the service table and nothing
//! else. A program guide, the time, the broadcaster list and a program's own
//! composition are somebody else's work -- but a stream carries them, and a
//! caller holding a capture should be able to say what is in one. They are
//! parsed here so that the Rust client reads the same bytes the same way the
//! daemon does.

/// Section identifiers, as STD-B10 and ISO/IEC 13818-1 assign them.
pub const TABLE_PAT: u8 = 0x00;
pub const TABLE_CAT: u8 = 0x01;
pub const TABLE_PMT: u8 = 0x02;
pub const TABLE_SDT_OWN: u8 = 0x42;
pub const TABLE_SDT_OTHER: u8 = 0x46;
pub const TABLE_EIT_OWN: u8 = 0x4e;
pub const TABLE_EIT_OTHER: u8 = 0x4f;
pub const TABLE_EIT_SCHEDULE_FIRST: u8 = 0x50;
pub const TABLE_EIT_SCHEDULE_LAST: u8 = 0x6f;
pub const TABLE_TDT: u8 = 0x70;
pub const TABLE_TOT: u8 = 0x73;
pub const TABLE_BIT: u8 = 0xc4;
/// SDTT (ARIB STD-B21): software download announcements. On PID 0x0023
/// (and 0x0028).
pub const TABLE_SDTT: u8 = 0xc3;
/// CDT (ARIB STD-B21): common data, which is where the service logos are.
/// On PID 0x0029.
pub const TABLE_CDT: u8 = 0xc8;
/// SIT (ARIB STD-B10 Part 3 / STD-B21): what a partial transport stream
/// carries in place of the SDT and EIT. On PID 0x001f.
pub const TABLE_SIT: u8 = 0x7f;
/// DIT: a discontinuity in a partial transport stream. On PID 0x001e.
pub const TABLE_DIT: u8 = 0x7e;
/// EMM message section (ARIB STD-B25 3.2.5): the messages of the conditional
/// access system, on the PID the CAT names.
pub const TABLE_EMM_MESSAGE: u8 = 0x85;

/// One descriptor as a transport stream writes it: a one-byte tag and a body
/// whose length is one byte. The body is left as it arrived, so that nothing a
/// table carried is thrown away silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Descriptor {
    pub tag: u8,
    pub body: Vec<u8>,
}

/// Reads a descriptor loop. A descriptor running past the end of the loop ends
/// the reading: what follows it cannot be found without the length that was
/// wrong.
pub fn descriptors(buffer: &[u8]) -> Vec<Descriptor> {
    let mut described = Vec::new();
    let mut at = 0;
    while at + 2 <= buffer.len() {
        let length = usize::from(buffer[at + 1]);
        let next = at + 2 + length;
        if next > buffer.len() {
            break;
        }
        described.push(Descriptor {
            tag: buffer[at],
            body: buffer[at + 2..next].to_vec(),
        });
        at = next;
    }
    described
}

/// The descriptor with this tag, or nothing. Where a loop carries the tag more
/// than once the first is taken, which is what a table means by repeating a
/// tag it defines once.
pub fn find(described: &[Descriptor], tag: u8) -> Option<&Descriptor> {
    described.iter().find(|one| one.tag == tag)
}

/// The part of a long-form section between the header and the check, with the
/// extension, version and numbering every such table has.
pub(crate) struct LongSection<'a> {
    pub table_id: u8,
    pub extension: u16,
    pub version: u8,
    pub number: u8,
    pub last_number: u8,
    pub body: &'a [u8],
}

pub(crate) fn long_section(section: &[u8]) -> Option<LongSection<'_>> {
    if section.len() < 12 || section[1] & 0x80 == 0 {
        return None;
    }
    let length = usize::from(be16(&section[1..])) & 0x0fff;
    let end = 3 + length;
    if end > section.len() || length < 9 {
        return None;
    }
    Some(LongSection {
        table_id: section[0],
        extension: be16(&section[3..]),
        version: section[5] >> 1 & 0x1f,
        number: section[6],
        last_number: section[7],
        body: &section[8..end - 4],
    })
}

/// One program's composition: where its clock is and which elementary streams
/// it is made of.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProgramMap {
    pub program_number: u16,
    pub version: u8,
    pub pcr_pid: u16,
    pub descriptors: Vec<Descriptor>,
    pub streams: Vec<ElementaryStream>,
}

/// One stream of a program: what it carries and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementaryStream {
    pub stream_type: u8,
    pub pid: u16,
    pub descriptors: Vec<Descriptor>,
}

/// Reads a PMT section.
pub fn program_map(section: &[u8]) -> Option<ProgramMap> {
    let header = long_section(section)?;
    if header.table_id != TABLE_PMT || header.body.len() < 4 {
        return None;
    }
    let body = header.body;
    let program_length = usize::from(be16(&body[2..])) & 0x0fff;
    if 4 + program_length > body.len() {
        return None;
    }
    let mut map = ProgramMap {
        program_number: header.extension,
        version: header.version,
        pcr_pid: be16(body) & 0x1fff,
        descriptors: descriptors(&body[4..4 + program_length]),
        streams: Vec::new(),
    };
    let mut at = 4 + program_length;
    while at + 5 <= body.len() {
        let length = usize::from(be16(&body[at + 3..])) & 0x0fff;
        let next = at + 5 + length;
        if next > body.len() {
            break;
        }
        map.streams.push(ElementaryStream {
            stream_type: body[at],
            pid: be16(&body[at + 1..]) & 0x1fff,
            descriptors: descriptors(&body[at + 5..next]),
        });
        at = next;
    }
    Some(map)
}

/// The CAT: where the entitlement messages for the whole stream are, as
/// descriptors that name their identifiers and packets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConditionalAccess {
    pub version: u8,
    pub descriptors: Vec<Descriptor>,
}

/// Reads a CAT section.
pub fn conditional_access(section: &[u8]) -> Option<ConditionalAccess> {
    let header = long_section(section)?;
    if header.table_id != TABLE_CAT {
        return None;
    }
    Some(ConditionalAccess {
        version: header.version,
        descriptors: descriptors(header.body),
    })
}

/// An SDT, the table that names services. It comes in two kinds: one for the
/// stream carrying it and one for a neighbour, told apart by the table
/// identifier and not by anything inside.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceDescription {
    pub table_id: u8,
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    pub version: u8,
    pub services: Vec<DescribedService>,
}

impl ServiceDescription {
    /// Whether the table describes the stream that carried it.
    pub fn own(&self) -> bool {
        self.table_id == TABLE_SDT_OWN
    }
}

/// One service of an SDT, with the descriptors that name it left for the
/// caller to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescribedService {
    pub service_id: u16,
    pub eit_schedule: bool,
    pub eit_present_following: bool,
    pub running_status: u8,
    /// Set where one or more of the service's streams are under conditional
    /// access: `free_CA_mode` read the way the standard writes it.
    pub scrambled: bool,
    pub descriptors: Vec<Descriptor>,
}

/// Reads an SDT section of either kind.
pub fn service_description(section: &[u8]) -> Option<ServiceDescription> {
    let header = long_section(section)?;
    if (header.table_id != TABLE_SDT_OWN && header.table_id != TABLE_SDT_OTHER)
        || header.body.len() < 3
    {
        return None;
    }
    let body = header.body;
    let mut table = ServiceDescription {
        table_id: header.table_id,
        transport_stream_id: header.extension,
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

/// An EIT: what a service is showing now, next, or on a day in its schedule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventTable {
    pub table_id: u8,
    pub service_id: u16,
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    pub version: u8,
    pub number: u8,
    pub last_number: u8,
    pub segment_last_number: u8,
    pub last_table_id: u8,
    pub events: Vec<Event>,
}

impl EventTable {
    /// Whether the events belong to the stream that carried the table.
    pub fn own(&self) -> bool {
        self.table_id == TABLE_EIT_OWN || (TABLE_EIT_SCHEDULE_FIRST..=0x5f).contains(&self.table_id)
    }

    /// Whether the table is a day of the schedule rather than the present and
    /// following pair.
    pub fn schedule(&self) -> bool {
        (TABLE_EIT_SCHEDULE_FIRST..=TABLE_EIT_SCHEDULE_LAST).contains(&self.table_id)
    }
}

/// One program. A start or a duration every bit of which is set is the table
/// saying it does not know, which is why both are held as options.
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

/// Reads an EIT section of any of its kinds.
pub fn event_table(section: &[u8]) -> Option<EventTable> {
    let header = long_section(section)?;
    if !(TABLE_EIT_OWN..=TABLE_EIT_SCHEDULE_LAST).contains(&header.table_id)
        || header.body.len() < 6
    {
        return None;
    }
    let body = header.body;
    let mut table = EventTable {
        table_id: header.table_id,
        service_id: header.extension,
        transport_stream_id: be16(body),
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

/// The time the stream says it is: the date table on its own, or the offset
/// table which carries the local offsets with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeTable {
    pub table_id: u8,
    pub time: BroadcastTime,
    pub descriptors: Vec<Descriptor>,
}

/// Reads a TDT or a TOT section. Neither is a long-form section: the date
/// table has no fields past the time, and the offset table follows it with a
/// descriptor loop and a check.
pub fn time_table(section: &[u8]) -> Option<TimeTable> {
    if section.len() < 8 || (section[0] != TABLE_TDT && section[0] != TABLE_TOT) {
        return None;
    }
    let length = usize::from(be16(&section[1..])) & 0x0fff;
    if 3 + length > section.len() {
        return None;
    }
    let time = broadcast_time(&section[3..8])?;
    let mut table = TimeTable {
        table_id: section[0],
        time,
        descriptors: Vec::new(),
    };
    if section[0] == TABLE_TDT {
        return Some(table);
    }
    if section.len() < 12 {
        return None;
    }
    let descriptors_length = usize::from(be16(&section[8..])) & 0x0fff;
    if 10 + descriptors_length <= 3 + length {
        table.descriptors = descriptors(&section[10..10 + descriptors_length]);
    }
    Some(table)
}

/// The BIT, which groups a network's services under the broadcasters that run
/// them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BroadcasterInformation {
    pub original_network_id: u16,
    pub version: u8,
    pub view_propriety: bool,
    pub descriptors: Vec<Descriptor>,
    pub broadcasters: Vec<Broadcaster>,
}

/// One broadcaster of the BIT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broadcaster {
    pub broadcaster_id: u8,
    pub descriptors: Vec<Descriptor>,
}

/// Reads a BIT section.
pub fn broadcaster_information(section: &[u8]) -> Option<BroadcasterInformation> {
    let header = long_section(section)?;
    if header.table_id != TABLE_BIT || header.body.len() < 2 {
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

/// A moment as a table writes it: the day as a modified Julian date and the
/// time of day as binary-coded decimal. The zone is the broadcaster's own --
/// Japan writes local time, DVB writes UTC -- so this is the wall clock as it
/// was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BroadcastTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// Reads the five bytes a table writes a moment in. A field of every bit set
/// is the table saying it does not know.
pub fn broadcast_time(field: &[u8]) -> Option<BroadcastTime> {
    if field.len() < 5 || field[..5].iter().all(|&value| value == 0xff) {
        return None;
    }
    let (year, month, day) = from_modified_julian(u32::from(be16(field)));
    Some(BroadcastTime {
        year,
        month,
        day,
        hour: from_bcd(field[2])?,
        minute: from_bcd(field[3])?,
        second: from_bcd(field[4])?,
    })
}

/// Reads the three binary-coded decimal bytes a table writes a length of time
/// in, as seconds.
pub fn duration(field: &[u8]) -> Option<u32> {
    if field.len() < 3 || field[..3].iter().all(|&value| value == 0xff) {
        return None;
    }
    Some(from_bcd(field[0])? * 3600 + from_bcd(field[1])? * 60 + from_bcd(field[2])?)
}

/// The civil date of a modified Julian day, by the conversion STD-B10 Annex C
/// gives.
fn from_modified_julian(day: u32) -> (i32, u32, u32) {
    let day = f64::from(day);
    let mut year = ((day - 15078.2) / 365.25).floor();
    let mut month = ((day - 14956.1 - (year * 365.25).floor()) / 30.6001).floor();
    let date = day - 14956.0 - (year * 365.25).floor() - (month * 30.6001).floor();
    // The correction is 1 where the month came out as January or February of
    // the year after the one the first division gave.
    let correction = if month == 14.0 || month == 15.0 {
        1.0
    } else {
        0.0
    };
    year += correction;
    month = month - 1.0 - correction * 12.0;
    (1900 + year as i32, month as u32, date as u32)
}

/// Reads one byte of binary-coded decimal. A nibble above nine is not a digit,
/// and a table writing one has written something this is not.
fn from_bcd(value: u8) -> Option<u32> {
    let (high, low) = (u32::from(value >> 4), u32::from(value & 0x0f));
    if high > 9 || low > 9 {
        return None;
    }
    Some(high * 10 + low)
}

pub(crate) fn be16(bytes: &[u8]) -> u16 {
    if bytes.len() < 2 {
        return 0;
    }
    u16::from(bytes[0]) << 8 | u16::from(bytes[1])
}

pub(crate) fn be32(bytes: &[u8]) -> u32 {
    if bytes.len() < 4 {
        return 0;
    }
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// One piece of software an SDTT announces for one maker's model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DownloadContent {
    pub group: u8,
    pub target_version: u16,
    pub new_version: u16,
    /// 0 arbitrary, 1 forced.
    pub download_level: u8,
    /// 0 all versions, 1 the target version and below, 2 the target version
    /// and above, 3 the target version alone.
    pub version_indicator: u8,
    /// `schedule_time_shift_information`.
    pub time_shift: u8,
    /// When the download runs: `(start, seconds)`.
    pub schedules: Vec<(BroadcastTime, u32)>,
    /// The descriptors of an SDTT.
    pub descriptors: Vec<Descriptor>,
    /// The descriptors of an MH-SDTT, which are MMT-SI ones with two-byte tags
    /// (STD-B60 7.4). `descriptors` is empty then.
    pub mh_descriptors: Vec<crate::mmttables::Descriptor>,
}

/// An SDTT section: which model the software is for, and what is offered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoftwareDownload {
    pub maker_id: u8,
    pub model_id: u8,
    pub version: u8,
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    pub service_id: u16,
    pub contents: Vec<DownloadContent>,
}

/// Reads an SDTT section (ARIB STD-B21 12.2.1.1). The MH-SDTT of MMT is
/// the same shape with a TLV stream identifier where the transport stream
/// identifier is and MMT-SI descriptors, read by
/// [`crate::mmttables::software_download`].
pub fn software_download(section: &[u8]) -> Option<SoftwareDownload> {
    download_table(section, TABLE_SDTT, false)
}

/// The body both trigger tables share. `mh` reads the descriptors as MMT-SI
/// ones.
pub(crate) fn download_table(section: &[u8], table_id: u8, mh: bool) -> Option<SoftwareDownload> {
    let header = long_section(section)?;
    if header.table_id != table_id || header.body.len() < 7 {
        return None;
    }
    let body = header.body;
    let mut table = SoftwareDownload {
        maker_id: (header.extension >> 8) as u8,
        model_id: header.extension as u8,
        version: header.version,
        transport_stream_id: be16(body),
        original_network_id: be16(&body[2..]),
        service_id: be16(&body[4..]),
        contents: Vec::new(),
    };
    let count = usize::from(body[6]);
    let mut at = 7;
    for _ in 0..count {
        let head = body.get(at..at + 8)?;
        // content_description_length 12 | reserved 4 | schedule 12 | shift 4.
        let content_length = usize::from(be16(&head[4..]) >> 4);
        let schedule_length = usize::from(be16(&head[6..]) >> 4);
        let mut content = DownloadContent {
            group: head[0] >> 4,
            target_version: (u16::from(head[0] & 0x0f) << 8) | u16::from(head[1]),
            new_version: (u16::from(head[2]) << 4) | u16::from(head[3] >> 4),
            download_level: (head[3] >> 2) & 0x03,
            version_indicator: head[3] & 0x03,
            time_shift: head[7] & 0x0f,
            schedules: Vec::new(),
            descriptors: Vec::new(),
            mh_descriptors: Vec::new(),
        };
        at += 8;
        let schedules = body.get(at..at + schedule_length)?;
        for entry in schedules.as_chunks::<8>().0 {
            if let (Some(start), Some(seconds)) =
                (broadcast_time(&entry[..5]), duration(&entry[5..8]))
            {
                content.schedules.push((start, seconds));
            }
        }
        at += schedule_length;
        let described = content_length.checked_sub(schedule_length)?;
        let described = body.get(at..at + described)?;
        if mh {
            content.mh_descriptors = crate::mmttables::descriptors(described);
        } else {
            content.descriptors = descriptors(described);
        }
        let described = described.len();
        at += described;
        table.contents.push(content);
    }
    Some(table)
}

/// A CDT section: one piece of common data, which in practice is a logo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommonData {
    pub download_data_id: u16,
    pub version: u8,
    pub number: u8,
    pub last_number: u8,
    pub original_network_id: u16,
    /// 0x01 is a logo.
    pub data_type: u8,
    pub descriptors: Vec<Descriptor>,
    /// For a logo: the kind (0 large 64x36 .. 5 small, ARIB TR-B14), the
    /// identifier and the version, then the PNG.
    pub logo_type: u8,
    pub logo_id: u16,
    pub logo_version: u16,
    pub data: Vec<u8>,
}

/// Reads a CDT section (ARIB STD-B21 12.2.2.2).
pub fn common_data(section: &[u8]) -> Option<CommonData> {
    let header = long_section(section)?;
    if header.table_id != TABLE_CDT || header.body.len() < 5 {
        return None;
    }
    let body = header.body;
    let described = usize::from(be16(&body[3..]) & 0x0fff);
    let mut table = CommonData {
        download_data_id: header.extension,
        version: header.version,
        number: header.number,
        last_number: header.last_number,
        original_network_id: be16(body),
        data_type: body[2],
        descriptors: descriptors(body.get(5..5 + described)?),
        ..CommonData::default()
    };
    let rest = &body[5 + described..];
    if table.data_type == 0x01 {
        let head = rest.get(..7)?;
        table.logo_type = head[0];
        table.logo_id = be16(&head[1..]) & 0x01ff;
        table.logo_version = be16(&head[3..]) & 0x0fff;
        let size = usize::from(be16(&head[5..]));
        table.data = rest.get(7..7 + size)?.to_vec();
    } else {
        table.data = rest.to_vec();
    }
    Some(table)
}

/// One service of a SIT, with the descriptors that describe it (a service
/// descriptor, and the event's own descriptors: short event, component,
/// audio component, content...).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectedService {
    pub service_id: u16,
    pub running_status: u8,
    pub descriptors: Vec<Descriptor>,
}

/// A SIT section: what a partial transport stream (a recording of one
/// service) carries in place of the NIT, SDT and EIT. The transmission
/// descriptors name the network (network identification descriptor) and
/// the stream's rate (partial transport stream descriptor); the services
/// carry what the SDT and EIT would have said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionInformation {
    pub version: u8,
    pub descriptors: Vec<Descriptor>,
    pub services: Vec<SelectedService>,
}

/// Reads a SIT section.
pub fn selection_information(section: &[u8]) -> Option<SelectionInformation> {
    let header = long_section(section)?;
    if header.table_id != TABLE_SIT || header.body.len() < 2 {
        return None;
    }
    let body = header.body;
    let described = usize::from(be16(body) & 0x0fff);
    let mut table = SelectionInformation {
        version: header.version,
        descriptors: descriptors(body.get(2..2 + described)?),
        services: Vec::new(),
    };
    let mut at = 2 + described;
    while at + 4 <= body.len() {
        let length = usize::from(be16(&body[at + 2..]) & 0x0fff);
        let next = at + 4 + length;
        if next > body.len() {
            break;
        }
        table.services.push(SelectedService {
            service_id: be16(&body[at..]),
            running_status: (body[at + 2] >> 4) & 0x07,
            descriptors: descriptors(&body[at + 4..next]),
        });
        at = next;
    }
    Some(table)
}

/// Reads a DIT section: whether the partial transport stream is about to
/// change (`transition_flag`).
pub fn discontinuity_information(section: &[u8]) -> Option<bool> {
    if section.len() < 4 || section[0] != TABLE_DIT {
        return None;
    }
    Some(section[3] & 0x80 != 0)
}

/// An EMM common message: one preset text that every receiver is sent alike.
/// A message held for one card picks it by its number and may add a few bytes
/// of its own, which is how the automatic display messages and the mail are
/// written. ARIB STD-B25 3.2.5.1 and TR-B15 5.10.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommonMessage {
    /// The preset text's number (`table_id_extension`, 0x0001 to 0xffff).
    pub fixed_message_id: u16,
    pub version: u8,
    pub broadcaster_group_id: u8,
    /// 0x00 the viewer may close it, 0x01 the viewer may not, 0x02 it is not
    /// shown (and one being shown is taken down). The fields after it mean
    /// nothing when it is 0x02.
    pub deletion_status: u8,
    /// How long it is shown, hidden and shown again after a channel is
    /// selected, in tenths of a minute. 0xff is for as long as the channel is.
    pub durations: [u8; 3],
    /// How many times the three durations repeat.
    pub cycle: u8,
    /// 0x01 is the only format defined (TR-B15 5.10.1).
    pub format_version: u8,
    pub message: Vec<u8>,
}

/// Where in the message the bytes a card's own message carries are put
/// (TR-B15 5.10.2).
pub const MESSAGE_INSERTER: u8 = 0x1a;

impl CommonMessage {
    /// The recommended position of format 0x01: the upper four bits across
    /// (0b0100 left, 0b0010 centre, 0b0001 right), the lower four down
    /// (0b0100 top, 0b0010 centre, 0b0001 bottom).
    pub fn position(&self) -> Option<u8> {
        if self.format_version != 0x01 {
            return None;
        }
        self.message.first().copied()
    }

    /// The text of format 0x01, in the eight-unit code of STD-B24, up to the
    /// NULL that ends it. Where it holds [`MESSAGE_INSERTER`] a card's own
    /// bytes go.
    pub fn text(&self) -> Option<&[u8]> {
        if self.format_version != 0x01 {
            return None;
        }
        let text = self.message.get(1..)?;
        let end = text
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(text.len());
        Some(&text[..end])
    }

    /// The text with `extra` put in place of every inserter, ready for
    /// [`crate::arib::decode`].
    pub fn compose(&self, extra: &[u8]) -> Option<Vec<u8>> {
        let mut composed = Vec::new();
        for &byte in self.text()? {
            if byte == MESSAGE_INSERTER {
                composed.extend_from_slice(extra);
            } else {
                composed.push(byte);
            }
        }
        Some(composed)
    }
}

/// Reads an EMM common message section. The same table identifier carries
/// the messages for single cards under the extension 0x0000; those are not
/// read here.
pub fn common_message(section: &[u8]) -> Option<CommonMessage> {
    let header = long_section(section)?;
    if header.table_id != TABLE_EMM_MESSAGE || header.extension == 0 {
        return None;
    }
    let body = header.body;
    let head = body.get(..9)?;
    let length = usize::from(be16(&head[7..]));
    Some(CommonMessage {
        fixed_message_id: header.extension,
        version: header.version,
        broadcaster_group_id: head[0],
        deletion_status: head[1],
        durations: [head[2], head[3], head[4]],
        cycle: head[5],
        format_version: head[6],
        message: body.get(9..9 + length)?.to_vec(),
    })
}

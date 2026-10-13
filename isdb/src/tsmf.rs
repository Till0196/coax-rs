// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Taking one stream out of a multiplexing frame.
//!
//! A Japanese cable carries several transport streams down one carrier: a frame
//! is a header packet followed by fifty-two slots, and the header says which
//! stream each slot belongs to. A television that knows nothing of this sees
//! one stream with several sets of tables in it and shows nothing.
//!
//! So the driver takes the frame apart itself. Slots are taken by position
//! rather than by anything in the packets: a frame is a header and then exactly
//! fifty-two packets, and counting positions is what keeps one stream's packets
//! in the order they were sent.

pub use mpegts::{PACKET, SYNC};

/// The frame header rides on a packet identifier of its own.
/// The identifier the frame header packets carry.
pub const HEADER_PID: u16 = 0x2f;

/// How many slots a frame has, not counting the header that opens it.
pub const SLOTS: usize = 52;

/// The two shapes a frame header's own sync word takes.
const HEADER_SYNC: [u16; 2] = [0x1a86, 0x0579];

/// One of the streams a frame carries, as its header names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relative {
    /// Which stream it is, 1 to 15, which is what a channel names to ask for
    /// it.
    pub number: u8,
    /// The transport stream identifier it carries, which is what a channel
    /// file and a viewer's saved channels are matched by.
    pub stream_id: u16,
    pub original_network_id: u16,
    /// Whether it carries a transport stream or the newer TLV packets, which
    /// is what the MMT/TLV services use.
    pub tlv: bool,
    /// How many of the frame's fifty-two slots it was given, which is its
    /// share of the carrier.
    pub slots: usize,
}

/// How a frame is split across carriers, where it is. A cable that cannot fit a
/// frame down one carrier sends it down several: the frame says which group it
/// belongs to, how many carriers carry it and which of them this is. A channel
/// on such a frame is not identified by the carrier alone, so the group goes
/// into the channel file with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub id: u8,
    pub carriers: u8,
    pub sequence: u8,
}

/// What a carrier turns out to be carrying. A carrier with no frame on it is
/// one stream and needs nothing said; a carrier with a frame needs every stream
/// on it named.
#[derive(Debug, Default)]
pub struct Survey {
    held: Vec<u8>,
    found: Vec<Relative>,
    frames: usize,
    group: Option<Group>,
}

impl Survey {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads what has arrived, looking for frame headers.
    pub fn feed(&mut self, arrived: &[u8]) {
        self.held.extend_from_slice(arrived);
        let mut at = 0;
        while at + PACKET <= self.held.len() {
            if self.held[at] != SYNC {
                at += 1;
                continue;
            }
            if let Some(header) = parse_header(&self.held[at..at + PACKET]) {
                self.frames += 1;
                if header.group.is_some() {
                    self.group = header.group;
                }
                for stream in header.streams {
                    if !self.found.iter().any(|seen| seen.number == stream.number) {
                        self.found.push(stream);
                    }
                }
            }
            at += PACKET;
        }
        self.held.drain(..at);
    }

    /// The streams found, in the order the frame numbers them.
    pub fn streams(&self) -> Vec<Relative> {
        let mut found = self.found.clone();
        found.sort_by_key(|stream| stream.number);
        found
    }

    /// Which group of a split frame this carrier is part of, where the frame
    /// is split at all.
    pub fn group(&self) -> Option<Group> {
        self.group
    }

    /// How many frame headers went past. A carrier with none is not
    /// multiplexed -- or was not watched for long enough, which is why the
    /// count is here rather than a yes or no.
    pub fn frames(&self) -> usize {
        self.frames
    }
}

/// What comes out of a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carriage {
    /// The packets as they are, which is a transport stream.
    Transport,
    /// The TLV packets inside them, put back together. The MMT/TLV services
    /// are carried this way, and a reader of them wants the TLV rather than
    /// the packets it arrived in.
    Tlv,
}

/// The packet identifier the TLV of a frame's slot rides on.
const TLV_PID: u16 = 0x2d;

/// One frame of a multiplexing frame's carrier, as it arrived. The slots are
/// the ones the caller asked for, in the places they sat: what puts a split
/// frame back together is where a slot was, not just what was in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The group this carrier belongs to, where the frame is one of several.
    pub group: Option<u8>,
    /// Where this frame sits in its superframe, and how many that run holds.
    pub position: u8,
    pub of: u8,
    /// Which carrier of the group this is, and how many there are.
    pub sequence: u8,
    pub carriers: u8,
    /// The wanted stream's packets, by slot. Slots belonging to another stream
    /// are nothing.
    pub slots: Vec<Option<Vec<u8>>>,
}

/// Reads a carrier as whole frames, for putting a split frame back together.
#[derive(Debug)]
pub struct Frames {
    wanted: u8,
    held: Vec<u8>,
    building: Option<(Frame, [u8; SLOTS], usize)>,
}

impl Frames {
    pub fn new(wanted: u8) -> Self {
        Self {
            wanted,
            held: Vec::new(),
            building: None,
        }
    }

    /// Takes what has arrived and gives back the frames it completed.
    pub fn feed(&mut self, arrived: &[u8]) -> Vec<Frame> {
        self.held.extend_from_slice(arrived);
        let mut whole = Vec::new();
        let mut at = 0;
        while at + PACKET <= self.held.len() {
            if self.held[at] != SYNC {
                at += 1;
                continue;
            }
            let packet = &self.held[at..at + PACKET];
            if let Some(header) = parse_header(packet) {
                if let Some((frame, _, _)) = self.building.take() {
                    whole.push(frame);
                }
                let group = header.group;
                self.building = Some((
                    Frame {
                        group: group.map(|group| group.id),
                        position: header.position,
                        of: header.of,
                        sequence: group.map_or(1, |group| group.sequence),
                        carriers: group.map_or(1, |group| group.carriers),
                        slots: vec![None; SLOTS],
                    },
                    header.slots,
                    0,
                ));
            } else if let Some((frame, slots, place)) = &mut self.building
                && *place < SLOTS
            {
                if slots[*place] == self.wanted {
                    frame.slots[*place] = Some(packet.to_vec());
                }
                *place += 1;
            }
            at += PACKET;
        }
        self.held.drain(..at);
        whole
    }
}

/// Joins the TLV inside a stream of slot packets.
#[derive(Debug, Default)]
pub struct TlvJoin {
    started: bool,
}

impl TlvJoin {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one packet's TLV bytes to what has been gathered.
    pub fn take(&mut self, packet: &[u8], into: &mut Vec<u8>) {
        if let Some(bytes) = tlv_payload(packet, &mut self.started) {
            into.extend_from_slice(bytes);
        }
    }
}

/// One stream of a multiplexing frame, as it is pulled out.
#[derive(Debug)]
pub struct Splitter {
    /// Which relative stream to keep, as the frame header numbers them: 1 to
    /// 15.
    wanted: u8,
    /// What to hand back: the packets, or the TLV inside them.
    carriage: Carriage,
    /// Whether the start of a TLV packet has gone past yet. A stream is joined
    /// wherever the tuner happened to be, and the bytes before the first start
    /// are the tail of a packet whose beginning was never seen.
    started: bool,
    /// What has arrived but does not yet make a whole packet.
    held: Vec<u8>,
    /// Which relative stream each slot of the frame being read belongs to, and
    /// how far into that frame the reading has got. Nothing is kept before the
    /// first header: a stream that began mid-frame has no way of knowing whose
    /// slots it is looking at.
    frame: Option<([u8; SLOTS], usize)>,
}

impl Splitter {
    /// Takes a slot as it stands, which is what a transport stream is.
    pub fn new(wanted: u8) -> Self {
        Self::of(wanted, Carriage::Transport)
    }

    /// Takes the TLV out of a slot and puts it back together.
    pub fn tlv(wanted: u8) -> Self {
        Self::of(wanted, Carriage::Tlv)
    }

    pub fn of(wanted: u8, carriage: Carriage) -> Self {
        Self {
            wanted,
            carriage,
            started: false,
            held: Vec::new(),
            frame: None,
        }
    }

    /// Takes what has arrived and gives back what belongs to the stream.
    pub fn feed(&mut self, arrived: &[u8]) -> Vec<u8> {
        self.held.extend_from_slice(arrived);
        let mut kept = Vec::with_capacity(arrived.len());
        let mut at = 0;
        while at + PACKET <= self.held.len() {
            if self.held[at] != SYNC {
                // Not on a packet boundary. This happens once, where the
                // stream was joined, and then never again.
                at += 1;
                continue;
            }
            let packet = &self.held[at..at + PACKET];
            if let Some(slots) = header_slots(packet) {
                self.frame = Some((slots, 0));
            } else if let Some((slots, position)) = &mut self.frame {
                if *position < SLOTS {
                    if slots[*position] == self.wanted {
                        match self.carriage {
                            Carriage::Transport => kept.extend_from_slice(packet),
                            Carriage::Tlv => {
                                if let Some(bytes) = tlv_payload(packet, &mut self.started) {
                                    kept.extend_from_slice(bytes);
                                }
                            }
                        }
                    }
                    *position += 1;
                } else {
                    // The frame ran out without a header where one was due,
                    // so nothing more can be placed until the next one.
                    self.frame = None;
                }
            }
            at += PACKET;
        }
        self.held.drain(..at);
        kept
    }
}

/// A frame header, read.
struct Header {
    slots: [u8; SLOTS],
    streams: Vec<Relative>,
    group: Option<Group>,
    /// Where this frame sits in its superframe, and how long that run is.
    position: u8,
    of: u8,
}

/// The TLV bytes a slot's packet carries, or nothing where it carries none.
///
/// A packet that starts a TLV packet keeps one byte less than one that
/// continues it. That is the layout the rest of this project reads these
/// streams with -- it is what turned real MMT/TLV captures into tables that parsed
/// -- and reading them any other way moves every packet boundary.
fn tlv_payload<'a>(packet: &'a [u8], started: &mut bool) -> Option<&'a [u8]> {
    let pid = (u16::from(packet[1] & 0x1f) << 8) | u16::from(packet[2]);
    if pid != TLV_PID {
        return None;
    }
    if packet[1] & 0x40 != 0 {
        *started = true;
        return Some(&packet[4..]);
    }
    if !*started {
        return None;
    }
    Some(&packet[3..])
}

/// Which relative stream each slot of a frame belongs to, or nothing when the
/// packet is not a frame header.
fn header_slots(packet: &[u8]) -> Option<[u8; SLOTS]> {
    parse_header(packet).map(|header| header.slots)
}

/// The whole of a frame header, or nothing when the packet is not one.
fn parse_header(packet: &[u8]) -> Option<Header> {
    // A packet the transmitter marked as damaged is not taken for a header,
    // whatever its bytes happen to say.
    if packet[1] & 0x80 != 0 {
        return None;
    }
    let pid = (u16::from(packet[1] & 0x1f) << 8) | u16::from(packet[2]);
    if pid != HEADER_PID {
        return None;
    }
    // The header fills a packet's payload exactly, so a packet carrying an
    // adaptation field or no payload is not one.
    if packet[3] & 0x30 != 0x10 {
        return None;
    }
    let payload = &packet[4..];
    let sync = ((u16::from(payload[0]) << 8) | u16::from(payload[1])) & 0x1fff;
    if !HEADER_SYNC.contains(&sync) || crc32(payload) != 0 {
        return None;
    }
    // A frame of the second kind is one of several carrying one frame, and
    // says which group it belongs to.
    const SPLIT: u8 = 2;
    let group = (payload[2] & 0x0f == SPLIT).then(|| Group {
        id: payload[123],
        carriers: payload[124],
        sequence: payload[125],
    });
    let group = group.filter(|group| {
        (1..=16).contains(&group.carriers) && (1..=group.carriers).contains(&group.sequence)
    });
    let mut slots = [0u8; SLOTS];
    let mut shares = [0usize; 16];
    for (slot, kept) in slots.iter_mut().enumerate() {
        let pair = payload[69 + slot / 2];
        *kept = if slot % 2 == 0 {
            pair >> 4
        } else {
            pair & 0x0f
        };
        if (1..=15).contains(kept) {
            shares[usize::from(*kept)] += 1;
        }
    }
    // Which streams carry TLV rather than a transport stream, a bit each,
    // starting at the top.
    let kinds = (u16::from(payload[121]) << 7) | u16::from(payload[122] >> 1);
    let mut streams = Vec::new();
    for number in 1..=15u8 {
        if shares[usize::from(number)] == 0 {
            continue;
        }
        let at = 5 + (usize::from(number) - 1) * 4;
        streams.push(Relative {
            number,
            stream_id: (u16::from(payload[at]) << 8) | u16::from(payload[at + 1]),
            original_network_id: (u16::from(payload[at + 2]) << 8) | u16::from(payload[at + 3]),
            tlv: (kinds >> (15 - u16::from(number))) & 1 == 0,
            slots: shares[usize::from(number)],
        });
    }
    Some(Header {
        slots,
        streams,
        group,
        position: payload[126] & 0x0f,
        of: payload[126] >> 4,
    })
}

/// The whole of a frame header as a receiver's "stream information" shows
/// it (多重フレームヘッダ). Everything the header says, whether or not the
/// demultiplexer uses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// The sync word as sent (one of two, alternating).
    pub sync: u16,
    pub version: u8,
    /// Whether the relative stream numbers are fixed or reassigned.
    pub relative_mode: u8,
    /// One is a frame of one carrier, two a frame split across carriers,
    /// fifteen "not used".
    pub frame_type: u8,
    /// The emergency warning start signal.
    pub emergency: bool,
    /// Which group of carriers, and where this carrier sits in it, for a
    /// split frame.
    pub group: Option<Group>,
    /// Frames in the superframe, and this frame's place.
    pub frames: u8,
    pub position: u8,
    /// Every relative stream the header marks active.
    pub streams: Vec<StreamInfo>,
    pub crc: u32,
}

/// One relative stream as the header describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamInfo {
    pub number: u8,
    pub stream_id: u16,
    pub original_network_id: u16,
    pub tlv: bool,
    /// Nought normal, one poor, two not received.
    pub receive_status: u8,
    /// How many of the fifty-two slots it holds.
    pub slots: usize,
}

/// Reads the whole of a frame header out of a packet, or nothing where the
/// packet is not one.
pub fn info(packet: &[u8]) -> Option<Info> {
    if packet.len() < PACKET {
        return None;
    }
    let header = parse_header(packet)?;
    let payload = &packet[4..];
    let streams = header
        .streams
        .iter()
        .map(|stream| {
            let i = usize::from(stream.number) - 1;
            StreamInfo {
                number: stream.number,
                stream_id: stream.stream_id,
                original_network_id: stream.original_network_id,
                tlv: stream.tlv,
                receive_status: (payload[65 + i / 4] >> ((3 - (i % 4)) * 2)) & 0x03,
                slots: stream.slots,
            }
        })
        .collect();
    Some(Info {
        sync: ((u16::from(payload[0]) << 8) | u16::from(payload[1])) & 0x1fff,
        version: payload[2] >> 5,
        relative_mode: (payload[2] >> 4) & 0x01,
        frame_type: payload[2] & 0x0f,
        emergency: payload[68] & 0x01 != 0,
        group: header.group,
        frames: header.of,
        position: header.position,
        streams,
        crc: u32::from_be_bytes([payload[180], payload[181], payload[182], payload[183]]),
    })
}

/// The check the tables of a transport stream carry, which comes to zero over
/// a table that is intact.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut register: u32 = 0xFFFF_FFFF;
    for byte in bytes {
        register ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            register = if register & 0x8000_0000 != 0 {
                (register << 1) ^ 0x04C1_1DB7
            } else {
                register << 1
            };
        }
    }
    register
}

#[cfg(test)]
mod tests {
    use super::{PACKET, SLOTS, SYNC, Splitter, crc32, info};

    /// A packet of a slot that carries TLV, with the bytes a caller should
    /// get back out of it.
    fn tlv_packet(mark: u8, starts: bool) -> Vec<u8> {
        let mut packet = vec![mark; PACKET];
        packet[0] = SYNC;
        packet[1] = if starts { 0x40 } else { 0x00 };
        packet[2] = 0x2d;
        packet[3] = 0x10;
        packet
    }

    /// A packet of one relative stream, marked so a test can tell them apart.
    fn slot_packet(mark: u8) -> Vec<u8> {
        let mut packet = vec![0u8; PACKET];
        packet[0] = SYNC;
        packet[1] = 0x01;
        packet[2] = 0x00;
        packet[3] = 0x10;
        packet[4] = mark;
        packet
    }

    /// A frame header whose slots are handed out as the caller says, and whose
    /// streams are numbered as their slots are.
    fn header(slots: &[u8; SLOTS]) -> Vec<u8> {
        let mut packet = vec![0u8; PACKET];
        packet[0] = SYNC;
        packet[1] = 0x00;
        packet[2] = 0x2f;
        packet[3] = 0x10;
        let payload = &mut packet[4..];
        payload[0] = 0x1a;
        payload[1] = 0x86;
        for (slot, relative) in slots.iter().enumerate() {
            let at = 69 + slot / 2;
            if slot % 2 == 0 {
                payload[at] = (payload[at] & 0x0f) | (relative << 4);
            } else {
                payload[at] = (payload[at] & 0xf0) | (relative & 0x0f);
            }
        }
        // Each stream's identifiers, at four bytes apiece.
        for number in 1..=15usize {
            let at = 5 + (number - 1) * 4;
            payload[at] = 0x40;
            payload[at + 1] = number as u8;
            payload[at + 2] = 0x00;
            payload[at + 3] = 0x04;
        }
        // The last four bytes carry the check, which comes to zero over the
        // whole payload.
        let check = crc32(&payload[..180]);
        payload[180..184].copy_from_slice(&check.to_be_bytes());
        packet
    }

    fn frame(slots: &[u8; SLOTS]) -> Vec<u8> {
        let mut bytes = header(slots);
        for slot in slots {
            bytes.extend_from_slice(&slot_packet(*slot));
        }
        bytes
    }

    fn alternating() -> [u8; SLOTS] {
        let mut slots = [0u8; SLOTS];
        for (at, slot) in slots.iter_mut().enumerate() {
            *slot = if at % 2 == 0 { 1 } else { 2 };
        }
        slots
    }

    /// The whole header comes out for a receiver to show: version, frame
    /// type, every stream with its identifiers, and the check as sent.
    #[test]
    fn the_whole_header_is_read_for_showing() {
        let packet = header(&alternating());
        let read = info(&packet).expect("a header");
        assert_eq!(read.sync, 0x1a86);
        assert_eq!(read.frame_type, 0);
        assert!(!read.emergency);
        assert_eq!(read.streams.len(), 2);
        assert_eq!(read.streams[0].number, 1);
        assert_eq!(read.streams[0].stream_id, 0x4001);
        assert_eq!(read.streams[0].original_network_id, 4);
        assert_eq!(read.streams[0].slots, 26);
        assert_eq!(read.streams[0].receive_status, 0);
        assert_eq!(
            read.crc,
            u32::from_be_bytes([packet[184], packet[185], packet[186], packet[187]])
        );
        // Not a header at all.
        assert!(info(&[0u8; PACKET]).is_none());
    }

    /// A packet the transmitter marked as damaged is not a header, even when
    /// its bytes still check out.
    #[test]
    fn a_damaged_packet_is_not_taken_for_a_header() {
        let mut packet = header(&alternating());
        assert!(info(&packet).is_some());
        packet[1] |= 0x80;
        assert!(info(&packet).is_none());
    }

    /// A frame that is split across carriers says so, and which group it
    /// belongs to: the carrier alone does not identify a channel on it.
    #[test]
    fn a_split_frame_says_which_group_it_belongs_to() {
        let mut split = header(&alternating());
        {
            let payload = &mut split[4..];
            payload[2] = (payload[2] & 0xf0) | 2;
            payload[123] = 0x2a;
            payload[124] = 2;
            payload[125] = 1;
            let check = crc32(&payload[..180]);
            payload[180..184].copy_from_slice(&check.to_be_bytes());
        }
        let mut survey = super::Survey::new();
        survey.feed(&split);
        let group = survey.group().expect("the frame is one of a group");
        assert_eq!(group.id, 0x2a);
        assert_eq!(group.carriers, 2);
        assert_eq!(group.sequence, 1);
        // An ordinary frame belongs to no group.
        let mut alone = super::Survey::new();
        alone.feed(&frame(&alternating()));
        assert_eq!(alone.group(), None);
    }

    /// A carrier says what it carries in its own header, which is what turns
    /// a carrier into a list of channels.
    #[test]
    fn a_survey_names_the_streams_a_carrier_holds() {
        let mut survey = super::Survey::new();
        survey.feed(&frame(&alternating()));
        let streams = survey.streams();
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].number, 1);
        assert_eq!(streams[0].stream_id, 0x4001);
        assert_eq!(streams[0].original_network_id, 4);
        assert_eq!(streams[0].slots, 26);
        assert_eq!(streams[1].number, 2);
        assert_eq!(streams[1].stream_id, 0x4002);
        assert_eq!(survey.frames(), 1);
    }

    /// A carrier with no frame on it carries one stream and says nothing.
    #[test]
    fn a_carrier_with_no_frame_says_nothing() {
        let mut survey = super::Survey::new();
        for _ in 0..20 {
            survey.feed(&slot_packet(1));
        }
        assert_eq!(survey.frames(), 0);
        assert!(survey.streams().is_empty());
    }

    /// The advanced services arrive as TLV inside the slot's packets, and a
    /// reader of them wants the TLV. A packet that starts one keeps one byte
    /// less than one that continues it.
    #[test]
    fn a_tlv_slot_comes_out_as_the_tlv_inside_it() {
        let mut slots = [0u8; SLOTS];
        slots[0] = 1;
        slots[1] = 1;
        slots[2] = 2;
        let mut stream = header(&slots);
        stream.extend_from_slice(&tlv_packet(0xa1, true));
        stream.extend_from_slice(&tlv_packet(0xa2, false));
        stream.extend_from_slice(&slot_packet(2));
        for _ in 3..SLOTS {
            stream.extend_from_slice(&slot_packet(0));
        }

        let mut splitter = Splitter::tlv(1);
        let kept = splitter.feed(&stream);
        // The packet that starts a TLV packet gives 184 bytes and the one that
        // continues it 185, and no other slot is in it.
        assert_eq!(kept.len(), 184 + 185);
        assert!(kept[..184].iter().all(|byte| *byte == 0xa1));
        assert!(kept[185..].iter().all(|byte| *byte == 0xa2));
    }

    /// A stream joined part way through a TLV packet has no beginning to that
    /// packet, and keeping its tail would leave a reader finding headers in
    /// the middle of a payload.
    #[test]
    fn the_tail_of_a_tlv_packet_whose_start_was_missed_is_dropped() {
        let mut slots = [0u8; SLOTS];
        slots[0] = 1;
        slots[1] = 1;
        let mut stream = header(&slots);
        stream.extend_from_slice(&tlv_packet(0xb1, false));
        stream.extend_from_slice(&tlv_packet(0xb2, true));
        for _ in 2..SLOTS {
            stream.extend_from_slice(&slot_packet(0));
        }

        let mut splitter = Splitter::tlv(1);
        let kept = splitter.feed(&stream);
        assert_eq!(kept.len(), 184);
        assert!(kept.iter().all(|byte| *byte == 0xb2));
    }

    #[test]
    fn only_the_slots_of_the_wanted_stream_come_out() {
        let mut splitter = Splitter::new(2);
        let kept = splitter.feed(&frame(&alternating()));
        assert_eq!(kept.len(), 26 * PACKET);
        for packet in kept.chunks(PACKET) {
            assert_eq!(packet[0], SYNC);
            assert_eq!(packet[4], 2);
        }
    }

    /// The header itself is not part of any stream, and a slot marked zero
    /// belongs to none.
    #[test]
    fn the_header_and_the_empty_slots_are_left_behind() {
        let mut slots = [0u8; SLOTS];
        slots[0] = 1;
        let mut splitter = Splitter::new(1);
        assert_eq!(splitter.feed(&frame(&slots)).len(), PACKET);
    }

    /// The daemon writes whenever the card has filled a buffer, which falls
    /// wherever it falls. A packet split across two of those is still one
    /// packet.
    #[test]
    fn a_packet_split_across_two_reads_is_still_one_packet() {
        let bytes = frame(&alternating());
        let mut whole = Splitter::new(1);
        let mut piecemeal = Splitter::new(1);
        let expected = whole.feed(&bytes);
        let mut got = Vec::new();
        for piece in bytes.chunks(100) {
            got.extend(piecemeal.feed(piece));
        }
        assert_eq!(got, expected);
        assert_eq!(got.len(), 26 * PACKET);
    }

    /// A stream is joined wherever the tuner happened to be. Until a header
    /// has been seen there is no way of knowing whose slots are going past, so
    /// nothing is kept.
    #[test]
    fn nothing_is_kept_before_the_first_header() {
        let mut splitter = Splitter::new(1);
        let mut bytes = Vec::new();
        for _ in 0..10 {
            bytes.extend_from_slice(&slot_packet(1));
        }
        assert!(splitter.feed(&bytes).is_empty());
        // And once a header arrives, the frame after it comes out.
        assert_eq!(splitter.feed(&frame(&alternating())).len(), 26 * PACKET);
    }

    /// Bytes that are not on a packet boundary are walked past a byte at a
    /// time until they are, which is what joining a stream mid-packet needs.
    #[test]
    fn a_stream_joined_mid_packet_finds_the_boundary() {
        let bytes = frame(&alternating());
        let mut splitter = Splitter::new(2);
        let mut ragged = bytes[57..].to_vec();
        ragged.extend_from_slice(&frame(&alternating()));
        // The first frame's header was cut, so only the second frame's slots
        // come out.
        assert_eq!(splitter.feed(&ragged).len(), 26 * PACKET);
    }
}

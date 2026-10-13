// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Taking a transport stream apart, one packet at a time.
//!
//! A multiplex is a single run of fixed-length packets carrying every stream of
//! every programme interleaved. A demultiplexer sorts them back out by packet
//! identifier, puts the tables back together section by section and the
//! elementary streams back together packet by packet, and hands each whole
//! thing on as it completes.
//!
//! What it does not do is read the tables it hands on: the map of a programme
//! means one thing under one broadcasting standard and another under the next.
//! The association table is the exception, because without reading it the
//! demultiplexer would not know where anything else is.

use std::collections::{BTreeMap, HashMap};

use crate::pat::Program;
use crate::sections::Sections;
use crate::{PACKET, SYNC, pes};

/// The identifier the association table rides on, which is the first there
/// is.
pub const PAT_PID: u16 = 0x0000;

/// The identifier that carries nothing: the stuffing a multiplex is padded to
/// its rate with, which is dropped before anything else looks at it.
pub const NULL_PID: u16 = 0x1FFF;

const TABLE_PAT: u8 = 0x00;
const TABLE_PMT: u8 = 0x02;

/// Whether the identifier is one the standards fix in advance, so that its
/// sections can be read without waiting for a table to say they are there: the
/// association table, conditional access, the network and service information
/// tables, the time and date, and the identifiers a broadcaster's own tables
/// are pinned to.
pub const fn is_fixed_table_pid(pid: u16) -> bool {
    matches!(pid, 0x0000..=0x001F | 0x0023..=0x0029 | 0x1FC8..=0x1FCF)
}

/// Whether a programme map's stream type means the identifier carries sections
/// rather than an elementary stream. A programme map does not say so directly,
/// so the caller reads the type and tells the demultiplexer with
/// [`Demuxer::expect`]. Data broadcasting rides in DSM-CC sections.
pub const fn carries_sections(stream_type: u8) -> bool {
    // DSM-CC in its four shapes, of which a broadcast here uses the carousel.
    matches!(stream_type, 0x0A..=0x0D)
}

/// The clock a programme is timed against, as the adaptation field sends it: a
/// count of the ninety kilohertz clock and, under it, one of the twenty-seven
/// megahertz clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pcr {
    pub base: u64,
    pub extension: u16,
}

impl Pcr {
    /// The clock in the units it is really kept in.
    pub fn to_27mhz(self) -> u64 {
        self.base * 300 + u64::from(self.extension)
    }

    /// The clock in the units the timestamps of an elementary stream use, so
    /// that the two can be compared.
    pub fn to_90khz(self) -> u64 {
        self.base
    }
}

/// One whole section of a table, checked and handed on undecoded.
#[derive(Debug, Clone, Copy)]
pub struct Section<'a> {
    pub pid: u16,
    /// Which table this is a section of, which is the section's first byte.
    pub table_id: u8,
    /// Whether the association table named this identifier as carrying a
    /// programme map. What such a section means is for a caller that knows the
    /// standard to say.
    pub is_program_map: bool,
    /// The section as sent, header and check included.
    pub data: &'a [u8],
}

/// The association table, which is the one table read here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pat {
    /// Which stream this is.
    pub stream_id: u16,
    /// The programmes, and where each one's map is.
    pub programs: Vec<Program>,
    /// Where the network's own table is, where the stream said.
    pub network_pid: Option<u16>,
}

/// One whole packet of an elementary stream, put back together from as many
/// transport packets as it took.
#[derive(Debug, Clone, Copy)]
pub struct Pes<'a> {
    pub pid: u16,
    pub header: pes::Header,
    /// Whether the transport packet the packet started in was marked as a
    /// point the decoder may start from.
    pub random_access: bool,
    /// Whether the transmitter said the count was deliberately broken before
    /// this packet, so that what came before it does not join to it.
    pub discontinuity: bool,
    /// How many packets of this identifier were missed since the last one was
    /// handed on. Anything above nought means the payload has a hole in it.
    pub lost: u32,
    /// Whether the packet ran to the length it declared. False where it was cut
    /// short -- the stream ended, or a new packet started before this one
    /// finished.
    pub complete: bool,
    /// The bytes of the elementary stream, header stripped.
    pub payload: &'a [u8],
}

/// What a demultiplexer hands its output to. Every method does nothing unless
/// it is written, so a caller that wants only sound writes only the one it
/// wants.
pub trait Handler {
    /// The association table, each time a whole one has arrived. A table sent
    /// in several sections is handed over once, when the last of them lands.
    /// What arrives is the transmission's list as it now stands: a programme no
    /// longer named has left.
    fn on_pat(&mut self, _pat: &Pat) {}

    /// A section of any other table, checked but undecoded.
    fn on_section(&mut self, _section: &Section<'_>) {}

    /// A whole packet of an elementary stream.
    fn on_pes(&mut self, _pes: &Pes<'_>) {}

    /// The programme clock, each time a packet carries it.
    fn on_pcr(&mut self, _pid: u16, _pcr: Pcr) {}
}

/// A handler that does nothing, for pushing packets through only to count
/// what went past.
#[derive(Debug, Default, Clone, Copy)]
pub struct Ignore;

impl Handler for Ignore {}

/// What a packet is expected to carry, once anything is known about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carries {
    /// Sections of a table.
    Sections,
    /// Packets of an elementary stream.
    Pes,
    /// Packets of an elementary stream nobody is reading: counted (arrivals,
    /// losses, damage, scrambling) but not assembled.
    Count,
}

/// The most a unit is gathered to. A unit that declares no length -- which
/// video units may -- ends when the next one starts; the largest picture a
/// broadcast carries is a few megabytes.
const MOST_BODY: usize = 32 * 1024 * 1024;

/// A packetised elementary stream packet that has begun and not yet ended.
#[derive(Debug)]
struct Open {
    header: pes::Header,
    random_access: bool,
    discontinuity: bool,
    lost: u32,
    /// How long the payload will be, where the packet declared a length.
    wanted: Option<usize>,
    body: Vec<u8>,
}

/// What arrived on one packet identifier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counted {
    /// Packets that arrived at all, damaged ones included.
    pub arrived: u64,
    /// Packets handed on to be assembled.
    pub passed: u64,
    /// Packets the continuity counters say were missed.
    pub lost: u64,
    /// Packets the transmitter marked as known to be damaged.
    pub errors: u64,
    /// Packets that arrived under conditional access and were not read.
    pub scrambled: u64,
}

/// What is known and what is half-assembled for one packet identifier.
#[derive(Debug)]
struct Stream {
    sections: Option<Sections>,
    is_program_map: bool,
    /// The counter last seen, or nothing where no packet of this identifier
    /// has arrived yet.
    counter: Option<u8>,
    /// How many packets of this identifier arrived at all, damaged ones
    /// included.
    ///
    /// **A receiver showing where a transmission is failing needs this per
    /// identifier**, not only over the whole stream: a fault in one service's
    /// picture and a fault in the whole multiplex look the same in a single
    /// total.
    arrived: u64,
    /// How many were handed on to be assembled -- neither damaged, nor
    /// scrambled, nor payloadless.
    passed: u64,
    /// How many the transmitter marked as known to be damaged.
    errors: u64,
    lost: u64,
    scrambled: u64,
    /// What was missed since the last elementary stream packet was handed on,
    /// which is reported with the next one.
    pending_lost: u32,
    /// Counted only, never assembled ([`Carries::Count`]).
    counted_only: bool,
    open: Option<Open>,
}

impl Stream {
    fn new(pid: u16) -> Self {
        Self {
            sections: is_fixed_table_pid(pid).then(|| Sections::new(pid)),
            is_program_map: false,
            counter: None,
            arrived: 0,
            passed: 0,
            errors: 0,
            lost: 0,
            scrambled: 0,
            pending_lost: 0,
            open: None,
            counted_only: false,
        }
    }

    /// How many packets the counter says were missed between the last one and
    /// this. A packet with no payload does not step the counter, a packet sent
    /// twice repeats it, and neither is a loss; nor is a step the transmitter
    /// announced.
    fn continuity(&mut self, counter: u8, has_payload: bool, discontinuity: bool) -> u32 {
        let previous = self.counter.replace(counter);
        let Some(previous) = previous else {
            return 0;
        };
        if discontinuity || !has_payload || counter == previous {
            return 0;
        }
        u32::from(counter.wrapping_sub(previous).wrapping_sub(1) & 0x0F)
    }
}

/// Sorts a transport stream back out into the tables and streams it carries.
#[derive(Debug, Default)]
pub struct Demuxer {
    streams: HashMap<u16, Stream>,
    program_map_pids: Vec<u16>,
    programs: Vec<Program>,
    /// Which version of the association table is being put back together.
    pat_version: Option<u8>,
    /// The programmes of each section of it that has arrived, by section
    /// number.
    ///
    /// **A table sent in several sections cannot be taken a section at a
    /// time.** Replacing what is known from one of them would drop every
    /// programme the others named, so the sections are held until there are as
    /// many as the table said.
    pat_parts: BTreeMap<u8, Vec<Program>>,
    /// Where the network's own table is, as the sections of this version said.
    pat_network: Option<u16>,
    packets: u64,
    lost: u64,
    scrambled: u64,
    /// How many packets the transmitter marked as known to be damaged.
    errors: u64,
}

impl Demuxer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Says in advance what an identifier carries, for what the demultiplexer
    /// cannot work out on its own: the stream types of a programme map say
    /// which identifiers carry data sections, and reading the map is the
    /// caller's job.
    pub fn expect(&mut self, pid: u16, carries: Carries) {
        let stream = self.streams.entry(pid).or_insert_with(|| Stream::new(pid));
        match carries {
            Carries::Sections => {
                stream.counted_only = false;
                if stream.sections.is_none() {
                    stream.sections = Some(Sections::new(pid));
                }
            }
            Carries::Pes => {
                stream.counted_only = false;
                stream.sections = None;
            }
            Carries::Count => {
                stream.counted_only = true;
                stream.sections = None;
                stream.open = None;
            }
        }
    }

    /// Takes one transport packet and hands on whatever it completed.
    pub fn push<H: Handler + ?Sized>(&mut self, packet: &[u8; PACKET], handler: &mut H) {
        if packet[0] != SYNC {
            return;
        }
        let pid = (u16::from(packet[1] & 0x1F) << 8) | u16::from(packet[2]);
        if pid == NULL_PID {
            return;
        }
        // The transmitter saying the packet is known to be damaged. Its
        // counter is no more to be trusted than its payload.
        //
        // **It is counted before it is dropped.** Which identifier is taking
        // the damage is the most useful number there is when a receiver is
        // showing why a picture is breaking up, and the identifier itself is
        // read out of the header rather than the payload: it is as good as the
        // sync byte that got this far.
        if packet[1] & 0x80 != 0 {
            let stream = self.streams.entry(pid).or_insert_with(|| Stream::new(pid));
            stream.arrived += 1;
            stream.errors += 1;
            self.errors += 1;
            // Its counter cannot be read, so it is taken to have used the next
            // number: the packet after it is not counted as missing the same
            // packet a second time.
            stream.counter = stream.counter.map(|counter| (counter + 1) & 0x0F);
            return;
        }
        self.packets += 1;
        self.streams
            .entry(pid)
            .or_insert_with(|| Stream::new(pid))
            .arrived += 1;

        let control = packet[3];
        let has_payload = control & 0x10 != 0;
        let mut at = 4;
        let mut discontinuity = false;
        let mut random_access = false;
        if control & 0x20 != 0 {
            let length = usize::from(packet[4]);
            if 5 + length > PACKET {
                return;
            }
            if length > 0 {
                let flags = packet[5];
                discontinuity = flags & 0x80 != 0;
                random_access = flags & 0x40 != 0;
                // The clock, where the field says it is there and is long
                // enough to hold it.
                if flags & 0x10 != 0 && length >= 7 {
                    handler.on_pcr(pid, read_pcr(&packet[6..12]));
                }
            }
            at = 5 + length;
        }

        let stream = self.streams.entry(pid).or_insert_with(|| Stream::new(pid));
        let lost = stream.continuity(control & 0x0F, has_payload, discontinuity);
        if lost > 0 {
            stream.lost += u64::from(lost);
            stream.pending_lost = stream.pending_lost.saturating_add(lost);
            self.lost += u64::from(lost);
        }
        if !has_payload || at >= PACKET {
            return;
        }
        // A packet whose scrambling control is set does not carry readable
        // bytes, so it is neither assembled nor read. It is counted, because
        // how much could not be read is worth knowing.
        //
        // **A packet being assembled loses its unit.** The continuity counter
        // still advances over such a packet, so nothing else would notice the
        // hole, and the readable packets after it would be appended to a body
        // missing 184 bytes in the middle. A decoder given that reads the
        // wrong bits as syntax and refuses the whole unit with a message about
        // something else -- measured as `gain control data` from the AAC
        // decoder at the head of a stream whose first packets still had the
        // control set. The next packet that starts a unit starts afresh, and
        // the unit that was dropped is counted as lost.
        if control & 0xC0 != 0 {
            stream.scrambled += 1;
            self.scrambled += 1;
            if stream.open.take().is_some() {
                stream.pending_lost = stream.pending_lost.saturating_add(1);
            }
            return;
        }

        stream.passed += 1;
        if stream.counted_only {
            return;
        }

        if stream.sections.is_some() {
            self.sections(pid, packet, handler);
            return;
        }
        let starts = packet[1] & 0x40 != 0;
        self.pes(
            pid,
            starts,
            at,
            packet,
            discontinuity,
            random_access,
            handler,
        );
    }

    /// Hands on every elementary stream packet still open, which is what the
    /// end of a stream leaves behind: a packet that declared no length ends
    /// where the next one starts, and the last one has no next one.
    pub fn flush<H: Handler + ?Sized>(&mut self, handler: &mut H) {
        let pids: Vec<u16> = self.streams.keys().copied().collect();
        for pid in pids {
            self.close(pid, handler);
        }
    }

    /// How many packets went past, stuffing aside.
    pub fn packets(&self) -> u64 {
        self.packets
    }

    /// How many packets the counters say were missed, over the whole stream.
    pub fn lost(&self) -> u64 {
        self.lost
    }

    /// How many packets were scrambled, over the whole stream.
    pub fn scrambled(&self) -> u64 {
        self.scrambled
    }

    /// How many packets the transmitter marked as damaged, over the whole
    /// stream.
    pub fn errors(&self) -> u64 {
        self.errors
    }

    /// What one identifier carried: how many packets arrived, how many were
    /// handed on to be assembled, how many the counters say were missed, and
    /// how many arrived marked as damaged.
    ///
    /// **This is what a receiver's stream information shows**, a row per
    /// identifier. Nothing for an identifier nothing has arrived on.
    ///
    /// An identifier a programme map named is known before anything arrives on
    /// it, and that is not the same as having arrived: a row of noughts reads
    /// as a stream that is failing, where the truth is that it has not started.
    pub fn counted_on(&self, pid: u16) -> Option<Counted> {
        self.streams
            .get(&pid)
            .filter(|stream| stream.arrived > 0)
            .map(|stream| Counted {
                arrived: stream.arrived,
                passed: stream.passed,
                lost: stream.lost,
                errors: stream.errors,
                scrambled: stream.scrambled,
            })
    }

    /// Every identifier something has arrived on, smallest first.
    ///
    /// **Sorted rather than in the order they were met.** They are held in a
    /// hash map, so the order they come out in is not an order at all, and a
    /// list of identifiers that reshuffles itself between one look and the
    /// next is unreadable.
    pub fn identifiers(&self) -> Vec<u16> {
        let mut pids: Vec<u16> = self
            .streams
            .iter()
            .filter(|(_, stream)| stream.arrived > 0)
            .map(|(pid, _)| *pid)
            .collect();
        pids.sort_unstable();
        pids
    }

    /// How many packets of one identifier were missed.
    pub fn lost_on(&self, pid: u16) -> u64 {
        self.streams.get(&pid).map_or(0, |stream| stream.lost)
    }

    /// How many packets of one identifier were scrambled.
    pub fn scrambled_on(&self, pid: u16) -> u64 {
        self.streams.get(&pid).map_or(0, |stream| stream.scrambled)
    }

    /// The identifiers the association table named as carrying programme
    /// maps, in the order it named them.
    pub fn program_map_pids(&self) -> &[u16] {
        &self.program_map_pids
    }

    /// The programmes the association table named.
    pub fn programs(&self) -> &[Program] {
        &self.programs
    }

    fn sections<H: Handler + ?Sized>(&mut self, pid: u16, packet: &[u8; PACKET], handler: &mut H) {
        let Some(stream) = self.streams.get_mut(&pid) else {
            return;
        };
        let Some(sections) = stream.sections.as_mut() else {
            return;
        };
        let is_program_map = stream.is_program_map;
        for section in sections.feed(packet) {
            let table_id = section[0];
            if pid == PAT_PID && table_id == TABLE_PAT {
                self.pat(&section, handler);
                continue;
            }
            handler.on_section(&Section {
                pid,
                table_id,
                is_program_map: is_program_map && table_id == TABLE_PMT,
                data: &section,
            });
        }
    }

    /// The one table read here, because where everything else is is in it.
    ///
    /// **The table replaces what was known rather than adding to it.** A
    /// programme is taken off the air by no longer being named, and nothing
    /// else says so. See [`Demuxer::pat_parts`] for why the replacing waits for
    /// the whole table.
    fn pat<H: Handler + ?Sized>(&mut self, section: &[u8], handler: &mut H) {
        if section.len() < 12 {
            return;
        }
        let version = (section[5] >> 1) & 0x1F;
        let number = section[6];
        let last = section[7];
        let mut programs = Vec::new();
        let mut network_pid = None;
        let end = section.len() - 4;
        let mut at = 8;
        while at + 4 <= end {
            let program = (u16::from(section[at]) << 8) | u16::from(section[at + 1]);
            let pid = (u16::from(section[at + 2] & 0x1F) << 8) | u16::from(section[at + 3]);
            at += 4;
            // Programme nought is not a programme: it says where the
            // network's own table is.
            if program == 0 {
                network_pid = Some(pid);
                continue;
            }
            programs.push(Program {
                number: program,
                pid,
            });
        }
        if self.pat_version != Some(version) {
            self.pat_version = Some(version);
            self.pat_parts.clear();
            self.pat_network = None;
        }
        if network_pid.is_some() {
            self.pat_network = network_pid;
        }
        self.pat_parts.insert(number, programs);
        // Every section of the table, or nothing yet. The count is enough
        // because the sections are held by number.
        if self.pat_parts.len() <= usize::from(last) {
            return;
        }
        let programs: Vec<Program> = self.pat_parts.values().flatten().copied().collect();
        let named: Vec<u16> = programs.iter().map(|program| program.pid).collect();
        for pid in std::mem::take(&mut self.program_map_pids) {
            if named.contains(&pid) {
                continue;
            }
            // This identifier no longer carries a map. What is half-assembled
            // on it is abandoned: it belongs to a programme that is no longer
            // named.
            if let Some(stream) = self.streams.get_mut(&pid) {
                stream.is_program_map = false;
                if !is_fixed_table_pid(pid) {
                    stream.sections = None;
                }
            }
        }
        for &pid in &named {
            let stream = self.streams.entry(pid).or_insert_with(|| Stream::new(pid));
            if stream.sections.is_none() {
                stream.sections = Some(Sections::new(pid));
            }
            stream.is_program_map = true;
        }
        self.program_map_pids = named;
        self.programs.clone_from(&programs);
        handler.on_pat(&Pat {
            stream_id: (u16::from(section[3]) << 8) | u16::from(section[4]),
            programs,
            network_pid: self.pat_network,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn pes<H: Handler + ?Sized>(
        &mut self,
        pid: u16,
        starts: bool,
        at: usize,
        packet: &[u8; PACKET],
        discontinuity: bool,
        random_access: bool,
        handler: &mut H,
    ) {
        if starts {
            // A packet that declared no length ends where the next one
            // begins, so the next one beginning is what ends it.
            self.close(pid, handler);
            let payload = &packet[at..];
            let Some(header) = pes::Header::parse(payload) else {
                return;
            };
            let Some(stream) = self.streams.get_mut(&pid) else {
                return;
            };
            let lost = std::mem::take(&mut stream.pending_lost);
            stream.open = Some(Open {
                header,
                random_access,
                discontinuity,
                lost,
                wanted: header.payload_len(),
                body: payload[header.payload_at..].to_vec(),
            });
        } else {
            let Some(stream) = self.streams.get_mut(&pid) else {
                return;
            };
            let Some(open) = stream.open.as_mut() else {
                return;
            };
            if open.body.len() + (PACKET - at) <= MOST_BODY {
                open.body.extend_from_slice(&packet[at..]);
            } else {
                // A unit that never says where it ends and never ends is not
                // one a decoder could take; it is dropped and counted as lost
                // rather than held without bound.
                stream.open = None;
                stream.pending_lost = stream.pending_lost.saturating_add(1);
                return;
            }
        }
        // A packet that declared a length is whole as soon as it has that
        // many bytes, and need not wait for the next one to start.
        let full = self
            .streams
            .get(&pid)
            .and_then(|stream| stream.open.as_ref())
            .is_some_and(|open| open.wanted.is_some_and(|wanted| open.body.len() >= wanted));
        if full {
            self.close(pid, handler);
        }
    }

    fn close<H: Handler + ?Sized>(&mut self, pid: u16, handler: &mut H) {
        let Some(stream) = self.streams.get_mut(&pid) else {
            return;
        };
        let Some(mut open) = stream.open.take() else {
            return;
        };
        let complete = match open.wanted {
            Some(wanted) => {
                if open.body.len() > wanted {
                    open.body.truncate(wanted);
                }
                open.body.len() == wanted
            }
            None => true,
        };
        handler.on_pes(&Pes {
            pid,
            header: open.header,
            random_access: open.random_access,
            discontinuity: open.discontinuity,
            lost: open.lost,
            complete,
            payload: &open.body,
        });
    }
}

/// The clock out of the six bytes the adaptation field spreads it over.
fn read_pcr(bytes: &[u8]) -> Pcr {
    let base = (u64::from(bytes[0]) << 25)
        | (u64::from(bytes[1]) << 17)
        | (u64::from(bytes[2]) << 9)
        | (u64::from(bytes[3]) << 1)
        | u64::from(bytes[4] >> 7);
    let extension = (u16::from(bytes[4] & 0x01) << 8) | u16::from(bytes[5]);
    Pcr { base, extension }
}

#[cfg(test)]
mod tests {
    use super::{Carries, Demuxer, Handler, Pat, Pcr, Pes, Section};
    use crate::{PACKET, SYNC, crc32, pes};

    /// What the demultiplexer handed on, kept for the test to look at.
    #[derive(Debug, Default)]
    struct Kept {
        pats: Vec<Pat>,
        sections: Vec<(u16, u8, bool, Vec<u8>)>,
        pes: Vec<(u16, pes::Header, u32, bool, Vec<u8>)>,
        pcrs: Vec<(u16, Pcr)>,
    }

    impl Handler for Kept {
        fn on_pat(&mut self, pat: &Pat) {
            self.pats.push(pat.clone());
        }

        fn on_section(&mut self, section: &Section<'_>) {
            self.sections.push((
                section.pid,
                section.table_id,
                section.is_program_map,
                section.data.to_vec(),
            ));
        }

        fn on_pes(&mut self, unit: &Pes<'_>) {
            self.pes.push((
                unit.pid,
                unit.header,
                unit.lost,
                unit.complete,
                unit.payload.to_vec(),
            ));
        }

        fn on_pcr(&mut self, pid: u16, pcr: Pcr) {
            self.pcrs.push((pid, pcr));
        }
    }

    /// One transport packet: the identifier, whether it starts a unit, the
    /// counter, and as much of the payload as fits.
    fn packet(pid: u16, starts: bool, counter: u8, payload: &[u8]) -> [u8; PACKET] {
        let mut out = [0xFFu8; PACKET];
        out[0] = SYNC;
        out[1] = ((pid >> 8) as u8) & 0x1F | if starts { 0x40 } else { 0x00 };
        out[2] = (pid & 0xFF) as u8;
        out[3] = 0x10 | (counter & 0x0F);
        assert!(payload.len() <= PACKET - 4);
        if payload.len() < PACKET - 4 {
            // Not a whole packet's worth, so the rest is stuffed out with an
            // adaptation field, which is how a real multiplexer does it.
            let stuffing = PACKET - 4 - payload.len();
            out[3] |= 0x20;
            out[4] = (stuffing - 1) as u8;
            if stuffing >= 2 {
                out[5] = 0x00;
            }
            out[4 + stuffing..].copy_from_slice(payload);
        } else {
            out[4..].copy_from_slice(payload);
        }
        out
    }

    /// A packetised elementary stream packet, with stamps where asked for.
    fn unit(stream_id: u8, pts: Option<u64>, dts: Option<u64>, payload: &[u8]) -> Vec<u8> {
        let mut optional = Vec::new();
        let mut flags = 0u8;
        if let Some(pts) = pts {
            flags |= 0x80;
            let prefix = if dts.is_some() { 0x3 } else { 0x2 };
            optional.extend_from_slice(&pes::write_timestamp(prefix, pts));
        }
        if let Some(dts) = dts {
            flags |= 0x40;
            optional.extend_from_slice(&pes::write_timestamp(0x1, dts));
        }
        let mut out = vec![0x00, 0x00, 0x01, stream_id, 0x00, 0x00, 0x84, flags];
        out.push(optional.len() as u8);
        out.extend_from_slice(&optional);
        out.extend_from_slice(payload);
        let length = out.len() - 6;
        out[4] = (length >> 8) as u8;
        out[5] = (length & 0xFF) as u8;
        out
    }

    /// Cuts a unit up into transport packets of one identifier.
    fn carry(pid: u16, counter: &mut u8, unit: &[u8]) -> Vec<[u8; PACKET]> {
        let mut out = Vec::new();
        let mut rest = unit;
        let mut starts = true;
        while !rest.is_empty() {
            let take = rest.len().min(PACKET - 4);
            out.push(packet(pid, starts, *counter, &rest[..take]));
            *counter = (*counter + 1) & 0x0F;
            starts = false;
            rest = &rest[take..];
        }
        out
    }

    fn pat_section(stream_id: u16, entries: &[(u16, u16)]) -> Vec<u8> {
        pat_section_at(stream_id, 0, 0, 0, entries)
    }

    /// The same, under a version, as one section of a table of several.
    fn pat_section_at(
        stream_id: u16,
        version: u8,
        number: u8,
        last: u8,
        entries: &[(u16, u16)],
    ) -> Vec<u8> {
        let mut section = vec![0x00, 0x00, 0x00];
        section.extend_from_slice(&stream_id.to_be_bytes());
        section.extend_from_slice(&[0xC1 | (version << 1), number, last]);
        for (number, pid) in entries {
            section.extend_from_slice(&number.to_be_bytes());
            section.extend_from_slice(&(pid | 0xE000).to_be_bytes());
        }
        let length = section.len() + 4 - 3;
        section[1] = 0xB0 | ((length >> 8) as u8 & 0x0F);
        section[2] = (length & 0xFF) as u8;
        section.extend_from_slice(&crc32(&section).to_be_bytes());
        section
    }

    /// Any section at all, under a table identifier, with its check on it.
    fn section(table_id: u8, body: &[u8]) -> Vec<u8> {
        let mut section = vec![table_id, 0x00, 0x00];
        section.extend_from_slice(body);
        let length = section.len() + 4 - 3;
        section[1] = 0xB0 | ((length >> 8) as u8 & 0x0F);
        section[2] = (length & 0xFF) as u8;
        section.extend_from_slice(&crc32(&section).to_be_bytes());
        section
    }

    /// A section in a packet, behind the pointer byte that says where it
    /// starts.
    fn section_packet(pid: u16, counter: u8, section: &[u8]) -> [u8; PACKET] {
        let mut payload = vec![0x00];
        payload.extend_from_slice(section);
        packet(pid, true, counter, &payload)
    }

    #[test]
    fn a_unit_split_across_packets_comes_back_whole() {
        let body: Vec<u8> = (0..500u32).map(|byte| byte as u8).collect();
        let sent = unit(0xE0, Some(90_000), Some(87_000), &body);
        let mut counter = 0u8;
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.expect(0x0100, Carries::Pes);
        for packet in carry(0x0100, &mut counter, &sent) {
            demuxer.push(&packet, &mut kept);
        }
        assert_eq!(kept.pes.len(), 1);
        let (pid, header, lost, complete, payload) = &kept.pes[0];
        assert_eq!(*pid, 0x0100);
        assert_eq!(header.stream_id, 0xE0);
        assert_eq!(header.pts, Some(90_000));
        assert_eq!(header.dts, Some(87_000));
        assert!(header.data_alignment);
        assert_eq!(*lost, 0);
        assert!(*complete);
        assert_eq!(payload, &body);
    }

    /// A video access unit is too long for the length field, so the field is
    /// left empty and the unit ends when the next one begins.
    #[test]
    fn a_unit_of_unstated_length_ends_when_the_next_one_starts() {
        let body: Vec<u8> = (0..400u32).map(|byte| byte as u8).collect();
        let mut sent = unit(0xE0, Some(1000), None, &body);
        sent[4] = 0;
        sent[5] = 0;
        let mut counter = 0u8;
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.expect(0x0100, Carries::Pes);
        for packet in carry(0x0100, &mut counter, &sent) {
            demuxer.push(&packet, &mut kept);
        }
        // Nothing yet: the multiplexer never said how long it was.
        assert!(kept.pes.is_empty());
        for packet in carry(0x0100, &mut counter, &sent) {
            demuxer.push(&packet, &mut kept);
        }
        assert_eq!(kept.pes.len(), 1);
        assert_eq!(kept.pes[0].4, body);
        // And the last one comes out when the stream ends.
        demuxer.flush(&mut kept);
        assert_eq!(kept.pes.len(), 2);
        assert_eq!(kept.pes[1].4, body);
    }

    /// The counter says what did not arrive, and the unit that follows the
    /// hole says so.
    #[test]
    fn a_skipped_counter_is_reported_with_the_next_unit() {
        let sent = unit(0xE0, Some(1), None, &[7; 40]);
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.expect(0x0100, Carries::Pes);
        let mut counter = 0u8;
        for packet in carry(0x0100, &mut counter, &sent) {
            demuxer.push(&packet, &mut kept);
        }
        // Three packets of this identifier never arrived.
        counter = (counter + 3) & 0x0F;
        for packet in carry(0x0100, &mut counter, &sent) {
            demuxer.push(&packet, &mut kept);
        }
        assert_eq!(demuxer.lost(), 3);
        assert_eq!(demuxer.lost_on(0x0100), 3);
        assert_eq!(kept.pes.len(), 2);
        assert_eq!(kept.pes[0].2, 0);
        assert_eq!(kept.pes[1].2, 3);
    }

    /// An identifier nobody reads is counted, losses included, but nothing is assembled.
    #[test]
    fn a_counted_only_identifier_is_counted_and_not_assembled() {
        let sent = unit(0xE0, Some(1), None, &[7; 40]);
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.expect(0x0100, Carries::Count);
        let mut counter = 0u8;
        for packet in carry(0x0100, &mut counter, &sent) {
            demuxer.push(&packet, &mut kept);
        }
        counter = (counter + 2) & 0x0F;
        for packet in carry(0x0100, &mut counter, &sent) {
            demuxer.push(&packet, &mut kept);
        }
        assert!(kept.pes.is_empty());
        assert_eq!(demuxer.lost_on(0x0100), 2);
        assert!(
            demuxer
                .counted_on(0x0100)
                .is_some_and(|counted| counted.arrived > 0)
        );
        // 読み始めたら、次の単位から組み立てる。
        demuxer.expect(0x0100, Carries::Pes);
        for packet in carry(0x0100, &mut counter, &sent) {
            demuxer.push(&packet, &mut kept);
        }
        demuxer.flush(&mut kept);
        assert_eq!(kept.pes.len(), 1);
    }

    /// A packet sent twice repeats its counter, which is not a loss.
    #[test]
    fn a_repeated_counter_is_not_a_loss() {
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.expect(0x0100, Carries::Pes);
        for counter in [0u8, 1, 1, 2] {
            demuxer.push(&packet(0x0100, false, counter, &[0; 20]), &mut kept);
        }
        assert_eq!(demuxer.lost(), 0);
    }

    /// A packet with no payload does not step the counter either.
    #[test]
    fn a_packet_without_payload_does_not_step_the_counter() {
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        let mut empty = packet(0x0100, false, 0, &[0; 20]);
        // Adaptation field only.
        empty[3] = 0x20;
        empty[4] = 183;
        demuxer.push(&packet(0x0100, false, 0, &[0; 20]), &mut kept);
        demuxer.push(&empty, &mut kept);
        demuxer.push(&packet(0x0100, false, 1, &[0; 20]), &mut kept);
        assert_eq!(demuxer.lost(), 0);
    }

    /// Scrambled bytes are not the bytes that were sent, so they are counted
    /// and dropped rather than assembled into something they are not.
    #[test]
    fn scrambled_payloads_are_counted_and_not_assembled() {
        let sent = unit(0xE0, Some(1), None, &[7; 40]);
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.expect(0x0100, Carries::Pes);
        let mut counter = 0u8;
        for mut packet in carry(0x0100, &mut counter, &sent) {
            packet[3] |= 0x80;
            demuxer.push(&packet, &mut kept);
        }
        assert!(kept.pes.is_empty());
        assert_eq!(demuxer.scrambled(), 1);
        assert_eq!(demuxer.scrambled_on(0x0100), 1);
        // And the counter is still followed, so a scrambled run is not also
        // reported as a lost one.
        assert_eq!(demuxer.lost(), 0);
    }

    /// A scrambled packet in the middle of a unit takes the unit with it: the
    /// clear packets after it are not appended to a body with a hole, and the
    /// next unit starts afresh, carrying the loss.
    #[test]
    fn a_scrambled_packet_in_the_middle_drops_the_unit_being_assembled() {
        // Two units, each long enough to take three packets.
        let first = unit(0xE0, Some(1), None, &[7; 400]);
        let second = unit(0xE0, Some(2), None, &[8; 400]);
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.expect(0x0100, Carries::Pes);
        let mut counter = 0u8;
        for (n, mut packet) in carry(0x0100, &mut counter, &first).into_iter().enumerate() {
            if n == 1 {
                packet[3] |= 0x80;
            }
            demuxer.push(&packet, &mut kept);
        }
        for packet in carry(0x0100, &mut counter, &second) {
            demuxer.push(&packet, &mut kept);
        }
        // A packet with no length ends when the next starts; the second is
        // closed by asking.
        demuxer.flush(&mut kept);
        let delivered: Vec<u64> = kept.pes.iter().filter_map(|p| p.1.pts).collect();
        assert_eq!(delivered, vec![2], "the holed unit was not handed on");
        // The dropped unit is counted as lost on the one after it.
        assert_eq!(kept.pes[0].2, 1);
        assert_eq!(demuxer.scrambled_on(0x0100), 1);
    }

    /// A programme the table stops naming has left the transmission. Nothing
    /// else says so, and a demultiplexer that only added would go on assembling
    /// the map of a programme that is off the air.
    #[test]
    fn a_programme_the_table_stops_naming_is_dropped() {
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.push(
            &section_packet(0x0000, 0, &pat_section(1, &[(101, 0x0101), (102, 0x0102)])),
            &mut kept,
        );
        assert_eq!(demuxer.program_map_pids(), &[0x0101, 0x0102]);

        // The map of the programme that is about to go, which arrives while
        // it is still named.
        demuxer.push(
            &section_packet(0x0102, 0, &section(0x02, &[0, 102])),
            &mut kept,
        );
        assert!(
            kept.sections
                .iter()
                .any(|(pid, _, is_map, _)| *pid == 0x0102 && *is_map)
        );
        kept.sections.clear();

        demuxer.push(
            &section_packet(0x0000, 1, &pat_section_at(1, 1, 0, 0, &[(101, 0x0101)])),
            &mut kept,
        );
        assert_eq!(demuxer.program_map_pids(), &[0x0101]);
        assert_eq!(demuxer.programs().len(), 1);
        assert_eq!(kept.pats.last().expect("a table").programs.len(), 1);

        // The same map again, on an identifier the table no longer names.
        demuxer.push(
            &section_packet(0x0102, 1, &section(0x02, &[0, 102])),
            &mut kept,
        );
        assert!(
            kept.sections.is_empty(),
            "a map was still read on an identifier the table dropped: {:?}",
            kept.sections
        );
    }

    /// A table sent in several sections is taken once it is whole. Taking it a
    /// section at a time would drop every programme the other sections named,
    /// which would look like the transmission going off the air every time the
    /// table went round.
    #[test]
    fn a_table_in_two_sections_is_taken_whole() {
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.push(
            &section_packet(0x0000, 0, &pat_section_at(1, 0, 0, 1, &[(101, 0x0101)])),
            &mut kept,
        );
        assert!(
            kept.pats.is_empty(),
            "half a table was taken as a whole one"
        );
        assert!(demuxer.program_map_pids().is_empty());

        demuxer.push(
            &section_packet(0x0000, 1, &pat_section_at(1, 0, 1, 1, &[(102, 0x0102)])),
            &mut kept,
        );
        assert_eq!(kept.pats.len(), 1);
        assert_eq!(kept.pats[0].programs.len(), 2);
        assert_eq!(demuxer.program_map_pids(), &[0x0101, 0x0102]);
    }

    #[test]
    fn the_association_table_marks_where_the_programme_maps_are() {
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.push(
            &section_packet(
                0x0000,
                0,
                &pat_section(16625, &[(0, 0x0010), (1024, 0x1FC8)]),
            ),
            &mut kept,
        );
        assert_eq!(kept.pats.len(), 1);
        assert_eq!(kept.pats[0].stream_id, 16625);
        assert_eq!(kept.pats[0].network_pid, Some(0x0010));
        assert_eq!(kept.pats[0].programs.len(), 1);
        assert_eq!(demuxer.program_map_pids(), &[0x1FC8]);
        assert_eq!(demuxer.programs().len(), 1);

        // A map section on the identifier the table named comes back as a
        // section, marked, for someone else to decode.
        demuxer.push(
            &section_packet(
                0x1FC8,
                0,
                &section(0x02, &[0x04, 0x00, 0xE1, 0x00, 0xF0, 0x00]),
            ),
            &mut kept,
        );
        assert_eq!(kept.sections.len(), 1);
        assert_eq!(kept.sections[0].0, 0x1FC8);
        assert_eq!(kept.sections[0].1, 0x02);
        assert!(kept.sections[0].2);
    }

    /// A table on one of the identifiers the standards fix is read without
    /// anything having to say it is there.
    #[test]
    fn a_fixed_table_identifier_is_read_without_being_announced() {
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        // The service description table.
        demuxer.push(
            &section_packet(0x0011, 0, &section(0x42, &[1, 2, 3, 4])),
            &mut kept,
        );
        assert_eq!(kept.sections.len(), 1);
        assert_eq!(kept.sections[0].1, 0x42);
        assert!(!kept.sections[0].2);
    }

    /// A section whose check does not come out says something that was never
    /// sent, so it is not handed on.
    #[test]
    fn a_damaged_section_is_not_handed_on() {
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        let mut damaged = section(0x42, &[1, 2, 3, 4]);
        damaged[4] ^= 0xFF;
        demuxer.push(&section_packet(0x0011, 0, &damaged), &mut kept);
        assert!(kept.sections.is_empty());
    }

    #[test]
    fn the_clock_comes_out_of_the_adaptation_field() {
        let mut with_clock = packet(0x0100, false, 0, &[0; 20]);
        with_clock[3] = 0x30;
        with_clock[4] = 7;
        with_clock[5] = 0x10;
        // A base of one and an extension of two.
        with_clock[6..12].copy_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x80 | 0x7E, 0x02]);
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.push(&with_clock, &mut kept);
        assert_eq!(kept.pcrs.len(), 1);
        assert_eq!(kept.pcrs[0].0, 0x0100);
        assert_eq!(kept.pcrs[0].1.to_90khz(), 1);
        assert_eq!(kept.pcrs[0].1.to_27mhz(), 302);
    }

    /// Stuffing is not a stream and is not counted.
    #[test]
    fn the_null_identifier_is_dropped() {
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.push(&packet(0x1FFF, false, 0, &[0; 20]), &mut kept);
        assert_eq!(demuxer.packets(), 0);
    }

    /// A packet the transmitter marked as damaged is not read at all.
    #[test]
    fn a_packet_marked_in_error_is_dropped() {
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        let mut broken = packet(0x0100, false, 0, &[0; 20]);
        broken[1] |= 0x80;
        demuxer.push(&broken, &mut kept);
        assert_eq!(demuxer.packets(), 0);
    }

    /// A damaged packet is counted against the identifier it named before it
    /// is dropped. **Which identifier is taking the damage is the whole
    /// point of counting**: a fault in one service and a fault in the
    /// multiplex look the same in a single total.
    #[test]
    fn a_damaged_packet_is_counted_against_its_identifier() {
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.expect(0x0100, Carries::Pes);
        let mut broken = packet(0x0100, false, 0, &[0; 20]);
        broken[1] |= 0x80;
        demuxer.push(&broken, &mut kept);
        demuxer.push(&packet(0x0100, true, 1, &[0; 20]), &mut kept);
        let counted = demuxer
            .counted_on(0x0100)
            .expect("識別子が数えられていない");
        assert_eq!(counted.arrived, 2, "壊れたほうも届いた数には入る");
        assert_eq!(counted.errors, 1);
        assert_eq!(counted.passed, 1, "壊れたほうは渡されない");
        assert_eq!(demuxer.errors(), 1);
        assert_eq!(demuxer.counted_on(0x0200), None, "来ていない識別子は無い");
    }

    /// 壊れたパケットは次の番号を使ったものとして扱い、その次のパケットで
    /// もう一度欠落として数えない（誤りと欠落を二重に数えない）。
    #[test]
    fn a_damaged_packet_is_not_counted_twice() {
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.expect(0x0100, Carries::Pes);
        demuxer.push(&packet(0x0100, true, 0, &[0; 20]), &mut kept);
        let mut broken = packet(0x0100, false, 1, &[0; 20]);
        broken[1] |= 0x80;
        demuxer.push(&broken, &mut kept);
        demuxer.push(&packet(0x0100, false, 2, &[0; 20]), &mut kept);
        let counted = demuxer.counted_on(0x0100).expect("数えられている");
        assert_eq!(counted.errors, 1);
        assert_eq!(counted.lost, 0, "壊れたパケットの分を欠落にも数えた");
        assert_eq!(demuxer.lost(), 0);
    }

    /// 識別子の一覧が、来た順ではなく番号の順に出ること。
    #[test]
    fn the_identifiers_come_out_smallest_first() {
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        for pid in [0x0200u16, 0x0100, 0x0150] {
            demuxer.expect(pid, Carries::Pes);
            demuxer.push(&packet(pid, true, 0, &[0; 20]), &mut kept);
        }
        assert_eq!(demuxer.identifiers(), [0x0100, 0x0150, 0x0200]);
    }

    /// Two units in a row on the same identifier, each declaring its length,
    /// come out one after the other without waiting.
    #[test]
    fn units_that_declare_their_length_come_out_as_they_finish() {
        let mut demuxer = Demuxer::new();
        let mut kept = Kept::default();
        demuxer.expect(0x0100, Carries::Pes);
        let mut counter = 0u8;
        for step in 0..3u64 {
            let sent = unit(0xC0, Some(step * 90_000), None, &[step as u8; 30]);
            for packet in carry(0x0100, &mut counter, &sent) {
                demuxer.push(&packet, &mut kept);
            }
        }
        assert_eq!(kept.pes.len(), 3);
        for (step, kept) in kept.pes.iter().enumerate() {
            assert_eq!(kept.1.pts, Some(step as u64 * 90_000));
            assert_eq!(kept.4, vec![step as u8; 30]);
            assert!(kept.3);
        }
    }
}

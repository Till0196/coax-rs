// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! TLV framing: the packets a stream is made of, and the datagrams inside their
//! IPv4, IPv6 and compressed-IP payloads.
//!
//! Two things are here. One is whether a run of bytes is framed as TLV at all,
//! which is what settles how the carriers of a split multiplexing frame line up
//! and needs nothing decoded: the payloads are encrypted on most feeds and the
//! framing is not. The other is the reading proper.

use std::collections::BTreeMap;

use crate::be16;
use crate::mmtp;

/// Every TLV packet starts with this, and its header is four bytes: the sync,
/// the type, and two of length.
pub const SYNC: u8 = 0x7f;
pub const HEADER: usize = 4;

/// What a TLV packet carries. Anything else means the framing has been lost.
pub const IPV4: u8 = 0x01;
pub const IPV6: u8 = 0x02;
/// An IP packet whose header was left out, because the one before it had the
/// same one. Most of a stream is these.
pub const COMPRESSED: u8 = 0x03;
/// The signalling of the transmission itself, which is where the network's own
/// table is.
pub const CONTROL: u8 = 0xfe;
pub const NULL: u8 = 0xff;

/// The context identifier headers a compressed packet may carry. A packet that
/// says it is compressed and then names none of these is not a packet.
const CID_PARTIAL_IPV4_UDP: u8 = 0x20;
const CID_IPV4_IDENTIFIER: u8 = 0x21;
const CID_PARTIAL_IPV6_UDP: u8 = 0x60;
const CID_NO_COMPRESSED_HEADER: u8 = 0x61;

/// How much of a run of bytes reads as TLV, and how the compressed packets in
/// it looked.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Integrity {
    /// How much of what was looked at was framed as TLV, nought to one.
    pub valid: f32,
    /// How many compressed packets went past, and how many of those named a
    /// context identifier that exists.
    pub compressed: usize,
    pub compressed_ok: usize,
}

/// What a run of bytes turned out to be framed as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// Transport packets, which is everything but the advanced services.
    Transport,
    Tlv,
    /// Neither, which is a carrier that did not lock or a sample too short to
    /// tell.
    Unknown,
}

/// What a sample is framed as.
///
/// A carrier says nothing about this: the same tuner hands back transport
/// packets from one transponder and TLV from the next, and a channel file that
/// names the wrong one is a channel that reads as noise. So it is decided from
/// the bytes -- several packets in a row whose lengths lead to the next.
///
/// Transport is looked for first: its packets are a fixed length, so the run
/// that settles it is the cheaper of the two to look for.
pub fn framing(bytes: &[u8]) -> Framing {
    /// How many packets in a row settle it.
    const ENOUGH: usize = 8;
    const PACKET: usize = mpegts::PACKET;
    if bytes.len() >= PACKET * ENOUGH {
        for at in 0..=bytes.len() - PACKET * ENOUGH {
            if (0..ENOUGH).all(|packet| bytes[at + packet * PACKET] == mpegts::SYNC) {
                return Framing::Transport;
            }
        }
    }
    if find_sync(bytes).is_some() {
        return Framing::Tlv;
    }
    Framing::Unknown
}

/// Where a TLV stream starts in a run of bytes, or nothing where none does. A
/// sync byte alone is not a start -- the payloads are full of them -- so what
/// makes one is three packets in a row whose lengths lead to the next.
pub fn find_sync(bytes: &[u8]) -> Option<usize> {
    let mut from = 0;
    while from < bytes.len() {
        let at = from + bytes[from..].iter().position(|byte| *byte == SYNC)?;
        let mut scan = at;
        let mut valid = 0;
        while valid < 3 && scan + HEADER <= bytes.len() {
            if bytes[scan] != SYNC {
                break;
            }
            let length = (usize::from(bytes[scan + 2]) << 8) | usize::from(bytes[scan + 3]);
            let next = scan + HEADER + length;
            if next > bytes.len() {
                break;
            }
            valid += 1;
            scan = next;
        }
        if valid >= 3 {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

/// How far the framing holds from where it started.
pub fn integrity(bytes: &[u8], from: usize) -> Integrity {
    let mut at = from;
    let mut compressed = 0;
    let mut compressed_ok = 0;
    while at + HEADER <= bytes.len() {
        if bytes[at] != SYNC {
            break;
        }
        let kind = bytes[at + 1];
        if !carries(kind) {
            break;
        }
        let length = (usize::from(bytes[at + 2]) << 8) | usize::from(bytes[at + 3]);
        let next = at + HEADER + length;
        if next > bytes.len() {
            break;
        }
        if kind == COMPRESSED && length >= 3 {
            compressed += 1;
            let cid = bytes[at + HEADER + 2];
            if !matches!(
                cid,
                CID_PARTIAL_IPV4_UDP
                    | CID_IPV4_IDENTIFIER
                    | CID_PARTIAL_IPV6_UDP
                    | CID_NO_COMPRESSED_HEADER
            ) {
                break;
            }
            compressed_ok += 1;
        }
        at = next;
    }
    let looked_at = (bytes.len() - from).max(1);
    Integrity {
        valid: (at - from) as f32 / looked_at as f32,
        compressed,
        compressed_ok,
    }
}

/// The port the transmission's clock is sent to.
const NTP_PORT: u16 = 123;

/// How far into an NTP message its transmit timestamp ends. The header before
/// it is forty bytes, and the timestamp itself is eight.
const NTP_TRANSMIT_END: usize = 48;

/// How the packets of a run of bytes turned out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub bytes: u64,
    pub packets: u64,
    pub null: u64,
    pub control: u64,
    pub ipv4: u64,
    pub ipv6: u64,
    pub compressed: u64,
    /// Bytes that were not a packet **after the framing had been found**.
    pub invalid: u64,
    /// Bytes stepped over before the framing was found at all.
    ///
    /// **This is not an error.** A receiver joins a stream wherever the tuner
    /// happened to be, which is the middle of a packet, and the bytes up to
    /// the first whole one are the tail of a packet that was already on its
    /// way. Counting those as errors puts a receiver at `E 1` the moment it
    /// tunes and a field that is never nought is a field a viewer stops reading.
    pub before_sync: u64,
    /// Compressed packets that arrived before the one that says what their
    /// header is. Their payload is still good; only the addresses are unknown.
    pub without_context: u64,
}

/// One TLV packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet<'a> {
    pub kind: u8,
    pub payload: &'a [u8],
    /// How far into the run of bytes it started, which is what a rate is
    /// counted in.
    pub at: usize,
}

/// A datagram out of a TLV packet, with the addresses that carried it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datagram<'a> {
    pub payload: &'a [u8],
    /// Empty for a compressed packet whose context has not been seen.
    pub source: Vec<u8>,
    pub destination: Vec<u8>,
    pub source_port: u16,
    pub destination_port: u16,
    /// Which header-compression context the packet belongs to, where it named
    /// one. It stands in for the addresses while the packet that would supply
    /// them has not gone past.
    pub context: Option<u16>,
}

impl Datagram<'_> {
    /// Whether this is the transmission's clock rather than the broadcast.
    pub fn is_ntp(&self) -> bool {
        self.destination_port == NTP_PORT
    }

    /// The wall clock the transmission is sent against, in NTP timestamp
    /// format: seconds since 1900 in the upper half, a fraction of a second in
    /// the lower.
    ///
    /// It is the transmit timestamp, the last of the four an NTP message
    /// carries, in the eight bytes ending forty-eight into the message: the one
    /// the sender filled in as it sent. Nothing where the datagram is not the
    /// clock or is too short.
    pub fn ntp(&self) -> Option<u64> {
        if !self.is_ntp() || self.payload.len() < NTP_TRANSMIT_END {
            return None;
        }
        let bytes: [u8; 8] = self.payload[NTP_TRANSMIT_END - 8..NTP_TRANSMIT_END]
            .try_into()
            .ok()?;
        Some(u64::from_be_bytes(bytes))
    }
}

/// The addresses a compression context stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Addresses {
    source: Vec<u8>,
    destination: Vec<u8>,
    source_port: u16,
    destination_port: u16,
}

/// Walks the TLV packets of a run of bytes.
#[derive(Debug)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    /// Whether the framing has been found. **Until it has, a candidate has to
    /// be confirmed by the packet after it**, and what is stepped over is not
    /// an error. See [`Stats::before_sync`] and [`Reader::candidate`].
    synced: bool,
    stats: Stats,
    contexts: BTreeMap<u16, Addresses>,
}

/// What is at a position, as far as the bytes there allow saying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Candidate {
    /// A packet, ending here.
    Packet(usize),
    /// Not a packet.
    No,
    /// Not enough bytes to say yet. **The caller waits rather than steps
    /// over**: a packet does not stop at the end of a chunk.
    More,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            at: 0,
            synced: false,
            stats: Stats::default(),
            contexts: BTreeMap::new(),
        }
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// The next packet, or nothing where the run of bytes has ended. A byte
    /// that is not a packet is stepped over to the next sync byte: a sample
    /// begins wherever the tuner happened to be.
    ///
    /// Not an iterator, deliberately: reading what a packet carries needs the
    /// reader as well -- the addresses of a compressed packet are in a context
    /// an earlier one established -- and an iterator that borrowed the reader
    /// would leave nothing to ask.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Packet<'a>> {
        loop {
            let start = self.at;
            let next = match self.candidate(start) {
                Candidate::Packet(next) => next,
                // Not enough bytes to say. **Left where it is rather than
                // stepped over**, so that a reader being fed a chunk at a time
                // finds it whole once the rest arrives.
                Candidate::More => return None,
                Candidate::No => {
                    if self.synced {
                        self.stats.invalid += 1;
                    }
                    let step = match self.bytes.get(start + 1..) {
                        Some(left) => left.iter().position(|byte| *byte == SYNC),
                        None => None,
                    };
                    match step {
                        Some(found) => {
                            if !self.synced {
                                self.stats.before_sync += (found + 1) as u64;
                            }
                            self.at = start + found + 1;
                        }
                        None => {
                            if !self.synced {
                                self.stats.before_sync += (self.bytes.len() - start) as u64;
                            }
                            self.at = self.bytes.len();
                            return None;
                        }
                    }
                    continue;
                }
            };
            let kind = self.bytes[start + 1];
            self.synced = true;
            self.at = next;
            self.stats.bytes += (next - start) as u64;
            self.stats.packets += 1;
            match kind {
                NULL => self.stats.null += 1,
                CONTROL => self.stats.control += 1,
                IPV4 => self.stats.ipv4 += 1,
                IPV6 => self.stats.ipv6 += 1,
                COMPRESSED => self.stats.compressed += 1,
                _ => {}
            }
            return Some(Packet {
                kind,
                payload: &self.bytes[start + 4..next],
                at: start,
            });
        }
    }

    /// What is at a position.
    ///
    /// **Until the framing is found, a candidate has to be confirmed by what
    /// follows it.** A sync byte alone is no evidence -- the payloads are full
    /// of them -- and taking the first one means reading a length out of the
    /// middle of somebody's picture and stepping over whatever it lands past.
    /// So an unconfirmed candidate is one whose packet is followed by another
    /// packet's sync byte, or which ends exactly where the bytes do.
    ///
    /// Once the framing is found the test is the packet itself again: waiting
    /// for the packet after it would hold every packet back by one, and lose
    /// the last of a recording altogether.
    fn candidate(&self, at: usize) -> Candidate {
        let Some(head) = self.bytes.get(at..at + HEADER) else {
            return Candidate::More;
        };
        if head[0] != SYNC || !carries(head[1]) {
            return Candidate::No;
        }
        let length = (usize::from(head[2]) << 8) | usize::from(head[3]);
        let next = at + HEADER + length;
        if next > self.bytes.len() {
            return Candidate::More;
        }
        if self.synced || next == self.bytes.len() {
            return Candidate::Packet(next);
        }
        match self.bytes.get(next..next + 2) {
            Some(after) if after[0] == SYNC && carries(after[1]) => Candidate::Packet(next),
            Some(_) => Candidate::No,
            None => Candidate::More,
        }
    }

    /// How far into the run of bytes the reading has got. What is past it is
    /// packets; what is left is the one that has not all arrived.
    pub fn at(&self) -> usize {
        self.at
    }

    /// What a packet carries, where it carries a datagram at all.
    pub fn datagram(&mut self, packet: &Packet<'a>) -> Option<Datagram<'a>> {
        match packet.kind {
            IPV4 => ipv4(packet.payload),
            IPV6 => ipv6(packet.payload),
            COMPRESSED => self.compressed(packet.payload),
            _ => None,
        }
    }

    /// An IP packet whose header was left out, or which is establishing the
    /// header the ones after it will leave out.
    fn compressed(&mut self, payload: &'a [u8]) -> Option<Datagram<'a>> {
        if payload.len() < 3 {
            return None;
        }
        let context = ((u16::from(payload[0]) << 8) | u16::from(payload[1])) >> 4;
        match payload[2] {
            // The header in full, which every packet of this context after it
            // is sent without.
            0x20 => {
                let datagram = whole(payload, 11, 15, 19, 23, context)?;
                self.remember(context, &datagram);
                Some(datagram)
            }
            0x60 => {
                let datagram = whole(payload, 9, 25, 41, 45, context)?;
                self.remember(context, &datagram);
                Some(datagram)
            }
            // No header at all. A sample often begins in the middle of a run
            // of these, and the payload is intact either way.
            0x21 | 0x61 => {
                let mut datagram = Datagram {
                    payload: &payload[3..],
                    source: Vec::new(),
                    destination: Vec::new(),
                    source_port: 0,
                    destination_port: 0,
                    context: Some(context),
                };
                match self.contexts.get(&context) {
                    Some(known) => {
                        datagram.source = known.source.clone();
                        datagram.destination = known.destination.clone();
                        datagram.source_port = known.source_port;
                        datagram.destination_port = known.destination_port;
                    }
                    None => self.stats.without_context += 1,
                }
                Some(datagram)
            }
            _ => None,
        }
    }

    fn remember(&mut self, context: u16, datagram: &Datagram<'a>) {
        self.contexts.insert(
            context,
            Addresses {
                source: datagram.source.clone(),
                destination: datagram.destination.clone(),
                source_port: datagram.source_port,
                destination_port: datagram.destination_port,
            },
        );
    }
}

/// Whether a byte names something a TLV packet may carry. A packet whose type
/// is none of them is the framing having been lost rather than a packet.
fn carries(kind: u8) -> bool {
    matches!(kind, IPV4 | IPV6 | COMPRESSED | CONTROL | NULL)
}

/// The datagram of a compressed packet that carried its header, by where the
/// addresses and ports sit in it.
fn whole(
    payload: &[u8],
    source: usize,
    destination: usize,
    ports: usize,
    body: usize,
    context: u16,
) -> Option<Datagram<'_>> {
    if payload.len() < body {
        return None;
    }
    Some(Datagram {
        payload: &payload[body..],
        source: payload[source..destination].to_vec(),
        destination: payload[destination..ports].to_vec(),
        source_port: be16(&payload[ports..]),
        destination_port: be16(&payload[ports + 2..]),
        context: Some(context),
    })
}

fn ipv4(payload: &[u8]) -> Option<Datagram<'_>> {
    if payload.len() < 20 || payload[0] >> 4 != 4 {
        return None;
    }
    let header = usize::from(payload[0] & 0x0f) * 4;
    let total = usize::from(be16(&payload[2..]));
    // Only whole UDP datagrams: a fragment says nothing on its own, and the
    // broadcast does not send them.
    if header < 20
        || total < header + 8
        || total > payload.len()
        || payload[9] != 17
        || be16(&payload[6..]) & 0x3fff != 0
    {
        return None;
    }
    udp(&payload[header..total], &payload[12..16], &payload[16..20])
}

fn ipv6(payload: &[u8]) -> Option<Datagram<'_>> {
    if payload.len() < 40 || payload[0] >> 4 != 6 {
        return None;
    }
    let end = 40 + usize::from(be16(&payload[4..]));
    if end > payload.len() {
        return None;
    }
    let mut next = payload[6];
    let mut body = &payload[40..end];
    // The extension headers, each of which says what the one after it is.
    while matches!(next, 0 | 43 | 51 | 60) {
        if body.len() < 2 {
            return None;
        }
        let length = match next {
            // The authentication header counts in four-byte units and from a
            // different base than the rest.
            51 => (usize::from(body[1]) + 2) * 4,
            _ => (usize::from(body[1]) + 1) * 8,
        };
        if length > body.len() {
            return None;
        }
        next = body[0];
        body = &body[length..];
    }
    if next != 17 {
        return None;
    }
    udp(body, &payload[8..24], &payload[24..40])
}

fn udp<'a>(payload: &'a [u8], source: &[u8], destination: &[u8]) -> Option<Datagram<'a>> {
    if payload.len() < 8 {
        return None;
    }
    let length = usize::from(be16(&payload[4..]));
    if length < 8 || length > payload.len() {
        return None;
    }
    Some(Datagram {
        payload: &payload[8..length],
        source: source.to_vec(),
        destination: destination.to_vec(),
        source_port: be16(payload),
        destination_port: be16(&payload[2..]),
        context: None,
    })
}

/// What came out of a chunk of a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    /// The transmission's own signalling, which is where the network table is.
    Control(Vec<u8>),
    /// The clock the transmission is sent against, each time one arrives. It
    /// is not the broadcast, and it is the only wall clock MMT has: the times
    /// of an access unit are worked out from it.
    Ntp(u64),
    /// An MMTP packet, out of the datagram that carried it.
    Mmtp {
        packet_id: u16,
        payload_type: u8,
        timestamp: u32,
        /// Counts the packets of this identifier, so a gap in it is a gap in
        /// the stream.
        sequence_number: u32,
        /// Whether what follows can be decoded without anything before it.
        rap: bool,
        /// Whether the payload is under conditional access, as the packet's
        /// own header extension says. **A reader that does not look at this
        /// reassembles ciphertext**: the fragmentation and the lengths are in
        /// the clear, so a scrambled payload reassembles into an access unit
        /// of the right size holding nothing readable, and every parser after
        /// it reports the wrong fault.
        scrambled: bool,
        /// How far into the stream it began, counted from the first byte ever
        /// fed. It is what a rate is worked out from -- see [`Clock`].
        at: usize,
        payload: Vec<u8>,
    },
}

/// The same reading, for a stream that arrives a chunk at a time. A packet does
/// not stop at the end of a chunk, so what is left over is held until the rest
/// of it arrives. The contexts a compressed packet's addresses come from are
/// held across chunks too.
#[derive(Debug, Default)]
pub struct Feed {
    held: Vec<u8>,
    /// Whether the framing has been found, kept across chunks: a stream is
    /// joined once, not once per chunk ([`Reader::candidate`]).
    synced: bool,
    /// How many bytes have been read and let go of, which is what makes the
    /// offsets a caller sees count from the start of the stream rather than
    /// from the start of the chunk.
    done: usize,
    stats: Stats,
    contexts: BTreeMap<u16, Addresses>,
}

impl Feed {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Takes what has arrived and gives back what it completed.
    pub fn feed(&mut self, arrived: &[u8]) -> Vec<Piece> {
        self.held.extend_from_slice(arrived);
        let mut reader = Reader {
            bytes: &self.held,
            at: 0,
            synced: self.synced,
            stats: self.stats,
            contexts: std::mem::take(&mut self.contexts),
        };
        let mut pieces = Vec::new();
        while let Some(packet) = reader.next() {
            match packet.kind {
                CONTROL => pieces.push(Piece::Control(packet.payload.to_vec())),
                NULL => {}
                _ => {
                    let Some(datagram) = reader.datagram(&packet) else {
                        continue;
                    };
                    // The transmission's clock is sent to a port of its own,
                    // and is not the broadcast.
                    if datagram.is_ntp() {
                        if let Some(ntp) = datagram.ntp() {
                            pieces.push(Piece::Ntp(ntp));
                        }
                        continue;
                    }
                    if let Some(packet_read) = mmtp::read(datagram.payload) {
                        pieces.push(Piece::Mmtp {
                            packet_id: packet_read.packet_id,
                            payload_type: packet_read.payload_type,
                            timestamp: packet_read.timestamp,
                            sequence_number: packet_read.sequence_number,
                            rap: packet_read.rap,
                            scrambled: packet_read.scrambled,
                            at: self.done + packet.at,
                            payload: packet_read.payload.to_vec(),
                        });
                    }
                }
            }
        }
        let read = reader.at();
        self.synced = reader.synced;
        self.stats = reader.stats;
        self.contexts = std::mem::take(&mut reader.contexts);
        drop(reader);
        self.held.drain(..read);
        self.done += read;
        pieces
    }
}

#[cfg(test)]
mod tests {
    use super::{CONTROL, Datagram, Feed, Framing, IPV4, NULL, Reader, SYNC, framing};

    /// A TLV packet: the sync byte, what it carries, and the length of what
    /// follows.
    fn tlv(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![super::SYNC, kind];
        packet.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        packet.extend_from_slice(payload);
        packet
    }

    #[test]
    fn the_packets_of_a_run_of_bytes_are_walked_in_order() {
        let mut bytes = tlv(NULL, &[0; 4]);
        bytes.extend(tlv(CONTROL, &[0x40, 0x01]));
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.next().map(|packet| packet.kind), Some(NULL));
        let control = reader.next().expect("the second packet is there");
        assert_eq!(control.kind, CONTROL);
        assert_eq!(control.payload, &[0x40, 0x01]);
        assert!(reader.next().is_none());
        let stats = reader.stats();
        assert_eq!(stats.packets, 2);
        assert_eq!(stats.null, 1);
        assert_eq!(stats.control, 1);
    }

    /// **途中から入るのは誤りではありません。**受信機はチューナーがその時点で
    /// 受信していた位置から読み始めるので、先頭は常にどれかのパケットの末尾です。
    /// ここを `invalid` に数えていたので、選局した瞬間に `E 1` から
    /// 始まっていました。
    #[test]
    fn a_run_that_begins_mid_packet_is_not_an_error() {
        let mut bytes = vec![0x12, 0x34, 0x56];
        bytes.extend(tlv(NULL, &[0; 2]));
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.next().map(|packet| packet.kind), Some(NULL));
        assert_eq!(reader.stats().invalid, 0, "誤りではない");
        assert_eq!(reader.stats().before_sync, 3, "読み飛ばしたバイトは数える");
    }

    /// **同期を確立したあとで崩れたら、それは誤りです。**受信機が報告すべき
    /// 内容が「途中から入った」から「伝送路で壊れた」へ変わります。
    #[test]
    fn losing_the_framing_after_it_was_found_is_an_error() {
        let mut bytes = tlv(NULL, &[0; 2]);
        bytes.extend(tlv(CONTROL, &[0x40, 0x01]));
        bytes.extend_from_slice(&[0x12, 0x34, 0x56]);
        bytes.extend(tlv(NULL, &[0; 2]));
        let mut reader = Reader::new(&bytes);
        while reader.next().is_some() {}
        assert_eq!(reader.stats().packets, 3);
        assert_eq!(reader.stats().invalid, 1);
        assert_eq!(reader.stats().before_sync, 0);
    }

    /// **同期バイト一つでは同期になりません。**ペイロードの中には 0x7f が
    /// いくらでもあり、そこを先頭だと思うと、長さを別のデータの途中から読んで、
    /// その長さの分だけ本物を読み飛ばします——読み飛ばした先が本物のパケットなら、
    /// 欠落として数えられないまま消えます。
    #[test]
    fn a_sync_byte_in_a_payload_is_not_taken_for_a_start() {
        // 先頭はどれかのパケットの末尾で、その中に「それらしい」ヘッダーがある:
        // 0x7f、種別 0x01、長さ 0x0002。**このまま信じると 4+2 バイト先へ
        // 飛び、本物のヘッダーを読み飛ばします。**
        let mut bytes = vec![SYNC, IPV4, 0x00, 0x02, 0xaa, 0xbb, 0xcc];
        bytes.extend(tlv(NULL, &[0; 2]));
        bytes.extend(tlv(CONTROL, &[0x40, 0x01]));
        let mut reader = Reader::new(&bytes);
        assert_eq!(
            reader.next().map(|packet| packet.kind),
            Some(NULL),
            "偽のヘッダーで同期して本物を読み飛ばした"
        );
        assert_eq!(reader.next().map(|packet| packet.kind), Some(CONTROL));
        assert_eq!(reader.stats().invalid, 0);
    }

    /// **確認に使う次のパケットが来ていないときは待ちます。**読み飛ばしてしまうと、次のチャンクで
    /// 続きが来ても、そのパケットはもう戻ってきません。
    #[test]
    fn an_unconfirmed_start_waits_for_the_bytes_that_would_confirm_it() {
        let whole = tlv(NULL, &[0; 2]);
        let mut feed = Feed::new();
        // 一つ目だけでは確かめられない——ただし、ちょうど終わっていれば
        // それ自体が証拠になるので、続きのヘッダーを一バイトだけ足しておく。
        let mut first = whole.clone();
        first.push(SYNC);
        feed.feed(&first);
        assert_eq!(feed.stats().packets, 0, "まだ確かめられない");
        let mut rest = vec![CONTROL, 0x00, 0x02, 0x40, 0x01];
        rest.extend(tlv(NULL, &[0; 2]));
        let pieces = feed.feed(&rest);
        assert_eq!(pieces.len(), 1, "確認できたら、先頭から読める");
        assert_eq!(feed.stats().packets, 3);
        assert_eq!(feed.stats().invalid, 0);
        assert_eq!(feed.stats().before_sync, 0);
    }

    #[test]
    fn a_datagram_is_taken_out_of_the_packet_that_carried_it() {
        let body = [0xaau8, 0xbb];
        let mut udp = vec![0x12, 0x34, 0x00, 0x7b];
        udp.extend_from_slice(&((8 + body.len()) as u16).to_be_bytes());
        udp.extend_from_slice(&[0x00, 0x00]);
        udp.extend_from_slice(&body);
        let mut ip = vec![0x45, 0x00];
        ip.extend_from_slice(&((20 + udp.len()) as u16).to_be_bytes());
        ip.extend_from_slice(&[0, 0, 0, 0, 64, 17, 0, 0]);
        ip.extend_from_slice(&[10, 0, 0, 1]);
        ip.extend_from_slice(&[239, 0, 0, 1]);
        ip.extend_from_slice(&udp);
        let bytes = tlv(IPV4, &ip);
        let mut reader = Reader::new(&bytes);
        let packet = reader.next().expect("the packet is there");
        let datagram = reader.datagram(&packet).expect("it carries a datagram");
        assert_eq!(datagram.payload, &body);
        assert_eq!(datagram.source, vec![10, 0, 0, 1]);
        assert_eq!(datagram.destination_port, 123);
        assert!(datagram.is_ntp());
        // Too short to hold a timestamp, so there is none to give.
        assert_eq!(datagram.ntp(), None);
    }

    #[test]
    fn a_compressed_packet_borrows_the_addresses_of_its_context() {
        // The context, established: two bytes of identifier, the kind, and
        // then the header this stands for.
        let mut whole = vec![0x00, 0x10, 0x20];
        whole.extend_from_slice(&[0; 8]);
        whole.extend_from_slice(&[10, 0, 0, 1]);
        whole.extend_from_slice(&[239, 0, 0, 1]);
        whole.extend_from_slice(&[0x12, 0x34, 0x00, 0x7b]);
        whole.extend_from_slice(&[0xaa]);
        let mut bytes = tlv(super::COMPRESSED, &whole);
        bytes.extend(tlv(super::COMPRESSED, &[0x00, 0x10, 0x21, 0xbb]));
        let mut reader = Reader::new(&bytes);
        let first = reader.next().expect("the first packet is there");
        let established = reader.datagram(&first).expect("it carries a datagram");
        assert_eq!(established.destination_port, 123);
        let second = reader.next().expect("the second packet is there");
        let Datagram {
            payload,
            source,
            destination_port,
            ..
        } = reader.datagram(&second).expect("it carries a datagram");
        assert_eq!(payload, &[0xbb]);
        assert_eq!(source, vec![10, 0, 0, 1]);
        assert_eq!(destination_port, 123);
        assert_eq!(reader.stats().without_context, 0);
    }

    #[test]
    fn a_compressed_packet_without_its_context_keeps_its_payload() {
        let bytes = tlv(super::COMPRESSED, &[0x00, 0x10, 0x21, 0xbb]);
        let mut reader = Reader::new(&bytes);
        let packet = reader.next().expect("the packet is there");
        let datagram = reader.datagram(&packet).expect("it carries a datagram");
        assert_eq!(datagram.payload, &[0xbb]);
        assert!(datagram.source.is_empty());
        assert_eq!(reader.stats().without_context, 1);
    }

    use super::{find_sync, integrity};

    /// A run of well-formed packets: a null, a compressed one, and a null
    /// again, which is what these streams are mostly made of.
    fn stream() -> Vec<u8> {
        let mut bytes = Vec::new();
        for _ in 0..6 {
            bytes.extend_from_slice(&[0x7f, 0xff, 0x00, 0x18]);
            bytes.extend(std::iter::repeat_n(0u8, 24));
            bytes.extend_from_slice(&[0x7f, 0x03, 0x00, 0x10]);
            bytes.extend_from_slice(&[0x00, 0x00, 0x61]);
            bytes.extend(std::iter::repeat_n(0u8, 13));
        }
        bytes
    }

    #[test]
    fn a_stream_that_is_framed_is_found_and_reads_to_the_end() {
        let bytes = stream();
        let at = find_sync(&bytes).expect("a framed stream is found");
        assert_eq!(at, 0);
        let seen = integrity(&bytes, at);
        assert!(seen.valid > 0.99, "it reads to the end: {}", seen.valid);
        assert_eq!(seen.compressed, 6);
        assert_eq!(seen.compressed_ok, 6);
    }

    /// Bytes before the stream are walked past, and a sync byte inside a
    /// payload is not a start.
    #[test]
    fn a_sync_byte_in_a_payload_is_not_a_start() {
        let mut bytes = vec![0x7f, 0x7f, 0x7f, 0x7f, 0x00, 0x11, 0x22];
        bytes.extend(stream());
        assert_eq!(find_sync(&bytes), Some(7));
    }

    /// A stream that has been put together wrongly stops reading almost at
    /// once, which is what tells one alignment from another.
    #[test]
    fn a_stream_that_is_not_framed_reads_no_distance() {
        let bytes: Vec<u8> = (0..4096u32).map(|at| (at * 37 % 251) as u8).collect();
        match find_sync(&bytes) {
            None => {}
            Some(at) => {
                let seen = integrity(&bytes, at);
                assert!(seen.valid < 0.5, "noise does not read as a stream");
            }
        }
    }

    /// What a sample is framed as is decided from the bytes, because nothing
    /// else says: one transponder of a satellite hands back transport packets
    /// and the next hands back TLV.
    #[test]
    fn a_sample_says_which_framing_it_is_by_how_it_is_framed() {
        let mut transport = vec![0u8; mpegts::PACKET * 8];
        for packet in 0..8 {
            transport[packet * mpegts::PACKET] = mpegts::SYNC;
        }
        assert_eq!(framing(&transport), Framing::Transport);

        let mut tlv = Vec::new();
        for _ in 0..4 {
            tlv.extend_from_slice(&[super::SYNC, 0xff, 0x00, 0x04, 0, 0, 0, 0]);
        }
        assert_eq!(framing(&tlv), Framing::Tlv);

        assert_eq!(framing(&[0u8; 64]), Framing::Unknown);
    }

    /// The transmission's clock, out of the datagram it is sent in.
    #[test]
    fn the_wall_clock_is_taken_out_of_the_message_that_carries_it() {
        let mut message = vec![0u8; 48];
        message[40..48].copy_from_slice(&0xe9ab_cdef_1234_5678u64.to_be_bytes());
        let mut udp = vec![0x00, 0x7b, 0x00, 0x7b];
        udp.extend_from_slice(&((8 + message.len()) as u16).to_be_bytes());
        udp.extend_from_slice(&[0x00, 0x00]);
        udp.extend_from_slice(&message);
        let mut ip = vec![0x45, 0x00];
        ip.extend_from_slice(&((20 + udp.len()) as u16).to_be_bytes());
        ip.extend_from_slice(&[0, 0, 0, 0, 64, 17, 0, 0]);
        ip.extend_from_slice(&[10, 0, 0, 1]);
        ip.extend_from_slice(&[239, 0, 0, 1]);
        ip.extend_from_slice(&udp);
        let bytes = tlv(IPV4, &ip);
        let mut reader = Reader::new(&bytes);
        let packet = reader.next().expect("the packet is there");
        let datagram = reader.datagram(&packet).expect("it carries a datagram");
        assert!(datagram.is_ntp());
        assert_eq!(datagram.ntp(), Some(0xe9ab_cdef_1234_5678));
    }
}

// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Putting access units back together out of the MPU payloads that carried
//! them.
//!
//! A media fragment unit is one access unit's worth of bytes, but it need not
//! be one packet's worth. A small one shares a payload with its neighbours,
//! behind a length each; a large one is split across as many payloads as it
//! takes, and only the last piece says the unit is finished. So a decoder has
//! to be handed what the payloads add up to.
//!
//! ## The sample number is not a count
//!
//! A timed fragment unit carries a `sample_number`, and it would be the obvious
//! thing to number access units by. It is not usable: broadcast leaves it at
//! nought -- every service measured did -- and so does `offset` and the movie
//! fragment sequence number.
//!
//! What is counted here instead is the processing unit boundary. The MPU
//! sequence number does move, so the units are counted from one within each and
//! the count starts again when the sequence number does.

use std::collections::BTreeMap;

use crate::mmtp::{self, COMPLETE, FIRST, LAST, MFU, MIDDLE, Mmtp};

/// How much of one split unit to hold while the rest of it arrives. A unit
/// this long is a stream that has gone wrong rather than a picture.
pub const MOST_HELD: usize = 64 << 20;

/// One access unit, put back together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessUnit {
    pub packet_id: u16,
    /// Which processing unit it belongs to.
    pub mpu_sequence_number: u32,
    /// Which unit of that processing unit it is, counted from one here rather
    /// than taken from the stream. See the module documentation for why.
    pub number: u32,
    pub data: Vec<u8>,
    /// Whether the packet that finished it said it was a random access point.
    pub rap: bool,
    /// Whether the asset has a timeline. An untimed one is a file sent
    /// alongside the media, and arrives whole.
    pub timed: bool,
    /// Whether anything was thrown away since the last unit given back for this
    /// identifier -- a gap in the packets, a payload that would not parse, or a
    /// split unit left unfinished. The unit is whole, but what came before it
    /// is not.
    pub after_loss: bool,
    /// What the stream said the sample number was, unmodified. Kept because it
    /// is the field a stream *ought* to number by, and a caller measuring a
    /// transmission wants to see it; nothing here counts from it.
    pub sample_number: u32,
    /// What the stream said the offset in the sample was, unmodified.
    pub offset: u32,
    /// Which item of an untimed asset this is. Nought for a timed one.
    pub item_id: u32,
    /// For an untimed unit, the payload's fragmentation indicator
    /// ([`COMPLETE`], [`FIRST`], [`MIDDLE`], [`LAST`]): **a file of a data
    /// broadcast is split over many packets**, and is put together by
    /// whoever keeps the files (ARIB TR-B39 part 3, 6.3.4). Nought for a
    /// timed one, which is put together here.
    pub fragmentation: u8,
}

/// What went past, and what did not survive the trip.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// MPU payloads read.
    pub payloads: u64,
    /// Access units given back.
    pub access_units: u64,
    /// Times the packet counter skipped.
    pub sequence_gaps: u64,
    /// How many packets those skips add up to.
    pub lost_packets: u64,
    /// Packets whose counter had gone back a little: the same packet sent
    /// again, dropped ([`track`]). **Not a fault of the stream**, any more
    /// than a repeated TS packet is.
    pub out_of_order_packets: u64,
    /// Times a counter went back a long way and the count started over from
    /// it: another sender's numbering, a CM spliced in ([`track`]). Not a
    /// fault either.
    pub restarted_counters: u64,
    /// Payloads carrying the boxes that describe the media rather than the
    /// media, which are not reassembled here.
    pub metadata_fragments: u64,
    pub untimed_units: u64,
    pub parse_errors: u64,
    /// Split units abandoned: a piece arrived without its opening, or an
    /// opening arrived while one was still being held.
    pub fragment_errors: u64,
    pub scrambled_packets: u64,
    /// Payloads whose fields the transmission actually filled in. All nought
    /// on broadcast, which is the whole reason units are counted here.
    pub non_zero_samples: u64,
    pub non_zero_offsets: u64,
    pub non_zero_movie_fragments: u64,
}

/// What is being held for one packet identifier, which is to say one asset.
#[derive(Debug, Default)]
struct Asset {
    /// The packet counter the next packet should carry.
    expect: u32,
    have_expect: bool,
    /// The processing unit the numbering is counting within.
    mpu_sequence_number: u32,
    have_mpu: bool,
    number: u32,
    /// A split unit part way through.
    held: Vec<u8>,
    holding: bool,
    held_mpu: u32,
    held_rap: bool,
    held_unit: mmtp::DataUnit<'static>,
    /// Whether a piece without its opening has already been complained about,
    /// so that the rest of the same unit goes quietly.
    ///
    /// **It starts true.** A receiver joins a stream wherever the tuner was,
    /// which for a picture is the middle of a unit -- an access unit can be
    /// dozens of packets -- and the pieces up to the first opening are the
    /// rest of a unit that was already on its way, not a unit that lost its
    /// opening. Counted as an error, every join to an MMT broadcast began at
    /// `E 1` (measured through coaxd), and the field said the same thing
    /// for tuning as for a broken path.
    orphan: bool,
    after_loss: bool,
    tally: Tally,
}

impl Asset {
    /// An asset as it is when its first packet arrives: joined, not begun.
    fn joining() -> Self {
        Self {
            orphan: true,
            ..Self::default()
        }
    }
}

/// What one packet identifier saw go past. The whole-stream [`Stats`] add
/// these up across every asset; a viewer asking why the picture is broken
/// wants them apart, because the loss is on one identifier and not the others.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    /// Packets that arrived under this identifier, scrambled ones included.
    pub arrived: u64,
    /// Packets the counter says were skipped.
    pub lost: u64,
    /// Packets that had gone back a little, and were dropped as sent again.
    pub out_of_order: u64,
    /// Times the counter started over (another sender's numbering).
    pub restarted: u64,
    /// Packets that arrived under conditional access and were not read.
    pub scrambled: u64,
    /// Payloads that could not be read, and split units that could not be
    /// finished.
    pub errors: u64,
}

/// Puts access units back together out of MMTP packets. State is kept per
/// packet identifier: two identifiers are two assets, their packets interleave,
/// and a split unit of one says nothing about the other.
#[derive(Debug)]
pub struct Reassembler {
    assets: BTreeMap<u16, Asset>,
    stats: Stats,
    most_held: usize,
}

impl Default for Reassembler {
    fn default() -> Self {
        Self::new()
    }
}

impl Reassembler {
    pub fn new() -> Self {
        Self::with_limit(MOST_HELD)
    }

    /// The same, holding at most this many bytes of one split unit.
    pub fn with_limit(most_held: usize) -> Self {
        Self {
            assets: BTreeMap::new(),
            stats: Stats::default(),
            most_held,
        }
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Every packet identifier a packet has arrived under, in order.
    pub fn packet_ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.assets.keys().copied()
    }

    /// What one packet identifier saw, if anything arrived under it.
    pub fn tally(&self, packet_id: u16) -> Option<Tally> {
        self.assets.get(&packet_id).map(|asset| asset.tally)
    }

    /// Takes one MMTP packet and gives back whatever it completed.
    pub fn push(&mut self, packet: &Mmtp<'_>) -> Vec<AccessUnit> {
        let mut out = Vec::new();
        let asset = self
            .assets
            .entry(packet.packet_id)
            .or_insert_with(Asset::joining);
        asset.tally.arrived += 1;
        if !track(asset, packet, &mut self.stats) {
            return out;
        }
        if packet.scrambled {
            self.stats.scrambled_packets += 1;
            asset.tally.scrambled += 1;
            drop_held(asset, &mut self.stats);
            asset.after_loss = true;
            return out;
        }
        if packet.payload_type != mmtp::MPU {
            return out;
        }
        let Some(read) = mmtp::read_mpu(packet.payload) else {
            self.stats.parse_errors += 1;
            asset.tally.errors += 1;
            drop_held(asset, &mut self.stats);
            asset.after_loss = true;
            return out;
        };
        self.stats.payloads += 1;
        // The boxes that describe the media are not the media. They are read
        // elsewhere, out of the signalling, and reassembling them here would
        // only put them in front of the pictures.
        if read.fragment_type != MFU {
            self.stats.metadata_fragments += 1;
            return out;
        }
        if !read.timed {
            // An untimed asset is a file, and arrives whole.
            self.stats.untimed_units += read.units.len() as u64;
            for unit in &read.units {
                let number = number(asset, read.mpu_sequence_number);
                out.push(AccessUnit {
                    packet_id: packet.packet_id,
                    mpu_sequence_number: read.mpu_sequence_number,
                    number,
                    data: unit.data.to_vec(),
                    rap: packet.rap,
                    timed: false,
                    after_loss: std::mem::take(&mut asset.after_loss),
                    sample_number: unit.sample_number,
                    offset: unit.offset,
                    item_id: unit.item_id,
                    fragmentation: read.fragmentation_indicator,
                });
                self.stats.access_units += 1;
            }
            return out;
        }
        if read.aggregation_flag {
            // Several whole units in one payload. A payload cannot both
            // aggregate and be a piece of something, so anything being held is
            // a unit that will never be finished.
            drop_held(asset, &mut self.stats);
            asset.orphan = false;
            for unit in &read.units {
                emit(
                    &mut out,
                    &mut self.stats,
                    asset,
                    packet.packet_id,
                    read.mpu_sequence_number,
                    unit,
                    unit.data.to_vec(),
                    packet.rap,
                );
            }
            return out;
        }
        let Some(unit) = read.units.first() else {
            return out;
        };
        match read.fragmentation_indicator {
            COMPLETE => {
                drop_held(asset, &mut self.stats);
                asset.orphan = false;
                emit(
                    &mut out,
                    &mut self.stats,
                    asset,
                    packet.packet_id,
                    read.mpu_sequence_number,
                    unit,
                    unit.data.to_vec(),
                    packet.rap,
                );
            }
            FIRST => {
                drop_held(asset, &mut self.stats);
                asset.orphan = false;
                if unit.data.len() > self.most_held {
                    overflow(asset, &mut self.stats);
                    return out;
                }
                asset.holding = true;
                asset.held_mpu = read.mpu_sequence_number;
                asset.held_rap = packet.rap;
                asset.held_unit = keep(unit);
                asset.held.clear();
                asset.held.extend_from_slice(unit.data);
            }
            MIDDLE => {
                if !asset.holding {
                    if !asset.orphan {
                        self.stats.fragment_errors += 1;
                        asset.tally.errors += 1;
                        asset.after_loss = true;
                        asset.orphan = true;
                    }
                    return out;
                }
                if asset.held.len() + unit.data.len() > self.most_held {
                    overflow(asset, &mut self.stats);
                    return out;
                }
                asset.held.extend_from_slice(unit.data);
            }
            LAST => {
                if !asset.holding {
                    if !asset.orphan {
                        self.stats.fragment_errors += 1;
                        asset.tally.errors += 1;
                        asset.after_loss = true;
                    }
                    // The unit is over either way, so the next opening is not
                    // the one being complained about.
                    asset.orphan = false;
                    return out;
                }
                if asset.held.len() + unit.data.len() > self.most_held {
                    overflow(asset, &mut self.stats);
                    asset.orphan = false;
                    return out;
                }
                asset.held.extend_from_slice(unit.data);
                asset.holding = false;
                let data = std::mem::take(&mut asset.held);
                let held_unit = asset.held_unit;
                let held_mpu = asset.held_mpu;
                let held_rap = asset.held_rap;
                emit(
                    &mut out,
                    &mut self.stats,
                    asset,
                    packet.packet_id,
                    held_mpu,
                    &held_unit,
                    data,
                    held_rap,
                );
            }
            _ => {}
        }
        out
    }
}

/// How far back a counter may go and still be the same packet sent again
/// rather than a counter that started over ([`track`]).
const REPLAYED_WITHIN: u32 = 16;

/// How far forward a counter may jump and still be packets that were lost
/// rather than another sender's counter.
const LOST_AT_MOST: u32 = 4096;

/// Whether the packet is in order, and what its counter says was lost. The
/// subtraction carries the wrap for free: a difference in the top half of the
/// range is a packet that has gone backwards, not four billion lost.
fn track(asset: &mut Asset, packet: &Mmtp<'_>, stats: &mut Stats) -> bool {
    if !asset.have_expect {
        asset.expect = packet.sequence_number.wrapping_add(1);
        asset.have_expect = true;
        return true;
    }
    let gap = packet.sequence_number.wrapping_sub(asset.expect);
    if gap >= 1 << 31 {
        if asset.expect.wrapping_sub(packet.sequence_number) <= REPLAYED_WITHIN {
            stats.out_of_order_packets += 1;
            asset.tally.out_of_order += 1;
            return false;
        }
        stats.restarted_counters += 1;
        asset.tally.restarted += 1;
        drop_held(asset, stats);
        asset.after_loss = true;
        asset.expect = packet.sequence_number.wrapping_add(1);
        return true;
    }
    if gap > 0 {
        // **A leap too long to be a loss is another counter.** What was
        // held belongs to the old numbering either way.
        if gap <= LOST_AT_MOST {
            stats.sequence_gaps += 1;
            stats.lost_packets += u64::from(gap);
            asset.tally.lost += u64::from(gap);
        }
        drop_held(asset, stats);
        asset.after_loss = true;
    }
    asset.expect = packet.sequence_number.wrapping_add(1);
    true
}

/// The next access unit number for this asset, counted afresh whenever the
/// processing unit changes.
fn number(asset: &mut Asset, mpu_sequence_number: u32) -> u32 {
    if !asset.have_mpu || asset.mpu_sequence_number != mpu_sequence_number {
        asset.mpu_sequence_number = mpu_sequence_number;
        asset.have_mpu = true;
        asset.number = 0;
    }
    asset.number += 1;
    asset.number
}

#[allow(clippy::too_many_arguments)]
fn emit(
    out: &mut Vec<AccessUnit>,
    stats: &mut Stats,
    asset: &mut Asset,
    packet_id: u16,
    mpu_sequence_number: u32,
    unit: &mmtp::DataUnit<'_>,
    data: Vec<u8>,
    rap: bool,
) {
    if unit.sample_number != 0 {
        stats.non_zero_samples += 1;
    }
    if unit.offset != 0 {
        stats.non_zero_offsets += 1;
    }
    if unit.movie_fragment_sequence_number != 0 {
        stats.non_zero_movie_fragments += 1;
    }
    stats.access_units += 1;
    let number = number(asset, mpu_sequence_number);
    out.push(AccessUnit {
        packet_id,
        mpu_sequence_number,
        number,
        data,
        rap,
        timed: true,
        after_loss: std::mem::take(&mut asset.after_loss),
        sample_number: unit.sample_number,
        offset: unit.offset,
        item_id: unit.item_id,
        fragmentation: 0,
    });
}

/// The header of a split unit, kept past the payload that carried it. Only the
/// borrowed bytes are dropped -- what is kept is the numbering.
fn keep(unit: &mmtp::DataUnit<'_>) -> mmtp::DataUnit<'static> {
    mmtp::DataUnit {
        movie_fragment_sequence_number: unit.movie_fragment_sequence_number,
        sample_number: unit.sample_number,
        offset: unit.offset,
        priority: unit.priority,
        dependency_counter: unit.dependency_counter,
        item_id: unit.item_id,
        data: &[],
    }
}

/// Throws away a split unit that will never be finished.
fn drop_held(asset: &mut Asset, stats: &mut Stats) {
    if asset.holding {
        stats.fragment_errors += 1;
        asset.tally.errors += 1;
        asset.holding = false;
        asset.held.clear();
        asset.after_loss = true;
    }
}

/// A split unit longer than anything a picture is. Whatever the stream is
/// doing, it is not sending one unit, so the rest of it is ignored until
/// something opens a new one.
fn overflow(asset: &mut Asset, stats: &mut Stats) {
    stats.fragment_errors += 1;
    asset.tally.errors += 1;
    asset.holding = false;
    asset.held = Vec::new();
    asset.orphan = true;
    asset.after_loss = true;
}

#[cfg(test)]
mod tests {
    use super::{AccessUnit, Reassembler};
    use crate::mmtp::{self, COMPLETE, FIRST, LAST, MFU, MIDDLE};

    /// An MMTP packet carrying one MPU payload, built the way the standard
    /// says and not the way the reader happens to read it.
    #[allow(clippy::too_many_arguments)]
    fn packet(
        packet_id: u16,
        sequence_number: u32,
        rap: bool,
        fragment_type: u8,
        timed: bool,
        fragmentation_indicator: u8,
        aggregation_flag: bool,
        mpu_sequence_number: u32,
        units: &[(u32, &[u8])],
    ) -> Vec<u8> {
        let mut body = Vec::new();
        for (sample_number, data) in units {
            let mut one = Vec::new();
            if fragment_type == MFU {
                if timed {
                    one.extend_from_slice(&0u32.to_be_bytes());
                    one.extend_from_slice(&sample_number.to_be_bytes());
                    one.extend_from_slice(&0u32.to_be_bytes());
                    one.push(0);
                    one.push(0);
                } else {
                    one.extend_from_slice(&sample_number.to_be_bytes());
                }
            }
            one.extend_from_slice(data);
            if aggregation_flag {
                body.extend_from_slice(&(one.len() as u16).to_be_bytes());
            }
            body.extend_from_slice(&one);
        }
        let mut payload = vec![
            0,
            0,
            (fragment_type << 4)
                | u8::from(timed) << 3
                | (fragmentation_indicator << 1)
                | u8::from(aggregation_flag),
            0,
        ];
        payload.extend_from_slice(&mpu_sequence_number.to_be_bytes());
        payload.extend_from_slice(&body);
        let length = (payload.len() - 2) as u16;
        payload[..2].copy_from_slice(&length.to_be_bytes());

        let mut out = vec![u8::from(rap), mmtp::MPU];
        out.extend_from_slice(&packet_id.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&sequence_number.to_be_bytes());
        out.extend_from_slice(&payload);
        out
    }

    /// A whole timed unit in one packet.
    fn whole(packet_id: u16, sequence_number: u32, mpu: u32, data: &[u8]) -> Vec<u8> {
        packet(
            packet_id,
            sequence_number,
            true,
            MFU,
            true,
            COMPLETE,
            false,
            mpu,
            &[(0, data)],
        )
    }

    fn push(reassembler: &mut Reassembler, bytes: &[u8]) -> Vec<AccessUnit> {
        let read = mmtp::read(bytes).expect("the packet is whole");
        reassembler.push(&read)
    }

    #[test]
    fn a_split_unit_is_put_back_together_from_its_pieces() {
        let mut reassembler = Reassembler::new();
        let first = packet(1, 0, true, MFU, true, FIRST, false, 7, &[(0, b"one ")]);
        let middle = packet(1, 1, false, MFU, true, MIDDLE, false, 7, &[(0, b"two ")]);
        let last = packet(1, 2, false, MFU, true, LAST, false, 7, &[(0, b"three")]);
        assert!(push(&mut reassembler, &first).is_empty());
        assert!(push(&mut reassembler, &middle).is_empty());
        let out = push(&mut reassembler, &last);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].data, b"one two three".as_slice());
        assert_eq!(out[0].mpu_sequence_number, 7);
        assert_eq!(out[0].number, 1);
        // The random access point is the one the unit opened with, not the one
        // the packet that closed it happened to carry.
        assert!(out[0].rap);
        assert!(!out[0].after_loss);
        assert_eq!(reassembler.stats().access_units, 1);
        assert_eq!(reassembler.stats().fragment_errors, 0);
    }

    #[test]
    fn several_units_share_one_aggregating_payload() {
        let mut reassembler = Reassembler::new();
        let bytes = packet(
            1,
            0,
            true,
            MFU,
            true,
            COMPLETE,
            true,
            3,
            &[(0, b"a"), (0, b"bb"), (0, b"ccc")],
        );
        let out = push(&mut reassembler, &bytes);
        let data: Vec<&[u8]> = out.iter().map(|unit| unit.data.as_slice()).collect();
        assert_eq!(data, vec![&b"a"[..], &b"bb"[..], &b"ccc"[..]]);
        // Three units of the one processing unit, numbered in turn.
        let numbers: Vec<u32> = out.iter().map(|unit| unit.number).collect();
        assert_eq!(numbers, vec![1, 2, 3]);
        assert_eq!(reassembler.stats().access_units, 3);
    }

    /// The reason any of this counts for itself: broadcast leaves the sample
    /// number at nought on every unit of every service.
    #[test]
    fn units_are_numbered_here_because_the_stream_leaves_it_at_nought() {
        let mut reassembler = Reassembler::new();
        let mut numbers = Vec::new();
        let mut sequence = 0;
        for mpu in 100..103u32 {
            for _ in 0..3 {
                let bytes = whole(1, sequence, mpu, b"x");
                sequence += 1;
                for unit in push(&mut reassembler, &bytes) {
                    assert_eq!(unit.sample_number, 0, "the stream numbers nothing");
                    numbers.push((unit.mpu_sequence_number, unit.number));
                }
            }
        }
        assert_eq!(
            numbers,
            vec![
                (100, 1),
                (100, 2),
                (100, 3),
                (101, 1),
                (101, 2),
                (101, 3),
                (102, 1),
                (102, 2),
                (102, 3),
            ]
        );
        assert_eq!(reassembler.stats().non_zero_samples, 0);
    }

    /// Two identifiers are two assets, and neither's split unit is the
    /// other's.
    #[test]
    fn assets_are_kept_apart_by_their_packet_identifier() {
        let mut reassembler = Reassembler::new();
        let first = packet(1, 0, true, MFU, true, FIRST, false, 1, &[(0, b"aa")]);
        let other = whole(2, 0, 1, b"zz");
        let last = packet(1, 1, false, MFU, true, LAST, false, 1, &[(0, b"bb")]);
        assert!(push(&mut reassembler, &first).is_empty());
        let out = push(&mut reassembler, &other);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].packet_id, 2);
        let out = push(&mut reassembler, &last);
        assert_eq!(out[0].data, b"aabb".as_slice());
        assert_eq!(reassembler.stats().fragment_errors, 0);
    }

    #[test]
    fn a_gap_in_the_packets_throws_away_what_was_being_held() {
        let mut reassembler = Reassembler::new();
        let first = packet(1, 0, true, MFU, true, FIRST, false, 1, &[(0, b"aa")]);
        // The middle piece never arrives, and the counter says so.
        let last = packet(1, 2, false, MFU, true, LAST, false, 1, &[(0, b"cc")]);
        assert!(push(&mut reassembler, &first).is_empty());
        assert!(push(&mut reassembler, &last).is_empty(), "half a unit");
        assert_eq!(reassembler.stats().sequence_gaps, 1);
        assert_eq!(reassembler.stats().lost_packets, 1);
        // Two complaints, and both are true: the gap threw away what was held,
        // and the piece that then arrived had nothing to be the end of.
        assert_eq!(reassembler.stats().fragment_errors, 2);
        // The next whole unit arrives, and says what happened before it.
        let next = whole(1, 3, 1, b"dd");
        let out = push(&mut reassembler, &next);
        assert_eq!(out.len(), 1);
        assert!(out[0].after_loss);
        // Only the one unit was ever completed, so it is the first.
        assert_eq!(out[0].number, 1);
        let after = whole(1, 4, 1, b"ee");
        let out = push(&mut reassembler, &after);
        assert!(!out[0].after_loss, "the loss is reported once");
        assert_eq!(out[0].number, 2);
    }

    /// ストリームの途中から選局された、先頭の無い断片は誤りではない。
    #[test]
    fn the_pieces_before_the_first_opening_are_a_join_and_not_an_error() {
        let mut reassembler = Reassembler::new();
        let middle = packet(1, 0, false, MFU, true, MIDDLE, false, 1, &[(0, b"bb")]);
        let more = packet(1, 1, false, MFU, true, MIDDLE, false, 1, &[(0, b"cc")]);
        let last = packet(1, 2, false, MFU, true, LAST, false, 1, &[(0, b"dd")]);
        assert!(push(&mut reassembler, &middle).is_empty());
        assert!(push(&mut reassembler, &more).is_empty());
        assert!(push(&mut reassembler, &last).is_empty());
        assert_eq!(reassembler.stats().fragment_errors, 0);
        assert_eq!(reassembler.stats().access_units, 0);
        // 続く完全な単位は、欠落のあとではない。
        let out = push(&mut reassembler, &whole(1, 3, 1, b"ee"));
        assert_eq!(out.len(), 1);
        assert!(!out[0].after_loss);
    }

    /// **一度始まったあとの、先頭の無い断片は誤りです。**一つの単位につき
    /// 一度だけ報告します。
    #[test]
    fn a_piece_without_its_opening_is_dropped_and_complained_about_once() {
        let mut reassembler = Reassembler::new();
        push(&mut reassembler, &whole(1, 0, 1, b"aa"));
        let middle = packet(1, 1, false, MFU, true, MIDDLE, false, 1, &[(0, b"bb")]);
        let more = packet(1, 2, false, MFU, true, MIDDLE, false, 1, &[(0, b"cc")]);
        let last = packet(1, 3, false, MFU, true, LAST, false, 1, &[(0, b"dd")]);
        assert!(push(&mut reassembler, &middle).is_empty());
        assert!(push(&mut reassembler, &more).is_empty());
        assert!(push(&mut reassembler, &last).is_empty());
        assert_eq!(reassembler.stats().fragment_errors, 1);
        assert_eq!(reassembler.stats().access_units, 1);
    }

    #[test]
    fn an_opening_while_one_is_held_abandons_the_one_being_held() {
        let mut reassembler = Reassembler::new();
        let first = packet(1, 0, true, MFU, true, FIRST, false, 1, &[(0, b"aa")]);
        let again = packet(1, 1, true, MFU, true, FIRST, false, 1, &[(0, b"xx")]);
        let last = packet(1, 2, false, MFU, true, LAST, false, 1, &[(0, b"yy")]);
        push(&mut reassembler, &first);
        push(&mut reassembler, &again);
        let out = push(&mut reassembler, &last);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].data, b"xxyy".as_slice());
        assert!(out[0].after_loss);
        assert_eq!(reassembler.stats().fragment_errors, 1);
    }

    #[test]
    fn a_packet_that_has_gone_backwards_is_dropped() {
        let mut reassembler = Reassembler::new();
        push(&mut reassembler, &whole(1, 5, 1, b"aa"));
        push(&mut reassembler, &whole(1, 6, 1, b"bb"));
        let out = push(&mut reassembler, &whole(1, 5, 1, b"aa"));
        assert!(out.is_empty());
        assert_eq!(reassembler.stats().out_of_order_packets, 1);
        assert_eq!(reassembler.stats().access_units, 2);
    }

    #[test]
    fn a_counter_that_started_over_is_followed() {
        let mut reassembler = Reassembler::new();
        push(&mut reassembler, &whole(1, 1_999_999, 1_000_000, b"aa"));
        push(&mut reassembler, &whole(1, 2_000_000, 1_000_000, b"bb"));
        let out = push(&mut reassembler, &whole(1, 80_000, 80_000, b"cc"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].data, b"cc".as_slice());
        assert!(out[0].after_loss);
        // A splice is not a fault: nothing went backwards and was dropped.
        assert_eq!(reassembler.stats().out_of_order_packets, 0);
        assert_eq!(reassembler.stats().restarted_counters, 1);
        assert_eq!(reassembler.tally(1).unwrap().restarted, 1);
        assert_eq!(reassembler.tally(1).unwrap().out_of_order, 0);
        // And the count goes on from there.
        let out = push(&mut reassembler, &whole(1, 80_001, 80_001, b"dd"));
        assert_eq!(out.len(), 1);
        // Back to the first numbering: a leap of two million forward is
        // the other counter again, not two million packets lost.
        let out = push(&mut reassembler, &whole(1, 2_000_001, 1_000_001, b"ee"));
        assert_eq!(out.len(), 1);
        assert!(out[0].after_loss);
        assert_eq!(reassembler.stats().lost_packets, 0);
        assert_eq!(reassembler.tally(1).unwrap().lost, 0);
        // A short gap is still a loss.
        push(&mut reassembler, &whole(1, 2_000_006, 1_000_002, b"ff"));
        assert_eq!(reassembler.stats().lost_packets, 4);
    }

    #[test]
    fn a_scrambled_payload_is_not_read_and_takes_the_held_unit_with_it() {
        let mut reassembler = Reassembler::new();
        push(
            &mut reassembler,
            &packet(1, 0, true, MFU, true, FIRST, false, 1, &[(0, b"aa")]),
        );
        let bytes = whole(1, 1, 1, b"bb");
        let mut scrambled = mmtp::read(&bytes).expect("the packet is whole");
        scrambled.scrambled = true;
        assert!(reassembler.push(&scrambled).is_empty());
        assert_eq!(reassembler.stats().scrambled_packets, 1);
        assert_eq!(reassembler.stats().fragment_errors, 1);
    }

    /// 識別子ごとの数は、全体の数をアセットごとに分けたもの。
    #[test]
    fn each_packet_identifier_keeps_its_own_tally() {
        let mut reassembler = Reassembler::new();
        for (packet_id, sequence) in [(1, 0), (2, 0), (1, 1), (1, 3), (2, 1)] {
            let bytes = whole(packet_id, sequence, 0, &[0, 0, 0, 1, 0x65]);
            reassembler.push(&mmtp::read(&bytes).expect("the packet is whole"));
        }
        let ones = reassembler.tally(1).expect("packets arrived on 1");
        assert_eq!((ones.arrived, ones.lost), (3, 1));
        let twos = reassembler.tally(2).expect("packets arrived on 2");
        assert_eq!((twos.arrived, twos.lost), (2, 0));
        assert!(reassembler.tally(3).is_none());
        assert_eq!(reassembler.packet_ids().collect::<Vec<_>>(), [1, 2]);
        assert_eq!(reassembler.stats().lost_packets, 1);
    }

    #[test]
    fn a_payload_that_will_not_parse_is_counted_and_dropped() {
        let mut reassembler = Reassembler::new();
        // A length longer than the payload it is at the front of.
        let mut bytes = vec![0, mmtp::MPU, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        bytes.extend_from_slice(&[0xff, 0xff, 0x20, 0, 0, 0, 0, 1]);
        assert!(push(&mut reassembler, &bytes).is_empty());
        assert_eq!(reassembler.stats().parse_errors, 1);
    }

    #[test]
    fn the_boxes_that_describe_the_media_are_not_the_media() {
        let mut reassembler = Reassembler::new();
        let metadata = packet(
            1,
            0,
            false,
            mmtp::MPU_METADATA,
            true,
            COMPLETE,
            false,
            1,
            &[(0, b"moov")],
        );
        assert!(push(&mut reassembler, &metadata).is_empty());
        assert_eq!(reassembler.stats().metadata_fragments, 1);
    }

    #[test]
    fn an_untimed_asset_arrives_whole() {
        let mut reassembler = Reassembler::new();
        let bytes = packet(1, 0, false, MFU, false, COMPLETE, false, 1, &[(9, b"file")]);
        let out = push(&mut reassembler, &bytes);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].data, b"file".as_slice());
        assert!(!out[0].timed);
        assert_eq!(out[0].item_id, 9);
        assert_eq!(reassembler.stats().untimed_units, 1);
    }

    #[test]
    fn a_unit_longer_than_anything_a_picture_is_gets_abandoned() {
        let mut reassembler = Reassembler::with_limit(4);
        let first = packet(1, 0, true, MFU, true, FIRST, false, 1, &[(0, b"aa")]);
        let middle = packet(1, 1, false, MFU, true, MIDDLE, false, 1, &[(0, b"bbb")]);
        let last = packet(1, 2, false, MFU, true, LAST, false, 1, &[(0, b"cc")]);
        push(&mut reassembler, &first);
        assert!(push(&mut reassembler, &middle).is_empty());
        assert!(push(&mut reassembler, &last).is_empty());
        assert_eq!(reassembler.stats().access_units, 0);
        assert!(reassembler.stats().fragment_errors >= 1);
        // And the stream is picked up again at the next opening.
        let out = push(&mut reassembler, &whole(1, 3, 1, b"dd"));
        assert_eq!(out.len(), 1);
        assert!(out[0].after_loss);
    }
}

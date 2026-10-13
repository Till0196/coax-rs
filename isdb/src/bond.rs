// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Putting a multiplexing frame back together when it was sent over several
//! carriers.
//!
//! A frame too wide for one carrier is split across several: each carries the
//! group identifier they share, each sends its own frames with its own place in
//! the group, and every carrier of a group holds the same streams. A receiver
//! that tunes one of them gets a third of a stream, so all of them are tuned
//! and their slots put back in order.
//!
//! That order is not "one carrier then the next". A superframe is a run of
//! frames; within it the slots go round the carriers, and the frame a slot
//! belongs to falls out of the arithmetic. What is here follows the order the
//! receiver that reads these streams today uses, because two implementations
//! that disagree produce two different streams.

use std::collections::BTreeMap;

use crate::tsmf::{Carriage, Frame, SLOTS, TlvJoin};
use mmttlv::tlv;
use mpegts::{PACKET, SYNC};

/// How many slots a frame holds, counting the header as the first of them,
/// which is what the arithmetic below counts in.
const SLOTS_WITH_HEADER: usize = SLOTS + 1;

/// The most frames a superframe may hold.
const MOST_FRAMES: usize = 15;

/// How many superframes to keep back before handing anything on, so that a
/// carrier that arrives a little later than the others has somewhere to land.
const HELD_BACK: usize = 2;

/// The most frames to hold for a carrier whose superframes never complete.
const MOST_HELD: usize = 32;

/// How many superframes every carrier must have before the carriers are lined
/// up against each other, and how many more to gather before trying again
/// after a try that settled nothing.
const ENOUGH_TO_LINE_UP: usize = 10;
const GATHER_AGAIN: usize = 60;

/// How many superframes to put through a candidate alignment when scoring it.
const SCORED_OVER: usize = 30;

/// The most superframes to hold while the carriers have not been lined up.
const MOST_UNALIGNED: usize = 600;

/// How far one carrier may run ahead of the slowest once they are lined up.
/// Further than this and the slowest has stopped, or the alignment no longer
/// holds; either way what the others pile up is never going to be handed on,
/// so the alignment is given up and worked out again -- under the limit above.
const MOST_AHEAD: usize = GATHER_AGAIN;

/// The most carriers a group may have, as the frame header is read
/// (`tsmf::Group`).
const MOST_CARRIERS: u8 = 16;

/// How much of a scored run must read as TLV for the alignment to be believed,
/// how many compressed packets it must hold, and how far ahead of the next
/// candidate it must be. A wrong alignment scores near nothing, so these are
/// not close calls.
const BELIEVED_ABOVE: f32 = 0.9;
const ENOUGH_COMPRESSED: usize = 16;
const AHEAD_BY: f32 = 0.02;

/// How many packets a transport stream must be scored over.
const ENOUGH_COUNTED: usize = 16;

/// One carrier of the group, and what has arrived on it.
#[derive(Debug, Default)]
struct Carrier {
    /// Frames that have not yet been made into a superframe.
    waiting: Vec<Frame>,
    /// The superframes made from them, oldest first.
    superframes: Vec<Vec<Frame>>,
}

/// The carriers of one group, bonded.
#[derive(Debug)]
pub struct Bond {
    /// How many carriers the group has, as the frames say.
    carriers: u8,
    carriage: Carriage,
    held: BTreeMap<u8, Carrier>,
    tlv: TlvJoin,
    /// How many superframes to drop from the front of each carrier so that
    /// they line up, once that has been worked out.
    offsets: Option<Vec<usize>>,
    applied: bool,
    /// How much has to pile up before trying to line them up again.
    try_again_at: usize,
}

impl Bond {
    pub fn new(carriage: Carriage) -> Self {
        Self {
            carriers: 0,
            carriage,
            held: BTreeMap::new(),
            tlv: TlvJoin::new(),
            offsets: None,
            applied: false,
            try_again_at: 0,
        }
    }

    /// How many carriers the group turned out to have, once a frame has said.
    pub fn carriers(&self) -> u8 {
        self.carriers
    }

    /// Takes frames from one carrier and gives back whatever the group is now
    /// able to say.
    pub fn feed(&mut self, frames: Vec<Frame>) -> Vec<u8> {
        for frame in frames {
            let sequence = frame.sequence.max(1);
            let carriers = frame.carriers.max(1);
            // A frame that names a carrier the group cannot have is a damaged
            // one, and taking it would start a carrier that never completes.
            if carriers > MOST_CARRIERS || sequence > carriers {
                continue;
            }
            if self.offsets.is_some() {
                // Once lined up, the group is what it was lined up as: a frame
                // that says otherwise is damaged, and believing it would hold
                // everything back waiting for a carrier that is not there.
                if sequence > self.carriers || !self.held.contains_key(&sequence) {
                    continue;
                }
            } else {
                self.carriers = self.carriers.max(carriers);
            }
            let carrier = self.held.entry(sequence).or_default();
            carrier.waiting.push(frame);
            pack(carrier);
            if carrier.waiting.len() > MOST_HELD {
                let drop = carrier.waiting.len() - MOST_HELD;
                carrier.waiting.drain(..drop);
            }
        }
        if self.offsets.is_none() {
            // Nothing may be handed on before the carriers are lined up, so
            // what has arrived is kept -- but not for ever.
            for carrier in self.held.values_mut() {
                if carrier.superframes.len() > MOST_UNALIGNED {
                    let drop = carrier.superframes.len() - MOST_UNALIGNED;
                    carrier.superframes.drain(..drop);
                }
            }
            self.line_up();
        }
        self.drain()
    }

    /// Works out how far apart the carriers are running.
    ///
    /// They are tuned one after another and arrive when they arrive, so one may
    /// be a superframe or two ahead of the next, and which way round cannot be
    /// measured from the outside. So every likely alignment is tried and the
    /// one that comes out framed as a TLV stream is the right one -- which is
    /// what makes this decidable without understanding a byte of the payload.
    fn line_up(&mut self) {
        if self.held.len() < usize::from(self.carriers.max(1)) {
            return;
        }
        let gathered = self
            .held
            .values()
            .map(|carrier| carrier.superframes.len())
            .min()
            .unwrap_or(0);
        if gathered < ENOUGH_TO_LINE_UP.max(self.try_again_at) {
            return;
        }
        let carriers: Vec<&Carrier> = self.held.values().collect();
        let mut scored: Vec<(Vec<usize>, f32)> = Vec::new();
        for offsets in candidates(&carriers) {
            let Some(built) = assemble(&carriers, &offsets, self.carriage, SCORED_OVER) else {
                continue;
            };
            let Some(valid) = score(&built, self.carriage) else {
                continue;
            };
            scored.push((offsets, valid));
        }
        scored.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let Some((best, valid)) = scored.first().cloned() else {
            // Nothing read as a stream, so more is gathered and it is tried
            // again rather than guessed at.
            self.try_again_at = gathered + GATHER_AGAIN;
            return;
        };
        if self.carriers > 1
            && let Some((_, runner_up)) = scored.get(1)
            && valid - runner_up < AHEAD_BY
        {
            // Two alignments look alike, which means neither has been
            // shown to be the one.
            self.try_again_at = gathered + GATHER_AGAIN;
            return;
        }
        self.offsets = Some(best);
        self.applied = false;
    }

    /// The slots of every superframe that all the carriers have reached, in
    /// the order they were sent.
    fn drain(&mut self) -> Vec<u8> {
        let wanted = usize::from(self.carriers.max(1));
        if self.held.len() < wanted {
            return Vec::new();
        }
        let Some(offsets) = self.offsets.clone() else {
            return Vec::new();
        };
        if !self.applied {
            for (carrier, drop) in self.held.values().zip(offsets.iter()) {
                if carrier.superframes.len() <= *drop {
                    return Vec::new();
                }
            }
            for (carrier, drop) in self.held.values_mut().zip(offsets.iter()) {
                carrier.superframes.drain(..*drop);
            }
            self.applied = true;
        }
        let ready = self
            .held
            .values()
            .map(|carrier| carrier.superframes.len())
            .min()
            .unwrap_or(0);
        let furthest = self
            .held
            .values()
            .map(|carrier| carrier.superframes.len())
            .max()
            .unwrap_or(0);
        if furthest > ready + MOST_AHEAD {
            self.offsets = None;
            self.applied = false;
            self.try_again_at = 0;
            return Vec::new();
        }
        if ready <= HELD_BACK {
            return Vec::new();
        }
        let taking = ready - HELD_BACK;
        let mut out = Vec::new();
        for which in 0..taking {
            let superframes: Vec<&Vec<Frame>> = self
                .held
                .values()
                .map(|carrier| &carrier.superframes[which])
                .collect();
            for packet in slots_in_order(&superframes) {
                match self.carriage {
                    Carriage::Transport => out.extend_from_slice(packet),
                    Carriage::Tlv => self.tlv.take(packet, &mut out),
                }
            }
        }
        for carrier in self.held.values_mut() {
            carrier.superframes.drain(..taking);
        }
        out
    }
}

/// The alignments worth trying: the carriers as they stand, and every way of
/// nudging one or two of them by a superframe or two.
fn candidates(carriers: &[&Carrier]) -> Vec<Vec<usize>> {
    let counts: Vec<usize> = carriers
        .iter()
        .map(|carrier| carrier.superframes.len())
        .collect();
    let fewest = counts.iter().copied().min().unwrap_or(0);
    let base: Vec<i64> = counts.iter().map(|count| (count - fewest) as i64).collect();
    let mut seen: Vec<String> = Vec::new();
    let mut candidates = Vec::new();
    let many = carriers.len() > 1;
    let mut add = |offsets: Vec<i64>| {
        if offsets.iter().any(|offset| *offset < 0) {
            return;
        }
        // With more than one carrier only the distance between them is a
        // different guess: shifting them all by the same amount tries the same
        // alignment against a different window.
        let key = if many {
            offsets
                .iter()
                .map(|offset| (offset - offsets[0]).to_string())
                .collect::<Vec<_>>()
                .join(",")
        } else {
            offsets
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        if seen.contains(&key) {
            return;
        }
        seen.push(key);
        candidates.push(offsets.iter().map(|offset| *offset as usize).collect());
    };

    add(base.clone());
    for which in 0..carriers.len() {
        for step in [1i64, -1, 2, -2] {
            let mut nudged = base.clone();
            nudged[which] += step;
            add(nudged);
        }
    }
    for left in 0..carriers.len() {
        for right in left + 1..carriers.len() {
            for one in [-1i64, 1] {
                for other in [-1i64, 1] {
                    let mut nudged = base.clone();
                    nudged[left] += one;
                    nudged[right] += other;
                    add(nudged);
                }
            }
        }
    }
    candidates
}

/// How well a run of bytes reads as the thing it is supposed to be, or nothing
/// where it does not read as it at all. This is what settles the alignment: a
/// stream put together the wrong way round is not merely worse, it is not
/// framed at all.
fn score(built: &[u8], carriage: Carriage) -> Option<f32> {
    match carriage {
        Carriage::Tlv => {
            let at = tlv::find_sync(built)?;
            let seen = tlv::integrity(built, at);
            if seen.valid < BELIEVED_ABOVE
                || seen.compressed < ENOUGH_COMPRESSED
                || seen.compressed_ok != seen.compressed
            {
                return None;
            }
            Some(seen.valid)
        }
        // A transport stream has no framing to lose -- every slot is a whole
        // packet whichever order they go in -- so what says whether the order
        // is right is the counter each stream carries.
        Carriage::Transport => {
            let (valid, counted) = continuity(built);
            if counted < ENOUGH_COUNTED || valid < BELIEVED_ABOVE {
                return None;
            }
            Some(valid)
        }
    }
}

/// How much of a transport stream runs in sequence: how many packets follow
/// the one before them on their own identifier, out of how many were looked
/// at.
fn continuity(built: &[u8]) -> (f32, usize) {
    let mut last: BTreeMap<u16, u8> = BTreeMap::new();
    let mut counted = 0usize;
    let mut in_order = 0usize;
    for packet in built.as_chunks::<PACKET>().0 {
        if packet[0] != SYNC {
            continue;
        }
        let pid = (u16::from(packet[1] & 0x1f) << 8) | u16::from(packet[2]);
        let counter = packet[3] & 0x0f;
        // A packet with no payload does not move the counter on.
        let moves = packet[3] & 0x10 != 0;
        if let Some(before) = last.insert(pid, counter) {
            counted += 1;
            let expected = if moves { (before + 1) & 0x0f } else { before };
            if counter == expected {
                in_order += 1;
            }
        }
    }
    if counted == 0 {
        return (0.0, 0);
    }
    (in_order as f32 / counted as f32, counted)
}

/// Puts a run of superframes together under one alignment for scoring.
/// Nothing is consumed: this is a question, not an answer.
fn assemble(
    carriers: &[&Carrier],
    offsets: &[usize],
    carriage: Carriage,
    most: usize,
) -> Option<Vec<u8>> {
    let available = carriers
        .iter()
        .zip(offsets.iter())
        .map(|(carrier, offset)| carrier.superframes.len().saturating_sub(*offset))
        .min()
        .unwrap_or(0);
    let taking = available.min(most);
    if taking == 0 {
        return None;
    }
    let mut join = TlvJoin::new();
    let mut built = Vec::new();
    for which in 0..taking {
        let superframes: Vec<&Vec<Frame>> = carriers
            .iter()
            .zip(offsets.iter())
            .map(|(carrier, offset)| &carrier.superframes[offset + which])
            .collect();
        for packet in slots_in_order(&superframes) {
            match carriage {
                Carriage::Tlv => join.take(packet, &mut built),
                Carriage::Transport => built.extend_from_slice(packet),
            }
        }
    }
    Some(built)
}

/// Makes superframes out of whatever runs of frames are complete: a run starts
/// at frame nought and holds as many frames as that frame says the run has.
fn pack(carrier: &mut Carrier) {
    let mut at = 0;
    while at < carrier.waiting.len() {
        let of = usize::from(carrier.waiting[at].of);
        if carrier.waiting[at].position != 0 || of == 0 || of > MOST_FRAMES {
            at += 1;
            continue;
        }
        if carrier.waiting.len() < at + of {
            return;
        }
        let whole = carrier.waiting[at..at + of]
            .iter()
            .enumerate()
            .all(|(place, frame)| {
                usize::from(frame.of) == of && usize::from(frame.position) == place
            });
        if !whole {
            at += 1;
            continue;
        }
        let run: Vec<Frame> = carrier.waiting.drain(at..at + of).collect();
        carrier.superframes.push(run);
    }
}

/// The slots of one superframe from every carrier, in the order they were sent.
/// The slots go round the carriers rather than down them, and which frame of
/// the superframe a slot belongs to falls out of the count: a superframe of `n`
/// frames holds `n` slots at each position of the round, and the header slot of
/// every frame is not data.
fn slots_in_order<'a>(superframes: &[&'a Vec<Frame>]) -> Vec<&'a [u8]> {
    let mut packets = Vec::new();
    for step in 0..SLOTS_WITH_HEADER {
        for within in 0..4 {
            for superframe in superframes {
                let of = superframe.len();
                if within >= of {
                    continue;
                }
                let slot = step * of + within;
                let frame = slot / SLOTS_WITH_HEADER;
                let inside = slot % SLOTS_WITH_HEADER;
                if frame >= of || inside == 0 {
                    continue;
                }
                if let Some(Some(packet)) = superframe[frame].slots.get(inside - 1) {
                    packets.push(packet.as_slice());
                }
            }
        }
    }
    packets
}

#[cfg(test)]
mod tests {
    use super::{Bond, Carrier, pack};
    use crate::tsmf::{Carriage, Frame};
    use mpegts::{PACKET, SYNC};

    /// A frame of one carrier, whose slots hold the packets given.
    fn frame(position: u8, of: u8, sequence: u8, carriers: u8, packets: Vec<Vec<u8>>) -> Frame {
        let mut slots = vec![None; 52];
        for (at, packet) in packets.into_iter().enumerate() {
            slots[at] = Some(packet);
        }
        Frame {
            group: Some(7),
            position,
            of,
            sequence,
            carriers,
            slots,
        }
    }

    /// A packet of a transport stream, on one identifier, with the count it
    /// carries.
    fn ts(counter: u8) -> Vec<u8> {
        let mut packet = vec![0u8; PACKET];
        packet[0] = SYNC;
        packet[1] = 0x01;
        packet[2] = 0x00;
        packet[3] = 0x10 | (counter & 0x0f);
        packet
    }

    /// A superframe is a run of frames starting at nought, and a run that is
    /// not whole waits for the rest rather than being handed on.
    #[test]
    fn frames_are_made_into_superframes_only_when_the_run_is_whole() {
        let mut carrier = Carrier::default();
        carrier.waiting.push(frame(0, 2, 1, 1, vec![ts(0)]));
        pack(&mut carrier);
        assert!(carrier.superframes.is_empty(), "half a run is not a run");
        carrier.waiting.push(frame(1, 2, 1, 1, vec![ts(1)]));
        pack(&mut carrier);
        assert_eq!(carrier.superframes.len(), 1);
        assert!(carrier.waiting.is_empty());
    }

    /// Two carriers, each holding every other packet of one stream. Put back
    /// together in the right order the counts run on; in any other order they
    /// jump, which is what the bonding decides by.
    #[test]
    fn two_carriers_are_lined_up_by_what_reads_as_a_stream() {
        let mut bond = Bond::new(Carriage::Transport);
        let mut out = Vec::new();
        let mut counter = 0u8;
        for _ in 0..40 {
            // The first carrier holds the even packets and the second the odd
            // ones, which is what going round the carriers produces.
            let first = frame(0, 1, 1, 2, vec![ts(counter)]);
            let second = frame(0, 1, 2, 2, vec![ts(counter + 1)]);
            counter = counter.wrapping_add(2) & 0x0f;
            out.extend(bond.feed(vec![first]));
            out.extend(bond.feed(vec![second]));
        }
        assert_eq!(bond.carriers(), 2);
        assert!(!out.is_empty(), "the carriers were lined up");
        assert_eq!(out.len() % PACKET, 0);
        // What comes out runs in sequence: the count goes up by one from each
        // packet to the next.
        let counts: Vec<u8> = out
            .as_chunks::<PACKET>()
            .0
            .iter()
            .map(|packet| packet[3] & 0x0f)
            .collect();
        for pair in counts.windows(2) {
            assert_eq!(pair[1], (pair[0] + 1) & 0x0f, "counts: {counts:?}");
        }
    }

    /// A group says nothing until every carrier of it has been heard from: a
    /// third of a stream is not a stream.
    #[test]
    fn nothing_comes_out_until_every_carrier_has_spoken() {
        let mut bond = Bond::new(Carriage::Transport);
        for counter in 0..40u8 {
            assert!(
                bond.feed(vec![frame(0, 1, 1, 2, vec![ts(counter & 0x0f)])])
                    .is_empty(),
                "one carrier of two says nothing"
            );
        }
        assert_eq!(bond.carriers(), 2);
    }

    /// A carrier that stops once the group is lined up does not leave the
    /// others piling up for ever: the alignment is given up, and what is held
    /// while it is not is bounded.
    #[test]
    fn a_carrier_that_stops_does_not_leave_the_others_piling_up() {
        let mut bond = Bond::new(Carriage::Transport);
        let mut counter = 0u8;
        for _ in 0..40 {
            let first = frame(0, 1, 1, 2, vec![ts(counter)]);
            let second = frame(0, 1, 2, 2, vec![ts(counter + 1)]);
            counter = counter.wrapping_add(2) & 0x0f;
            bond.feed(vec![first]);
            bond.feed(vec![second]);
        }
        assert!(bond.offsets.is_some(), "the carriers were lined up");
        for _ in 0..2000 {
            bond.feed(vec![frame(0, 1, 1, 2, vec![ts(counter)])]);
            counter = counter.wrapping_add(2) & 0x0f;
        }
        let most = bond
            .held
            .values()
            .map(|carrier| carrier.superframes.len())
            .max()
            .unwrap_or(0);
        assert!(most <= super::MOST_UNALIGNED, "{most} superframes held");
    }

    /// A damaged frame naming a carrier the group does not have is not a new
    /// carrier.
    #[test]
    fn a_frame_naming_a_carrier_out_of_range_is_ignored() {
        let mut bond = Bond::new(Carriage::Transport);
        bond.feed(vec![frame(0, 1, 9, 2, vec![ts(0)])]);
        bond.feed(vec![frame(0, 1, 1, 200, vec![ts(0)])]);
        assert!(bond.held.is_empty());
        assert_eq!(bond.carriers(), 0);
    }
}

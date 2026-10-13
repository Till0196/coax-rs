// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! How much of a carrier arrived: what a receiver can say about a signal from
//! the bytes alone.
//!
//! Every packet of a transport stream carries a counter that steps by one for
//! each packet of its stream, so what the counter skips is what the receiver
//! did not get. It is the only measure of a signal that every tuner gives.

use crate::{PACKET, SYNC};

/// The packet identifier that carries nothing: the stuffing a multiplex is
/// padded to its rate with. Its counter does not have to step, and a run of it
/// is not a loss.
const NULL: u16 = 0x1FFF;

/// How many sync bytes have to line up, one packet apart, before an offset is
/// believed to be the packet boundary. One proves nothing: 0x47 is an
/// ordinary byte.
const CONFIRM: usize = 4;

/// Counts what arrived and what did not, over bytes handed in as they come. The
/// bytes need not be whole packets or start at one: a receiver's reads are as
/// long as its buffer and begin wherever the last one ended, so this keeps what
/// it could not use yet.
///
/// The packet boundary is believed only once several packets line up, and once
/// it is lost every stream's counter is forgotten: what follows a break does
/// not follow what came before it, and the break is counted once.
#[derive(Debug)]
pub struct Continuity {
    /// Bytes not yet counted: a packet the last feed ended in the middle of,
    /// or what is still being searched for the boundary.
    held: Vec<u8>,
    synced: bool,
    /// Whether the current search for the boundary has been counted as a
    /// break already.
    broken: bool,
    /// The counter last seen on each stream, by packet identifier. A stream
    /// carries four bits of counter, so this is one byte each and the array is
    /// the whole identifier space -- 8KB, against a map that would be asked a
    /// question per packet.
    last: [u8; 8192],
    packets: u64,
    errors: u64,
    /// Each place the reader had to look for the packet boundary, which is a
    /// break in the stream as surely as a skipped counter is.
    breaks: u64,
}

/// The value that says nothing has been seen on this stream yet, which no
/// counter can be: the counter is four bits.
const UNSEEN: u8 = 0xFF;

impl Default for Continuity {
    fn default() -> Self {
        Self::new()
    }
}

impl Continuity {
    pub fn new() -> Self {
        Self {
            held: Vec::new(),
            synced: false,
            broken: false,
            last: [UNSEEN; 8192],
            packets: 0,
            errors: 0,
            breaks: 0,
        }
    }

    /// How many packets went past, stuffing aside.
    pub fn packets(&self) -> u64 {
        self.packets
    }

    /// How many packets were missed: what the counters skipped, what the
    /// transmitter marked as received in error, and each place the reader lost
    /// the packet boundary.
    pub fn errors(&self) -> u64 {
        self.errors + self.breaks
    }

    /// What share of the carrier did not arrive, where enough of it went past
    /// to say. Nothing where too little did: a handful of packets says nothing
    /// about a signal, and a comparison drawn from it would be noise.
    pub fn losses(&self) -> Option<f32> {
        /// Fewer than this and the answer is not worth having: a second of a
        /// television multiplex is about twenty thousand packets, and this is
        /// a small fraction of a second.
        const ENOUGH: u64 = 2000;
        if self.packets < ENOUGH {
            return None;
        }
        Some(self.errors() as f32 / self.packets as f32)
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let mut held = std::mem::take(&mut self.held);
        held.extend_from_slice(bytes);
        let span = PACKET * (CONFIRM - 1);
        let mut at = 0;
        loop {
            if !self.synced {
                let aligned =
                    |offset: usize| (0..CONFIRM).all(|i| held[offset + i * PACKET] == SYNC);
                let found = (at..held.len().saturating_sub(span)).find(|offset| aligned(*offset));
                let Some(found) = found else {
                    // Keep only the tail that has not been fully tried.
                    let keep = span + 1;
                    if held.len() - at > keep {
                        self.lost_boundary();
                        at = held.len() - keep;
                    }
                    break;
                };
                if found > at {
                    self.lost_boundary();
                }
                at = found;
                self.synced = true;
                self.broken = false;
            }
            while at + PACKET <= held.len() && held[at] == SYNC {
                self.packet(&held[at..at + PACKET]);
                at += PACKET;
            }
            if at + PACKET > held.len() {
                break;
            }
            // The boundary is lost, and the counters go with it.
            self.synced = false;
            self.lost_boundary();
            self.last = [UNSEEN; 8192];
            at += 1;
        }
        held.drain(..at);
        self.held = held;
    }

    /// Counts one break per search for the boundary.
    fn lost_boundary(&mut self) {
        if !self.broken {
            self.breaks += 1;
            self.broken = true;
        }
    }

    /// One whole packet, which begins with a sync byte.
    fn packet(&mut self, packet: &[u8]) {
        let identifier = (u16::from(packet[1] & 0x1F) << 8) | u16::from(packet[2]);
        if identifier == NULL {
            return;
        }
        self.packets += 1;
        // The transmitter saying the packet is known to be damaged. Its counter
        // is not to be trusted either, so it is taken to have filled the next
        // slot: the packet after it is not counted as missing the same one
        // again.
        if packet[1] & 0x80 != 0 {
            self.errors += 1;
            let last = &mut self.last[identifier as usize];
            if *last != UNSEEN {
                *last = (*last + 1) & 0x0F;
            }
            return;
        }
        let control = packet[3];
        let counter = control & 0x0F;
        let has_adaptation = control & 0x20 != 0;
        // A transmitter that means to break the count says so in the
        // adaptation field, and that is not a loss.
        let deliberate = has_adaptation && packet[4] > 0 && packet[5] & 0x80 != 0;
        let last = self.last[identifier as usize];
        self.last[identifier as usize] = counter;
        if last == UNSEEN || deliberate {
            return;
        }
        // The same packet sent twice repeats the counter, and a packet with
        // no payload does not step it. Neither is a loss.
        if counter == last {
            return;
        }
        let expected = (last + 1) & 0x0F;
        if counter == expected {
            return;
        }
        // How far it jumped, which is how many packets of this stream were
        // missed -- as far as four bits can say.
        self.errors += u64::from((counter.wrapping_sub(expected)) & 0x0F);
    }
}

#[cfg(test)]
mod tests {
    use super::Continuity;
    use crate::{PACKET, SYNC};

    /// A packet of one stream with a counter on it.
    fn packet(identifier: u16, counter: u8) -> Vec<u8> {
        let mut packet = vec![0u8; PACKET];
        packet[0] = SYNC;
        packet[1] = (identifier >> 8) as u8;
        packet[2] = (identifier & 0xFF) as u8;
        // Payload only, with the counter in the low four bits.
        packet[3] = 0x10 | (counter & 0x0F);
        packet
    }

    fn stream(identifier: u16, counters: &[u8]) -> Vec<u8> {
        counters
            .iter()
            .flat_map(|counter| packet(identifier, *counter))
            .collect()
    }

    #[test]
    fn a_stream_that_arrived_whole_has_nothing_missing() {
        let mut counting = Continuity::new();
        counting.feed(&stream(0x100, &[0, 1, 2, 3, 4, 5, 6, 7]));
        assert_eq!(counting.packets(), 8);
        assert_eq!(counting.errors(), 0);
    }

    #[test]
    fn a_skipped_counter_is_as_many_packets_as_it_skipped() {
        let mut counting = Continuity::new();
        counting.feed(&stream(0x100, &[0, 1, 5, 6]));
        assert_eq!(counting.packets(), 4);
        assert_eq!(counting.errors(), 3);
    }

    #[test]
    fn the_counter_wraps_round_without_that_being_a_loss() {
        let mut counting = Continuity::new();
        counting.feed(&stream(0x100, &[14, 15, 0, 1]));
        assert_eq!(counting.errors(), 0);
    }

    /// Two streams are counted apart: their counters have nothing to do with
    /// one another, and reading them as one makes every packet a loss.
    #[test]
    fn each_stream_is_counted_on_its_own() {
        let mut counting = Continuity::new();
        for step in 0..4u8 {
            counting.feed(&packet(0x100, step));
            counting.feed(&packet(0x101, step));
        }
        assert_eq!(counting.packets(), 8);
        assert_eq!(counting.errors(), 0);
    }

    /// The stuffing a multiplex is padded with is not a stream and is not
    /// counted: its counter need not step at all.
    #[test]
    fn stuffing_is_not_counted() {
        let mut counting = Continuity::new();
        counting.feed(&stream(0x1FFF, &[0, 0, 0, 0]));
        assert_eq!(counting.packets(), 0);
        assert_eq!(counting.errors(), 0);
    }

    /// Reads are as long as a buffer, not as long as a packet.
    #[test]
    fn a_packet_split_across_two_reads_is_still_one_packet() {
        let whole = stream(0x100, &[0, 1, 2, 3]);
        let mut counting = Continuity::new();
        counting.feed(&whole[..100]);
        counting.feed(&whole[100..300]);
        counting.feed(&whole[300..]);
        assert_eq!(counting.packets(), 4);
        assert_eq!(counting.errors(), 0);
    }

    /// A stream picked up part way begins wherever it begins.
    #[test]
    fn a_reader_that_joined_mid_packet_finds_the_next_one() {
        let whole = stream(0x100, &[0, 1, 2, 3, 4, 5]);
        let mut counting = Continuity::new();
        counting.feed(&whole[40..]);
        assert_eq!(counting.packets(), 5);
        // The lost boundary is a break, and only one.
        assert_eq!(counting.errors(), 1);
    }

    /// A packet the transmitter marked as damaged is a packet that did not
    /// arrive.
    #[test]
    fn a_packet_marked_in_error_counts_against_the_carrier() {
        let mut broken = packet(0x100, 1);
        broken[1] |= 0x80;
        let mut counting = Continuity::new();
        counting.feed(&packet(0x100, 0));
        counting.feed(&broken);
        counting.feed(&packet(0x100, 2));
        counting.feed(&packet(0x100, 3));
        assert_eq!(counting.packets(), 4);
        // Counted once: the packet after it is not missing it again.
        assert_eq!(counting.errors(), 1);
    }

    /// A sync byte inside a payload is an ordinary byte: the boundary is only
    /// believed where several packets line up.
    #[test]
    fn a_sync_byte_in_a_payload_is_not_taken_for_a_boundary() {
        let mut first = packet(0x100, 0);
        first[100] = SYNC;
        let mut bytes = first[100..].to_vec();
        bytes.extend(stream(0x100, &[1, 2, 3, 4]));
        let mut counting = Continuity::new();
        counting.feed(&bytes);
        assert_eq!(counting.packets(), 4);
        assert_eq!(counting.errors(), 1, "one break, no skipped counters");
    }

    /// What follows a lost boundary does not follow what came before it, so
    /// the counters start again rather than reading the gap as a skip.
    #[test]
    fn losing_the_boundary_forgets_the_counters() {
        let mut bytes = stream(0x100, &[0, 1, 2, 3]);
        bytes.extend_from_slice(&[0u8; 50]);
        bytes.extend(stream(0x100, &[9, 10, 11, 12]));
        let mut counting = Continuity::new();
        counting.feed(&bytes);
        assert_eq!(counting.packets(), 8);
        assert_eq!(counting.errors(), 1, "the break alone");
    }

    /// Too little to judge by is answered with nothing rather than with a
    /// figure that means nothing.
    #[test]
    fn a_handful_of_packets_says_nothing_about_the_signal() {
        let mut counting = Continuity::new();
        counting.feed(&stream(0x100, &[0, 1, 2]));
        assert_eq!(counting.losses(), None);
    }
}

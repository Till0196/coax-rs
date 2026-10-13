// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Putting a stream's tables back together.
//!
//! A table is sent as sections, a section is longer than a packet, and a packet
//! may hold the end of one section and the start of the next. So the bytes of
//! one identifier are gathered, cut at the marks the stream puts in them, and
//! handed on a whole section at a time -- and only when its check comes out,
//! for the sections that carry one.

use crate::{PACKET, SYNC, crc32};

/// Whether a section's check comes out. A table read from damaged bytes says
/// something that was never sent. The check is over the section including the
/// four bytes of the check itself, which is why it comes out at nought.
pub fn check(section: &[u8]) -> bool {
    section.len() >= 4 && crc32(section) == 0
}

/// Whether a section is to be taken: one with `section_syntax_indicator` set
/// carries a check that has to come out; one without (the TDT, ARIB STD-B10
/// 5.2.8) has none, and its length is all there is to go on (ISO/IEC 13818-1
/// 2.4.4.10).
pub fn sound(section: &[u8]) -> bool {
    match section.get(1) {
        Some(flags) if flags & 0x80 == 0 => true,
        Some(_) => check(section),
        None => false,
    }
}

/// The sections of one packet identifier, as they arrive.
#[derive(Debug)]
pub struct Sections {
    pid: u16,
    held: Vec<u8>,
    section: Vec<u8>,
    /// How long the section being gathered will be, once its header has been
    /// seen.
    wanted: usize,
}

impl Sections {
    pub fn new(pid: u16) -> Self {
        Self {
            pid,
            held: Vec::new(),
            section: Vec::new(),
            wanted: 0,
        }
    }

    /// Takes what has arrived and gives back the sections it completed.
    pub fn feed(&mut self, arrived: &[u8]) -> Vec<Vec<u8>> {
        self.held.extend_from_slice(arrived);
        let mut whole = Vec::new();
        let mut at = 0;
        while at + PACKET <= self.held.len() {
            if self.held[at] != SYNC {
                at += 1;
                continue;
            }
            let packet: Vec<u8> = self.held[at..at + PACKET].to_vec();
            self.packet(&packet, &mut whole);
            at += PACKET;
        }
        self.held.drain(..at);
        whole
    }

    fn packet(&mut self, packet: &[u8], whole: &mut Vec<Vec<u8>>) {
        let pid = (u16::from(packet[1] & 0x1f) << 8) | u16::from(packet[2]);
        if pid != self.pid {
            return;
        }
        let starts = packet[1] & 0x40 != 0;
        // Past the adaptation field, where there is one.
        let mut at = 4;
        if packet[3] & 0x20 != 0 {
            at += 1 + usize::from(packet[4]);
        }
        if packet[3] & 0x10 == 0 || at >= PACKET {
            return;
        }
        if starts {
            // The first byte says how much of what follows belongs to the
            // section before this one.
            let skip = usize::from(packet[at]);
            at += 1;
            if !self.section.is_empty() && at + skip <= PACKET {
                let tail: Vec<u8> = packet[at..at + skip].to_vec();
                self.take(&tail, whole);
            }
            at += skip;
            self.section.clear();
            self.wanted = 0;
        }
        if at < PACKET {
            let piece: Vec<u8> = packet[at..].to_vec();
            self.take(&piece, whole);
        }
    }

    fn take(&mut self, bytes: &[u8], whole: &mut Vec<Vec<u8>>) {
        for byte in bytes {
            if self.section.is_empty() && *byte == 0xff && self.wanted == 0 {
                // Padding between sections.
                continue;
            }
            self.section.push(*byte);
            if self.section.len() == 3 {
                self.wanted = 3
                    + (((usize::from(self.section[1]) & 0x0f) << 8) | usize::from(self.section[2]));
            }
            if self.wanted > 0 && self.section.len() == self.wanted {
                let section = std::mem::take(&mut self.section);
                self.wanted = 0;
                if sound(&section) {
                    whole.push(section);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Sections;
    use crate::{PACKET, SYNC, crc32};

    /// One packet of `pid` that starts a unit and carries `sections` back to
    /// back, the rest stuffing.
    fn packet(pid: u16, sections: &[Vec<u8>]) -> Vec<u8> {
        let mut packet = vec![SYNC, 0x40 | (pid >> 8) as u8, pid as u8, 0x10, 0x00];
        for section in sections {
            packet.extend_from_slice(section);
        }
        packet.resize(PACKET, 0xff);
        packet
    }

    fn long_section(table_id: u8, body: &[u8], good: bool) -> Vec<u8> {
        let length = 5 + body.len() + 4;
        let mut section = vec![
            table_id,
            0xb0 | (length >> 8) as u8,
            length as u8,
            0x00,
            0x01,
            0xc1,
            0x00,
            0x00,
        ];
        section.extend_from_slice(body);
        let check = crc32(&section) ^ u32::from(!good);
        section.extend_from_slice(&check.to_be_bytes());
        section
    }

    /// A TDT has no check (section_syntax_indicator 0), and is taken on its
    /// length alone.
    #[test]
    fn a_section_without_a_check_is_taken() {
        let tdt = vec![0x70, 0x70, 0x05, 0xc0, 0x79, 0x12, 0x45, 0x00];
        let mut sections = Sections::new(0x14);
        assert_eq!(
            sections.feed(&packet(0x14, std::slice::from_ref(&tdt))),
            [tdt]
        );
    }

    /// A section that carries a check is taken only when it comes out.
    #[test]
    fn a_section_whose_check_fails_is_left_out() {
        let good = long_section(0x73, &[1, 2, 3], true);
        let bad = long_section(0x73, &[4, 5, 6], false);
        let mut sections = Sections::new(0x14);
        assert_eq!(sections.feed(&packet(0x14, &[bad, good.clone()])), [good]);
    }
}

// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The table that says which programmes a transport stream carries.
//!
//! It is the first table of any transport stream and the shortest: the stream's
//! own identifier, and then a programme number and the packet identifier its
//! description is on. That is enough to say a stream is a stream and not noise
//! that happened to check out, and enough to count what is on it without
//! waiting for the service table.

use crate::sections::Sections;

/// The table rides on the first packet identifier there is.
const PAT_PID: u16 = 0x0000;
const TABLE_PAT: u8 = 0x00;

/// One programme of a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Program {
    /// What selects it, which for a broadcast is the service identifier.
    pub number: u16,
    /// Where its description is: the packet identifier of its map, or of the
    /// network table for the entry that names no programme.
    pub pid: u16,
}

/// Reads the table out of a stream as it arrives.
#[derive(Debug)]
pub struct Reader {
    sections: Sections,
    stream_id: Option<u16>,
    programs: Vec<Program>,
    network_pid: Option<u16>,
}

impl Default for Reader {
    fn default() -> Self {
        Self::new()
    }
}

impl Reader {
    pub fn new() -> Self {
        Self {
            sections: Sections::new(PAT_PID),
            stream_id: None,
            programs: Vec::new(),
            network_pid: None,
        }
    }

    pub fn feed(&mut self, arrived: &[u8]) {
        for section in self.sections.feed(arrived) {
            if section[0] != TABLE_PAT || section.len() < 12 {
                continue;
            }
            self.stream_id
                .get_or_insert((u16::from(section[3]) << 8) | u16::from(section[4]));
            let end = section.len() - 4;
            let mut at = 8;
            while at + 4 <= end {
                let number = (u16::from(section[at]) << 8) | u16::from(section[at + 1]);
                let pid = ((u16::from(section[at + 2]) & 0x1f) << 8) | u16::from(section[at + 3]);
                at += 4;
                // Programme nought is not a programme: it says where the
                // network's own table is.
                if number == 0 {
                    self.network_pid.get_or_insert(pid);
                    continue;
                }
                if !self.programs.iter().any(|known| known.number == number) {
                    self.programs.push(Program { number, pid });
                }
            }
        }
    }

    /// What the stream says it is, or nothing where the table has not arrived.
    pub fn stream_id(&self) -> Option<u16> {
        self.stream_id
    }

    /// The programmes named so far, in the order they were named.
    pub fn programs(&self) -> &[Program] {
        &self.programs
    }

    /// Where the network's own table is, where the stream said.
    pub fn network_pid(&self) -> Option<u16> {
        self.network_pid
    }
}

#[cfg(test)]
mod tests {
    use super::Reader;
    use crate::{PACKET, SYNC, crc32};

    fn pat(stream_id: u16, entries: &[(u16, u16)]) -> Vec<u8> {
        let mut section = vec![0x00, 0x00, 0x00];
        section.extend_from_slice(&stream_id.to_be_bytes());
        section.extend_from_slice(&[0xc1, 0x00, 0x00]);
        for (number, pid) in entries {
            section.extend_from_slice(&number.to_be_bytes());
            section.extend_from_slice(&(pid | 0xe000).to_be_bytes());
        }
        let length = section.len() + 4 - 3;
        section[1] = 0xb0 | ((length >> 8) as u8 & 0x0f);
        section[2] = (length & 0xff) as u8;
        let check = crc32(&section);
        section.extend_from_slice(&check.to_be_bytes());
        section
    }

    fn packets(section: &[u8]) -> Vec<u8> {
        let mut packet = vec![0xffu8; PACKET];
        packet[0] = SYNC;
        packet[1] = 0x40;
        packet[2] = 0x00;
        packet[3] = 0x10;
        packet[4] = 0;
        packet[5..5 + section.len()].copy_from_slice(section);
        packet
    }

    /// The table says which stream this is and what is on it.
    #[test]
    fn a_stream_says_which_programmes_it_carries() {
        let mut reader = Reader::new();
        reader.feed(&packets(&pat(16625, &[(0, 0x0010), (1024, 0x1fc8)])));
        assert_eq!(reader.stream_id(), Some(16625));
        // The entry that names no programme is where the network table is
        // rather than a programme of its own.
        assert_eq!(reader.network_pid(), Some(0x0010));
        assert_eq!(reader.programs().len(), 1);
        assert_eq!(reader.programs()[0].number, 1024);
        assert_eq!(reader.programs()[0].pid, 0x1fc8);
    }

    /// The table is sent again for as long as the stream runs, so a programme
    /// already named is not counted twice.
    #[test]
    fn a_table_sent_again_does_not_name_the_same_programme_twice() {
        let mut reader = Reader::new();
        let sent = packets(&pat(16625, &[(1024, 0x1fc8)]));
        reader.feed(&sent);
        reader.feed(&sent);
        assert_eq!(reader.programs().len(), 1);
    }
}

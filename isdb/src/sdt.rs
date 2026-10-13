// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! What a stream says it is, and what it says it carries.
//!
//! The service table names the network a stream belongs to and the stream's own
//! identifier, in the first bytes of every section it sends. Those two numbers
//! are what says which platform a carrier belongs to, and a carrier with no
//! multiplexing frame on it has nothing else to say so.
//!
//! After them comes the stream's own account of its services: a number for
//! each, what kind of thing it is, whether it is scrambled, and the names. The
//! names are in the broadcast character encoding -- see [`crate::arib`] -- and
//! are what tells a channel worth keeping from the data and promotional
//! services sharing the multiplex with it.

use crate::arib;
use mpegts::sections::Sections;

/// The service table rides on a packet identifier of its own.
const SDT_PID: u16 = 0x11;

/// The table that describes this stream, and the one that describes a
/// neighbour. What the second says is the neighbour's, but the network table's
/// listing wants those names, and [`namings`] is where it gets them.
const TABLE_OWN: u8 = 0x42;
const TABLE_OTHER: u8 = 0x46;

/// The descriptor that names a service and says what kind it is.
const SERVICE_DESCRIPTOR: u8 = 0x48;

/// What a stream calls itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    pub stream_id: u16,
    pub original_network_id: u16,
}

/// One service of a stream, as the stream describes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Service {
    pub id: u16,
    /// What kind of service it is, as ARIB STD-B10 numbers them: television,
    /// sound, data, and the temporary and promotional kinds that share a
    /// multiplex with them. Zero where the stream did not describe it.
    pub kind: u8,
    /// Set where one or more of the service's streams are under conditional
    /// access, which is `free_CA_mode` read the way the standard writes it:
    /// set means a card is needed.
    pub scrambled: bool,
    pub name: String,
    pub provider: String,
    /// Set where something was dropped in reading the names. The bytes are
    /// kept beside them so that a caller can show what arrived.
    pub lossy: bool,
    /// The name as it was sent, for a caller that would rather record the
    /// bytes than this program's reading of them.
    pub name_raw: Vec<u8>,
}

/// What a kind of service is called, or nothing for a kind that has no name.
/// These are the words the host writes in its own channel scan, not the
/// standard's: a viewer comparing the two should not have to work out that two
/// names are the same thing.
pub fn kind_name(kind: u8) -> Option<&'static str> {
    Some(match kind {
        0x01 => "TV",
        0x02 => "音声",
        // Not a service a viewer watches: the broadcaster's own stream, for
        // the receivers rather than for the people. It is named because a
        // multiplex holding one is not a multiplex holding nothing.
        0xa4 => "エンジニアリング",
        0xa5 => "プロモーション映像",
        // One-segment is carried as a data service and is told from a genuine
        // one only by the layer it rides in, which is why the host names them
        // together.
        0xc0 => "データ/ワンセグ",
        0xad => "4K TV",
        _ => return None,
    })
}

/// Reads a stream's own identity as it arrives.
#[derive(Debug)]
pub struct Reader {
    sections: Sections,
    found: Option<Identity>,
    /// The services, in the order the stream first described them. A section
    /// is sent again for as long as the stream runs, so a service already
    /// described is left as it was rather than added twice.
    services: Vec<Service>,
}

impl Default for Reader {
    fn default() -> Self {
        Self::new()
    }
}

impl Reader {
    pub fn new() -> Self {
        Self {
            sections: Sections::new(SDT_PID),
            found: None,
            services: Vec::new(),
        }
    }

    pub fn feed(&mut self, arrived: &[u8]) {
        for section in self.sections.feed(arrived) {
            if section[0] != TABLE_OWN || section.len() < 12 {
                continue;
            }
            if self.found.is_none() {
                self.found = Some(Identity {
                    stream_id: (u16::from(section[3]) << 8) | u16::from(section[4]),
                    original_network_id: (u16::from(section[8]) << 8) | u16::from(section[9]),
                });
            }
            self.describe(&section);
        }
    }

    /// Keeps what one section describes, less what is already known. A section
    /// is sent again for as long as the stream runs, so a service already
    /// described is left as it was rather than added twice.
    fn describe(&mut self, section: &[u8]) {
        for service in described(section) {
            if !self.services.iter().any(|known| known.id == service.id) {
                self.services.push(service);
            }
        }
    }

    /// What the stream said it is, or nothing where it did not say.
    pub fn identity(&self) -> Option<Identity> {
        self.found
    }

    /// The services described so far.
    pub fn services(&self) -> &[Service] {
        &self.services
    }
}

/// What a service table called one service of some stream, which need not be
/// this one: a satellite sends a table for every multiplex of its network. A
/// service identifier is unique only within its multiplex, so all three numbers
/// are the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Naming {
    pub original_network_id: u16,
    pub stream_id: u16,
    pub service_id: u16,
    pub name: String,
    pub lossy: bool,
}

/// Reads the services one service table section describes, of either kind.
///
/// **The loop runs to the check at the end of the section rather than to the
/// length in its header**: the check is not a service.
pub fn described(section: &[u8]) -> Vec<Service> {
    if section.len() < 16 || !matches!(section[0], TABLE_OWN | TABLE_OTHER) {
        return Vec::new();
    }
    let end = section.len() - 4;
    let mut read = Vec::new();
    let mut at = 11;
    while at + 5 <= end {
        let length = (usize::from(section[at + 3] & 0x0f) << 8) | usize::from(section[at + 4]);
        let next = at + 5 + length;
        if next > end {
            break;
        }
        let mut service = Service {
            id: (u16::from(section[at]) << 8) | u16::from(section[at + 1]),
            scrambled: section[at + 3] & 0x10 != 0,
            ..Service::default()
        };
        describe_service(&section[at + 5..next], &mut service);
        read.push(service);
        at = next;
    }
    read
}

/// Reads the namings out of one service table section, of either kind. What
/// the section says about running status and access is the caller's business;
/// this is only the names.
pub fn namings(section: &[u8]) -> Vec<Naming> {
    if section.len() < 16 {
        return Vec::new();
    }
    let stream_id = (u16::from(section[3]) << 8) | u16::from(section[4]);
    let original_network_id = (u16::from(section[8]) << 8) | u16::from(section[9]);
    described(section)
        .into_iter()
        .filter(|service| !service.name.is_empty())
        .map(|service| Naming {
            original_network_id,
            stream_id,
            service_id: service.id,
            name: service.name,
            lossy: service.lossy,
        })
        .collect()
}

/// Reads one service's descriptors, for the one that names it. Every other
/// descriptor is stepped over by the length it declares: a reader that looked
/// for the one it wanted without following the lengths would find its tag
/// inside another's body.
fn describe_service(descriptors: &[u8], service: &mut Service) {
    let mut at = 0;
    while at + 2 <= descriptors.len() {
        let length = usize::from(descriptors[at + 1]);
        let next = at + 2 + length;
        if next > descriptors.len() {
            return;
        }
        if descriptors[at] == SERVICE_DESCRIPTOR && length >= 3 {
            let body = &descriptors[at + 2..next];
            service.kind = body[0];
            let provider = usize::from(body[1]);
            // The two names are written one after the other, each behind its
            // own length, so the second cannot be found without the first.
            if 2 + provider >= body.len() {
                return;
            }
            let name = usize::from(body[2 + provider]);
            let from = 3 + provider;
            if from + name > body.len() {
                return;
            }
            let provider = arib::read(&body[2..2 + provider]);
            let name_raw = &body[from..from + name];
            let name = arib::read(name_raw);
            service.provider = provider.text.trim().to_owned();
            service.name = name.text.trim().to_owned();
            service.lossy = provider.lossy || name.lossy;
            service.name_raw = name_raw.to_vec();
        }
        at = next;
    }
}

#[cfg(test)]
mod tests {
    use super::{Identity, Reader};
    use mpegts::{PACKET, SYNC, crc32};

    /// A service table section that says which stream it belongs to, and
    /// describes one service with the descriptors it is given.
    fn sdt_with(
        table: u8,
        stream_id: u16,
        original_network_id: u16,
        scrambled: bool,
        descriptors: &[u8],
    ) -> Vec<u8> {
        let mut section = vec![table, 0x00, 0x00];
        section.extend_from_slice(&stream_id.to_be_bytes());
        section.extend_from_slice(&[0xc1, 0x00, 0x00]);
        section.extend_from_slice(&original_network_id.to_be_bytes());
        section.push(0xff);
        section.extend_from_slice(&[0x04, 0x00, 0x00]);
        let flags = if scrambled { 0x90 } else { 0x80 };
        section.push(flags | ((descriptors.len() >> 8) as u8 & 0x0f));
        section.push((descriptors.len() & 0xff) as u8);
        section.extend_from_slice(descriptors);
        let length = section.len() + 4 - 3;
        section[1] = 0xb0 | ((length >> 8) as u8 & 0x0f);
        section[2] = (length & 0xff) as u8;
        let check = crc32(&section);
        section.extend_from_slice(&check.to_be_bytes());
        section
    }

    /// A service table section that says which stream it belongs to.
    fn sdt(table: u8, stream_id: u16, original_network_id: u16) -> Vec<u8> {
        let mut section = vec![table, 0x00, 0x00];
        section.extend_from_slice(&stream_id.to_be_bytes());
        section.extend_from_slice(&[0xc1, 0x00, 0x00]);
        section.extend_from_slice(&original_network_id.to_be_bytes());
        section.push(0xff);
        // One service, described by nothing.
        section.extend_from_slice(&[0x04, 0x00, 0x00, 0x80, 0x00]);
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
        packet[2] = 0x11;
        packet[3] = 0x10;
        packet[4] = 0;
        packet[5..5 + section.len()].copy_from_slice(section);
        packet
    }

    #[test]
    fn a_stream_says_which_network_it_belongs_to() {
        let mut reader = Reader::new();
        reader.feed(&packets(&sdt(0x42, 16625, 4)));
        assert_eq!(
            reader.identity(),
            Some(Identity {
                stream_id: 16625,
                original_network_id: 4,
            })
        );
    }

    /// The table that describes a neighbour says what the neighbour is, not
    /// what this stream is.
    #[test]
    fn a_table_about_another_stream_is_left_alone() {
        let mut reader = Reader::new();
        reader.feed(&packets(&sdt(0x46, 16625, 4)));
        assert_eq!(reader.identity(), None);
    }

    /// The descriptor that names a service: what kind it is, who runs it and
    /// what it calls itself -- the last two in the broadcast's own character
    /// encoding.
    #[test]
    fn a_service_is_read_with_the_names_it_was_sent_under() {
        // The alphabet moved onto the low half of the byte range and then the
        // letters; and 衛星 out of the two-byte set a string starts in.
        let provider: &[u8] = &[0x0e, b'S', b'A', b'M', b'P', b'L', b'E'];
        let name: &[u8] = &[0x31, 0x52, 0x40, 0x31];
        let mut descriptor = vec![
            0x48,
            (3 + provider.len() + name.len()) as u8,
            0x01,
            provider.len() as u8,
        ];
        descriptor.extend_from_slice(provider);
        descriptor.push(name.len() as u8);
        descriptor.extend_from_slice(name);
        let mut reader = Reader::new();
        reader.feed(&packets(&sdt_with(0x42, 16625, 4, true, &descriptor)));
        let services = reader.services();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].id, 0x0400);
        assert_eq!(services[0].kind, 0x01);
        assert_eq!(services[0].name, "衛星");
        assert_eq!(services[0].provider, "ＳＡＭＰＬＥ");
        // free_CA_mode set means a card is needed, which is the way round the
        // standard writes it.
        assert!(services[0].scrambled);
        assert!(!services[0].lossy);
    }

    /// A section is sent again for as long as the stream runs, so a service
    /// already described is not added a second time.
    #[test]
    fn a_section_sent_again_does_not_describe_the_same_service_twice() {
        let mut reader = Reader::new();
        let section = packets(&sdt(0x42, 16625, 4));
        reader.feed(&section);
        reader.feed(&section);
        assert_eq!(reader.services().len(), 1);
    }

    /// A descriptor this does not want is stepped over by the length it
    /// declares. A reader hunting for the tag it wanted would find it inside
    /// another descriptor's body soon enough.
    #[test]
    fn a_descriptor_that_is_not_wanted_is_stepped_over() {
        // A descriptor whose body holds the tag of the one that names a
        // service, followed by the real thing.
        let mut descriptor = vec![0xc1, 0x04, 0x48, 0x02, 0x02, 0x00];
        descriptor.extend_from_slice(&[0x48, 0x05, 0x01, 0x01, b'A', 0x01, b'B']);
        let mut reader = Reader::new();
        reader.feed(&packets(&sdt_with(0x42, 16625, 4, false, &descriptor)));
        let services = reader.services();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].kind, 0x01);
        assert!(!services[0].scrambled);
    }
}

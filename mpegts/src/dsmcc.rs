// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The DSM-CC data carousel: modules of data sent round and round in
//! sections, so that a receiver joining at any moment gets all of them
//! within one turn. ISO/IEC 13818-6 clauses 7 and 9.
//!
//! Three tables ride on one identifier. The download info indication (`DII`,
//! table 0x3B) lists the modules of the carousel -- their identifiers, sizes
//! and versions, and a few bytes describing each. The download data blocks
//! (`DDB`, table 0x3C) carry the modules cut into blocks of the size the DII
//! said. The stream descriptors (table 0x3D) are not part of the download at
//! all: they carry events timed against the stream, and are handed on as
//! they come.
//!
//! [`Carousel`] does the gathering: it takes sections of one identifier and
//! gives back whole modules, once, when their last block arrives. What the
//! bytes of a module mean -- and what its descriptors say -- is for the
//! broadcasting standard on top to read; nothing here looks inside.

/// The table identifiers of the three sections.
pub const TABLE_DII: u8 = 0x3B;
pub const TABLE_DDB: u8 = 0x3C;
pub const TABLE_STREAM_DESCRIPTORS: u8 = 0x3D;

/// What a `dsmccMessageHeader` says the message is.
const PROTOCOL_DISCRIMINATOR: u8 = 0x11;
const TYPE_DOWNLOAD: u8 = 0x03;
const MESSAGE_DII: u16 = 0x1002;
const MESSAGE_DDB: u16 = 0x1003;

/// The download info indication: the list of modules in the carousel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dii {
    /// The `transactionId` of the message header. It changes whenever the
    /// list does, and a DII carrying the one already read says nothing new.
    pub transaction_id: u32,
    /// Ties the blocks to this list: a DDB with a different `downloadId` is
    /// of a different download.
    pub download_id: u32,
    /// How long every block of every module is, except the last of each.
    pub block_size: u16,
    pub modules: Vec<ModuleInfo>,
    /// The bytes after the module list, which a standard on top fills with
    /// descriptors of its own.
    pub private_data: Vec<u8>,
}

/// One module as the DII describes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleInfo {
    pub id: u16,
    pub size: u32,
    pub version: u8,
    /// The `moduleInfoByte`s: descriptors, in the shape the standard on top
    /// gives them.
    pub info: Vec<u8>,
}

/// One block of a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ddb<'a> {
    pub download_id: u32,
    pub module_id: u16,
    pub module_version: u8,
    pub block_number: u16,
    pub data: &'a [u8],
}

/// A stream descriptors section: the descriptor loop as sent, with the
/// header fields a standard on top gives meaning to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamDescriptors {
    pub extension: u16,
    pub version: u8,
    pub number: u8,
    pub last_number: u8,
    pub descriptors: Vec<u8>,
}

fn be16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// The long section header every DSM-CC section starts with, and the bytes
/// between it and the check.
struct Header<'a> {
    table_id: u8,
    extension: u16,
    version: u8,
    number: u8,
    last_number: u8,
    body: &'a [u8],
}

fn header(section: &[u8]) -> Option<Header<'_>> {
    if section.len() < 12 || section[1] & 0x80 == 0 {
        return None;
    }
    let length = usize::from(be16(&section[1..])) & 0x0fff;
    let end = 3 + length;
    if end > section.len() || length < 9 {
        return None;
    }
    Some(Header {
        table_id: section[0],
        extension: be16(&section[3..]),
        version: section[5] >> 1 & 0x1f,
        number: section[6],
        last_number: section[7],
        body: &section[8..end - 4],
    })
}

/// The message header the DII and DDB share, and the message after it. The
/// fourth word is the `transactionId` of a DII and the `downloadId` of a
/// DDB.
fn message(body: &[u8], wanted: u16) -> Option<(u32, &[u8])> {
    if body.len() < 12 || body[0] != PROTOCOL_DISCRIMINATOR || body[1] != TYPE_DOWNLOAD {
        return None;
    }
    if be16(&body[2..]) != wanted {
        return None;
    }
    let id = be32(&body[4..]);
    let adaptation = usize::from(body[9]);
    let length = usize::from(be16(&body[10..]));
    if length < adaptation || 12 + length > body.len() {
        return None;
    }
    Some((id, &body[12 + adaptation..12 + length]))
}

/// Reads a DII section.
pub fn dii(section: &[u8]) -> Option<Dii> {
    let header = header(section)?;
    if header.table_id != TABLE_DII {
        return None;
    }
    let (transaction_id, message) = message(header.body, MESSAGE_DII)?;
    if message.len() < 20 {
        return None;
    }
    let download_id = be32(message);
    let block_size = be16(&message[4..]);
    // windowSize, ackPeriod, tCDownloadWindow, tCDownloadScenario: eight
    // bytes nothing here uses.
    let compatibility = usize::from(be16(&message[16..]));
    let mut at = 18 + compatibility;
    if at + 2 > message.len() {
        return None;
    }
    let count = usize::from(be16(&message[at..]));
    at += 2;
    let mut modules = Vec::with_capacity(count.min(64));
    for _ in 0..count {
        if at + 8 > message.len() {
            return None;
        }
        let info_length = usize::from(message[at + 7]);
        let end = at + 8 + info_length;
        if end > message.len() {
            return None;
        }
        modules.push(ModuleInfo {
            id: be16(&message[at..]),
            size: be32(&message[at + 2..]),
            version: message[at + 6],
            info: message[at + 8..end].to_vec(),
        });
        at = end;
    }
    if at + 2 > message.len() {
        return None;
    }
    let private_length = usize::from(be16(&message[at..]));
    at += 2;
    let private_data = message.get(at..at + private_length)?.to_vec();
    Some(Dii {
        transaction_id,
        download_id,
        block_size,
        modules,
        private_data,
    })
}

/// Reads a DDB section.
pub fn ddb(section: &[u8]) -> Option<Ddb<'_>> {
    let header = header(section)?;
    if header.table_id != TABLE_DDB {
        return None;
    }
    let (download_id, message) = message(header.body, MESSAGE_DDB)?;
    if message.len() < 6 {
        return None;
    }
    Some(Ddb {
        download_id,
        module_id: be16(message),
        module_version: message[2],
        block_number: be16(&message[4..]),
        data: &message[6..],
    })
}

/// Reads a stream descriptors section.
pub fn stream_descriptors(section: &[u8]) -> Option<StreamDescriptors> {
    let header = header(section)?;
    if header.table_id != TABLE_STREAM_DESCRIPTORS {
        return None;
    }
    Some(StreamDescriptors {
        extension: header.extension,
        version: header.version,
        number: header.number,
        last_number: header.last_number,
        descriptors: header.body.to_vec(),
    })
}

/// A module, whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub download_id: u32,
    pub id: u16,
    pub version: u8,
    /// The `moduleInfoByte`s the DII gave for it.
    pub info: Vec<u8>,
    pub data: Vec<u8>,
}

/// What one section of the carousel brought.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Brought {
    /// A new list of modules. The carousel's contents from here on are the
    /// modules of this list; whatever was gathered of a module not in it is
    /// let go.
    Listed(Dii),
    /// A module's last block arrived, and here is all of it.
    Module(Module),
    /// A stream descriptors section, as it came.
    Stream(StreamDescriptors),
}

/// The largest module gathered. A data broadcast's modules are tens of
/// kilobytes; this leaves room for any that is real and none for a size field
/// that has been damaged into gigabytes.
const MOST_MODULE: usize = 16 * 1024 * 1024;

/// The most blocks a module can come in: a block's number is sixteen bits.
const MOST_BLOCKS: usize = 1 << 16;

/// A module part way through arriving.
#[derive(Debug)]
struct Gathering {
    info: ModuleInfo,
    /// How many blocks the module comes in.
    count: usize,
    /// The blocks that have arrived, made room for when the first does: a
    /// list may name many modules, and room for all of them at once is room
    /// a receiver tuned for a moment never fills.
    blocks: Vec<Option<Vec<u8>>>,
    have: usize,
    /// Whether this version was already given back whole, so that the next
    /// turn of the carousel does not give it back again.
    done: bool,
}

/// Gathers the modules of one carousel out of its sections.
///
/// **One identifier, one carousel.** A DII on the identifier lists what is
/// there; the blocks are matched to the list by `downloadId`, module
/// identifier and version, and a module comes back when its last block does.
/// A list read before (same `transactionId`) is not news, and a module
/// given back is not given back again while its version stands.
#[derive(Debug, Default)]
pub struct Carousel {
    transaction_id: Option<u32>,
    download_id: u32,
    block_size: usize,
    modules: Vec<Gathering>,
}

impl Carousel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes one section, and says what it brought.
    pub fn push(&mut self, section: &[u8]) -> Option<Brought> {
        match *section.first()? {
            TABLE_DII => self.listed(dii(section)?),
            TABLE_DDB => self.block(ddb(section)?),
            TABLE_STREAM_DESCRIPTORS => Some(Brought::Stream(stream_descriptors(section)?)),
            _ => None,
        }
    }

    fn listed(&mut self, dii: Dii) -> Option<Brought> {
        if self.transaction_id == Some(dii.transaction_id) {
            return None;
        }
        let same_download = self.download_id == dii.download_id;
        self.transaction_id = Some(dii.transaction_id);
        self.download_id = dii.download_id;
        self.block_size = usize::from(dii.block_size).max(1);
        let old = std::mem::take(&mut self.modules);
        self.modules = dii
            .modules
            .iter()
            .map(|info| {
                // A module that was already whole at this version stays
                // done, so that a list re-sent with one module changed
                // gives back only the one.
                let kept = same_download
                    && old.iter().any(|was| {
                        was.done && was.info.id == info.id && was.info.version == info.version
                    });
                let size = usize::try_from(info.size).unwrap_or(usize::MAX);
                let count = size.div_ceil(self.block_size);
                // A module larger than anything broadcast carries, or in more
                // blocks than a block number can count, is a damaged list. It
                // is passed over rather than made room for.
                let unreasonable = size > MOST_MODULE || count > MOST_BLOCKS;
                Gathering {
                    info: info.clone(),
                    count: if kept || unreasonable { 0 } else { count },
                    blocks: Vec::new(),
                    have: 0,
                    done: kept || unreasonable,
                }
            })
            .collect();
        Some(Brought::Listed(dii))
    }

    fn block(&mut self, ddb: Ddb<'_>) -> Option<Brought> {
        if self.transaction_id.is_none() || ddb.download_id != self.download_id {
            return None;
        }
        let download_id = self.download_id;
        let gathering = self.modules.iter_mut().find(|gathering| {
            gathering.info.id == ddb.module_id && gathering.info.version == ddb.module_version
        })?;
        if gathering.done {
            return None;
        }
        let at = usize::from(ddb.block_number);
        if at >= gathering.count {
            return None;
        }
        if gathering.blocks.is_empty() {
            gathering.blocks = vec![None; gathering.count];
        }
        let slot = gathering.blocks.get_mut(at)?;
        if slot.is_some() {
            return None;
        }
        *slot = Some(ddb.data.to_vec());
        gathering.have += 1;
        if gathering.have < gathering.count {
            return None;
        }
        let mut data = Vec::with_capacity(
            usize::try_from(gathering.info.size)
                .unwrap_or(0)
                .min(MOST_MODULE),
        );
        for block in std::mem::take(&mut gathering.blocks).into_iter().flatten() {
            data.extend_from_slice(&block);
        }
        gathering.done = true;
        Some(Brought::Module(Module {
            download_id,
            id: gathering.info.id,
            version: gathering.info.version,
            info: gathering.info.info.clone(),
            data,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc32;

    fn section(table_id: u8, extension: u16, body: &[u8]) -> Vec<u8> {
        let length = 9 + body.len();
        let mut out = vec![
            table_id,
            0xB0 | (length >> 8) as u8,
            length as u8,
            (extension >> 8) as u8,
            extension as u8,
            0xC1,
            0,
            0,
        ];
        out.extend_from_slice(body);
        out.extend_from_slice(&crc32(&out).to_be_bytes());
        out
    }

    fn message_body(message_id: u16, id: u32, message: &[u8]) -> Vec<u8> {
        let mut out = vec![PROTOCOL_DISCRIMINATOR, TYPE_DOWNLOAD];
        out.extend_from_slice(&message_id.to_be_bytes());
        out.extend_from_slice(&id.to_be_bytes());
        out.push(0xFF);
        out.push(0);
        out.extend_from_slice(&(message.len() as u16).to_be_bytes());
        out.extend_from_slice(message);
        out
    }

    fn a_dii(
        transaction: u32,
        download: u32,
        block_size: u16,
        modules: &[(u16, u32, u8)],
    ) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(&download.to_be_bytes());
        message.extend_from_slice(&block_size.to_be_bytes());
        message.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        message.extend_from_slice(&[0, 0]); // compatibilityDescriptor
        message.extend_from_slice(&(modules.len() as u16).to_be_bytes());
        for (id, size, version) in modules {
            message.extend_from_slice(&id.to_be_bytes());
            message.extend_from_slice(&size.to_be_bytes());
            message.push(*version);
            message.push(3);
            message.extend_from_slice(&[0x01, 1, b'x']);
        }
        message.extend_from_slice(&[0, 1, 0xF0]);
        section(
            TABLE_DII,
            transaction as u16,
            &message_body(MESSAGE_DII, transaction, &message),
        )
    }

    fn a_ddb(download: u32, module: u16, version: u8, block: u16, data: &[u8]) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(&module.to_be_bytes());
        message.push(version);
        message.push(0xFF);
        message.extend_from_slice(&block.to_be_bytes());
        message.extend_from_slice(data);
        section(
            TABLE_DDB,
            module,
            &message_body(MESSAGE_DDB, download, &message),
        )
    }

    #[test]
    fn a_dii_lists_its_modules() {
        let read = dii(&a_dii(7, 0x1000_0001, 4, &[(1, 10, 2), (2, 3, 0)])).expect("a DII");
        assert_eq!(read.transaction_id, 7);
        assert_eq!(read.download_id, 0x1000_0001);
        assert_eq!(read.block_size, 4);
        assert_eq!(read.modules.len(), 2);
        assert_eq!(read.modules[0].info, [0x01, 1, b'x']);
        assert_eq!(read.private_data, [0xF0]);
    }

    #[test]
    fn a_module_comes_back_once_its_last_block_does() {
        let mut carousel = Carousel::new();
        assert!(matches!(
            carousel.push(&a_dii(1, 1, 4, &[(1, 10, 0)])),
            Some(Brought::Listed(_))
        ));
        assert_eq!(carousel.push(&a_ddb(1, 1, 0, 2, b"ij")), None);
        assert_eq!(carousel.push(&a_ddb(1, 1, 0, 0, b"abcd")), None);
        // A block of a version not listed is nothing.
        assert_eq!(carousel.push(&a_ddb(1, 1, 3, 1, b"efgh")), None);
        let Some(Brought::Module(module)) = carousel.push(&a_ddb(1, 1, 0, 1, b"efgh")) else {
            panic!("the last block makes the module whole");
        };
        assert_eq!(module.data, b"abcdefghij");
        assert_eq!((module.id, module.version), (1, 0));
        // Round again: the same blocks are not news, and neither is the list.
        assert_eq!(carousel.push(&a_ddb(1, 1, 0, 0, b"abcd")), None);
        assert_eq!(carousel.push(&a_dii(1, 1, 4, &[(1, 10, 0)])), None);
    }

    /// A list naming a module of four gigabytes, or one cut into more blocks
    /// than can be numbered, is damaged: nothing is made room for, and its
    /// blocks are nothing.
    #[test]
    fn a_module_too_large_to_be_real_is_passed_over() {
        let mut carousel = Carousel::new();
        assert!(matches!(
            carousel.push(&a_dii(1, 1, 4, &[(1, u32::MAX, 0), (2, 1 << 20, 0)])),
            Some(Brought::Listed(_))
        ));
        assert!(
            carousel
                .modules
                .iter()
                .all(|module| module.blocks.is_empty())
        );
        assert_eq!(carousel.push(&a_ddb(1, 1, 0, 0, b"abcd")), None);
        assert_eq!(carousel.push(&a_ddb(1, 2, 0, 0, b"abcd")), None);
        assert!(
            carousel
                .modules
                .iter()
                .all(|module| module.blocks.is_empty())
        );
    }

    #[test]
    fn a_new_list_keeps_what_was_whole_and_gathers_what_changed() {
        let mut carousel = Carousel::new();
        carousel.push(&a_dii(1, 1, 4, &[(1, 2, 0), (2, 2, 0)]));
        assert!(matches!(
            carousel.push(&a_ddb(1, 1, 0, 0, b"ab")),
            Some(Brought::Module(_))
        ));
        // Module 2 goes to version 1; module 1 stands.
        assert!(matches!(
            carousel.push(&a_dii(2, 1, 4, &[(1, 2, 0), (2, 2, 1)])),
            Some(Brought::Listed(_))
        ));
        assert_eq!(carousel.push(&a_ddb(1, 1, 0, 0, b"ab")), None);
        assert_eq!(carousel.push(&a_ddb(1, 2, 0, 0, b"xx")), None);
        let Some(Brought::Module(module)) = carousel.push(&a_ddb(1, 2, 1, 0, b"cd")) else {
            panic!("the changed module is gathered afresh");
        };
        assert_eq!(
            (module.id, module.version, module.data.as_slice()),
            (2, 1, &b"cd"[..])
        );
    }

    #[test]
    fn a_damaged_section_is_refused() {
        let mut bad = a_dii(1, 1, 4, &[(1, 10, 0)]);
        bad.truncate(20);
        assert_eq!(dii(&bad), None);
        assert_eq!(ddb(&a_dii(1, 1, 4, &[])), None);
        assert_eq!(Carousel::new().push(&a_ddb(1, 1, 0, 0, b"ab")), None);
    }
}

// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The tables a data transmission message carries, and the entitlement messages
//! that ride beside them. Like the rest of the parse-only tables these are read
//! and not acted on -- a client does not present a data broadcast, and the
//! messages are for whatever program the stream is passed through -- but a
//! capture holds them and a reader should be able to say what is in one.

use crate::mmttables::{
    self, TABLE_MH_CAT, TABLE_MH_ECM_FIRST, TABLE_MH_ECM_LAST, TABLE_MH_EMM_LAST,
};
use crate::tstables::{self, Descriptor, be16, be32, long_section};

pub const TABLE_DDMT: u8 = 0xa3;
pub const TABLE_DAMT: u8 = 0xa4;
pub const TABLE_DCCT: u8 = 0xa5;
pub const TABLE_EMT: u8 = 0xa6;

/// The DDMT: the directory tree a data broadcast presents, with each node and
/// file given the tag its content is carried under. STD-B60 Table 9-6.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataDirectory {
    pub session_id: u8,
    pub version: u8,
    pub number: u8,
    pub last_number: u8,
    pub base_path: String,
    pub nodes: Vec<DirectoryNode>,
}

/// One directory of the tree and the files directly in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirectoryNode {
    pub node_tag: u16,
    pub version: u8,
    pub path: String,
    pub files: Vec<DirectoryFile>,
}

/// One file, named here and carried under its tag elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryFile {
    pub node_tag: u16,
    pub name: String,
}

/// Reads a DDMT section.
pub fn data_directory(section: &[u8]) -> Option<DataDirectory> {
    let header = long_section(section)?;
    if header.table_id != TABLE_DDMT || header.body.len() < 2 {
        return None;
    }
    let body = header.body;
    // The session identifier is the first byte of what a long-form section
    // calls the table identifier extension; the second is kept for later.
    let mut table = DataDirectory {
        session_id: (header.extension >> 8) as u8,
        version: header.version,
        number: header.number,
        last_number: header.last_number,
        ..DataDirectory::default()
    };
    let mut at = 0;
    let path = usize::from(body[at]);
    at += 1;
    if at + path + 1 > body.len() {
        return None;
    }
    table.base_path = String::from_utf8_lossy(&body[at..at + path]).into_owned();
    at += path;
    let nodes = usize::from(body[at]);
    at += 1;
    for _ in 0..nodes {
        if at + 4 > body.len() {
            return Some(table);
        }
        let mut node = DirectoryNode {
            node_tag: be16(&body[at..]),
            version: body[at + 2],
            ..DirectoryNode::default()
        };
        let node_path = usize::from(body[at + 3]);
        at += 4;
        if at + node_path + 2 > body.len() {
            return Some(table);
        }
        node.path = String::from_utf8_lossy(&body[at..at + node_path]).into_owned();
        at += node_path;
        let files = usize::from(be16(&body[at..]));
        at += 2;
        for _ in 0..files {
            if at + 3 > body.len() {
                return Some(table);
            }
            let name = usize::from(body[at + 2]);
            if at + 3 + name > body.len() {
                return Some(table);
            }
            node.files.push(DirectoryFile {
                node_tag: be16(&body[at..]),
                name: String::from_utf8_lossy(&body[at + 3..at + 3 + name]).into_owned(),
            });
            at += 3 + name;
        }
        table.nodes.push(node);
    }
    Some(table)
}

/// The DAMT: which media processing units of a component carry which items,
/// and how big each of them is. STD-B60 Table 9-7.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataAssetManagement {
    pub session_id: u8,
    pub version: u8,
    pub number: u8,
    pub last_number: u8,
    pub transaction_id: u32,
    pub component_tag: u16,
    pub download_id: u32,
    pub units: Vec<ManagedUnit>,
    pub component_info: Vec<u8>,
}

/// One media processing unit and the items it carries. An index item holds the
/// item table itself, in which case the items are named here only by their
/// tags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagedUnit {
    pub sequence_number: u32,
    pub size: u32,
    pub index_item: bool,
    pub compression_type: u8,
    pub index_item_id: Option<u32>,
    pub items: Vec<ManagedItem>,
    pub info: Vec<u8>,
}

/// One item of a unit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagedItem {
    pub node_tag: u16,
    pub item_id: u32,
    pub size: u32,
    pub version: u8,
    pub checksum: Option<u32>,
    pub info: Vec<u8>,
}

/// Reads a DAMT section.
pub fn data_asset_management(section: &[u8]) -> Option<DataAssetManagement> {
    let header = long_section(section)?;
    if header.table_id != TABLE_DAMT || header.body.len() < 11 {
        return None;
    }
    let body = header.body;
    let mut table = DataAssetManagement {
        session_id: (header.extension >> 8) as u8,
        version: header.version,
        number: header.number,
        last_number: header.last_number,
        transaction_id: be32(body),
        component_tag: be16(&body[4..]),
        download_id: be32(&body[6..]),
        ..DataAssetManagement::default()
    };
    let units = usize::from(body[10]);
    let mut at = 11;
    for _ in 0..units {
        if at + 9 > body.len() {
            return Some(table);
        }
        let mut unit = ManagedUnit {
            sequence_number: be32(&body[at..]),
            size: be32(&body[at + 4..]),
            index_item: body[at + 8] & 0x80 != 0,
            compression_type: body[at + 8] >> 4 & 0x03,
            ..ManagedUnit::default()
        };
        let has_index_id = body[at + 8] & 0x40 != 0;
        at += 9;
        if unit.index_item && has_index_id {
            if at + 4 > body.len() {
                return Some(table);
            }
            unit.index_item_id = Some(be32(&body[at..]));
            at += 4;
        }
        if at + 2 > body.len() {
            return Some(table);
        }
        let items = usize::from(be16(&body[at..]));
        at += 2;
        for _ in 0..items {
            if at + 2 > body.len() {
                return Some(table);
            }
            let mut item = ManagedItem {
                node_tag: be16(&body[at..]),
                ..ManagedItem::default()
            };
            at += 2;
            // The unit that carries the index describes its items there rather
            // than here, so only the tag is written for those.
            if !unit.index_item {
                if at + 10 > body.len() {
                    return Some(table);
                }
                item.item_id = be32(&body[at..]);
                item.size = be32(&body[at + 4..]);
                item.version = body[at + 8];
                let checksum = body[at + 9] & 0x80 != 0;
                at += 10;
                if checksum {
                    if at + 4 > body.len() {
                        return Some(table);
                    }
                    item.checksum = Some(be32(&body[at..]));
                    at += 4;
                }
                if at >= body.len() {
                    return Some(table);
                }
                let info = usize::from(body[at]);
                at += 1;
                if at + info > body.len() {
                    return Some(table);
                }
                item.info = body[at..at + info].to_vec();
                at += info;
            }
            unit.items.push(item);
        }
        if at >= body.len() {
            return Some(table);
        }
        let info = usize::from(body[at]);
        at += 1;
        if at + info > body.len() {
            return Some(table);
        }
        unit.info = body[at..at + info].to_vec();
        at += info;
        table.units.push(unit);
    }
    if at < body.len() {
        let info = usize::from(body[at]);
        at += 1;
        if at + info <= body.len() {
            table.component_info = body[at..at + info].to_vec();
        }
    }
    Some(table)
}

/// The DCCT: how the files of a data broadcast are grouped into the units a
/// receiver presents together. STD-B60 Table 9-8.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataContentConfiguration {
    pub session_id: u8,
    pub version: u8,
    pub number: u8,
    pub last_number: u8,
    pub content_id: u16,
    pub content_version: u8,
    pub content_size: u32,
    pub units: Vec<PresentedUnit>,
    /// The alternative to units: a content that names no presentation unit
    /// lists its nodes flat instead.
    pub nodes: Vec<u16>,
    pub descriptors: Vec<Descriptor>,
}

/// One presentation unit and the nodes that make it up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresentedUnit {
    pub tag: u8,
    pub size: u32,
    pub nodes: Vec<u16>,
    pub descriptors: Vec<Descriptor>,
}

/// Reads a DCCT section.
pub fn data_content_configuration(section: &[u8]) -> Option<DataContentConfiguration> {
    let header = long_section(section)?;
    if header.table_id != TABLE_DCCT || header.body.len() < 8 {
        return None;
    }
    let body = header.body;
    let mut table = DataContentConfiguration {
        session_id: (header.extension >> 8) as u8,
        version: header.version,
        number: header.number,
        last_number: header.last_number,
        content_id: be16(body),
        content_version: body[2],
        content_size: be32(&body[3..]),
        ..DataContentConfiguration::default()
    };
    let units = body[7] & 0x80 != 0;
    let content = body[7] & 0x40 != 0;
    let mut at = 8;
    if units {
        if at >= body.len() {
            return Some(table);
        }
        let count = usize::from(body[at]);
        at += 1;
        for _ in 0..count {
            if at + 6 > body.len() {
                return Some(table);
            }
            let mut unit = PresentedUnit {
                tag: body[at],
                size: be32(&body[at + 1..]),
                ..PresentedUnit::default()
            };
            let members = usize::from(body[at + 5]);
            at += 6;
            if at + 2 * members + 1 > body.len() {
                return Some(table);
            }
            for index in 0..members {
                unit.nodes.push(be16(&body[at + 2 * index..]));
            }
            at += 2 * members;
            let described = usize::from(body[at]);
            at += 1;
            if at + described > body.len() {
                return Some(table);
            }
            unit.descriptors = tstables::descriptors(&body[at..at + described]);
            at += described;
            table.units.push(unit);
        }
    } else {
        if at + 2 > body.len() {
            return Some(table);
        }
        let count = usize::from(be16(&body[at..]));
        at += 2;
        if at + 2 * count > body.len() {
            return Some(table);
        }
        for index in 0..count {
            table.nodes.push(be16(&body[at + 2 * index..]));
        }
        at += 2 * count;
    }
    if content && at < body.len() {
        let length = usize::from(body[at]);
        at += 1;
        if at + length <= body.len() {
            table.descriptors = tstables::descriptors(&body[at..at + length]);
        }
    }
    Some(table)
}

/// The EMT: the messages a data broadcast's application is listening for,
/// carried as descriptors. STD-B60 Table 11-1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventMessage {
    pub data_event_id: u8,
    pub message_group_id: u16,
    pub version: u8,
    pub number: u8,
    pub last_number: u8,
    pub descriptors: Vec<Descriptor>,
}

/// Reads an EMT section. Its descriptors are written the way a transport
/// stream writes them, not the way MMT-SI does.
pub fn event_message(section: &[u8]) -> Option<EventMessage> {
    let header = long_section(section)?;
    if header.table_id != TABLE_EMT {
        return None;
    }
    Some(EventMessage {
        data_event_id: (header.extension >> 12) as u8,
        message_group_id: header.extension & 0x0fff,
        version: header.version,
        number: header.number,
        last_number: header.last_number,
        descriptors: tstables::descriptors(header.body),
    })
}

/// An ECM or an EMM: the header a receiver routes it by, and a body only the
/// conditional access system can read. STD-B60 Tables 7-13 and 7-14.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntitlementMessage {
    pub table_id: u8,
    pub extension: u16,
    pub version: u8,
    pub number: u8,
    pub last_number: u8,
    pub data: Vec<u8>,
}

impl EntitlementMessage {
    /// Whether the message carries the key for a program rather than an
    /// entitlement for a receiver.
    pub fn control(&self) -> bool {
        self.table_id <= TABLE_MH_ECM_LAST
    }
}

/// Reads an ECM or EMM section.
pub fn entitlement_message(section: &[u8]) -> Option<EntitlementMessage> {
    let header = long_section(section)?;
    if !(TABLE_MH_ECM_FIRST..=TABLE_MH_EMM_LAST).contains(&header.table_id) {
        return None;
    }
    Some(EntitlementMessage {
        table_id: header.table_id,
        extension: header.extension,
        version: header.version,
        number: header.number,
        last_number: header.last_number,
        data: header.body.to_vec(),
    })
}

/// The MH-CAT, which says where the entitlement messages of the package are.
/// It is an MMT table rather than a section: a version and a two-byte length,
/// and then descriptors. STD-B60 Table 7-17.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConditionalAccess {
    pub version: u8,
    pub descriptors: Vec<mmttables::Descriptor>,
}

/// Reads an MH-CAT table.
pub fn conditional_access(table: &[u8]) -> Option<ConditionalAccess> {
    if table.len() < 4 || table[0] != TABLE_MH_CAT {
        return None;
    }
    let length = usize::from(be16(&table[2..]));
    if 4 + length > table.len() {
        return None;
    }
    Some(ConditionalAccess {
        version: table[1],
        descriptors: mmttables::descriptors(&table[4..4 + length]),
    })
}

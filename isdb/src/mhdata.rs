// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! MMT のデータ伝送。**MMT のデータ放送（ARIB-HTML5）のファイルがどう
//! 運ばれるか。**
//!
//! DSM-CC のカルーセルに当たるものが、MMT では三つに分かれています
//! （ARIB STD-B60 10.3、運用は TR-B39 第 3 編 6 章）。
//!
//! - **データディレクトリ管理テーブル（DDMT、0xA3）**: ディレクトリツリー。
//!   ノードに `node_tag` が付き、MPU はそのノードに対応する。
//! - **データアセット管理テーブル（DAMT、0xA4）**: どの MPU がどのノードか
//!   （MPU ノード記述子 0x8035）と、MPU のバージョン。DII に当たる。
//! - **インデックスアイテム**: 各 MPU の最初のアイテム（`item_id` 0）で、その MPU の
//!   ファイルの名前・MIME・圧縮（zlib）・元のサイズを示す。
//!
//! ファイルそのものは、タイムラインの無い MPU のアイテム（MFU）として
//! `item_id` ごとに届きます（`mmttlv::mmtp`）。ここはテーブルとインデックスを読むだけ。

use crate::tstables::{be16, be32, long_section};

/// データ伝送メッセージの message_id。
pub const MESSAGE_DATA_TRANSMISSION: u16 = 0x8003;
pub const TABLE_DDMT: u8 = 0xa3;
pub const TABLE_DAMT: u8 = 0xa4;
pub const TABLE_DCCT: u8 = 0xa5;
/// MPU ノード記述子。DAMT の `mpu_info` に置かれ、MPU を DDMT のノードへ結び付ける。
pub const DESCRIPTOR_MPU_NODE: u16 = 0x8035;

/// 一つのディレクトリノード。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryNode {
    pub tag: u16,
    pub version: u8,
    /// ベースからの相対パス。`top/img/` のように末尾に `/`。
    pub path: String,
    /// ノードに書かれたファイル（運用では 0 個で、インデックスアイテムが示す）。
    pub files: Vec<(u16, String)>,
}

/// DDMT の一つのセクション。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirectoryTable {
    pub session: u8,
    pub version: u8,
    pub section: u8,
    pub last_section: u8,
    /// ベースパス。`base/` のように末尾に `/`。
    pub base: String,
    pub nodes: Vec<DirectoryNode>,
}

/// DDMT のセクションを読む。
pub fn directory_table(section: &[u8]) -> Option<DirectoryTable> {
    let header = long_section(section)?;
    if header.table_id != TABLE_DDMT {
        return None;
    }
    let body = header.body;
    let mut at = 0;
    let base_len = usize::from(*body.first()?);
    at += 1;
    let base = String::from_utf8_lossy(body.get(at..at + base_len)?).into_owned();
    at += base_len;
    let count = usize::from(*body.get(at)?);
    at += 1;
    let mut table = DirectoryTable {
        session: (header.extension >> 8) as u8,
        version: header.version,
        section: header.number,
        last_section: header.last_number,
        base,
        nodes: Vec::new(),
    };
    for _ in 0..count {
        let tag = be16(body.get(at..at + 2)?);
        let version = *body.get(at + 2)?;
        let path_len = usize::from(*body.get(at + 3)?);
        at += 4;
        let path = String::from_utf8_lossy(body.get(at..at + path_len)?).into_owned();
        at += path_len;
        let files = usize::from(be16(body.get(at..at + 2)?));
        at += 2;
        let mut named = Vec::new();
        for _ in 0..files {
            let file_tag = be16(body.get(at..at + 2)?);
            let name_len = usize::from(*body.get(at + 2)?);
            at += 3;
            named.push((
                file_tag,
                String::from_utf8_lossy(body.get(at..at + name_len)?).into_owned(),
            ));
            at += name_len;
        }
        table.nodes.push(DirectoryNode {
            tag,
            version,
            path,
            files: named,
        });
    }
    Some(table)
}

/// DAMT が示す一つの MPU。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedMpu {
    pub sequence: u32,
    pub size: u32,
    /// インデックスアイテムがあるか。
    pub indexed: bool,
    /// MPU ノード記述子が結び付ける DDMT のノード。
    pub node: Option<u16>,
}

/// DAMT の一つのセクション。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetTable {
    pub session: u8,
    pub version: u8,
    pub section: u8,
    pub last_section: u8,
    pub transaction: u32,
    pub component_tag: u16,
    pub download_id: u32,
    pub mpus: Vec<ManagedMpu>,
}

/// DAMT のセクションを読む。アイテムの詳細（インデックスアイテムの無い運用）は読み飛ばす。
pub fn asset_table(section: &[u8]) -> Option<AssetTable> {
    let header = long_section(section)?;
    if header.table_id != TABLE_DAMT {
        return None;
    }
    let body = header.body;
    let mut table = AssetTable {
        session: (header.extension >> 8) as u8,
        version: header.version,
        section: header.number,
        last_section: header.last_number,
        transaction: be32(body.get(0..4)?),
        component_tag: be16(body.get(4..6)?),
        download_id: be32(body.get(6..10)?),
        mpus: Vec::new(),
    };
    let count = usize::from(*body.get(10)?);
    let mut at = 11;
    for _ in 0..count {
        let sequence = be32(body.get(at..at + 4)?);
        let size = be32(body.get(at + 4..at + 8)?);
        let flags = *body.get(at + 8)?;
        at += 9;
        let indexed = flags & 0x80 != 0;
        if indexed && flags & 0x40 != 0 {
            at += 4;
        }
        let items = usize::from(be16(body.get(at..at + 2)?));
        at += 2;
        for _ in 0..items {
            at += 2;
            if !indexed {
                // item_id, item_size, item_version, then the checksum flag
                // (STD-B60 Table 10-10).
                at += 4 + 4 + 1 + 1;
                let checksum = *body.get(at - 1)? & 0x80 != 0;
                if checksum {
                    at += 4;
                }
                let info = usize::from(*body.get(at)?);
                at += 1 + info;
            }
        }
        let info_len = usize::from(*body.get(at)?);
        at += 1;
        let info = body.get(at..at + info_len)?;
        at += info_len;
        let node = crate::mmttables::descriptors(info)
            .into_iter()
            .find(|d| d.tag == DESCRIPTOR_MPU_NODE)
            .and_then(|d| d.body.get(0..2).map(be16));
        table.mpus.push(ManagedMpu {
            sequence,
            size,
            indexed,
            node,
        });
    }
    Some(table)
}

/// インデックスアイテムが示す一つのファイル。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedItem {
    pub id: u32,
    /// 伝送されるサイズ（圧縮後）。0 なら不定。
    pub size: u32,
    pub version: u8,
    pub name: String,
    pub checksum: Option<u32>,
    /// MIME。`application/javascript` など。
    pub kind: String,
    /// 0x00 は zlib、0xFF は無圧縮。
    pub compression: u8,
    pub original_size: u32,
}

impl IndexedItem {
    pub fn compressed(&self) -> bool {
        self.compression == 0x00
    }
}

/// インデックスアイテム（`item_id` 0 の MFU）を読む。TR-B39 第 3 編 表 6-21。
pub fn index_item(bytes: &[u8]) -> Option<Vec<IndexedItem>> {
    let count = usize::from(be16(bytes.get(0..2)?));
    let mut at = 2;
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        let id = be32(bytes.get(at..at + 4)?);
        let size = be32(bytes.get(at + 4..at + 8)?);
        let version = *bytes.get(at + 8)?;
        let name_len = usize::from(*bytes.get(at + 9)?);
        at += 10;
        let name = String::from_utf8_lossy(bytes.get(at..at + name_len)?).into_owned();
        at += name_len;
        let flag = *bytes.get(at)?;
        at += 1;
        let checksum = if flag & 0x80 != 0 {
            let sum = be32(bytes.get(at..at + 4)?);
            at += 4;
            Some(sum)
        } else {
            None
        };
        let kind_len = usize::from(*bytes.get(at)?);
        at += 1;
        let kind = String::from_utf8_lossy(bytes.get(at..at + kind_len)?).into_owned();
        at += kind_len;
        let compression = *bytes.get(at)?;
        at += 1;
        // 元のサイズは圧縮しているときだけ。無圧縮（0xFF）の運用では省かれる。
        let original_size = if compression == 0xff {
            size
        } else {
            let v = be32(bytes.get(at..at + 4)?);
            at += 4;
            v
        };
        items.push(IndexedItem {
            id,
            size,
            version,
            name,
            checksum,
            kind,
            compression,
            original_size,
        });
    }
    Some(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 長形式のセクション：ヘッダー、拡張、番号、テーブルの内容、CRC。
    fn section(
        table: u8,
        extension: u16,
        version: u8,
        number: u8,
        last: u8,
        body: &[u8],
    ) -> Vec<u8> {
        let mut section = vec![table, 0x00, 0x00];
        section.extend_from_slice(&extension.to_be_bytes());
        section.extend_from_slice(&[0xc1 | version << 1, number, last]);
        section.extend_from_slice(body);
        let length = section.len() + 4 - 3;
        section[1] = 0xb0 | ((length >> 8) as u8 & 0x0f);
        section[2] = (length & 0xff) as u8;
        let check = mpegts::crc32(&section);
        section.extend_from_slice(&check.to_be_bytes());
        section
    }

    /// 長さ一バイトの後に内容。
    fn counted(bytes: &[u8]) -> Vec<u8> {
        let mut out = vec![bytes.len() as u8];
        out.extend_from_slice(bytes);
        out
    }

    #[test]
    fn the_directory_table_names_the_nodes() {
        let mut body = counted(b"base/");
        body.push(2);
        // ノード 0x0101：ファイルを書かない（インデックスアイテムが示す）。
        body.extend_from_slice(&[0x01, 0x01, 0x03]);
        body.extend(counted(b"top/"));
        body.extend_from_slice(&0u16.to_be_bytes());
        // ノード 0x0102：ファイルを一つ書く。
        body.extend_from_slice(&[0x01, 0x02, 0x04]);
        body.extend(counted(b"top/img/"));
        body.extend_from_slice(&1u16.to_be_bytes());
        body.extend_from_slice(&0x0010u16.to_be_bytes());
        body.extend(counted(b"a.png"));

        let table =
            directory_table(&section(TABLE_DDMT, 0x0700, 2, 0, 1, &body)).expect("readable");
        assert_eq!(table.session, 0x07);
        assert_eq!(table.version, 2);
        assert_eq!(table.section, 0);
        assert_eq!(table.last_section, 1);
        assert_eq!(table.base, "base/");
        assert_eq!(table.nodes.len(), 2);
        assert_eq!(table.nodes[0].tag, 0x0101);
        assert_eq!(table.nodes[0].version, 3);
        assert_eq!(table.nodes[0].path, "top/");
        assert!(table.nodes[0].files.is_empty());
        assert_eq!(table.nodes[1].path, "top/img/");
        assert_eq!(table.nodes[1].files, vec![(0x0010, "a.png".to_string())]);
        assert!(directory_table(&section(TABLE_DAMT, 0x0700, 2, 0, 1, &body)).is_none());
    }

    #[test]
    fn the_asset_table_ties_mpus_to_nodes() {
        let node = |tag: u16| {
            let mut info = DESCRIPTOR_MPU_NODE.to_be_bytes().to_vec();
            info.push(2);
            info.extend_from_slice(&tag.to_be_bytes());
            counted(&info)
        };
        let mut body = Vec::new();
        body.extend_from_slice(&0x0000_0001u32.to_be_bytes()); // transaction
        body.extend_from_slice(&0x0040u16.to_be_bytes()); // component_tag
        body.extend_from_slice(&0x0000_1234u32.to_be_bytes()); // download_id
        body.push(2);
        // インデックスアイテムのある MPU：アイテムの詳細は書かれない。
        body.extend_from_slice(&0x0101_0001u32.to_be_bytes());
        body.extend_from_slice(&0x0000_0100u32.to_be_bytes());
        body.push(0xbf);
        body.extend_from_slice(&0u16.to_be_bytes());
        body.extend(node(0x0101));
        // インデックスアイテムの無い MPU：アイテムごとの詳細（チェックサム付き）を読み飛ばす。
        body.extend_from_slice(&0x0102_0001u32.to_be_bytes());
        body.extend_from_slice(&0x0000_0200u32.to_be_bytes());
        body.push(0x3f);
        body.extend_from_slice(&1u16.to_be_bytes());
        body.extend_from_slice(&[0x00, 0x01]); // node_tag
        body.extend_from_slice(&[0, 0, 0, 0x20]); // item_id
        body.extend_from_slice(&[0, 0, 0x01, 0]); // item_size
        body.push(0x03); // item_version
        body.push(0x80); // チェックサムあり
        body.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        body.extend(counted(&[]));
        body.extend(node(0x0102));

        let table = asset_table(&section(TABLE_DAMT, 0x0700, 1, 0, 0, &body)).expect("readable");
        assert_eq!(table.component_tag, 0x0040);
        assert_eq!(table.download_id, 0x1234);
        assert_eq!(table.mpus.len(), 2);
        assert_eq!(table.mpus[0].sequence, 0x0101_0001);
        assert_eq!(table.mpus[0].size, 0x100);
        assert!(table.mpus[0].indexed);
        assert_eq!(table.mpus[0].node, Some(0x0101));
        assert!(!table.mpus[1].indexed);
        assert_eq!(table.mpus[1].node, Some(0x0102));
    }

    #[test]
    fn the_index_item_names_the_files() {
        let mut bytes = 2u16.to_be_bytes().to_vec();
        // zlib で圧縮、チェックサムなし：元のサイズが続く。
        bytes.extend_from_slice(&0x0000_0001u32.to_be_bytes());
        bytes.extend_from_slice(&0x0000_0080u32.to_be_bytes());
        bytes.push(1);
        bytes.extend(counted(b"index.html"));
        bytes.push(0x7f);
        bytes.extend(counted(b"text/html"));
        bytes.push(0x00);
        bytes.extend_from_slice(&0x0000_0200u32.to_be_bytes());
        // 無圧縮、チェックサムあり：元のサイズは省かれる。
        bytes.extend_from_slice(&0x0000_0002u32.to_be_bytes());
        bytes.extend_from_slice(&0x0000_0040u32.to_be_bytes());
        bytes.push(1);
        bytes.extend(counted(b"a.png"));
        bytes.push(0xff);
        bytes.extend_from_slice(&0x0102_0304u32.to_be_bytes());
        bytes.extend(counted(b"image/png"));
        bytes.push(0xff);

        let items = index_item(&bytes).expect("readable");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, 1);
        assert_eq!(items[0].name, "index.html");
        assert_eq!(items[0].kind, "text/html");
        assert!(items[0].compressed());
        assert_eq!(items[0].checksum, None);
        assert_eq!(items[0].size, 0x80);
        assert_eq!(items[0].original_size, 0x200);
        assert_eq!(items[1].name, "a.png");
        assert!(!items[1].compressed());
        assert_eq!(items[1].checksum, Some(0x0102_0304));
        assert_eq!(items[1].original_size, 0x40);
    }
}

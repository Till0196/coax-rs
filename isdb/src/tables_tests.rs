// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The tables in [`crate::tstables`], [`crate::mmttables`] and
//! [`crate::datatables`] are read but never acted on, so nothing else in the
//! client would notice them going wrong. These tests are what says they are
//! read correctly.

use crate::{datatables, mmttables, tstables};
use mpegts::crc32;

/// A long-form section: the header, the extension and numbering every such
/// table has, and then the table's own fields.
fn long(table: u8, extension: u16, body: &[u8]) -> Vec<u8> {
    let mut section = vec![table, 0x00, 0x00];
    section.extend_from_slice(&extension.to_be_bytes());
    section.extend_from_slice(&[0xc1, 0x00, 0x00]);
    section.extend_from_slice(body);
    let length = section.len() + 4 - 3;
    section[1] = 0xb0 | ((length >> 8) as u8 & 0x0f);
    section[2] = (length & 0xff) as u8;
    let check = crc32(&section);
    section.extend_from_slice(&check.to_be_bytes());
    section
}

/// A short-form section, which is how the two time tables are written.
fn short(table: u8, body: &[u8]) -> Vec<u8> {
    let mut section = vec![table, 0x00, 0x00];
    section.extend_from_slice(body);
    let length = section.len() + 4 - 3;
    section[1] = 0x70 | ((length >> 8) as u8 & 0x0f);
    section[2] = (length & 0xff) as u8;
    section.extend_from_slice(&0u32.to_be_bytes());
    section
}

#[test]
fn the_program_map_says_what_a_program_is_made_of() {
    let body = [
        0xe1, 0x00, // the clock is on 0x100
        0xf0, 0x05, // the program's own descriptors
        0xc1, 0x01, 0x84, 0x09, 0x00, 0x02, 0xe1, 0x11, 0xf0, 0x03, 0x52, 0x01,
        0x00, // video, with a stream identifier
        0x0f, 0xe1, 0x12, 0xf0, 0x00, // audio, with nothing said about it
    ];
    let map = tstables::program_map(&long(tstables::TABLE_PMT, 0x0400, &body))
        .expect("the map was not read");
    assert_eq!(map.program_number, 0x0400);
    assert_eq!(map.pcr_pid, 0x100);
    assert_eq!(map.descriptors.len(), 2);
    assert_eq!(map.streams.len(), 2);
    assert_eq!(map.streams[0].stream_type, 0x02);
    assert_eq!(map.streams[0].pid, 0x111);
    assert!(tstables::find(&map.streams[0].descriptors, 0x52).is_some());
    assert!(map.streams[1].descriptors.is_empty());
}

/// A cable and a satellite both name the services of their neighbours. The
/// table is told apart from the stream's own only by its identifier, so a
/// reader that takes one for the other lists another multiplex's services as
/// this one's.
#[test]
fn the_service_table_of_another_stream_is_read_as_another_stream_s() {
    let body = [
        0x00, 0x04, 0xff, // original network 4
        0x03, 0xa1, 0xff, 0x90, 0x05, // service 929, running, under access
        0x48, 0x03, 0x01, 0x00, 0x00,
    ];
    let table = tstables::service_description(&long(tstables::TABLE_SDT_OTHER, 0x1234, &body))
        .expect("the table was not read");
    assert!(!table.own());
    assert_eq!(table.transport_stream_id, 0x1234);
    assert_eq!(table.original_network_id, 4);
    assert_eq!(table.services.len(), 1);
    let one = &table.services[0];
    assert_eq!(one.service_id, 0x03a1);
    assert!(one.eit_schedule && one.eit_present_following && one.scrambled);
}

#[test]
fn the_event_table_says_when_a_program_is_and_how_long_it_runs() {
    let event = [
        0x12, 0x34, // event 0x1234
        0xef, 0x66, 0x00, 0x00, 0x00, // 3 September 2026, midnight
        0x00, 0x30, 0x00, // half an hour
        0x90, 0x07, // running, under access, seven bytes of descriptors
        0x4d, 0x05, 0x6a, 0x70, 0x6e, 0x01, 0x41, 0x00,
    ];
    let mut body = vec![0x00, 0x40, 0x00, 0x04, 0x00, 0x4e];
    body.extend_from_slice(&event);
    let table = tstables::event_table(&long(tstables::TABLE_EIT_OWN, 0x0065, &body))
        .expect("the table was not read");
    assert!(table.own() && !table.schedule());
    assert_eq!(table.service_id, 0x0065);
    assert_eq!(table.transport_stream_id, 0x0040);
    assert_eq!(table.events.len(), 1);
    let one = &table.events[0];
    let start = one.start.expect("the event has a start");
    assert_eq!((start.year, start.month, start.day), (2026, 9, 3));
    assert_eq!((start.hour, start.minute, start.second), (0, 0, 0));
    assert_eq!(one.duration, Some(30 * 60));
    assert_eq!(one.running_status, 4);
    assert!(one.scrambled);
    assert!(tstables::find(&one.descriptors, 0x4d).is_some());
}

/// A start of every bit set is the table saying it does not know when the
/// event begins, which is not the same as the epoch.
#[test]
fn an_undefined_start_is_not_a_moment() {
    let event = [
        0x00, 0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00,
    ];
    let mut body = vec![0x00, 0x40, 0x00, 0x04, 0x00, 0x4e];
    body.extend_from_slice(&event);
    let table =
        tstables::event_table(&long(tstables::TABLE_EIT_OWN, 1, &body)).expect("read the table");
    assert_eq!(table.events.len(), 1);
    assert!(table.events[0].start.is_none());
    assert!(table.events[0].duration.is_none());
}

#[test]
fn the_time_tables_say_what_time_the_stream_thinks_it_is() {
    let body = [
        0xef, 0x66, 0x12, 0x36, 0x49, 0xf0, 0x0d, 0x58, 0x0b, 0x4a, 0x50, 0x4e, 0xff, 0x09, 0x00,
        0xef, 0x66, 0x00, 0x00, 0x00,
    ];
    let table = tstables::time_table(&short(tstables::TABLE_TOT, &body)).expect("read the table");
    assert_eq!(
        (
            table.time.year,
            table.time.month,
            table.time.day,
            table.time.hour,
            table.time.minute,
            table.time.second
        ),
        (2026, 9, 3, 12, 36, 49)
    );
    assert_eq!(table.descriptors.len(), 1);
    assert_eq!(table.descriptors[0].tag, 0x58);

    let plain = tstables::time_table(&short(tstables::TABLE_TDT, &body[..5])).expect("read it");
    assert_eq!(plain.time, table.time);
    assert!(plain.descriptors.is_empty());
}

#[test]
fn the_broadcaster_table_names_the_broadcasters() {
    let body = [0xf0, 0x00, 0x01, 0xf0, 0x05, 0xd8, 0x03, 0x53, 0x4d, 0x50];
    let table = tstables::broadcaster_information(&long(tstables::TABLE_BIT, 0x0004, &body))
        .expect("read the table");
    assert_eq!(table.original_network_id, 4);
    assert_eq!(table.broadcasters.len(), 1);
    assert_eq!(table.broadcasters[0].broadcaster_id, 1);
    let name = tstables::find(&table.broadcasters[0].descriptors, 0xd8).expect("a name");
    assert_eq!(name.body, b"SMP");
}

#[test]
fn the_access_table_says_where_the_entitlements_are() {
    let body = [0x09, 0x04, 0x00, 0x05, 0xe0, 0x01];
    let table = tstables::conditional_access(&long(tstables::TABLE_CAT, 0xffff, &body))
        .expect("read the table");
    assert_eq!(table.descriptors.len(), 1);
    assert_eq!(table.descriptors[0].tag, 0x09);
}

/// The package table is the one that says what an MMT broadcast is made of. The
/// asset locations are what a demultiplexer would follow, so the packet
/// identifier has to come out of the right end of them.
#[test]
fn the_package_table_says_what_a_broadcast_is_made_of() {
    let asset = [
        0x00, // identifier type
        0x00, 0x00, 0x00, 0x00, // scheme
        0x02, 0x00, 0x65, // a two-byte identifier
        b'h', b'e', b'v', b'1', // the asset type
        0x00, // no clock relation
        0x01, // one location
        0x00, 0xf3, 0x00, // in this stream, packet 0xf300
        0x00, 0x03, 0x80, 0x01, 0x00, // one descriptor
    ];
    let mut body = vec![0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x65];
    body.extend_from_slice(&[0x00, 0x00, 0x01]);
    body.extend_from_slice(&asset);
    let mut table = vec![
        mmttables::TABLE_MPT_COMPLETE,
        0x0d,
        (body.len() >> 8) as u8,
        (body.len() & 0xff) as u8,
    ];
    table.extend_from_slice(&body);

    let read = mmttables::package_table(&table).expect("the table was not read");
    assert_eq!(read.service_id(), 0x0065);
    assert_eq!(read.assets.len(), 1);
    assert_eq!(read.assets[0].kind, "hev1");
    assert_eq!(read.assets[0].packet_id(), Some(0xf300));
    assert_eq!(read.assets[0].descriptors.len(), 1);
    assert_eq!(read.assets[0].descriptors[0].tag, 0x8001);
}

#[test]
fn the_mh_service_table_of_another_stream_is_read() {
    let body = [0x00, 0x0b, 0xff, 0x00, 0x65, 0xfc, 0x80, 0x00, 0x00];
    let table = mmttables::service_description(&long(mmttables::TABLE_MH_SDT_OTHER, 0xb0f1, &body))
        .expect("read the table");
    assert!(!table.own());
    assert_eq!(table.tlv_stream_id, 0xb0f1);
    assert_eq!(table.services.len(), 1);
    assert_eq!(table.services[0].service_id, 0x0065);
}

#[test]
fn the_mh_event_table_is_read_with_its_two_byte_descriptor_lengths() {
    let event = [
        0x00, 0x01, 0xef, 0x66, 0x18, 0x00, 0x00, 0x00, 0x45, 0x00, 0x80, 0x08, 0xf0, 0x01, 0x00,
        0x04, 0x6a, 0x70, 0x6e, 0x00,
    ];
    let mut body = vec![0xb0, 0xf1, 0x00, 0x0b, 0x00, 0x8b];
    body.extend_from_slice(&event);
    let table = mmttables::event_table(&long(mmttables::TABLE_MH_EIT_PRESENT, 0x0065, &body))
        .expect("read the table");
    assert_eq!(table.events.len(), 1);
    let one = &table.events[0];
    let start = one.start.expect("a start");
    assert_eq!(
        (start.year, start.month, start.day, start.hour),
        (2026, 9, 3, 18)
    );
    assert_eq!(one.duration, Some(45 * 60));
    assert_eq!(one.descriptors.len(), 1);
    assert_eq!(one.descriptors[0].tag, 0xf001);
}

#[test]
fn the_mh_time_broadcaster_and_application_tables_are_read() {
    let moment = [0xef, 0x66, 0x15, 0x01, 0x23, 0xf0, 0x00];
    let table =
        mmttables::time_table(&short(mmttables::TABLE_MH_TOT, &moment)).expect("read the table");
    assert_eq!(
        (table.time.hour, table.time.minute, table.time.second),
        (15, 1, 23)
    );

    let broadcasters = [0xf0, 0x00, 0x01, 0xf0, 0x00];
    let table =
        mmttables::broadcaster_information(&long(mmttables::TABLE_MH_BIT, 0x000b, &broadcasters))
            .expect("read the table");
    assert_eq!(table.broadcasters.len(), 1);
    assert_eq!(table.broadcasters[0].broadcaster_id, 1);

    let applications = [
        0xf0, 0x00, 0xf0, 0x09, 0x00, 0x00, 0x00, 0x07, 0x40, 0x01, 0x01, 0xf0, 0x00,
    ];
    let table =
        mmttables::application_information(&long(mmttables::TABLE_MH_AIT, 0x0010, &applications))
            .expect("read the table");
    assert_eq!(table.applications.len(), 1);
    assert_eq!(table.applications[0].organization_id, 7);
    assert_eq!(table.applications[0].application_id, 0x4001);
}

/// An EMT carrying one general event message (TR-B39 6.2.4: group 0x001,
/// data_event_id 0xf, one event message descriptor): message 5 of tag 0x40,
/// firing at once, with six bytes of private data.
#[test]
fn an_event_message_table_is_read() {
    let private = b"sample";
    let mut descriptor = vec![0xf0, 0x03, 0x00, (14 + private.len()) as u8];
    descriptor.extend_from_slice(&[0x00, 0x1f, 0x00]); // group 0x001, time mode 0
    descriptor.extend_from_slice(&[0; 8]);
    descriptor.extend_from_slice(&[0x01, 0x05, 0x1b]); // type 1, id 5 バージョン 0x1b
    descriptor.extend_from_slice(private);
    let section = long(mmttables::TABLE_EMT, 0xf001, &descriptor);
    let table = mmttables::event_message_table(&section).expect("read the table");
    assert_eq!(table.data_event_id, 0xf);
    assert_eq!(table.group_id, 0x001);
    assert_eq!(table.descriptors.len(), 1);
    assert_eq!(
        table.descriptors[0].tag,
        mmttables::DESCRIPTOR_EVENT_MESSAGE
    );
    let message = mmttables::event_message(&table.descriptors[0].body).expect("read the message");
    assert!(message.immediate());
    // `event_msg_id` は上 8 ビットが message_id、下 8 ビットが message_version
    // （STD-B62 第二編）。0xb074 なら id 176、バージョン 0x74。
    assert_eq!(message.message_id(), 5);
    assert_eq!(message.message_version(), 0x1b);
    assert_eq!(message.kind, 1);
    assert_eq!(message.private, private);
}

/// The address map is what says which datagrams a service is carried in. These
/// are the bytes one satellite feed sends: service 101 on an IPv6 group.
#[test]
fn the_address_map_says_which_datagrams_a_service_is() {
    let section: Vec<u8> = [
        "fef0310000df0000007f0065fc222401dbc010000000000000000000000280ff3e",
        "00000000000000000000a000100080ba2034ff",
    ]
    .concat()
    .as_bytes()
    .chunks(2)
    .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
    .collect();
    let table = mmttables::address_map(&section).expect("the table was not read");
    assert_eq!(table.services.len(), 1);
    let one = &table.services[0];
    assert_eq!(one.service_id, 101);
    assert_eq!(one.source.to_string(), "2401:dbc0:1000::2");
    assert_eq!(one.source_mask, 128);
    assert_eq!(one.destination.to_string(), "ff3e::a000:1000");
    assert_eq!(one.destination_mask, 128);
}

#[test]
fn the_data_transmission_tables_are_read() {
    let directory = [
        0x01, b'/', 0x01, 0x00, 0x02, 0x00, 0x01, b'a', 0x00, 0x01, 0x00, 0x03, 0x05, b's', b't',
        b'a', b'r', b't',
    ];
    let tree = datatables::data_directory(&long(datatables::TABLE_DDMT, 0x0100, &directory))
        .expect("read the directory");
    assert_eq!(tree.base_path, "/");
    assert_eq!(tree.nodes.len(), 1);
    assert_eq!(tree.nodes[0].path, "a");
    assert_eq!(tree.nodes[0].files.len(), 1);
    assert_eq!(tree.nodes[0].files[0].name, "start");

    let assets = [
        0x00, 0x00, 0x00, 0x01, // transaction
        0x00, 0x40, // component
        0x00, 0x00, 0x00, 0x02, // download
        0x01, // one unit
        0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x10, 0x00, 0x00, // sequence, size, flags
        0x00, 0x01, // one item
        0x00, 0x03, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x20, 0x01, 0x00, 0x00,
        0x00, // no unit info
        0x00, // no component info
    ];
    let managed = datatables::data_asset_management(&long(datatables::TABLE_DAMT, 0x0100, &assets))
        .expect("read the asset table");
    assert_eq!(managed.component_tag, 0x0040);
    assert_eq!(managed.units.len(), 1);
    assert_eq!(managed.units[0].items.len(), 1);
    assert_eq!(managed.units[0].items[0].size, 0x20);

    let content = [
        0x00, 0x07, 0x01, 0x00, 0x00, 0x08, 0x00, 0x80, 0x01, 0x09, 0x00, 0x00, 0x04, 0x00, 0x01,
        0x00, 0x03, 0x00,
    ];
    let configured =
        datatables::data_content_configuration(&long(datatables::TABLE_DCCT, 0x0100, &content))
            .expect("read the content table");
    assert_eq!(configured.content_id, 7);
    assert_eq!(configured.units.len(), 1);
    assert_eq!(configured.units[0].tag, 9);
    assert_eq!(configured.units[0].nodes, vec![3]);

    let messages = datatables::event_message(&long(
        datatables::TABLE_EMT,
        0x1002,
        &[0xf0, 0x02, 0x00, 0x01],
    ))
    .expect("read the message table");
    assert_eq!(messages.data_event_id, 1);
    assert_eq!(messages.message_group_id, 2);
    assert_eq!(messages.descriptors.len(), 1);
    assert_eq!(messages.descriptors[0].tag, 0xf0);
}

#[test]
fn the_entitlement_messages_and_their_table_are_read() {
    let control = datatables::entitlement_message(&long(
        mmttables::TABLE_MH_ECM_FIRST,
        0x0001,
        &[0x01, 0x02, 0x03],
    ))
    .expect("read the control message");
    assert!(control.control());
    assert_eq!(control.data, vec![0x01, 0x02, 0x03]);

    let management =
        datatables::entitlement_message(&long(mmttables::TABLE_MH_EMM_LAST, 0x0001, &[0x04]))
            .expect("read the management message");
    assert!(!management.control());

    let table = [
        mmttables::TABLE_MH_CAT,
        0x01,
        0x00,
        0x06,
        0x80,
        0x04,
        0x00,
        0x05,
        0x00,
        0x01,
    ];
    let access = datatables::conditional_access(&table).expect("read the access table");
    assert_eq!(access.descriptors.len(), 1);
    assert_eq!(access.descriptors[0].tag, 0x8004);
}

/// A descriptor loop entry with a one-byte length, which is what both of the
/// timestamp descriptors have.
fn described(tag: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = tag.to_be_bytes().to_vec();
    bytes.push(body.len() as u8);
    bytes.extend_from_slice(body);
    bytes
}

/// A second, as a network time.
const SECOND: u64 = 1 << 32;

#[test]
fn the_mpu_timestamp_descriptor_says_when_each_mpu_is_shown() {
    let base = 3_000_000_000u64 << 32;
    let mut body = Vec::new();
    for (sequence, ntp) in [(5u32, base), (6, base + SECOND)] {
        body.extend_from_slice(&sequence.to_be_bytes());
        body.extend_from_slice(&ntp.to_be_bytes());
    }
    // A trailing part-entry is not one and is ignored.
    body.push(0x00);
    let loop_bytes = described(mmttables::DESCRIPTOR_MPU_TIMESTAMP, &body);
    let read = mmttables::descriptors(&loop_bytes);
    let one = mmttables::find(&read, mmttables::DESCRIPTOR_MPU_TIMESTAMP).expect("the descriptor");
    let times = mmttables::mpu_timestamps(&one.body);
    assert_eq!(
        times,
        [
            mmttables::MpuTimestamp {
                sequence: 5,
                ntp: base
            },
            mmttables::MpuTimestamp {
                sequence: 6,
                ntp: base + SECOND
            },
        ]
    );
}

/// An extended timestamp descriptor whose access units all take the same
/// presentation offset: three frames of a 29.97 stream counted in the
/// transport stream's own clock.
fn default_offset_descriptor() -> Vec<u8> {
    let mut body = vec![0x03];
    body.extend_from_slice(&90_000u32.to_be_bytes());
    body.extend_from_slice(&3003u16.to_be_bytes());
    body.extend_from_slice(&5u32.to_be_bytes());
    body.push(0x00);
    body.extend_from_slice(&6006u16.to_be_bytes());
    body.push(3);
    for offset in [0u16, 3003, 6006] {
        body.extend_from_slice(&offset.to_be_bytes());
    }
    described(mmttables::DESCRIPTOR_MPU_EXTENDED_TIMESTAMP, &body)
}

/// The same, but writing a presentation offset for every access unit, which
/// makes each of them four bytes rather than two.
fn per_unit_offset_descriptor() -> Vec<u8> {
    let mut body = vec![0x05];
    body.extend_from_slice(&1000u32.to_be_bytes());
    body.extend_from_slice(&7u32.to_be_bytes());
    body.push(0x40);
    body.extend_from_slice(&20u16.to_be_bytes());
    body.push(2);
    for (dts_pts, pts) in [(10u16, 33u16), (0, 34)] {
        body.extend_from_slice(&dts_pts.to_be_bytes());
        body.extend_from_slice(&pts.to_be_bytes());
    }
    described(mmttables::DESCRIPTOR_MPU_EXTENDED_TIMESTAMP, &body)
}

fn extended(loop_bytes: &[u8]) -> mmttables::ExtendedTimestamp {
    let read = mmttables::descriptors(loop_bytes);
    let one = mmttables::find(&read, mmttables::DESCRIPTOR_MPU_EXTENDED_TIMESTAMP)
        .expect("the descriptor");
    mmttables::extended_timestamp(&one.body)
}

#[test]
fn one_presentation_offset_stands_for_every_access_unit() {
    let read = extended(&default_offset_descriptor());
    assert!(!read.truncated);
    assert_eq!(read.pts_offset_type, 1);
    assert_eq!(read.timescale, Some(90_000));
    assert_eq!(read.default_pts_offset, 3003);
    let entry = read.entry(5).expect("the MPU");
    assert_eq!(entry.leap, 0);
    assert_eq!(entry.decoding_time_offset, 6006);
    assert_eq!(
        entry.access_units,
        [
            mmttables::AccessUnitTiming {
                dts_pts_offset: 0,
                pts_offset: 3003
            },
            mmttables::AccessUnitTiming {
                dts_pts_offset: 3003,
                pts_offset: 3003
            },
            mmttables::AccessUnitTiming {
                dts_pts_offset: 6006,
                pts_offset: 3003
            },
        ]
    );
    assert!(read.entry(6).is_none());
}

#[test]
fn every_access_unit_can_write_its_own_presentation_offset() {
    let read = extended(&per_unit_offset_descriptor());
    assert!(!read.truncated);
    assert_eq!(read.pts_offset_type, 2);
    assert_eq!(read.timescale, Some(1000));
    assert_eq!(read.default_pts_offset, 0);
    let entry = read.entry(7).expect("the MPU");
    assert_eq!(entry.leap, 1);
    assert_eq!(entry.decoding_time_offset, 20);
    assert_eq!(
        entry.access_units,
        [
            mmttables::AccessUnitTiming {
                dts_pts_offset: 10,
                pts_offset: 33
            },
            mmttables::AccessUnitTiming {
                dts_pts_offset: 0,
                pts_offset: 34
            },
        ]
    );
}

#[test]
fn a_descriptor_that_runs_out_says_so() {
    assert!(mmttables::extended_timestamp(&[]).truncated);
    // A timescale is promised and not written.
    assert!(mmttables::extended_timestamp(&[0x01, 0x00, 0x01]).truncated);
    // A timescale of zero is no timescale.
    assert!(mmttables::extended_timestamp(&[0x01, 0x00, 0x00, 0x00, 0x00]).truncated);
    // Three access units are promised and two are written.
    let read = mmttables::extended_timestamp(&[
        0x03, 0x00, 0x00, 0x03, 0xe8, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x14, 0x03,
        0x00, 0x01, 0x00, 0x02,
    ]);
    assert!(read.truncated);
    assert!(read.entries.is_empty());
}

#[test]
fn the_transport_clock_counts_from_one_base() {
    let base = 3_000_000_000u64 << 32;
    let mut clock = mmttables::ClockBase::new();
    assert!(clock.to_90k(base).is_none());
    clock.set(base);
    clock.set(base + SECOND);
    assert_eq!(clock.ntp(), Some(base));
    assert_eq!(clock.to_90k(base), Some(0));
    assert_eq!(clock.to_90k(base + SECOND), Some(90_000));
    assert_eq!(clock.to_90k(base + SECOND / 2), Some(45_000));
    assert_eq!(clock.to_90k(base - SECOND), Some(-90_000));
}

#[test]
fn a_count_of_a_timescale_rounds_the_same_way_on_either_side_of_zero() {
    assert_eq!(mmttables::ticks_to_90k(1, 0), 0);
    assert_eq!(mmttables::ticks_to_90k(1, 90_000), 1);
    assert_eq!(mmttables::ticks_to_90k(20, 1000), 1800);
    assert_eq!(mmttables::ticks_to_90k(1, 7), 12_857);
    assert_eq!(mmttables::ticks_to_90k(-1, 7), -12_857);
    assert_eq!(mmttables::ticks_to_90k(1, 1_000_000), 0);
    assert_eq!(mmttables::ticks_to_90k(6, 1_000_000), 1);
}

#[test]
fn the_access_units_of_an_mpu_are_decoded_before_the_mpu_is_shown() {
    let read = extended(&default_offset_descriptor());
    let entry = read.entry(5).expect("the MPU");
    let timescale = read.timescale.expect("the timescale");
    let base = 3_000_000_000u64 << 32;
    let mut clock = mmttables::ClockBase::new();
    let times = |clock: &mut mmttables::ClockBase, ntp, sample| {
        mmttables::access_unit_times(
            clock,
            entry,
            timescale,
            read.pts_offset_type,
            read.default_pts_offset,
            ntp,
            sample,
        )
    };
    // The MPU's own time is the base, so its access units are counted from
    // zero: the first is decoded two frames early and shown at once.
    assert_eq!(
        times(&mut clock, base, 1),
        Some(mmttables::AccessUnitTimes {
            dts: -6006,
            pts: -6006
        })
    );
    assert_eq!(
        times(&mut clock, base, 2),
        Some(mmttables::AccessUnitTimes { dts: -3003, pts: 0 })
    );
    assert_eq!(
        times(&mut clock, base, 3),
        Some(mmttables::AccessUnitTimes { dts: 0, pts: 6006 })
    );
    // A later MPU is counted from the same base, not from its own time.
    assert_eq!(
        times(&mut clock, base + SECOND, 1),
        Some(mmttables::AccessUnitTimes {
            dts: 90_000 - 6006,
            pts: 90_000 - 6006
        })
    );
    assert_eq!(clock.ntp(), Some(base));
    // Access units are counted from one, and the entry has three.
    assert!(times(&mut clock, base, 0).is_none());
    assert!(times(&mut clock, base, 4).is_none());
}

#[test]
fn the_presentation_offsets_written_per_access_unit_are_the_ones_added_up() {
    let read = extended(&per_unit_offset_descriptor());
    let entry = read.entry(7).expect("the MPU");
    let timescale = read.timescale.expect("the timescale");
    let base = 3_000_000_000u64 << 32;
    let mut clock = mmttables::ClockBase::new();
    let mut times = |ntp, sample| {
        mmttables::access_unit_times(
            &mut clock,
            entry,
            timescale,
            read.pts_offset_type,
            read.default_pts_offset,
            ntp,
            sample,
        )
    };
    // Twenty of a millisecond timescale before the MPU's time, and shown ten
    // of them after that.
    assert_eq!(
        times(base, 1),
        Some(mmttables::AccessUnitTimes {
            dts: -1800,
            pts: -900
        })
    );
    // The second is the first access unit's own offset later, which is the
    // thirty-three the descriptor wrote for it rather than a default.
    assert_eq!(
        times(base, 2),
        Some(mmttables::AccessUnitTimes {
            dts: 1170,
            pts: 1170
        })
    );
    // A timescale of zero is no timescale, and no times come of it.
    assert!(
        mmttables::access_unit_times(
            &mut mmttables::ClockBase::new(),
            entry,
            0,
            read.pts_offset_type,
            read.default_pts_offset,
            base,
            1,
        )
        .is_none()
    );
}

/// CDT：局ロゴの PNG が種類・番号・バージョンと一緒に取れる。
#[test]
fn a_cdt_carries_a_logo() {
    let mut body = vec![0x7f, 0xe1, 0x01, 0xf0, 0x00];
    // logo_type 5、logo_id 0x123、バージョン 7、4 バイト。
    body.extend_from_slice(&[
        0x05, 0xff, 0x23, 0xf0, 0x07, 0x00, 0x04, 0x89, b'P', b'N', b'G',
    ]);
    let table = tstables::common_data(&long(tstables::TABLE_CDT, 0x0001, &body)).expect("read");
    assert_eq!(table.original_network_id, 0x7fe1);
    assert_eq!(
        (table.logo_type, table.logo_id, table.logo_version),
        (5, 0x123, 7)
    );
    assert_eq!(table.data, [0x89, b'P', b'N', b'G']);
}

/// SIT：伝送情報の記述子と、サービスごとの記述子。
// ビットフィールドごとに書く。`| 0x00` は「そのフィールドが 0」の意味。
#[allow(clippy::identity_op)]
#[test]
fn a_sit_names_the_recorded_service() {
    // 網識別記述子（JPN、地上、NID 0x7fe1）。
    let network = [0xc2, 0x07, b'J', b'P', b'N', 0x00, 0x01, 0x7f, 0xe1];
    // サービスの記述子：サービス記述子（種別 1、事業者無し、名前 "SMP"）。
    let service = [0x48, 0x06, 0x01, 0x00, 0x03, b'S', b'M', b'P'];
    let mut body = vec![0xf0 | 0x00, network.len() as u8];
    body.extend_from_slice(&network);
    body.extend_from_slice(&[0x04, 0x00, 0x40, service.len() as u8]);
    body.extend_from_slice(&service);
    let table =
        tstables::selection_information(&long(tstables::TABLE_SIT, 0xffff, &body)).expect("read");
    assert_eq!(table.descriptors.len(), 1);
    assert_eq!(table.services.len(), 1);
    assert_eq!(table.services[0].service_id, 0x0400);
    assert_eq!(table.services[0].running_status, 4);
    assert_eq!(table.services[0].descriptors[0].tag, 0x48);
}

/// SDTT：機種と、配信の内容と予定。
#[test]
fn an_sdtt_announces_software() {
    let mut body = vec![0x7f, 0xe1, 0x7f, 0xe1, 0x04, 0x00, 0x01];
    // グループ 1、対象バージョン 0x100、新バージョン 0x101、強制、全バージョン；記述 8 バイト = 予定 8 バイト。
    body.extend_from_slice(&[0x11, 0x00, 0x10, 0x14, 0x00, 0x80, 0x00, 0x80]);
    // 2026/09/19 12:00:00 から 3600 秒（MJD 61302 = 0xef66）。
    body.extend_from_slice(&[0xef, 0x66, 0x12, 0x00, 0x00, 0x01, 0x00, 0x00]);
    let table =
        tstables::software_download(&long(tstables::TABLE_SDTT, 0x1234, &body)).expect("read");
    assert_eq!((table.maker_id, table.model_id), (0x12, 0x34));
    assert_eq!(table.contents.len(), 1);
    let content = &table.contents[0];
    assert_eq!(
        (content.group, content.target_version, content.new_version),
        (1, 0x100, 0x101)
    );
    assert_eq!(content.download_level, 1);
    assert_eq!(content.schedules.len(), 1);
    assert_eq!(content.schedules[0].1, 3600);
    assert_eq!(content.schedules[0].0.hour, 12);
}

/// MH-SDTT：構造は SDTT と同じで、記述子は MMT-SI のもの（2 バイトのタグ）。
#[test]
fn an_mh_sdtt_reads_its_descriptors_as_mmt_ones() {
    let mut body = vec![0x00, 0x01, 0x7f, 0xe1, 0x04, 0x00, 0x01];
    // 記述 13 バイト = 予定 8 バイト + 記述子 5 バイト（タグ 0x8000、長さ 2）。
    body.extend_from_slice(&[0x11, 0x00, 0x10, 0x14, 0x00, 0xd0, 0x00, 0x80]);
    body.extend_from_slice(&[0xef, 0x66, 0x12, 0x00, 0x00, 0x01, 0x00, 0x00]);
    body.extend_from_slice(&[0x80, 0x00, 0x02, 0xab, 0xcd]);
    let table =
        mmttables::software_download(&long(mmttables::TABLE_MH_SDTT, 0x1234, &body)).expect("read");
    let content = &table.contents[0];
    assert!(content.descriptors.is_empty());
    assert_eq!(content.mh_descriptors.len(), 1);
    assert_eq!(content.mh_descriptors[0].tag, 0x8000);
    assert_eq!(content.mh_descriptors[0].body, [0xab, 0xcd]);
    assert_eq!(content.schedules.len(), 1);
}

/// PLT と LCT（セクションではないテーブル）。
#[test]
fn a_plt_and_an_lct_are_read() {
    let plt = [
        mmttables::TABLE_PLT,
        0x00,
        0x00,
        0x00,
        0x02,
        0x01,
        0xaa,
        0x00,
        0x01,
        0x23,
        0x01,
        0xbb,
        0x05,
        0x03,
        b'a',
        b'b',
        b'c',
        0x00,
    ];
    let read = mmttables::package_list(&plt).expect("read");
    assert_eq!(read.packages.len(), 2);
    assert_eq!(read.packages[0].package_id, [0xaa]);
    assert_eq!(
        read.packages[0].location,
        mmttables::PackageLocation::Here { packet_id: 0x0123 }
    );
    assert_eq!(
        read.packages[1].location,
        mmttables::PackageLocation::Url("abc".into())
    );
    let lct = [
        mmttables::TABLE_LCT,
        0x00,
        0x00,
        0x00,
        0x01,
        0x00,
        0x00,
        0x02,
        0x00,
        0,
        0,
        50,
        100,
        1,
        0x01,
        50,
        0,
        100,
        100,
        2,
    ];
    let layouts = mmttables::layout_configuration(&lct).expect("read");
    assert_eq!(layouts.len(), 1);
    assert_eq!(layouts[0].regions.len(), 2);
    assert_eq!(layouts[0].regions[1].left, 50);
    assert_eq!(layouts[0].regions[1].layer, 2);
}

#[test]
fn an_emm_common_message_is_read_and_composed() {
    let mut body = vec![
        0x12, // the broadcaster group
        0x01, // the viewer may not close it
        0x0a, 0x05, 0xff, // one minute, half a minute, then for good
        0x02, // twice
        0x01, // format 0x01
    ];
    let message = [0x41, 0xb0, 0xa2, 0x1a, 0xcd, 0xcd, 0x00, 0xff];
    body.extend_from_slice(&(message.len() as u16).to_be_bytes());
    body.extend_from_slice(&message);
    let read =
        tstables::common_message(&long(tstables::TABLE_EMM_MESSAGE, 0x0123, &body)).expect("read");
    assert_eq!(read.fixed_message_id, 0x0123);
    assert_eq!(read.broadcaster_group_id, 0x12);
    assert_eq!(read.deletion_status, 0x01);
    assert_eq!(read.durations, [0x0a, 0x05, 0xff]);
    assert_eq!(read.cycle, 2);
    assert_eq!(read.position(), Some(0x41));
    assert_eq!(read.text(), Some(&[0xb0, 0xa2, 0x1a, 0xcd, 0xcd][..]));
    assert_eq!(
        read.compose(&[0xc5, 0xc4]),
        Some(vec![0xb0, 0xa2, 0xc5, 0xc4, 0xcd, 0xcd])
    );
}

#[test]
fn the_messages_for_single_cards_are_not_common_ones() {
    let body = [0u8; 20];
    assert!(tstables::common_message(&long(tstables::TABLE_EMM_MESSAGE, 0x0000, &body)).is_none());
}

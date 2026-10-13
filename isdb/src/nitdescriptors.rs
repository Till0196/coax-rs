// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The rest of what ARIB STD-B10 Table 6-1 allows in a network information
//! table.
//!
//! The delivery system descriptors are in [`crate::nit`], beside the shapes
//! they fill in; these are the others, which say things about a multiplex
//! rather than where to find it. Every one reports nothing where the bytes ran
//! out rather than half a value.

/// The linkage descriptor (ARIB STD-B10 6.2.7, tag 0x4A), which points at
/// another service or transport stream. In a network table's first loop it
/// names a service attached to the network operator.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Linkage {
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    pub service_id: u16,
    /// What the link is for. See the standard's table; a scanner reports the
    /// number rather than naming a use it does not act on.
    pub linkage_type: u8,
    pub private_data: Vec<u8>,
}

/// Reads the linkage descriptor.
#[must_use]
pub fn linkage(body: &[u8]) -> Option<Linkage> {
    if body.len() < 7 {
        return None;
    }
    Some(Linkage {
        transport_stream_id: be16(&body[0..2]),
        original_network_id: be16(&body[2..4]),
        service_id: be16(&body[4..6]),
        linkage_type: body[6],
        private_data: body[7..].to_vec(),
    })
}

/// One service the emergency descriptor names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmergencyService {
    pub service_id: u16,
    /// The start_end_flag: true while the alarm has started or is being
    /// broadcast, false once it has ended.
    pub started: bool,
    /// Which of the two kinds of start signal this is.
    pub signal_level: u8,
    /// The areas the warning is for, twelve bits each.
    pub area_codes: Vec<u16>,
}

/// Reads the emergency information descriptor (ARIB STD-B10 6.2.24, tag 0xFC),
/// which carries the emergency warning signal that used to be sent as a tone.
/// It is mandatory while such a broadcast is being made, so its presence is the
/// signal.
#[must_use]
pub fn emergency_information(body: &[u8]) -> Vec<EmergencyService> {
    let mut services = Vec::new();
    let mut at = 0;
    while at + 4 <= body.len() {
        let mut service = EmergencyService {
            service_id: be16(&body[at..at + 2]),
            started: body[at + 2] & 0x80 != 0,
            signal_level: (body[at + 2] >> 6) & 0x01,
            area_codes: Vec::new(),
        };
        let codes = usize::from(body[at + 3]);
        at += 4;
        if at + codes > body.len() {
            // The area list runs past the end, so this service did not arrive
            // whole. What came before it stands; this is left off.
            break;
        }
        // Twelve bits of area code and four reserved, so two bytes each.
        for pair in body[at..at + codes].as_chunks::<2>().0 {
            service
                .area_codes
                .push(u16::from(pair[0]) << 4 | u16::from(pair[1] >> 4));
        }
        at += codes;
        services.push(service);
    }
    services
}

/// The connected transmission descriptor (ARIB STD-B10 6.2.41, tag 0xFD), which
/// says a terrestrial multiplex is one of a group sent together. Mandatory
/// where that is being done, so it is how a scan learns that two multiplexes
/// are halves of one thing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectedTransmission {
    /// Labels the group. Multiplexes sharing it are sent together.
    pub group_id: u16,
    /// How many layers there are and which this is.
    pub segment_type: u8,
    /// The three layers' modulations, as the descriptor numbers them.
    pub modulation_a: u8,
    pub modulation_b: u8,
    pub modulation_c: u8,
    pub extra: Vec<u8>,
}

/// Reads the connected transmission descriptor.
#[must_use]
pub fn connected_transmission(body: &[u8]) -> Option<ConnectedTransmission> {
    if body.len() < 3 {
        return None;
    }
    Some(ConnectedTransmission {
        group_id: be16(&body[0..2]),
        segment_type: body[2] >> 6,
        modulation_a: (body[2] >> 4) & 0x03,
        modulation_b: (body[2] >> 2) & 0x03,
        modulation_c: body[2] & 0x03,
        extra: body[3..].to_vec(),
    })
}

/// One primary service and the secondary it is grouped with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServiceGroupPair {
    pub primary_service_id: u16,
    pub secondary_service_id: u16,
}

/// The service group descriptor (ARIB STD-B10, tag 0xE0), which pairs services
/// that belong together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceGroup {
    /// What the grouping means. One is the only type with a shape this reads;
    /// anything else carries private data.
    pub group_type: u8,
    /// The grouped services, for group type one.
    pub pairs: Vec<ServiceGroupPair>,
    /// What a group of any other type carried instead.
    pub private_data: Vec<u8>,
}

/// Reads the service group descriptor.
#[must_use]
pub fn service_group(body: &[u8]) -> Option<ServiceGroup> {
    let first = *body.first()?;
    let mut group = ServiceGroup {
        group_type: first >> 4,
        ..ServiceGroup::default()
    };
    let rest = &body[1..];
    if group.group_type != 1 {
        group.private_data = rest.to_vec();
        return Some(group);
    }
    for pair in rest.as_chunks::<4>().0 {
        group.pairs.push(ServiceGroupPair {
            primary_service_id: be16(&pair[0..2]),
            secondary_service_id: be16(&pair[2..4]),
        });
    }
    Some(group)
}

/// One transmitting point of an area broadcast.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AreaBroadcastingStation {
    /// Twenty-four bits; the standard reserves everything above 0x3FFFFF.
    pub station_id: u32,
    pub location_code: u16,
    /// Which transmission format the station uses.
    pub signal_format: u8,
    pub extra: Vec<u8>,
}

/// Reads the area broadcasting information descriptor (ARIB STD-B10, tag
/// 0xE1), which names the transmitting stations of an area broadcast -- the
/// small local transmissions, as against a network's own.
#[must_use]
pub fn area_broadcasting(body: &[u8]) -> Vec<AreaBroadcastingStation> {
    let Some(&count) = body.first() else {
        return Vec::new();
    };
    let mut stations = Vec::new();
    // The count is written out, but the loop is bounded by the bytes: a count
    // larger than what arrived is a descriptor that was cut short.
    let mut at = 1;
    for _ in 0..count {
        if at + 7 > body.len() {
            break;
        }
        let mut station = AreaBroadcastingStation {
            station_id: u32::from(body[at]) << 16
                | u32::from(body[at + 1]) << 8
                | u32::from(body[at + 2]),
            location_code: be16(&body[at + 3..at + 5]),
            signal_format: body[at + 5],
            extra: Vec::new(),
        };
        let extra = usize::from(body[at + 6]);
        at += 7;
        if at + extra > body.len() {
            break;
        }
        station.extra = body[at..at + extra].to_vec();
        at += extra;
        stations.push(station);
    }
    stations
}

fn be16(bytes: &[u8]) -> u16 {
    u16::from(bytes[0]) << 8 | u16::from(bytes[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each against bytes laid out as the standard writes them, and each against
    // a body that ran out part way. The daemon's own readers are checked
    // against the same bytes.

    #[test]
    fn linkage_points_at_a_service() {
        let got = linkage(&[0x40, 0x11, 0x00, 0x04, 0x04, 0x00, 0x01, 0xaa]).expect("a link");
        assert_eq!(got.transport_stream_id, 0x4011);
        assert_eq!(got.original_network_id, 4);
        assert_eq!(got.service_id, 0x0400);
        assert_eq!(got.linkage_type, 1);
        assert_eq!(got.private_data, vec![0xaa]);
        assert!(linkage(&[0x40, 0x11, 0x00, 0x04, 0x04, 0x00]).is_none());
    }

    /// Its presence is the signal: the descriptor is sent only while an
    /// emergency alarm broadcast is being made.
    #[test]
    fn emergency_information_names_the_areas() {
        let got = emergency_information(&[0x04, 0x00, 0x80, 0x04, 0x01, 0x20, 0x03, 0x40]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].service_id, 0x0400);
        assert!(got[0].started);
        assert_eq!(got[0].signal_level, 0);
        assert_eq!(got[0].area_codes, vec![0x012, 0x034]);

        // The end of the alarm is the same descriptor with the flag cleared.
        let ended = emergency_information(&[0x04, 0x00, 0x00, 0x00]);
        assert_eq!(ended.len(), 1);
        assert!(!ended[0].started);

        // An area list longer than what arrived is a descriptor cut short.
        assert!(emergency_information(&[0x04, 0x00, 0x80, 0x08, 0x01]).is_empty());
        assert!(emergency_information(&[]).is_empty());
    }

    /// The advanced services send the same signal in the MPT under a two-byte
    /// tag (STD-B60 Table 7-36): only the tag differs, so the body reads alike.
    #[test]
    fn emergency_information_reads_the_mh_descriptor_alike() {
        let loop_ = [
            0x80, 0x07, 0x06, 0x00, 0x65, 0x40, 0x02, 0x5a, 0x50, // the MH descriptor
            0x80, 0x40, 0x00, // something else after it
        ];
        let described = crate::mmttables::descriptors(&loop_);
        let found = crate::mmttables::find(
            &described,
            crate::descriptors::mh_tag::EMERGENCY_INFORMATION,
        )
        .expect("the descriptor");
        let got = emergency_information(&found.body);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].service_id, 0x0065);
        // 0x40: the alarm has ended (start_end_flag 0), and was of the second kind.
        assert!(!got[0].started);
        assert_eq!(got[0].signal_level, 1);
        assert_eq!(got[0].area_codes, vec![0x5a5]);
    }

    /// Two multiplexes sharing a group are halves of one thing, which is what
    /// a scan cannot work out from either of them alone.
    #[test]
    fn connected_transmission_names_its_group() {
        let got = connected_transmission(&[0x12, 0x34, 0x6c, 0xff]).expect("a group");
        // 0x6c is 01 10 11 00: segment type 1, and the three layers 2, 3, 0.
        assert_eq!(got.group_id, 0x1234);
        assert_eq!(got.segment_type, 1);
        assert_eq!(
            (got.modulation_a, got.modulation_b, got.modulation_c),
            (2, 3, 0)
        );
        assert_eq!(got.extra, vec![0xff]);
        assert!(connected_transmission(&[0x12, 0x34]).is_none());
    }

    #[test]
    fn service_group_pairs_services() {
        let got = service_group(&[0x10, 0x04, 0x00, 0x04, 0x01]).expect("a group");
        assert_eq!(got.group_type, 1);
        assert_eq!(got.pairs.len(), 1);
        assert_eq!(got.pairs[0].primary_service_id, 0x0400);
        assert_eq!(got.pairs[0].secondary_service_id, 0x0401);

        // Any other type carries private data, which is kept rather than read
        // as pairs it is not.
        let other = service_group(&[0x20, 0xde, 0xad]).expect("a group");
        assert_eq!(other.group_type, 2);
        assert!(other.pairs.is_empty());
        assert_eq!(other.private_data, vec![0xde, 0xad]);

        assert!(service_group(&[]).is_none());
    }

    #[test]
    fn area_broadcasting_names_its_stations() {
        let got = area_broadcasting(&[0x01, 0x00, 0x12, 0x34, 0x56, 0x78, 0x02, 0x01, 0x99]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].station_id, 0x0000_1234);
        assert_eq!(got[0].location_code, 0x5678);
        assert_eq!(got[0].signal_format, 2);
        assert_eq!(got[0].extra, vec![0x99]);

        // A count larger than what arrived is a descriptor cut short: what did
        // arrive stands, and nothing is invented for the rest.
        assert!(area_broadcasting(&[0x02, 0x00, 0x12]).is_empty());
        assert!(area_broadcasting(&[]).is_empty());
    }
}

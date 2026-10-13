// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! MMT signalling messages, which arrive fragmented across packets or several
//! to a packet, and have to be put back together before they parse.

use std::collections::BTreeMap;

use crate::{be16, be32};

/// One signalling message, put back together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: u16,
    pub version: u8,
    pub payload: Vec<u8>,
}

/// How much of one message to hold while the rest of it arrives. A message
/// this long is a stream that has gone wrong rather than a table.
const MOST_HELD: usize = 4 << 20;

/// Puts the signalling messages back together. A message may be split across
/// packets, and a packet may carry several messages. What is held is kept per
/// packet identifier: two identifiers are two senders, and their fragments
/// interleave.
#[derive(Debug, Default)]
pub struct Signalling {
    held: BTreeMap<u16, Vec<u8>>,
}

impl Signalling {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes one MMTP payload and gives back whatever it completed.
    pub fn push(&mut self, packet_id: u16, payload: &[u8]) -> Vec<Message> {
        if payload.len() < 2 {
            return Vec::new();
        }
        let fragment = payload[0] >> 6;
        let aggregated = payload[0] & 1 != 0;
        let long_lengths = payload[0] & 2 != 0;
        let body = &payload[2..];
        if aggregated {
            // Several messages in one packet, which is not something a
            // fragment can also be.
            if fragment != 0 {
                return Vec::new();
            }
            return aggregate(body, long_lengths);
        }
        match fragment {
            // Not a fragment at all.
            0 => {
                self.held.remove(&packet_id);
                message(body).into_iter().collect()
            }
            1 => {
                self.held.insert(packet_id, body.to_vec());
                Vec::new()
            }
            2 => {
                if let Some(held) = self.held.get_mut(&packet_id)
                    && held.len() + body.len() <= MOST_HELD
                {
                    held.extend_from_slice(body);
                }
                Vec::new()
            }
            _ => match self.held.remove(&packet_id) {
                Some(mut held) => {
                    held.extend_from_slice(body);
                    message(&held).into_iter().collect()
                }
                None => Vec::new(),
            },
        }
    }
}

/// The messages of a packet that carried several.
fn aggregate(mut buffer: &[u8], long_lengths: bool) -> Vec<Message> {
    let width = if long_lengths { 4 } else { 2 };
    let mut messages = Vec::new();
    while buffer.len() >= width {
        let length = match width {
            4 => be32(buffer) as usize,
            _ => usize::from(be16(buffer)),
        };
        buffer = &buffer[width..];
        if length > buffer.len() {
            break;
        }
        if let Some(message) = message(&buffer[..length]) {
            messages.push(message);
        }
        buffer = &buffer[length..];
    }
    messages
}

/// One message: its identifier, its version, and the length of what follows.
/// The length is two bytes for most of them and four for the ones that may be
/// longer than a packet -- which is the message's own to say, so it is read
/// from the identifier.
fn message(buffer: &[u8]) -> Option<Message> {
    if buffer.len() < 5 {
        return None;
    }
    let id = be16(buffer);
    let long = id == 0 || id == 0x8003 || id >= 0xf000 || (0x7000..=0x7fff).contains(&id);
    let header = if long { 7 } else { 5 };
    if buffer.len() < header {
        return None;
    }
    let length = if long {
        be32(&buffer[3..]) as usize
    } else {
        usize::from(be16(&buffer[3..]))
    };
    if header + length > buffer.len() {
        return None;
    }
    Some(Message {
        id,
        version: buffer[2],
        payload: buffer[header..header + length].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::Signalling;

    #[test]
    fn a_message_split_across_packets_is_put_back_together() {
        // The message: identifier, version, length, and a body of four bytes.
        let mut whole = vec![0x00, 0x10, 0x01];
        whole.extend_from_slice(&4u16.to_be_bytes());
        whole.extend_from_slice(&[1, 2, 3, 4]);
        let mut signalling = Signalling::new();
        let mut first = vec![0x40, 0x00];
        first.extend_from_slice(&whole[..3]);
        assert!(signalling.push(9, &first).is_empty());
        let mut last = vec![0xc0, 0x00];
        last.extend_from_slice(&whole[3..]);
        let messages = signalling.push(9, &last);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, 0x0010);
        assert_eq!(messages[0].payload, vec![1, 2, 3, 4]);
    }
}

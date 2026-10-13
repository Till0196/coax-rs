// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The readers of a daemon, reached over its HTTP ([`crate::card`]).

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::server::{Card, Cards};
use super::table::{Slot, Slots};
use super::wire::{Code, PROTOCOL_T1, SHARE_EXCLUSIVE, SHARE_SHARED, attribute};
use crate::card::{self, Held, Refusal};

/// How long one request for a card's events waits at the daemon. Short enough
/// that a slot nobody watches any more lets go soon after.
const WATCH_WAIT: u32 = 10;

/// How long an open card's connection may sit on one exchange.
const EXCHANGE_WAIT: u32 = 30;

/// How long to wait before asking again after the daemon did not answer.
const RETRY_AFTER: Duration = Duration::from_secs(1);

/// A daemon's slots as readers.
#[derive(Debug, Clone)]
pub struct DaemonCards {
    cards: card::Cards,
}

impl DaemonCards {
    pub fn new(cards: card::Cards) -> Self {
        Self { cards }
    }
}

/// What the daemon reported, as the status code a client library can read
/// rather than an HTTP failure it has no way to.
fn translate(refusal: &Refusal) -> Code {
    match refusal {
        Refusal::Refused { code, message, .. } => match code.as_str() {
            "unknown_tuner" => Code::UNKNOWN_READER,
            "invalid_parameter" => Code::INVALID_PARAMETER,
            "no_card_reader" => Code::NO_SMARTCARD,
            "card_error" if message.contains("not holding a card") => Code::INVALID_HANDLE,
            _ => Code::NOT_TRANSACTED,
        },
        // Nothing answered, so from the caller's side it is the resource
        // manager that is unreachable rather than the card.
        Refusal::Broken(_) => Code::NO_SERVICE,
    }
}

/// What the daemon's query parameter calls a disposition.
fn disposition_name(disposition: u32) -> &'static str {
    match disposition {
        1 => "reset",
        2 => "unpower",
        3 => "eject",
        _ => "leave",
    }
}

impl Cards for DaemonCards {
    fn readers(&self) -> Result<Vec<String>, Code> {
        let listed = self.cards.list().map_err(|refusal| translate(&refusal))?;
        Ok(listed.into_iter().map(|slot| slot.reader).collect())
    }

    fn connect(
        &self,
        reader: &str,
        share: u32,
        protocols: u32,
    ) -> Result<(Box<dyn Card>, u32), Code> {
        if !matches!(share, SHARE_SHARED | SHARE_EXCLUSIVE) {
            // No reader-level channel to connect to without a card.
            return Err(Code::INVALID_VALUE);
        }
        if protocols & PROTOCOL_T1 == 0 {
            return Err(Code::PROTO_MISMATCH);
        }
        let held = self
            .cards
            .open(reader, EXCHANGE_WAIT)
            .map_err(|refusal| translate(&refusal))?;
        Ok((
            Box::new(DaemonCard {
                cards: self.cards.clone(),
                reader: reader.to_owned(),
                held: Some(held),
            }),
            PROTOCOL_T1,
        ))
    }
}

/// One open card.
#[derive(Debug)]
struct DaemonCard {
    cards: card::Cards,
    reader: String,
    held: Option<Held>,
}

impl DaemonCard {
    fn held(&mut self) -> Result<&mut Held, Code> {
        self.held.as_mut().ok_or(Code::INVALID_HANDLE)
    }
}

impl Card for DaemonCard {
    fn reconnect(&mut self, share: u32, protocols: u32, initialization: u32) -> Result<u32, Code> {
        if !matches!(share, SHARE_SHARED | SHARE_EXCLUSIVE) {
            return Err(Code::INVALID_VALUE);
        }
        if protocols & PROTOCOL_T1 == 0 {
            return Err(Code::PROTO_MISMATCH);
        }
        let mut old = self.held.take().ok_or(Code::INVALID_HANDLE)?;
        let _ = old.close(disposition_name(initialization));
        let held = self
            .cards
            .open(&self.reader, EXCHANGE_WAIT)
            .map_err(|refusal| translate(&refusal))?;
        self.held = Some(held);
        Ok(PROTOCOL_T1)
    }

    fn disconnect(&mut self, disposition: u32) -> Result<(), Code> {
        let mut held = self.held.take().ok_or(Code::INVALID_HANDLE)?;
        held.close(disposition_name(disposition))
            .map_err(|refusal| translate(&refusal))
    }

    fn begin_transaction(&mut self) -> Result<(), Code> {
        self.held()?
            .begin_transaction()
            .map_err(|refusal| translate(&refusal))
    }

    fn end_transaction(&mut self, disposition: u32) -> Result<(), Code> {
        self.held()?
            .end_transaction(disposition_name(disposition))
            .map_err(|refusal| translate(&refusal))
    }

    fn status(&mut self) -> Result<(bool, Vec<u8>), Code> {
        let atr = self.held()?.atr().to_vec();
        let present = self
            .cards
            .events(&self.reader, 0, 0)
            .map_or(true, |seen| seen.present);
        Ok((present, atr))
    }

    fn transmit(&mut self, protocol: u32, send: &[u8]) -> Result<(Vec<u8>, u32), Code> {
        if protocol != PROTOCOL_T1 {
            return Err(Code::INVALID_PARAMETER);
        }
        let held = self.held()?;
        if held.max_input() > 0 && send.len() > held.max_input() {
            return Err(Code::INVALID_PARAMETER);
        }
        let response = held.transmit(send).map_err(|refusal| translate(&refusal))?;
        Ok((response, PROTOCOL_T1))
    }

    fn control(&mut self, _code: u32, _send: &[u8]) -> Result<Vec<u8>, Code> {
        Err(Code::UNSUPPORTED_FEATURE)
    }

    fn get_attrib(&mut self, id: u32) -> Result<Vec<u8>, Code> {
        let reader = self.reader.clone();
        let held = self.held()?;
        match id {
            attribute::ATR_STRING => Ok(held.atr().to_vec()),
            attribute::CURRENT_PROTOCOL => Ok(PROTOCOL_T1.to_le_bytes().to_vec()),
            attribute::MAX_INPUT if held.max_input() > 0 => {
                Ok((held.max_input() as u32).to_le_bytes().to_vec())
            }
            attribute::VENDOR_NAME => Ok(text("coax")),
            attribute::VENDOR_IFD_TYPE => Ok(text("coaxd")),
            attribute::DEVICE_FRIENDLY_NAME_A | attribute::DEVICE_SYSTEM_NAME_A => {
                Ok(text(&reader))
            }
            attribute::DEVICE_FRIENDLY_NAME_W | attribute::DEVICE_SYSTEM_NAME_W => {
                Ok(wide(&reader))
            }
            _ => Err(Code::UNSUPPORTED_FEATURE),
        }
    }

    fn set_attrib(&mut self, _id: u32, _value: &[u8]) -> Result<(), Code> {
        Err(Code::UNSUPPORTED_FEATURE)
    }
}

/// The A attributes: a C string.
fn text(value: &str) -> Vec<u8> {
    let mut encoded = value.as_bytes().to_vec();
    encoded.push(0);
    encoded
}

/// The W attributes: UTF-16LE, terminated.
fn wide(value: &str) -> Vec<u8> {
    let mut encoded: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
    encoded.extend_from_slice(&[0, 0]);
    encoded
}

impl Slots for DaemonCards {
    fn readers(&self) -> io::Result<Vec<String>> {
        let listed = self
            .cards
            .list()
            .map_err(|refusal| io::Error::other(refusal.to_string()))?;
        Ok(listed.into_iter().map(|slot| slot.reader).collect())
    }

    /// Watches a slot by waiting at the daemon for its events, which is next to
    /// the hardware and where the polling belongs.
    fn open(&self, reader: &str) -> io::Result<Box<dyn Slot>> {
        let first = self
            .cards
            .events(reader, 0, 0)
            .map_err(|refusal| io::Error::other(refusal.to_string()))?;
        let watched = Arc::new(Watched {
            seen: Mutex::new(first),
            stop: AtomicBool::new(false),
        });
        let cards = self.cards.clone();
        let reader = reader.to_owned();
        let watcher = Arc::clone(&watched);
        std::thread::Builder::new()
            .name(String::from("pcscd-slot"))
            .spawn(move || {
                while !watcher.stop.load(Ordering::Relaxed) {
                    let since = watcher
                        .seen
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .events;
                    match cards.events(&reader, since, WATCH_WAIT) {
                        Ok(seen) => *watcher.seen.lock().unwrap_or_else(|e| e.into_inner()) = seen,
                        Err(_) => std::thread::sleep(RETRY_AFTER),
                    }
                }
            })?;
        Ok(Box::new(WatchedSlot(watched)))
    }
}

#[derive(Debug)]
struct Watched {
    seen: Mutex<card::SlotState>,
    stop: AtomicBool,
}

#[derive(Debug)]
struct WatchedSlot(Arc<Watched>);

impl Slot for WatchedSlot {
    fn present(&self) -> bool {
        self.0
            .seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .present
    }

    fn atr(&self) -> Vec<u8> {
        self.0
            .seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .atr
            .clone()
    }

    fn sharing(&self) -> i32 {
        self.0
            .seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sharing
    }
}

impl Drop for WatchedSlot {
    fn drop(&mut self) {
        // The thread notices after its current wait at the daemon.
        self.0.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daemons_refusals_become_status_codes() {
        let refused = |code: &str, message: &str| Refusal::Refused {
            status: 400,
            code: code.to_owned(),
            message: message.to_owned(),
        };
        assert_eq!(
            translate(&refused("unknown_tuner", "")),
            Code::UNKNOWN_READER
        );
        assert_eq!(
            translate(&refused("no_card_reader", "")),
            Code::NO_SMARTCARD
        );
        assert_eq!(
            translate(&refused(
                "card_error",
                "this connection is not holding a card"
            )),
            Code::INVALID_HANDLE
        );
        assert_eq!(
            translate(&refused("card_error", "timeout")),
            Code::NOT_TRANSACTED
        );
        assert_eq!(
            translate(&Refusal::Broken(io::Error::other("gone"))),
            Code::NO_SERVICE
        );
    }

    #[test]
    fn names_are_given_as_both_kinds_of_string() {
        assert_eq!(text("ab"), b"ab\0");
        assert_eq!(wide("ab"), [b'a', 0, b'b', 0, 0, 0]);
    }
}

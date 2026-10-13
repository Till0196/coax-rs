// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The reader array the socket publishes.
//!
//! A client library answers `SCardListReaders`, `SCardStatus` and
//! `SCardGetStatusChange` from its own copy of this rather than asking, so it
//! is the only way of saying anything about a slot.
//!
//! Whether a card is seated is measured only while a client is waiting for an
//! event, and otherwise the last measurement stands: looking can mean opening a
//! board a recording streams from. Measuring stops a little while after the
//! last waiter goes, rather than the moment it does, because a client that has
//! just been woken asks again straight away.

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::wire::{
    self, MAX_READERS, PROTOCOL_T1, ReaderState, STATE_ABSENT, STATE_NEGOTIABLE, STATE_POWERED,
    STATE_PRESENT, STATE_SPECIFIC, STATE_UNKNOWN,
};

/// The hardware the table publishes, as a trait so the protocol can be tested
/// against something other than a tuner.
pub trait Slots: Send + Sync {
    /// The slots, by the name they are published under. Nothing is opened.
    fn readers(&self) -> io::Result<Vec<String>>;
    /// Starts watching one slot. Dropping it stops.
    fn open(&self, reader: &str) -> io::Result<Box<dyn Slot>>;
}

/// One slot being watched.
pub trait Slot: Send + Sync {
    fn present(&self) -> bool;
    fn atr(&self) -> Vec<u8>;
    /// How many connections hold the card, or -1 for an exclusive one.
    fn sharing(&self) -> i32;
}

/// How often a watched slot is looked at. Looking oftener than the state can
/// change gains nothing.
pub const POLL_INTERVAL: Duration = Duration::from_millis(400);

/// How long measuring goes on after the last waiter.
const LINGER: Duration = Duration::from_secs(5);

/// What to call when something moves, with the result to wake waiters with.
pub type Notify = Arc<dyn Fn(u32) + Send + Sync>;

/// The reader array. Cheap to clone; the clones are the same table.
#[derive(Clone)]
pub struct Table {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Table {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Table")
            .field("readers", &self.lock().names)
            .finish()
    }
}

struct Inner {
    slots: Box<dyn Slots>,
    interval: Duration,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    names: Vec<String>,
    cards: HashMap<String, Entry>,
    /// Times the set of readers changed, which is what the PnP pseudo-reader
    /// reports.
    events: u32,
    /// Clients waiting for an event.
    watchers: usize,
    /// Set while the measuring thread runs; setting the flag stops it.
    measuring: Option<Arc<AtomicBool>>,
    notify: Option<Notify>,
}

#[derive(Debug, Default)]
struct Entry {
    present: bool,
    measured: bool,
    atr: Vec<u8>,
    events: u32,
    sharing: i32,
}

impl Table {
    pub fn new(slots: Box<dyn Slots>, interval: Duration) -> Self {
        let table = Self {
            inner: Arc::new(Inner {
                slots,
                interval: if interval.is_zero() {
                    POLL_INTERVAL
                } else {
                    interval
                },
                state: Mutex::new(State::default()),
            }),
        };
        table.refresh();
        table
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Sets what to call when something moves. Separate from [`Table::new`]
    /// because the server and the table refer to each other.
    pub fn on_event(&self, notify: Notify) {
        self.lock().notify = Some(notify);
    }

    fn notify(&self) {
        let notify = self.lock().notify.clone();
        if let Some(notify) = notify {
            notify(wire::Code::SUCCESS.0);
        }
    }

    /// Re-enumerates the readers. Nothing is opened: the list comes from what
    /// the hardware is, not from looking at it.
    pub fn refresh(&self) {
        let mut names = self.inner.slots.readers().unwrap_or_default();
        names.truncate(MAX_READERS);
        let changed = {
            let mut state = self.lock();
            let changed = names != state.names;
            for name in &names {
                state.cards.entry(name.clone()).or_default();
            }
            state.cards.retain(|name, _| names.contains(name));
            state.names = names;
            if changed {
                state.events = state.events.wrapping_add(1);
            }
            changed
        };
        if changed {
            self.notify();
        }
    }

    /// Records what a card connection just saw, so that a client asking about a
    /// reader it is using gets an answer measured rather than remembered.
    pub fn observed(&self, reader: &str, present: bool, atr: &[u8]) {
        let mut state = self.lock();
        let Some(known) = state.cards.get_mut(reader) else {
            return;
        };
        if known.measured && known.present != present {
            known.events = known.events.wrapping_add(1);
        }
        known.measured = true;
        known.present = present;
        known.atr = atr.to_vec();
    }

    pub fn events(&self) -> u32 {
        self.lock().events
    }

    pub fn names(&self) -> Vec<String> {
        self.lock().names.clone()
    }

    /// The array a client reads.
    pub fn states(&self) -> Vec<ReaderState> {
        let state = self.lock();
        state
            .names
            .iter()
            .map(|name| {
                let known = &state.cards[name];
                let (flags, atr) = if known.present {
                    // Powered and specific follow presence because a card these
                    // interfaces can see at all has been through its answer to
                    // reset: there is no state in between.
                    (
                        STATE_PRESENT | STATE_POWERED | STATE_NEGOTIABLE | STATE_SPECIFIC,
                        known.atr.clone(),
                    )
                } else if known.measured {
                    (STATE_ABSENT, Vec::new())
                } else {
                    // Nothing has looked yet.
                    (STATE_UNKNOWN, Vec::new())
                };
                ReaderState {
                    name: name.clone(),
                    events: known.events,
                    state: flags,
                    sharing: known.sharing,
                    atr,
                    protocol: PROTOCOL_T1,
                }
            })
            .collect()
    }

    /// A client is waiting, which is what makes the table measure.
    pub fn watch(&self) {
        let start = {
            let mut state = self.lock();
            state.watchers += 1;
            match &state.measuring {
                Some(_) => None,
                None => {
                    let stop = Arc::new(AtomicBool::new(false));
                    state.measuring = Some(Arc::clone(&stop));
                    Some(stop)
                }
            }
        };
        if let Some(stop) = start {
            let table = self.clone();
            let spawned = std::thread::Builder::new()
                .name(String::from("pcscd-measure"))
                .spawn(move || table.measure(&stop));
            if spawned.is_err() {
                self.lock().measuring = None;
            }
        }
    }

    /// One waiting client stopped waiting.
    pub fn unwatch(&self) {
        let mut state = self.lock();
        state.watchers = state.watchers.saturating_sub(1);
    }

    /// Stops measuring for good.
    pub fn close(&self) {
        if let Some(stop) = self.lock().measuring.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    /// Looks at every slot while anything waits, keeping each one open for the
    /// whole time rather than per look: opening is what powers a board up.
    fn measure(&self, stop: &AtomicBool) {
        let mut held: HashMap<String, Box<dyn Slot>> = HashMap::new();
        let mut idle_since: Option<Instant> = None;
        while !stop.load(Ordering::Relaxed) {
            if self.sweep(&mut held) {
                self.notify();
            }
            std::thread::sleep(self.inner.interval);
            let mut state = self.lock();
            if state.watchers > 0 {
                idle_since = None;
                continue;
            }
            let since = *idle_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= LINGER {
                // Given up under the lock, so a watcher arriving now starts a
                // new thread rather than relying on this one.
                state.measuring = None;
                break;
            }
        }
    }

    /// Measures every reader once and reports whether anything moved.
    fn sweep(&self, held: &mut HashMap<String, Box<dyn Slot>>) -> bool {
        let names = self.names();
        held.retain(|name, _| names.contains(name));
        let mut moved = false;
        for name in names {
            if !held.contains_key(&name) {
                match self.inner.slots.open(&name) {
                    Ok(slot) => {
                        held.insert(name.clone(), slot);
                    }
                    Err(_) => continue,
                }
            }
            let slot = &held[&name];
            let present = slot.present();
            let atr = slot.atr();
            let sharing = slot.sharing();
            let mut state = self.lock();
            let Some(known) = state.cards.get_mut(&name) else {
                continue;
            };
            if !known.measured || known.present != present {
                known.events = known.events.wrapping_add(1);
                moved = true;
            }
            if known.sharing != sharing {
                moved = true;
            }
            known.measured = true;
            known.present = present;
            known.sharing = sharing;
            known.atr = if present { atr } else { Vec::new() };
        }
        moved
    }
}

// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Answers pcsc-lite's client library on a Unix socket.
//!
//! It is not a general resource manager: it serves one set of [`Cards`] and
//! nothing else. An application with an external reader keeps using the real
//! daemon for it by being pointed at the other socket; the two never have to
//! agree, because a client library talks to exactly one.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::table::Table;
use super::wire::{
    self, Code, EJECT_CARD, Header, LEAVE_CARD, MAX_ATTRIBUTE, MAX_READER_NAME, MAX_READERS,
    MAX_TRANSMIT, OLD_READER_STATE, PROTOCOL_UNDEFINED, READER_STATE_LEN, SCOPE_GLOBAL,
    SHARE_DIRECT, SHARE_EXCLUSIVE, SHARE_SHARED, command,
};
use crate::daemon::unix_socket::{Listener, Socket};

/// Where readers come from.
pub trait Cards: Send + Sync {
    /// The readers, by name.
    fn readers(&self) -> Result<Vec<String>, Code>;
    /// Opens a card in one of them and reports the agreed protocol. The share
    /// mode, protocols and reader have been checked already.
    fn connect(
        &self,
        reader: &str,
        share: u32,
        protocols: u32,
    ) -> Result<(Box<dyn Card>, u32), Code>;
}

/// One connection to one card, not the card itself.
pub trait Card: Send {
    fn reconnect(&mut self, share: u32, protocols: u32, initialization: u32) -> Result<u32, Code>;
    fn disconnect(&mut self, disposition: u32) -> Result<(), Code>;
    fn begin_transaction(&mut self) -> Result<(), Code>;
    fn end_transaction(&mut self, disposition: u32) -> Result<(), Code>;
    /// Whether a card is seated, and its answer to reset.
    fn status(&mut self) -> Result<(bool, Vec<u8>), Code>;
    /// One command APDU; the response with its status bytes, and the protocol
    /// it came back under.
    fn transmit(&mut self, protocol: u32, send: &[u8]) -> Result<(Vec<u8>, u32), Code>;
    fn control(&mut self, code: u32, send: &[u8]) -> Result<Vec<u8>, Code>;
    fn get_attrib(&mut self, id: u32) -> Result<Vec<u8>, Code>;
    fn set_attrib(&mut self, id: u32, value: &[u8]) -> Result<(), Code>;
}

/// The socket's side of every client.
#[derive(Clone)]
pub struct Server {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Server")
            .field("connections", &self.connections())
            .finish()
    }
}

struct Inner {
    cards: Box<dyn Cards>,
    table: Table,
    report: Box<dyn Fn(&str) + Send + Sync>,
    shared: Mutex<Shared>,
    next_peer: AtomicU64,
}

#[derive(Default)]
struct Shared {
    /// The identifier a client knows a context by, to the connection that owns
    /// it, so that a cancel arriving on another connection finds the waiter.
    contexts: HashMap<u32, Arc<Peer>>,
    /// Connections waiting for an event, by their own number.
    waiters: HashMap<u64, Arc<Peer>>,
    next_handle: u32,
    connections: usize,
}

/// What other threads need of a connection: a way to write to it.
struct Peer {
    id: u64,
    socket: Arc<Socket>,
    /// Replies are serialised, because a waiting client is written to by
    /// whichever thread noticed the event as well as by its own.
    writing: Mutex<()>,
}

impl Peer {
    fn reply(&self, raw: &[u8]) -> io::Result<()> {
        let _writing = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        (&*self.socket).write_all(raw)
    }

    /// Wakes a waiting client: the same eight bytes the wait is answered with.
    fn signal(&self, result: u32) {
        let mut raw = [0u8; wire::SIZE_WAIT];
        wire::put(&mut raw, 1, result);
        let _ = self.reply(&raw);
    }
}

impl Server {
    pub fn new(
        cards: Box<dyn Cards>,
        table: Table,
        report: Box<dyn Fn(&str) + Send + Sync>,
    ) -> Self {
        let server = Self {
            inner: Arc::new(Inner {
                cards,
                table: table.clone(),
                report,
                shared: Mutex::new(Shared::default()),
                next_peer: AtomicU64::new(1),
            }),
        };
        // The table outlives nothing the server does, and holding the server
        // weakly keeps the two from keeping each other alive.
        let weak = Arc::downgrade(&server.inner);
        table.on_event(Arc::new(move |result| {
            if let Some(inner) = weak.upgrade() {
                (Self { inner }).notify(result);
            }
        }));
        server
    }

    fn shared(&self) -> MutexGuard<'_, Shared> {
        self.inner.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Accepts clients until the listener fails.
    pub fn serve(&self, listener: &Listener) -> io::Result<()> {
        loop {
            let socket = listener.accept()?;
            let server = self.clone();
            self.shared().connections += 1;
            let spawned = std::thread::Builder::new()
                .name(String::from("pcscd-client"))
                .spawn(move || server.client(socket));
            if spawned.is_err() {
                self.shared().connections -= 1;
            }
        }
    }

    /// How many clients are attached. A host that stops when idle must still
    /// see a client sitting in `SCardGetStatusChange` that has said nothing in
    /// minutes, which counting requests would miss.
    pub fn connections(&self) -> usize {
        self.shared().connections
    }

    /// Tells every waiting client that something moved. A waiter is taken off
    /// the list as it is signalled, as pcscd does: the client asks again once
    /// it has read the new state.
    pub fn notify(&self, result: u32) {
        let waiting: Vec<Arc<Peer>> = self
            .shared()
            .waiters
            .drain()
            .map(|(_, peer)| peer)
            .collect();
        for peer in waiting {
            self.inner.table.unwatch();
            peer.signal(result);
        }
    }

    fn next_handle(&self) -> u32 {
        let mut shared = self.shared();
        shared.next_handle = shared.next_handle.wrapping_add(1).max(1);
        shared.next_handle
    }

    fn client(&self, socket: Socket) {
        let socket = Arc::new(socket);
        let peer = Arc::new(Peer {
            id: self.inner.next_peer.fetch_add(1, Ordering::Relaxed),
            socket: Arc::clone(&socket),
            writing: Mutex::new(()),
        });
        let mut connection = Connection {
            server: self.clone(),
            socket,
            peer: Arc::clone(&peer),
            contexts: Vec::new(),
            cards: HashMap::new(),
            old_api: false,
        };
        while let Ok(request) = wire::read_header(&mut &*connection.socket) {
            if let Err(reason) = connection.dispatch(request) {
                if reason.kind() != io::ErrorKind::UnexpectedEof {
                    (self.inner.report)(&format!("client dropped: {reason}"));
                }
                break;
            }
        }
        connection.close();
    }
}

/// One client, served by its own thread.
struct Connection {
    server: Server,
    socket: Arc<Socket>,
    peer: Arc<Peer>,
    contexts: Vec<u32>,
    /// Each card with the context and the reader it was opened in.
    cards: HashMap<i32, Opened>,
    /// A client of a protocol minor before the reader array became
    /// variable-sized, which expects the array after registering to wait.
    old_api: bool,
}

struct Opened {
    context: u32,
    reader: String,
    card: Box<dyn Card>,
}

impl Connection {
    fn reply(&self, raw: &[u8]) -> io::Result<()> {
        self.peer.reply(raw)
    }

    fn table(&self) -> &Table {
        &self.server.inner.table
    }

    fn close(&mut self) {
        {
            let mut shared = self.server.shared();
            if shared.waiters.remove(&self.peer.id).is_some() {
                self.table().unwatch();
            }
            shared.contexts.retain(|_, owner| owner.id != self.peer.id);
            shared.connections = shared.connections.saturating_sub(1);
        }
        for (_, mut opened) in self.cards.drain() {
            let _ = opened.card.disconnect(LEAVE_CARD);
        }
        self.contexts.clear();
        self.socket.shutdown();
        self.table().refresh();
    }

    fn body(&mut self, request: Header, expected: usize) -> io::Result<Vec<u8>> {
        wire::body(&mut &*self.socket, request.size, expected)
    }

    fn dispatch(&mut self, request: Header) -> io::Result<()> {
        match request.command {
            command::VERSION => self.version(request),
            command::ESTABLISH_CONTEXT => self.establish_context(request),
            command::RELEASE_CONTEXT => self.release_context(request),
            command::CONNECT => self.connect(request),
            command::RECONNECT => self.reconnect(request),
            command::DISCONNECT => self.disconnect(request),
            command::BEGIN_TRANSACTION => self.begin_transaction(request),
            command::END_TRANSACTION => self.end_transaction(request),
            command::STATUS => self.status(request),
            command::TRANSMIT => self.transmit(request),
            command::CONTROL => self.control(request),
            command::GET_ATTRIB | command::SET_ATTRIB => self.attribute(request),
            command::CANCEL => self.cancel(request),
            command::GET_READERS_STATE | command::GET_STATE_ARRAY => self.send_reader_states(),
            command::GET_STATE_SIZE => {
                let mut raw = [0u8; 4];
                wire::put(&mut raw, 0, MAX_READERS as u32);
                self.reply(&raw)
            }
            command::GET_READER_EVENTS => {
                let mut raw = [0u8; wire::SIZE_EVENTS];
                wire::put(&mut raw, 0, self.table().events());
                self.reply(&raw)
            }
            command::WAIT_READER_STATE_CHANGE => self.wait_for_change(),
            command::STOP_WAITING_READER_STATE_CHANGE => self.stop_waiting(),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown command 0x{other:02x}"),
            )),
        }
    }

    /// Agrees a protocol version, the first thing a client does.
    fn version(&mut self, request: Header) -> io::Result<()> {
        let mut raw = self.body(request, wire::SIZE_VERSION)?;
        let (major, minor) = (wire::field(&raw, 0), wire::field(&raw, 1));
        let result = if major != wire::VERSION_MAJOR
            || !(wire::BACKWARD_MINOR..=wire::VERSION_MINOR).contains(&minor)
        {
            Code::SERVICE_STOPPED
        } else {
            self.old_api = minor <= OLD_READER_STATE;
            Code::SUCCESS
        };
        // pcscd answers with its own version whether or not it accepted the
        // client's, so that a client one minor behind can try again.
        wire::put(&mut raw, 0, wire::VERSION_MAJOR);
        wire::put(&mut raw, 1, wire::VERSION_MINOR);
        wire::put(&mut raw, 2, result.0);
        self.reply(&raw)
    }

    fn establish_context(&mut self, request: Header) -> io::Result<()> {
        let mut raw = self.body(request, wire::SIZE_ESTABLISH)?;
        let (identifier, result) = if wire::field(&raw, 0) > SCOPE_GLOBAL {
            (0, Code::INVALID_VALUE)
        } else {
            let identifier = self.server.next_handle();
            self.contexts.push(identifier);
            self.server
                .shared()
                .contexts
                .insert(identifier, Arc::clone(&self.peer));
            (identifier, Code::SUCCESS)
        };
        wire::put(&mut raw, 1, identifier);
        wire::put(&mut raw, 2, result.0);
        self.reply(&raw)
    }

    fn release_context(&mut self, request: Header) -> io::Result<()> {
        let mut raw = self.body(request, wire::SIZE_RELEASE)?;
        let identifier = wire::field(&raw, 0);
        let result = if let Some(at) = self.contexts.iter().position(|&c| c == identifier) {
            self.contexts.remove(at);
            self.server.shared().contexts.remove(&identifier);
            let mut first = Code::SUCCESS;
            let theirs: Vec<i32> = self
                .cards
                .iter()
                .filter(|(_, opened)| opened.context == identifier)
                .map(|(&card, _)| card)
                .collect();
            for card in theirs {
                if let Some(mut opened) = self.cards.remove(&card) {
                    let code = Code::of(&opened.card.disconnect(LEAVE_CARD));
                    if first == Code::SUCCESS {
                        first = code;
                    }
                }
            }
            first
        } else {
            Code::INVALID_HANDLE
        };
        wire::put(&mut raw, 1, result.0);
        self.table().refresh();
        self.reply(&raw)
    }

    fn connect(&mut self, request: Header) -> io::Result<()> {
        let mut raw = self.body(request, wire::SIZE_CONNECT)?;
        let context = wire::field(&raw, 0);
        let reader = wire::c_string(&raw[4..4 + MAX_READER_NAME]);
        let share = wire::word(&raw, 132);
        let protocols = wire::word(&raw, 136);
        let opened = if !self.contexts.contains(&context) {
            Err(Code::INVALID_HANDLE)
        } else {
            self.open(&reader, share, protocols)
        };
        let (identifier, active, result) = match opened {
            Ok((card, active)) => {
                let identifier = self.server.next_handle() as i32;
                self.cards.insert(
                    identifier,
                    Opened {
                        context,
                        reader,
                        card,
                    },
                );
                (identifier, active, Code::SUCCESS)
            }
            Err(code) => (0, PROTOCOL_UNDEFINED, code),
        };
        wire::set_word(&mut raw, 140, identifier as u32);
        wire::set_word(&mut raw, 144, active);
        wire::set_word(&mut raw, 148, result.0);
        self.table().refresh();
        self.reply(&raw)
    }

    fn open(&self, reader: &str, share: u32, protocols: u32) -> Result<(Box<dyn Card>, u32), Code> {
        if !matches!(share, SHARE_EXCLUSIVE | SHARE_SHARED | SHARE_DIRECT) {
            return Err(Code::INVALID_VALUE);
        }
        if share != SHARE_DIRECT && protocols == PROTOCOL_UNDEFINED {
            return Err(Code::INVALID_VALUE);
        }
        let readers = self.server.inner.cards.readers()?;
        if !readers.iter().any(|name| name == reader) {
            return Err(Code::UNKNOWN_READER);
        }
        self.server.inner.cards.connect(reader, share, protocols)
    }

    fn card(&mut self, identifier: u32) -> Result<&mut Box<dyn Card>, Code> {
        self.cards
            .get_mut(&(identifier as i32))
            .map(|opened| &mut opened.card)
            .ok_or(Code::INVALID_HANDLE)
    }

    fn reconnect(&mut self, request: Header) -> io::Result<()> {
        let mut raw = self.body(request, wire::SIZE_RECONNECT)?;
        let (share, protocols, initialization) = (
            wire::field(&raw, 1),
            wire::field(&raw, 2),
            wire::field(&raw, 3),
        );
        let answer = self.card(wire::field(&raw, 0)).and_then(|card| {
            disposition(initialization)?;
            card.reconnect(share, protocols, initialization)
        });
        wire::put(&mut raw, 4, *answer.as_ref().unwrap_or(&PROTOCOL_UNDEFINED));
        wire::put(&mut raw, 5, Code::of(&answer).0);
        self.table().refresh();
        self.reply(&raw)
    }

    fn disconnect(&mut self, request: Header) -> io::Result<()> {
        let mut raw = self.body(request, wire::SIZE_DISCONNECT)?;
        let identifier = wire::field(&raw, 0) as i32;
        let chosen = wire::field(&raw, 1);
        let result = match disposition(chosen) {
            Err(code) => Err(code),
            Ok(()) => match self.cards.remove(&identifier) {
                Some(mut opened) => opened.card.disconnect(chosen),
                None => Err(Code::INVALID_HANDLE),
            },
        };
        wire::put(&mut raw, 2, Code::of(&result).0);
        self.table().refresh();
        self.reply(&raw)
    }

    fn begin_transaction(&mut self, request: Header) -> io::Result<()> {
        let mut raw = self.body(request, wire::SIZE_BEGIN)?;
        let result = self
            .card(wire::field(&raw, 0))
            .and_then(|card| card.begin_transaction());
        wire::put(&mut raw, 1, Code::of(&result).0);
        self.reply(&raw)
    }

    fn end_transaction(&mut self, request: Header) -> io::Result<()> {
        let mut raw = self.body(request, wire::SIZE_END)?;
        let chosen = wire::field(&raw, 1);
        let result = self.card(wire::field(&raw, 0)).and_then(|card| {
            disposition(chosen)?;
            card.end_transaction(chosen)
        });
        wire::put(&mut raw, 2, Code::of(&result).0);
        self.reply(&raw)
    }

    /// `SCardStatus`, of which the client uses only the return value: it
    /// reads the card's state and ATR from the reader array instead, so what
    /// the card says goes there.
    fn status(&mut self, request: Header) -> io::Result<()> {
        let mut raw = self.body(request, wire::SIZE_STATUS)?;
        let identifier = wire::field(&raw, 0) as i32;
        let result = match self.cards.get_mut(&identifier) {
            Some(opened) => match opened.card.status() {
                Ok((present, atr)) => {
                    self.server
                        .inner
                        .table
                        .observed(&opened.reader, present, &atr);
                    Code::SUCCESS
                }
                Err(code) => code,
            },
            None => Code::INVALID_HANDLE,
        };
        wire::put(&mut raw, 1, result.0);
        self.reply(&raw)
    }

    /// One APDU. The command follows the structure on the socket, and the
    /// answer follows the answering structure the same way.
    fn transmit(&mut self, request: Header) -> io::Result<()> {
        let mut raw = self.body(request, wire::SIZE_TRANSMIT)?;
        let send_protocol = wire::field(&raw, 1);
        let length = wire::field(&raw, 3) as usize;
        let capacity = wire::field(&raw, 6) as usize;
        let command = self.following(length, "a command")?;
        let mut answer = self.card(wire::field(&raw, 0)).and_then(|card| {
            if command.is_empty() {
                return Err(Code::INVALID_PARAMETER);
            }
            card.transmit(send_protocol, &command)
        });
        if matches!(&answer, Ok((received, _)) if received.len() > capacity) {
            answer = Err(Code::INSUFFICIENT_BUFFER);
        }
        let (received, protocol) = answer.clone().unwrap_or((Vec::new(), send_protocol));
        wire::put(&mut raw, 4, protocol);
        wire::put(&mut raw, 6, received.len() as u32);
        wire::put(&mut raw, 7, Code::of(&answer).0);
        self.reply(&raw)?;
        if answer.is_ok() {
            self.reply(&received)?;
        }
        Ok(())
    }

    /// Reads the bytes that follow a structure, refusing an impossible length.
    fn following(&mut self, length: usize, what: &str) -> io::Result<Vec<u8>> {
        if length > MAX_TRANSMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{what} of {length} bytes is over the limit"),
            ));
        }
        let mut data = vec![0u8; length];
        (&*self.socket).read_exact(&mut data)?;
        Ok(data)
    }

    /// A reader control code, which these readers do not have.
    fn control(&mut self, request: Header) -> io::Result<()> {
        let mut raw = self.body(request, wire::SIZE_CONTROL)?;
        let code = wire::field(&raw, 1);
        let length = wire::field(&raw, 2) as usize;
        let capacity = wire::field(&raw, 3) as usize;
        let command = self.following(length, "a control command")?;
        let mut answer = self
            .card(wire::field(&raw, 0))
            .and_then(|card| card.control(code, &command));
        if matches!(&answer, Ok(received) if received.len() > capacity) {
            answer = Err(Code::INSUFFICIENT_BUFFER);
        }
        let received = answer.clone().unwrap_or_default();
        wire::put(&mut raw, 4, received.len() as u32);
        wire::put(&mut raw, 5, Code::of(&answer).0);
        self.reply(&raw)?;
        if answer.is_ok() {
            self.reply(&received)?;
        }
        Ok(())
    }

    /// Reads or writes an attribute. Both directions use one structure, with
    /// the value inside it rather than after it.
    fn attribute(&mut self, request: Header) -> io::Result<()> {
        let mut raw = self.body(request, wire::SIZE_GET_SET)?;
        let id = wire::field(&raw, 1);
        let mut length = wire::word(&raw, 8 + MAX_ATTRIBUTE) as usize;
        if length > MAX_ATTRIBUTE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("an attribute of {length} bytes is over the limit"),
            ));
        }
        let identifier = wire::field(&raw, 0);
        let result = if request.command == command::GET_ATTRIB {
            match self.card(identifier).and_then(|card| card.get_attrib(id)) {
                Ok(value) if value.len() > length => Code::INSUFFICIENT_BUFFER,
                Ok(value) => {
                    raw[8..8 + value.len()].copy_from_slice(&value);
                    length = value.len();
                    Code::SUCCESS
                }
                Err(code) => code,
            }
        } else {
            let value = raw[8..8 + length].to_vec();
            Code::of(
                &self
                    .card(identifier)
                    .and_then(|card| card.set_attrib(id, &value)),
            )
        };
        wire::set_word(&mut raw, 8 + MAX_ATTRIBUTE, length as u32);
        wire::set_word(&mut raw, 12 + MAX_ATTRIBUTE, result.0);
        self.reply(&raw)
    }

    /// Unblocks another connection's wait. It arrives on a connection of its
    /// own, since the waiting one is blocked reading; the context identifier
    /// is the only thing tying the two together.
    fn cancel(&mut self, request: Header) -> io::Result<()> {
        let mut raw = self.body(request, wire::SIZE_CANCEL)?;
        let identifier = wire::field(&raw, 0);
        let (owner, waiting) = {
            let mut shared = self.server.shared();
            let owner = shared.contexts.get(&identifier).cloned();
            let waiting = owner
                .as_ref()
                .and_then(|owner| shared.waiters.remove(&owner.id));
            (owner, waiting)
        };
        let result = match (owner, waiting) {
            (None, _) => Code::INVALID_HANDLE,
            (Some(_), Some(waiter)) => {
                self.table().unwatch();
                waiter.signal(Code::CANCELLED.0);
                Code::SUCCESS
            }
            // Nothing was waiting, and a cancel kept for the next wait would
            // cancel one the application never asked to cancel.
            (Some(_), None) => Code::SUCCESS,
        };
        wire::put(&mut raw, 1, result.0);
        self.reply(&raw)
    }

    fn wait_for_change(&mut self) -> io::Result<()> {
        let registered = self
            .server
            .shared()
            .waiters
            .insert(self.peer.id, Arc::clone(&self.peer))
            .is_none();
        if registered {
            self.table().watch();
        }
        if self.old_api {
            return self.send_reader_states();
        }
        Ok(())
    }

    /// Takes the client off the event list. The answer is the message an event
    /// would have been, so a client signalled at the same moment reads exactly
    /// one of the two.
    fn stop_waiting(&mut self) -> io::Result<()> {
        let registered = self.server.shared().waiters.remove(&self.peer.id).is_some();
        if !registered {
            return Ok(());
        }
        self.table().unwatch();
        let mut raw = [0u8; wire::SIZE_WAIT];
        wire::put(&mut raw, 1, Code::SUCCESS.0);
        self.reply(&raw)
    }

    /// The reader array, always the full fixed number of entries.
    fn send_reader_states(&mut self) -> io::Result<()> {
        let entries = self.table().states();
        let mut raw = Vec::with_capacity(MAX_READERS * READER_STATE_LEN);
        for index in 0..MAX_READERS {
            match entries.get(index) {
                Some(entry) => raw.extend_from_slice(&entry.encode()),
                None => raw.extend_from_slice(&[0u8; READER_STATE_LEN]),
            }
        }
        self.reply(&raw)
    }
}

fn disposition(chosen: u32) -> Result<(), Code> {
    if (LEAVE_CARD..=EJECT_CARD).contains(&chosen) {
        Ok(())
    } else {
        Err(Code::INVALID_VALUE)
    }
}

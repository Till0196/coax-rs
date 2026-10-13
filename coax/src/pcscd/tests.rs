// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! The socket, driven the way pcsc-lite's client library drives it, against a
//! reader that is not a tuner.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::server::{Card, Cards, Server};
use super::table::{Slot, Slots, Table};
use super::wire::{self, Code, command, *};
use crate::daemon::unix_socket::{Listener, Socket};

const READER: &str = "Fake Smart Card Reader 0";

#[derive(Debug, Default)]
struct Fake {
    present: AtomicBool,
}

#[derive(Debug, Clone)]
struct Shared(Arc<Fake>);

impl Slots for Shared {
    fn readers(&self) -> io::Result<Vec<String>> {
        Ok(vec![READER.to_owned()])
    }

    fn open(&self, _reader: &str) -> io::Result<Box<dyn Slot>> {
        Ok(Box::new(self.clone()))
    }
}

impl Slot for Shared {
    fn present(&self) -> bool {
        self.0.present.load(Ordering::SeqCst)
    }

    fn atr(&self) -> Vec<u8> {
        vec![0x3B, 0xF0]
    }

    fn sharing(&self) -> i32 {
        0
    }
}

impl Cards for Shared {
    fn readers(&self) -> Result<Vec<String>, Code> {
        Ok(vec![READER.to_owned()])
    }

    fn connect(&self, _: &str, _: u32, _: u32) -> Result<(Box<dyn Card>, u32), Code> {
        if !self.present() {
            return Err(Code::NO_SMARTCARD);
        }
        Ok((Box::new(FakeCard), PROTOCOL_T1))
    }
}

#[derive(Debug)]
struct FakeCard;

impl Card for FakeCard {
    fn reconnect(&mut self, _: u32, _: u32, _: u32) -> Result<u32, Code> {
        Ok(PROTOCOL_T1)
    }
    fn disconnect(&mut self, _: u32) -> Result<(), Code> {
        Ok(())
    }
    fn begin_transaction(&mut self) -> Result<(), Code> {
        Ok(())
    }
    fn end_transaction(&mut self, _: u32) -> Result<(), Code> {
        Ok(())
    }
    fn status(&mut self) -> Result<(bool, Vec<u8>), Code> {
        Ok((true, vec![0x3B, 0xF0]))
    }
    /// Echoes the first byte, then "90 00".
    fn transmit(&mut self, _: u32, send: &[u8]) -> Result<(Vec<u8>, u32), Code> {
        Ok((vec![send[0], 0x90, 0x00], PROTOCOL_T1))
    }
    fn control(&mut self, _: u32, _: &[u8]) -> Result<Vec<u8>, Code> {
        Err(Code::UNSUPPORTED_FEATURE)
    }
    fn get_attrib(&mut self, id: u32) -> Result<Vec<u8>, Code> {
        match id {
            attribute::ATR_STRING => Ok(vec![0x3B, 0xF0]),
            _ => Err(Code::UNSUPPORTED_FEATURE),
        }
    }
    fn set_attrib(&mut self, _: u32, _: &[u8]) -> Result<(), Code> {
        Err(Code::UNSUPPORTED_FEATURE)
    }
}

struct Running {
    path: PathBuf,
    fake: Arc<Fake>,
    table: Table,
    _dir: TempDir,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.table.close();
    }
}

/// A directory removed afterwards.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "coax-pcscd-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn running(present: bool) -> Running {
    let fake = Arc::new(Fake {
        present: AtomicBool::new(present),
    });
    let table = Table::new(
        Box::new(Shared(Arc::clone(&fake))),
        Duration::from_millis(5),
    );
    let server = Server::new(
        Box::new(Shared(Arc::clone(&fake))),
        table.clone(),
        Box::new(|_| {}),
    );
    let dir = TempDir::new();
    let path = dir.0.join("pcscd.comm");
    let listener = Listener::bind(&path).unwrap();
    std::thread::spawn(move || server.serve(&listener));
    Running {
        path,
        fake,
        table,
        _dir: dir,
    }
}

struct Client(Socket);

impl Client {
    fn dial(running: &Running) -> Self {
        let socket = Socket::connect(&running.path.to_string_lossy()).unwrap();
        socket.set_timeout(Duration::from_secs(5)).unwrap();
        Self(socket)
    }

    fn send(&mut self, command: u32, payload: &[u8]) {
        let mut raw = [0u8; 8];
        wire::put(&mut raw, 0, payload.len() as u32);
        wire::put(&mut raw, 1, command);
        self.0.write_all(&raw).unwrap();
        self.0.write_all(payload).unwrap();
    }

    fn receive(&mut self, length: usize) -> Vec<u8> {
        let mut raw = vec![0u8; length];
        self.0.read_exact(&mut raw).unwrap();
        raw
    }

    fn handshake(&mut self, major: u32, minor: u32) -> u32 {
        let mut raw = [0u8; SIZE_VERSION];
        wire::put(&mut raw, 0, major);
        wire::put(&mut raw, 1, minor);
        self.send(command::VERSION, &raw);
        wire::field(&self.receive(SIZE_VERSION), 2)
    }

    fn establish(&mut self) -> (u32, u32) {
        let mut raw = [0u8; SIZE_ESTABLISH];
        wire::put(&mut raw, 0, 2);
        self.send(command::ESTABLISH_CONTEXT, &raw);
        let answer = self.receive(SIZE_ESTABLISH);
        (wire::field(&answer, 1), wire::field(&answer, 2))
    }

    fn connect(&mut self, context: u32) -> (i32, u32, u32) {
        let mut raw = [0u8; SIZE_CONNECT];
        wire::put(&mut raw, 0, context);
        put_c_string(&mut raw[4..4 + MAX_READER_NAME], READER);
        set_word(&mut raw, 132, SHARE_SHARED);
        set_word(&mut raw, 136, 0x0003);
        self.send(command::CONNECT, &raw);
        let answer = self.receive(SIZE_CONNECT);
        (
            word(&answer, 140) as i32,
            word(&answer, 144),
            word(&answer, 148),
        )
    }

    fn wait_result(&mut self) -> u32 {
        wire::field(&self.receive(SIZE_WAIT), 1)
    }
}

#[test]
fn the_version_handshake_takes_the_current_and_older_minors() {
    let running = running(true);
    assert_eq!(
        Client::dial(&running).handshake(VERSION_MAJOR, VERSION_MINOR),
        Code::SUCCESS.0
    );
    assert_eq!(
        Client::dial(&running).handshake(VERSION_MAJOR, BACKWARD_MINOR),
        Code::SUCCESS.0,
        "a client one protocol behind"
    );
    assert_eq!(
        Client::dial(&running).handshake(VERSION_MAJOR + 1, 0),
        Code::SERVICE_STOPPED.0,
        "a client of another protocol"
    );
}

#[test]
fn the_reader_array_names_the_reader_and_is_full_length() {
    let running = running(true);
    let mut client = Client::dial(&running);
    client.handshake(VERSION_MAJOR, VERSION_MINOR);
    client.send(command::GET_STATE_SIZE, &[]);
    assert_eq!(wire::field(&client.receive(4), 0) as usize, MAX_READERS);
    client.send(command::GET_STATE_ARRAY, &[]);
    let raw = client.receive(MAX_READERS * READER_STATE_LEN);
    assert_eq!(c_string(&raw[0..MAX_READER_NAME]), READER);
    assert_eq!(
        c_string(&raw[READER_STATE_LEN..READER_STATE_LEN + MAX_READER_NAME]),
        ""
    );
}

#[test]
fn a_card_is_connected_spoken_to_and_let_go() {
    let running = running(true);
    let mut client = Client::dial(&running);
    client.handshake(VERSION_MAJOR, VERSION_MINOR);
    let (context, result) = client.establish();
    assert_eq!(result, Code::SUCCESS.0);
    let (card, protocol, result) = client.connect(context);
    assert_eq!(result, Code::SUCCESS.0);
    assert_eq!(protocol, PROTOCOL_T1);

    let apdu = [0x90u8, 0x30, 0x00, 0x00, 0x00];
    let mut raw = [0u8; SIZE_TRANSMIT];
    wire::put(&mut raw, 0, card as u32);
    wire::put(&mut raw, 1, PROTOCOL_T1);
    wire::put(&mut raw, 2, 8);
    wire::put(&mut raw, 3, apdu.len() as u32);
    wire::put(&mut raw, 6, 264);
    client.send(command::TRANSMIT, &raw);
    client.0.write_all(&apdu).unwrap();
    let answer = client.receive(SIZE_TRANSMIT);
    assert_eq!(wire::field(&answer, 7), Code::SUCCESS.0);
    let length = wire::field(&answer, 6) as usize;
    assert_eq!(client.receive(length), [0x90, 0x90, 0x00]);

    let mut raw = [0u8; SIZE_DISCONNECT];
    wire::put(&mut raw, 0, card as u32);
    client.send(command::DISCONNECT, &raw);
    assert_eq!(
        wire::field(&client.receive(SIZE_DISCONNECT), 2),
        Code::SUCCESS.0
    );

    // Gone once disconnected.
    let mut raw = [0u8; SIZE_STATUS];
    wire::put(&mut raw, 0, card as u32);
    client.send(command::STATUS, &raw);
    assert_eq!(
        wire::field(&client.receive(SIZE_STATUS), 1),
        Code::INVALID_HANDLE.0
    );
}

#[test]
fn a_reply_too_long_for_the_buffer_is_refused() {
    let running = running(true);
    let mut client = Client::dial(&running);
    client.handshake(VERSION_MAJOR, VERSION_MINOR);
    let (context, _) = client.establish();
    let (card, _, _) = client.connect(context);
    let mut raw = [0u8; SIZE_TRANSMIT];
    wire::put(&mut raw, 0, card as u32);
    wire::put(&mut raw, 1, PROTOCOL_T1);
    wire::put(&mut raw, 3, 1);
    wire::put(&mut raw, 6, 2);
    client.send(command::TRANSMIT, &raw);
    client.0.write_all(&[0x00]).unwrap();
    let answer = client.receive(SIZE_TRANSMIT);
    assert_eq!(wire::field(&answer, 7), Code::INSUFFICIENT_BUFFER.0);
    // Nothing follows a refusal: the next reply is the next question's.
    client.send(command::GET_STATE_SIZE, &[]);
    assert_eq!(wire::field(&client.receive(4), 0) as usize, MAX_READERS);
}

#[test]
fn an_unknown_reader_is_refused_by_name() {
    let running = running(true);
    let mut client = Client::dial(&running);
    client.handshake(VERSION_MAJOR, VERSION_MINOR);
    let (context, _) = client.establish();
    let mut raw = [0u8; SIZE_CONNECT];
    wire::put(&mut raw, 0, context);
    put_c_string(&mut raw[4..4 + MAX_READER_NAME], "Nobody");
    set_word(&mut raw, 132, SHARE_SHARED);
    set_word(&mut raw, 136, PROTOCOL_T1);
    client.send(command::CONNECT, &raw);
    assert_eq!(
        word(&client.receive(SIZE_CONNECT), 148),
        Code::UNKNOWN_READER.0
    );
}

/// The cancel arrives on a connection of its own, because the waiting one is
/// blocked reading. This is the case the server's context table exists for.
#[test]
fn a_cancel_from_another_connection_wakes_the_waiter() {
    let running = running(true);
    let mut waiter = Client::dial(&running);
    waiter.handshake(VERSION_MAJOR, VERSION_MINOR);
    let (context, _) = waiter.establish();
    // The first look at a slot is itself a change; read that one first.
    waiter.send(command::WAIT_READER_STATE_CHANGE, &[]);
    assert_eq!(waiter.wait_result(), Code::SUCCESS.0);
    waiter.send(command::WAIT_READER_STATE_CHANGE, &[]);

    let mut canceller = Client::dial(&running);
    canceller.handshake(VERSION_MAJOR, VERSION_MINOR);
    let mut raw = [0u8; SIZE_CANCEL];
    wire::put(&mut raw, 0, context);
    canceller.send(command::CANCEL, &raw);
    assert_eq!(
        wire::field(&canceller.receive(SIZE_CANCEL), 1),
        Code::SUCCESS.0
    );
    assert_eq!(waiter.wait_result(), Code::CANCELLED.0);
}

#[test]
fn a_card_arriving_wakes_a_waiting_client() {
    let running = running(false);
    let mut client = Client::dial(&running);
    client.handshake(VERSION_MAJOR, VERSION_MINOR);
    client.establish();
    client.send(command::WAIT_READER_STATE_CHANGE, &[]);
    assert_eq!(
        client.wait_result(),
        Code::SUCCESS.0,
        "the first measurement"
    );
    client.send(command::GET_STATE_ARRAY, &[]);
    let raw = client.receive(MAX_READERS * READER_STATE_LEN);
    assert_eq!(word(&raw, 132), STATE_ABSENT);

    client.send(command::WAIT_READER_STATE_CHANGE, &[]);
    running.fake.present.store(true, Ordering::SeqCst);
    assert_eq!(client.wait_result(), Code::SUCCESS.0);
    client.send(command::GET_STATE_ARRAY, &[]);
    let raw = client.receive(MAX_READERS * READER_STATE_LEN);
    assert_ne!(word(&raw, 132) & STATE_PRESENT, 0);
    assert_eq!(word(&raw, 176), 2, "the ATR's length");
}

/// Stopping a wait nothing has answered is answered like an event, so the
/// client reads exactly one message either way.
#[test]
fn stopping_a_wait_is_answered_once() {
    let running = running(true);
    let mut client = Client::dial(&running);
    client.handshake(VERSION_MAJOR, VERSION_MINOR);
    client.send(command::WAIT_READER_STATE_CHANGE, &[]);
    assert_eq!(
        client.wait_result(),
        Code::SUCCESS.0,
        "the first measurement"
    );
    client.send(command::WAIT_READER_STATE_CHANGE, &[]);
    client.send(command::STOP_WAITING_READER_STATE_CHANGE, &[]);
    assert_eq!(client.wait_result(), Code::SUCCESS.0);
    client.send(command::GET_STATE_SIZE, &[]);
    assert_eq!(wire::field(&client.receive(4), 0) as usize, MAX_READERS);
}

/// A client of the older protocol gets the array right after registering.
#[test]
fn an_older_client_gets_the_array_with_its_wait() {
    let running = running(true);
    let mut client = Client::dial(&running);
    client.handshake(VERSION_MAJOR, OLD_READER_STATE);
    client.send(command::WAIT_READER_STATE_CHANGE, &[]);
    let raw = client.receive(MAX_READERS * READER_STATE_LEN);
    assert_eq!(c_string(&raw[0..MAX_READER_NAME]), READER);
}

#[test]
fn an_unknown_command_drops_the_client() {
    let running = running(true);
    let mut client = Client::dial(&running);
    client.handshake(VERSION_MAJOR, VERSION_MINOR);
    client.send(0x7F, &[]);
    let mut one = [0u8; 1];
    assert_eq!(client.0.read(&mut one).unwrap(), 0, "closed, not answered");
}

/// A client going away while it waits leaves nothing measuring for it.
#[test]
fn a_departed_waiter_is_forgotten() {
    let running = running(true);
    {
        let mut client = Client::dial(&running);
        client.handshake(VERSION_MAJOR, VERSION_MINOR);
        client.send(command::WAIT_READER_STATE_CHANGE, &[]);
        assert_eq!(client.wait_result(), Code::SUCCESS.0);
        client.send(command::WAIT_READER_STATE_CHANGE, &[]);
    }
    let mut other = Client::dial(&running);
    other.handshake(VERSION_MAJOR, VERSION_MINOR);
    // Answered at all means the server is still serving after the departure.
    other.send(command::GET_READER_EVENTS, &[]);
    other.receive(SIZE_EVENTS);
}

/// 作成した側だけがソケットを削除し、動作中のソケットに二つ目は作成されない。
#[test]
fn a_served_socket_is_refused_twice_and_removed_when_dropped() {
    let dir = TempDir::new();
    let path = dir.0.join("pcscd.comm");
    let nowhere =
        crate::daemon::Endpoint::Socket(dir.0.join("no-coaxd.sock").to_string_lossy().into_owned());
    let serving = super::serve(nowhere.clone(), &path, |_| {}).expect("serves");
    assert!(Socket::connect(&path.to_string_lossy()).is_ok(), "answers");
    let again = super::serve(nowhere, &path, |_| {}).expect_err("already served");
    assert_eq!(again.kind(), io::ErrorKind::AddrInUse);
    drop(serving);
    assert!(!path.exists(), "removed");
}

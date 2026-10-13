// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Unix domain sockets on Windows.
//!
//! Windows has had `AF_UNIX` since 2018 and the daemon listens on one by
//! default, but the standard library offers them on Unix only. So the four
//! calls it takes are made here directly. The path goes into the address as
//! its UTF-8 bytes, which is what `bind` on the daemon's side is given.

use std::ffi::c_void;
use std::io::{self, Read, Write};
use std::os::windows::io::{AsRawSocket, FromRawSocket, OwnedSocket};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

type Socket_ = usize;

const INVALID_SOCKET: Socket_ = usize::MAX;
const AF_UNIX: i32 = 1;
const SOCK_STREAM: i32 = 1;
const SD_BOTH: i32 = 2;

/// The address a Unix socket is named by: a family and a path.
#[repr(C)]
struct SocketAddress {
    family: u16,
    path: [u8; 108],
}

#[repr(C)]
struct WsaData {
    version: u16,
    high_version: u16,
    max_sockets: u16,
    max_udp: u16,
    vendor_info: *mut u8,
    description: [u8; 257],
    system_status: [u8; 129],
}

#[link(name = "ws2_32")]
unsafe extern "system" {
    fn WSAStartup(version: u16, data: *mut WsaData) -> i32;
    // Reads this thread's last error.
    safe fn WSAGetLastError() -> i32;
    // Makes a new socket from numbers, like opening a file.
    safe fn socket(family: i32, kind: i32, protocol: i32) -> Socket_;
    fn connect(socket: Socket_, address: *const c_void, length: i32) -> i32;
    fn bind(socket: Socket_, address: *const c_void, length: i32) -> i32;
    fn listen(socket: Socket_, backlog: i32) -> i32;
    fn accept(socket: Socket_, address: *mut c_void, length: *mut i32) -> Socket_;
    fn send(socket: Socket_, data: *const u8, length: i32, flags: i32) -> i32;
    fn recv(socket: Socket_, into: *mut u8, length: i32, flags: i32) -> i32;
    fn shutdown(socket: Socket_, how: i32) -> i32;
    fn setsockopt(socket: Socket_, level: i32, name: i32, value: *const c_void, length: i32)
    -> i32;
    fn closesocket(socket: Socket_) -> i32;
    fn WSAIoctl(
        socket: Socket_,
        code: u32,
        input: *const c_void,
        input_length: u32,
        output: *mut c_void,
        output_length: u32,
        returned: *mut u32,
        overlapped: *mut c_void,
        completion: *mut c_void,
    ) -> i32;
}

/// `SIO_AF_UNIX_GETPEERPID` (`_WSAIOR(IOC_VENDOR, 256)`): the process at the
/// other end of a Unix socket. Windows 10 1903 and later.
const SIO_AF_UNIX_GETPEERPID: u32 = 0x5800_0100;

/// The socket options this sets: how long a read or a write may sit before it
/// gives up, in milliseconds.
const SOL_SOCKET: i32 = 0xffff;
const SO_RCVTIMEO: i32 = 0x1006;
const SO_SNDTIMEO: i32 = 0x1005;

/// Winsock has to be started before any of it is used, once per process. What
/// it said is kept, so that a Winsock that would not start is the error the
/// caller sees rather than whatever the first call made without it says.
fn start_winsock() -> io::Result<()> {
    use std::sync::OnceLock;
    static STARTED: OnceLock<i32> = OnceLock::new();
    let said = *STARTED.get_or_init(|| {
        let mut data = WsaData {
            version: 0,
            high_version: 0,
            max_sockets: 0,
            max_udp: 0,
            vendor_info: std::ptr::null_mut(),
            description: [0; 257],
            system_status: [0; 129],
        };
        // 2.2, which is what every Windows since 98 has.
        // SAFETY: the answer is written to a local.
        unsafe { WSAStartup(0x0202, &mut data) }
    });
    match said {
        0 => Ok(()),
        code => Err(io::Error::other(format!(
            "could not start Winsock: error {code}"
        ))),
    }
}

/// A length Winsock takes as an `int`. A buffer longer than that is read or
/// written in part, which `Read` and `Write` allow.
fn clamped(length: usize) -> i32 {
    i32::try_from(length).unwrap_or(i32::MAX)
}

/// A connected Unix domain socket. The descriptor is behind an `Arc` so that an
/// [`Interrupter`] can be handed out without a second one: what it does is
/// shut this one down, and a descriptor value already closed is one Winsock may
/// have handed to somebody else in the same process.
///
/// The descriptor is closed exactly once, when the last holder lets go. The
/// only sharing is the reader thread against a shutdown from another, which is
/// what the API is for.
#[derive(Debug)]
pub struct Socket {
    handle: Arc<OwnedSocket>,
}

/// The address for a path, refusing one it cannot hold.
fn address_of(path: &str) -> io::Result<SocketAddress> {
    let bytes = path.as_bytes();
    let mut address = SocketAddress {
        family: AF_UNIX as u16,
        path: [0; 108],
    };
    if bytes.len() >= address.path.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the socket path is too long for an address: {path}"),
        ));
    }
    address.path[..bytes.len()].copy_from_slice(bytes);
    Ok(address)
}

impl Socket {
    pub fn connect(path: &str) -> io::Result<Self> {
        start_winsock()?;
        let address = address_of(path)?;
        let handle = socket(AF_UNIX, SOCK_STREAM, 0);
        if handle == INVALID_SOCKET {
            return Err(last_error("open a socket"));
        }
        let socket = Self::from_raw(handle);
        let length = size_of::<SocketAddress>() as i32;
        // SAFETY: the address is a local of the length given.
        if unsafe { connect(socket.raw(), (&raw const address).cast(), length) } != 0 {
            return Err(last_error(&format!("reach the daemon at {path}")));
        }
        Ok(socket)
    }

    fn from_raw(handle: Socket_) -> Self {
        // SAFETY: only called on a socket just made or accepted, which nothing
        // else holds.
        let handle =
            unsafe { OwnedSocket::from_raw_socket(handle as std::os::windows::io::RawSocket) };
        Self {
            handle: Arc::new(handle),
        }
    }

    fn raw(&self) -> Socket_ {
        self.handle.as_raw_socket() as Socket_
    }

    fn receive(&self, into: &mut [u8], doing: &str) -> io::Result<usize> {
        // SAFETY: Winsock writes at most the length given into `into`.
        let read = unsafe { recv(self.raw(), into.as_mut_ptr(), clamped(into.len()), 0) };
        match read {
            ..0 => Err(last_error(doing)),
            read => Ok(read as usize),
        }
    }

    fn transmit(&self, data: &[u8], doing: &str) -> io::Result<usize> {
        // SAFETY: Winsock reads at most the length given from `data`.
        let written = unsafe { send(self.raw(), data.as_ptr(), clamped(data.len()), 0) };
        match written {
            ..0 => Err(last_error(doing)),
            written => Ok(written as usize),
        }
    }

    /// How long a read or a write may sit before it gives up. Without this a
    /// daemon that stops answering leaves the caller waiting for ever, and a
    /// sweep that has stopped looks exactly like a sweep that is being slow.
    ///
    /// Zero is a millisecond rather than zero, because to Winsock zero means
    /// no limit at all, which is the opposite of what was asked for.
    pub fn set_timeout(&self, how_long: Duration) -> io::Result<()> {
        let milliseconds = u32::try_from(how_long.as_millis())
            .unwrap_or(u32::MAX)
            .max(1);
        for which in [SO_RCVTIMEO, SO_SNDTIMEO] {
            // SAFETY: the value is a local of the length given.
            let set = unsafe {
                setsockopt(
                    self.raw(),
                    SOL_SOCKET,
                    which,
                    (&raw const milliseconds).cast(),
                    size_of::<u32>() as i32,
                )
            };
            if set != 0 {
                return Err(last_error("set a timeout on the socket"));
            }
        }
        Ok(())
    }

    /// Ends reads and writes on this socket, from any thread.
    pub fn shutdown(&self) {
        shut(&self.handle);
    }

    /// The process at the other end, as Winsock recorded it when the connection
    /// was made. Asked of a connection to the daemon, it says which program is
    /// serving the socket, whoever started it.
    pub fn peer_process(&self) -> io::Result<u32> {
        let mut pid: u32 = 0;
        let mut returned: u32 = 0;
        // SAFETY: the answer and its length are written to locals of the sizes given.
        let said = unsafe {
            WSAIoctl(
                self.raw(),
                SIO_AF_UNIX_GETPEERPID,
                std::ptr::null(),
                0,
                (&raw mut pid).cast(),
                size_of::<u32>() as u32,
                &raw mut returned,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if said != 0 {
            return Err(last_error("ask who is at the other end of the socket"));
        }
        if pid == 0 {
            return Err(io::Error::other(
                "Winsock named no process at the other end",
            ));
        }
        Ok(pid)
    }

    /// A handle that can end a read this socket is waiting in. It shares the
    /// descriptor rather than duplicating it, and only ever shuts it down; the
    /// descriptor is closed when the socket and every interrupter are gone.
    pub fn interrupter(&self) -> Interrupter {
        Interrupter {
            handle: Arc::clone(&self.handle),
        }
    }
}

impl Read for Socket {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.receive(into, "read from the daemon")
    }
}

impl Write for Socket {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.transmit(data, "write to the daemon")
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Reading on one thread while another writes, as a server answering a
/// client and signalling it at once does. Winsock allows the two at once on
/// one socket.
impl Read for &Socket {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.receive(into, "read from the socket")
    }
}

impl Write for &Socket {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.transmit(data, "write to the socket")
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A socket being listened on.
#[derive(Debug)]
pub struct Listener {
    handle: Socket_,
    closed: AtomicBool,
}

// As for `Socket`: the sharing is an accept against a close from another thread.
unsafe impl Send for Listener {}
unsafe impl Sync for Listener {}

impl Listener {
    pub fn bind(path: &Path) -> io::Result<Self> {
        start_winsock()?;
        let written = path.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("the socket path is not text: {}", path.display()),
            )
        })?;
        let address = address_of(written)?;
        let handle = socket(AF_UNIX, SOCK_STREAM, 0);
        if handle == INVALID_SOCKET {
            return Err(last_error("open a socket"));
        }
        let listener = Self {
            handle,
            closed: AtomicBool::new(false),
        };
        let length = size_of::<SocketAddress>() as i32;
        // SAFETY: the address is a local of the length given.
        if unsafe { bind(handle, (&raw const address).cast(), length) } != 0 {
            return Err(last_error(&format!("listen on {written}")));
        }
        // SOMAXCONN.
        // SAFETY: takes numbers only; the socket is this listener's.
        if unsafe { listen(handle, 0x7fff_ffff) } != 0 {
            return Err(last_error(&format!("listen on {written}")));
        }
        Ok(listener)
    }

    pub fn accept(&self) -> io::Result<Socket> {
        // SAFETY: asks for no address, so nothing is written.
        let handle = unsafe { accept(self.handle, std::ptr::null_mut(), std::ptr::null_mut()) };
        if handle == INVALID_SOCKET {
            return Err(last_error("accept a client"));
        }
        Ok(Socket::from_raw(handle))
    }

    /// Ends an accept another thread is waiting in: on Windows that takes
    /// closing the socket, so it is closed here once and not again on drop.
    pub fn stop(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            // SAFETY: the flag lets this close the listener's socket once.
            unsafe { closesocket(self.handle) };
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Ends a read another thread is waiting in.
#[derive(Debug, Clone)]
pub struct Interrupter {
    handle: Arc<OwnedSocket>,
}

impl Interrupter {
    pub fn interrupt(&self) {
        shut(&self.handle);
    }
}

/// Ends reads and writes on a socket, from any thread.
fn shut(socket: &OwnedSocket) {
    // SAFETY: the socket is open for as long as it is borrowed.
    unsafe { shutdown(socket.as_raw_socket() as Socket_, SD_BOTH) };
}

fn last_error(doing: &str) -> io::Error {
    /// The winsock errors a caller acts on differently from a failure. A read
    /// that timed out has to read as a timeout: a caller waiting on a stream
    /// that is merely quiet would otherwise take its own patience running out
    /// for the stream having broken.
    const WSAEINTR: i32 = 10004;
    const WSAEWOULDBLOCK: i32 = 10035;
    const WSAECONNABORTED: i32 = 10053;
    const WSAECONNRESET: i32 = 10054;
    const WSAESHUTDOWN: i32 = 10058;
    const WSAETIMEDOUT: i32 = 10060;

    let code = WSAGetLastError();
    let kind = match code {
        WSAETIMEDOUT => io::ErrorKind::TimedOut,
        WSAEWOULDBLOCK => io::ErrorKind::WouldBlock,
        WSAEINTR => io::ErrorKind::Interrupted,
        WSAECONNRESET => io::ErrorKind::ConnectionReset,
        WSAECONNABORTED => io::ErrorKind::ConnectionAborted,
        WSAESHUTDOWN => io::ErrorKind::BrokenPipe,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, format!("could not {doing}: winsock error {code}"))
}

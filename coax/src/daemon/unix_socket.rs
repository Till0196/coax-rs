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
use std::os::windows::io::{AsRawSocket, FromRawSocket, IntoRawSocket, OwnedSocket};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

type Socket_ = usize;

const INVALID_SOCKET: Socket_ = usize::MAX;
const AF_UNIX: i32 = 1;
const SOCK_STREAM: i32 = 1;
const SD_BOTH: i32 = 2;
const WSA_FLAG_OVERLAPPED: u32 = 0x01;
const WSA_FLAG_NO_HANDLE_INHERIT: u32 = 0x80;
const HANDLE_FLAG_INHERIT: u32 = 0x01;

/// The address a Unix socket is named by: a family and a path.
#[repr(C)]
struct SocketAddress {
    family: u16,
    path: [u8; 108],
}

#[cfg(target_pointer_width = "64")]
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

#[cfg(target_pointer_width = "32")]
#[repr(C)]
struct WsaData {
    version: u16,
    high_version: u16,
    description: [u8; 257],
    system_status: [u8; 129],
    max_sockets: u16,
    max_udp: u16,
    vendor_info: *mut u8,
}

#[link(name = "ws2_32")]
unsafe extern "system" {
    fn WSAStartup(version: u16, data: *mut WsaData) -> i32;
    // Reads this thread's last error.
    safe fn WSAGetLastError() -> i32;
    fn WSASocketW(
        family: i32,
        kind: i32,
        protocol: i32,
        info: *const c_void,
        group: u32,
        flags: u32,
    ) -> Socket_;
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

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
}

fn new_socket() -> io::Result<OwnedSocket> {
    start_winsock()?;
    // SAFETY: no protocol information is supplied. Disable inheritance at
    // creation so a concurrently started child cannot keep this socket open.
    unsafe {
        own_socket(
            WSASocketW(
                AF_UNIX,
                SOCK_STREAM,
                0,
                std::ptr::null(),
                0,
                WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
            ),
            "open a socket",
        )
    }
}

/// Takes ownership of a newly created or accepted socket.
///
/// # Safety
/// `handle` must be a fresh, exclusively owned socket, or `INVALID_SOCKET`.
unsafe fn own_socket(handle: Socket_, doing: &str) -> io::Result<OwnedSocket> {
    if handle == INVALID_SOCKET {
        return Err(last_error(doing));
    }
    // SAFETY: the caller transfers sole ownership of this valid socket.
    Ok(unsafe { OwnedSocket::from_raw_socket(handle as _) })
}

fn checked(status: i32, doing: &str) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(last_error(doing))
    }
}

fn prevent_inheritance(socket: &OwnedSocket) -> io::Result<()> {
    // SAFETY: the borrow keeps this socket open throughout the call.
    if unsafe { SetHandleInformation(socket.as_raw_socket() as _, HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

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
        let address = address_of(path)?;
        let socket = Self::from_handle(new_socket()?);
        let doing = format!("reach the daemon at {path}");
        // SAFETY: the address is a local of the length given.
        checked(
            unsafe {
                connect(
                    socket.raw(),
                    (&raw const address).cast(),
                    size_of::<SocketAddress>() as i32,
                )
            },
            &doing,
        )?;
        Ok(socket)
    }

    fn from_handle(handle: OwnedSocket) -> Self {
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
            checked(set, "set a timeout on the socket")?;
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

impl Listener {
    pub fn bind(path: &Path) -> io::Result<Self> {
        let written = path.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("the socket path is not text: {}", path.display()),
            )
        })?;
        let address = address_of(written)?;
        let listener = Self {
            handle: new_socket()?.into_raw_socket() as Socket_,
            closed: AtomicBool::new(false),
        };
        let doing = format!("listen on {written}");
        // SAFETY: the address is a local of the length given.
        checked(
            unsafe {
                bind(
                    listener.handle,
                    (&raw const address).cast(),
                    size_of::<SocketAddress>() as i32,
                )
            },
            &doing,
        )?;
        // SAFETY: takes numbers only; the socket is this listener's.
        checked(unsafe { listen(listener.handle, 0x7fff_ffff) }, &doing)?;
        Ok(listener)
    }

    pub fn accept(&self) -> io::Result<Socket> {
        // SAFETY: accept writes no address, and transfers sole ownership of
        // its result to own_socket. Errors cannot leak the accepted socket.
        let handle = unsafe {
            own_socket(
                accept(self.handle, std::ptr::null_mut(), std::ptr::null_mut()),
                "accept a client",
            )
        }?;
        prevent_inheritance(&handle)?;
        Ok(Socket::from_handle(handle))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetHandleInformation(handle: *mut c_void, flags: *mut u32) -> i32;
    }

    fn assert_not_inheritable(handle: Socket_) {
        let mut flags = 0;
        // SAFETY: flags is writable and the caller keeps the socket open.
        assert_ne!(
            unsafe { GetHandleInformation(handle as *mut c_void, &mut flags) },
            0
        );
        assert_eq!(
            flags & HANDLE_FLAG_INHERIT,
            0,
            "a child could retain the socket"
        );
    }

    #[test]
    fn winsock_data_matches_the_windows_abi() {
        let (size, description, vendor) = if cfg!(target_pointer_width = "64") {
            (408, 16, 8)
        } else {
            (400, 4, 396)
        };
        assert_eq!(size_of::<WsaData>(), size);
        assert_eq!(std::mem::offset_of!(WsaData, description), description);
        assert_eq!(std::mem::offset_of!(WsaData, vendor_info), vendor);
    }

    #[test]
    fn connected_and_accepted_sockets_are_not_inheritable() {
        let path = std::env::temp_dir().join(format!("coax-socket-{}.sock", std::process::id()));
        let listener = Listener::bind(&path).unwrap();
        assert_not_inheritable(listener.handle);
        let client = Socket::connect(path.to_str().unwrap()).unwrap();
        assert_not_inheritable(client.raw());
        let accepted = listener.accept().unwrap();
        assert_not_inheritable(accepted.raw());
        drop((accepted, client, listener));
        let _ = std::fs::remove_file(path);
    }
}

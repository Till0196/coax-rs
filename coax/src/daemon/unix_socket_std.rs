// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Unix domain sockets where the standard library has them.
//!
//! The same shape as the Windows module beside this one, over `UnixStream`
//! rather than four calls made by hand. Both are here because the socket is
//! what the daemon listens on by default on every system.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// The longest a socket path can be, which is the size of the address the
/// kernel copies it into. Checked here because the error otherwise is `EINVAL`
/// from `connect`, which says nothing about the length.
#[cfg(target_os = "macos")]
const PATH_ROOM: usize = 104;
#[cfg(not(target_os = "macos"))]
const PATH_ROOM: usize = 108;

/// A connected Unix domain socket. The stream is behind an `Arc` so that an
/// [`Interrupter`] can be handed out without a second descriptor: what it does
/// is shut this one down, and a descriptor closed twice is one another thread
/// may have reopened.
#[derive(Debug)]
pub struct Socket {
    stream: Arc<UnixStream>,
}

impl Socket {
    pub fn connect(path: &str) -> io::Result<Self> {
        if path.len() >= PATH_ROOM {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("the socket path is too long for an address: {path}"),
            ));
        }
        let stream = UnixStream::connect(path).map_err(|reason| {
            io::Error::new(
                reason.kind(),
                format!("reach the daemon at {path}: {reason}"),
            )
        })?;
        Ok(Self {
            stream: Arc::new(stream),
        })
    }

    /// How long a read or a write may sit before it gives up. Without this a
    /// daemon that stops answering leaves the caller waiting for ever, and a
    /// sweep that has stopped looks exactly like a sweep that is being slow.
    pub fn set_timeout(&self, how_long: Duration) -> io::Result<()> {
        self.stream.set_read_timeout(Some(how_long))?;
        self.stream.set_write_timeout(Some(how_long))
    }

    /// Ends reads and writes on this socket, from any thread.
    pub fn shutdown(&self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }

    /// A handle that can end a read this socket is waiting in.
    pub fn interrupter(&self) -> Interrupter {
        Interrupter {
            stream: Arc::clone(&self.stream),
        }
    }

    /// The process at the other end, as the kernel recorded it when the
    /// connection was made. Asked of a connection to the daemon, it says which
    /// program is serving the socket, whoever started it.
    pub fn peer_process(&self) -> io::Result<u32> {
        use std::os::fd::AsRawFd;
        peer_process(self.stream.as_raw_fd())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn peer_process(fd: i32) -> io::Result<u32> {
    #[repr(C)]
    struct Credentials {
        pid: i32,
        uid: u32,
        gid: u32,
    }
    const SOL_SOCKET: i32 = 1;
    const SO_PEERCRED: i32 = 17;
    let mut credentials = Credentials {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = size_of::<Credentials>() as u32;
    // SAFETY: the answer and its length are written to locals of the sizes given.
    let said = unsafe {
        getsockopt(
            fd,
            SOL_SOCKET,
            SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut length,
        )
    };
    if said != 0 {
        return Err(io::Error::last_os_error());
    }
    u32::try_from(credentials.pid)
        .ok()
        .filter(|pid| *pid != 0)
        .ok_or_else(|| io::Error::other("the kernel named no process at the other end"))
}

#[cfg(target_os = "macos")]
fn peer_process(fd: i32) -> io::Result<u32> {
    // SOL_LOCAL and LOCAL_PEERPID, from sys/un.h.
    const SOL_LOCAL: i32 = 0;
    const LOCAL_PEERPID: i32 = 2;
    let mut pid: i32 = 0;
    let mut length = size_of::<i32>() as u32;
    // SAFETY: the answer and its length are written to locals of the sizes given.
    let said = unsafe {
        getsockopt(
            fd,
            SOL_LOCAL,
            LOCAL_PEERPID,
            (&raw mut pid).cast(),
            &raw mut length,
        )
    };
    if said != 0 {
        return Err(io::Error::last_os_error());
    }
    u32::try_from(pid)
        .ok()
        .filter(|pid| *pid != 0)
        .ok_or_else(|| io::Error::other("the kernel named no process at the other end"))
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
fn peer_process(_fd: i32) -> io::Result<u32> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "this system does not say who is at the other end of a socket",
    ))
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
unsafe extern "C" {
    fn getsockopt(
        fd: i32,
        level: i32,
        name: i32,
        value: *mut std::ffi::c_void,
        length: *mut u32,
    ) -> i32;
}

impl Read for Socket {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        (&*self.stream).read(into)
    }
}

impl Write for Socket {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        (&*self.stream).write(data)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&*self.stream).flush()
    }
}

/// Reading on one thread while another writes, as a server answering a
/// client and signalling it at once does.
impl Read for &Socket {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        (&*self.stream).read(into)
    }
}

impl Write for &Socket {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        (&*self.stream).write(data)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&*self.stream).flush()
    }
}

/// A socket being listened on.
#[derive(Debug)]
pub struct Listener {
    listener: UnixListener,
}

impl Listener {
    pub fn bind(path: &Path) -> io::Result<Self> {
        let listener = UnixListener::bind(path).map_err(|reason| {
            io::Error::new(
                reason.kind(),
                format!("listen on {}: {reason}", path.display()),
            )
        })?;
        Ok(Self { listener })
    }

    pub fn accept(&self) -> io::Result<Socket> {
        let (stream, _) = self.listener.accept()?;
        Ok(Socket {
            stream: Arc::new(stream),
        })
    }

    /// Ends an accept another thread is waiting in, and every one after it.
    pub fn stop(&self) {
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn shutdown(socket: i32, how: i32) -> i32;
        }
        // SHUT_RDWR is 2 on every system this builds for. The descriptor is
        // the listener's own and still open.
        unsafe { shutdown(self.listener.as_raw_fd(), 2) };
    }
}

/// Ends a read another thread is waiting in.
#[derive(Debug, Clone)]
pub struct Interrupter {
    stream: Arc<UnixStream>,
}

impl Interrupter {
    pub fn interrupt(&self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::thread;

    use super::Socket;

    /// The round trip the driver makes: connect, ask, read the answer.
    #[test]
    fn a_socket_carries_a_request_and_its_answer() {
        let directory = std::env::temp_dir().join(format!("coax-socket-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a scratch directory can be made");
        let path = directory.join("http.sock");

        let listener = UnixListener::bind(&path).expect("the socket can be bound");
        let answering = thread::spawn(move || {
            let (mut accepted, _) = listener.accept().expect("a client arrives");
            let mut asked = [0u8; 5];
            accepted
                .read_exact(&mut asked)
                .expect("the request arrives");
            accepted.write_all(b"there").expect("the answer is written");
            asked
        });

        let mut socket = Socket::connect(&path.to_string_lossy()).expect("the daemon is reached");
        socket.write_all(b"hello").expect("the request is written");
        let mut answer = [0u8; 5];
        socket.read_exact(&mut answer).expect("the answer arrives");

        assert_eq!(&answer, b"there");
        assert_eq!(&answering.join().expect("the listener finishes"), b"hello");
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A path the address cannot hold is refused with the reason, rather than
    /// as `EINVAL` from `connect`.
    #[test]
    fn a_path_too_long_says_so() {
        let path = "/tmp/".to_owned() + &"x".repeat(200);
        let refused = Socket::connect(&path).expect_err("a path this long cannot be used");
        assert!(refused.to_string().contains("too long"), "{refused}");
    }
}

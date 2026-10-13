// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! A lock between processes of one user, which leaves nothing behind.
//!
//! The kernel lets go of it when the process does, however the process goes,
//! and what it is made of goes with it:
//!
//! - On Windows it is a named mutex, in a private namespace bounded by the
//!   user's SID. Nothing is written to disk. Only a process carrying that SID
//!   can make or open the namespace, so another user cannot take the name first
//!   and keep this one out, which a name in the global namespace would allow.
//!   A private namespace is not a session's, so a process in session 0 and one
//!   on the desktop find the same lock.
//! - Elsewhere it is `flock` on a file in the user's runtime directory. The
//!   holder removes the name before letting go, and whoever takes the lock
//!   checks afterwards that the name still leads to the file it locked; a
//!   process that was waiting on the file just removed starts again on the one
//!   there now. Only a crash leaves the file, which means nothing, in a
//!   directory the system clears.

use std::io;

/// A lock held. Dropping it lets go.
#[derive(Debug)]
pub struct Held {
    _inner: imp::Held,
}

/// Takes the lock called `name`, waiting for whoever has it when `wait` is set.
/// `Ok(None)` is somebody else holding it, and only comes back without `wait`.
///
/// The name is anything that tells this lock from others -- a path, a purpose;
/// it is hashed, so any characters do. The same name taken twice in one
/// process is two takers, the second refused or waiting like any other.
pub fn take(name: &str, wait: bool) -> io::Result<Option<Held>> {
    Ok(imp::take(&hash(name), wait)?.map(|inner| Held { _inner: inner }))
}

/// FNV-1a, which is enough to tell apart the handful of locks one user has and
/// is less than taking a dependency to do it.
fn hash(name: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use std::sync::OnceLock;
    use std::time::Duration;

    type Handle = *mut c_void;

    const ERROR_ALREADY_EXISTS: i32 = 183;
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_USER_CLASS: i32 = 1;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        // A constant that stands for this process; it needs no closing.
        safe fn GetCurrentProcess() -> Handle;
        fn CreateBoundaryDescriptorW(name: *const u16, flags: u32) -> Handle;
        fn AddSIDToBoundaryDescriptor(boundary: *mut Handle, sid: *mut c_void) -> i32;
        fn DeleteBoundaryDescriptor(boundary: Handle);
        fn CreatePrivateNamespaceW(
            attributes: *mut c_void,
            boundary: Handle,
            alias: *const u16,
        ) -> Handle;
        fn OpenPrivateNamespaceW(boundary: Handle, alias: *const u16) -> Handle;
        fn CreateMutexW(attributes: *mut c_void, owned: i32, name: *const u16) -> Handle;
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
        fn GetTokenInformation(
            token: Handle,
            class: i32,
            information: *mut c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
    }

    /// The prefix the mutexes are named under, and the boundary's name.
    const ALIAS: &str = "coax";

    /// How often a waiter looks again. Holding is having the mutex exist, so
    /// there is nothing to wait on but the name going away.
    const LOOK_AGAIN: Duration = Duration::from_millis(20);

    /// A handle is the process's, not a thread's: the mutex is never owned, so
    /// it can be let go of from anywhere.
    #[derive(Debug)]
    pub struct Held {
        _mutex: OwnedHandle,
    }

    /// A handle a call has just given this process, closed when dropped.
    fn owned(handle: Handle) -> OwnedHandle {
        // SAFETY: only called on a handle just returned and not null, which
        // nothing else holds.
        unsafe { OwnedHandle::from_raw_handle(handle) }
    }

    /// Holding is having made the mutex rather than owning it. A Windows mutex
    /// is owned by a thread and owned again by the same thread when asked
    /// twice, and the callers of this include libraries a host loads twice and
    /// calls from one thread.
    pub fn take(name: &str, wait: bool) -> io::Result<Option<Held>> {
        namespace()?;
        let wide = wide(&format!("{ALIAS}\\{name}"));
        loop {
            // SAFETY: the name is a terminated wide string that outlives the call.
            let handle = unsafe { CreateMutexW(std::ptr::null_mut(), 0, wide.as_ptr()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let existed = io::Error::last_os_error().raw_os_error() == Some(ERROR_ALREADY_EXISTS);
            let handle = owned(handle);
            if !existed {
                return Ok(Some(Held { _mutex: handle }));
            }
            drop(handle);
            if !wait {
                return Ok(None);
            }
            std::thread::sleep(LOOK_AGAIN);
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The namespace, made or opened once and kept for the life of the
    /// process: a process may have one namespace of an alias open.
    fn namespace() -> io::Result<()> {
        static OPENED: OnceLock<Result<usize, i32>> = OnceLock::new();
        match OPENED
            .get_or_init(|| open_namespace().map_err(|why| why.raw_os_error().unwrap_or(-1)))
        {
            Ok(_) => Ok(()),
            Err(code) => Err(io::Error::from_raw_os_error(*code)),
        }
    }

    fn open_namespace() -> io::Result<usize> {
        let mut token: Handle = std::ptr::null_mut();
        // SAFETY: the token is written to a local.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = owned(token);
        let token_raw = std::os::windows::io::AsRawHandle::as_raw_handle(&token);
        let mut needed = 0u32;
        // SAFETY: asks only for the size, into a local.
        unsafe {
            GetTokenInformation(
                token_raw,
                TOKEN_USER_CLASS,
                std::ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        // Pointer-aligned: the answer starts with a pointer to the SID inside it.
        let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>()).max(1)];
        // SAFETY: the buffer is as long as the length given.
        let read = unsafe {
            GetTokenInformation(
                token_raw,
                TOKEN_USER_CLASS,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * size_of::<usize>()) as u32,
                &mut needed,
            )
        };
        drop(token);
        if read == 0 {
            return Err(io::Error::last_os_error());
        }
        // TOKEN_USER begins with the SID's address.
        let sid = buffer[0] as *mut c_void;

        let alias = wide(ALIAS);
        // SAFETY: the name is a terminated wide string that outlives the call.
        let mut boundary = unsafe { CreateBoundaryDescriptorW(alias.as_ptr(), 0) };
        if boundary.is_null() {
            return Err(io::Error::last_os_error());
        }
        // Adding a SID moves the descriptor, so it is freed by whatever it is
        // last. The namespace keeps a copy of its own.
        struct Boundary(*mut Handle);
        impl Drop for Boundary {
            fn drop(&mut self) {
                // SAFETY: the descriptor is made above and freed only here.
                unsafe { DeleteBoundaryDescriptor(*self.0) };
            }
        }
        let _free = Boundary(&mut boundary);
        // SAFETY: the SID points into `buffer`, which is still alive.
        if unsafe { AddSIDToBoundaryDescriptor(&mut boundary, sid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the boundary and the alias are alive for the call.
        let made =
            unsafe { CreatePrivateNamespaceW(std::ptr::null_mut(), boundary, alias.as_ptr()) };
        if !made.is_null() {
            return Ok(made as usize);
        }
        let why = io::Error::last_os_error();
        if why.raw_os_error() != Some(ERROR_ALREADY_EXISTS) {
            return Err(why);
        }
        // SAFETY: the boundary and the alias are alive for the call.
        let opened = unsafe { OpenPrivateNamespaceW(boundary, alias.as_ptr()) };
        if opened.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(opened as usize)
    }
}

#[cfg(unix)]
mod imp {
    use std::fs::{File, TryLockError};
    use std::io;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};

    const O_NOFOLLOW_LINUX: i32 = 0o400000;
    const O_NOFOLLOW_BSD: i32 = 0x0100;

    #[derive(Debug)]
    pub struct Held {
        _file: File,
        pub(super) path: PathBuf,
    }

    impl Drop for Held {
        fn drop(&mut self) {
            // The name goes first, while the lock is still held: a waiter that
            // gets the lock next finds the name gone and starts again, rather
            // than holding the lock on a file nobody else can reach. Closing
            // the file afterwards is letting go.
            let _ = std::fs::remove_file(&self.path);
        }
    }

    pub fn take(name: &str, wait: bool) -> io::Result<Option<Held>> {
        let path = directory().join(file_name(name));
        loop {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .mode(0o600)
                .custom_flags(if cfg!(target_os = "linux") {
                    O_NOFOLLOW_LINUX
                } else {
                    O_NOFOLLOW_BSD
                })
                .open(&path)?;
            // Both are flock on the whole file.
            if wait {
                file.lock()?;
            } else {
                match file.try_lock() {
                    Ok(()) => {}
                    Err(TryLockError::WouldBlock) => return Ok(None),
                    Err(TryLockError::Error(why)) => return Err(why),
                }
            }
            if still_named(&file, &path) {
                return Ok(Some(Held { _file: file, path }));
            }
        }
    }

    /// Whether the name still leads to the file opened under it, which it does
    /// not once the holder before has removed it.
    fn still_named(file: &File, path: &Path) -> bool {
        let (Ok(opened), Ok(named)) = (file.metadata(), std::fs::symlink_metadata(path)) else {
            return false;
        };
        named.file_type().is_file() && named.dev() == opened.dev() && named.ino() == opened.ino()
    }

    /// The user's own runtime directory where the system has one, which it
    /// clears and nobody else can write to; else the temporary one, which on
    /// macOS is the user's own too.
    fn directory() -> PathBuf {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|directory| directory.is_absolute() && directory.is_dir())
            .unwrap_or_else(std::env::temp_dir)
    }

    /// Whose it is is part of the name: a temporary directory may be shared,
    /// and another user's file is one this program may not open at all.
    fn file_name(name: &str) -> String {
        format!("coax-{}-{name}.lock", crate::daemon::user_id())
    }
}

#[cfg(not(any(windows, unix)))]
mod imp {
    use std::io;

    /// Nothing to take it with, so everybody has it.
    #[derive(Debug)]
    pub struct Held;

    pub fn take(_name: &str, _wait: bool) -> io::Result<Option<Held>> {
        Ok(Some(Held))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lock_is_held_by_one_taker_at_a_time() {
        let name = format!("lock-test-{}", std::process::id());
        let first = take(&name, false).unwrap().expect("nobody had it");
        assert!(take(&name, false).unwrap().is_none(), "taken twice");
        drop(first);
        let again = take(&name, false).unwrap();
        assert!(again.is_some(), "let go of, and still held");
    }

    #[test]
    fn a_waiter_gets_the_lock_when_it_is_let_go_of() {
        let name = format!("lock-wait-{}", std::process::id());
        let first = take(&name, false).unwrap().expect("nobody had it");
        let waiter = std::thread::spawn({
            let name = name.clone();
            move || take(&name, true).unwrap().is_some()
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        drop(first);
        assert!(waiter.join().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn nothing_is_left_once_let_go_of() {
        let name = format!("lock-left-{}", std::process::id());
        let held = take(&name, false).unwrap().expect("nobody had it");
        let path = held._inner.path.clone();
        assert!(path.exists());
        drop(held);
        assert!(!path.exists(), "the lock file was left behind");
    }
}

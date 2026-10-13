// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! What can be learned of another process from its number: where its program
//! is.
//!
//! Used on the daemon, whose number the socket gives ([`crate::daemon::Daemon::serving`]).
//! A player that keeps its own copy of the daemon has to tell that copy apart
//! from one somebody runs from elsewhere before it replaces a file or stops
//! using it.

use std::io;
use std::path::PathBuf;

/// Where the program of a running process is.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn executable_of(pid: u32) -> io::Result<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
}

/// Where the program of a running process is.
#[cfg(target_os = "macos")]
pub fn executable_of(pid: u32) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    // PROC_PIDPATHINFO_MAXSIZE, from libproc.h.
    const ROOM: usize = 4 * 1024;
    unsafe extern "C" {
        fn proc_pidpath(pid: i32, buffer: *mut std::ffi::c_void, size: u32) -> i32;
    }
    let pid = i32::try_from(pid).map_err(io::Error::other)?;
    let mut buffer = vec![0_u8; ROOM];
    let length = unsafe { proc_pidpath(pid, buffer.as_mut_ptr().cast(), ROOM as u32) };
    if length <= 0 {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(length as usize);
    Ok(PathBuf::from(std::ffi::OsString::from_vec(buffer)))
}

/// Where the program of a running process is.
#[cfg(windows)]
pub fn executable_of(pid: u32) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    // Long paths can be longer than MAX_PATH; this is the most Windows allows.
    const ROOM: u32 = 32 * 1024;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn QueryFullProcessImageNameW(
            process: isize,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut name = vec![0_u16; ROOM as usize];
    let mut size = ROOM;
    let named = unsafe { QueryFullProcessImageNameW(process, 0, name.as_mut_ptr(), &raw mut size) };
    let failure = io::Error::last_os_error();
    unsafe { CloseHandle(process) };
    if named == 0 {
        return Err(failure);
    }
    name.truncate(size as usize);
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&name)))
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    windows
)))]
pub fn executable_of(_pid: u32) -> io::Result<PathBuf> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "this system does not say where a process's program is",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn this_process_knows_where_its_program_is() {
        let here = executable_of(std::process::id()).expect("its own program");
        let said = std::env::current_exe().expect("the standard library says");
        assert_eq!(
            here.canonicalize().expect("exists"),
            said.canonicalize().expect("exists")
        );
    }
}

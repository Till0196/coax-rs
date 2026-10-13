// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Starting the daemon when nothing else has.
//!
//! A viewer opens their television software; they do not first start a service.
//! So the driver does what the command line tool does: if nothing answers,
//! start the daemon and wait for it to. The daemon takes itself down again when
//! it has been idle for a while.
//!
//! Two drivers starting at once is safe whatever happens: the daemon's
//! protection against being run twice is the socket bind itself. It is still
//! worth not racing -- a television host loads a driver and a card library into
//! one process, and a daemon on its way down holds its claim on the socket
//! until it has given every tuner back, so a second one started in that moment
//! exits without having served anything. So [`Claim`] settles who starts one,
//! and everybody else waits for the socket to answer.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where to look for the daemon.
#[derive(Debug, Clone, Default)]
pub struct Launcher {
    /// A path given in the settings, which is looked at before anything else.
    pub named: Option<PathBuf>,
    /// The directory the library was loaded from. A viewer who unpacked the
    /// daemon beside the driver expects that to be enough.
    pub beside: Option<PathBuf>,
}

/// What the daemon's executable is called.
#[cfg(windows)]
const DAEMON: &str = "coaxd.exe";
#[cfg(not(windows))]
const DAEMON: &str = "coaxd";

/// The environment variable the command line tool reads, honoured here so that
/// a machine set up for one is set up for the other.
const OVERRIDE: &str = "COAXD";

impl Launcher {
    /// Finds the daemon, in the order the command line tool looks: what the
    /// settings name, then the environment, then beside this library, then the
    /// search path.
    ///
    /// A relative path in the settings is taken from the library's directory,
    /// never from the host's current directory: the host decides that, and a
    /// program that happens to be lying where the host was started from is not
    /// one the settings meant. A relative path in the environment is passed
    /// over for the same reason.
    pub fn find(&self) -> Option<PathBuf> {
        if let Some(named) = &self.named {
            let named = if named.is_absolute() {
                Some(named.clone())
            } else {
                self.beside.as_ref().map(|beside| beside.join(named))
            };
            if let Some(named) = named.filter(|named| named.is_file()) {
                return Some(named);
            }
        }
        if let Some(from_environment) = std::env::var_os(OVERRIDE) {
            let path = PathBuf::from(from_environment);
            if path.is_absolute() && path.is_file() {
                return Some(path);
            }
        }
        if let Some(beside) = &self.beside {
            let path = beside.join(DAEMON);
            if path.is_file() {
                return Some(path);
            }
        }
        on_the_path(DAEMON)
    }

    /// Starts the daemon on a given socket, detached.
    ///
    /// The socket is passed on rather than left to the daemon's own default, as
    /// `coax` does: without it, a settings file naming a socket anywhere but
    /// the usual place starts a daemon that listens somewhere else. Only the
    /// one socket -- the daemon serves the card over the same HTTP as
    /// everything else.
    ///
    /// Detached because the daemon outlives the request that needed it, and
    /// because a console appearing in front of a viewer's television is not
    /// something to explain away. Its output goes nowhere: it has its own log.
    pub fn start(&self, socket: &str) -> io::Result<PathBuf> {
        let daemon = self.find().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("no {DAEMON} to start"))
        })?;
        let mut command = Command::new(&daemon);
        command.arg("--socket").arg(socket);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        detach(&mut command);
        let child = command.spawn()?;
        reap(child);
        Ok(daemon)
    }
}

/// Nothing to do on Windows: an exited process leaves nothing behind but the
/// handle, and dropping the child closes that. No thread either, which matters
/// here more than anywhere -- this runs inside a library the host may unload
/// long before the daemon exits, and a thread still waiting would come back to
/// code that is no longer there.
#[cfg(windows)]
fn reap(child: std::process::Child) {
    drop(child);
}

/// Elsewhere a daemon that later finds itself idle and exits would stay behind
/// as a zombie, one for every time it was started, so it is waited for on a
/// thread of its own.
#[cfg(not(windows))]
fn reap(mut child: std::process::Child) {
    let _ = std::thread::Builder::new()
        .name(String::from("coaxd-reaper"))
        .spawn(move || child.wait());
}

/// The right to be the one starting a daemon on a socket.
///
/// Held by whoever is starting one and released when they have finished
/// waiting, whether or not it worked. Nobody waits for it: a caller that cannot
/// take it wants a daemon running, and what says so is the socket answering. So
/// a caller that fails to take it polls the socket like everyone else and tries
/// again.
///
/// Named after the socket, so two daemons on one machine do not hold each other
/// up.
#[derive(Debug)]
pub struct Claim {
    /// Held for as long as the claim is, and never read: letting go of it is
    /// the whole of what it does. Nothing when the lock could not be used at
    /// all, in which case the socket bind is what settles it.
    _held: Option<crate::lock::Held>,
}

impl Claim {
    /// Takes the claim, or nothing when somebody already has it.
    pub fn take(socket: &str) -> Option<Self> {
        match crate::lock::take(&format!("coaxd-start-{}", claim_name(socket)), false) {
            Ok(Some(held)) => Some(Self { _held: Some(held) }),
            Ok(None) => None,
            // A lock that cannot be used at all says nothing about anybody
            // starting a daemon. Waiting on it would only wait out the
            // deadline, so go ahead without one: two daemons racing is safe,
            // just untidy.
            Err(_) => Some(Self { _held: None }),
        }
    }
}

/// The name both halves of a machine work out the same way. Folded to lower
/// case first, because the two names of one file on Windows have to come to one
/// claim. Hashed, so that whatever the path holds the name is plain.
fn claim_name(socket: &str) -> String {
    // FNV-1a, which is enough to tell apart the handful of sockets one machine
    // has and is less than taking a dependency to do it.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in socket.to_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Keeps the daemon out of the host's console and out of its lifetime.
#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    /// A console of its own that is never shown. Not `DETACHED_PROCESS`, which
    /// leaves the daemon with no console at all: measured here, one started
    /// that way exited before it had opened its socket.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn detach(_command: &mut Command) {}

/// Looks down the search path, as the shell would, but only at the absolute
/// entries: a relative one means the host's current directory, which is not a
/// place this library chose to run programs from.
fn on_the_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

/// The name of the daemon's executable, for the messages that mention it.
pub fn daemon_file_name() -> OsString {
    OsString::from(DAEMON)
}

/// Whether a path names something that can be run.
pub fn is_runnable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{Claim, DAEMON, Launcher, claim_name};

    /// One socket is one claim, and two sockets are two: a machine may run a
    /// daemon of its own beside one somebody's settings file names, and
    /// starting either must not wait on the other.
    #[test]
    fn a_claim_is_named_after_its_socket() {
        assert_eq!(
            claim_name("/run/coaxd/http.sock"),
            claim_name("/run/coaxd/http.sock")
        );
        assert_ne!(
            claim_name("/run/coaxd/http.sock"),
            claim_name("/run/coaxd/other.sock")
        );
        // Windows spells one file several ways, and they are one claim.
        assert_eq!(
            claim_name(r"C:\Users\a\AppData\Local\coaxd\http.sock"),
            claim_name(r"c:\users\a\appdata\local\coaxd\http.sock")
        );
        // A name a mutex and a file name will both carry, whatever the path
        // held.
        let name = claim_name("/run/coaxd/ソケット .sock");
        assert!(name.chars().all(|letter| letter.is_ascii_hexdigit()));
    }

    /// The whole of what the claim is for: the second caller does not start a
    /// daemon, and does once the first has let go.
    #[test]
    fn only_one_holder_at_a_time() {
        let socket = "/run/coaxd/one-holder-at-a-time.sock";
        let first = Claim::take(socket).expect("nobody holds this one");
        assert!(
            Claim::take(socket).is_none(),
            "two callers were both told to start a daemon"
        );
        drop(first);
        assert!(
            Claim::take(socket).is_some(),
            "the claim was not let go of, so nobody could start one after"
        );
    }

    /// Two sockets do not hold each other up.
    #[test]
    fn a_claim_on_one_socket_leaves_another_alone() {
        let held = Claim::take("/run/coaxd/leaves-another-alone.sock").expect("free");
        assert!(Claim::take("/run/coaxd/a-different-one.sock").is_some());
        drop(held);
    }

    /// A directory of this test's own, cleaned up by the caller.
    fn scratch(name: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!("bondriver-coax-{name}"));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("a scratch directory can be made");
        directory
    }

    #[test]
    fn the_daemon_beside_the_library_is_found() {
        let directory = scratch("beside");
        let daemon = directory.join(DAEMON);
        fs::write(&daemon, b"not really a daemon").expect("the file can be written");

        let launcher = Launcher {
            named: None,
            beside: Some(directory.clone()),
        };
        assert_eq!(launcher.find(), Some(daemon));
        let _ = fs::remove_dir_all(directory);
    }

    /// What the settings name comes first: a viewer who wrote a path down meant
    /// that one, whatever else is lying around.
    #[test]
    fn a_named_daemon_wins() {
        let directory = scratch("named");
        let beside = directory.join(DAEMON);
        let named = directory.join("elsewhere.exe");
        fs::write(&beside, b"one").expect("the file can be written");
        fs::write(&named, b"another").expect("the file can be written");

        let launcher = Launcher {
            named: Some(named.clone()),
            beside: Some(directory.clone()),
        };
        assert_eq!(launcher.find(), Some(named));
        let _ = fs::remove_dir_all(directory);
    }

    /// A path that names nothing is not an answer, so the search goes on.
    #[test]
    fn a_named_daemon_that_is_not_there_is_passed_over() {
        let directory = scratch("missing");
        let beside = directory.join(DAEMON);
        fs::write(&beside, b"one").expect("the file can be written");

        let launcher = Launcher {
            named: Some(directory.join("nothing-here.exe")),
            beside: Some(directory.clone()),
        };
        assert_eq!(launcher.find(), Some(beside));
        let _ = fs::remove_dir_all(directory);
    }

    /// A relative path in the settings is read from the library's directory,
    /// not from wherever the host happened to be started.
    #[test]
    fn a_relative_named_daemon_is_taken_from_beside_the_library() {
        let directory = scratch("relative");
        fs::create_dir_all(directory.join("bin")).expect("a directory can be made");
        let named = directory.join("bin").join("elsewhere.exe");
        fs::write(&named, b"another").expect("the file can be written");

        let launcher = Launcher {
            named: Some(std::path::Path::new("bin").join("elsewhere.exe")),
            beside: Some(directory.clone()),
        };
        assert_eq!(launcher.find(), Some(named));
        let _ = fs::remove_dir_all(directory);
    }
}

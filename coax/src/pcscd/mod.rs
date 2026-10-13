// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! A PC/SC socket for the card slots a daemon has.
//!
//! pcsc-lite's client library reaches its daemon over a Unix socket, and says
//! where with `PCSCLITE_CSOCK_NAME`. Speaking that daemon's protocol here lets a
//! decoder that links pcsc-lite find the card interfaces built into tuner boards
//! by being pointed at this socket, with nothing installed or replaced. The
//! protocol is pcsc-lite's internal interface, not a published one; it is
//! versioned and both ends check the version first, so a mismatch is a clean
//! refusal rather than a misread structure.
//!
//! The socket belongs to the machine running the decoder, not to the one
//! holding the card: the protocol is C structures in the host's own layout,
//! sound between two programs on one machine and nothing to send over a
//! network. So the card is reached over the daemon's HTTP like everything else,
//! and the daemon may be anywhere.
//!
//! Windows has `AF_UNIX` too, and a pcsc-lite client built for it finds the
//! socket the same way.
//!
//! This is the same thing as `coax pcsc`, for a program that wants to serve the
//! socket itself rather than start another.

pub mod wire;

mod daemoncard;
pub mod server;
pub mod table;

pub use serving::{Serving, default_socket, serve};

#[cfg(test)]
mod tests;

mod serving {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::daemoncard::DaemonCards;
    use super::server::Server;
    use super::table::{POLL_INTERVAL, Table};
    use crate::card::Cards;
    use crate::daemon::Endpoint;
    use crate::daemon::unix_socket::{Listener, Socket};

    /// Where the socket goes when nobody said: pcsc-lite's own variable if it
    /// is set, so a machine already pointed somewhere stays pointed there, and
    /// otherwise beside the daemon's socket, as `coax pcsc` puts it.
    pub fn default_socket() -> PathBuf {
        if let Some(named) = std::env::var_os("PCSCLITE_CSOCK_NAME").filter(|v| !v.is_empty()) {
            return PathBuf::from(named);
        }
        PathBuf::from(crate::daemon::runtime_directory()).join("pcscd.comm")
    }

    /// A socket being served. Dropping it stops accepting and removes the
    /// socket; clients already connected finish on their own.
    #[derive(Debug)]
    pub struct Serving {
        path: PathBuf,
        server: Server,
        table: Table,
        listener: Arc<Listener>,
    }

    impl Serving {
        pub fn path(&self) -> &Path {
            &self.path
        }

        /// How many clients are attached.
        pub fn connections(&self) -> usize {
            self.server.connections()
        }
    }

    impl Drop for Serving {
        fn drop(&mut self) {
            self.table.close();
            self.listener.stop();
            // This process's to clean up: a socket file left behind is one the
            // next run has to decide whether to remove.
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Serves the daemon's card slots on a socket, on a thread of its own.
    ///
    /// Refuses a socket a live server is already on (a `coax pcsc`, or another
    /// program doing this): the caller can use that one instead. A socket left
    /// behind by a process that was killed is removed.
    pub fn serve(
        endpoint: Endpoint,
        path: &Path,
        report: impl Fn(&str) + Send + Sync + 'static,
    ) -> io::Result<Serving> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if Socket::connect(&path.to_string_lossy()).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("something is already serving PC/SC on {}", path.display()),
            ));
        }
        match std::fs::remove_file(path) {
            Err(reason) if reason.kind() != io::ErrorKind::NotFound => return Err(reason),
            _ => {}
        }
        let listener = Arc::new(Listener::bind(path)?);
        // The user's own: the decoders that connect run as the same user.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }

        let cards = Cards::new(endpoint);
        let table = Table::new(Box::new(DaemonCards::new(cards.clone())), POLL_INTERVAL);
        let server = Server::new(
            Box::new(DaemonCards::new(cards)),
            table.clone(),
            Box::new(report),
        );
        let accepting = Arc::clone(&listener);
        let serving = server.clone();
        std::thread::Builder::new()
            .name(String::from("pcscd"))
            .spawn(move || {
                let _ = serving.serve(&accepting);
            })?;
        Ok(Serving {
            path: path.to_owned(),
            server,
            table,
            listener,
        })
    }
}

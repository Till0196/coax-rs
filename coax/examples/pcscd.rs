// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Serves a daemon's card slots on a PC/SC socket until interrupted, the way
//! `coax pcsc` does.
//!
//! ```sh
//! cargo run -p coax --example pcscd -- [<daemon>] [<socket>]
//! PCSCLITE_CSOCK_NAME=<socket> pcsc_scan
//! ```

fn main() {
    let mut arguments = std::env::args().skip(1);
    let endpoint = arguments
        .next()
        .map_or_else(coax::daemon::Endpoint::default_endpoint, |written| {
            coax::daemon::Endpoint::parse(&written)
        });
    let path = arguments
        .next()
        .map_or_else(coax::pcscd::default_socket, std::path::PathBuf::from);
    match coax::pcscd::serve(endpoint, &path, |said| eprintln!("{said}")) {
        Ok(serving) => {
            println!("serving PC/SC on {}", serving.path().display());
            loop {
                std::thread::park();
            }
        }
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(1);
        }
    }
}

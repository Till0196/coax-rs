// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Reads one carrier the two ways a client can ask for it, and says what came off it: for
//! finding out whether a board minds being tuned and then read a moment later, rather
//! than tuned and read in one request. Not part of the driver. Run it beside a daemon:
//!
//!     flow <tuner> <frequency> [oneshot|split|both]
//!     flow <tuner> <frequency>,<frequency>,... walk

use std::time::{Duration, Instant};

use coax::daemon::{Daemon, Endpoint};

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let tuner = arguments.get(1).cloned().unwrap_or_default();
    let frequency = arguments.get(2).cloned().unwrap_or_default();
    let which = arguments
        .get(3)
        .cloned()
        .unwrap_or_else(|| "both".to_owned());
    let host = std::env::var("COAX_HOST").unwrap_or_default();
    let daemon = Daemon::new(Endpoint::parse(&host));
    let properties = vec![
        ("DELIVERY_SYSTEM", "ISDBT".to_owned()),
        ("FREQUENCY", frequency.clone()),
    ];
    let read_for = Duration::from_secs(
        std::env::var("COAX_FLOW_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(5),
    );

    // Watching a tuner somebody else is using: what the daemon's own analyser
    // makes of the stream, second by second, without taking the tuner.
    if which == "status" {
        let mut before: Option<(u64, u64, u64, u64, u64)> = None;
        for _ in 0..std::env::var("COAX_FLOW_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(20u32)
        {
            match daemon.seen(&tuner) {
                Ok(seen) => {
                    let now = (
                        seen.bytes,
                        seen.packets,
                        seen.errors,
                        seen.discontinuities,
                        seen.dropped,
                    );
                    let (bytes, packets, errors, gaps, dropped) = match before {
                        Some(was) => (
                            now.0 - was.0,
                            now.1 - was.1,
                            now.2 - was.2,
                            now.3 - was.3,
                            now.4 - was.4,
                        ),
                        None => (0, 0, 0, 0, 0),
                    };
                    before = Some(now);
                    println!(
                        "{:.1} Mbps, {packets} packets, errors {errors}, discontinuities {gaps}, dropped {dropped}, cnr {:?}, sync losses {}",
                        bytes as f32 * 8.0 / 1_000_000.0,
                        seen.cnr,
                        seen.sync_losses,
                    );
                }
                Err(reason) => println!("status: {reason}"),
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        return;
    }

    // A walk: several carriers one after another on one held tuner, which is
    // what a sweep does and what a single carrier does not exercise.
    if which.starts_with("walk") {
        // How the walk treats the connection: "walk" reads for its own window
        // and moves on, "walk-finish" reads until the daemon ends the stream,
        // "walk-fresh" lets go of the tuner between carriers.
        let finish = which == "walk-finish";
        let fresh = which == "walk-fresh";
        let mut lease = daemon.hold(if tuner.is_empty() { None } else { Some(&tuner) });
        for (round, carrier) in frequency.split(',').enumerate() {
            let properties = vec![
                ("DELIVERY_SYSTEM", "ISDBT".to_owned()),
                ("FREQUENCY", carrier.to_owned()),
            ];
            if fresh {
                lease.release();
            }
            let started = Instant::now();
            let tuned = match lease.tune(&properties) {
                Ok(tuned) => tuned,
                Err(reason) => {
                    println!("{round}: {carrier}: refused: {reason}");
                    continue;
                }
            };
            let mut locked = tuned.locked;
            while !locked && started.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(250));
                if let Some(named) = lease.tuner().map(str::to_owned)
                    && let Ok(seen) = daemon.seen(&named)
                {
                    locked = seen.locked.unwrap_or(false);
                }
            }
            if !locked {
                println!(
                    "{round}: {carrier}: no lock after {:.1}s",
                    started.elapsed().as_secs_f32()
                );
                continue;
            }
            let reading = Instant::now();
            let mut bytes = 0usize;
            match lease.stream(
                &properties,
                Some(128 * 1024 * 1024),
                Some(read_for),
                Some(Duration::from_millis(500)),
            ) {
                Ok(mut answer) => {
                    let mut buffer = vec![0u8; 32 * 1024];
                    let until = read_for + Duration::from_secs(if finish { 5 } else { 0 });
                    // What arrived in each second, to tell a carrier that is
                    // slow to start from one that is slow throughout.
                    let mut each_second: Vec<usize> = Vec::new();
                    let mut second = 0usize;
                    let mut so_far = 0usize;
                    while reading.elapsed() < until {
                        match answer.read_body(&mut buffer) {
                            Ok(0) => break,
                            Ok(read) => bytes += read,
                            Err(reason) if timed_out(&reason) => {}
                            Err(_) => break,
                        }
                        while reading.elapsed().as_secs() as usize > second {
                            each_second.push(bytes - so_far);
                            so_far = bytes;
                            second += 1;
                            // Asked while the stream is running: the analyser
                            // belongs to it and has nothing to say once it has
                            // been closed.
                            if second == 3
                                && std::env::var("COAX_FLOW_NO_STATUS").is_err()
                                && let Some(named) = lease.tuner().map(str::to_owned)
                                && let Ok(seen) = daemon.seen(&named)
                            {
                                println!(
                                    "{round}: {carrier}: while running: cnr={:?} format={} bytes={} packets={} errors={} discontinuities={} sync_losses={} dropped={}",
                                    seen.cnr,
                                    seen.format,
                                    seen.bytes,
                                    seen.packets,
                                    seen.errors,
                                    seen.discontinuities,
                                    seen.sync_losses,
                                    seen.dropped,
                                );
                            }
                        }
                        if !finish && reading.elapsed() >= read_for {
                            break;
                        }
                    }
                    if false && let Ok(seen) = daemon.seen("") {
                        println!(
                            "{round}: {carrier}: cnr={:?} strength={:?} format={} bytes={} packets={} errors={} discontinuities={} sync_losses={} dropped={}",
                            seen.cnr,
                            seen.strength,
                            seen.format,
                            seen.bytes,
                            seen.packets,
                            seen.errors,
                            seen.discontinuities,
                            seen.sync_losses,
                            seen.dropped,
                        );
                    }
                    let per_second: Vec<String> = each_second
                        .iter()
                        .map(|count| format!("{:.1}", *count as f32 * 8.0 / 1_000_000.0))
                        .collect();
                    let finished = answer.finished();
                    lease.recover(answer);
                    println!(
                        "{round}: {carrier}: {bytes} bytes in {:.1}s ({:.1} Mbps) [{}], ended {finished}, kept {}",
                        reading.elapsed().as_secs_f32(),
                        rate(bytes, reading.elapsed()),
                        per_second.join(" "),
                        lease.tuner().is_some(),
                    );
                    let _ = (started, finished);
                    #[allow(unreachable_code)]
                    if false {
                        println!(
                            "{round}: {carrier}: locked in {:.2}s, {bytes} bytes in {:.1}s ({:.1} Mbps), stream ended {finished}, kept {}",
                            started.elapsed().as_secs_f32(),
                            reading.elapsed().as_secs_f32(),
                            rate(bytes, reading.elapsed()),
                            lease.tuner().is_some(),
                        );
                    }
                }
                Err(reason) => println!("{round}: {carrier}: stream refused: {reason}"),
            }
        }
        return;
    }

    if which != "split" {
        let started = Instant::now();
        let mut bytes = 0usize;
        match daemon.stream_waiting(&properties, Some(Duration::from_millis(500))) {
            Ok((mut answer, _interrupter)) => {
                let mut buffer = vec![0u8; 32 * 1024];
                while started.elapsed() < read_for {
                    match answer.read_body(&mut buffer) {
                        Ok(0) => break,
                        Ok(read) => bytes += read,
                        Err(reason) if timed_out(&reason) => {}
                        Err(_) => break,
                    }
                }
                println!(
                    "one request: {bytes} bytes in {:.1}s ({:.1} Mbps)",
                    started.elapsed().as_secs_f32(),
                    rate(bytes, started.elapsed())
                );
            }
            Err(reason) => println!("one request: refused: {reason}"),
        }
    }

    if which != "oneshot" {
        let mut lease = daemon.hold(if tuner.is_empty() { None } else { Some(&tuner) });
        let started = Instant::now();
        let tuned = match lease.tune(&properties) {
            Ok(tuned) => tuned,
            Err(reason) => {
                println!("tune then read: refused: {reason}");
                return;
            }
        };
        println!(
            "tune then read: tuned in {:.2}s, locked={}, tuner={}",
            started.elapsed().as_secs_f32(),
            tuned.locked,
            tuned.tuner
        );
        let mut locked = tuned.locked;
        while !locked && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(250));
            if let Some(named) = lease.tuner().map(str::to_owned)
                && let Ok(seen) = daemon.seen(&named)
            {
                locked = seen.locked.unwrap_or(false);
            }
        }
        println!(
            "tune then read: locked={locked} after {:.2}s",
            started.elapsed().as_secs_f32()
        );
        let reading = Instant::now();
        let mut bytes = 0usize;
        match lease.stream(
            &properties,
            Some(128 * 1024 * 1024),
            Some(read_for),
            Some(Duration::from_millis(500)),
        ) {
            Ok(mut answer) => {
                let mut buffer = vec![0u8; 32 * 1024];
                while reading.elapsed() < read_for {
                    match answer.read_body(&mut buffer) {
                        Ok(0) => break,
                        Ok(read) => bytes += read,
                        Err(reason) if timed_out(&reason) => {}
                        Err(_) => break,
                    }
                }
                println!(
                    "tune then read: {bytes} bytes in {:.1}s ({:.1} Mbps)",
                    reading.elapsed().as_secs_f32(),
                    rate(bytes, reading.elapsed())
                );
            }
            Err(reason) => println!("tune then read: refused: {reason}"),
        }
    }
}

fn timed_out(reason: &std::io::Error) -> bool {
    matches!(
        reason.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

fn rate(bytes: usize, took: Duration) -> f32 {
    (bytes as f32 * 8.0) / took.as_secs_f32().max(0.001) / 1_000_000.0
}

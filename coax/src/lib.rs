// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! A client of `coaxd`, which owns the tuner hardware.
//!
//! Everything here is about reaching that daemon and understanding what it
//! says: where it is and how to ask it, the card slots it publishes, and the
//! vocabulary its answers are written in -- delivery systems, the Japanese
//! channel plan, which platform a stream belongs to.
//!
//! Nothing here knows what the asking is for; see `coax_driver` for the
//! BonDriver's share of it. What the streams themselves are made of is not here
//! either: that is `mpegts`, `mmttlv` and `isdb`, which know nothing of this
//! crate.

pub mod card;
pub mod daemon;
pub mod delivery;
pub mod group;
pub mod http;
pub mod json;
pub mod launch;
pub mod lock;
pub mod pcscd;
pub mod plan;
pub mod process;
pub mod scan;
pub mod tuning;

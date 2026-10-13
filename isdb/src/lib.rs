// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! Japanese digital broadcasting: what the two transmission layers underneath
//! carry once the bytes have been put back together.
//!
//! The service information of both layers -- the tables of a transport stream
//! and the MMT signalling of the advanced services -- the character encoding
//! those tables spell names in, and TSMF, which is how a cable carries several
//! transport streams on one carrier.
//!
//! `mpegts` and `mmttlv` are underneath and are not this: they are
//! international standards, and someone who wants a transport stream taken
//! apart should not have to take Japanese broadcasting with it.

pub mod arib;
pub mod bond;
pub mod bxml;
pub mod caption;
pub mod datatables;
pub mod descriptors;
pub mod logo;
pub mod mh;
pub mod mhcaption;
pub mod mhdata;
pub mod mmttables;
pub mod nit;
pub mod nitdescriptors;
pub mod sdt;
pub mod svg;
pub mod tstables;
pub mod ttml;
pub mod xml;

#[cfg(test)]
mod tables_tests;
pub mod tsmf;

// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! How fast a stream runs, from the times its packets say they were sent at.
//!
//! There is no programme clock reference in this transmission. What there is is
//! the time each MMTP packet was handed to the delivery layer, and two of those
//! far enough apart with the bytes between them is a rate.

use std::collections::BTreeMap;

/// How finely the transmission clock counts: NTP short format, whose lower
/// half is a fraction of a second.
pub const TICKS_PER_SECOND: u32 = 1 << 16;

/// The shortest stretch of clock worth dividing by. Shorter than this and the
/// answer says more about where two packets happened to fall than about the
/// rate of the stream.
const LEAST_SPAN: u32 = TICKS_PER_SECOND / 10;

/// How fast a stream runs, from the times its packets were sent at.
///
/// The same idea as a transport stream's rate from its programme clock: two
/// readings far enough apart, and the bytes between them. Readings are kept per
/// packet identifier, because two identifiers are two senders; the bytes
/// counted are all the bytes, padding and signalling included.
#[derive(Debug, Default)]
pub struct Clock {
    readings: BTreeMap<u16, Reading>,
}

#[derive(Debug)]
struct Reading {
    first_at: usize,
    last_at: usize,
    first: u32,
    last: u32,
    count: usize,
}

impl Clock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that a packet of this identifier began this far into the stream
    /// saying this.
    pub fn add(&mut self, packet_id: u16, at: usize, timestamp: u32) {
        self.readings
            .entry(packet_id)
            .and_modify(|reading| {
                reading.last_at = at;
                reading.last = timestamp;
                reading.count += 1;
            })
            .or_insert(Reading {
                first_at: at,
                last_at: at,
                first: timestamp,
                last: timestamp,
                count: 1,
            });
    }

    /// Bits per second, or nothing where there was not enough clock to say.
    pub fn bitrate(&self) -> Option<f64> {
        let best = self
            .readings
            .values()
            .max_by_key(|reading| reading.count)
            .filter(|reading| reading.count >= 2)?;
        // The subtraction carries the wrap for free: the clock runs out every
        // 65536 seconds, a little over eighteen hours, and starts again.
        let ticks = best.last.wrapping_sub(best.first);
        if ticks < LEAST_SPAN {
            return None;
        }
        let bytes = best.last_at.checked_sub(best.first_at)?;
        if bytes == 0 {
            return None;
        }
        Some(bytes as f64 * 8.0 * f64::from(TICKS_PER_SECOND) / f64::from(ticks))
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, TICKS_PER_SECOND};

    #[test]
    fn a_rate_needs_a_stretch_of_clock_to_be_worked_out_from() {
        let mut clock = Clock::new();
        clock.add(1, 0, 0);
        clock.add(1, 1_000, 100);
        assert_eq!(clock.bitrate(), None);
        let mut clock = Clock::new();
        clock.add(1, 0, 0);
        // Half a second later, and a megabyte further on.
        clock.add(1, 1_000_000, TICKS_PER_SECOND / 2);
        let bitrate = clock.bitrate().expect("there is enough clock");
        assert!((bitrate - 16_000_000.0).abs() < 1.0, "{bitrate}");
    }
}

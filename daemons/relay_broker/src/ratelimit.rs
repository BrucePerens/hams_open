// SPDX-License-Identifier: AGPL-3.0-or-later
//! Per-peer rate limiting: a token bucket per UID. A worker that loops on a refused open cannot
//! turn the broker into a log flood or a CPU sink. Time is passed in, so the tests need no clock.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Token buckets keyed by UID.
#[derive(Debug)]
pub struct RateLimiter {
    capacity: u32,
    refill_every: Duration,
    buckets: HashMap<u32, (u32, Instant)>,
}

impl RateLimiter {
    // [@ANCHOR: relay_broker_rate_limiter_new]
    /// `per_minute` requests sustained, with a burst of the same size. Zero means one a minute.
    pub fn new(per_minute: u32) -> RateLimiter {
        let capacity = per_minute.max(1);
        RateLimiter {
            capacity,
            refill_every: Duration::from_secs(60) / capacity,
            buckets: HashMap::new(),
        }
    }

    // [@ANCHOR: relay_broker_rate_limiter_allow]
    /// Takes one token for `uid` at time `now`; false when the bucket is empty. At most 256
    /// peers are tracked; beyond that the oldest entry is dropped.
    pub fn allow(&mut self, uid: u32, now: Instant) -> bool {
        if self.buckets.len() >= 256 && !self.buckets.contains_key(&uid) {
            if let Some(oldest) = self
                .buckets
                .iter()
                .min_by_key(|(_, (_, t))| *t)
                .map(|(u, _)| *u)
            {
                self.buckets.remove(&oldest);
            }
        }
        let entry = self.buckets.entry(uid).or_insert((self.capacity, now));
        let elapsed = now.saturating_duration_since(entry.1);
        let refill = (elapsed.as_nanos() / self.refill_every.as_nanos().max(1)) as u32;
        if refill > 0 {
            entry.0 = entry.0.saturating_add(refill).min(self.capacity);
            entry.1 = now;
        }
        if entry.0 == 0 {
            return false;
        }
        entry.0 = entry.0.saturating_sub(1);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: relay_broker_rate_limiter_new]
    // Tests [@ANCHOR: relay_broker_rate_limiter_allow]
    #[test]
    fn a_burst_is_allowed_then_refused_until_time_passes_per_uid() {
        let mut rl = RateLimiter::new(3);
        let t0 = Instant::now();
        assert!(rl.allow(1000, t0));
        assert!(rl.allow(1000, t0));
        assert!(rl.allow(1000, t0));
        assert!(!rl.allow(1000, t0));
        assert!(rl.allow(1001, t0), "another uid has its own bucket");
        assert!(!rl.allow(1000, t0 + Duration::from_secs(5)));
        assert!(
            rl.allow(1000, t0 + Duration::from_secs(21)),
            "one token every 20 s at 3/min"
        );
        assert!(!rl.allow(1000, t0 + Duration::from_secs(21)));
        let mut rl = RateLimiter::new(0);
        assert!(rl.allow(1, t0));
        assert!(!rl.allow(1, t0));
        let mut rl = RateLimiter::new(1);
        for uid in 0..300 {
            assert!(rl.allow(uid, t0 + Duration::from_millis(u64::from(uid))));
        }
        assert!(rl.buckets.len() <= 256);
    }
}

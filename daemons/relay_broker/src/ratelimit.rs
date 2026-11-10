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
    /// uid -> (tokens, time up to which refills are accounted, time of the last request)
    buckets: HashMap<u32, (u32, Instant, Instant)>,
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
            // Drop the peer that asked longest ago (not the one whose bucket was refilled longest ago).
            if let Some(oldest) = self
                .buckets
                .iter()
                .min_by_key(|(_, (_, _, used))| *used)
                .map(|(u, _)| *u)
            {
                self.buckets.remove(&oldest);
            }
        }
        let entry = self.buckets.entry(uid).or_insert((self.capacity, now, now));
        entry.2 = now;
        let elapsed = now.saturating_duration_since(entry.1);
        let every = self.refill_every.as_nanos().max(1);
        let refill = u32::try_from((elapsed.as_nanos() / every).min(u128::from(self.capacity)))
            .unwrap_or(self.capacity);
        if refill > 0 {
            entry.0 = entry.0.saturating_add(refill).min(self.capacity);
            // Keep the fractional remainder: advance by whole refills, not to `now` (unless full).
            entry.1 = if entry.0 >= self.capacity {
                now
            } else {
                entry.1 + self.refill_every * refill
            };
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

    // Tests [@ANCHOR: relay_broker_rate_limiter_allow]
    #[test]
    fn the_sustained_rate_keeps_the_fraction_and_a_huge_rate_does_not_wrap() {
        // 3/min = one token per 20 s. Empty the bucket, then drain it at every 30 s poll for 600 s: the sustained
        // rate is 30 tokens (one per 20 s); dropping the remainder at each refill would give only 20.
        let mut rl = RateLimiter::new(3);
        let t0 = Instant::now();
        while rl.allow(1, t0) {}
        let mut granted = 0;
        for step in 1..=20u64 {
            while rl.allow(1, t0 + Duration::from_secs(30 * step)) {
                granted += 1;
            }
        }
        assert_eq!(granted, 30);
        // A rate near u32::MAX must not wrap the refill count to a small number.
        let mut rl = RateLimiter::new(u32::MAX);
        assert!(rl.allow(7, t0));
        assert!(rl.allow(7, t0 + Duration::from_secs(3600)));
    }

    // Tests [@ANCHOR: relay_broker_rate_limiter_allow]
    #[test]
    fn the_peer_evicted_is_the_one_that_asked_longest_ago() {
        let mut rl = RateLimiter::new(1);
        let t0 = Instant::now();
        // uid 0 is throttled but keeps asking; the others ask once, early.
        for uid in 0..256 {
            assert!(rl.allow(uid, t0 + Duration::from_millis(u64::from(uid))));
        }
        assert!(!rl.allow(0, t0 + Duration::from_secs(1)));
        assert!(rl.allow(1000, t0 + Duration::from_secs(2)));
        assert!(
            rl.buckets.contains_key(&0),
            "the active peer keeps its bucket"
        );
        assert!(!rl.buckets.contains_key(&1), "an idle peer was dropped");
    }
}

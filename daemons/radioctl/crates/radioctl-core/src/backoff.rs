// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reconnect backoff: a pure schedule of waits, so a retry loop is bounded and testable without
//! a clock. The delay grows by a factor per failed attempt up to a ceiling; it is never zero
//! and never unbounded.

use std::time::Duration;

/// An exponential backoff schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    /// The wait after the first failure.
    pub initial: Duration,
    /// The longest wait.
    pub max: Duration,
    /// Multiplier per further failure (2 doubles).
    pub factor: u32,
}

impl Default for Backoff {
    fn default() -> Backoff {
        Backoff {
            initial: Duration::from_millis(250),
            max: Duration::from_secs(8),
            factor: 2,
        }
    }
}

impl Backoff {
    // [@ANCHOR: radioctl_core_backoff_delay]
    /// The wait after `failures` failed attempts in a row (1 is the first failure). The result
    /// is at least 1 ms and at most `max`.
    pub fn delay(&self, failures: u32) -> Duration {
        let mut wait = self.initial.max(Duration::from_millis(1));
        let ceiling = self.max.max(wait);
        for _ in 1..failures {
            wait = wait.saturating_mul(self.factor.max(1)).min(ceiling);
            if wait >= ceiling {
                break;
            }
        }
        wait.min(ceiling)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_core_backoff_delay]
    #[test]
    fn delays_grow_to_the_ceiling_and_stay_there() {
        let backoff = Backoff {
            initial: Duration::from_millis(100),
            max: Duration::from_millis(1000),
            factor: 2,
        };
        let waits: Vec<u128> = (1..=6).map(|n| backoff.delay(n).as_millis()).collect();
        assert_eq!(waits, vec![100, 200, 400, 800, 1000, 1000]);
        assert_eq!(backoff.delay(0), Duration::from_millis(100));
        assert_eq!(backoff.delay(u32::MAX), Duration::from_millis(1000));
        let silly = Backoff {
            initial: Duration::ZERO,
            max: Duration::ZERO,
            factor: 0,
        };
        assert_eq!(silly.delay(5), Duration::from_millis(1));
        assert_eq!(Backoff::default().delay(1), Duration::from_millis(250));
    }
}

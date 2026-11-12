// SPDX-License-Identifier: AGPL-3.0-or-later
//! The clock ceiling for `set_system_time` (RELAY_TIME_WITHOUT_THE_INTERNET.md): the broker applies its own limits and does
//! not trust the relay's judgement. The target must not be before the build date; a step beyond `max_unproven_step_secs`
//! needs a GPS source or two agreeing independent sources; at most `max_steps_per_hour` steps an hour, and one more attempt
//! trips a circuit breaker that refuses everything for an hour.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::wire::TimeSource;

/// 2026-11-01 00:00:00 UTC: no target earlier than the code's own age is believable.
pub const BUILD_DATE_UNIX_SECS: u64 = 1_793_491_200;
/// The window the step count and the breaker use.
const HOUR: Duration = Duration::from_secs(3600);

/// Why a step was refused. Short and loggable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeDenial {
    /// The target is earlier than [`BUILD_DATE_UNIX_SECS`].
    BeforeBuildDate,
    /// A large step without a GPS source or two agreeing sources.
    UnprovenLargeStep,
    /// The circuit breaker is open.
    BreakerOpen,
}

impl TimeDenial {
    // [@ANCHOR: relay_broker_time_denial_reason]
    /// The reason text sent to the relay and written to the audit log.
    pub fn reason(self) -> &'static str {
        match self {
            TimeDenial::BeforeBuildDate => "target is before the build date",
            TimeDenial::UnprovenLargeStep => "large step needs a GPS lock or two agreeing sources",
            TimeDenial::BreakerOpen => "too many clock steps: circuit breaker open",
        }
    }
}

/// The step history and the breaker.
#[derive(Debug)]
pub struct TimeGuard {
    max_unproven_step_secs: u64,
    max_steps_per_hour: usize,
    steps: VecDeque<Instant>,
    breaker_until: Option<Instant>,
}

impl TimeGuard {
    // [@ANCHOR: relay_broker_time_guard_new]
    /// A guard with the configured ceilings.
    pub fn new(max_unproven_step_secs: u64, max_steps_per_hour: u32) -> TimeGuard {
        TimeGuard {
            max_unproven_step_secs,
            max_steps_per_hour: max_steps_per_hour as usize,
            steps: VecDeque::new(),
            breaker_until: None,
        }
    }

    // [@ANCHOR: relay_broker_time_guard_expire]
    /// Forgets steps older than an hour and closes an expired breaker.
    fn expire(&mut self, now: Instant) {
        while self
            .steps
            .front()
            .is_some_and(|t| now.duration_since(*t) >= HOUR)
        {
            self.steps.pop_front();
        }
        if self.breaker_until.is_some_and(|t| now >= t) {
            self.breaker_until = None;
        }
    }

    // [@ANCHOR: relay_broker_time_guard_check]
    /// Decides whether the clock may be stepped from `current_unix_secs` to `target_unix_secs`. An allowed step is NOT recorded
    /// here (the caller records it with `record_step` once the OS call succeeded); a refusal because of the step count trips
    /// the breaker.
    pub fn check(
        &mut self,
        now: Instant,
        current_unix_secs: u64,
        target_unix_secs: u64,
        source: TimeSource,
        agreeing: u8,
    ) -> Result<(), TimeDenial> {
        self.expire(now);
        if self.breaker_until.is_some() {
            return Err(TimeDenial::BreakerOpen);
        }
        if target_unix_secs < BUILD_DATE_UNIX_SECS {
            return Err(TimeDenial::BeforeBuildDate);
        }
        let step = current_unix_secs.abs_diff(target_unix_secs);
        let proven = source == TimeSource::Gps || agreeing >= 2;
        if step > self.max_unproven_step_secs && !proven {
            return Err(TimeDenial::UnprovenLargeStep);
        }
        if self.steps.len() >= self.max_steps_per_hour {
            self.breaker_until = Some(now + HOUR);
            return Err(TimeDenial::BreakerOpen);
        }
        Ok(())
    }

    // [@ANCHOR: relay_broker_time_guard_record_step]
    /// Counts one completed step.
    pub fn record_step(&mut self, now: Instant) {
        self.steps.push_back(now);
    }

    // [@ANCHOR: relay_broker_time_guard_status]
    /// `(steps in the last hour, breaker open)`.
    pub fn status(&mut self, now: Instant) -> (u8, bool) {
        self.expire(now);
        (
            u8::try_from(self.steps.len()).unwrap_or(u8::MAX),
            self.breaker_until.is_some(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = BUILD_DATE_UNIX_SECS + 1000;

    // Tests [@ANCHOR: relay_broker_time_guard_new]
    // Tests [@ANCHOR: relay_broker_time_guard_check]
    // Tests [@ANCHOR: relay_broker_time_guard_record_step]
    // Tests [@ANCHOR: relay_broker_time_guard_status]
    // Tests [@ANCHOR: relay_broker_time_denial_reason]
    // Tests [@ANCHOR: relay_broker_time_guard_expire]
    #[test]
    fn the_ceilings_hold_and_the_breaker_trips_and_resets() {
        let t0 = Instant::now();
        let mut g = TimeGuard::new(5, 2);
        // Before the build date: refused whatever the source.
        assert_eq!(
            g.check(t0, T, BUILD_DATE_UNIX_SECS - 1, TimeSource::Gps, 3),
            Err(TimeDenial::BeforeBuildDate)
        );
        // A small step needs no proof; a large one needs GPS or two sources.
        assert_eq!(g.check(t0, T, T + 5, TimeSource::Network, 1), Ok(()));
        assert_eq!(
            g.check(t0, T, T + 6, TimeSource::Network, 1),
            Err(TimeDenial::UnprovenLargeStep)
        );
        assert_eq!(g.check(t0, T, T + 600, TimeSource::Gps, 0), Ok(()));
        assert_eq!(g.check(t0, T, T - 600, TimeSource::Network, 2), Ok(()));
        // Two recorded steps fill the hour; the third attempt trips the breaker for an hour.
        g.record_step(t0);
        g.record_step(t0);
        assert_eq!(g.status(t0), (2, false));
        assert_eq!(
            g.check(t0, T, T + 1, TimeSource::Gps, 0),
            Err(TimeDenial::BreakerOpen)
        );
        assert_eq!(g.status(t0), (2, true));
        let t1 = t0 + Duration::from_secs(1800);
        assert_eq!(
            g.check(t1, T, T + 1, TimeSource::Gps, 0),
            Err(TimeDenial::BreakerOpen)
        );
        // After the hour the steps and the breaker are gone.
        let t2 = t0 + HOUR;
        assert_eq!(g.status(t2), (0, false));
        assert_eq!(g.check(t2, T, T + 1, TimeSource::Gps, 0), Ok(()));
        assert!(!TimeDenial::BreakerOpen.reason().is_empty());
    }
}

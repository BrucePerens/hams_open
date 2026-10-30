// SPDX-License-Identifier: AGPL-3.0-or-later
//! Adaptive per-field polling. Polling is the lowest priority work of the actor: it runs only
//! when no command is queued, backs off while a field does not change, and (for rigs with
//! transceive) slows to its maximum interval once the rig has reported on its own.

use std::time::Duration;

use tokio::time::Instant;

use crate::capability::{Capability, CapabilitySet};
use crate::op::{Field, Op};
use crate::units::Vfo;

/// Polling intervals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollConfig {
    /// Master switch.
    pub enabled: bool,
    /// Fastest interval for frequency, mode and transmit state.
    pub fast_min: Duration,
    /// Slowest interval for those fields.
    pub fast_max: Duration,
    /// Fastest interval for meters.
    pub slow_min: Duration,
    /// Slowest interval for meters.
    pub slow_max: Duration,
}

impl Default for PollConfig {
    fn default() -> PollConfig {
        PollConfig {
            enabled: true,
            fast_min: Duration::from_millis(250),
            fast_max: Duration::from_secs(2),
            slow_min: Duration::from_secs(1),
            slow_max: Duration::from_secs(5),
        }
    }
}

/// One polled field.
#[derive(Debug, Clone)]
pub struct PollEntry {
    /// The field.
    pub field: Field,
    /// The read command.
    pub op: Op,
    min: Duration,
    max: Duration,
    interval: Duration,
    due: Instant,
}

/// The poll schedule of one rig.
#[derive(Debug, Clone)]
pub struct PollSchedule {
    entries: Vec<PollEntry>,
    transceive_active: bool,
}

impl PollSchedule {
    // [@ANCHOR: radioctl_core_poll_for_capabilities]
    /// Builds the schedule for the fields the capabilities allow, first due at `now`.
    pub fn for_capabilities(
        caps: &CapabilitySet,
        config: &PollConfig,
        now: Instant,
    ) -> PollSchedule {
        let mut entries = Vec::new();
        if config.enabled {
            let table = [
                (
                    Capability::ReadFrequency,
                    Field::Frequency(Vfo::A),
                    Op::GetFrequency(Vfo::A),
                    true,
                ),
                (Capability::ReadMode, Field::Mode, Op::GetMode, true),
                (Capability::ReadPtt, Field::Ptt, Op::GetPtt, true),
                (Capability::ReadSignal, Field::Signal, Op::GetSignal, false),
                (Capability::ReadPower, Field::Power, Op::GetPower, false),
            ];
            for (capability, field, op, fast) in table {
                if caps.has(capability) {
                    let (min, max) = if fast {
                        (config.fast_min, config.fast_max)
                    } else {
                        (config.slow_min, config.slow_max)
                    };
                    entries.push(PollEntry {
                        field,
                        op,
                        min,
                        max,
                        interval: min,
                        due: now,
                    });
                }
            }
        }
        PollSchedule {
            entries,
            transceive_active: false,
        }
    }

    // [@ANCHOR: radioctl_core_poll_next_due]
    /// The earliest due time, if any field is polled.
    pub fn next_due(&self) -> Option<Instant> {
        self.entries.iter().map(|e| e.due).min()
    }

    // [@ANCHOR: radioctl_core_poll_take_due]
    /// The entry to poll now, if one is due. While the rig transmits only transmit state and
    /// power are polled; the others are pushed back by their interval.
    pub fn take_due(&mut self, now: Instant, transmitting: bool) -> Option<PollEntry> {
        let index = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.due <= now)
            .min_by_key(|(_, e)| e.due)
            .map(|(i, _)| i)?;
        let entry = self.entries.get_mut(index)?;
        if transmitting && !matches!(entry.field, Field::Ptt | Field::Power) {
            entry.due = now + entry.interval;
            return None;
        }
        Some(entry.clone())
    }

    // [@ANCHOR: radioctl_core_poll_report]
    /// Records the outcome of polling `field`: a change resets the interval to its minimum, no
    /// change doubles it up to the maximum, a failure jumps to the maximum. With transceive
    /// active the interval is always the maximum.
    pub fn report(&mut self, field: Field, changed: bool, failed: bool, now: Instant) {
        let transceive = self.transceive_active;
        if let Some(entry) = self.entries.iter_mut().find(|e| e.field == field) {
            entry.interval = if failed || transceive {
                entry.max
            } else if changed {
                entry.min
            } else {
                (entry.interval * 2).min(entry.max)
            };
            entry.due = now + entry.interval;
        }
    }

    // [@ANCHOR: radioctl_core_poll_note_transceive]
    /// Notes that the rig reports on its own; polling slows to the maximum interval.
    pub fn note_transceive(&mut self) {
        self.transceive_active = true;
    }

    // [@ANCHOR: radioctl_core_poll_interval]
    /// The current interval of `field`, for tests and diagnostics.
    pub fn interval(&self, field: Field) -> Option<Duration> {
        self.entries
            .iter()
            .find(|e| e.field == field)
            .map(|e| e.interval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::description::RigDescription;

    fn schedule(now: Instant) -> PollSchedule {
        let caps = RigDescription::ic7300().capabilities;
        PollSchedule::for_capabilities(&caps, &PollConfig::default(), now)
    }

    // Tests [@ANCHOR: radioctl_core_poll_for_capabilities]
    #[test]
    fn only_readable_fields_are_polled() {
        let now = Instant::now();
        let caps = RigDescription::ts2000().capabilities;
        let kenwood = PollSchedule::for_capabilities(&caps, &PollConfig::default(), now);
        assert!(kenwood.interval(Field::Ptt).is_none());
        assert!(kenwood.interval(Field::Signal).is_some());
        let off = PollConfig {
            enabled: false,
            ..PollConfig::default()
        };
        let none = PollSchedule::for_capabilities(&caps, &off, now);
        assert!(none.next_due().is_none());
    }

    // Tests [@ANCHOR: radioctl_core_poll_next_due]
    // Tests [@ANCHOR: radioctl_core_poll_take_due]
    #[test]
    fn due_entries_come_out_one_at_a_time() {
        let now = Instant::now();
        let mut schedule = schedule(now);
        assert_eq!(schedule.next_due(), Some(now));
        let mut taken = 0;
        while let Some(entry) = schedule.take_due(now, false) {
            schedule.report(entry.field, false, false, now);
            taken += 1;
        }
        assert_eq!(taken, 5);
        assert!(schedule.next_due() > Some(now));
    }

    // Tests [@ANCHOR: radioctl_core_poll_take_due]
    #[test]
    fn while_transmitting_only_ptt_and_power_are_polled() {
        let now = Instant::now();
        let mut schedule = schedule(now);
        let mut polled = Vec::new();
        loop {
            match schedule.take_due(now, true) {
                Some(entry) => {
                    polled.push(entry.field);
                    schedule.report(entry.field, false, false, now);
                }
                None if schedule.next_due().is_some_and(|d| d <= now) => continue,
                None => break,
            }
        }
        assert_eq!(polled, vec![Field::Ptt, Field::Power]);
    }

    // Tests [@ANCHOR: radioctl_core_poll_report]
    // Tests [@ANCHOR: radioctl_core_poll_interval]
    #[test]
    fn intervals_back_off_and_reset_on_change() {
        let now = Instant::now();
        let mut schedule = schedule(now);
        let field = Field::Mode;
        assert_eq!(schedule.interval(field), Some(Duration::from_millis(250)));
        schedule.report(field, false, false, now);
        assert_eq!(schedule.interval(field), Some(Duration::from_millis(500)));
        for _ in 0..6 {
            schedule.report(field, false, false, now);
        }
        assert_eq!(schedule.interval(field), Some(Duration::from_secs(2)));
        schedule.report(field, true, false, now);
        assert_eq!(schedule.interval(field), Some(Duration::from_millis(250)));
        schedule.report(field, false, true, now);
        assert_eq!(schedule.interval(field), Some(Duration::from_secs(2)));
    }

    // Tests [@ANCHOR: radioctl_core_poll_note_transceive]
    #[test]
    fn transceive_pins_polling_to_the_maximum() {
        let now = Instant::now();
        let mut schedule = schedule(now);
        schedule.note_transceive();
        schedule.report(Field::Mode, true, false, now);
        assert_eq!(schedule.interval(Field::Mode), Some(Duration::from_secs(2)));
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! The radio lifecycle: `Off`, `PoweringOn`, `Booting`, `Ready`, `Degraded`, `Unreachable`, as a
//! sans-IO state machine. It is fed snapshots of evidence (the switch channel state, the rig
//! actor's link state, hotplug or ping evidence) and the time, and says what changed. It does no
//! I/O, never switches anything and never restores a radio's settings.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Where a radio is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RadioState {
    /// Not powered (the switch is confirmed off).
    Off,
    /// The power-on was commanded and the switch has not confirmed it yet.
    PoweringOn,
    /// Powered; waiting for it to boot and for the link to come up.
    Booting,
    /// The link is up.
    Ready,
    /// Powered and still present, but the link was lost or is failing: reconnecting.
    Degraded,
    /// Powered or of unknown power, and nothing answers: it did not boot, it dropped, or it was
    /// never seen.
    Unreachable,
}

/// What the switch channel says about the radio's power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvidence {
    /// Confirmed on by read-back.
    On,
    /// Confirmed off by read-back.
    Off,
    /// No power source, no read-back, or a failed read.
    Unknown,
}

/// One snapshot of everything known about a radio right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Evidence {
    /// The switch channel state.
    pub power: PowerEvidence,
    /// The rig actor's link state (`RigHandle::link_up`; the actor raised `Alarm::LinkDown` when
    /// this fell).
    pub link_up: bool,
    /// Hotplug (USB) or ping (network) evidence: `Some(true)` the device is there, `Some(false)`
    /// it is not, `None` no evidence.
    pub present: Option<bool>,
}

/// The timing the machine uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleTimes {
    /// Expected boot time; a radio still without a link this long plus `boot_grace` after power
    /// is confirmed on is Unreachable.
    pub boot_time: Duration,
    /// Extra time allowed on top of the boot time.
    pub boot_grace: Duration,
    /// How long a switch may take to confirm a power-on before the radio is Unreachable.
    pub power_on_timeout: Duration,
    /// How long a radio may stay Degraded before it is Unreachable.
    pub degraded_timeout: Duration,
}

impl Default for LifecycleTimes {
    // [@ANCHOR: radioctl_station_times_default]
    /// 20 s boot, 10 s grace, 10 s switch confirmation, 60 s degraded.
    fn default() -> LifecycleTimes {
        LifecycleTimes {
            boot_time: Duration::from_secs(20),
            boot_grace: Duration::from_secs(10),
            power_on_timeout: Duration::from_secs(10),
            degraded_timeout: Duration::from_secs(60),
        }
    }
}

/// A state change with its reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    /// The state before.
    pub from: RadioState,
    /// The state after.
    pub to: RadioState,
    /// Why, in words.
    pub reason: &'static str,
}

/// The state machine of one radio. Time is a [`Duration`] since any fixed start.
#[derive(Debug, Clone)]
pub struct Lifecycle {
    state: RadioState,
    since: Duration,
    times: LifecycleTimes,
    prev_present: Option<bool>,
}

impl Lifecycle {
    // [@ANCHOR: radioctl_station_lifecycle_new]
    /// A machine in `state` since `now`.
    pub fn new(state: RadioState, now: Duration, times: LifecycleTimes) -> Lifecycle {
        Lifecycle {
            state,
            since: now,
            times,
            prev_present: None,
        }
    }

    // [@ANCHOR: radioctl_station_lifecycle_state]
    /// The current state.
    pub fn state(&self) -> RadioState {
        self.state
    }

    // [@ANCHOR: radioctl_station_lifecycle_since]
    /// When the current state began.
    pub fn since(&self) -> Duration {
        self.since
    }

    // [@ANCHOR: radioctl_station_lifecycle_goto]
    /// Moves to `to` unless already there.
    fn goto(&mut self, to: RadioState, now: Duration, reason: &'static str) -> Option<Transition> {
        if self.state == to {
            return None;
        }
        let t = Transition {
            from: self.state,
            to,
            reason,
        };
        self.state = to;
        self.since = now;
        Some(t)
    }

    // [@ANCHOR: radioctl_station_lifecycle_power_on_commanded]
    /// The power-on was commanded (the power button or a profile). A radio that is already
    /// Booting, Ready or Degraded is left alone.
    pub fn power_on_commanded(&mut self, now: Duration) -> Option<Transition> {
        match self.state {
            RadioState::Off | RadioState::Unreachable => {
                self.goto(RadioState::PoweringOn, now, "power-on commanded")
            }
            _ => None,
        }
    }

    // [@ANCHOR: radioctl_station_lifecycle_assume_powered]
    /// A momentary power-on line was pulsed, which has no read-back: the radio is taken to be
    /// powered and booting.
    pub fn assume_powered(&mut self, now: Duration) -> Option<Transition> {
        match self.state {
            RadioState::PoweringOn => self.goto(RadioState::Booting, now, "power line pulsed"),
            _ => None,
        }
    }

    // [@ANCHOR: radioctl_station_lifecycle_power_off_commanded]
    /// The power-off was commanded and the switch is going off: the radio is no longer expected
    /// to answer. The switch's confirmed state then keeps it Off.
    pub fn power_off_commanded(&mut self, now: Duration) -> Option<Transition> {
        self.goto(RadioState::Off, now, "power-off commanded")
    }

    // [@ANCHOR: radioctl_station_lifecycle_step]
    /// Feeds one snapshot at `now` and returns the transition it caused, if any. At most one
    /// transition per call, so every intermediate state is seen by the event stream.
    pub fn step(&mut self, now: Duration, ev: &Evidence) -> Option<Transition> {
        let waited = now.saturating_sub(self.since);
        // a device that has been there all along is not "appearing"
        let appeared = ev.present == Some(true) && self.prev_present != Some(true);
        self.prev_present = ev.present;
        if ev.power == PowerEvidence::Off {
            return self.goto(RadioState::Off, now, "switch is off");
        }
        match self.state {
            RadioState::Off => match (ev.power, ev.link_up) {
                (PowerEvidence::On, true) => {
                    self.goto(RadioState::Ready, now, "switched on elsewhere, link up")
                }
                (PowerEvidence::On, false) => {
                    self.goto(RadioState::Booting, now, "switch is on, waiting for boot")
                }
                (_, true) => self.goto(RadioState::Ready, now, "link up"),
                _ => None,
            },
            RadioState::PoweringOn => {
                if ev.power == PowerEvidence::On {
                    self.goto(RadioState::Booting, now, "switch confirmed on")
                } else if waited >= self.times.power_on_timeout {
                    self.goto(RadioState::Unreachable, now, "switch never confirmed on")
                } else {
                    None
                }
            }
            RadioState::Booting => {
                if ev.link_up {
                    self.goto(RadioState::Ready, now, "link up")
                } else if waited >= self.times.boot_time + self.times.boot_grace {
                    self.goto(RadioState::Unreachable, now, "did not boot in time")
                } else {
                    None
                }
            }
            RadioState::Ready => {
                if ev.link_up {
                    None
                } else if ev.present == Some(false) {
                    self.goto(RadioState::Unreachable, now, "link down, device gone")
                } else {
                    self.goto(RadioState::Degraded, now, "link down")
                }
            }
            RadioState::Degraded => {
                if ev.link_up {
                    self.goto(RadioState::Ready, now, "link restored")
                } else if ev.present == Some(false) {
                    self.goto(RadioState::Unreachable, now, "device gone")
                } else if waited >= self.times.degraded_timeout {
                    self.goto(RadioState::Unreachable, now, "no link in time")
                } else {
                    None
                }
            }
            RadioState::Unreachable => {
                if ev.link_up {
                    self.goto(RadioState::Ready, now, "link up")
                } else if appeared {
                    self.goto(RadioState::Booting, now, "device appeared")
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(0);

    fn ev(power: PowerEvidence, link_up: bool, present: Option<bool>) -> Evidence {
        Evidence {
            power,
            link_up,
            present,
        }
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    // Tests [@ANCHOR: radioctl_station_times_default]
    // Tests [@ANCHOR: radioctl_station_lifecycle_new]
    // Tests [@ANCHOR: radioctl_station_lifecycle_state]
    // Tests [@ANCHOR: radioctl_station_lifecycle_since]
    // Tests [@ANCHOR: radioctl_station_lifecycle_goto]
    // Tests [@ANCHOR: radioctl_station_lifecycle_power_on_commanded]
    // Tests [@ANCHOR: radioctl_station_lifecycle_assume_powered]
    // Tests [@ANCHOR: radioctl_station_lifecycle_step]
    #[test]
    fn the_happy_path_goes_through_every_state_in_order() {
        let mut l = Lifecycle::new(RadioState::Off, T0, LifecycleTimes::default());
        assert_eq!(l.state(), RadioState::Off);
        assert!(l.power_on_commanded(secs(1)).is_some());
        assert_eq!(l.since(), secs(1));
        assert!(l.power_on_commanded(secs(2)).is_none());
        // a momentary line has no read-back: pulsing it is taken as powered
        let mut m = Lifecycle::new(RadioState::PoweringOn, T0, LifecycleTimes::default());
        assert_eq!(m.assume_powered(secs(1)).unwrap().to, RadioState::Booting);
        assert!(m.assume_powered(secs(2)).is_none());
        let t = l
            .step(secs(2), &ev(PowerEvidence::On, false, None))
            .unwrap();
        assert_eq!(
            (t.from, t.to),
            (RadioState::PoweringOn, RadioState::Booting)
        );
        assert!(l
            .step(secs(5), &ev(PowerEvidence::On, false, None))
            .is_none());
        let t = l.step(secs(9), &ev(PowerEvidence::On, true, None)).unwrap();
        assert_eq!(t.to, RadioState::Ready);
        assert!(l
            .step(secs(10), &ev(PowerEvidence::On, true, None))
            .is_none());
    }

    // Tests [@ANCHOR: radioctl_station_lifecycle_step]
    #[test]
    fn a_switch_that_confirms_but_a_radio_that_never_boots_ends_unreachable() {
        let mut l = Lifecycle::new(RadioState::Off, T0, LifecycleTimes::default());
        l.power_on_commanded(T0);
        l.step(secs(1), &ev(PowerEvidence::On, false, None));
        assert_eq!(l.state(), RadioState::Booting);
        // boot time 20 s plus grace 10 s counted from entering Booting at 1 s
        assert!(l
            .step(secs(30), &ev(PowerEvidence::On, false, None))
            .is_none());
        let t = l
            .step(secs(31), &ev(PowerEvidence::On, false, None))
            .unwrap();
        assert_eq!(t.to, RadioState::Unreachable);
        assert_eq!(t.reason, "did not boot in time");
    }

    // Tests [@ANCHOR: radioctl_station_lifecycle_step]
    #[test]
    fn a_switch_that_never_confirms_ends_unreachable() {
        let mut l = Lifecycle::new(RadioState::Off, T0, LifecycleTimes::default());
        l.power_on_commanded(T0);
        assert!(l
            .step(secs(9), &ev(PowerEvidence::Unknown, false, None))
            .is_none());
        let t = l
            .step(secs(10), &ev(PowerEvidence::Unknown, false, None))
            .unwrap();
        assert_eq!(t.to, RadioState::Unreachable);
    }

    // Tests [@ANCHOR: radioctl_station_lifecycle_step]
    #[test]
    fn a_radio_that_drops_while_powered_degrades_then_recovers_or_gives_up() {
        let mut l = Lifecycle::new(RadioState::Ready, T0, LifecycleTimes::default());
        let t = l
            .step(secs(5), &ev(PowerEvidence::On, false, Some(true)))
            .unwrap();
        assert_eq!(t.to, RadioState::Degraded);
        let t = l
            .step(secs(6), &ev(PowerEvidence::On, true, Some(true)))
            .unwrap();
        assert_eq!(t.to, RadioState::Ready);
        l.step(secs(10), &ev(PowerEvidence::On, false, None));
        assert!(l
            .step(secs(69), &ev(PowerEvidence::On, false, None))
            .is_none());
        assert_eq!(
            l.step(secs(70), &ev(PowerEvidence::On, false, None))
                .unwrap()
                .to,
            RadioState::Unreachable
        );
        // the device vanishing from USB goes straight to Unreachable
        let mut l = Lifecycle::new(RadioState::Ready, T0, LifecycleTimes::default());
        let t = l
            .step(secs(1), &ev(PowerEvidence::On, false, Some(false)))
            .unwrap();
        assert_eq!(t.to, RadioState::Unreachable);
    }

    // Tests [@ANCHOR: radioctl_station_lifecycle_step]
    // Tests [@ANCHOR: radioctl_station_lifecycle_power_off_commanded]
    #[test]
    fn an_unreachable_radio_can_be_power_cycled() {
        let mut l = Lifecycle::new(RadioState::Unreachable, T0, LifecycleTimes::default());
        // nothing happens by itself without evidence
        assert!(l
            .step(secs(1), &ev(PowerEvidence::On, false, None))
            .is_none());
        let t = l.power_off_commanded(secs(2)).unwrap();
        assert_eq!(t.to, RadioState::Off);
        assert!(l
            .step(secs(3), &ev(PowerEvidence::Off, false, None))
            .is_none());
        assert_eq!(
            l.power_on_commanded(secs(4)).unwrap().to,
            RadioState::PoweringOn
        );
        assert_eq!(
            l.step(secs(5), &ev(PowerEvidence::On, false, None))
                .unwrap()
                .to,
            RadioState::Booting
        );
        assert_eq!(
            l.step(secs(6), &ev(PowerEvidence::On, true, None))
                .unwrap()
                .to,
            RadioState::Ready
        );
    }

    // Tests [@ANCHOR: radioctl_station_lifecycle_step]
    #[test]
    fn the_switch_going_off_wins_and_hotplug_wakes_an_unreachable_radio() {
        let mut l = Lifecycle::new(RadioState::Ready, T0, LifecycleTimes::default());
        assert_eq!(
            l.step(secs(1), &ev(PowerEvidence::Off, true, None))
                .unwrap()
                .to,
            RadioState::Off
        );
        // a radio with no switch: link decides
        let mut l = Lifecycle::new(RadioState::Off, T0, LifecycleTimes::default());
        assert!(l
            .step(secs(1), &ev(PowerEvidence::Unknown, false, None))
            .is_none());
        assert_eq!(
            l.step(secs(2), &ev(PowerEvidence::Unknown, true, None))
                .unwrap()
                .to,
            RadioState::Ready
        );
        let mut u = Lifecycle::new(RadioState::Unreachable, T0, LifecycleTimes::default());
        assert_eq!(
            u.step(secs(1), &ev(PowerEvidence::On, false, Some(true)))
                .unwrap()
                .to,
            RadioState::Booting
        );
        let mut o = Lifecycle::new(RadioState::Off, T0, LifecycleTimes::default());
        assert_eq!(
            o.step(secs(1), &ev(PowerEvidence::On, false, None))
                .unwrap()
                .to,
            RadioState::Booting
        );
        let mut o = Lifecycle::new(RadioState::Off, T0, LifecycleTimes::default());
        assert_eq!(
            o.step(secs(1), &ev(PowerEvidence::On, true, None))
                .unwrap()
                .to,
            RadioState::Ready
        );
    }
}

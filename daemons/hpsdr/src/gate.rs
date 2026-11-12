// SPDX-License-Identifier: AGPL-3.0-or-later
//! The transmit gate: closed by default, opened only by the caller, for a bounded time.
//!
//! Transmit safety baseline: `hams_com/docs/proposals/RELAY_SDR_RADIOCTL_ROADMAP.md` section 2a items
//! 1 (fail closed) and 6 (underrun protection).
//!
//! Design: every packet builder that can carry a PTT/MOX bit, a drive level or transmit I/Q takes a
//! `&TxGate` and a clock reading, and writes zeros unless the gate is open at that instant. The
//! receive builders have no such parameters at all. A gate opens for a fixed lease and then closes
//! itself, so a caller that dies or stalls cannot leave it open. A poisoned lock reads as closed.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The longest a lease can run, whatever deadline the caller passes: a caller that asks for more gets this, so "a fixed lease" is
/// bounded by this crate and not only by its callers.
pub const MAX_LEASE: Duration = Duration::from_secs(60);

// [@ANCHOR: hpsdr_gate_tx_gate]
/// A shared, clonable transmit gate. `TxGate::default()` is closed.
#[derive(Clone, Debug, Default)]
pub struct TxGate {
    lease: Arc<Mutex<Option<Instant>>>,
    limits: Arc<Mutex<Limits>>,
}

// [@ANCHOR: hpsdr_gate_limits]
/// What may be sent while the gate is open. Only ever narrowed ([`TxGate::narrow_limits`]).
#[derive(Clone, Debug)]
struct Limits {
    max_drive: u8,
    /// `None`: the caller has set no band limit (this crate has no band plan of its own; the relay's band check is the authority).
    bands: Option<Vec<(u32, u32)>>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_drive: u8::MAX,
            bands: None,
        }
    }
}

impl TxGate {
    // [@ANCHOR: hpsdr_gate_new]
    /// A closed gate.
    pub fn new() -> Self {
        Self::default()
    }

    // [@ANCHOR: hpsdr_gate_open_until]
    /// Open the gate until `deadline`, but never longer than [`MAX_LEASE`] from now; returns false (and leaves it closed) if the
    /// lock is poisoned.
    pub fn open_until(&self, deadline: Instant) -> bool {
        let deadline = deadline.min(Instant::now() + MAX_LEASE);
        match self.lease.lock() {
            Ok(mut g) => {
                *g = Some(deadline);
                true
            }
            Err(_) => false,
        }
    }

    // [@ANCHOR: hpsdr_gate_narrow_limits]
    /// Narrow what may be sent: the drive cap becomes the lower of the old and `max_drive`, and the allowed bands become the
    /// overlap of the old bands (if any were set) with `bands` (inclusive `(low_hz, high_hz)` pairs). It can never widen, so a later setting cannot undo an earlier one.
    pub fn narrow_limits(&self, max_drive: u8, bands: &[(u32, u32)]) {
        let mut l = match self.limits.lock() {
            Ok(l) => l,
            Err(p) => p.into_inner(),
        };
        l.max_drive = l.max_drive.min(max_drive);
        l.bands = Some(match l.bands.take() {
            None => bands.to_vec(),
            Some(old) => old
                .iter()
                .flat_map(|o| {
                    bands.iter().filter_map(move |n| {
                        let (lo, hi) = (o.0.max(n.0), o.1.min(n.1));
                        (lo <= hi).then_some((lo, hi))
                    })
                })
                .collect(),
        });
    }

    // [@ANCHOR: hpsdr_gate_check_tx]
    /// The drive to send for a key request at `tx_hz`: `drive` capped at the gate's limit, or [`HpsdrError::OutsideLimits`] when
    /// the frequency is outside the allowed bands. A poisoned lock refuses.
    pub fn check_tx(&self, tx_hz: u32, drive: u8) -> Result<u8, crate::error::HpsdrError> {
        let l = self
            .limits
            .lock()
            .map_err(|_| crate::error::HpsdrError::OutsideLimits)?;
        if let Some(bands) = &l.bands {
            if !bands.iter().any(|&(lo, hi)| (lo..=hi).contains(&tx_hz)) {
                return Err(crate::error::HpsdrError::OutsideLimits);
            }
        }
        Ok(drive.min(l.max_drive))
    }

    // [@ANCHOR: hpsdr_gate_close]
    /// Close the gate now.
    pub fn close(&self) {
        let mut g = match self.lease.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *g = None;
    }

    // [@ANCHOR: hpsdr_gate_is_open]
    /// True only while a lease granted by `open_until` has not expired at `now`.
    pub fn is_open(&self, now: Instant) -> bool {
        match self.lease.lock() {
            Ok(g) => matches!(*g, Some(t) if now < t),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // Tests [@ANCHOR: hpsdr_gate_tx_gate]
    #[test]
    fn default_gate_is_closed_and_lease_expires() {
        // Tests [@ANCHOR: hpsdr_gate_new]
        // Tests [@ANCHOR: hpsdr_gate_is_open]
        // Tests [@ANCHOR: hpsdr_gate_open_until]
        // Tests [@ANCHOR: hpsdr_gate_close]
        let now = Instant::now();
        let g = TxGate::new();
        assert!(!g.is_open(now));
        assert!(!TxGate::default().is_open(now));
        assert!(g.open_until(now + Duration::from_secs(5)));
        assert!(g.is_open(now));
        assert!(g.clone().is_open(now), "clones share the lease");
        assert!(
            !g.is_open(now + Duration::from_secs(5)),
            "lease is exclusive of its deadline"
        );
        assert!(!g.is_open(now + Duration::from_secs(60)));
        g.close();
        assert!(!g.is_open(now));
    }

    #[test]
    fn poisoned_gate_reads_closed() {
        // Tests [@ANCHOR: hpsdr_gate_is_open]
        // Tests [@ANCHOR: hpsdr_gate_open_until]
        // Tests [@ANCHOR: hpsdr_gate_close]
        let now = Instant::now();
        let g = TxGate::new();
        assert!(g.open_until(now + Duration::from_secs(5)));
        let g2 = g.clone();
        let _ = std::thread::spawn(move || {
            let _hold = g2.lease.lock();
            std::panic::resume_unwind(Box::new("poison the gate"));
        })
        .join();
        assert!(!g.is_open(now), "poisoned lock fails closed");
        assert!(!g.open_until(now + Duration::from_secs(5)));
        g.close();
        assert!(!g.is_open(now));
    }

    #[test]
    fn a_lease_is_never_longer_than_the_maximum() {
        // Tests [@ANCHOR: hpsdr_gate_open_until]
        let g = TxGate::new();
        let now = Instant::now();
        assert!(g.open_until(now + Duration::from_secs(86_400)));
        assert!(g.is_open(now + MAX_LEASE - Duration::from_secs(2)));
        assert!(!g.is_open(now + MAX_LEASE + Duration::from_secs(2)));
    }

    // Tests [@ANCHOR: hpsdr_gate_limits]
    #[test]
    fn limits_only_narrow() {
        // Tests [@ANCHOR: hpsdr_gate_narrow_limits]
        // Tests [@ANCHOR: hpsdr_gate_check_tx]
        let g = TxGate::new();
        assert_eq!(g.check_tx(7_100_000, 255), Ok(255), "no limit set");
        g.narrow_limits(100, &[(7_000_000, 7_300_000), (14_000_000, 14_350_000)]);
        assert_eq!(g.check_tx(7_100_000, 255), Ok(100));
        assert_eq!(g.check_tx(7_100_000, 40), Ok(40));
        assert_eq!(
            g.check_tx(10_000_000, 10),
            Err(crate::error::HpsdrError::OutsideLimits)
        );
        // A wider setting later changes nothing; a narrower one applies.
        g.narrow_limits(200, &[(0, 30_000_000)]);
        assert_eq!(g.check_tx(7_100_000, 255), Ok(100));
        assert_eq!(
            g.check_tx(10_000_000, 10),
            Err(crate::error::HpsdrError::OutsideLimits)
        );
        g.narrow_limits(50, &[(7_050_000, 7_150_000)]);
        assert_eq!(g.check_tx(7_100_000, 255), Ok(50));
        assert_eq!(
            g.check_tx(7_200_000, 10),
            Err(crate::error::HpsdrError::OutsideLimits)
        );
        assert_eq!(
            g.check_tx(14_100_000, 10),
            Err(crate::error::HpsdrError::OutsideLimits)
        );
        // A clone shares the limits.
        assert_eq!(
            g.clone().check_tx(7_200_000, 1),
            Err(crate::error::HpsdrError::OutsideLimits)
        );
    }
}

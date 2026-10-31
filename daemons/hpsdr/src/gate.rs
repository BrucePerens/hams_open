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
use std::time::Instant;

/// A shared, clonable transmit gate. `TxGate::default()` is closed.
#[derive(Clone, Debug, Default)]
pub struct TxGate {
    lease: Arc<Mutex<Option<Instant>>>,
}

impl TxGate {
    // [@ANCHOR: hpsdr_gate_new]
    /// A closed gate.
    pub fn new() -> Self {
        Self::default()
    }

    // [@ANCHOR: hpsdr_gate_open_until]
    /// Open the gate until `deadline`; returns false (and leaves it closed) if the lock is poisoned.
    pub fn open_until(&self, deadline: Instant) -> bool {
        match self.lease.lock() {
            Ok(mut g) => {
                *g = Some(deadline);
                true
            }
            Err(_) => false,
        }
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
}

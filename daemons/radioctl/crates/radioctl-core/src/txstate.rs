// SPDX-License-Identifier: AGPL-3.0-or-later
//! Receive and transmit frequency, first class (Hamlib issue 138: a client that asks for "the
//! frequency" while the rig transmits in split mode got the receive frequency).
//!
//! The pure derivation: which VFO receives, which transmits, and what frequency each is on, from
//! the values a rig has reported. The actor applies it after every observation and publishes the
//! result as [`crate::op::Observation::ActiveTxFrequency`], so the status-change callback fires
//! when the transmit frequency changes, including a change made on the front panel.
//!
//! Rules. The receive VFO is the reported `RxVfo`, else VFO A (the selected VFO on CI-V, where
//! the unselected VFO is VFO B). The transmit VFO is the receive VFO when split is off, else the reported
//! `TxVfo`, else the other VFO. The transmit frequency is
//! that VFO's frequency. A RIT or XIT offset is deliberately not folded in: it moves the
//! receive (RIT) or transmit (XIT) frequency by at most a few kilohertz, and a follower such as
//! an amplifier band decoder wants the VFO frequency.

use crate::op::{Field, Observation};
use crate::units::{Hz, Vfo};

/// The cached values the derivation reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TxState {
    /// The reported receive VFO.
    pub rx_vfo: Option<Vfo>,
    /// The reported transmit VFO.
    pub tx_vfo: Option<Vfo>,
    /// Split operation, when known.
    pub split: Option<bool>,
    /// VFO A's frequency.
    pub freq_a: Option<Hz>,
    /// VFO B's frequency.
    pub freq_b: Option<Hz>,
}

impl TxState {
    // [@ANCHOR: radioctl_core_txstate_from_cache]
    /// Reads the inputs through `get`, which returns the cached observation of a field.
    pub fn from_cache(get: impl Fn(Field) -> Option<Observation>) -> TxState {
        let vfo = |field| match get(field) {
            Some(Observation::RxVfo(v) | Observation::TxVfo(v)) => Some(v),
            _ => None,
        };
        let freq = |v: Vfo| match get(Field::Frequency(v)) {
            Some(Observation::Frequency(_, hz)) => Some(hz),
            _ => None,
        };
        TxState {
            rx_vfo: vfo(Field::RxVfo),
            tx_vfo: vfo(Field::TxVfo),
            split: match get(Field::Split) {
                Some(Observation::Split(on)) => Some(on),
                _ => None,
            },
            freq_a: freq(Vfo::A),
            freq_b: freq(Vfo::B),
        }
    }

    // [@ANCHOR: radioctl_core_txstate_rx_vfo]
    /// The VFO the rig receives on.
    pub fn rx_vfo(&self) -> Vfo {
        self.rx_vfo.unwrap_or(Vfo::A)
    }

    // [@ANCHOR: radioctl_core_txstate_tx_vfo]
    /// The VFO the rig transmits on.
    pub fn tx_vfo(&self) -> Vfo {
        match (self.split, self.tx_vfo) {
            (Some(false), _) => self.rx_vfo(),
            (_, Some(v)) => v,
            (Some(true), None) => self.rx_vfo().other(),
            (None, None) => self.rx_vfo(),
        }
    }

    fn frequency(&self, vfo: Vfo) -> Option<Hz> {
        match vfo {
            Vfo::A => self.freq_a,
            Vfo::B => self.freq_b,
        }
    }

    // [@ANCHOR: radioctl_core_txstate_rx_frequency]
    /// The receive frequency, `None` until the receive VFO's frequency is known.
    pub fn rx_frequency(&self) -> Option<Hz> {
        self.frequency(self.rx_vfo())
    }

    // [@ANCHOR: radioctl_core_txstate_tx_frequency]
    /// The frequency of the transmit VFO (equal to the receive frequency when not split).
    pub fn tx_frequency(&self) -> Option<Hz> {
        self.frequency(self.tx_vfo())
    }

    // [@ANCHOR: radioctl_core_txstate_active_tx_frequency]
    /// The frequency the rig transmits on now, or will when keyed: what an amplifier, tuner,
    /// antenna selector or band decoder follows. Correct in split mode and while keyed.
    pub fn active_tx_frequency(&self) -> Option<Hz> {
        self.tx_frequency()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(split: bool, tx: Option<Vfo>) -> TxState {
        TxState {
            rx_vfo: None,
            tx_vfo: tx,
            split: Some(split),
            freq_a: Some(Hz(14_195_000)),
            freq_b: Some(Hz(14_205_000)),
        }
    }

    // Tests [@ANCHOR: radioctl_core_txstate_rx_vfo]
    // Tests [@ANCHOR: radioctl_core_txstate_tx_vfo]
    // Tests [@ANCHOR: radioctl_core_txstate_rx_frequency]
    // Tests [@ANCHOR: radioctl_core_txstate_tx_frequency]
    // Tests [@ANCHOR: radioctl_core_txstate_active_tx_frequency]
    #[test]
    fn split_transmits_on_the_other_vfo_and_simplex_on_the_receive_vfo() {
        let simplex = state(false, None);
        assert_eq!(simplex.rx_frequency(), Some(Hz(14_195_000)));
        assert_eq!(simplex.active_tx_frequency(), Some(Hz(14_195_000)));
        let split = state(true, None);
        assert_eq!(split.rx_frequency(), Some(Hz(14_195_000)));
        assert_eq!(split.tx_vfo(), Vfo::B);
        assert_eq!(split.tx_frequency(), Some(Hz(14_205_000)));
        assert_eq!(split.active_tx_frequency(), Some(Hz(14_205_000)));
        // A reported transmit VFO wins, and a Kenwood rig receiving on B in split transmits on A.
        let mut kenwood = state(true, Some(Vfo::A));
        kenwood.rx_vfo = Some(Vfo::B);
        assert_eq!(kenwood.rx_frequency(), Some(Hz(14_205_000)));
        assert_eq!(kenwood.active_tx_frequency(), Some(Hz(14_195_000)));
        let mut unknown = state(true, None);
        unknown.freq_b = None;
        assert_eq!(unknown.active_tx_frequency(), None);
        assert_eq!(unknown.rx_frequency(), Some(Hz(14_195_000)));
    }

    // Tests [@ANCHOR: radioctl_core_txstate_from_cache]
    #[test]
    fn the_inputs_come_from_the_cached_observations() {
        let get = |field| match field {
            Field::Split => Some(Observation::Split(true)),
            Field::TxVfo => Some(Observation::TxVfo(Vfo::B)),
            Field::RxVfo => Some(Observation::RxVfo(Vfo::A)),
            Field::Frequency(Vfo::A) => Some(Observation::Frequency(Vfo::A, Hz(7_000_000))),
            Field::Frequency(Vfo::B) => Some(Observation::Frequency(Vfo::B, Hz(7_100_000))),
            _ => None,
        };
        let state = TxState::from_cache(get);
        assert_eq!(state.active_tx_frequency(), Some(Hz(7_100_000)));
        assert_eq!(TxState::from_cache(|_| None).active_tx_frequency(), None);
    }
}

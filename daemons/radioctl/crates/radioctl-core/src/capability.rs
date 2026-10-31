// SPDX-License-Identifier: AGPL-3.0-or-later
//! The capability model: what a rig can do. The safety gates and any UI are generated from it.

use std::collections::BTreeSet;

/// One thing a rig can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// Read the frequency.
    ReadFrequency,
    /// Set the frequency.
    SetFrequency,
    /// Read the mode.
    ReadMode,
    /// Set the mode.
    SetMode,
    /// Read the second VFO.
    SecondVfo,
    /// Read the transmit state.
    ReadPtt,
    /// Key and unkey the transmitter. Transmit-capable.
    SetPtt,
    /// Read the S-meter.
    ReadSignal,
    /// Read the power meter.
    ReadPower,
    /// Send Morse text. Transmit-capable.
    SendCw,
    /// Report changes without being polled (CI-V transceive, Kenwood auto-information).
    Transceive,
    /// Read and set split operation.
    Split,
    /// Read and set RIT, XIT and their offset.
    RitXit,
    /// Read and set the receive filter bandwidth.
    Filter,
    /// Read and set numeric levels (gains, keyer speed, power setting).
    Levels,
    /// Read the SWR meter.
    ReadSwr,
    /// Read the raw transmitter and supply meters (SWR, ALC, COMP, Vd, Id) in the rig's own
    /// scale. Read-only.
    ReadMeters,
    /// Ask the rig for its identifier and firmware revision.
    Identify,
    /// Spectrum scope: settings and waveform frames.
    Scope,
}

/// Every capability with its file-format name.
const NAMES: [(Capability, &str); 19] = [
    (Capability::ReadFrequency, "read-frequency"),
    (Capability::SetFrequency, "set-frequency"),
    (Capability::ReadMode, "read-mode"),
    (Capability::SetMode, "set-mode"),
    (Capability::SecondVfo, "second-vfo"),
    (Capability::ReadPtt, "read-ptt"),
    (Capability::SetPtt, "set-ptt"),
    (Capability::ReadSignal, "read-signal"),
    (Capability::ReadPower, "read-power"),
    (Capability::SendCw, "send-cw"),
    (Capability::Transceive, "transceive"),
    (Capability::Split, "split"),
    (Capability::RitXit, "rit-xit"),
    (Capability::Filter, "filter"),
    (Capability::Levels, "levels"),
    (Capability::ReadSwr, "read-swr"),
    (Capability::ReadMeters, "read-meters"),
    (Capability::Identify, "identify"),
    (Capability::Scope, "scope"),
];

impl Capability {
    // [@ANCHOR: radioctl_core_capability_name]
    /// The name used in rig description files.
    pub fn name(self) -> &'static str {
        NAMES
            .iter()
            .find(|(c, _)| *c == self)
            .map(|(_, n)| *n)
            .unwrap_or("unknown")
    }

    // [@ANCHOR: radioctl_core_capability_from_name]
    /// The capability with a file-format name.
    pub fn from_name(name: &str) -> Option<Capability> {
        NAMES.iter().find(|(_, n)| *n == name).map(|(c, _)| *c)
    }

    // [@ANCHOR: radioctl_core_capability_transmits]
    /// True when using the capability can put RF on the air.
    pub fn transmits(self) -> bool {
        matches!(self, Capability::SetPtt | Capability::SendCw)
    }
}

/// The set of capabilities of one rig.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilitySet(BTreeSet<Capability>);

impl CapabilitySet {
    // [@ANCHOR: radioctl_core_capability_set_of]
    /// Builds a set from a list.
    pub fn of(list: &[Capability]) -> CapabilitySet {
        CapabilitySet(list.iter().copied().collect())
    }

    // [@ANCHOR: radioctl_core_capability_set_has]
    /// True when the capability is present.
    pub fn has(&self, capability: Capability) -> bool {
        self.0.contains(&capability)
    }

    // [@ANCHOR: radioctl_core_capability_set_iter]
    /// The capabilities in a stable order.
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_core_capability_transmits]
    #[test]
    fn only_ptt_and_cw_transmit() {
        assert!(Capability::SetPtt.transmits());
        assert!(Capability::SendCw.transmits());
        assert!(!Capability::ReadPtt.transmits());
        assert!(!Capability::SetFrequency.transmits());
    }

    // Tests [@ANCHOR: radioctl_core_capability_name]
    // Tests [@ANCHOR: radioctl_core_capability_from_name]
    #[test]
    fn names_round_trip_for_every_capability() {
        for (capability, name) in NAMES {
            assert_eq!(capability.name(), name);
            assert_eq!(Capability::from_name(name), Some(capability));
        }
        assert_eq!(Capability::from_name("fly"), None);
    }

    // Tests [@ANCHOR: radioctl_core_capability_set_of]
    // Tests [@ANCHOR: radioctl_core_capability_set_has]
    // Tests [@ANCHOR: radioctl_core_capability_set_iter]
    #[test]
    fn set_membership_and_order() {
        let set = CapabilitySet::of(&[Capability::SetMode, Capability::ReadFrequency]);
        assert!(set.has(Capability::SetMode));
        assert!(!set.has(Capability::SetPtt));
        let order: Vec<_> = set.iter().collect();
        assert_eq!(order, vec![Capability::ReadFrequency, Capability::SetMode]);
    }
}

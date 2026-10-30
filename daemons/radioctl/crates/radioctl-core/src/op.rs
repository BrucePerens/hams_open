// SPDX-License-Identifier: AGPL-3.0-or-later
//! The command vocabulary shared by the actor and the protocol engines, and the values a rig
//! reports back.

use crate::capability::Capability;
use crate::units::{Hz, Mode, Vfo};

/// A command to a rig.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Read a VFO's frequency.
    GetFrequency(Vfo),
    /// Set a VFO's frequency.
    SetFrequency(Vfo, Hz),
    /// Read the mode.
    GetMode,
    /// Set the mode.
    SetMode(Mode),
    /// Read the transmit state.
    GetPtt,
    /// Key (`true`) or unkey (`false`) the transmitter.
    SetPtt(bool),
    /// Read the S-meter (raw units).
    GetSignal,
    /// Read the power meter (raw units).
    GetPower,
    /// Send Morse text.
    SendCw(String),
    /// Turn the rig's unsolicited reporting on or off.
    SetTransceive(bool),
}

/// A kind of value the actor caches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    /// A VFO's frequency.
    Frequency(Vfo),
    /// The mode.
    Mode,
    /// The transmit state.
    Ptt,
    /// The S-meter.
    Signal,
    /// The power meter.
    Power,
}

/// A value reported by a rig, solicited or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    /// A VFO's frequency.
    Frequency(Vfo, Hz),
    /// The mode.
    Mode(Mode),
    /// The transmit state.
    Ptt(bool),
    /// The S-meter, raw units.
    Signal(u16),
    /// The power meter, raw units.
    Power(u16),
}

impl Observation {
    // [@ANCHOR: radioctl_core_observation_field]
    /// The cache field this observation updates.
    pub fn field(&self) -> Field {
        match self {
            Observation::Frequency(vfo, _) => Field::Frequency(*vfo),
            Observation::Mode(_) => Field::Mode,
            Observation::Ptt(_) => Field::Ptt,
            Observation::Signal(_) => Field::Signal,
            Observation::Power(_) => Field::Power,
        }
    }
}

impl Op {
    // [@ANCHOR: radioctl_core_op_capability]
    /// The capability a rig needs for this command.
    pub fn required_capability(&self) -> Capability {
        match self {
            Op::GetFrequency(Vfo::A) => Capability::ReadFrequency,
            Op::GetFrequency(Vfo::B) => Capability::SecondVfo,
            Op::SetFrequency(Vfo::A, _) => Capability::SetFrequency,
            Op::SetFrequency(Vfo::B, _) => Capability::SecondVfo,
            Op::GetMode => Capability::ReadMode,
            Op::SetMode(_) => Capability::SetMode,
            Op::GetPtt => Capability::ReadPtt,
            Op::SetPtt(_) => Capability::SetPtt,
            Op::GetSignal => Capability::ReadSignal,
            Op::GetPower => Capability::ReadPower,
            Op::SendCw(_) => Capability::SendCw,
            Op::SetTransceive(_) => Capability::Transceive,
        }
    }

    // [@ANCHOR: radioctl_core_op_transmits]
    /// True when the command can put RF on the air. Unkeying never transmits.
    pub fn transmits(&self) -> bool {
        match self {
            Op::SetPtt(on) => *on,
            Op::SendCw(_) => true,
            _ => false,
        }
    }

    // [@ANCHOR: radioctl_core_op_is_emergency]
    /// True for the commands that jump the queue: unkeying the transmitter.
    pub fn is_emergency(&self) -> bool {
        matches!(self, Op::SetPtt(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_core_observation_field]
    #[test]
    fn observation_maps_to_its_field() {
        assert_eq!(
            Observation::Frequency(Vfo::B, Hz(1)).field(),
            Field::Frequency(Vfo::B)
        );
        assert_eq!(Observation::Ptt(true).field(), Field::Ptt);
        assert_eq!(Observation::Mode(Mode::Usb).field(), Field::Mode);
        assert_eq!(Observation::Signal(1).field(), Field::Signal);
        assert_eq!(Observation::Power(1).field(), Field::Power);
    }

    // Tests [@ANCHOR: radioctl_core_op_capability]
    #[test]
    fn ops_name_their_capability() {
        assert_eq!(
            Op::GetFrequency(Vfo::A).required_capability(),
            Capability::ReadFrequency
        );
        assert_eq!(
            Op::SetFrequency(Vfo::B, Hz(1)).required_capability(),
            Capability::SecondVfo
        );
        assert_eq!(Op::SetPtt(false).required_capability(), Capability::SetPtt);
        assert_eq!(
            Op::SendCw("x".into()).required_capability(),
            Capability::SendCw
        );
    }

    // Tests [@ANCHOR: radioctl_core_op_transmits]
    #[test]
    fn keying_and_cw_transmit_but_unkeying_does_not() {
        assert!(Op::SetPtt(true).transmits());
        assert!(Op::SendCw("cq".into()).transmits());
        assert!(!Op::SetPtt(false).transmits());
        assert!(!Op::SetFrequency(Vfo::A, Hz(7_000_000)).transmits());
    }

    // Tests [@ANCHOR: radioctl_core_op_is_emergency]
    #[test]
    fn only_unkeying_is_an_emergency() {
        assert!(Op::SetPtt(false).is_emergency());
        assert!(!Op::SetPtt(true).is_emergency());
        assert!(!Op::GetMode.is_emergency());
    }
}

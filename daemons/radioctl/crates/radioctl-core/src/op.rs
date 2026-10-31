// SPDX-License-Identifier: AGPL-3.0-or-later
//! The command vocabulary shared by the actor and the protocol engines, and the values a rig
//! reports back.

use crate::capability::Capability;
use crate::scope::ScopeAxis;
use crate::units::{Hz, Level, Meter, Mode, Vfo};

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
    /// Stop Morse in progress: clear the rig's CW text buffer and end the element being sent.
    /// It is a stop path, so it jumps the queue like an unkey and needs no transmit gate. It
    /// does not unkey a rig held keyed by PTT; polling reconciles the transmit state after it.
    StopCw,
    /// Turn the rig's unsolicited reporting on or off.
    SetTransceive(bool),
    /// Read split operation.
    GetSplit,
    /// Read which VFO the rig transmits on.
    GetTxVfo,
    /// Enter (`true`) or leave (`false`) split operation.
    SetSplit(bool),
    /// Read whether RIT is on.
    GetRit,
    /// Turn RIT on or off.
    SetRit(bool),
    /// Read whether XIT is on.
    GetXit,
    /// Turn XIT on or off.
    SetXit(bool),
    /// Read the RIT/XIT offset in hertz.
    GetRitOffset,
    /// Set the RIT/XIT offset in hertz (the rig limits the range).
    SetRitOffset(i16),
    /// Read the receive filter bandwidth.
    GetFilterWidth,
    /// Set the receive filter bandwidth.
    SetFilterWidth(Hz),
    /// Read a level.
    GetLevel(Level),
    /// Set a level, in the rig's units.
    SetLevel(Level, u16),
    /// Read the SWR meter (tenths of a unit).
    GetSwr,
    /// Read a transmitter or supply meter in the rig's raw scale (see [`Meter`]). Read-only:
    /// it works with the transmit gate closed and means most while the rig is keyed by other
    /// means.
    GetMeter(Meter),
    /// Read the rig's model identifier.
    GetModelId,
    /// Read the rig's main firmware revision.
    GetFirmware,
    /// Switch the rig's waveform output (the scope sweeps it sends on its own) on or off.
    SetScopeOutput(bool),
    /// Read whether waveform output is on.
    GetScopeOutput,
    /// Set how a receiver's scope axis is anchored (receiver 0 is Main, 1 is Sub).
    SetScopeAxis(u8, ScopeAxis),
    /// Read a receiver's scope axis mode.
    GetScopeAxis(u8),
    /// Set a receiver's scope span (the full width in hertz; the rig offers fixed choices).
    SetScopeSpan(u8, Hz),
    /// Read a receiver's scope span.
    GetScopeSpan(u8),
    /// Set the fixed-mode edges: range number, edge number (1 based), lower and upper edge.
    SetScopeEdges(u8, u8, Hz, Hz),
    /// Set a receiver's reference level, in hundredths of a dB.
    SetScopeReference(u8, i16),
    /// Read a receiver's reference level.
    GetScopeReference(u8),
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
    /// Split operation.
    Split,
    /// The VFO the rig receives on, when it can say (Kenwood `IF`/`FR`); otherwise VFO A.
    RxVfo,
    /// The VFO the rig transmits on.
    TxVfo,
    /// The frequency the rig transmits on, derived by the actor from the other fields; no
    /// engine reports it and no command reads it.
    ActiveTxFrequency,
    /// RIT on or off.
    Rit,
    /// XIT on or off.
    Xit,
    /// The RIT/XIT offset.
    RitOffset,
    /// The receive filter bandwidth.
    FilterWidth,
    /// A numeric level.
    Level(Level),
    /// The SWR meter.
    Swr,
    /// A raw meter.
    Meter(Meter),
    /// The model identifier.
    ModelId,
    /// The firmware revision.
    Firmware,
    /// Waveform output on or off.
    ScopeOutput,
    /// A receiver's scope axis mode.
    ScopeAxis(u8),
    /// A receiver's scope span.
    ScopeSpan(u8),
    /// A receiver's scope reference level.
    ScopeReference(u8),
    /// A safety condition the actor reports on its own (see [`Alarm`]).
    Alarm,
}

/// A safety condition the actor reports on its own. It is not a value read from the rig and it
/// is never deduplicated: every occurrence is an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Alarm {
    /// The transmitter was keyed for longer than the rig's maximum transmit duration; the actor
    /// unkeyed it.
    TransmitTimeout,
    /// The caller's heartbeat lapsed while the rig was transmitting; the actor unkeyed it.
    HeartbeatLapsed,
    /// An unkey failed after every attempt: the rig may still be transmitting.
    UnkeyFailed,
    /// The transport to the rig failed. A rig that was keyed cannot be unkeyed by command until
    /// the link is back; every command is refused with `LinkDown` meanwhile.
    LinkDown,
    /// The transport came back after [`Alarm::LinkDown`]; the actor has run its unkey pass
    /// (see `unkey_failures` / [`Alarm::UnkeyFailed`] for one that did not succeed).
    LinkRestored,
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
    /// Split operation on or off.
    Split(bool),
    /// The VFO the rig receives on.
    RxVfo(Vfo),
    /// The VFO the rig transmits on.
    TxVfo(Vfo),
    /// The frequency the rig transmits on (derived; see [`crate::txstate`]).
    ActiveTxFrequency(Hz),
    /// RIT on or off.
    Rit(bool),
    /// XIT on or off.
    Xit(bool),
    /// The RIT/XIT offset in hertz.
    RitOffset(i16),
    /// The receive filter bandwidth.
    FilterWidth(Hz),
    /// A numeric level in the rig's units.
    Level(Level, u16),
    /// The SWR in tenths of a unit (10 is 1.0:1).
    Swr(u16),
    /// A meter deflection in the rig's raw scale (Icom: 0 to 255), uncalibrated.
    Meter(Meter, u16),
    /// The model identifier the rig reports.
    ModelId(u16),
    /// The firmware revision, major and minor (`02.37` is `(2, 37)`).
    Firmware(u8, u8),
    /// Waveform output on or off.
    ScopeOutput(bool),
    /// A receiver's scope axis mode.
    ScopeAxis(u8, ScopeAxis),
    /// A receiver's scope span (full width).
    ScopeSpan(u8, Hz),
    /// A receiver's scope reference level in hundredths of a dB.
    ScopeReference(u8, i16),
    /// A safety condition reported by the actor, never by the rig.
    Alarm(Alarm),
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
            Observation::Split(_) => Field::Split,
            Observation::RxVfo(_) => Field::RxVfo,
            Observation::TxVfo(_) => Field::TxVfo,
            Observation::ActiveTxFrequency(_) => Field::ActiveTxFrequency,
            Observation::Rit(_) => Field::Rit,
            Observation::Xit(_) => Field::Xit,
            Observation::RitOffset(_) => Field::RitOffset,
            Observation::FilterWidth(_) => Field::FilterWidth,
            Observation::Level(level, _) => Field::Level(*level),
            Observation::Swr(_) => Field::Swr,
            Observation::Meter(meter, _) => Field::Meter(*meter),
            Observation::ModelId(_) => Field::ModelId,
            Observation::Firmware(..) => Field::Firmware,
            Observation::ScopeOutput(_) => Field::ScopeOutput,
            Observation::ScopeAxis(rx, _) => Field::ScopeAxis(*rx),
            Observation::ScopeSpan(rx, _) => Field::ScopeSpan(*rx),
            Observation::ScopeReference(rx, _) => Field::ScopeReference(*rx),
            Observation::Alarm(_) => Field::Alarm,
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
            Op::SendCw(_) | Op::StopCw => Capability::SendCw,
            Op::SetTransceive(_) => Capability::Transceive,
            Op::GetSplit | Op::SetSplit(_) | Op::GetTxVfo => Capability::Split,
            Op::GetRit
            | Op::SetRit(_)
            | Op::GetXit
            | Op::SetXit(_)
            | Op::GetRitOffset
            | Op::SetRitOffset(_) => Capability::RitXit,
            Op::GetFilterWidth | Op::SetFilterWidth(_) => Capability::Filter,
            Op::GetLevel(_) | Op::SetLevel(..) => Capability::Levels,
            Op::GetSwr => Capability::ReadSwr,
            Op::GetMeter(_) => Capability::ReadMeters,
            Op::GetModelId | Op::GetFirmware => Capability::Identify,
            Op::SetScopeOutput(_)
            | Op::GetScopeOutput
            | Op::SetScopeAxis(..)
            | Op::GetScopeAxis(_)
            | Op::SetScopeSpan(..)
            | Op::GetScopeSpan(_)
            | Op::SetScopeEdges(..)
            | Op::SetScopeReference(..)
            | Op::GetScopeReference(_) => Capability::Scope,
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
    /// True for the commands that jump the queue: unkeying the transmitter and stopping Morse.
    /// Both preempt a command waiting for a reply, skip inter-command spacing and need no gate.
    pub fn is_emergency(&self) -> bool {
        matches!(self, Op::SetPtt(false) | Op::StopCw)
    }

    // [@ANCHOR: radioctl_core_op_is_unkey]
    /// True for the one command whose failure means the rig may still be on the air: the unkey.
    /// Only it is coalesced when the emergency queue is full and only its failure is
    /// [`crate::op::Alarm::UnkeyFailed`]; a stop-Morse that fails is reported to its caller alone.
    pub fn is_unkey(&self) -> bool {
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
        assert_eq!(Observation::Split(true).field(), Field::Split);
        assert_eq!(Observation::RxVfo(Vfo::B).field(), Field::RxVfo);
        assert_eq!(Observation::TxVfo(Vfo::B).field(), Field::TxVfo);
        assert_eq!(
            Observation::ActiveTxFrequency(Hz(1)).field(),
            Field::ActiveTxFrequency
        );
        assert_eq!(Observation::Rit(true).field(), Field::Rit);
        assert_eq!(Observation::Xit(true).field(), Field::Xit);
        assert_eq!(Observation::RitOffset(-5).field(), Field::RitOffset);
        assert_eq!(
            Observation::FilterWidth(Hz(500)).field(),
            Field::FilterWidth
        );
        assert_eq!(
            Observation::Level(Level::AfGain, 9).field(),
            Field::Level(Level::AfGain)
        );
        assert_eq!(Observation::Swr(15).field(), Field::Swr);
        assert_eq!(
            Observation::Meter(Meter::Alc, 120).field(),
            Field::Meter(Meter::Alc)
        );
        assert_eq!(Observation::ModelId(17).field(), Field::ModelId);
        assert_eq!(Observation::Firmware(2, 37).field(), Field::Firmware);
        assert_eq!(Observation::ScopeOutput(true).field(), Field::ScopeOutput);
        assert_eq!(
            Observation::ScopeAxis(1, ScopeAxis::Fixed).field(),
            Field::ScopeAxis(1)
        );
        assert_eq!(
            Observation::ScopeSpan(0, Hz(50_000)).field(),
            Field::ScopeSpan(0)
        );
        assert_eq!(
            Observation::ScopeReference(0, -200).field(),
            Field::ScopeReference(0)
        );
        assert_eq!(Observation::Alarm(Alarm::UnkeyFailed).field(), Field::Alarm);
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
        assert_eq!(Op::SetSplit(true).required_capability(), Capability::Split);
        assert_eq!(Op::GetTxVfo.required_capability(), Capability::Split);
        assert_eq!(
            Op::SetScopeSpan(0, Hz(50_000)).required_capability(),
            Capability::Scope
        );
        assert_eq!(
            Op::SetScopeEdges(1, 1, Hz(1), Hz(2)).required_capability(),
            Capability::Scope
        );
        assert_eq!(Op::GetRitOffset.required_capability(), Capability::RitXit);
        assert_eq!(
            Op::SetFilterWidth(Hz(500)).required_capability(),
            Capability::Filter
        );
        assert_eq!(
            Op::GetLevel(Level::MicGain).required_capability(),
            Capability::Levels
        );
        assert_eq!(Op::GetSwr.required_capability(), Capability::ReadSwr);
        assert_eq!(
            Op::GetMeter(Meter::Swr).required_capability(),
            Capability::ReadMeters
        );
        assert!(!Op::GetMeter(Meter::Alc).transmits());
        assert_eq!(Op::GetFirmware.required_capability(), Capability::Identify);
        assert_eq!(
            Op::SendCw("x".into()).required_capability(),
            Capability::SendCw
        );
        assert_eq!(Op::StopCw.required_capability(), Capability::SendCw);
    }

    // Tests [@ANCHOR: radioctl_core_op_transmits]
    #[test]
    fn keying_and_cw_transmit_but_unkeying_does_not() {
        assert!(Op::SetPtt(true).transmits());
        assert!(Op::SendCw("cq".into()).transmits());
        assert!(!Op::SetPtt(false).transmits());
        assert!(!Op::StopCw.transmits());
        assert!(!Op::SetFrequency(Vfo::A, Hz(7_000_000)).transmits());
    }

    // Tests [@ANCHOR: radioctl_core_op_is_emergency]
    #[test]
    fn unkeying_and_stopping_morse_are_emergencies() {
        assert!(Op::SetPtt(false).is_emergency());
        assert!(Op::StopCw.is_emergency());
        assert!(!Op::SetPtt(true).is_emergency());
        assert!(!Op::SendCw("cq".into()).is_emergency());
        assert!(!Op::GetMode.is_emergency());
    }

    // Tests [@ANCHOR: radioctl_core_op_is_unkey]
    #[test]
    fn only_unkeying_is_the_unkey() {
        assert!(Op::SetPtt(false).is_unkey());
        assert!(!Op::StopCw.is_unkey());
        assert!(!Op::SetPtt(true).is_unkey());
    }
}

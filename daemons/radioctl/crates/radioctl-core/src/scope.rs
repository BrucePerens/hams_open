// SPDX-License-Identifier: AGPL-3.0-or-later
//! The spectrum scope (band scope) as typed data: how the frequency axis is anchored, and the
//! waveform frames a rig sends.
//!
//! A rig with a scope has the capability [`crate::capability::Capability::Scope`]. Settings (span,
//! axis mode, fixed edges, reference level, waveform output) are ordinary operations
//! ([`crate::op::Op::SetScopeSpan`] and friends) whose reads land in the cache like any other
//! field. Waveform frames are not cache fields (a frame owns its samples and arrives many times a
//! second): the engine returns each complete sweep as [`crate::engine::Inbound::ScopeFrame`] and
//! the actor publishes it on [`crate::rig::RigHandle::scope_frames`], a bounded stream of its own
//! that never goes through the status-change stream.
//!
//! The relay's `ScopeSource` frame (centre and span in hertz, bins, a sequence number) is built
//! from a [`ScopeFrame`] by the relay; this crate keeps the rig's own integers (hertz and the
//! rig's amplitude units) and does not guess a dB calibration.

use crate::units::Hz;

/// How the scope's frequency axis is anchored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeAxis {
    /// Centred on the receive frequency.
    Center,
    /// Between fixed edges.
    Fixed,
    /// Scrolling, centred.
    ScrollCenter,
    /// Scrolling, fixed edges.
    ScrollFixed,
}

/// A scope setting that is a small choice code of the rig's own (a switch or a short list), as
/// opposed to the span, axis, reference level and edges that have operations of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeSetting {
    /// The scope display on the rig's screen: 0 off, 1 on.
    Display,
    /// Which scope layout the rig shows (bandscope, waterfall, audio scope and so on).
    Layout,
    /// Which of the rig's stored fixed-mode ranges is in use (0 is the rig's choice of default).
    Range,
    /// Display pause: 0 running, 1 paused.
    Pause,
    /// Which frequencies get a marker: 0 receive only, 1 receive and transmit.
    Marker,
    /// The scope's own attenuator, in steps of 10 dB from 0.
    Attenuator,
    /// Peak hold: 0 off, 1 on.
    MaxHold,
}

/// One complete sweep of a rig's scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeFrame {
    /// Sequence number in the order the actor published frames, from 1.
    pub seq: u64,
    /// The receiver: 0 (Main) or 1 (Sub).
    pub receiver: u8,
    /// How the axis was anchored for this sweep.
    pub axis: ScopeAxis,
    /// Lowest frequency on the axis.
    pub low: Hz,
    /// Highest frequency on the axis.
    pub high: Hz,
    /// True when the receive frequency is outside a fixed scope.
    pub out_of_range: bool,
    /// The largest value a sample can take (the rig's amplitude units; 0 is the display bottom).
    pub amplitude_max: u16,
    /// One amplitude per pixel, lowest frequency first.
    pub samples: Vec<u8>,
}

/// One sweep of a rig's filter scope (the passband display, not the band): a few hundred pixels
/// of amplitude with no frequency axis, since the rig draws it around the receive passband.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterScopeFrame {
    /// Sequence number in the order the actor published filter-scope frames, from 1.
    pub seq: u64,
    /// The receiver: 0 (Main) or 1 (Sub).
    pub receiver: u8,
    /// The largest value a sample can take (the rig's amplitude units; 0 is the display bottom).
    pub amplitude_max: u16,
    /// One amplitude per pixel, lowest audio frequency first.
    pub samples: Vec<u8>,
}

impl FilterScopeFrame {
    // [@ANCHOR: radioctl_core_filter_scope_frame_peak]
    /// The largest sample in the sweep, or 0 for an empty one.
    pub fn peak(&self) -> u8 {
        self.samples.iter().copied().max().unwrap_or(0)
    }
}

impl ScopeFrame {
    // [@ANCHOR: radioctl_core_scope_frame_span]
    /// The width of the axis.
    pub fn span(&self) -> Hz {
        Hz(self.high.0.saturating_sub(self.low.0))
    }

    // [@ANCHOR: radioctl_core_scope_frame_centre]
    /// The centre of the axis.
    pub fn centre(&self) -> Hz {
        Hz(self.low.0 + self.span().0 / 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_core_scope_frame_span]
    // Tests [@ANCHOR: radioctl_core_scope_frame_centre]
    #[test]
    fn span_and_centre_come_from_the_edges() {
        let frame = ScopeFrame {
            seq: 1,
            receiver: 0,
            axis: ScopeAxis::Center,
            low: Hz(14_070_000),
            high: Hz(14_080_000),
            out_of_range: false,
            amplitude_max: 160,
            samples: vec![0; 4],
        };
        assert_eq!(frame.span(), Hz(10_000));
        assert_eq!(frame.centre(), Hz(14_075_000));
    }

    // Tests [@ANCHOR: radioctl_core_filter_scope_frame_peak]
    #[test]
    fn filter_scope_peak_is_the_largest_sample() {
        let mut frame = FilterScopeFrame {
            seq: 1,
            receiver: 0,
            amplitude_max: 50,
            samples: vec![3, 40, 7],
        };
        assert_eq!(frame.peak(), 40);
        frame.samples.clear();
        assert_eq!(frame.peak(), 0);
    }
}

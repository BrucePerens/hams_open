// SPDX-License-Identifier: AGPL-3.0-or-later
//! Typed physical values. Frequencies are integer hertz, never floating point.

/// A frequency in hertz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hz(pub u64);

impl Hz {
    // [@ANCHOR: radioctl_core_hz_from_khz]
    /// Builds a frequency from whole kilohertz, or `None` if the value overflows.
    pub fn from_khz(khz: u64) -> Option<Hz> {
        khz.checked_mul(1_000).map(Hz)
    }

    // [@ANCHOR: radioctl_core_hz_as_mhz]
    /// The frequency in megahertz, for display only.
    pub fn as_mhz(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }
}

/// An operating mode. Per-family code tables live in the rig description, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mode {
    /// Lower sideband.
    Lsb,
    /// Upper sideband.
    Usb,
    /// Amplitude modulation.
    Am,
    /// CW (Morse).
    Cw,
    /// CW, reverse sideband.
    CwR,
    /// RTTY / FSK.
    Rtty,
    /// RTTY / FSK, reverse.
    RttyR,
    /// Narrow FM.
    Fm,
    /// Wide FM.
    Wfm,
}

const MODE_NAMES: [(Mode, &str); 9] = [
    (Mode::Lsb, "lsb"),
    (Mode::Usb, "usb"),
    (Mode::Am, "am"),
    (Mode::Cw, "cw"),
    (Mode::CwR, "cw-r"),
    (Mode::Rtty, "rtty"),
    (Mode::RttyR, "rtty-r"),
    (Mode::Fm, "fm"),
    (Mode::Wfm, "wfm"),
];

impl Mode {
    // [@ANCHOR: radioctl_core_mode_name]
    /// The name used in rig description files.
    pub fn name(self) -> &'static str {
        MODE_NAMES
            .iter()
            .find(|(m, _)| *m == self)
            .map(|(_, n)| *n)
            .unwrap_or("unknown")
    }

    // [@ANCHOR: radioctl_core_mode_from_name]
    /// The mode with a file-format name.
    pub fn from_name(name: &str) -> Option<Mode> {
        MODE_NAMES.iter().find(|(_, n)| *n == name).map(|(m, _)| *m)
    }
}

/// A VFO. Only the VFOs a description lists are usable on a given rig.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Vfo {
    /// VFO A (the receive/operating VFO unless split).
    A,
    /// VFO B.
    B,
}

impl Vfo {
    // [@ANCHOR: radioctl_core_vfo_name]
    /// The name used in rig description files.
    pub fn name(self) -> &'static str {
        match self {
            Vfo::A => "a",
            Vfo::B => "b",
        }
    }

    // [@ANCHOR: radioctl_core_vfo_from_name]
    /// The VFO with a file-format name.
    pub fn from_name(name: &str) -> Option<Vfo> {
        match name {
            "a" => Some(Vfo::A),
            "b" => Some(Vfo::B),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_core_mode_name]
    // Tests [@ANCHOR: radioctl_core_mode_from_name]
    #[test]
    fn mode_names_round_trip() {
        for (mode, name) in MODE_NAMES {
            assert_eq!(mode.name(), name);
            assert_eq!(Mode::from_name(name), Some(mode));
        }
        assert_eq!(Mode::from_name("ssb"), None);
    }

    // Tests [@ANCHOR: radioctl_core_vfo_name]
    // Tests [@ANCHOR: radioctl_core_vfo_from_name]
    #[test]
    fn vfo_names_round_trip() {
        for vfo in [Vfo::A, Vfo::B] {
            assert_eq!(Vfo::from_name(vfo.name()), Some(vfo));
        }
        assert_eq!(Vfo::from_name("c"), None);
    }

    // Tests [@ANCHOR: radioctl_core_hz_from_khz]
    #[test]
    fn from_khz_scales_and_detects_overflow() {
        assert_eq!(Hz::from_khz(14_074), Some(Hz(14_074_000)));
        assert_eq!(Hz::from_khz(u64::MAX), None);
    }

    // Tests [@ANCHOR: radioctl_core_hz_as_mhz]
    #[test]
    fn as_mhz_displays_megahertz() {
        assert!((Hz(14_074_000).as_mhz() - 14.074).abs() < 1e-9);
    }
}

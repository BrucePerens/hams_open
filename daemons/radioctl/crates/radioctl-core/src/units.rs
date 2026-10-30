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

#[cfg(test)]
mod tests {
    use super::*;

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

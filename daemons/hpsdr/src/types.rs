// SPDX-License-Identifier: AGPL-3.0-or-later
//! Types shared by Protocol 1 and Protocol 2: boards, sample rates and hardware status.
//!
//! Sources (facts only, all `verified=unverified` until checked against a real radio):
//! * Protocol 2 spec v4.4, "Discovery Reply Packet" (board type table) and Appendix A
//!   (Hermes calibration constants).
//! * Metis "How it works" v1.33, "Metis Discovery reply format" (Protocol 1 board IDs).

use crate::error::HpsdrError;

/// Which wire protocol a radio speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Metis/USB-framed protocol over UDP port 1024 (Hermes, ANAN-10, ANAN-100 original firmware).
    P1,
    /// The newer "openHPSDR Ethernet Protocol" with one UDP port per stream.
    P2,
}

/// A board type as reported by discovery. Protocol 1 and 2 number boards differently; both map here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    /// Metis or Atlas based system.
    Metis,
    /// Hermes: ANAN-10, ANAN-100.
    Hermes,
    /// Hermes variant: ANAN-10E, ANAN-100B (Protocol 2 board type 2).
    HermesB,
    /// Angelia: ANAN-100D.
    Angelia,
    /// Orion: ANAN-200D.
    Orion,
    /// Orion Mk II: ANAN-7000DLE, ANAN-8000DLE.
    OrionMk2,
    /// Hermes Lite.
    HermesLite,
    /// Saturn: ANAN-G2.
    Saturn,
    /// An identifier this crate does not know (kept, never rejected).
    Other(u8),
}

impl Board {
    // [@ANCHOR: hpsdr_types_board_from_p1_id]
    /// Protocol 1 `Board_ID` (Metis v1.33: 0 Metis, 1 Hermes, 2 Griffin, 4 Angelia, 5 Orion, 6 Hermes Lite).
    pub fn from_p1_id(id: u8) -> Self {
        match id {
            0 => Self::Metis,
            1 => Self::Hermes,
            4 => Self::Angelia,
            5 => Self::Orion,
            6 => Self::HermesLite,
            n => Self::Other(n),
        }
    }

    // [@ANCHOR: hpsdr_types_board_from_p2_type]
    /// Protocol 2 board type (spec v4.4 Discovery Reply byte 11).
    pub fn from_p2_type(t: u8) -> Self {
        match t {
            0 => Self::Metis,
            1 => Self::Hermes,
            2 => Self::HermesB,
            3 => Self::Angelia,
            4 => Self::Orion,
            5 => Self::OrionMk2,
            6 => Self::HermesLite,
            10 => Self::Saturn,
            n => Self::Other(n),
        }
    }

    // [@ANCHOR: hpsdr_types_board_dsp_clock]
    /// DSP clock in hertz, needed for phase words (spec v4.4 Appendix A gives 122.88 MHz for Hermes);
    /// `None` for boards whose clock this crate has not read from the spec.
    pub fn dsp_clock_hz(self) -> Option<f64> {
        match self {
            Self::Hermes | Self::HermesB | Self::Angelia | Self::Orion | Self::OrionMk2 => {
                Some(122_880_000.0)
            }
            _ => None,
        }
    }

    // [@ANCHOR: hpsdr_types_board_power_constant]
    /// Constant 2 of the forward/reverse power formula (spec v4.4 Appendix A, Hermes: 0.09 for the
    /// ANAN-10, 0.095 for the ANAN-100). The ANAN-10 and ANAN-100 share board type 1, so the caller
    /// chooses by model; `None` when the spec gives no constant for the board.
    pub fn power_constant(self, anan10: bool) -> Option<f64> {
        match self {
            Self::Hermes => Some(if anan10 { 0.09 } else { 0.095 }),
            _ => None,
        }
    }
}

/// A DDC sample rate. Protocol 1 supports the first four; Protocol 2 all six (spec v4.4 DDC Specific
/// bytes 18-19: 48/96/192/384/768/1536 ksps).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rate {
    /// 48 ksps.
    K48,
    /// 96 ksps.
    K96,
    /// 192 ksps.
    K192,
    /// 384 ksps.
    K384,
    /// 768 ksps (Protocol 2 only).
    K768,
    /// 1536 ksps (Protocol 2 only).
    K1536,
}

impl Rate {
    // [@ANCHOR: hpsdr_types_rate_hz]
    /// The rate in hertz.
    pub fn hz(self) -> u32 {
        match self {
            Self::K48 => 48_000,
            Self::K96 => 96_000,
            Self::K192 => 192_000,
            Self::K384 => 384_000,
            Self::K768 => 768_000,
            Self::K1536 => 1_536_000,
        }
    }

    // [@ANCHOR: hpsdr_types_rate_from_ksps]
    /// Parse a rate in kilohertz (48, 96, 192, 384, 768, 1536).
    pub fn from_ksps(k: u16) -> Result<Self, HpsdrError> {
        match k {
            48 => Ok(Self::K48),
            96 => Ok(Self::K96),
            192 => Ok(Self::K192),
            384 => Ok(Self::K384),
            768 => Ok(Self::K768),
            1536 => Ok(Self::K1536),
            n => Err(HpsdrError::Config(format!("sample rate {n} ksps"))),
        }
    }

    // [@ANCHOR: hpsdr_types_rate_p1_code]
    /// The Protocol 1 speed code (USB protocol v1.60, C0=0 C1[1:0]: 00 48k, 01 96k, 10 192k, 11 384k).
    pub fn p1_code(self) -> Result<u8, HpsdrError> {
        match self {
            Self::K48 => Ok(0),
            Self::K96 => Ok(1),
            Self::K192 => Ok(2),
            Self::K384 => Ok(3),
            _ => Err(HpsdrError::Config(
                "Protocol 1 supports 48/96/192/384 ksps".into(),
            )),
        }
    }

    // [@ANCHOR: hpsdr_types_rate_from_p1_code]
    /// Inverse of [`Rate::p1_code`] (only the low two bits are used).
    pub fn from_p1_code(c: u8) -> Self {
        match c & 3 {
            0 => Self::K48,
            1 => Self::K96,
            2 => Self::K192,
            _ => Self::K384,
        }
    }
}

/// Hardware status gathered from Protocol 1 command-and-control bytes or the Protocol 2 high-priority
/// status packet. Raw ADC counts are kept; the methods convert them.
///
/// Temperature is deliberately absent: neither the Protocol 1 USB spec v1.60 nor the Protocol 2
/// spec v4.4 (Appendix A, Hermes) defines a temperature report for ANAN/Hermes hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HardwareStatus {
    /// ADC overload flags, ADC0 first.
    pub adc_overload: [bool; 4],
    /// Largest ADC magnitude seen in the last status period, per ADC (Protocol 2 only).
    pub adc_max_magnitude: [u16; 2],
    /// Front-panel/PTT input active (not a transmit state: a request from the radio's own jack).
    pub ptt_input: bool,
    /// CW dot input active.
    pub dot_input: bool,
    /// CW dash input active.
    pub dash_input: bool,
    /// Exciter forward power, raw 12-bit ADC count.
    pub exciter_raw: u16,
    /// PA forward power, raw 12-bit ADC count.
    pub forward_raw: u16,
    /// PA reverse power, raw 12-bit ADC count.
    pub reverse_raw: u16,
    /// Supply voltage, raw 12-bit ADC count.
    pub supply_raw: u16,
    /// Firmware version byte as reported.
    pub firmware: u8,
    /// Reference PLL locked (Protocol 2 only).
    pub pll_locked: Option<bool>,
}

impl HardwareStatus {
    // [@ANCHOR: hpsdr_types_status_supply_volts]
    /// Supply volts: `adc / 4095 * 3.3` (spec v4.4 Appendix A, Hermes).
    pub fn supply_volts(&self) -> f64 {
        f64::from(self.supply_raw) / 4095.0 * 3.3
    }

    // [@ANCHOR: hpsdr_types_status_watts]
    /// Convert a raw power count to watts: `(adc / 4095 * 3.3)^2 / c2` (spec v4.4 Appendix A).
    pub fn watts(raw: u16, c2: f64) -> f64 {
        let v = f64::from(raw) / 4095.0 * 3.3;
        v * v / c2
    }

    // [@ANCHOR: hpsdr_types_status_swr]
    /// Standing wave ratio from forward and reverse power counts; `None` with no forward power.
    /// Reverse above forward is reported as infinity.
    pub fn swr(&self, c2: f64) -> Option<f64> {
        let f = Self::watts(self.forward_raw, c2);
        let r = Self::watts(self.reverse_raw, c2);
        if f <= 0.0 {
            return None;
        }
        let rho = (r / f).sqrt();
        if rho >= 1.0 {
            Some(f64::INFINITY)
        } else {
            Some((1.0 + rho) / (1.0 - rho))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_ids_map_and_unknown_is_kept() {
        // Tests [@ANCHOR: hpsdr_types_board_from_p1_id]
        // Tests [@ANCHOR: hpsdr_types_board_from_p2_type]
        // Tests [@ANCHOR: hpsdr_types_board_dsp_clock]
        // Tests [@ANCHOR: hpsdr_types_board_power_constant]
        assert_eq!(Board::from_p1_id(1), Board::Hermes);
        assert_eq!(Board::from_p1_id(6), Board::HermesLite);
        assert_eq!(Board::from_p1_id(2), Board::Other(2));
        assert_eq!(Board::from_p2_type(10), Board::Saturn);
        assert_eq!(Board::from_p2_type(2), Board::HermesB);
        assert_eq!(Board::from_p2_type(99), Board::Other(99));
        assert_eq!(Board::Hermes.dsp_clock_hz(), Some(122_880_000.0));
        assert_eq!(Board::HermesLite.dsp_clock_hz(), None);
        assert_eq!(Board::Hermes.power_constant(true), Some(0.09));
        assert_eq!(Board::Hermes.power_constant(false), Some(0.095));
        assert_eq!(Board::Orion.power_constant(false), None);
    }

    #[test]
    fn rates_round_trip() {
        // Tests [@ANCHOR: hpsdr_types_rate_hz]
        // Tests [@ANCHOR: hpsdr_types_rate_from_ksps]
        // Tests [@ANCHOR: hpsdr_types_rate_p1_code]
        // Tests [@ANCHOR: hpsdr_types_rate_from_p1_code]
        for (k, code) in [(48u16, 0u8), (96, 1), (192, 2), (384, 3)] {
            let r = Rate::from_ksps(k).unwrap_or(Rate::K48);
            assert_eq!(r.hz(), u32::from(k) * 1000);
            assert_eq!(r.p1_code(), Ok(code));
            assert_eq!(Rate::from_p1_code(code), r);
        }
        assert_eq!(Rate::K768.hz(), 768_000);
        assert_eq!(Rate::K1536.hz(), 1_536_000);
        assert!(Rate::K768.p1_code().is_err());
        assert!(Rate::from_ksps(100).is_err());
        assert_eq!(Rate::from_ksps(1536), Ok(Rate::K1536));
        assert_eq!(Rate::from_ksps(768), Ok(Rate::K768));
    }

    #[test]
    fn power_and_swr_from_counts() {
        // Tests [@ANCHOR: hpsdr_types_status_supply_volts]
        // Tests [@ANCHOR: hpsdr_types_status_watts]
        // Tests [@ANCHOR: hpsdr_types_status_swr]
        let s = HardwareStatus {
            supply_raw: 4095,
            forward_raw: 4095,
            reverse_raw: 0,
            ..Default::default()
        };
        assert!((s.supply_volts() - 3.3).abs() < 1e-9);
        assert!((HardwareStatus::watts(4095, 0.095) - 3.3 * 3.3 / 0.095).abs() < 1e-9);
        assert_eq!(s.swr(0.095), Some(1.0));
        let mismatched = HardwareStatus {
            forward_raw: 4000,
            reverse_raw: 1000,
            ..Default::default()
        };
        let swr = mismatched.swr(0.095).unwrap_or(0.0);
        assert!(swr > 1.0 && swr < 10.0);
        let rho = (1000.0f64 / 4000.0).powi(2).sqrt();
        assert!((swr - (1.0 + rho) / (1.0 - rho)).abs() < 1e-9);
        assert_eq!(
            HardwareStatus::default().swr(0.095),
            None,
            "no forward power"
        );
        let bad = HardwareStatus {
            forward_raw: 100,
            reverse_raw: 200,
            ..Default::default()
        };
        assert_eq!(bad.swr(0.095), Some(f64::INFINITY));
    }
}

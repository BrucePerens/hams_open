// SPDX-License-Identifier: AGPL-3.0-or-later
//! The AD9859 register images the VNWA's microcontroller loads into the two DDS chips. Layout from the
//! AD9859 data sheet (Rev. B, Table 5): CFR1 at serial address 0x00 (4 bytes), CFR2 at 0x01 (3 bytes),
//! ASF 0x02, ARR 0x03, FTW0 0x04 (4 bytes), POW0 0x05 (2 bytes). The vendor program sends the
//! register values little endian (least significant byte first); the microcontroller sends them on.

/// Request 0x40 carries CFR1 (4 bytes) then CFR2 (3 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cfr {
    pub cfr1: u32,
    pub cfr2: u32,
}

impl Cfr {
    /// Seven payload bytes of request 0x40.
    pub fn from_bytes(b: &[u8]) -> Result<Cfr, String> {
        // [@ANCHOR: vnwa_open_cfr_decode]
        if b.len() != 7 {
            return Err(format!("request 0x40 payload is 7 bytes, got {}", b.len()));
        }
        Ok(Cfr {
            cfr1: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            cfr2: u32::from_le_bytes([b[4], b[5], b[6], 0]),
        })
    }

    pub fn to_bytes(self) -> [u8; 7] {
        let (a, c) = (self.cfr1.to_le_bytes(), self.cfr2.to_le_bytes());
        [a[0], a[1], a[2], a[3], c[0], c[1], c[2]]
    }

    /// CFR2<7:3>: the REFCLK multiplier. 4 to 20 multiply the reference clock; 0 to 3 bypass the PLL.
    pub fn refclk_multiplier(self) -> u8 {
        ((self.cfr2 & 0xff) >> 3) as u8
    }

    /// CFR2<2>: VCO range.
    pub fn vco_range_high(self) -> bool {
        self.cfr2 & 0x04 != 0
    }

    /// CFR2<1:0>: charge pump current.
    pub fn charge_pump(self) -> u8 {
        (self.cfr2 & 0x03) as u8
    }

    /// CFR1<8>: LSB-first serial transfers. The vendor program leaves this clear (MSB first).
    pub fn lsb_first(self) -> bool {
        self.cfr1 & 0x100 != 0
    }
}

/// The core clock for a reference frequency and the multiplier in CFR2.
pub fn core_clock_hz(reference_hz: f64, cfr: Cfr) -> f64 {
    reference_hz * f64::from(cfr.refclk_multiplier())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: vnwa_open_cfr_decode]
    #[test]
    fn the_vendor_setup_word_selects_x15_with_the_high_vco_range() {
        let b = [0x02, 0x00, 0x08, 0x01, 0x7d, 0x00, 0x00];
        let cfr = Cfr::from_bytes(&b).unwrap();
        assert_eq!(cfr.cfr1, 0x0108_0002);
        assert_eq!(cfr.cfr2, 0x7d);
        assert_eq!(cfr.refclk_multiplier(), 15);
        assert!(cfr.vco_range_high());
        assert_eq!(cfr.charge_pump(), 1);
        assert!(!cfr.lsb_first());
        // 24 MHz reference times 15 is the 360 MHz core clock the tuning words imply.
        assert_eq!(core_clock_hz(24.0e6, cfr), 360.0e6);
        assert_eq!(cfr.to_bytes(), b);
    }

    #[test]
    fn a_wrong_length_is_refused() {
        assert!(Cfr::from_bytes(&[0; 6]).is_err());
    }
}

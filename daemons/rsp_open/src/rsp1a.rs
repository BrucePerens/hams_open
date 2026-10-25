// SPDX-License-Identifier: AGPL-3.0-or-later
//! RSP1A (USB 1df7:3000) board layer: band table, LNA-state tables and the two board bytes.
//!
//! The RSP1A shares the chipset layer with the RSP2pro (MSi2500 registers, tuner words, ADC
//! clock, stream formats: `docs/PROTOCOL.md` 2.4); only what is written to the board's
//! expander ports with request 0x49 differs (2.5-2.8). Section numbers refer to that document.
//!
//! Byte 0x12 (port A): bits 4:0 band filter, bits 7:5 front-end attenuator step.
//! Byte 0x13 (port B): bit 0 DAB notch (0 = on), bit 1 front-end step, bit 2 FM/MW notch
//! (0 = on), bit 4 bias-T (0 = on); bits 7, 6 and 3 are always 1 and bit 5 always 0.

use crate::encode::{Band, FrontEndStep, GainTable, LoPlan, TunerMode, RF_MAX_HZ, RF_MIN_HZ};

const M: u64 = 1_000_000;

/// Band table (2.6). The filter field is byte 0x12 bits 4:0. As on the RSP2pro, 375-420 MHz is
/// not offered (the API moves the AM-mode first IF to 144/168 MHz there and runs a VCO
/// calibration); `encode::band_in` refuses that range before this table is searched.
pub const BANDS: [Band; 10] = [
    band(
        RF_MIN_HZ,
        2 * M,
        TunerMode::Am,
        LoPlan::UpBy120M,
        0x00,
        GainTable::Am,
    ),
    band(
        2 * M,
        12 * M,
        TunerMode::Am,
        LoPlan::UpBy120M,
        0x02,
        GainTable::Am,
    ),
    band(
        12 * M,
        30 * M,
        TunerMode::Am,
        LoPlan::UpBy120M,
        0x01,
        GainTable::Am,
    ),
    band(
        30 * M,
        60 * M,
        TunerMode::Am,
        LoPlan::UpBy120M,
        0x03,
        GainTable::Am,
    ),
    band(
        60 * M,
        120 * M,
        TunerMode::Vhf,
        LoPlan::Direct,
        0x04,
        GainTable::Vhf,
    ),
    band(
        120 * M,
        250 * M,
        TunerMode::B3,
        LoPlan::Direct,
        0x14,
        GainTable::Vhf,
    ),
    band(
        250 * M,
        300 * M,
        TunerMode::Am,
        LoPlan::DownBy120M,
        0x10,
        GainTable::AmDown,
    ),
    // 300-380 MHz uses filter 0x08; this driver stops at 375 MHz (see above). 380-420 MHz
    // (filter 0x18) is not offered.
    band(
        300 * M,
        375 * M,
        TunerMode::Am,
        LoPlan::DownBy120M,
        0x08,
        GainTable::AmDown,
    ),
    band(
        420 * M,
        1000 * M,
        TunerMode::B45,
        LoPlan::Direct,
        0x04,
        GainTable::Uhf,
    ),
    band(
        1000 * M,
        RF_MAX_HZ + 1,
        TunerMode::L,
        LoPlan::Direct,
        0x14,
        GainTable::L,
    ),
];

const fn band(
    lo_hz: u64,
    hi_hz: u64,
    mode: TunerMode,
    lo_plan: LoPlan,
    filter: u8,
    gain_table: GainTable,
) -> Band {
    Band {
        lo_hz,
        hi_hz,
        mode,
        lo_plan,
        filter,
        gain_table,
    }
}

// Attenuator step codes in byte 0x12 bits 7:5 (0xe_, 0xc_, 0xa_, 0x6_ in the captures).
const A7: u8 = 0b111;
const A6: u8 = 0b110;
const A5: u8 = 0b101;
const A3: u8 = 0b011;

/// One LNA state. `FrontEndStep::board_step` holds the RSP1A's two board fields packed as
/// `attenuator code (byte 0x12 bits 7:5) << 1 | byte 0x13 bit 1`.
const fn st(atten: u8, b13_step: u8, am_buf: u8, mixer_gr: bool, lna_gr: bool) -> FrontEndStep {
    FrontEndStep {
        board_step: (atten << 1) | (b13_step & 1),
        am_buf,
        mixer_gr,
        lna_gr,
    }
}

/// LNA-state tables (2.8), one per gain table.
pub fn lna_table(t: GainTable) -> &'static [FrontEndStep] {
    // [@ANCHOR: rsp_rsp1a_lna_table]
    const AM: [FrontEndStep; 7] = [
        st(A7, 0, 0, false, false),
        st(A6, 0, 0, false, false),
        st(A5, 0, 0, false, false),
        st(A3, 0, 0, false, false),
        st(A3, 0, 0, true, false),
        st(A3, 0, 3, false, false),
        st(A3, 0, 3, true, false),
    ];
    const VHF: [FrontEndStep; 10] = [
        st(A7, 0, 0, false, false),
        st(A6, 0, 0, false, false),
        st(A5, 0, 0, false, false),
        st(A3, 0, 0, false, false),
        st(A7, 1, 0, false, false),
        st(A6, 1, 0, false, false),
        st(A5, 1, 0, false, false),
        st(A3, 1, 0, false, false),
        st(A3, 1, 0, true, false),
        st(A3, 1, 0, false, true),
    ];
    const AM_DOWN: [FrontEndStep; 10] = [
        st(A7, 0, 0, false, false),
        st(A6, 0, 0, false, false),
        st(A5, 0, 0, false, false),
        st(A3, 0, 0, false, false),
        st(A7, 1, 0, false, false),
        st(A6, 1, 0, false, false),
        st(A5, 1, 0, false, false),
        st(A3, 1, 0, false, false),
        st(A3, 1, 0, true, false),
        st(A3, 1, 3, false, false),
    ];
    const UHF: [FrontEndStep; 10] = [
        st(A7, 0, 0, false, false),
        st(A7, 0, 0, false, true),
        st(A6, 0, 0, false, true),
        st(A5, 0, 0, false, true),
        st(A7, 1, 0, false, false),
        st(A7, 1, 0, false, true),
        st(A6, 1, 0, false, true),
        st(A5, 1, 0, false, true),
        st(A3, 1, 0, false, true),
        st(A3, 1, 0, true, true),
    ];
    const L: [FrontEndStep; 9] = [
        st(A7, 0, 0, false, false),
        st(A6, 0, 0, false, false),
        st(A5, 0, 0, false, false),
        st(A7, 1, 0, false, false),
        st(A6, 1, 0, false, false),
        st(A5, 1, 0, false, false),
        st(A3, 1, 0, false, false),
        st(A3, 1, 0, false, true),
        st(A3, 1, 0, true, true),
    ];
    match t {
        GainTable::Am | GainTable::HiZ => &AM,
        GainTable::Vhf => &VHF,
        GainTable::AmDown => &AM_DOWN,
        GainTable::Uhf => &UHF,
        GainTable::L => &L,
    }
}

/// Byte 0x12 (expander port A): attenuator code in bits 7:5, band filter in bits 4:0.
pub fn board_a_byte(filter: u8, board_step: u8) -> u8 {
    // [@ANCHOR: rsp_rsp1a_board_a_byte]
    (((board_step >> 1) & 7) << 5) | (filter & 0x1f)
}

/// Byte 0x13 (expander port B). All switches are active low: a notch or the bias-T is on when
/// its bit is 0. Bits 7, 6 and 3 are 1 and bit 5 is 0 in every capture.
pub fn board_b_byte(board_step: u8, fm_mw_notch: bool, dab_notch: bool, bias_t: bool) -> u8 {
    // [@ANCHOR: rsp_rsp1a_board_b_byte]
    let fixed = 0b1100_1000;
    let bias = if bias_t { 0 } else { 1 << 4 };
    let notch = if fm_mw_notch { 0 } else { 1 << 2 };
    let dab = if dab_notch { 0 } else { 1 };
    fixed | bias | notch | ((board_step & 1) << 1) | dab
}

/// Byte 0x13 written in the middle of the open sequence (0xdf in every captured open, where
/// both notches and the bias-T were off): the front-end step bit set. With a notch on at open
/// the notch bits follow the setting [guess; not captured].
pub fn open_transient_b(fm_mw_notch: bool, dab_notch: bool) -> u8 {
    // [@ANCHOR: rsp_rsp1a_open_transient_b]
    board_b_byte(1, fm_mw_notch, dab_notch, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::{band_in, lo_hz, mode_byte, pll, r0, r1, tuner_word, Antenna};
    use crate::encode::{Bandwidth, IfMode};

    /// (RF Hz, byte 0x12 at LNA state 0, r0 with zero IF and 200 kHz BW, r2) from the RSP1A
    /// frequency-sweep and band-edge captures (2.6).
    #[test]
    // Tests [@ANCHOR: rsp_rsp1a_board_a_byte]
    // Tests [@ANCHOR: rsp_encode_band_in]
    fn rsp1a_band_table_matches_capture() {
        let cases: &[(u64, u8, u32, u32)] = &[
            (1_000, 0xe0, 0x043e10, 0x140002),
            (500_000, 0xe0, 0x043e10, 0x140f92),
            (1_000_000, 0xe0, 0x043e10, 0x141f32),
            (1_950_000, 0xe0, 0x043e10, 0x143ce2),
            (2_000_000, 0xe2, 0x043e10, 0x143e72),
            (2_200_000, 0xe2, 0x043e10, 0x1444b2),
            (10_000_000, 0xe2, 0x043e10, 0x157cf2),
            (11_900_000, 0xe2, 0x043e10, 0x15b852),
            (12_000_000, 0xe1, 0x043e10, 0x15bb72),
            (29_750_000, 0xe1, 0x043e10, 0x18b3a2),
            (30_000_000, 0xe3, 0x043e10, 0x18bb72),
            (55_000_000, 0xe3, 0x043e10, 0x1d1f32),
            (59_250_000, 0xe3, 0x043e10, 0x1da402),
            (60_000_000, 0xe4, 0x043420, 0x13bb72),
            (100_000_000, 0xe4, 0x043420, 0x213e72),
            (120_000_000, 0xf4, 0x043440, 0x13bb72),
            (200_000_000, 0xf4, 0x043440, 0x213e72),
            (250_000_000, 0xf0, 0x043e10, 0x157cf2),
            (300_000_000, 0xe8, 0x043e10, 0x1dbb72),
            (420_000_000, 0xe4, 0x043480, 0x115db2),
            (450_000_000, 0xe4, 0x043480, 0x128c92),
            (1_000_000_000, 0xf4, 0x043500, 0x149c32),
            (1_500_000_000, 0xf4, 0x043500, 0x1f2ed2),
        ];
        for &(rf, b12, r0v, r2v) in cases {
            let b = band_in(&BANDS, rf).unwrap();
            let s0 = lna_table(b.gain_table)[0];
            assert_eq!(board_a_byte(b.filter, s0.board_step), b12, "B12 at {rf}");
            let w = tuner_word(
                0,
                r0(mode_byte(b.mode, Antenna::A), IfMode::Zero, Bandwidth::K200) >> 4,
            );
            assert_eq!(w, r0v, "r0 at {rf}");
            let p = pll(lo_hz(&b, rf), b.mode.lo_div(), 0).unwrap();
            assert_eq!(p.r2, r2v, "r2 at {rf}");
        }
        assert!(band_in(&BANDS, 380_000_000).is_err());
        assert!(band_in(&BANDS, 400_000_000).is_err());
        assert!(band_in(&BANDS, 374_999_999).is_ok());
    }

    /// (r1, byte 0x12, byte 0x13) per LNA state at IF GR 40, from the RSP1A gain capture, at
    /// the frequencies it was recorded at (2.8).
    #[test]
    // Tests [@ANCHOR: rsp_rsp1a_lna_table]
    // Tests [@ANCHOR: rsp_rsp1a_board_a_byte]
    // Tests [@ANCHOR: rsp_rsp1a_board_b_byte]
    fn rsp1a_lna_tables_match_capture() {
        type Row = (u32, u8, u8);
        let tables: &[(u64, &[Row])] = &[
            (
                100_000_000,
                &[
                    (0x00c281, 0xe4, 0xdd),
                    (0x00c281, 0xc4, 0xdd),
                    (0x00c281, 0xa4, 0xdd),
                    (0x00c281, 0x64, 0xdd),
                    (0x00c281, 0xe4, 0xdf),
                    (0x00c281, 0xc4, 0xdf),
                    (0x00c281, 0xa4, 0xdf),
                    (0x00c281, 0x64, 0xdf),
                    (0x00d281, 0x64, 0xdf),
                    (0x00e281, 0x64, 0xdf),
                ],
            ),
            (
                200_000_000,
                &[
                    (0x00c281, 0xf4, 0xdd),
                    (0x00c281, 0xd4, 0xdd),
                    (0x00c281, 0xb4, 0xdd),
                    (0x00c281, 0x74, 0xdd),
                    (0x00c281, 0xf4, 0xdf),
                    (0x00c281, 0xd4, 0xdf),
                    (0x00c281, 0xb4, 0xdf),
                    (0x00c281, 0x74, 0xdf),
                    (0x00d281, 0x74, 0xdf),
                    (0x00e281, 0x74, 0xdf),
                ],
            ),
            (
                1_000_000,
                &[
                    (0x00c281, 0xe0, 0xdd),
                    (0x00c281, 0xc0, 0xdd),
                    (0x00c281, 0xa0, 0xdd),
                    (0x00c281, 0x60, 0xdd),
                    (0x00d281, 0x60, 0xdd),
                    (0x00ce81, 0x60, 0xdd),
                    (0x00de81, 0x60, 0xdd),
                ],
            ),
            (
                10_000_000,
                &[
                    (0x00c281, 0xe2, 0xdd),
                    (0x00c281, 0xc2, 0xdd),
                    (0x00c281, 0xa2, 0xdd),
                    (0x00c281, 0x62, 0xdd),
                    (0x00d281, 0x62, 0xdd),
                    (0x00ce81, 0x62, 0xdd),
                    (0x00de81, 0x62, 0xdd),
                ],
            ),
            (
                55_000_000,
                &[
                    (0x00c281, 0xe3, 0xdd),
                    (0x00c281, 0xc3, 0xdd),
                    (0x00c281, 0xa3, 0xdd),
                    (0x00c281, 0x63, 0xdd),
                    (0x00d281, 0x63, 0xdd),
                    (0x00ce81, 0x63, 0xdd),
                    (0x00de81, 0x63, 0xdd),
                ],
            ),
            (
                300_000_000,
                &[
                    (0x00c281, 0xe8, 0xdd),
                    (0x00c281, 0xc8, 0xdd),
                    (0x00c281, 0xa8, 0xdd),
                    (0x00c281, 0x68, 0xdd),
                    (0x00c281, 0xe8, 0xdf),
                    (0x00c281, 0xc8, 0xdf),
                    (0x00c281, 0xa8, 0xdf),
                    (0x00c281, 0x68, 0xdf),
                    (0x00d281, 0x68, 0xdf),
                    (0x00ce81, 0x68, 0xdf),
                ],
            ),
            (
                450_000_000,
                &[
                    (0x00c281, 0xe4, 0xdd),
                    (0x00e281, 0xe4, 0xdd),
                    (0x00e281, 0xc4, 0xdd),
                    (0x00e281, 0xa4, 0xdd),
                    (0x00c281, 0xe4, 0xdf),
                    (0x00e281, 0xe4, 0xdf),
                    (0x00e281, 0xc4, 0xdf),
                    (0x00e281, 0xa4, 0xdf),
                    (0x00e281, 0x64, 0xdf),
                    (0x00f281, 0x64, 0xdf),
                ],
            ),
            (
                1_500_000_000,
                &[
                    (0x00c281, 0xf4, 0xdd),
                    (0x00c281, 0xd4, 0xdd),
                    (0x00c281, 0xb4, 0xdd),
                    (0x00c281, 0xf4, 0xdf),
                    (0x00c281, 0xd4, 0xdf),
                    (0x00c281, 0xb4, 0xdf),
                    (0x00c281, 0x74, 0xdf),
                    (0x00e281, 0x74, 0xdf),
                    (0x00f281, 0x74, 0xdf),
                ],
            ),
        ];
        for &(rf, rows) in tables {
            let b = band_in(&BANDS, rf).unwrap();
            let t = lna_table(b.gain_table);
            assert_eq!(t.len(), rows.len(), "state count at {rf}");
            for (i, (s, &(w1, b12, b13))) in t.iter().zip(rows).enumerate() {
                assert_eq!(r1(40, s), w1, "r1 at {rf} state {i}");
                assert_eq!(board_a_byte(b.filter, s.board_step), b12, "B12 {rf}/{i}");
                assert_eq!(
                    board_b_byte(s.board_step, false, false, false),
                    b13,
                    "B13 {rf}/{i}"
                );
            }
        }
    }

    /// Notch, DAB notch and bias-T bits (toggle capture, 2.7).
    #[test]
    // Tests [@ANCHOR: rsp_rsp1a_board_b_byte]
    // Tests [@ANCHOR: rsp_rsp1a_open_transient_b]
    fn rsp1a_front_byte_bits() {
        assert_eq!(board_b_byte(0, false, false, false), 0xdd);
        assert_eq!(board_b_byte(0, true, false, false), 0xd9);
        assert_eq!(board_b_byte(0, false, true, false), 0xdc);
        assert_eq!(board_b_byte(0, false, false, true), 0xcd);
        // Notch on with LNA state 5 at 100 MHz.
        let s5 = lna_table(GainTable::Vhf)[5];
        assert_eq!(board_b_byte(s5.board_step, true, false, false), 0xdb);
        assert_eq!(board_a_byte(0x04, s5.board_step), 0xc4);
        assert_eq!(open_transient_b(false, false), 0xdf);
    }
}

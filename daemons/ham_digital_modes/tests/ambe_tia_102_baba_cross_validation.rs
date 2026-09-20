// SPDX-License-Identifier: LGPL-3.0-or-later
//! Regression tests from the TIA-102.BABA cross-validation against the mbelib and kchmck `imbe.rs` decoders
//! (`docs/references/tia_102_baba_cross_validation.md`).
//!
//! The golden numbers below were produced by running small frames, generated here from raw quantizer indices,
//! through mbelib 1.3.0 (commit 9a04ed5c) with its one wrong table entry corrected, and are this project's own
//! measurements (no external source is copied). Each disagreement between an external decoder and this crate
//! was settled by reading the standard itself; the tests guard both directions, so nobody "fixes" this crate
//! toward an external decoder's mistake.

use ham_digital_modes::ambe::float::tia_102_baba::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::float::tia_102_baba::parameter_encoding::decode_voicing_decisions_per_harmonic;
use ham_digital_modes::ambe::float::tia_102_baba::tables::gain_bit_allocation;

/// Annex F, page 72 of the standard: for L = 23 the fifth gain element (`b_hat_6`) has 4 bits and step 0.058, the
/// same as L = 22 and L = 24. mbelib's table says 0.068 for L = 23 (a typo); `imbe.rs` and the standard agree
/// with this crate.
#[test]
fn annex_f_row_l23_b6_step_is_0_058_not_mbelibs_0_068() {
    assert_eq!(gain_bit_allocation(23, 5), Some((4, 0.058)));
    assert_eq!(gain_bit_allocation(22, 5), Some((4, 0.058)));
    assert_eq!(gain_bit_allocation(24, 5), Some((4, 0.058)));
}

/// Eq. 50-51 (page 26 of the standard): harmonic `l` uses band `floor((l+2)/3)` for `l <= 36` and band 12 for
/// every harmonic above 36, and band `k` is bit `K - k` of `b_hat_1`. `imbe.rs` gets this wrong for `L > 36`
/// (it shifts the whole map); mbelib and this crate agree with the standard.
#[test]
fn voicing_expansion_above_36_harmonics_follows_eq_50_and_51() {
    let l = 54;
    let k = 12;
    let only_band_1 = decode_voicing_decisions_per_harmonic(0x800, k, l);
    let voiced: Vec<usize> = (0..l as usize).filter(|&i| only_band_1[i]).map(|i| i + 1).collect();
    assert_eq!(voiced, vec![1, 2, 3]);
    let only_band_12 = decode_voicing_decisions_per_harmonic(0x001, k, l);
    let voiced: Vec<usize> = (0..l as usize).filter(|&i| only_band_12[i]).map(|i| i + 1).collect();
    assert_eq!(voiced, (34..=54).collect::<Vec<usize>>());
}

/// One golden frame: the wire words, and what mbelib reports for it.
struct Golden {
    /// The eight code vectors `c_hat_0..c_hat_7` (the standard's wire layer, before interleaving).
    c: [u32; 8],
    /// Reset the decoder before this frame.
    reset: bool,
    l: u32,
    k: u32,
    w0: f64,
    voiced: &'static str,
    /// `log2` of the first six unenhanced spectral amplitudes.
    log2_head: [f64; 6],
    /// Sum of `log2` over all harmonics, a checksum of the whole amplitude vector.
    log2_sum: f64,
}

include!("ambe_tia_102_baba_cross_validation_golden.in");

#[test]
fn decoder_matches_mbelib_on_golden_frames() {
    let mut dec = DecoderState::new();
    for (i, g) in GOLDEN.iter().enumerate() {
        if g.reset {
            dec = DecoderState::new();
        }
        let Some(FrameOutcome::Decoded(p)) = dec.decode_parameters(g.c) else {
            panic!("golden frame {i}: not decoded");
        };
        assert_eq!((p.l_hat, p.k_hat), (g.l, g.k), "frame {i}");
        assert!((p.omega0_tilde - g.w0).abs() < 1e-7, "frame {i}: w0 {} vs {}", p.omega0_tilde, g.w0);
        let v: String = p.voiced.iter().map(|&b| if b { '1' } else { '0' }).collect();
        assert_eq!(v, g.voiced, "frame {i}: voicing");
        let log2: Vec<f64> = p.reconstructed_amplitudes.iter().map(|m| m.log2()).collect();
        for (j, want) in g.log2_head.iter().enumerate() {
            assert!((log2[j] - want).abs() < 2e-4, "frame {i}: log2 M_{} = {} vs {}", j + 1, log2[j], want);
        }
        let sum: f64 = log2.iter().sum();
        assert!((sum - g.log2_sum).abs() < 2e-3 * (1.0 + g.log2_sum.abs() / 50.0), "frame {i}: checksum {sum} vs {}", g.log2_sum);
        dec.advance_history(&p);
    }
}

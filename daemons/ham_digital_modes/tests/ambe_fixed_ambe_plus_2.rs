// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::ambe_plus_2::decode::dequantize` against its floating-point sibling
//! across a representative sweep of real parameter combinations, run as short multi-frame sequences
//! (not isolated single calls) so the `log2_ml`/`gamma` recursion -- which depends on the *previous*
//! frame's own state, including when `L` changes between frames -- is genuinely exercised, not just
//! each frame's own from-scratch dequantization.
#![cfg(feature = "ambe_plus_2")]

use ham_digital_modes::ambe::fixed::ambe_plus_2::decode as fixed_decode;
use ham_digital_modes::ambe::fixed::general::mbe_speech::MbeDecoderState;
use ham_digital_modes::ambe::float::ambe_plus_2::decode as float_decode;
use ham_digital_modes::ambe::float::ambe_plus_2::decode::{DecoderState, DequantizedFrame, RawParameters};

/// `Ml`'s own documented tolerance from `ambe::fixed`'s module doc comment: within 1% of the
/// floating-point sibling's own output for the same input bits.
const ML_RELATIVE_TOLERANCE: f64 = 0.01;
/// `w0`'s own tolerance. `f0` (`W0_TABLE`) ranges down to about `0.008` at the highest `b0` values --
/// Q16.16's fixed 16-bit fractional precision means a single-LSB rounding error there
/// (`1/65536 ~= 1.5e-5` absolute) is already about `0.2%` *relative*, the same inherent
/// small-magnitude precision property documented on `exp2_q16`. `3e-3` leaves real margin above
/// that floor rather than chasing a tighter number the table's own quantization can't deliver.
const W0_RELATIVE_TOLERANCE: f64 = 3e-3;

#[allow(clippy::too_many_arguments)] // mirrors RawParameters' own 9 fields one-for-one; a builder
                                      // would be more ceremony than the 9 short-lived call sites need
fn raw(b0: u32, b1: u32, b2: u32, b3: u32, b4: u32, b5: u32, b6: u32, b7: u32, b8: u32) -> RawParameters {
    RawParameters { b0, b1, b2, b3, b4, b5, b6, b7, b8 }
}

/// A representative sweep of real parameter combinations -- spanning low/mid/high `L` (via `b0`),
/// several `VUV`/`DG`/`PRBA`/`HOC` indices each, not just one fixed point. Every value is within its
/// real bit-width range (`extract_raw_parameters`'s own scatter, `AMBE_CHIP_VALIDATION_FINDINGS.md`
/// section 40).
fn representative_sequences() -> Vec<Vec<RawParameters>> {
    vec![
        // A short "sequence" of speech frames at a fixed low L, exercising the ordinary case.
        vec![
            raw(5, 3, 10, 50, 20, 5, 3, 2, 1),
            raw(5, 3, 12, 55, 22, 6, 4, 3, 2),
            raw(5, 3, 8, 45, 18, 4, 2, 1, 0),
        ],
        // A sequence at a high L (many harmonics), still fixed.
        vec![
            raw(115, 20, 25, 400, 100, 25, 12, 12, 6),
            raw(115, 20, 20, 410, 105, 26, 13, 13, 7),
        ],
        // L changes between frames -- exercises the previous-frame resampling
        // (`int_kl`/`delta_l_q16` in the fixed port, `flokl`/`intkl`/`deltal` in the float sibling).
        vec![
            raw(5, 0, 0, 0, 0, 0, 0, 0, 0),
            raw(115, 31, 31, 511, 127, 31, 15, 15, 7),
            raw(60, 15, 15, 256, 64, 15, 7, 7, 3),
        ],
        // Every index at its own extreme (0 or max) -- boundary coverage for every table lookup.
        vec![raw(0, 0, 0, 0, 0, 0, 0, 0, 0)],
        vec![raw(119, 31, 31, 511, 127, 31, 15, 15, 7)],
    ]
}

#[test]
fn fixed_dequantize_matches_float_across_representative_sequences() {
    for sequence in representative_sequences() {
        let mut float_state = DecoderState::initial();
        let mut fixed_state = MbeDecoderState::initial();

        for (frame_idx, raw_params) in sequence.iter().enumerate() {
            let float_result = float_decode::dequantize(raw_params, &mut float_state);
            let fixed_result = fixed_decode::dequantize(raw_params, &mut fixed_state);

            match (float_result, fixed_result) {
                (DequantizedFrame::Speech(float_params), fixed_decode::DequantizedFrame::Speech(fixed_params)) => {
                    assert_eq!(
                        float_params.l, fixed_params.l,
                        "frame {frame_idx}, raw={raw_params:?}: L mismatch"
                    );
                    let float_w0 = float_params.w0;
                    let fixed_w0 = fixed_params.w0_q16 as f64 / 65536.0;
                    let w0_rel_err = ((fixed_w0 - float_w0) / float_w0).abs();
                    assert!(
                        w0_rel_err <= W0_RELATIVE_TOLERANCE,
                        "frame {frame_idx}, raw={raw_params:?}: w0 float={float_w0}, fixed={fixed_w0}, rel_err={w0_rel_err}"
                    );
                    assert_eq!(
                        float_params.voiced, fixed_params.voiced,
                        "frame {frame_idx}, raw={raw_params:?}: voiced decisions differ"
                    );
                    assert_eq!(float_params.ml.len(), fixed_params.ml_q16.len());
                    for (h, (&float_ml, &fixed_ml_q16)) in
                        float_params.ml.iter().zip(fixed_params.ml_q16.iter()).enumerate().skip(1)
                    {
                        let fixed_ml = fixed_ml_q16 as f64 / 65536.0;
                        let rel_err = if float_ml.abs() > 1e-9 {
                            ((fixed_ml - float_ml) / float_ml).abs()
                        } else {
                            fixed_ml.abs() // both effectively zero -- absolute check instead
                        };
                        assert!(
                            rel_err <= ML_RELATIVE_TOLERANCE,
                            "frame {frame_idx}, raw={raw_params:?}, harmonic {h}: Ml float={float_ml}, fixed={fixed_ml}, rel_err={rel_err}"
                        );
                    }
                }
                (float_other, fixed_other) => {
                    panic!(
                        "frame {frame_idx}, raw={raw_params:?}: frame-kind mismatch or unexpected \
                         non-Speech classification (float is_speech={}, fixed is_speech={})",
                        matches!(float_other, DequantizedFrame::Speech(_)),
                        matches!(fixed_other, fixed_decode::DequantizedFrame::Speech(_))
                    );
                }
            }
        }
    }
}

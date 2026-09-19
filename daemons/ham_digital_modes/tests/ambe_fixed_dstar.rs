// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::dstar::decode::dequantize` against its floating-point sibling across a
//! representative sweep of real parameter combinations, run as short multi-frame sequences (same
//! reasoning as `tests/ambe_fixed_ambe_plus_2.rs`: the `log2_ml`/`gamma` recursion depends on
//! previous-frame state, including when `L` changes between frames).

use ham_digital_modes::ambe::fixed::dstar::decode as fixed_decode;
use ham_digital_modes::ambe::fixed::general::mbe_speech::MbeDecoderState;
use ham_digital_modes::ambe::float::dstar::decode as float_decode;
use ham_digital_modes::ambe::float::dstar::decode::{DStarDecoderState, DequantizedFrame, RawParameters};
use ham_digital_modes::ambe::float::dstar::encode::pack_raw_parameters;

const ML_RELATIVE_TOLERANCE: f64 = 0.01;
const W0_RELATIVE_TOLERANCE: f64 = 3e-3; // see tests/ambe_fixed_ambe_plus_2.rs's own comment on why.

#[allow(clippy::too_many_arguments)]
fn raw_d(b0: u32, b1: u32, b2: u32, b3: u32, b4: u32, b5: u32, b6: u32, b7: u32, b8: u32) -> u64 {
    pack_raw_parameters(&RawParameters { b0, b1, b2, b3, b4, b5, b6, b7, b8 })
}

/// A representative sweep, spanning low/mid/high `L` (via `b0`, kept away from D-STAR's own
/// `b0 & 0x7E == 0x7E` tone-frame trigger) and several `VUV`/`DG`/`PRBA`/`HOC` indices each.
/// Largest value Q16.16 in an `i32` represents.
const I32_Q16_MAX: f64 = i32::MAX as f64 / 65536.0;

fn representative_sequences() -> Vec<Vec<u64>> {
    vec![
        vec![
            raw_d(5, 3, 10, 50, 20, 5, 3, 2, 1),
            raw_d(5, 3, 12, 55, 22, 6, 4, 3, 2),
            raw_d(5, 3, 8, 45, 18, 4, 2, 1, 0),
        ],
        vec![
            raw_d(100, 15, 63, 400, 100, 12, 12, 12, 6),
            raw_d(100, 15, 60, 410, 105, 13, 13, 13, 7),
        ],
        // L changes between frames -- exercises the previous-frame resampling.
        vec![raw_d(5, 0, 0, 0, 0, 0, 0, 0, 0), raw_d(120, 15, 63, 511, 127, 15, 15, 15, 6), raw_d(60, 8, 32, 256, 64, 8, 8, 8, 4)],
        vec![raw_d(0, 0, 0, 0, 0, 0, 0, 0, 0)],
        // Highest valid L_TABLE index that isn't a tone trigger (125 & 0x7E = 0x7C, not tone).
        vec![raw_d(125, 15, 63, 511, 127, 15, 15, 15, 6)],
    ]
}

/// `classify_tone_index`'s `Single { hz_q16 }` must exactly match the float sibling's own
/// `Single { hz }` (converted to Q16.16) for every valid index -- an exact check, not a tolerance,
/// since `31.25` has no rounding error in Q16.16 (see `fixed::dstar::decode`'s own doc comment).
#[test]
fn classify_tone_index_hz_matches_the_float_sibling_exactly() {
    for index in 5..=122u32 {
        let float_kind = float_decode::classify_tone_index(index);
        let fixed_kind = fixed_decode::classify_tone_index(index);
        match (float_kind, fixed_kind) {
            (
                ham_digital_modes::ambe::float::dstar::decode::ToneKind::Single { hz },
                fixed_decode::ToneKind::Single { hz_q16 },
            ) => {
                let expected_q16 = (hz * 65536.0).round() as i32;
                assert_eq!(hz_q16, expected_q16, "index={index}: hz={hz}");
            }
            _ => panic!("index={index}: expected Single on both sides"),
        }
    }
}

#[test]
fn fixed_dequantize_matches_float_across_representative_sequences() {
    for sequence in representative_sequences() {
        let mut float_state = DStarDecoderState::initial();
        let mut fixed_state = MbeDecoderState::initial();

        for (frame_idx, &d) in sequence.iter().enumerate() {
            let float_result = float_decode::dequantize(d, &mut float_state);
            let fixed_result = fixed_decode::dequantize(d, &mut fixed_state);

            match (float_result, fixed_result) {
                (DequantizedFrame::Speech(float_params), fixed_decode::DequantizedFrame::Speech(fixed_params)) => {
                    assert_eq!(float_params.l, fixed_params.l, "frame {frame_idx}, d={d:#x}: L mismatch");
                    let float_w0 = float_params.w0;
                    let fixed_w0 = fixed_params.w0_q16 as f64 / 65536.0;
                    let w0_rel_err = ((fixed_w0 - float_w0) / float_w0).abs();
                    assert!(
                        w0_rel_err <= W0_RELATIVE_TOLERANCE,
                        "frame {frame_idx}, d={d:#x}: w0 float={float_w0}, fixed={fixed_w0}, rel_err={w0_rel_err}"
                    );
                    assert_eq!(
                        float_params.voiced, fixed_params.voiced,
                        "frame {frame_idx}, d={d:#x}: voiced decisions differ"
                    );
                    assert_eq!(float_params.ml.len(), fixed_params.ml_q16.len());
                    for (h, (&float_ml, &fixed_ml_q16)) in
                        float_params.ml.iter().zip(fixed_params.ml_q16.iter()).enumerate().skip(1)
                    {
                        let fixed_ml = fixed_ml_q16 as f64 / 65536.0;
                        if float_ml >= I32_Q16_MAX {
                            // The chip's doubled D-STAR gain lets the extreme frames exceed what Q16.16 in an i32 can hold
                            // (a harmonic amplitude above 32768 would clip the 16-bit output anyway): the fixed value must
                            // saturate at the maximum rather than wrap.
                            assert_eq!(fixed_ml_q16, i32::MAX, "frame {frame_idx}, d={d:#x}, harmonic {h}: float {float_ml} must saturate");
                            continue;
                        }
                        let rel_err = if float_ml.abs() > 1e-9 {
                            ((fixed_ml - float_ml) / float_ml).abs()
                        } else {
                            fixed_ml.abs()
                        };
                        assert!(
                            rel_err <= ML_RELATIVE_TOLERANCE,
                            "frame {frame_idx}, d={d:#x}, harmonic {h}: Ml float={float_ml}, fixed={fixed_ml}, rel_err={rel_err}"
                        );
                    }
                }
                (float_other, fixed_other) => {
                    panic!(
                        "frame {frame_idx}, d={d:#x}: frame-kind mismatch (float is_speech={}, fixed is_speech={})",
                        matches!(float_other, DequantizedFrame::Speech(_)),
                        matches!(fixed_other, fixed_decode::DequantizedFrame::Speech(_))
                    );
                }
            }
        }
    }
}

// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::tia_102_baba::reconstruct`/`prediction` against their floating-point
//! siblings across a representative sweep of real parameter combinations, run as short multi-frame
//! sequences (including an `L`-changing sequence, exercising the previous-frame resampling in
//! `prediction`'s own `harmonic_index_ratio`).
//!
//! **Every quantizer value used here is kept within its own real bit-width range for the chosen
//! `L`** (via [`in_range_value`]), not just "small-looking" -- a real, disclosed lesson from this
//! test's own first draft: RATET(27)'s own bit budget shrinks as `L` grows (Annex F/G), so a
//! quantizer value that looks moderate at low `L` can be wildly out of range at high `L` (e.g. `20`
//! against a 3-bit field meant to span `0..8`), and `dequantize_uniform`'s own bin-center formula
//! has no bounds check -- both the float and fixed sides apply it identically, so an out-of-range
//! input produces a huge value on *both* sides, and Q16.16 then legitimately saturates while `f64`
//! doesn't. That's expected fixed-point behavior for invalid input, not a bug, but it's also not a
//! meaningful test of anything -- so this file only ever feeds genuinely in-range values.

use ham_digital_modes::ambe::fixed::tia_102_baba::reconstruct::reconstruct_spectral_amplitudes_q16;
use ham_digital_modes::ambe::float::tia_102_baba::reconstruct::reconstruct_spectral_amplitudes;
use ham_digital_modes::ambe::float::tia_102_baba::tables::{gain_vector_bits, higher_order_bit_allocation};

const ML_RELATIVE_TOLERANCE: f64 = 0.01;

/// A quantizer value guaranteed to fit within `bits`' own real range (`0..2^bits`), built from
/// `seed` by folding it into range rather than clamping -- so different seeds still exercise
/// different in-range values instead of all collapsing to the same clamped extreme.
fn in_range_value(bits: u8, seed: u32) -> u32 {
    if bits == 0 {
        return 0;
    }
    let range = 1u32 << bits;
    seed % range
}

fn gain_values_for(l: u32, seed: u32) -> [u32; 5] {
    std::array::from_fn(|idx| {
        let element = idx as u32 + 2;
        let bits = gain_vector_bits(l, element).expect("valid (l, element)");
        in_range_value(bits, seed.wrapping_add(idx as u32 * 13))
    })
}

fn hoc_values_for(l: u32, seed: u32) -> Vec<u32> {
    higher_order_bit_allocation(l)
        .unwrap()
        .iter()
        .filter(|&&bits| bits > 0)
        .enumerate()
        .map(|(i, &bits)| in_range_value(bits, seed.wrapping_add(i as u32 * 7)))
        .collect()
}

struct FrameInput {
    b2: u8,
    l: u32,
    seed: u32,
}

fn representative_sequences() -> Vec<Vec<FrameInput>> {
    vec![
        // Fixed L, a short sequence.
        vec![
            FrameInput { b2: 17, l: 20, seed: 1 },
            FrameInput { b2: 20, l: 20, seed: 5 },
            FrameInput { b2: 15, l: 20, seed: 9 },
        ],
        // L changes between frames -- exercises the prediction's own resampling. Every quantizer
        // value is now in-range for its own L (see this file's own doc comment on why that matters).
        vec![
            FrameInput { b2: 30, l: 9, seed: 2 },
            FrameInput { b2: 32, l: 56, seed: 11 },
            FrameInput { b2: 25, l: 35, seed: 20 },
        ],
        // Boundary L values, single frames each.
        vec![FrameInput { b2: 20, l: 9, seed: 0 }],
        vec![FrameInput { b2: 40, l: 56, seed: 3 }],
    ]
}

#[test]
fn fixed_reconstruct_spectral_amplitudes_matches_float_across_representative_sequences() {
    use ham_digital_modes::ambe::float::tia_102_baba::prediction::INITIAL_L_HAT_PREV;

    for sequence in representative_sequences() {
        // Both sides start from the spec's own stated initial history: L_hat(-1)=30, all-1.0
        // amplitude (float) / all-Q16.16-1.0 (fixed) -- see prediction.rs's own module doc comment.
        let mut float_l_prev = INITIAL_L_HAT_PREV;
        let mut float_prev_m: Vec<f64> = vec![1.0; INITIAL_L_HAT_PREV as usize];
        let mut fixed_l_prev = INITIAL_L_HAT_PREV;
        let mut fixed_prev_m_q16: Vec<i32> = vec![65536; INITIAL_L_HAT_PREV as usize];

        for (frame_idx, input) in sequence.iter().enumerate() {
            let gain_values = gain_values_for(input.l, input.seed);
            let hoc_values = hoc_values_for(input.l, input.seed);

            let float_result = reconstruct_spectral_amplitudes(
                input.b2,
                gain_values,
                &hoc_values,
                input.l,
                float_l_prev,
                &float_prev_m,
            )
            .unwrap_or_else(|| panic!("frame {frame_idx}: float reconstruction returned None"));

            let fixed_result = reconstruct_spectral_amplitudes_q16(
                input.b2,
                gain_values,
                &hoc_values,
                input.l,
                fixed_l_prev,
                &fixed_prev_m_q16,
            )
            .unwrap_or_else(|| panic!("frame {frame_idx}: fixed reconstruction returned None"));

            assert_eq!(float_result.len(), fixed_result.len(), "frame {frame_idx}: length mismatch");
            for (h, (&float_ml, &fixed_ml_q16)) in float_result.iter().zip(fixed_result.iter()).enumerate() {
                let fixed_ml = fixed_ml_q16 as f64 / 65536.0;
                let rel_err = if float_ml.abs() > 1e-9 {
                    ((fixed_ml - float_ml) / float_ml).abs()
                } else {
                    fixed_ml.abs()
                };
                assert!(
                    rel_err <= ML_RELATIVE_TOLERANCE,
                    "frame {frame_idx}, harmonic {h} (l={}): Ml float={float_ml}, fixed={fixed_ml}, rel_err={rel_err}",
                    input.l
                );
            }

            float_l_prev = input.l;
            float_prev_m = float_result;
            fixed_l_prev = input.l;
            fixed_prev_m_q16 = fixed_result;
        }
    }
}

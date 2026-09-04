// SPDX-License-Identifier: LGPL-3.0-or-later
//! Fixed-point-oriented primitives for this port, built against the
//! real, measured bit-width bounds in
//! `docs/references/CODEC2_MOD_FIXED_POINT_PLAN.md` -- but only for the
//! stages that plan doc's own advisor review confirmed transfer
//! cleanly to *this* port's own structurally-identical code, with an
//! acceptance criterion that doesn't require a product-judgment call
//! first (see `hams_com/night_shift_todo.md`'s 2026-09-04 entry for why
//! Levinson-Durbin specifically is deliberately NOT here yet: its
//! `|k|>1` clamp-boundary behavior is a real accept-vs-stabilize
//! decision reserved for Bruce, and this port's own divergence baseline
//! from the reference is separately unmeasured).
//!
//! `log2_lut`/`exp2_lut` below replace the plain-float `log10`/`powf`
//! round trip in `quantise::encode_energy`/`decode_energy` with the
//! same 8-bit (256-entry), linearly-interpolated log2/exp2 LUT
//! primitive the plan doc validated for `aks_to_mag2`'s own `R^(2*BETA)`
//! treatment (`x^k == 2^(k*log2(x))`, computed in base 2 specifically
//! because that's what a real fixed-point target would implement --
//! `frexp`-style exponent extraction is free, only the mantissa's own
//! log2 needs a table). Everything surrounding the LUT call itself
//! (the multiply by 10, the linear quantizer) deliberately stays in
//! `f32` here, isolating the log-domain treatment specifically, matching
//! the plan doc's own validation scope for `aks_to_mag2` (Q-format
//! widths for the surrounding fixed-point arithmetic are a separate,
//! later engineering step, not attempted here).

use std::sync::OnceLock;

/// LUT resolution used by the real `encode_energy_lut`/`decode_energy_lut`
/// below -- 8 bits, matching the plan doc's own validated `aks_to_mag2`
/// result (max relative error 8.25e-7 there). Kept as a named constant
/// (not hardcoded into the table size) so the negative-control test can
/// build a deliberately coarser table with the same generic code path
/// and confirm the choice of resolution is actually load-bearing, not
/// just present.
const LOG2_LUT_BITS: u32 = 8;
const LOG2_LUT_SIZE: usize = (1 << LOG2_LUT_BITS) + 1;

fn log2_lut_table() -> &'static [f32; LOG2_LUT_SIZE] {
    static TABLE: OnceLock<[f32; LOG2_LUT_SIZE]> = OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|i| (1.0 + i as f32 / (1u32 << LOG2_LUT_BITS) as f32).log2()))
}

fn exp2_lut_table() -> &'static [f32; LOG2_LUT_SIZE] {
    static TABLE: OnceLock<[f32; LOG2_LUT_SIZE]> = OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|i| (i as f32 / (1u32 << LOG2_LUT_BITS) as f32).exp2()))
}

/// `log2(x)` via IEEE754 exponent/mantissa split (exact, free -- just
/// the bit pattern) plus a linearly-interpolated table lookup for the
/// mantissa's own log2 -- the real fixed-point-friendly approximation
/// shape (`frexpf` + LUT) the plan doc validated, not a float shortcut.
/// `bits`/`table` are parameterized so the negative-control test below
/// can exercise the exact same interpolation code at a deliberately
/// coarser resolution.
fn log2_lut_generic(x: f32, bits: u32, table: &[f32]) -> f32 {
    debug_assert!(x > 0.0, "log2_lut_generic: x must be positive, got {x}");
    let levels = 1u32 << bits;
    let raw = x.to_bits();
    let exponent = ((raw >> 23) & 0xFF) as i32 - 127;
    let mantissa = f32::from_bits((raw & 0x007F_FFFF) | 0x3F80_0000); // [1.0, 2.0)
    let scaled = (mantissa - 1.0) * levels as f32;
    let idx = (scaled as usize).min(levels as usize - 1);
    let frac = scaled - idx as f32;
    exponent as f32 + table[idx] + frac * (table[idx + 1] - table[idx])
}

/// `2^y` via integer/fractional split (`2^y = 2^floor(y) * 2^frac`) plus
/// a linearly-interpolated table lookup for `2^frac` -- the inverse of
/// `log2_lut_generic`, same real fixed-point-friendly shape.
fn exp2_lut_generic(y: f32, bits: u32, table: &[f32]) -> f32 {
    let levels = 1u32 << bits;
    let floor_y = y.floor();
    let frac = y - floor_y;
    let scaled = frac * levels as f32;
    let idx = (scaled as usize).min(levels as usize - 1);
    let t = scaled - idx as f32;
    let mantissa = table[idx] + t * (table[idx + 1] - table[idx]);
    mantissa * 2f32.powi(floor_y as i32)
}

fn log2_lut(x: f32) -> f32 {
    log2_lut_generic(x, LOG2_LUT_BITS, log2_lut_table())
}

fn exp2_lut(y: f32) -> f32 {
    exp2_lut_generic(y, LOG2_LUT_BITS, exp2_lut_table())
}

const LOG2_10: f32 = std::f32::consts::LOG2_10;

/// LUT-based equivalent of `quantise::encode_energy` -- same quantizer
/// range/step, only the `log10` call replaced with the log2/exp2-LUT
/// primitive validated above.
pub fn encode_energy_lut(e_linear: f32) -> u32 {
    let e_db = 10.0 * (log2_lut(e_linear.max(1e-12)) / LOG2_10);
    super::quantise::quantize_linear(e_db, super::E_MIN_DB, super::E_MAX_DB, super::E_BITS)
}

/// LUT-based equivalent of `quantise::decode_energy`.
pub fn decode_energy_lut(index: u32) -> f32 {
    let e_db = super::quantise::dequantize_linear(index, super::E_MIN_DB, super::E_MAX_DB, super::E_BITS);
    exp2_lut(e_db / 10.0 * LOG2_10)
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! fixture {
        ($name:literal) => {
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/codec2_3200/", $name)
        };
    }

    fn read_dump(path: &str, cols: usize) -> Vec<Vec<f32>> {
        std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{path}: {e}"))
            .lines()
            .map(|line| {
                let v: Vec<f32> = line.split_whitespace().map(|s| s.parse().unwrap()).collect();
                assert_eq!(v.len(), cols, "line has {} fields, expected {cols}", v.len());
                v
            })
            .collect()
    }

    #[test]
    fn log2_lut_and_exp2_lut_are_real_inverses_across_a_wide_dynamic_range() {
        // Sweeps many decades (matching the real E_MIN_DB..E_MAX_DB dB
        // span converted to linear, 1e-1..1e4, with real margin either
        // side) -- confirms the two LUTs are actually inverses of each
        // other, not just individually plausible.
        let mut x = 1e-3f32;
        let mut max_rel_err = 0.0f32;
        while x < 1e6 {
            let y = log2_lut(x);
            let back = exp2_lut(y);
            let rel_err = ((back - x) / x).abs();
            max_rel_err = max_rel_err.max(rel_err);
            x *= 1.0173; // dense, irrational-ish multiplicative step
        }
        assert!(max_rel_err < 1e-4, "log2_lut/exp2_lut round trip relative error too large: {max_rel_err}");
    }

    #[test]
    fn energy_lut_quantizer_matches_the_float_quantizer_on_real_encoder_side_data_with_zero_index_mismatches() {
        // Real ENCODER-side e (the actual encode_energy() call-site
        // argument, captured directly -- see quantise.rs's own test of
        // the same fixture for why that distinction matters).
        let e_path = fixture!("codec2_enc_e_dump.txt");
        let es = read_dump(e_path, 1);
        assert!(es.len() > 300, "expected the real captured fixture corpus, got {} rows", es.len());

        let mut mismatches = 0;
        for row in &es {
            let e = row[0];
            let float_idx = super::super::quantise::encode_energy(e);
            let lut_idx = encode_energy_lut(e);
            if float_idx != lut_idx {
                mismatches += 1;
            }
        }
        assert_eq!(mismatches, 0, "LUT-based encode_energy diverged from the float version on {mismatches}/{} real frames -- the plan doc's own validated result for this LUT design is zero mismatches", es.len());

        // decode_energy_lut should track decode_energy closely too
        // (checked at a handful of real quantizer indices, not just
        // round-numbers) -- not exact (LUT interpolation vs a single
        // powf call), but tightly bounded.
        let mut max_rel_err = 0.0f32;
        for idx in 0..(1u32 << super::super::E_BITS) {
            let float_back = super::super::quantise::decode_energy(idx);
            let lut_back = decode_energy_lut(idx);
            let rel_err = ((lut_back - float_back) / float_back).abs();
            max_rel_err = max_rel_err.max(rel_err);
        }
        assert!(max_rel_err < 1e-4, "decode_energy_lut diverged from decode_energy by {max_rel_err} relative -- too large for an 8-bit LUT");
    }

    #[test]
    fn a_deliberately_coarse_4_bit_lut_produces_real_index_mismatches_confirming_the_test_above_is_not_vacuous() {
        // Negative control, same methodology the plan doc itself used
        // for aks_to_mag2: rerun the real fixture corpus through the
        // exact same interpolation code at a much coarser resolution
        // and confirm it actually degrades -- if it didn't, the
        // zero-mismatches result above wouldn't be evidence the 8-bit
        // resolution matters, just that log2/exp2-LUT-shaped code
        // happens to always match float here regardless of table size.
        const COARSE_BITS: u32 = 4;
        let coarse_log2: Vec<f32> = (0..=(1u32 << COARSE_BITS)).map(|i| (1.0 + i as f32 / (1u32 << COARSE_BITS) as f32).log2()).collect();

        let e_path = fixture!("codec2_enc_e_dump.txt");
        let es = read_dump(e_path, 1);

        let mut mismatches = 0;
        for row in &es {
            let e = row[0].max(1e-12);
            let e_db_coarse = 10.0 * (log2_lut_generic(e, COARSE_BITS, &coarse_log2) / LOG2_10);
            let coarse_idx = super::super::quantise::quantize_linear(e_db_coarse, super::super::E_MIN_DB, super::super::E_MAX_DB, super::super::E_BITS);
            let float_idx = super::super::quantise::encode_energy(e);
            if coarse_idx != float_idx {
                mismatches += 1;
            }
        }
        assert!(mismatches > 0, "expected the deliberately coarse 4-bit LUT to produce at least one real index mismatch against the float quantizer -- got zero, which would mean the 8-bit result above isn't evidence of anything");
    }
}

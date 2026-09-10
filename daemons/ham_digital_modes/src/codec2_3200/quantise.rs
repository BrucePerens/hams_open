// SPDX-License-Identifier: LGPL-3.0-or-later
//! Bitstream field quantizers (bit packing itself lives in `bits.rs`).
//!
//! The LSP delta-scalar quantizer below reproduces Codec2's real
//! quantization boundaries (needed for bitstream interoperability -- a
//! real Codec2/Codec2-mod decoder must land on the same reconstructed
//! LSP values this encoder intended), but as a closed-form nearest-level
//! computation rather than a lookup table. Checked directly: the
//! vendored reference's own `delta_lsp_cb[10][32]` table (which reads,
//! on first look, like 320 opaque trained values) is actually a
//! piecewise-uniform step function in every one of its 10 dimensions --
//! 7 dimensions step by a uniform 25Hz across all 32 levels, and 3 step
//! by 25Hz for the first 8 levels then 50Hz for the rest. Verified this
//! against every one of the real 320 table entries (no dimension
//! deviates), and separately verified a closed-form nearest-level
//! formula (with "lower index wins a tie", matching the reference's own
//! tie-break rule) reproduces the reference's real binary-search-based
//! quantizer decision across a dense half-million-point sweep, not just
//! at the table's own listed values. A simple formula parameterized by a
//! handful of scalar constants (base step, an optional breakpoint,
//! second step) needs no separate data file or license question the way
//! an opaque trained codebook would.
//!
//! The original `f32` `encode_lsps_delta_scalar` (and the `lsp_dim_
//! nearest_level` helper it alone used) moved to `floating_reference::
//! quantise` once this module's own `encode_lsps_delta_scalar_fixed`
//! made it the only remaining caller. This module keeps, and exports via
//! `pub(crate)`, the per-dimension pieces `decode_lsps_delta_scalar`
//! (the one shared `Decoder` needs) also uses: `LspDim`, `LSP_DIMS`,
//! `LSP_LEVELS`, `lsp_dim_value_hz`.

use super::{E_BITS, E_MAX_DB, E_MIN_DB, LPC_ORD, W0_MAX, W0_MIN, WO_BITS};

// [@ANCHOR: encode_wo]
pub fn encode_wo(wo: f32) -> u32 {
    quantize_linear(wo, W0_MIN, W0_MAX, WO_BITS)
}

// [@ANCHOR: decode_wo]
pub fn decode_wo(index: u32) -> f32 {
    dequantize_linear(index, W0_MIN, W0_MAX, WO_BITS)
}

/// `W0_MIN` in Q23 (`lpc::COEF_FRAC_BITS`'s own format, matching the
/// angle domain used throughout the LSP chain -- `Wo` is also a small
/// radians-per-sample angular frequency, `[0.039, 0.314]`, well within
/// Q23's headroom).
fn w0_min_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| super::fixed_point::f32_to_q_exact_round(W0_MIN, super::lpc::COEF_FRAC_BITS))
}

/// One quantizer step, `(W0_MAX - W0_MIN) / 2^WO_BITS`, in the same Q23
/// format as `w0_min_q23()`.
fn w0_step_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        let step = (W0_MAX - W0_MIN) / (1u32 << WO_BITS) as f32;
        super::fixed_point::f32_to_q_exact_round(step, super::lpc::COEF_FRAC_BITS)
    })
}

/// Fixed-point `decode_wo`: `index` is a plain integer count, so unlike
/// most of this port's fixed-point arithmetic, no rescale is needed at
/// all -- `w0_min_q23() + w0_step_q23() * index` is already Q23 by
/// construction (a Q23 value plus an integer multiple of another Q23
/// value stays Q23).
// [@ANCHOR: decode_wo_fixed]
pub fn decode_wo_fixed(index: u32) -> i64 {
    w0_min_q23() + w0_step_q23() * index as i64
}

/// Uses `fixed_point::log2_lut` (an 8-bit, linearly-interpolated
/// log2/exp2 LUT -- the real fixed-point-friendly shape validated in
/// `docs/references/CODEC2_MOD_FIXED_POINT_PLAN.md` for `aks_to_mag2`'s
/// own `R^(2*BETA)` treatment, reused here for the same reason: it's
/// the primitive a real fixed-point target would implement instead of a
/// float `log10`/`powf` round trip) rather than plain `log10` -- see
/// `fixed_point.rs`'s own tests for the real captured-data validation
/// (zero index mismatches against a plain-float reference on 2539 real
/// frames).
// [@ANCHOR: encode_energy]
pub fn encode_energy(e_linear: f32) -> u32 {
    let e_db =
        10.0 * (super::fixed_point::log2_lut(e_linear.max(1e-12)) / std::f32::consts::LOG2_10);
    quantize_linear(e_db, E_MIN_DB, E_MAX_DB, E_BITS)
}

/// See `encode_energy`'s own doc comment for why this calls
/// `fixed_point::exp2_lut` instead of plain `powf`.
// [@ANCHOR: decode_energy]
pub fn decode_energy(index: u32) -> f32 {
    let e_db = dequantize_linear(index, E_MIN_DB, E_MAX_DB, E_BITS);
    super::fixed_point::exp2_lut(e_db / 10.0 * std::f32::consts::LOG2_10)
}

/// `E_MIN_DB/10*LOG2_10` in Q23 -- the `y` `exp2_q23` would see for
/// `index == 0`, precomputed the same one-time way `w0_min_q23()` is.
fn energy_y_min_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        super::fixed_point::f32_to_q_exact_round(E_MIN_DB / 10.0 * std::f32::consts::LOG2_10, 23)
    })
}

/// One quantizer step's own contribution to `y` (`step_db/10*LOG2_10`),
/// in Q23.
fn energy_y_step_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        let step_db = (E_MAX_DB - E_MIN_DB) / (1u32 << E_BITS) as f32;
        super::fixed_point::f32_to_q_exact_round(step_db / 10.0 * std::f32::consts::LOG2_10, 23)
    })
}

/// Fixed-point `decode_energy`: genuinely integer in/out (Q23 linear
/// energy, `i64`) -- `index` (a plain integer count) linearly
/// determines `y = e_db/10*LOG2_10` the same way `decode_wo_fixed`
/// linearly determines `Wo` (no rescale needed, `energy_y_min_q23() +
/// energy_y_step_q23() * index` is already Q23 by construction), then
/// `fixed_point::exp2_q23` -- `decode_energy`'s own no-FPU sibling --
/// turns that into the actual linear energy value. No `f32` touches
/// this function at all, not even at entry/exit.
// [@ANCHOR: decode_energy_fixed]
pub fn decode_energy_fixed(index: u32) -> i64 {
    let y_q23 = energy_y_min_q23() + energy_y_step_q23() * index as i64;
    super::fixed_point::exp2_q23(y_q23)
}

/// Real linear scalar quantizer shared by `Wo` and energy: `bits`
/// levels evenly spaced across `[min, max]`, index rounded to nearest
/// and clamped. `pub(crate)` so `fixed_point.rs`'s LUT-based energy
/// quantizer can reuse the exact same clamp/rounding logic rather than
/// duplicating it.
// [@ANCHOR: quantize_linear]
pub(crate) fn quantize_linear(value: f32, min: f32, max: f32, bits: u32) -> u32 {
    let levels = 1u32 << bits;
    let norm = (value - min) / (max - min);
    let index = (levels as f32 * norm + 0.5) as i32;
    index.clamp(0, levels as i32 - 1) as u32
}

// [@ANCHOR: dequantize_linear]
pub(crate) fn dequantize_linear(index: u32, min: f32, max: f32, bits: u32) -> f32 {
    let levels = 1u32 << bits;
    let step = (max - min) / levels as f32;
    min + step * index as f32
}

/// One dimension of the LSP delta-scalar quantizer: 32 levels (5 bits),
/// `step1`Hz apart for the first `breakpoint` levels, `step2`Hz apart
/// after that (`step1 == step2` for a purely uniform dimension).
pub(crate) struct LspDim {
    pub(crate) step1: f32,
    pub(crate) breakpoint: u32,
    pub(crate) step2: f32,
}

/// The real per-dimension parameters, reverse-derived from the
/// reference's own real quantizer boundaries (see this module's own doc
/// comment) -- 7 of 10 dimensions are uniform, 3 (indices 3, 4, 5) widen
/// to a coarser step after level 8.
pub(crate) const LSP_DIMS: [LspDim; LPC_ORD] = {
    const UNIFORM: LspDim = LspDim {
        step1: 25.0,
        breakpoint: 32,
        step2: 25.0,
    };
    const WIDENED: LspDim = LspDim {
        step1: 25.0,
        breakpoint: 8,
        step2: 50.0,
    };
    [
        UNIFORM, UNIFORM, UNIFORM, WIDENED, WIDENED, WIDENED, UNIFORM, UNIFORM, UNIFORM, UNIFORM,
    ]
};

pub(crate) const LSP_LEVELS: u32 = 32;

// [@ANCHOR: lsp_dim_value_hz]
pub(crate) fn lsp_dim_value_hz(dim: &LspDim, level: u32) -> f32 {
    if level < dim.breakpoint {
        dim.step1 * (level + 1) as f32
    } else {
        dim.step1 * dim.breakpoint as f32 + dim.step2 * (level - dim.breakpoint + 1) as f32
    }
}

// [@ANCHOR: decode_lsps_delta_scalar]
pub fn decode_lsps_delta_scalar(indexes: &[u32; LPC_ORD]) -> [f32; LPC_ORD] {
    const RAD_PER_HZ: f32 = std::f32::consts::PI / 4000.0;
    let mut lsp = [0.0f32; LPC_ORD];
    let mut lsp_hz = 0.0f32;
    for i in 0..LPC_ORD {
        lsp_hz += lsp_dim_value_hz(&LSP_DIMS[i], indexes[i]);
        lsp[i] = RAD_PER_HZ * lsp_hz;
    }
    lsp
}

/// Q16-fixed-point sibling of `LspDim`: `step1`/`step2`/breakpoint are
/// always exact integer Hz values in the real reference (`25.0`,
/// `50.0`), so unlike `HZ_PER_RAD` (a genuine irrational ratio, still a
/// one-time float conversion below), these need no float conversion at
/// all -- `25i64 << LSP_HZ_FRAC_BITS` is exact.
struct LspDimQ16 {
    step1_q16: i64,
    breakpoint: u32,
    step2_q16: i64,
}

/// Fractional bits for the Hz-domain fixed-point values below -- 16
/// bits gives ~1/65536 Hz resolution, far finer than the real 25Hz
/// quantizer step needs; the choice only has to be "generously enough,"
/// not tight, the same margin philosophy `COEF_FRAC_BITS`/`CHEB_FRAC_
/// BITS` etc. were held to elsewhere in this port.
const LSP_HZ_FRAC_BITS: u32 = 16;

const LSP_DIMS_Q16: [LspDimQ16; LPC_ORD] = {
    const UNIFORM: LspDimQ16 = LspDimQ16 {
        step1_q16: 25i64 << LSP_HZ_FRAC_BITS,
        breakpoint: 32,
        step2_q16: 25i64 << LSP_HZ_FRAC_BITS,
    };
    const WIDENED: LspDimQ16 = LspDimQ16 {
        step1_q16: 25i64 << LSP_HZ_FRAC_BITS,
        breakpoint: 8,
        step2_q16: 50i64 << LSP_HZ_FRAC_BITS,
    };
    [
        UNIFORM, UNIFORM, UNIFORM, WIDENED, WIDENED, WIDENED, UNIFORM, UNIFORM, UNIFORM, UNIFORM,
    ]
};

/// `HZ_PER_RAD` (`4000/pi`) in Q16 -- a genuine one-time float division,
/// computed once (`OnceLock`, matching this port's established
/// table-construction convention) via `fixed_point::f32_to_q_exact_
/// round` rather than a separately-typed literal, so there's no risk of
/// the `BW_GAMMA_Q23`-style independent-computation mismatch.
fn hz_per_rad_q16() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        super::fixed_point::f32_to_q_exact_round(4000.0 / std::f32::consts::PI, LSP_HZ_FRAC_BITS)
    })
}

// [@ANCHOR: lsp_dim_value_hz_q16]
fn lsp_dim_value_hz_q16(dim: &LspDimQ16, level: u32) -> i64 {
    if level < dim.breakpoint {
        dim.step1_q16 * (level + 1) as i64
    } else {
        dim.step1_q16 * dim.breakpoint as i64 + dim.step2_q16 * (level - dim.breakpoint + 1) as i64
    }
}

/// Fixed-point sibling of `lsp_dim_nearest_level`: same linear scan,
/// same `dist < best_dist` strict-less compare (keeping the *first*,
/// i.e. lowest, level on an exact tie -- the real tie-break rule that
/// function's own doc comment records a genuine bug from getting wrong
/// once already). Exact ties are *more* likely here than in the float
/// version (integer subtraction has no rounding noise to break a tie by
/// accident), so this comparison's direction is genuinely load-bearing,
/// not incidental.
// [@ANCHOR: lsp_dim_nearest_level_q16]
fn lsp_dim_nearest_level_q16(dim: &LspDimQ16, target_q16: i64) -> u32 {
    let mut best_level = 0u32;
    let mut best_dist = i64::MAX;
    for level in 0..LSP_LEVELS {
        let dist = (lsp_dim_value_hz_q16(dim, level) - target_q16).abs();
        if dist < best_dist {
            best_dist = dist;
            best_level = level;
        }
    }
    best_level
}

/// Fixed-point `encode_lsps_delta_scalar`: same signature (every real
/// caller's `lsp[]` is still `f32`-typed, coming from `lpc::lpc_to_lsp_
/// from_integer_ak`'s own boundary conversion), but every quantizer
/// decision from here on runs in `i64` Q16 arithmetic -- no `log10`/
/// `powf`-style transcendental was ever needed here (this quantizer
/// never had one), so the only float operation left is the one
/// boundary conversion per dimension (`f32_to_q_exact_round`, exact bit
/// extraction, not a float multiply).
// [@ANCHOR: encode_lsps_delta_scalar_fixed]
pub fn encode_lsps_delta_scalar_fixed(lsp: &[f32; LPC_ORD]) -> [u32; LPC_ORD] {
    let mut indexes = [0u32; LPC_ORD];
    let mut last_q_hz_q16 = 0i64;
    for i in 0..LPC_ORD {
        let angle_q23 = super::fixed_point::f32_to_q_exact_round(lsp[i], 23);
        let lsp_hz_q16 = (angle_q23 * hz_per_rad_q16()) >> 23;
        let target = if i == 0 {
            lsp_hz_q16
        } else {
            lsp_hz_q16 - last_q_hz_q16
        };
        let level = lsp_dim_nearest_level_q16(&LSP_DIMS_Q16[i], target);
        indexes[i] = level;
        let q_hz_q16 = lsp_dim_value_hz_q16(&LSP_DIMS_Q16[i], level);
        last_q_hz_q16 = if i == 0 {
            q_hz_q16
        } else {
            last_q_hz_q16 + q_hz_q16
        };
    }
    indexes
}

/// `pi/4000` in Q23 (`lpc::COEF_FRAC_BITS`'s own format, the angle
/// domain `decode_lsps_delta_scalar_fixed` returns) -- the reciprocal
/// direction of `hz_per_rad_q16()` above, computed the same
/// exact-bit-extraction way for the same reason (no independently
/// typed literal to risk a rounding-tie mismatch against).
fn rad_per_hz_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        super::fixed_point::f32_to_q_exact_round(
            std::f32::consts::PI / 4000.0,
            super::lpc::COEF_FRAC_BITS,
        )
    })
}

/// Fixed-point `decode_lsps_delta_scalar`: same per-dimension
/// accumulation as the real `decode_lsps_delta_scalar` above, but
/// entirely in `i64` Q16 Hz arithmetic (`LSP_DIMS_Q16`/`lsp_dim_value_
/// hz_q16`, the same tables `encode_lsps_delta_scalar_fixed` already
/// built), converting to Q23 radians (`lpc::COEF_FRAC_BITS`'s own
/// format -- what `lsp_to_lpc`'s upcoming fixed-point cos LUT will
/// want) only once per dimension, not per intermediate Hz value.
// [@ANCHOR: decode_lsps_delta_scalar_fixed]
pub fn decode_lsps_delta_scalar_fixed(indexes: &[u32; LPC_ORD]) -> [i64; LPC_ORD] {
    let mut lsp_q23 = [0i64; LPC_ORD];
    let mut lsp_hz_q16 = 0i64;
    for i in 0..LPC_ORD {
        lsp_hz_q16 += lsp_dim_value_hz_q16(&LSP_DIMS_Q16[i], indexes[i]);
        lsp_q23[i] = (lsp_hz_q16 * rad_per_hz_q23()) >> 16;
    }
    lsp_q23
}

// `pub(crate)`, not the usual bare `mod tests`: `floating_reference::
// quantise`'s own tests reuse this module's `fixture!`/`read_dump`
// rather than duplicating real fixture-parsing infrastructure across
// files.
#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    macro_rules! fixture {
        ($name:literal) => {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codec2_3200/",
                $name
            )
        };
    }
    // See `lpc.rs`'s own `mod tests` for why this is a `use` re-export
    // rather than a visibility qualifier directly on `macro_rules!`
    // (which Rust doesn't support).
    pub(crate) use fixture;

    pub(crate) fn read_dump(path: &str, cols: usize) -> Vec<Vec<f32>> {
        std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{path}: {e}"))
            .lines()
            .map(|line| {
                let v: Vec<f32> = line
                    .split_whitespace()
                    .map(|s| s.parse().unwrap())
                    .collect();
                assert_eq!(
                    v.len(),
                    cols,
                    "line has {} fields, expected {cols}",
                    v.len()
                );
                v
            })
            .collect()
    }

    #[test]
    // Tests [@ANCHOR: quantize_linear]
    // Tests [@ANCHOR: dequantize_linear]
    // Tests [@ANCHOR: encode_wo]
    // Tests [@ANCHOR: decode_wo]
    fn wo_quantizer_round_trips_within_one_step() {
        let step = (W0_MAX - W0_MIN) / (1 << WO_BITS) as f32;
        for i in 0..200 {
            let wo = W0_MIN + (W0_MAX - W0_MIN) * i as f32 / 200.0;
            let idx = encode_wo(wo);
            let back = decode_wo(idx);
            assert!(
                (back - wo).abs() <= step,
                "Wo {wo} round-tripped to {back}, step {step}"
            );
        }
    }

    #[test]
    // Tests [@ANCHOR: encode_energy]
    // Tests [@ANCHOR: decode_energy]
    fn energy_quantizer_matches_real_encoder_side_data_within_the_real_5_bit_step() {
        // Real ENCODER-side e (not decoder-side reconstruction -- this
        // session's own earlier methodology error, corrected: capture
        // the exact call-site argument, not a value from elsewhere in
        // the pipeline with the same name).
        let e_path = fixture!("codec2_enc_e_dump.txt");
        let es = read_dump(e_path, 1);
        assert!(
            es.len() > 300,
            "expected the real captured fixture corpus, got {} rows",
            es.len()
        );
        let step_db = (E_MAX_DB - E_MIN_DB) / (1 << E_BITS) as f32;
        for row in es {
            let e = row[0];
            let idx = encode_energy(e);
            let back = decode_energy(idx);
            let e_db = 10.0 * e.max(1e-12).log10();
            let back_db = 10.0 * back.max(1e-12).log10();
            // e_db legitimately exceeds [E_MIN_DB, E_MAX_DB] on real loud
            // frames -- the quantizer clamps there by real design (same
            // as the reference's own `index.clamp`), so a real, large
            // error at the boundary is expected, not a bug. Only assert
            // the tight within-range tolerance for values the quantizer
            // was actually designed to represent losslessly.
            if (E_MIN_DB..=E_MAX_DB).contains(&e_db) {
                assert!(
                    (back_db - e_db).abs() <= step_db,
                    "in-range real e={e} (db={e_db}) round-tripped to {back} (db={back_db}), step {step_db}dB"
                );
            }
        }
    }

    #[test]
    fn encode_lsps_delta_scalar_fixed_matches_the_float_version_exactly_on_real_captured_lsp_data()
    {
        use crate::codec2_3200::floating_reference::quantise::encode_lsps_delta_scalar;
        // The real acceptance bar for a quantizer is index agreement,
        // not a tolerance: encode_lsps_delta_scalar (already validated
        // above against an independent reference transcription) and its
        // fixed-point sibling must produce byte-identical transmitted
        // indices, not merely "close" ones -- any single-level
        // disagreement anywhere in the delta chain would also shift
        // every later dimension's own target (each depends on the
        // previous dimension's *reconstructed* value), so this is a
        // strict, unforgiving comparison by construction.
        let lsp_path = fixture!("codec2_lsp_dump.txt");
        let lsp_rows = read_dump(lsp_path, LPC_ORD + 1);
        assert!(
            lsp_rows.len() > 300,
            "expected the real captured fixture corpus, got {} rows",
            lsp_rows.len()
        );
        let mut n_checked = 0;
        let mut n_mismatched = 0;
        for row in &lsp_rows {
            let roots = row[0] as i32;
            if roots as usize != LPC_ORD {
                continue;
            }
            let mut lsp = [0.0f32; LPC_ORD];
            lsp.copy_from_slice(&row[1..]);
            let float_indexes = encode_lsps_delta_scalar(&lsp);
            let fixed_indexes = encode_lsps_delta_scalar_fixed(&lsp);
            if float_indexes != fixed_indexes {
                n_mismatched += 1;
            }
            n_checked += 1;
        }
        println!("encode_lsps_delta_scalar_fixed: {n_mismatched}/{n_checked} mismatches ({} rows in fixture)", lsp_rows.len());
        assert!(n_checked > 150, "only checked {n_checked} real frames");
        assert_eq!(
            n_mismatched, 0,
            "encode_lsps_delta_scalar_fixed diverged from the float version on {n_mismatched}/{n_checked} real captured frames -- expected byte-identical transmitted indices"
        );
    }

    #[test]
    // Tests [@ANCHOR: lsp_dim_value_hz_q16]
    fn lsp_dim_nearest_level_q16_keeps_the_lower_index_on_an_exact_tie() {
        // The float version's own doc comment records a real bug: an
        // epsilon meant to nudge only exact ties toward the lower index
        // instead swallowed a genuine, non-tied margin on real data.
        // Exact ties are *more* likely in this Q16 integer version (no
        // rounding noise to break one by accident), so this is a live
        // concern here, not a historical footnote -- construct an exact
        // midpoint between two adjacent levels of a uniform dimension
        // and confirm the lower index wins, matching `dist < best_dist`
        // (strict) scanning upward.
        let dim = &LSP_DIMS_Q16[0]; // uniform: levels at 25, 50, 75, ... Hz
        let level0_hz_q16 = lsp_dim_value_hz_q16(dim, 0); // 25Hz
        let level1_hz_q16 = lsp_dim_value_hz_q16(dim, 1); // 50Hz
        let exact_midpoint = (level0_hz_q16 + level1_hz_q16) / 2; // 37.5Hz, exact in Q16
        assert_eq!(
            lsp_dim_nearest_level_q16(dim, exact_midpoint),
            0,
            "exact tie between level 0 and level 1 should keep the lower index"
        );
    }

    #[test]
    // Tests [@ANCHOR: lsp_dim_nearest_level_q16]
    fn lsp_dim_nearest_level_q16_matches_the_float_version_across_a_dense_sweep() {
        use crate::codec2_3200::floating_reference::quantise::lsp_dim_nearest_level;
        let mut mismatches = 0;
        let mut hz_q16 = -50i64 << LSP_HZ_FRAC_BITS;
        let step = 1i64 << (LSP_HZ_FRAC_BITS - 4); // 1/16 Hz steps
        while hz_q16 < (5000i64 << LSP_HZ_FRAC_BITS) {
            let target_hz = hz_q16 as f32 / (1i64 << LSP_HZ_FRAC_BITS) as f32;
            for (dim_f, dim_q16) in LSP_DIMS.iter().zip(LSP_DIMS_Q16.iter()) {
                let float_level = lsp_dim_nearest_level(dim_f, target_hz);
                let fixed_level = lsp_dim_nearest_level_q16(dim_q16, hz_q16);
                if float_level != fixed_level {
                    mismatches += 1;
                }
            }
            hz_q16 += step;
        }
        assert_eq!(
            mismatches, 0,
            "lsp_dim_nearest_level_q16 diverged from the float version somewhere in a dense sweep across every real dimension shape"
        );
    }

    #[test]
    // Tests [@ANCHOR: decode_lsps_delta_scalar]
    // Tests [@ANCHOR: decode_lsps_delta_scalar_fixed]
    // Tests [@ANCHOR: encode_lsps_delta_scalar_fixed]
    // Tests [@ANCHOR: lsp_dim_value_hz]
    fn decode_lsps_delta_scalar_fixed_matches_the_float_version_within_quantization_noise_on_real_captured_indices(
    ) {
        // Real transmitted indices, derived the same way a real decoder
        // would see them: `encode_lsps_delta_scalar_fixed` (already
        // verified byte-identical to the float encoder above) on real
        // captured LSPs. Acceptance bar is in Hz (the real quantizer's
        // own unit, same convention `acos_lut_fixed`'s own dense-sweep
        // test uses and for the same reason: a flat rad threshold isn't
        // the natural metric here) -- `decode_lsps_delta_scalar_fixed`
        // accumulates in Q16 Hz then converts to Q23 radians once per
        // dimension via `rad_per_hz_q23()`'s own one-time-rounded
        // constant, vs. the float version's per-dimension `f32`
        // accumulation in Hz directly multiplied by an exact `RAD_PER_
        // HZ` on every use; both are exact reconstructions of the same
        // integer codebook values, so any divergence is pure Q23
        // constant-rounding noise (measured ~0.23Hz at the real
        // quantizer's own ~5000Hz top end), not a formula difference --
        // negligible next to the real 25Hz quantizer step.
        let lsp_path = fixture!("codec2_lsp_dump.txt");
        let lsp_rows = read_dump(lsp_path, LPC_ORD + 1);
        assert!(
            lsp_rows.len() > 300,
            "expected the real captured fixture corpus, got {} rows",
            lsp_rows.len()
        );
        const HZ_PER_RAD: f32 = 4000.0 / std::f32::consts::PI;
        let mut n_checked = 0;
        let mut max_err_hz = 0.0f32;
        for row in &lsp_rows {
            let roots = row[0] as i32;
            if roots as usize != LPC_ORD {
                continue;
            }
            let mut lsp = [0.0f32; LPC_ORD];
            lsp.copy_from_slice(&row[1..]);
            let indexes = encode_lsps_delta_scalar_fixed(&lsp);

            let float_back = decode_lsps_delta_scalar(&indexes);
            let fixed_back_q23 = decode_lsps_delta_scalar_fixed(&indexes);
            for i in 0..LPC_ORD {
                let fixed_back =
                    fixed_back_q23[i] as f32 / (1i64 << super::super::lpc::COEF_FRAC_BITS) as f32;
                max_err_hz = max_err_hz.max((fixed_back - float_back[i]).abs() * HZ_PER_RAD);
            }
            n_checked += 1;
        }
        assert!(n_checked > 150, "only checked {n_checked} real frames");
        assert!(
            max_err_hz < 1.0,
            "decode_lsps_delta_scalar_fixed diverged from the float version by {max_err_hz}Hz, expected agreement well within the real 25Hz quantizer step"
        );
    }

    #[test]
    // Tests [@ANCHOR: decode_wo_fixed]
    fn decode_wo_fixed_matches_decode_wo_on_every_valid_index() {
        let mut max_abs_err = 0.0f32;
        for index in 0..(1u32 << super::super::WO_BITS) {
            let float_wo = decode_wo(index);
            let fixed_wo =
                decode_wo_fixed(index) as f32 / (1i64 << super::super::lpc::COEF_FRAC_BITS) as f32;
            max_abs_err = max_abs_err.max((fixed_wo - float_wo).abs());
        }
        assert!(
            max_abs_err < 1e-5,
            "decode_wo_fixed diverged from decode_wo by {max_abs_err} rad, more than ordinary Q23 rounding noise"
        );
    }

    #[test]
    // Tests [@ANCHOR: decode_energy_fixed]
    fn decode_energy_fixed_matches_decode_energy_on_every_valid_index() {
        let mut max_rel_err = 0.0f32;
        for index in 0..(1u32 << E_BITS) {
            let float_e = decode_energy(index);
            let fixed_e = decode_energy_fixed(index) as f32 / (1i64 << 23) as f32;
            max_rel_err = max_rel_err.max(((fixed_e - float_e) / float_e).abs());
        }
        assert!(
            max_rel_err < 1e-4,
            "decode_energy_fixed diverged from decode_energy by {max_rel_err} relative, more than ordinary Q23/LUT rounding noise"
        );
    }

    /// Adversarial regression test, bug-hunt pass 2026-09-10: every one of
    /// this module's real ENCODE-side entry points, fed the exact
    /// degenerate values a genuinely all-silent/digitally-muted audio
    /// frame drives through the real production pipeline -- `Wo` at its
    /// own lower search-range bound (`nlp_fixed` returning `0.0` Hz maps,
    /// via `f0_to_wo`, to a `Wo` below `W0_MIN`; the quantizer's own
    /// `quantize_linear` must clamp that, not produce a garbage index),
    /// `e_linear == 0.0` exactly (real digital silence, before
    /// `encode_energy`'s own `.max(1e-12)` floor), and an all-zero LSP
    /// vector (`lsp[i] == 0.0` for every `i`, matching this crate's own
    /// `mod.rs::fallback_lsp()`'s first entry exactly and bracketing every
    /// other entry from below -- the real value `EncoderFixed::encode`
    /// substitutes on an LPC-to-LSP conversion failure, itself a real,
    /// already-confirmed-reachable outcome on all-silent audio per
    /// `lpc.rs`'s own `r0_normalize_fixed` bug-hunt fix this same pass).
    /// This crate has already been shown (this same campaign, several
    /// sibling files) to mishandle exactly this "all-silent frame" input
    /// class via unguarded preconditions reachable only at a value range
    /// nothing else in the file's own test suite happens to exercise --
    /// checked directly here rather than assumed safe from reading the
    /// arithmetic alone.
    #[test]
    // Tests [@ANCHOR: quantize_linear]
    // Tests [@ANCHOR: encode_wo]
    // Tests [@ANCHOR: encode_energy]
    // Tests [@ANCHOR: encode_lsps_delta_scalar_fixed]
    // Tests [@ANCHOR: lsp_dim_nearest_level_q16]
    fn encode_side_quantizers_do_not_panic_on_an_all_silent_degenerate_frame() {
        // Below W0_MIN (an out-of-range Wo the real quantizer must clamp,
        // not index out of bounds on) -- `nlp.rs::f0_to_wo(0.0)` (the real
        // value a `f0 == 0.0` pitch estimate, itself a real return value
        // `nlp_fixed`'s own silence test already confirms is reachable,
        // maps to) is `TAU * 0.0 / SAMPLE_RATE == 0.0` exactly, well below
        // `W0_MIN` -- passed directly here rather than via `f0_to_wo`
        // itself, since `quantise.rs` has no dependency on `nlp.rs` by
        // design and the formula's own result at `f0 == 0.0` is `0.0`
        // regardless of `SAMPLE_RATE`.
        let wo_idx = encode_wo(0.0);
        assert!(
            wo_idx < (1u32 << WO_BITS),
            "encode_wo produced an out-of-range index {wo_idx} for a below-range Wo"
        );

        let e_idx = encode_energy(0.0);
        assert!(
            e_idx < (1u32 << E_BITS),
            "encode_energy produced an out-of-range index {e_idx} for exact-zero linear energy"
        );
        // decode_energy(e_idx) must also stay finite -- exercises
        // exp2_lut's own boundary at the quantizer's own minimum index.
        assert!(
            decode_energy(e_idx).is_finite(),
            "decode_energy({e_idx}) produced a non-finite energy from the silence-derived index"
        );

        // All-zero LSP vector: matches `mod.rs::fallback_lsp()`'s own
        // first entry (`i=0` gives exactly `0.0`) and is more extreme than
        // every later entry (which only grow from there) -- the tightest
        // real case for `encode_lsps_delta_scalar_fixed`'s own per-
        // dimension delta accumulation, since every delta target is
        // exactly `0.0 - last_q_hz_q16` (zero or negative throughout,
        // never the ordinary positive-delta case every other test in this
        // file happens to exercise).
        let lsp = [0.0f32; LPC_ORD];
        let indexes = encode_lsps_delta_scalar_fixed(&lsp);
        for (i, &idx) in indexes.iter().enumerate() {
            assert!(
                idx < LSP_LEVELS,
                "encode_lsps_delta_scalar_fixed produced out-of-range index {idx} at dimension {i}"
            );
        }
        // Round-trip through the fixed-point decoder too -- confirms the
        // whole chain (encode then decode) doesn't panic on this input and
        // stays within a physically sane angle range, not just that
        // encoding alone avoids a panic. `decode_lsps_delta_scalar_fixed`
        // returns `i64` (always representable, no `is_finite()` to check),
        // so the real assertion is a sane bound: every real LSP angle is
        // in `[0, pi]` radians, generously bracketed here in Q23.
        let lsp_back_q23 = decode_lsps_delta_scalar_fixed(&indexes);
        let pi_q23 = (std::f32::consts::PI * (1i64 << super::super::lpc::COEF_FRAC_BITS) as f32)
            as i64;
        for (i, &v) in lsp_back_q23.iter().enumerate() {
            assert!(
                (-pi_q23..=2 * pi_q23).contains(&v),
                "decode_lsps_delta_scalar_fixed produced an implausible angle {v} (Q23) at dimension {i}"
            );
        }
    }
}

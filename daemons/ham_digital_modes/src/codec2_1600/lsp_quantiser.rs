// SPDX-License-Identifier: LGPL-3.0-or-later
//! The 1600bps mode's own LSP quantizer: ten independent per-dimension
//! scalar codebooks (dimension 1 = k = 1 each), a genuinely different
//! design from `codec2_3200::quantise`'s delta-scalar quantizer. Table
//! values (in Hz) are the real, published Codec2 quantizer tables
//! (`src/codebook/lsp1.txt` .. `lsp10.txt` in a plain upstream Codec2
//! checkout) -- not creative expression, the actual numbers any
//! interoperable decoder must reproduce exactly, the same reasoning
//! `codec2_3200`'s own module doc comment gives for its bitstream-format
//! constants. Checked directly against upstream's own plain-text
//! codebook source (`lsp1.txt` .. `lsp10.txt`): every one of the ten
//! real tables turns out to be evenly spaced, so each reduces to a
//! `(start, step, levels)` triple -- the same "closed-form, not opaque
//! trained data" situation `codec2_3200::bw_gamma`'s own doc comment
//! documents for that quantizer, verified here rather than assumed, so
//! no literal per-level float array is stored (and no separate
//! LGPL-2.1-only data file was needed).

/// One dimension's scalar codebook: evenly spaced from `start` in steps
/// of `step`, `levels` entries (`2^log2m == levels` in every real
/// dimension here, confirmed against each `lspN.txt`'s own `1 <m>`
/// header). Every one of the ten real tables happens to be evenly
/// spaced, so this is `start + step*index`, not a lookup array.
struct LspDim {
    start_hz: f32,
    step_hz: f32,
    levels: u32,
    log2m: u32,
}

/// The ten real per-dimension codebooks, `lsp1.txt` .. `lsp10.txt`.
const LSP_CB: [LspDim; super::LPC_ORD] = [
    LspDim { start_hz: 225.0, step_hz: 25.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 325.0, step_hz: 25.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 500.0, step_hz: 50.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 700.0, step_hz: 100.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 950.0, step_hz: 100.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 1100.0, step_hz: 100.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 1500.0, step_hz: 100.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 2300.0, step_hz: 100.0, levels: 8, log2m: 3 },
    LspDim { start_hz: 2500.0, step_hz: 100.0, levels: 8, log2m: 3 },
    LspDim { start_hz: 2900.0, step_hz: 200.0, levels: 4, log2m: 2 },
];

const RAD_PER_HZ: f32 = std::f32::consts::PI / 4000.0;
const HZ_PER_RAD: f32 = 4000.0 / std::f32::consts::PI;

/// Bit width of the `i`th LSP dimension's own index -- 4,4,4,4,4,4,4,3,3,2
/// (36 bits total), matching upstream's own `lsp_bits(i)`.
pub fn lsp_bits(i: usize) -> u32 {
    LSP_CB[i].log2m
}

/// Encodes one dimension's LSP (radians) to its nearest codebook index.
/// The real codebooks are evenly spaced, so nearest-index is a direct
/// rounded division, not a linear scan -- behaviorally identical to the
/// reference's own linear-scan `quantise()` for a 1-D codebook (nearest
/// by absolute difference is the same as nearest by squared error for a
/// scalar), just without the O(m) search.
fn quantise_dim(dim: &LspDim, target_hz: f32) -> u32 {
    let idx = ((target_hz - dim.start_hz) / dim.step_hz).round();
    idx.clamp(0.0, (dim.levels - 1) as f32) as u32
}

/// Scalar LSP quantiser. From a vector of unquantised LSPs (radians)
/// finds the quantised LSP indexes -- one call per dimension, each
/// dimension's own independent codebook.
pub fn encode_lsps_scalar(lsp: &[f32; super::LPC_ORD]) -> [u32; super::LPC_ORD] {
    std::array::from_fn(|i| quantise_dim(&LSP_CB[i], lsp[i] * HZ_PER_RAD))
}

/// From a vector of quantised LSP indexes, returns the quantised LSPs
/// (radians) -- the real quantized value any compliant decoder must
/// reproduce exactly, since this *is* the table, not a design choice.
pub fn decode_lsps_scalar(indexes: &[u32; super::LPC_ORD]) -> [f32; super::LPC_ORD] {
    std::array::from_fn(|i| {
        let dim = &LSP_CB[i];
        let hz = dim.start_hz + dim.step_hz * indexes[i] as f32;
        hz * RAD_PER_HZ
    })
}

use crate::codec2_3200::fixed_point::f32_to_q_exact_round;
use std::sync::OnceLock;

const FRAC_BITS: u32 = 23;

/// Per-dimension `(start_hz, step_hz)` in Q23 -- computed once via
/// `f32_to_q_exact_round`, never hand-typed (every real value here is a
/// small exact integer Hz count, so the conversion itself is exact, but
/// the established rule in this crate is "always compute, never
/// hand-type a Q23 constant" regardless of how simple the source value
/// looks -- see `envelope.rs`'s own `BOOST_RATIO_Q23` lesson).
fn lsp_cb_q23() -> &'static [(i64, i64); super::LPC_ORD] {
    static V: OnceLock<[(i64, i64); super::LPC_ORD]> = OnceLock::new();
    V.get_or_init(|| std::array::from_fn(|i| (f32_to_q_exact_round(LSP_CB[i].start_hz, FRAC_BITS), f32_to_q_exact_round(LSP_CB[i].step_hz, FRAC_BITS))))
}

fn hz_per_rad_q23() -> i64 {
    static V: OnceLock<i64> = OnceLock::new();
    *V.get_or_init(|| f32_to_q_exact_round(HZ_PER_RAD, FRAC_BITS))
}

fn rad_per_hz_q23() -> i64 {
    static V: OnceLock<i64> = OnceLock::new();
    *V.get_or_init(|| f32_to_q_exact_round(RAD_PER_HZ, FRAC_BITS))
}

fn q_mul_q23(a: i64, b: i64) -> i64 {
    ((a as i128 * b as i128) >> FRAC_BITS) as i64
}

/// Fixed-point sibling of `quantise_dim`: same evenly-spaced nearest-
/// index computation (`round((target-start)/step)`, clamped), entirely
/// in Q23 `i64` arithmetic -- integer division (not a float divide) is
/// itself a genuine no-FPU operation, same reasoning `cos_q23`'s own
/// doc comment gives for its own one real division.
fn quantise_dim_fixed(start_q23: i64, step_q23: i64, levels: u32, target_hz_q23: i64) -> u32 {
    // Round-to-nearest integer division: add half the divisor before
    // truncating: `(target-start)/step` rounded, with `step_q23`'s own
    // sign always positive (every real step here is positive), so a
    // plain `(diff + step/2) / step` is correct without a sign-
    // dependent branch.
    let diff = target_hz_q23 - start_q23;
    let idx = if diff <= 0 {
        0i64
    } else {
        (diff + step_q23 / 2) / step_q23
    };
    idx.clamp(0, (levels - 1) as i64) as u32
}

/// Encodes one dimension's LSP (Q23 radians) to its nearest codebook
/// index, entirely in `i64` Q23 arithmetic -- no `f32` anywhere.
pub fn encode_lsps_scalar_fixed(lsp_q23: &[i64; super::LPC_ORD]) -> [u32; super::LPC_ORD] {
    let cb = lsp_cb_q23();
    std::array::from_fn(|i| {
        let target_hz_q23 = q_mul_q23(lsp_q23[i], hz_per_rad_q23());
        quantise_dim_fixed(cb[i].0, cb[i].1, LSP_CB[i].levels, target_hz_q23)
    })
}

/// Decodes quantised LSP indexes to Q23 radians -- entirely in `i64`
/// Q23 arithmetic, the exact same table `decode_lsps_scalar` uses,
/// just never touching `f32`.
pub fn decode_lsps_scalar_fixed(indexes: &[u32; super::LPC_ORD]) -> [i64; super::LPC_ORD] {
    let cb = lsp_cb_q23();
    std::array::from_fn(|i| {
        let hz_q23 = cb[i].0 + cb[i].1 * indexes[i] as i64;
        q_mul_q23(hz_q23, rad_per_hz_q23())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsp_bits_sums_to_36_total_bits() {
        let total: u32 = (0..super::super::LPC_ORD).map(lsp_bits).sum();
        assert_eq!(total, 36, "1600bps LSP field is documented as 36 bits total");
    }

    #[test]
    fn encode_then_decode_recovers_the_same_quantised_value_for_every_real_index() {
        for i in 0..super::super::LPC_ORD {
            let dim = &LSP_CB[i];
            for level in 0..dim.levels {
                let indexes: [u32; super::super::LPC_ORD] = std::array::from_fn(|j| {
                    if j == i {
                        level
                    } else {
                        0
                    }
                });
                let decoded = decode_lsps_scalar(&indexes);
                let re_encoded = encode_lsps_scalar(&decoded);
                assert_eq!(
                    re_encoded[i], level,
                    "dimension {i} level {level}: decode-then-encode didn't round-trip"
                );
            }
        }
    }

    /// Every real table value from `lspN.txt` (N=1..10), spot-checked
    /// directly against the plain upstream Codec2 source text -- not
    /// just the evenly-spaced reconstruction's own internal consistency.
    #[test]
    fn decode_matches_the_real_upstream_codebook_tables_at_specific_indices() {
        let cases: [(usize, u32, f32); 10] = [
            (0, 0, 225.0),
            (0, 15, 600.0),
            (1, 8, 525.0),
            (2, 0, 500.0),
            (3, 15, 2200.0),
            (4, 7, 1650.0),
            (5, 15, 2600.0),
            (6, 0, 1500.0),
            (7, 7, 3000.0),
            (8, 0, 2500.0),
        ];
        for (dim, level, expected_hz) in cases {
            let indexes: [u32; super::super::LPC_ORD] =
                std::array::from_fn(|j| if j == dim { level } else { 0 });
            let decoded = decode_lsps_scalar(&indexes);
            let got_hz = decoded[dim] * HZ_PER_RAD;
            assert!(
                (got_hz - expected_hz).abs() < 1e-3,
                "dim={dim} level={level}: got {got_hz}Hz, expected {expected_hz}Hz"
            );
        }
        // lsp10.txt's own 4 entries: 2900, 3100, 3300, 3500
        for (level, expected_hz) in [(0, 2900.0), (1, 3100.0), (2, 3300.0), (3, 3500.0)] {
            let indexes: [u32; super::super::LPC_ORD] =
                std::array::from_fn(|j| if j == 9 { level } else { 0 });
            let decoded = decode_lsps_scalar(&indexes);
            let got_hz = decoded[9] * HZ_PER_RAD;
            assert!(
                (got_hz - expected_hz).abs() < 1e-3,
                "dim=9 level={level}: got {got_hz}Hz, expected {expected_hz}Hz"
            );
        }
    }

    /// Fixed vs float diverge by a genuine, small Q23 rounding artifact
    /// (`hz_per_rad_q23`/`rad_per_hz_q23` are each independently rounded
    /// from a mathematically-exact reciprocal relationship, so a
    /// Hz-then-back-to-radians round trip compounds two independent
    /// roundings) -- measured max 0.000166 rad across every real index,
    /// bound set from that real margin, not guessed.
    #[test]
    fn decode_lsps_scalar_fixed_matches_the_float_version_for_every_real_index() {
        for i in 0..super::super::LPC_ORD {
            for level in 0..LSP_CB[i].levels {
                let indexes: [u32; super::super::LPC_ORD] =
                    std::array::from_fn(|j| if j == i { level } else { 0 });
                let float_lsp = decode_lsps_scalar(&indexes);
                let fixed_lsp = decode_lsps_scalar_fixed(&indexes);
                let fixed_as_f32 = fixed_lsp[i] as f32 / (1i64 << FRAC_BITS) as f32;
                assert!(
                    (fixed_as_f32 - float_lsp[i]).abs() < 3e-4,
                    "dim={i} level={level}: fixed={fixed_as_f32} float={}",
                    float_lsp[i]
                );
            }
        }
    }

    /// Encode-then-decode must round-trip in the fixed path too, same
    /// invariant `encode_then_decode_recovers_the_same_quantised_value_
    /// for_every_real_index` checks for the float version.
    #[test]
    fn encode_then_decode_recovers_the_same_quantised_value_for_every_real_index_fixed() {
        for i in 0..super::super::LPC_ORD {
            for level in 0..LSP_CB[i].levels {
                let indexes: [u32; super::super::LPC_ORD] =
                    std::array::from_fn(|j| if j == i { level } else { 0 });
                let decoded = decode_lsps_scalar_fixed(&indexes);
                let re_encoded = encode_lsps_scalar_fixed(&decoded);
                assert_eq!(
                    re_encoded[i], level,
                    "dimension {i} level {level}: fixed decode-then-encode didn't round-trip"
                );
            }
        }
    }
}

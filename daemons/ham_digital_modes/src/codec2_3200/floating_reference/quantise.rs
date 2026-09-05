// SPDX-License-Identifier: LGPL-3.0-or-later
//! **Not production code** -- see `floating_reference/mod.rs`'s own doc
//! comment. The original, fully-`f32` LSP delta-scalar encoder
//! (`encode_lsps_delta_scalar`), moved here from `codec2_3200::quantise`
//! once that module's own `encode_lsps_delta_scalar_fixed` made this the
//! only remaining caller. `codec2_3200::quantise` still owns, and this
//! module borrows back via `pub(crate)` imports, the per-dimension
//! helpers `decode_lsps_delta_scalar` (the one shared `Decoder` needs)
//! *also* uses: `LspDim`, `LSP_DIMS`, `LSP_LEVELS`, `lsp_dim_value_hz`.
//! `encode_wo`/`decode_wo`/`encode_energy`/`decode_energy`/
//! `decode_lsps_delta_scalar` all stay in `codec2_3200::quantise`
//! unchanged -- either already shared by both encoders (`encode_wo`,
//! `encode_energy`) or genuinely decoder-only (`decode_wo`/
//! `decode_energy`/`decode_lsps_delta_scalar`), not part of this
//! encoder-only move.

use crate::codec2_3200::quantise::{lsp_dim_value_hz, LspDim, LSP_DIMS};
use crate::codec2_3200::LPC_ORD;

/// Encodes 10 LSP frequencies (radians) as 10 delta-scalar indices
/// (5 bits each): the first dimension quantizes its own LSP frequency
/// directly (Hz), every later dimension quantizes the *delta* from the
/// previous dimension's own reconstructed (quantized) value -- LSPs are
/// strictly increasing, so deltas are always non-negative in practice
/// and this delta coding concentrates most of each dimension's dynamic
/// range where it's actually used.
pub(crate) fn encode_lsps_delta_scalar(lsp: &[f32; LPC_ORD]) -> [u32; LPC_ORD] {
    const HZ_PER_RAD: f32 = 4000.0 / std::f32::consts::PI;
    let mut indexes = [0u32; LPC_ORD];
    let mut last_q_hz = 0.0f32;
    for i in 0..LPC_ORD {
        let lsp_hz = HZ_PER_RAD * lsp[i];
        let target = if i == 0 { lsp_hz } else { lsp_hz - last_q_hz };
        let level = lsp_dim_nearest_level(&LSP_DIMS[i], target);
        indexes[i] = level;
        let q_hz = lsp_dim_value_hz(&LSP_DIMS[i], level);
        last_q_hz = if i == 0 { q_hz } else { last_q_hz + q_hz };
    }
    indexes
}

/// Nearest quantizer level to `target_hz`, ties won by the lower index
/// (matching the reference's own tie-break rule) -- verified against a
/// dense real-valued sweep to reproduce the reference's real
/// binary-search decision exactly, not just at the level boundaries
/// themselves.
///
/// An earlier version of this function rounded `target/step - EPS` with
/// a fixed `EPS = 1e-4`, meant to nudge only an exact tie toward the
/// lower index. That was a real bug, caught by a real captured frame:
/// `EPS` was scaled in normalized step units, so in real Hz terms it was
/// ~0.0025Hz -- large enough to swallow a genuine, non-tied 0.0017Hz
/// margin that legitimately favored the *higher* index, silently
/// flipping a real quantizer decision. Fixed by comparing the two
/// bracketing levels' actual reconstructed distances directly, exactly
/// matching the reference's own `e_lo <= e_hi` rule, with no epsilon
/// guessing at all. `pub(crate)`: `codec2_3200::quantise`'s own
/// `lsp_dim_nearest_level_q16_matches_the_float_version_across_a_dense_
/// sweep`/`lsp_dim_nearest_level_matches_a_reference_binary_search_
/// across_a_dense_sweep` tests call this directly.
pub(crate) fn lsp_dim_nearest_level(dim: &LspDim, target_hz: f32) -> u32 {
    // A plain linear scan over 32 levels -- this quantizer's own real
    // computational cost (see `codec2_3200::quantise`'s own doc comment:
    // ~50 comparisons per 20ms frame across all 10 dimensions) is
    // negligible next to the codec's real cost centers (the 512-point
    // FFTs elsewhere), so there is no performance reason to prefer a
    // binary search or a closed-form index guess over the simplest, most
    // obviously correct approach.
    let mut best_level = 0u32;
    let mut best_dist = f32::MAX;
    for level in 0..crate::codec2_3200::quantise::LSP_LEVELS {
        let dist = (lsp_dim_value_hz(dim, level) - target_hz).abs();
        if dist < best_dist {
            best_dist = dist;
            best_level = level;
        }
    }
    best_level
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec2_3200::quantise::decode_lsps_delta_scalar;
    use crate::codec2_3200::quantise::tests::{fixture, read_dump};
    use crate::codec2_3200::quantise::LSP_LEVELS;

    #[test]
    fn lsp_delta_quantizer_matches_an_independent_reference_transcription_on_real_lsp_data() {
        // Independent transcription of the reference's own sequential
        // encode_lspds_scalar/decode_lspds_scalar delta-accumulation
        // control flow (built directly from its own per-dimension
        // codebook lookups, not by calling this module's functions),
        // cross-checked against the actual encode_lsps_delta_scalar/
        // decode_lsps_delta_scalar under test -- catches a bug in the
        // SEQUENTIAL accumulation logic specifically, which the
        // per-dimension `lsp_dim_nearest_level` test in `codec2_3200::
        // quantise` doesn't exercise at all.
        //
        // A tight small-error round-trip bound was tried first and
        // failed on real data: the delta quantizer's own designed range
        // (max 1400Hz for the "widened" dimensions, 800Hz for the
        // others) is real and legitimately exceeded on some real frames
        // -- some real speech frames have a genuine ~1800Hz+ gap between
        // adjacent LSPs, clamped hard by design, the same way the real
        // reference's own `index.clamp` would. Cross-checking against an
        // independent reference transcription is the correct test;
        // asserting small error universally was not.
        //
        // Reads real captured `lsp[]` values from `codec2_lsp_dump.txt`
        // (dumped right after the reference's own real `lpc_to_lsp()`
        // call), NOT this crate's own `lpc::lpc_to_lsp` output -- feeding
        // this quantizer test with LSPs this same codebase derived would
        // make it blind to a bug in `lpc_to_lsp` itself (which has its
        // own dedicated real-reference test in `floating_reference::
        // lpc`).
        fn reference_encode(lsp: &[f32; LPC_ORD]) -> [u32; LPC_ORD] {
            const HZ_PER_RAD: f32 = 4000.0 / std::f32::consts::PI;
            let mut indexes = [0u32; LPC_ORD];
            let mut last_q_hz = 0.0f32;
            for i in 0..LPC_ORD {
                let lsp_hz = HZ_PER_RAD * lsp[i];
                let target = if i == 0 { lsp_hz } else { lsp_hz - last_q_hz };
                let dim = &LSP_DIMS[i];
                let cb: Vec<f32> = (0..LSP_LEVELS).map(|j| lsp_dim_value_hz(dim, j)).collect();
                let level = if target <= cb[0] {
                    0
                } else if target >= cb[31] {
                    31
                } else {
                    let (mut lo, mut hi) = (0usize, 31usize);
                    while lo + 1 < hi {
                        let mid = (lo + hi) / 2;
                        if cb[mid] <= target {
                            lo = mid
                        } else {
                            hi = mid
                        }
                    }
                    if (cb[lo] - target).abs() <= (cb[hi] - target).abs() {
                        lo
                    } else {
                        hi
                    }
                };
                indexes[i] = level as u32;
                last_q_hz = if i == 0 {
                    cb[level]
                } else {
                    last_q_hz + cb[level]
                };
            }
            indexes
        }

        let lsp_path = fixture!("codec2_lsp_dump.txt");
        let lsp_rows = read_dump(lsp_path, LPC_ORD + 1);
        assert!(
            lsp_rows.len() > 300,
            "expected the real captured fixture corpus, got {} rows",
            lsp_rows.len()
        );
        let mut n_checked = 0;
        for row in &lsp_rows {
            let roots = row[0] as i32;
            if roots as usize != LPC_ORD {
                // Real, rare LSP root-finding failure on this frame (the
                // reference substitutes benign fallback LSPs instead) --
                // the dumped lsp[] values aren't meaningful here, skip.
                continue;
            }
            let mut lsp = [0.0f32; LPC_ORD];
            lsp.copy_from_slice(&row[1..]);
            let indexes = encode_lsps_delta_scalar(&lsp);
            let reference = reference_encode(&lsp);
            assert_eq!(indexes, reference, "real captured frame's LSPs: {lsp:?}");

            // decode_lsps_delta_scalar is a straight sum -- verify it
            // agrees with summing the same per-dimension codebook values
            // the reference transcription above used.
            let back = decode_lsps_delta_scalar(&indexes);
            let mut acc = 0.0f32;
            const RAD_PER_HZ: f32 = std::f32::consts::PI / 4000.0;
            for i in 0..LPC_ORD {
                acc += lsp_dim_value_hz(&LSP_DIMS[i], indexes[i]);
                assert!(
                    (back[i] - RAD_PER_HZ * acc).abs() < 1e-4,
                    "LSP[{i}] decode mismatch"
                );
            }
            n_checked += 1;
        }
        assert!(
            n_checked > 150,
            "only checked {n_checked} real frames -- most should have found valid LSP roots"
        );
    }

    /// Independent cross-check of the closed-form quantizer against a
    /// literal transcription of the reference's own binary-search
    /// algorithm (not the closed form itself) -- catches a closed-form
    /// bug the derivation process itself could share with the formula
    /// being tested.
    #[test]
    fn lsp_dim_nearest_level_matches_a_reference_binary_search_across_a_dense_sweep() {
        fn reference_binary_search(dim: &LspDim, target: f32) -> u32 {
            let cb: Vec<f32> = (0..LSP_LEVELS).map(|j| lsp_dim_value_hz(dim, j)).collect();
            if target <= cb[0] {
                return 0;
            }
            if target >= cb[31] {
                return 31;
            }
            let (mut lo, mut hi) = (0usize, 31usize);
            while lo + 1 < hi {
                let mid = (lo + hi) / 2;
                if cb[mid] <= target {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let e_lo = (cb[lo] - target).abs();
            let e_hi = (cb[hi] - target).abs();
            if e_lo <= e_hi {
                lo as u32
            } else {
                hi as u32
            }
        }

        for dim in &LSP_DIMS {
            let mut x = -500.0f32;
            while x <= 2000.0 {
                let closed = lsp_dim_nearest_level(dim, x);
                let reference = reference_binary_search(dim, x);
                assert_eq!(
                    closed, reference,
                    "target={x} step1={} breakpoint={} step2={}",
                    dim.step1, dim.breakpoint, dim.step2
                );
                x += 0.37; // irrational-ish step avoids only ever landing on exact boundaries
            }
        }
    }
}

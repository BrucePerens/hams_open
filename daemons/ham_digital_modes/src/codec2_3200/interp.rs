// SPDX-License-Identifier: LGPL-3.0-or-later
//! Decoder-side interpolation: `Wo`, energy, and LSPs are transmitted
//! once per 20ms frame, but synthesis runs per 10ms sub-frame, so the
//! first sub-frame's parameters are interpolated between the previous
//! frame's decoded state and this frame's newly received one. Plain
//! linear interpolation (a geometric mean for energy, since it's a
//! power-domain quantity) -- standard, not creative expression.

use super::LPC_ORD;

/// Interpolated `Wo`/voicing for the first (earlier) 10ms sub-frame,
/// given the previous frame's own decoded `(Wo, voiced)` and this
/// frame's newly received `(wo_next, voiced_next)`. Voicing-aware:
/// unvoiced frames don't have a meaningful `Wo` to interpolate (there's
/// no pitch), so an interpolated-voiced sub-frame borrows whichever
/// neighbor is actually voiced, or the plain midpoint if both are; an
/// interpolated-unvoiced sub-frame just gets `Wo` reset to its own
/// unvoiced floor.
pub fn interp_wo(
    voiced0: bool,
    prev_wo: f32,
    prev_voiced: bool,
    next_wo: f32,
    next_voiced: bool,
    w0_min: f32,
) -> f32 {
    // A voiced sub-frame flanked by two unvoiced neighbors is probably a
    // misclassified boundary -- treat as unvoiced rather than trust a
    // Wo that has nothing real to interpolate between.
    let voiced0 = voiced0 && (prev_voiced || next_voiced);
    if !voiced0 {
        return w0_min;
    }
    match (prev_voiced, next_voiced) {
        (true, true) => prev_wo + 0.5 * (next_wo - prev_wo),
        (false, true) => next_wo,
        (true, false) => prev_wo,
        (false, false) => w0_min, // unreachable given the voiced0 guard above
    }
}

/// Same voicing-aware correction `interp_wo` applies, exposed
/// separately since the caller also needs the corrected voicing flag
/// itself (not just the `Wo` it implies).
pub fn interp_voiced(voiced0: bool, prev_voiced: bool, next_voiced: bool) -> bool {
    voiced0 && (prev_voiced || next_voiced)
}

/// Energy is a power-domain quantity, so its natural interpolation is
/// geometric (equal-ratio steps), not arithmetic.
pub fn interp_energy(prev_e: f32, next_e: f32) -> f32 {
    (prev_e * next_e).sqrt()
}

pub fn interpolate_lsp(prev: &[f32; LPC_ORD], next: &[f32; LPC_ORD]) -> [f32; LPC_ORD] {
    std::array::from_fn(|i| prev[i] + 0.5 * (next[i] - prev[i]))
}

/// Fixed-point `interp_wo`: `prev_wo`/`next_wo`/`w0_min` all in Q23
/// (`lpc::COEF_FRAC_BITS`'s own angle-domain format -- `Wo`'s own real
/// range, `[0.039, 0.314]`, is the same small magnitude as an LSP
/// angle). The `(true, true)` midpoint is `(prev+next)>>1` rather than
/// `prev + (next-prev)/2` -- mathematically the same value, but avoids
/// the sign-dependent truncation-toward-zero a subtraction-based
/// integer divide would introduce on a negative `next-prev`.
pub fn interp_wo_fixed(
    voiced0: bool,
    prev_wo: i64,
    prev_voiced: bool,
    next_wo: i64,
    next_voiced: bool,
    w0_min: i64,
) -> i64 {
    let voiced0 = voiced0 && (prev_voiced || next_voiced);
    if !voiced0 {
        return w0_min;
    }
    match (prev_voiced, next_voiced) {
        (true, true) => (prev_wo + next_wo) >> 1,
        (false, true) => next_wo,
        (true, false) => prev_wo,
        (false, false) => w0_min,
    }
}

/// Fixed-point `interp_energy`: `prev_e`/`next_e`/return value all Q23
/// linear energy (`quantise::decode_energy_fixed`'s own format).
/// Geometric mean via the log-domain identity `sqrt(a*b) ==
/// exp2((log2(a)+log2(b))/2)` -- avoids a fixed-point square root
/// entirely, composed from `fixed_point::log2_q23`/`exp2_q23` (already
/// validated genuinely-integer-in/out primitives) instead.
pub fn interp_energy_fixed(prev_e: i64, next_e: i64) -> i64 {
    let log_sum = super::fixed_point::log2_q23(prev_e) + super::fixed_point::log2_q23(next_e);
    super::fixed_point::exp2_q23(log_sum >> 1)
}

/// Fixed-point `interpolate_lsp`: elementwise `(prev[i]+next[i])>>1` in
/// Q23 (`lpc::COEF_FRAC_BITS`) -- same reasoning as `interp_wo_fixed`'s
/// own midpoint for avoiding a subtraction-then-divide.
pub fn interpolate_lsp_fixed(prev: &[i64; LPC_ORD], next: &[i64; LPC_ORD]) -> [i64; LPC_ORD] {
    std::array::from_fn(|i| (prev[i] + next[i]) >> 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wo_interpolation_averages_when_both_neighbors_are_voiced() {
        let wo = interp_wo(true, 1.0, true, 2.0, true, 0.1);
        assert!((wo - 1.5).abs() < 1e-6);
    }

    #[test]
    fn wo_interpolation_borrows_the_voiced_neighbor_when_only_one_is_voiced() {
        assert!((interp_wo(true, 1.0, false, 2.0, true, 0.1) - 2.0).abs() < 1e-6);
        assert!((interp_wo(true, 1.0, true, 2.0, false, 0.1) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn wo_interpolation_falls_back_to_the_floor_when_unvoiced() {
        assert!((interp_wo(false, 1.0, true, 2.0, true, 0.1) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn a_voiced_flag_flanked_by_two_unvoiced_neighbors_is_corrected_to_unvoiced() {
        assert!(!interp_voiced(true, false, false));
        assert!(interp_voiced(true, true, false));
        assert!(interp_voiced(true, false, true));
    }

    #[test]
    fn energy_interpolation_is_geometric() {
        let e = interp_energy(4.0, 9.0);
        assert!(
            (e - 6.0).abs() < 1e-5,
            "geometric mean of 4 and 9 should be 6, got {e}"
        );
    }

    #[test]
    fn lsp_interpolation_is_the_elementwise_midpoint() {
        let prev = [0.0f32; LPC_ORD];
        let next: [f32; LPC_ORD] = std::array::from_fn(|i| i as f32 * 2.0);
        let mid = interpolate_lsp(&prev, &next);
        for (i, &m) in mid.iter().enumerate() {
            assert!((m - i as f32).abs() < 1e-6);
        }
    }

    // All three of this module's fixed-point domains (Wo/LSP angles,
    // linear energy) share 23 fractional bits -- Wo/LSP happen to reuse
    // `lpc::COEF_FRAC_BITS`'s own value since both are small angle-
    // domain quantities, while energy (up to ~1e4, `decode_energy_
    // fixed`'s own convention) needs more integer headroom but the same
    // fractional-bit count -- so one plain `23` here covers all three,
    // rather than implying a shared Q-format name across domains that
    // don't actually share one.
    const FRAC_BITS: u32 = 23;
    fn to_q23(x: f32) -> i64 {
        super::super::fixed_point::f32_to_q_exact_round(x, FRAC_BITS)
    }
    fn from_q23(x: i64) -> f32 {
        x as f32 / (1i64 << FRAC_BITS) as f32
    }

    #[test]
    fn wo_interpolation_fixed_averages_when_both_neighbors_are_voiced() {
        let wo = interp_wo_fixed(true, to_q23(1.0), true, to_q23(2.0), true, to_q23(0.1));
        assert!((from_q23(wo) - 1.5).abs() < 1e-5);
    }

    #[test]
    fn wo_interpolation_fixed_borrows_the_voiced_neighbor_when_only_one_is_voiced() {
        assert!(
            (from_q23(interp_wo_fixed(true, to_q23(1.0), false, to_q23(2.0), true, to_q23(0.1)))
                - 2.0)
                .abs()
                < 1e-5
        );
        assert!(
            (from_q23(interp_wo_fixed(true, to_q23(1.0), true, to_q23(2.0), false, to_q23(0.1)))
                - 1.0)
                .abs()
                < 1e-5
        );
    }

    #[test]
    fn wo_interpolation_fixed_falls_back_to_the_floor_when_unvoiced() {
        assert!(
            (from_q23(interp_wo_fixed(false, to_q23(1.0), true, to_q23(2.0), true, to_q23(0.1)))
                - 0.1)
                .abs()
                < 1e-5
        );
    }

    #[test]
    fn energy_interpolation_fixed_is_geometric() {
        let e = interp_energy_fixed(to_q23(4.0), to_q23(9.0));
        assert!(
            (from_q23(e) - 6.0).abs() < 1e-3,
            "geometric mean of 4 and 9 should be ~6, got {}",
            from_q23(e)
        );
    }

    #[test]
    fn energy_interpolation_fixed_matches_the_float_version_across_real_captured_energy_pairs() {
        // Real captured encoder-side e values (not synthetic 4/9),
        // covering the format's own real dynamic range -- see
        // fixed_point.rs's own tests for why this fixture is the right
        // one to use for energy-domain validation.
        let e_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_3200/codec2_enc_e_dump.txt"
        );
        let es: Vec<f32> = std::fs::read_to_string(e_path)
            .unwrap()
            .lines()
            .map(|l| l.trim().parse().unwrap())
            .collect();
        assert!(es.len() > 300, "expected the real captured fixture corpus, got {} rows", es.len());

        let mut max_rel_err = 0.0f32;
        for pair in es.chunks(2) {
            if pair.len() < 2 {
                continue;
            }
            let (a, b) = (pair[0].max(1e-3), pair[1].max(1e-3));
            let float_mid = interp_energy(a, b);
            let fixed_mid = from_q23(interp_energy_fixed(to_q23(a), to_q23(b)));
            max_rel_err = max_rel_err.max(((fixed_mid - float_mid) / float_mid).abs());
        }
        assert!(
            max_rel_err < 1e-3,
            "interp_energy_fixed diverged from interp_energy by {max_rel_err} relative on real captured data"
        );
    }

    #[test]
    fn lsp_interpolation_fixed_is_the_elementwise_midpoint() {
        let prev = [0i64; LPC_ORD];
        let next: [i64; LPC_ORD] = std::array::from_fn(|i| to_q23(i as f32 * 2.0));
        let mid = interpolate_lsp_fixed(&prev, &next);
        for (i, &m) in mid.iter().enumerate() {
            assert!((from_q23(m) - i as f32).abs() < 1e-5);
        }
    }
}

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
}

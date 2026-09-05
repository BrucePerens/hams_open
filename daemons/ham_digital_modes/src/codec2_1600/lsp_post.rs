// SPDX-License-Identifier: LGPL-3.0-or-later
//! Decoder-side LSP post-processing specific to 1600bps (and used more
//! broadly upstream, but nothing else in this crate needs it yet):
//! enforcing ascending LSP order after scalar dequantization, bandwidth
//! expansion to keep adjacent LSPs from collapsing together, and the
//! generalized (arbitrary-weight) LSP interpolation 40ms-cadence LSPs
//! need across the three intermediate 10ms sub-frames -- unlike
//! `codec2_3200::interp::interpolate_lsp`'s fixed 50/50 midpoint, which
//! only ever needs one.

use crate::codec2_3200::LPC_ORD;

/// Forces ascending LSP order after dequantization by swapping any
/// adjacent pair found out of order, nudging each apart by 0.1 radians.
/// Mirrors the reference's own real (if unusual) restart behavior after
/// a swap exactly: a C `for (i=1; i<order; i++) { ...; i=1; }` resets
/// to `i=1` but the `for`'s own post-body `i++` still runs before the
/// next check, so the scan actually resumes at index 2, not 1 -- this
/// loop reproduces that with an explicit index rather than "fixing" it,
/// since it's the real reference's own real behavior, not a bug to
/// correct.
pub fn check_lsp_order(lsp: &mut [f32; LPC_ORD]) -> usize {
    let mut swaps = 0usize;
    let mut i = 1usize;
    while i < LPC_ORD {
        if lsp[i] < lsp[i - 1] {
            swaps += 1;
            let tmp = lsp[i - 1];
            lsp[i - 1] = lsp[i] - 0.1;
            lsp[i] = tmp + 0.1;
            i = 1;
        }
        i += 1;
    }
    swaps
}

const HZ_TO_RAD: f32 = std::f32::consts::PI / 4000.0;

/// Bandwidth expansion: prevents any two adjacent LSPs (indices 1..4
/// use `min_sep_low`, 4..order use `min_sep_high`) from separating by
/// less than the given Hz margin -- LSP quantization errors under
/// ~12.5Hz are inaudible, so this is the real reference's own minimum
/// separation floor, not a tunable choice made here.
pub fn bw_expand_lsps(lsp: &mut [f32; LPC_ORD], min_sep_low: f32, min_sep_high: f32) {
    for i in 1..4 {
        if (lsp[i] - lsp[i - 1]) < min_sep_low * HZ_TO_RAD {
            lsp[i] = lsp[i - 1] + min_sep_low * HZ_TO_RAD;
        }
    }
    for i in 4..LPC_ORD {
        if lsp[i] - lsp[i - 1] < min_sep_high * HZ_TO_RAD {
            lsp[i] = lsp[i - 1] + min_sep_high * HZ_TO_RAD;
        }
    }
}

/// Weighted elementwise LSP interpolation -- `codec2_3200`'s own
/// `interpolate_lsp` is this at a fixed `weight=0.5`; 1600bps's LSPs
/// update only once per 40ms, so the three intermediate 10ms sub-frames
/// need weights 0.25/0.5/0.75 instead of one single midpoint.
pub fn interpolate_lsp_ver2(
    prev: &[f32; LPC_ORD],
    next: &[f32; LPC_ORD],
    weight: f32,
) -> [f32; LPC_ORD] {
    std::array::from_fn(|i| (1.0 - weight) * prev[i] + weight * next[i])
}

use crate::codec2_3200::fixed_point::f32_to_q_exact_round;
use std::sync::OnceLock;

const FRAC_BITS: u32 = 23;

fn swap_nudge_q23() -> i64 {
    static V: OnceLock<i64> = OnceLock::new();
    *V.get_or_init(|| f32_to_q_exact_round(0.1, FRAC_BITS))
}

/// Fixed-point sibling of `check_lsp_order`: identical logic (including
/// the same real reference restart-index quirk), entirely in `i64` Q23
/// arithmetic.
pub fn check_lsp_order_fixed(lsp: &mut [i64; LPC_ORD]) -> usize {
    let mut swaps = 0usize;
    let mut i = 1usize;
    while i < LPC_ORD {
        if lsp[i] < lsp[i - 1] {
            swaps += 1;
            let tmp = lsp[i - 1];
            lsp[i - 1] = lsp[i] - swap_nudge_q23();
            lsp[i] = tmp + swap_nudge_q23();
            i = 1;
        }
        i += 1;
    }
    swaps
}

/// `50.0 * (pi/4000)` in Q23 -- the real reference's own `min_sep_low`
/// for the 1600bps decoder's one real `bw_expand_lsps_fixed` call site,
/// computed once via `OnceLock` rather than hand-typed. A no-argument
/// function rather than a value looked up by Hz: this mode's decoder
/// only ever needs these two specific margins, so there's no real
/// "wrong Hz value" case to guard against at runtime.
pub fn min_sep_low_q23() -> i64 {
    static V: OnceLock<i64> = OnceLock::new();
    *V.get_or_init(|| f32_to_q_exact_round(50.0 * HZ_TO_RAD, FRAC_BITS))
}

/// `100.0 * (pi/4000)` in Q23 -- the real reference's own `min_sep_high`
/// for the same call site `min_sep_low_q23` documents.
pub fn min_sep_high_q23() -> i64 {
    static V: OnceLock<i64> = OnceLock::new();
    *V.get_or_init(|| f32_to_q_exact_round(100.0 * HZ_TO_RAD, FRAC_BITS))
}

/// Fixed-point sibling of `bw_expand_lsps`, entirely in `i64` Q23
/// arithmetic. `min_sep_low_q23`/`min_sep_high_q23` come from
/// `min_sep_low_q23()`/`min_sep_high_q23()` above.
pub fn bw_expand_lsps_fixed(lsp: &mut [i64; LPC_ORD], min_sep_low_q23: i64, min_sep_high_q23: i64) {
    for i in 1..4 {
        if (lsp[i] - lsp[i - 1]) < min_sep_low_q23 {
            lsp[i] = lsp[i - 1] + min_sep_low_q23;
        }
    }
    for i in 4..LPC_ORD {
        if lsp[i] - lsp[i - 1] < min_sep_high_q23 {
            lsp[i] = lsp[i - 1] + min_sep_high_q23;
        }
    }
}

/// Fixed-point sibling of `interpolate_lsp_ver2`, restricted to the
/// three exact quarter-weights the decoder ever actually uses
/// (0.25/0.5/0.75, `quarters` = 1/2/3) -- each is an exact binary
/// fraction, so this is a plain integer multiply-add and a rounded
/// right-shift, no division and no rounding loss beyond that one
/// final round-to-nearest shift.
pub fn interpolate_lsp_ver2_fixed(
    prev: &[i64; LPC_ORD],
    next: &[i64; LPC_ORD],
    quarters: i64,
) -> [i64; LPC_ORD] {
    debug_assert!((1..=3).contains(&quarters), "quarters must be 1, 2, or 3 (0.25/0.5/0.75)");
    std::array::from_fn(|i| ((4 - quarters) * prev[i] + quarters * next[i] + 2) >> 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_lsp_order_leaves_an_already_ordered_vector_unchanged() {
        let mut lsp: [f32; super::LPC_ORD] =
            std::array::from_fn(|i| 0.1 + 0.2 * i as f32);
        let original = lsp;
        let swaps = check_lsp_order(&mut lsp);
        assert_eq!(swaps, 0);
        assert_eq!(lsp, original);
    }

    #[test]
    fn check_lsp_order_fixes_a_single_out_of_order_pair() {
        let mut lsp: [f32; super::LPC_ORD] =
            std::array::from_fn(|i| 0.1 + 0.2 * i as f32);
        lsp.swap(3, 4);
        check_lsp_order(&mut lsp);
        for i in 1..lsp.len() {
            assert!(lsp[i] > lsp[i - 1], "still out of order at {i}: {lsp:?}");
        }
    }

    #[test]
    fn bw_expand_lsps_separates_two_lsps_that_start_too_close() {
        let mut lsp = [0.0f32; super::LPC_ORD];
        for (i, v) in lsp.iter_mut().enumerate() {
            *v = 0.05 * i as f32;
        }
        bw_expand_lsps(&mut lsp, 50.0, 100.0);
        for i in 1..4 {
            assert!(
                lsp[i] - lsp[i - 1] >= 50.0 * HZ_TO_RAD - 1e-6,
                "index {i}: gap {} below min_sep_low",
                lsp[i] - lsp[i - 1]
            );
        }
        for i in 4..lsp.len() {
            assert!(
                lsp[i] - lsp[i - 1] >= 100.0 * HZ_TO_RAD - 1e-6,
                "index {i}: gap {} below min_sep_high",
                lsp[i] - lsp[i - 1]
            );
        }
    }

    #[test]
    fn interpolate_lsp_ver2_at_weight_zero_and_one_returns_the_endpoints() {
        let prev: [f32; super::LPC_ORD] = std::array::from_fn(|i| i as f32);
        let next: [f32; super::LPC_ORD] = std::array::from_fn(|i| 10.0 + i as f32);
        assert_eq!(interpolate_lsp_ver2(&prev, &next, 0.0), prev);
        assert_eq!(interpolate_lsp_ver2(&prev, &next, 1.0), next);
    }

    fn to_q23(x: f32) -> i64 {
        f32_to_q_exact_round(x, FRAC_BITS)
    }
    fn from_q23(x: i64) -> f32 {
        x as f32 / (1i64 << FRAC_BITS) as f32
    }

    #[test]
    fn check_lsp_order_fixed_matches_the_float_version_on_an_out_of_order_pair() {
        let mut lsp: [f32; super::LPC_ORD] = std::array::from_fn(|i| 0.1 + 0.2 * i as f32);
        lsp.swap(3, 4);
        let mut lsp_q23: [i64; super::LPC_ORD] = std::array::from_fn(|i| to_q23(lsp[i]));

        check_lsp_order(&mut lsp);
        check_lsp_order_fixed(&mut lsp_q23);

        for i in 0..super::LPC_ORD {
            assert!(
                (from_q23(lsp_q23[i]) - lsp[i]).abs() < 1e-4,
                "index {i}: fixed={} float={}",
                from_q23(lsp_q23[i]),
                lsp[i]
            );
        }
    }

    #[test]
    fn bw_expand_lsps_fixed_matches_the_float_version() {
        let mut lsp = [0.0f32; super::LPC_ORD];
        for (i, v) in lsp.iter_mut().enumerate() {
            *v = 0.05 * i as f32;
        }
        let mut lsp_q23: [i64; super::LPC_ORD] = std::array::from_fn(|i| to_q23(lsp[i]));

        bw_expand_lsps(&mut lsp, 50.0, 100.0);
        bw_expand_lsps_fixed(&mut lsp_q23, min_sep_low_q23(), min_sep_high_q23());

        for i in 0..super::LPC_ORD {
            assert!(
                (from_q23(lsp_q23[i]) - lsp[i]).abs() < 1e-4,
                "index {i}: fixed={} float={}",
                from_q23(lsp_q23[i]),
                lsp[i]
            );
        }
    }

    #[test]
    fn interpolate_lsp_ver2_fixed_matches_the_float_version_at_all_three_real_weights() {
        let prev: [f32; super::LPC_ORD] = std::array::from_fn(|i| 0.1 + 0.05 * i as f32);
        let next: [f32; super::LPC_ORD] = std::array::from_fn(|i| 0.2 + 0.06 * i as f32);
        let prev_q23: [i64; super::LPC_ORD] = std::array::from_fn(|i| to_q23(prev[i]));
        let next_q23: [i64; super::LPC_ORD] = std::array::from_fn(|i| to_q23(next[i]));

        for (weight, quarters) in [(0.25, 1), (0.5, 2), (0.75, 3)] {
            let float_result = interpolate_lsp_ver2(&prev, &next, weight);
            let fixed_result = interpolate_lsp_ver2_fixed(&prev_q23, &next_q23, quarters);
            for i in 0..super::LPC_ORD {
                assert!(
                    (from_q23(fixed_result[i]) - float_result[i]).abs() < 1e-5,
                    "weight={weight} index {i}: fixed={} float={}",
                    from_q23(fixed_result[i]),
                    float_result[i]
                );
            }
        }
    }
}

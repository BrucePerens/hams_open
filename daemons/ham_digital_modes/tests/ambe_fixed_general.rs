// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::general`'s arithmetic primitives against `f64` std -- kept as an
//! integration test (outside `src/ambe/fixed`) rather than an inline `#[cfg(test)]` module,
//! specifically so no `f64` token ever needs to appear inside `src/ambe/fixed` itself. See
//! `ambe::fixed`'s own doc comment for why that's enforced, not just a style preference.

use ham_digital_modes::ambe::fixed::general::trig::{cos_q16, sin_q16};

fn phase_from_radians(radians: f64) -> u32 {
    let turns = (radians / (2.0 * std::f64::consts::PI)).rem_euclid(1.0);
    (turns * (u32::MAX as f64 + 1.0)) as u64 as u32
}

/// Q16.16 sine/cosine must be within a few parts in 2^16 of the real `f64` value across a dense
/// sweep -- the interpolation error budget for a 257-entry quarter-wave table is roughly
/// `(pi/512)^2/8 ~ 4.7e-6` relative (the standard piecewise-linear-interpolation error bound for a
/// function with bounded second derivative, here `|sin''| <= 1`), well under 1 part in 65536; this
/// asserts a looser, safely-covering bound (`3` counts, ~4.6e-5) rather than the tightest bound the
/// math predicts, so the test isn't brittle to exactly which angles are swept.
#[test]
fn sin_cos_q16_track_f64_across_a_dense_sweep() {
    const MAX_ERROR_COUNTS: i32 = 3;
    for i in 0..10_000 {
        let radians = 2.0 * std::f64::consts::PI * (i as f64) / 10_000.0;
        let phase = phase_from_radians(radians);
        let expected_sin = (radians.sin() * 65536.0).round() as i32;
        let expected_cos = (radians.cos() * 65536.0).round() as i32;
        let actual_sin = sin_q16(phase);
        let actual_cos = cos_q16(phase);
        assert!(
            (actual_sin - expected_sin).abs() <= MAX_ERROR_COUNTS,
            "radians={radians}: sin_q16={actual_sin}, expected={expected_sin}"
        );
        assert!(
            (actual_cos - expected_cos).abs() <= MAX_ERROR_COUNTS,
            "radians={radians}: cos_q16={actual_cos}, expected={expected_cos}"
        );
    }
}

#[test]
fn sin_cos_q16_hit_the_four_cardinal_angles_exactly_at_full_scale() {
    assert_eq!(sin_q16(0), 0);
    assert_eq!(cos_q16(0), 65536);
    assert_eq!(sin_q16(1 << 30), 65536);
    assert_eq!(cos_q16(1 << 30), 0);
    assert_eq!(sin_q16(1 << 31), 0);
    assert_eq!(cos_q16(1 << 31), -65536);
    assert_eq!(sin_q16(3 << 30), -65536);
    assert_eq!(cos_q16(3 << 30), 0);
}

#[test]
fn sin_q16_never_exceeds_full_scale_magnitude() {
    for i in 0..10_000u32 {
        let phase = i.wrapping_mul(u32::MAX / 10_000);
        assert!(sin_q16(phase).abs() <= 65536, "phase={phase}: sin_q16={}", sin_q16(phase));
        assert!(cos_q16(phase).abs() <= 65536, "phase={phase}: cos_q16={}", cos_q16(phase));
    }
}

/// The Pythagorean identity, checked in fixed-point arithmetic itself (not by comparison to `f64`)
/// -- a real end-to-end sanity check that `sin_q16`/`cos_q16` are self-consistent, not just each
/// individually close to floating point.
#[test]
fn sin_q16_squared_plus_cos_q16_squared_is_approximately_one() {
    for i in 0..1000u32 {
        let phase = i.wrapping_mul(u32::MAX / 1000);
        let s = sin_q16(phase) as i64;
        let c = cos_q16(phase) as i64;
        let sum_sq = (s * s + c * c) >> 16; // back to Q16.16 after squaring doubled the scale
        let one = 1i64 << 16;
        assert!((sum_sq - one).abs() <= 8, "phase={phase}: sin^2+cos^2={sum_sq}, expected {one}");
    }
}

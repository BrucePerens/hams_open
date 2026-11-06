// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::general`'s arithmetic primitives against `f64` std -- kept as an
//! integration test (outside `src/ambe/fixed`) rather than an inline `#[cfg(test)]` module,
//! specifically so no `f64` token ever needs to appear inside `src/ambe/fixed` itself. See
//! `ambe::fixed`'s own doc comment for why that's enforced, not just a style preference.

use ham_digital_modes::ambe::fixed::general::explog::{exp2_q16, log2_q16, log2_q16_i64};
use ham_digital_modes::ambe::fixed::general::fixed_ops::{
    div_q16, div_q16_i64, mul_q16, mul_q16_i64, sqrt_q16, sqrt_wide_q16,
};
use ham_digital_modes::ambe::fixed::general::trig::{
    cos_pi_frac, cos_q16, phase_from_radians_q16, phase_from_radians_q32, radians_from_phase_q16,
    radians_q32_from_phase, sin_q16,
};

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
// Tests [@ANCHOR: ambe_fixed_sin_q16]
// Tests [@ANCHOR: ambe_fixed_quarter_sine_interp]
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

// Tests [@ANCHOR: ambe_fixed_sin_q16]
// Tests [@ANCHOR: ambe_fixed_quarter_sine_interp]
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
        assert!(
            sin_q16(phase).abs() <= 65536,
            "phase={phase}: sin_q16={}",
            sin_q16(phase)
        );
        assert!(
            cos_q16(phase).abs() <= 65536,
            "phase={phase}: cos_q16={}",
            cos_q16(phase)
        );
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
        assert!(
            (sum_sq - one).abs() <= 8,
            "phase={phase}: sin^2+cos^2={sum_sq}, expected {one}"
        );
    }
}

fn to_q16(v: f64) -> i32 {
    (v * 65536.0).round() as i32
}

/// `log2_q16` must track `f64::log2` within a small, stated error across many decades of magnitude
/// -- the dequantize chain calls this on amplitude values that can range widely, not just values
/// near 1.0, so the sweep spans from a small fraction to a large multiple of one Q16.16 unit.
// Tests [@ANCHOR: ambe_fixed_log2_q16]
#[test]
fn log2_q16_tracks_f64_across_many_decades() {
    const MAX_ERROR_COUNTS: i32 = 6;
    for i in 1..2000 {
        // Geometric sweep from about 2^-10 to 2^10 in real-value terms.
        let real = 2f64.powf(-10.0 + 20.0 * (i as f64) / 2000.0);
        let x = to_q16(real);
        if x <= 0 {
            continue; // too small to represent in Q16.16 at all -- not this test's concern.
        }
        // Compare against what `x` itself represents post-quantization, not the original `real`
        // before it -- Q16.16 has only 16 fractional bits, so a small `real` already loses relative
        // precision converting into `x` in the first place, and that loss is not `log2_q16`'s own
        // error to account for.
        let x_real = x as f64 / 65536.0;
        let expected = to_q16(x_real.log2());
        let actual = log2_q16(x);
        let actual_real = actual as f64 / 65536.0;
        let expected_real = expected as f64 / 65536.0;
        assert!(
            (actual - expected).abs() <= MAX_ERROR_COUNTS,
            "x_real={x_real}: log2_q16={actual} ({actual_real}), expected={expected} ({expected_real})"
        );
    }
}

/// `exp2_q16` must track `f64::exp2` within a small, stated *relative* error (an absolute Q16.16
/// count bound doesn't make sense here since the output magnitude itself varies by many decades) --
/// across a range where Q16.16's own fixed 16-bit fractional precision still leaves enough
/// *relative* precision to make that a fair bar (see [`exp2_q16`]'s own doc comment: a result near
/// `2^-10` has only ~6 significant bits left, an inherent fixed-point property, not a bug). `-8..8`
/// is comfortably inside that: even at the extreme, `2^-8` still has 8 significant bits (`2^16 *
/// 2^-8 = 2^8`).
// Tests [@ANCHOR: ambe_fixed_exp2_q16]
#[test]
fn exp2_q16_tracks_f64_within_its_comfortable_precision_range() {
    // 8 significant bits at the `2^-8` edge of this range gives a quantization step of `1/256`
    // (~0.4%); this bound leaves headroom above that rather than chasing a tighter number the
    // format can't actually deliver at the edge.
    const MAX_RELATIVE_ERROR: f64 = 3e-3;
    for i in -800..800 {
        let exponent = (i as f64) / 100.0; // -8.0..8.0
        let y = to_q16(exponent);
        // Compare against what `y` itself represents post-quantization, same reasoning as
        // `log2_q16`'s own test above.
        let y_real = y as f64 / 65536.0;
        let expected_real = y_real.exp2();
        let actual_real = exp2_q16(y) as f64 / 65536.0;
        let relative_error = ((actual_real - expected_real) / expected_real).abs();
        assert!(
            relative_error <= MAX_RELATIVE_ERROR,
            "y_real={y_real}: exp2_q16 real={actual_real}, expected={expected_real}, rel_err={relative_error}"
        );
    }
}

/// Pins the known precision floor from [`exp2_q16`]'s own doc comment as an actual test, rather than
/// leaving it as an unverified prose claim: relative error must still be *bounded* (never wildly
/// wrong, never a panic) even well outside the comfortable range, even though it's no longer tight.
#[test]
fn exp2_q16_degrades_gracefully_rather_than_incorrectly_at_extreme_magnitudes() {
    for &exponent in &[-16.0, -20.0, -30.0, 20.0, 30.0] {
        let y = to_q16(exponent);
        let y_real = y as f64 / 65536.0;
        let expected_real = y_real.exp2();
        let actual = exp2_q16(y);
        let actual_real = actual as f64 / 65536.0;
        // Q16.16's smallest representable positive value is 1/65536 -- a true result below half
        // that (the round-to-nearest threshold) has no closer representable value than 0, so
        // rounding down to exactly 0 is the *correct* fixed-point answer, not underflow-as-bug.
        if expected_real < 0.5 / 65536.0 {
            assert_eq!(
                actual_real, 0.0,
                "y_real={y_real}: expected_real={expected_real} rounds to 0 in Q16.16"
            );
            continue;
        }
        // Likewise, a true result above i32::MAX's own Q16.16 range (~32768.0) genuinely cannot be
        // represented -- saturating to i32::MAX is the correct answer, not a large relative error.
        if expected_real > i32::MAX as f64 / 65536.0 {
            assert_eq!(
                actual,
                i32::MAX,
                "y_real={y_real}: expected_real={expected_real} saturates in Q16.16"
            );
            continue;
        }
        let relative_error = ((actual_real - expected_real) / expected_real).abs();
        assert!(
            relative_error <= 0.5,
            "y_real={y_real}: exp2_q16 real={actual_real}, expected={expected_real}, rel_err={relative_error} (expected bounded, not necessarily tight)"
        );
    }
}

/// `log2_q16` and `exp2_q16` must round-trip each other closely within the same comfortable
/// precision range the two tests above establish.
#[test]
fn log2_then_exp2_round_trips_closely() {
    const MAX_RELATIVE_ERROR: f64 = 1e-3;
    for i in 1..500 {
        let real = 2f64.powf(-6.0 + 12.0 * (i as f64) / 500.0); // 2^-6 .. 2^6
        let x = to_q16(real);
        let x_real = x as f64 / 65536.0; // what `x` itself represents, post-quantization
        let roundtrip = exp2_q16(log2_q16(x));
        let roundtrip_real = roundtrip as f64 / 65536.0;
        let relative_error = ((roundtrip_real - x_real) / x_real).abs();
        assert!(
            relative_error <= MAX_RELATIVE_ERROR,
            "x_real={x_real}: round-trip={roundtrip_real}, rel_err={relative_error}"
        );
    }
}

#[test]
fn log2_q16_of_non_positive_input_returns_the_negative_sentinel_not_a_panic() {
    assert_eq!(log2_q16(0), i32::MIN);
    assert_eq!(log2_q16(-1), i32::MIN);
    assert_eq!(log2_q16(i32::MIN), i32::MIN);
}

#[test]
fn exp2_q16_saturates_instead_of_overflowing_or_panicking() {
    assert_eq!(exp2_q16(i32::MAX), i32::MAX);
    assert_eq!(exp2_q16(i32::MIN), 0);
}

// Tests [@ANCHOR: ambe_fixed_log2_q16]
// Tests [@ANCHOR: ambe_fixed_exp2_q16]
#[test]
fn log2_q16_and_exp2_q16_hit_exact_powers_of_two() {
    assert_eq!(log2_q16(65536), 0); // log2(1.0) = 0
    assert_eq!(log2_q16(131072), 65536); // log2(2.0) = 1.0
    assert_eq!(log2_q16(32768), -65536); // log2(0.5) = -1.0
    assert_eq!(exp2_q16(0), 65536); // 2^0 = 1.0
    assert_eq!(exp2_q16(65536), 131072); // 2^1 = 2.0
    assert_eq!(exp2_q16(-65536), 32768); // 2^-1 = 0.5
}

// Tests [@ANCHOR: ambe_fixed_mul_q16]
#[test]
fn mul_q16_rounds_to_nearest_and_tracks_f64() {
    assert_eq!(mul_q16(98304, 131072), 196608); // 1.5 * 2.0 = 3.0
    assert_eq!(mul_q16(1, 32768), 1); // 0.5 LSB rounds up
    assert_eq!(mul_q16(-1, 32768), -1); // and away from zero when negative
    assert_eq!(mul_q16(1, 32767), 0); // just under half an LSB rounds down
                                      // A negative product is biased down by up to one count (`(p - half) >> 16` floors): see the to-do
                                      // `ambe-fixed-mul-q16-negative-rounding-bias-*`. The bound below is today's behaviour, not the ideal.
    assert!((mul_q16(65536, -65536) + 65536).abs() <= 1);
    assert_eq!(mul_q16(65536, 65536), 65536);
    for a in (-200_000..200_000).step_by(7919) {
        for b in (-200_000..200_000).step_by(6007) {
            let want = ((a as f64 / 65536.0) * (b as f64 / 65536.0) * 65536.0).round() as i32;
            assert!((mul_q16(a, b) - want).abs() <= 1, "{a} * {b}");
        }
    }
}

// Tests [@ANCHOR: ambe_fixed_div_q16]
#[test]
fn div_q16_truncates_toward_zero_and_saturates_on_zero_or_overflow_instead_of_panicking() {
    assert_eq!(div_q16(196608, 131072), 98304); // 3.0 / 2.0 = 1.5
    assert_eq!(div_q16(-196608, 131072), -98304);
    assert_eq!(div_q16(65536, 196608), 21845); // 1/3, truncated
    assert_eq!(div_q16(5, 0), i32::MAX);
    assert_eq!(div_q16(0, 0), i32::MAX);
    assert_eq!(div_q16(-5, 0), i32::MIN);
    assert_eq!(div_q16(i32::MAX, 1), i32::MAX); // the true quotient does not fit
    assert_eq!(div_q16(i32::MIN, 1), i32::MIN);
}

// Tests [@ANCHOR: ambe_fixed_mul_q16_i64]
#[test]
fn mul_q16_i64_keeps_a_product_that_overflows_i64_and_rounds_like_mul_q16() {
    for a in [
        0i64,
        1,
        -1,
        12345,
        -987_654_321,
        1 << 40,
        -(1 << 50),
        i64::MAX,
        i64::MIN + 1,
    ] {
        // Times 1.0; a negative `a` can come out one count low (the same floor bias as `mul_q16`).
        assert!((mul_q16_i64(a, 65536) - a).abs() <= 1, "a = {a}");
        if a >= 0 {
            assert_eq!(mul_q16_i64(a, 65536), a, "a = {a}");
        }
        if a.unsigned_abs() < (1 << 62) {
            assert!((mul_q16_i64(a, 131072) - a * 2).abs() <= 1, "a = {a}");
        }
    }
    assert_eq!(mul_q16_i64(1, 32768), 1);
    assert!((mul_q16_i64(-1, 32768) + 1).abs() <= 1);
    assert_eq!(mul_q16_i64(1, 32767), 0);
    // a * b alone is 2^66, above i64::MAX, but the result 2^66 / 2^16 = 2^50 fits.
    assert_eq!(mul_q16_i64(1 << 62, 1 << 4), 1 << 50);
}

// Tests [@ANCHOR: ambe_fixed_div_q16_i64]
#[test]
fn div_q16_i64_gives_the_ratio_of_wide_values_and_saturates_instead_of_panicking() {
    assert_eq!(div_q16_i64(3 << 16, 2 << 16), 98304);
    // The same ratio at a magnitude where `a << 16` alone would overflow i64.
    assert_eq!(div_q16_i64(3 << 50, 2 << 50), 98304);
    assert_eq!(div_q16_i64(-(1 << 40), 1 << 40), -65536);
    assert_eq!(div_q16_i64(0, 7), 0);
    assert_eq!(div_q16_i64(5, 0), i32::MAX);
    assert_eq!(div_q16_i64(-5, 0), i32::MIN);
    // After normalising, the divisor shifts away to zero: the ratio is far beyond i32.
    assert_eq!(div_q16_i64(1 << 62, 1), i32::MAX);
    assert_eq!(div_q16_i64(-(1 << 62), 1), i32::MIN);
}

// Tests [@ANCHOR: ambe_fixed_sqrt_q16]
#[test]
fn sqrt_q16_is_exact_to_the_nearest_q16_value_and_is_zero_for_a_negative() {
    assert_eq!(sqrt_q16(4 << 16), 2 << 16);
    assert_eq!(sqrt_q16(0), 0);
    assert_eq!(sqrt_q16(-1), 0);
    assert_eq!(sqrt_q16(i32::MIN), 0);
    for x in (1..i32::MAX).step_by(104_729) {
        let want = ((x as f64 / 65536.0).sqrt() * 65536.0).floor() as i32;
        assert!((sqrt_q16(x) - want).abs() <= 1, "x = {x}");
    }
    sqrt_q16(i32::MAX); // the largest input does not panic or overflow
}

// Tests [@ANCHOR: ambe_fixed_sqrt_wide_q16]
#[test]
fn sqrt_wide_q16_handles_values_far_beyond_i32_and_is_zero_for_zero_or_negative() {
    assert_eq!(sqrt_wide_q16(4 << 16), 2 << 16);
    assert_eq!(sqrt_wide_q16(0), 0);
    assert_eq!(sqrt_wide_q16(-5), 0);
    // Real values up to about 1e5 * 65536 (the unvoiced-synthesis range) and the largest i64.
    for x in [1i64, 65536, 47_000_000_000_000_000, 1 << 60, i64::MAX] {
        let want = (x as f64 / 65536.0).sqrt() * 65536.0;
        let got = sqrt_wide_q16(x) as f64;
        assert!(
            (got - want).abs() <= 1.0 + want * 1e-9,
            "x = {x}: {got} against {want}"
        );
    }
}

// Tests [@ANCHOR: ambe_fixed_log2_q16_i64]
#[test]
fn log2_q16_i64_matches_log2_q16_where_both_apply_and_reaches_beyond_i32() {
    for x in [1i32, 3, 100, 32768, 65536, 100_000, 7_777_777, i32::MAX] {
        assert_eq!(log2_q16_i64(x as i64), log2_q16(x), "x = {x}");
    }
    // 2^40 / 2^16 = 2^24, so the log is 24.0.
    assert_eq!(log2_q16_i64(1 << 40), 24 << 16);
    assert_eq!(log2_q16_i64(0), i32::MIN);
    assert_eq!(log2_q16_i64(-1), i32::MIN);
    let want = ((1e12f64).log2() * 65536.0).round();
    let got = log2_q16_i64((1e12f64 * 65536.0) as i64) as f64;
    assert!((got - want).abs() <= 3.0, "{got} against {want}");
}

// Tests [@ANCHOR: ambe_fixed_cos_pi_frac]
#[test]
fn cos_pi_frac_tracks_f64_for_the_dct_shapes_and_hits_a_half_turn_exactly() {
    assert_eq!(cos_pi_frac(2, 1, 1), -65536); // cos(pi * 2 * 0.5 / 1)
    assert_eq!(cos_pi_frac(0, 1, 1), 65536);
    for a in 0..9i64 {
        for b2 in (1..16i64).step_by(2) {
            for c in 1..9i64 {
                let want = ((std::f64::consts::PI * a as f64 * (b2 as f64 / 2.0) / c as f64).cos()
                    * 65536.0)
                    .round() as i32;
                assert!(
                    (cos_pi_frac(a, b2, c) - want).abs() <= 6,
                    "a={a} b2={b2} c={c}: {} against {want}",
                    cos_pi_frac(a, b2, c)
                );
            }
        }
    }
}

// Tests [@ANCHOR: ambe_fixed_radians_from_phase_q16]
// Tests [@ANCHOR: ambe_fixed_round_div_to_u32]
#[test]
fn radians_from_phase_q16_inverts_phase_from_radians_q16_within_one_count() {
    // A quarter turn is pi/2 rad, a half turn wraps to -pi (the i32 reading of 1 << 31).
    assert_eq!(radians_from_phase_q16(0), 0);
    assert!((radians_from_phase_q16(1 << 30) - 102_944).abs() <= 1);
    assert!((radians_from_phase_q16(1 << 31) + 205_887).abs() <= 1);
    assert!((radians_from_phase_q16(3 << 30) + 102_944).abs() <= 1);
    for angle in (-205_000..=205_000).step_by(997) {
        let back = radians_from_phase_q16(phase_from_radians_q16(angle));
        assert!((back - angle).abs() <= 1, "angle {angle}: back {back}");
    }
}

// Tests [@ANCHOR: ambe_fixed_radians_q32_from_phase]
// Tests [@ANCHOR: ambe_fixed_round_div_to_u32]
#[test]
fn radians_q32_from_phase_recovers_a_bounded_radian_value_and_round_trips_the_phase() {
    assert_eq!(radians_q32_from_phase(0), 0);
    let quarter = (std::f64::consts::FRAC_PI_2 * 4_294_967_296.0).round();
    assert!((radians_q32_from_phase(1 << 30) as f64 - quarter).abs() <= 1.0);
    assert!(radians_q32_from_phase(3 << 30) < 0);
    for phase in (0..u32::MAX - 1_000_003).step_by(1_000_003) {
        assert_eq!(
            phase_from_radians_q32(radians_q32_from_phase(phase)),
            phase,
            "phase {phase}"
        );
    }
}

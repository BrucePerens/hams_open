// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::general`'s arithmetic primitives against `f64` std -- kept as an
//! integration test (outside `src/ambe/fixed`) rather than an inline `#[cfg(test)]` module,
//! specifically so no `f64` token ever needs to appear inside `src/ambe/fixed` itself. See
//! `ambe::fixed`'s own doc comment for why that's enforced, not just a style preference.

use ham_digital_modes::ambe::fixed::general::explog::{exp2_q16, log2_q16};
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

fn to_q16(v: f64) -> i32 {
    (v * 65536.0).round() as i32
}

/// `log2_q16` must track `f64::log2` within a small, stated error across many decades of magnitude
/// -- the dequantize chain calls this on amplitude values that can range widely, not just values
/// near 1.0, so the sweep spans from a small fraction to a large multiple of one Q16.16 unit.
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
            assert_eq!(actual_real, 0.0, "y_real={y_real}: expected_real={expected_real} rounds to 0 in Q16.16");
            continue;
        }
        // Likewise, a true result above i32::MAX's own Q16.16 range (~32768.0) genuinely cannot be
        // represented -- saturating to i32::MAX is the correct answer, not a large relative error.
        if expected_real > i32::MAX as f64 / 65536.0 {
            assert_eq!(actual, i32::MAX, "y_real={y_real}: expected_real={expected_real} saturates in Q16.16");
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

#[test]
fn log2_q16_and_exp2_q16_hit_exact_powers_of_two() {
    assert_eq!(log2_q16(65536), 0); // log2(1.0) = 0
    assert_eq!(log2_q16(131072), 65536); // log2(2.0) = 1.0
    assert_eq!(log2_q16(32768), -65536); // log2(0.5) = -1.0
    assert_eq!(exp2_q16(0), 65536); // 2^0 = 1.0
    assert_eq!(exp2_q16(65536), 131072); // 2^1 = 2.0
    assert_eq!(exp2_q16(-65536), 32768); // 2^-1 = 0.5
}

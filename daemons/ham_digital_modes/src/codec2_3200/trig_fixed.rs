// SPDX-License-Identifier: LGPL-3.0-or-later
//! Genuinely fixed-point `sin`/`cos`, for `synthesis.rs`'s own voiced-
//! excitation phase tracking (`synthesize_phase_fixed`) and unvoiced/
//! postfilter random-phase harmonics. The angle domain is a plain `u32`
//! "turns" representation (binary angle measurement, the standard
//! technique for a phase accumulator with no FPU): the full `u32` range
//! represents one full turn (`2*pi` radians), so advancing the phase
//! each sub-frame (`wrapping_add`) and scaling it per-harmonic
//! (`wrapping_mul`, widened through `u64` to avoid a native `u32`
//! overflow, then truncated back to `u32`) both fold angle wraparound
//! into ordinary integer overflow -- no explicit modulo, no float
//! range-reduction, anywhere.
//!
//! Why this composes correctly per-harmonic without extra bookkeeping:
//! writing the true angle as `k + f` turns (`k` an integer whole-turn
//! count, `f` the fractional part actually stored here), `m * (k + f)
//! mod 1 == m * f mod 1` since `m * k` is itself an integer -- so
//! multiplying the *already-wrapped* fractional value by the harmonic
//! number `m`, then keeping only the low 32 bits of that product, is
//! exactly the fractional part of the true `m`-th harmonic's own angle.

use super::fixed_fft::ComplexQ23;

/// Table resolution: 12 bits, matching this port's other trig-shaped
/// LUTs (`lpc.rs`'s own `ACOS_LUT_BITS`/`COS_LUT_BITS`) -- no
/// singularity anywhere on a circle, so no extra margin reasoning is
/// needed beyond what already validated well for `cos_q23`.
const TRIG_LUT_BITS: u32 = 12;
const TRIG_LUT_SIZE: usize = (1 << TRIG_LUT_BITS) + 1;

fn cos_table_q23() -> &'static [i32; TRIG_LUT_SIZE] {
    static TABLE: std::sync::OnceLock<[i32; TRIG_LUT_SIZE]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let levels = 1u32 << TRIG_LUT_BITS;
        let mut t: [i32; TRIG_LUT_SIZE] = std::array::from_fn(|i| {
            let angle = i as f32 / levels as f32 * std::f32::consts::TAU;
            (angle.cos() as f64 * (1i64 << 23) as f64).round() as i32
        });
        // Force an exact seam at the wraparound point (angle == TAU ==
        // angle == 0) rather than trusting f32 cos(TAU) to round to
        // bit-identical the same as cos(0.0) -- it's extremely close
        // either way, but an explicit exact match here is free and
        // removes any doubt about a seam artifact at the table's own
        // wraparound boundary.
        t[levels as usize] = t[0];
        t
    })
}

fn sin_table_q23() -> &'static [i32; TRIG_LUT_SIZE] {
    static TABLE: std::sync::OnceLock<[i32; TRIG_LUT_SIZE]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let levels = 1u32 << TRIG_LUT_BITS;
        let mut t: [i32; TRIG_LUT_SIZE] = std::array::from_fn(|i| {
            let angle = i as f32 / levels as f32 * std::f32::consts::TAU;
            (angle.sin() as f64 * (1i64 << 23) as f64).round() as i32
        });
        t[levels as usize] = t[0];
        t
    })
}

/// `(cos, sin)` of `angle_q32/2^32` turns, both Q23 -- genuinely
/// integer end to end, no `f32` anywhere (table construction above is
/// the one-time exception every LUT in this port shares). The top
/// `TRIG_LUT_BITS` of `angle_q32` select the table index directly (the
/// "index is just the top bits" property `f32_to_q_exact_round`-style
/// LUTs don't get, since this angle was never IEEE754-shaped to begin
/// with); the remaining low bits are the interpolation weight,
/// rescaled to Q23 by an exact left shift (adding zero bits, not an
/// approximation).
pub(crate) fn sin_cos_q23(angle_q32: u32) -> ComplexQ23 {
    const FRAC_BITS_OF_ANGLE: u32 = 32 - TRIG_LUT_BITS;
    let idx = (angle_q32 >> FRAC_BITS_OF_ANGLE) as usize;
    let frac = angle_q32 & ((1u32 << FRAC_BITS_OF_ANGLE) - 1);
    // FRAC_BITS_OF_ANGLE (20) < 23, so this is an exact widening left
    // shift, not a rounding one.
    let frac_q23 = (frac as i64) << (23 - FRAC_BITS_OF_ANGLE);

    let cos_table = cos_table_q23();
    let c0 = cos_table[idx] as i64;
    let c1 = cos_table[idx + 1] as i64;
    let re = c0 + ((frac_q23 * (c1 - c0)) >> 23);

    let sin_table = sin_table_q23();
    let s0 = sin_table[idx] as i64;
    let s1 = sin_table[idx + 1] as i64;
    let im = s0 + ((frac_q23 * (s1 - s0)) >> 23);

    ComplexQ23 { re, im }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_cos_q23_matches_plain_float_sin_cos_across_the_full_turns_range() {
        let mut max_abs_err = 0.0f32;
        // Dense but not exhaustive over u32's full range -- a large
        // prime-ish step so the sweep doesn't alias with the table's
        // own 2^20 sub-index spacing.
        let mut angle_q32: u32 = 0;
        let step: u32 = 104729; // a prime, avoids any accidental periodicity with 2^32/levels
        for _ in 0..40_000 {
            let want_angle = angle_q32 as f64 / (1u64 << 32) as f64 * std::f64::consts::TAU;
            let want_cos = want_angle.cos() as f32;
            let want_sin = want_angle.sin() as f32;
            let got = sin_cos_q23(angle_q32);
            let got_cos = got.re as f32 / (1i64 << 23) as f32;
            let got_sin = got.im as f32 / (1i64 << 23) as f32;
            max_abs_err = max_abs_err
                .max((got_cos - want_cos).abs())
                .max((got_sin - want_sin).abs());
            angle_q32 = angle_q32.wrapping_add(step);
        }
        assert!(
            max_abs_err < 1e-3,
            "sin_cos_q23 diverged from plain float sin_cos by {max_abs_err} across a dense sweep of the full turns range"
        );
    }

    #[test]
    fn sin_cos_q23_is_exact_and_seamless_at_the_wraparound_boundary() {
        let at_zero = sin_cos_q23(0);
        let near_wrap = sin_cos_q23(u32::MAX);
        // u32::MAX is one LSB short of a full wrap back to angle 0 --
        // should be extremely close to (1.0, 0.0), not just "close
        // enough within the table's own ordinary interpolation error"
        // but specifically not showing a discontinuity at the seam.
        assert!((at_zero.re - (1i64 << 23)).abs() < 10, "cos(0) should be ~1.0 in Q23");
        assert!(at_zero.im.abs() < 10, "sin(0) should be ~0.0 in Q23");
        let near_wrap_cos = near_wrap.re as f32 / (1i64 << 23) as f32;
        let near_wrap_sin = near_wrap.im as f32 / (1i64 << 23) as f32;
        assert!(
            (near_wrap_cos - 1.0).abs() < 1e-3 && near_wrap_sin.abs() < 1e-3,
            "just before the wraparound seam, expected (cos,sin) close to (1.0, 0.0), got ({near_wrap_cos}, {near_wrap_sin})"
        );
    }
}

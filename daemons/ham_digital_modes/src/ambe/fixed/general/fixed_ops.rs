// SPDX-License-Identifier: LGPL-3.0-or-later
//! General Q16.16 arithmetic helpers -- multiplication, division, square root, and a couple of
//! constants -- needed throughout the MBE dequantize/synthesis chain (`super::mbe_speech`) on top of
//! the transcendental primitives in [`super::trig`]/[`super::explog`].

use super::isqrt::isqrt_u64;

/// `round(pi * 65536)`. `TIA-102.BABA_2003.pdf`'s own equations and mbelib's real dequantize code
/// both use `pi`/`2*pi` directly as plain scalars (not as a turn/phase), so this is kept as an
/// ordinary Q16.16 constant rather than folded into [`super::trig`]'s phase convention.
pub const PI_Q16_16: i32 = 205887;
/// `round(2 * pi * 65536)` -- computed directly rather than `2 * PI_Q16_16`, so this constant isn't
/// carrying `PI_Q16_16`'s own rounding error doubled.
pub const TWO_PI_Q16_16: i32 = 411775;

/// `a * b` for two Q16.16 values, rounding to nearest rather than truncating (adds half an LSB
/// before the final shift, the same idiom [`super::explog::exp2_q16`] uses).
pub fn mul_q16(a: i32, b: i32) -> i32 {
    let product = (a as i64) * (b as i64);
    let rounded = if product >= 0 { product + (1 << 15) } else { product - (1 << 15) };
    (rounded >> 16) as i32
}

/// `a / b` for two Q16.16 values, returning a Q16.16 result. Saturates to `i32::MAX`/`i32::MIN`
/// (matching the sign of the true result) on division by zero, rather than panicking -- this crate's
/// own established "never panic" convention, extended to a case `f64` would represent as `inf`/`-inf`.
pub fn div_q16(a: i32, b: i32) -> i32 {
    if b == 0 {
        return if a >= 0 { i32::MAX } else { i32::MIN };
    }
    let numerator = (a as i64) << 16;
    let denominator = b as i64;
    let quotient = numerator / denominator;
    quotient.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// `sqrt(x)` for a non-negative Q16.16 `x`, returning a Q16.16 result -- exact to the nearest
/// representable Q16.16 value (built directly on [`isqrt_u64`]'s own exact integer result, not an
/// iterative approximation). Returns `0` for a negative `x` (matching this crate's own "never panic"
/// convention; `f64::sqrt` of a negative number is `NaN`, which has no fixed-point representation).
///
/// Derivation: `sqrt(x_real) = sqrt(x_q16 / 65536) = sqrt(x_q16) / 256`. To get that result *as*
/// Q16.16 (i.e. multiplied back up by 65536): `sqrt(x_q16)/256 * 65536 = sqrt(x_q16) * 256 =
/// sqrt(x_q16 * 256^2) = sqrt(x_q16 * 65536)` -- so a single `isqrt_u64` call on `x_q16 * 65536`
/// gives the Q16.16 result directly.
pub fn sqrt_q16(x: i32) -> i32 {
    if x < 0 {
        return 0;
    }
    isqrt_u64((x as u64) * 65536) as i32
}

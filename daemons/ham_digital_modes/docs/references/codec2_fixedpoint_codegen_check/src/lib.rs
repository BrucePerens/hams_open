// no_std for the real (staticlib) build; the host unit tests at the bottom need std's test harness.
#![cfg_attr(not(test), no_std)]

// Standalone extraction of lpc.rs's two i128-using functions
// (`q_mul` and `div_round_i128`), for cross-compiling to a real
// no-FPU 32-bit target and inspecting the emitted codegen -- see
// ../../CODEC2_FIXED_POINT_WIDTH_REDUCTION_STUDY.md's "Real, measured
// findings" section for what this was used to determine and why.
// Kept byte-for-byte in sync with lpc.rs's own implementations
// (minus the debug_assert!, which pulls in panic formatting
// machinery irrelevant to the question this crate answers) -- if
// lpc.rs's q_mul/div_round_i128 change, update this file to match
// before re-running the check, or the codegen inspected here no
// longer reflects the real functions.

#[cfg(not(test))]
use core::panic::PanicInfo;

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

const LEVINSON_FRAC_BITS: u32 = 40;

// [@ANCHOR: codegen_check_q_mul_i128]
#[no_mangle]
pub fn q_mul_i128(a: i64, b: i64) -> i64 {
    let product = a as i128 * b as i128;
    let half = 1i128 << (LEVINSON_FRAC_BITS - 1);
    let shifted = (product + half) >> LEVINSON_FRAC_BITS;
    shifted as i64
}

// [@ANCHOR: codegen_check_div_round_i128_fn]
#[no_mangle]
pub fn div_round_i128_fn(n: i128, d: i128) -> i64 {
    let half = d / 2;
    (if n >= 0 {
        (n + half) / d
    } else {
        (n - half) / d
    }) as i64
}

// cheb_poly_eval_fixed, both the real i64-storage implementation and the
// i32-storage candidate -- see ../../CODEC2_FIXED_POINT_WIDTH_REDUCTION_
// STUDY.md's Question 2. Kept in sync with lpc.rs's own two functions
// (real one + the i32 candidate proven bit-exact against it there); this
// crate exists to compare their real codegen, not to re-derive
// correctness, which lpc.rs's own test suite already owns.
const CHEB_FRAC_BITS: u32 = 29;

// [@ANCHOR: codegen_check_cheb_i64]
#[no_mangle]
pub fn cheb_poly_eval_fixed_i64(coef: &[i32; 6], x_q: i64) -> i64 {
    let mut t_prev2: i64 = 1i64 << CHEB_FRAC_BITS;
    let mut t_prev1: i64 = x_q;

    let mut sum: i64 = (coef[5] as i64 * t_prev2) >> CHEB_FRAC_BITS;
    sum += (coef[4] as i64 * t_prev1) >> CHEB_FRAC_BITS;

    for i in 2..=5 {
        let t_i = ((2 * x_q * t_prev1) >> CHEB_FRAC_BITS) - t_prev2;
        sum += (coef[5 - i] as i64 * t_i) >> CHEB_FRAC_BITS;
        t_prev2 = t_prev1;
        t_prev1 = t_i;
    }
    sum
}

// [@ANCHOR: codegen_check_cheb_i32]
#[no_mangle]
pub fn cheb_poly_eval_fixed_i32(coef: &[i32; 6], x_q: i32) -> i32 {
    let mut t_prev2: i32 = 1i32 << CHEB_FRAC_BITS;
    let mut t_prev1: i32 = x_q;

    let mut sum: i32 = ((coef[5] as i64 * t_prev2 as i64) >> CHEB_FRAC_BITS) as i32;
    sum += ((coef[4] as i64 * t_prev1 as i64) >> CHEB_FRAC_BITS) as i32;

    for i in 2..=5 {
        let t_i: i32 =
            (((2i64 * x_q as i64 * t_prev1 as i64) >> CHEB_FRAC_BITS) - t_prev2 as i64) as i32;
        sum += ((coef[5 - i] as i64 * t_i as i64) >> CHEB_FRAC_BITS) as i32;
        t_prev2 = t_prev1;
        t_prev1 = t_i;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_Q40: i64 = 1 << LEVINSON_FRAC_BITS;
    const ONE_Q29: i64 = 1 << CHEB_FRAC_BITS;

    // Tests [@ANCHOR: codegen_check_q_mul_i128]
    #[test]
    fn q_mul_is_a_rounded_q40_product() {
        assert_eq!(q_mul_i128(ONE_Q40, ONE_Q40), ONE_Q40); // 1.0 * 1.0
        assert_eq!(q_mul_i128(ONE_Q40 / 2, ONE_Q40 / 2), ONE_Q40 / 4); // 0.5 * 0.5
        assert_eq!(q_mul_i128(-ONE_Q40, ONE_Q40 / 2), -(ONE_Q40 / 2));
        assert_eq!(q_mul_i128(0, 12345), 0);
        // Rounds half up: exactly 0.5 LSB rounds to 1, just under rounds to 0.
        assert_eq!(q_mul_i128(1, ONE_Q40 / 2), 1);
        assert_eq!(q_mul_i128(1, ONE_Q40 / 2 - 1), 0);
        // The i128 intermediate does not overflow for full-range i64 inputs.
        assert_eq!(q_mul_i128(i64::MAX, ONE_Q40), i64::MAX);
    }

    // Tests [@ANCHOR: codegen_check_div_round_i128_fn]
    #[test]
    fn div_round_rounds_half_away_from_zero() {
        assert_eq!(div_round_i128_fn(10, 4), 3); // 2.5 -> 3
        assert_eq!(div_round_i128_fn(-10, 4), -3); // -2.5 -> -3
        assert_eq!(div_round_i128_fn(9, 4), 2); // 2.25 -> 2
        assert_eq!(div_round_i128_fn(-9, 4), -2);
        assert_eq!(div_round_i128_fn(0, 7), 0);
        assert_eq!(div_round_i128_fn(1i128 << 100, 1i128 << 90), 1 << 10);
    }

    fn cheb_float(coef: &[i32; 6], x: f64) -> f64 {
        // Direct Chebyshev recurrence in f64, coef[5] multiplying T0 down to coef[0] on T5.
        let mut t = [1.0, x, 0.0, 0.0, 0.0, 0.0];
        for i in 2..6 {
            t[i] = 2.0 * x * t[i - 1] - t[i - 2];
        }
        (0..6).map(|i| coef[5 - i] as f64 * t[i]).sum()
    }

    // Tests [@ANCHOR: codegen_check_cheb_i64]
    #[test]
    fn cheb_i64_matches_the_float_recurrence() {
        let coef = [1000, -2000, 3000, -4000, 5000, 6000];
        for step in -10..=10 {
            let x = step as f64 / 10.0;
            let got = cheb_poly_eval_fixed_i64(&coef, (x * ONE_Q29 as f64) as i64) as f64;
            let want = cheb_float(&coef, x);
            assert!((got - want).abs() < 16.0, "x={x} got={got} want={want}");
        }
        // Only the T0 term nonzero: the result is coef[5] regardless of x = 0.
        assert_eq!(cheb_poly_eval_fixed_i64(&[0, 0, 0, 0, 0, 7], 0), 7);
    }

    // Tests [@ANCHOR: codegen_check_cheb_i32]
    #[test]
    fn cheb_i32_is_bit_exact_with_i64_in_range() {
        let coefs = [[1000, -2000, 3000, -4000, 5000, 6000], [1, 2, 3, 4, 5, 6], [0; 6]];
        for coef in &coefs {
            for step in -20..=20 {
                let x_q = (step as i64 * ONE_Q29) / 40; // |x| <= 0.5, no i32 overflow
                assert_eq!(
                    cheb_poly_eval_fixed_i32(coef, x_q as i32) as i64,
                    cheb_poly_eval_fixed_i64(coef, x_q),
                    "coef={coef:?} x_q={x_q}"
                );
            }
        }
    }
}

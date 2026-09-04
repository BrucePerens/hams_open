#![no_std]

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

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

const LEVINSON_FRAC_BITS: u32 = 40;

#[no_mangle]
pub fn q_mul_i128(a: i64, b: i64) -> i64 {
    let product = a as i128 * b as i128;
    let half = 1i128 << (LEVINSON_FRAC_BITS - 1);
    let shifted = (product + half) >> LEVINSON_FRAC_BITS;
    shifted as i64
}

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

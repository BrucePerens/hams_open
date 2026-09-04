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

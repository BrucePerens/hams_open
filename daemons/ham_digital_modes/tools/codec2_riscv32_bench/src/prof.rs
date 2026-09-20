pub static mut ACC: [u64; 16] = [0; 16];
static mut LAST: u32 = 0;
#[inline(always)]
pub fn instret() -> u32 {
    let x: u32;
    unsafe { core::arch::asm!("csrr {0}, minstret", out(reg) x) };
    x
}
/// Hook the crate's `profile_mark!` calls (feature `codec2_profile`) resolve to.
#[no_mangle]
pub extern "Rust" fn codec2_profile_mark(id: usize) {
    mark(id)
}
/// Adds instructions retired since the previous mark to bucket `i` (mark(15) just resets the baseline).
#[inline(never)]
pub fn mark(i: usize) {
    let now = instret();
    unsafe {
        if i != 15 { ACC[i] += now.wrapping_sub(LAST) as u64; }
        LAST = instret();
    }
}

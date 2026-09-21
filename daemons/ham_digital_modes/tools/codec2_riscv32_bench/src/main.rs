#![no_std]
#![no_main]
#![allow(dead_code, static_mut_refs, unused_imports, unused_variables)]
mod prof;
use ham_digital_modes::codec2_3200::{DecoderFixed, EncoderFixed, BYTES_PER_FRAME, SAMPLES_PER_FRAME};
use core::fmt::Write;

core::arch::global_asm!(r#"
.section .text.init
.global _start
_start:
    la sp, __stack_top
    la t0, __bss_start
    la t1, __bss_end
1:  bgeu t0, t1, 2f
    sw zero, 0(t0)
    addi t0, t0, 4
    j 1b
2:  call main
3:  j 3b
"#);

struct Uart;
impl Write for Uart {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() { unsafe { core::ptr::write_volatile(0x1000_0000 as *mut u8, b) }; }
        Ok(())
    }
}
fn exit(code: u32) -> ! {
    unsafe { core::ptr::write_volatile(0x10_0000 as *mut u32, if code == 0 { 0x5555 } else { (code << 16) | 0x3333 }) };
    loop {}
}
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Uart, "PANIC: {}", info);
    exit(1)
}



extern "C" {
    static __bss_end: u8;
    static __stack_top: u8;
}
/// Fill the unused part of the stack area with a pattern so the high-water mark can be read back.
fn paint_stack() -> usize {
    unsafe {
        let lo = &__bss_end as *const u8 as usize;
        let sp: usize;
        core::arch::asm!("mv {0}, sp", out(reg) sp);
        let mut p = lo;
        while p + 4 <= sp - 256 {
            core::ptr::write_volatile(p as *mut u32, 0xA5A5_A5A5);
            p += 4;
        }
        sp
    }
}
/// Bytes of stack used below the stack pointer `sp` that `paint_stack` returned, i.e. by the callees
/// only (not by `main`'s own frame, which here holds the 28 KB decoder state being constructed).
fn stack_high_water(sp: usize) -> usize {
    unsafe {
        let lo = &__bss_end as *const u8 as usize;
        let mut p = lo;
        while p + 4 <= sp && core::ptr::read_volatile(p as *const u32) == 0xA5A5_A5A5 {
            p += 4;
        }
        sp - p
    }
}

#[cfg(not(feature = "small"))]
static SPEECH: &[u8] = include_bytes!("../speech.raw");
#[cfg(feature = "small")]
static SPEECH: &[u8] = include_bytes!("../speech_small.raw");
// Encode marks. Marks 2, 5, 9 and 10 sit inside the pitch estimator (`nlp_fixed_bin`, called twice per
// frame): 9 = notch filter, 10 = decimation filter, 2 = history shift and Hann window, 5 = the 512-point
// transform, 0 = power spectrum, peak search and sub-multiple check. Mark 1 = voicing, 3 = window and
// autocorrelation, 4 = white-noise correction and Levinson-Durbin, 6 = line spectral pair search
// (with bandwidth expansion), 7 = quantisers, 8 = packing. (The former "encode_wo" and "lpc_energy"
// marks, a few hundred instructions each, now read as part of neighbours.)
const E_NAMES: [&str; 11] = ["nlp: power+peak", "voicing(x2)", "nlp: shift+window", "window+autocorr", "white+levinson", "nlp: 512-pt transform", "bw+lpc_to_lsp", "quantise", "pack", "nlp: notch filter", "nlp: decimation"];
const D_NAMES: [&str; 5] = ["unpack+dequant+interp", "lsp_to_lpc(x2)", "harmonic_amps(x2)", "first_harm(x2)", "synth(x2)"];

// Kept out of line so the stack high-water mark below includes the codec's whole call tree (with
// link-time inlining the frames would otherwise be folded into `main`'s own frame and not measured).
#[inline(never)]
fn do_encode(enc: &mut EncoderFixed, fr: &[i16; SAMPLES_PER_FRAME]) -> [u8; BYTES_PER_FRAME] {
    enc.encode(fr)
}
#[inline(never)]
fn do_decode(dec: &mut DecoderFixed, fr: &[u8; BYTES_PER_FRAME]) -> [i16; SAMPLES_PER_FRAME] {
    dec.decode(fr)
}

#[cfg(feature = "decode16k")]
#[inline(never)]
fn do_decode_16k(dec: &mut DecoderFixed, fr: &[u8; BYTES_PER_FRAME]) -> [i16; 2 * SAMPLES_PER_FRAME] {
    dec.decode_16k_fixed(fr)
}

#[no_mangle]
pub extern "C" fn main() -> ! {
    let mut u = Uart;
    let n = SPEECH.len() / 2 / SAMPLES_PER_FRAME;
    let _ = writeln!(u, "sizeof EncoderFixed {} bytes, DecoderFixed {} bytes", core::mem::size_of::<EncoderFixed>(), core::mem::size_of::<DecoderFixed>());
    let sp0 = paint_stack();
    let mut enc = EncoderFixed::new();
    let mut dec = DecoderFixed::new();
    let mut frames = [[0u8; BYTES_PER_FRAME]; 200];
    let mut cksum: u32 = 0;
    // pass 1: encode
    let mut first = 0u32;
    let mut enc_tot = 0u64; let mut enc_max = 0u32;
    for f in 0..n {
        let mut fr = [0i16; SAMPLES_PER_FRAME];
        for i in 0..SAMPLES_PER_FRAME {
            let o = (f * SAMPLES_PER_FRAME + i) * 2;
            fr[i] = i16::from_le_bytes([SPEECH[o], SPEECH[o + 1]]);
        }
        let t0 = prof::instret();
        frames[f] = do_encode(&mut enc, &fr);
        let d = prof::instret().wrapping_sub(t0);
        if f == 0 { first = d; unsafe { prof::ACC = [0; 16]; } } else { enc_tot += d as u64; enc_max = enc_max.max(d); }
        for &b in &frames[f] { cksum = cksum.wrapping_mul(16777619).wrapping_add(b as u32); }
    }
    let _ = writeln!(u, "frames={} ", n);
    let enc_stack = stack_high_water(sp0);
    let _ = writeln!(u, "peak stack during encode {} bytes", enc_stack);
    let sp1 = paint_stack();
    let _ = writeln!(u, "ENCODE first-frame(incl table init) {} instr; steady avg {} max {} instr/frame", first, enc_tot / (n as u64 - 1), enc_max);
    for i in 0..E_NAMES.len() { let _ = writeln!(u, "  enc {:<18} {:>9}", E_NAMES[i], unsafe { prof::ACC[i] } / (n as u64 - 1)); }
    unsafe { prof::ACC = [0; 16]; }
    let mut dfirst = 0u32; let mut dec_tot = 0u64; let mut dec_max = 0u32;
    for f in 0..n {
        let t0 = prof::instret();
        let out = do_decode(&mut dec, &frames[f]);
        let d = prof::instret().wrapping_sub(t0);
        if f == 0 { dfirst = d; unsafe { prof::ACC = [0; 16]; } } else { dec_tot += d as u64; dec_max = dec_max.max(d); }
        for &s in &out { cksum = cksum.wrapping_mul(16777619).wrapping_add(s as u16 as u32); }
    }
    let _ = writeln!(u, "peak stack during decode {} bytes", stack_high_water(sp1));
    let _ = writeln!(u, "DECODE first-frame {} instr; steady avg {} max {} instr/frame", dfirst, dec_tot / (n as u64 - 1), dec_max);
    for i in 0..D_NAMES.len() { let _ = writeln!(u, "  dec {:<22} {:>9}", D_NAMES[i], unsafe { prof::ACC[i] } / (n as u64 - 1)); }
    let names = ["fft1", "a2", "fft2+a2g", "loop1(log/exp)", "harm loop", "synth:h+phase", "synth:postfilter", "synth:fill+sym(13)", "synth:ifft(14)"];
    for i in 6..15 { let _ = writeln!(u, "  fine[{}] {:>9}", i, unsafe { prof::ACC[i] } / (n as u64 - 1)); }
    let _ = writeln!(u, "checksum {:08x}", cksum);
    #[cfg(feature = "decode16k")]
    {
        // 16 kHz spectral-bridge decode of the same bitstream, on a fresh decoder (the 8 kHz and 16 kHz
        // entry points share inter-frame state, so they are never interleaved on one instance).
        let mut dec16 = DecoderFixed::new();
        let mut ck16: u32 = 0;
        unsafe { prof::ACC = [0; 16]; }
        let (mut first16, mut tot16, mut max16) = (0u32, 0u64, 0u32);
        for f in 0..n {
            let t0 = prof::instret();
            let out = do_decode_16k(&mut dec16, &frames[f]);
            let d = prof::instret().wrapping_sub(t0);
            if f == 0 { first16 = d; unsafe { prof::ACC = [0; 16]; } } else { tot16 += d as u64; max16 = max16.max(d); }
            for &s in &out { ck16 = ck16.wrapping_mul(16777619).wrapping_add(s as u16 as u32); }
        }
        let _ = writeln!(u, "DECODE16K first-frame {} instr; steady avg {} max {} instr/frame", first16, tot16 / (n as u64 - 1), max16);
        for i in 6..15 { let _ = writeln!(u, "  fine16[{}] {:>9}", i, unsafe { prof::ACC[i] } / (n as u64 - 1)); }
        let _ = writeln!(u, "checksum16k {:08x}", ck16);
    }
    #[cfg(feature = "mode1600")]
    {
        use ham_digital_modes::codec2_1600 as c;
        // Codec2 1600 (40 ms frames) over the same speech excerpt, own checksum.
        let m = SPEECH.len() / 2 / c::SAMPLES_PER_FRAME;
        let mut e = c::EncoderFixed::new();
        let mut d = c::DecoderFixed::new();
        let mut fr8 = [[0u8; c::BYTES_PER_FRAME]; 100];
        let mut ck: u32 = 0;
        let (mut et, mut emax, mut dt, mut dmax) = (0u64, 0u32, 0u64, 0u32);
        for f in 0..m {
            let mut fr = [0i16; c::SAMPLES_PER_FRAME];
            for i in 0..c::SAMPLES_PER_FRAME {
                let o = (f * c::SAMPLES_PER_FRAME + i) * 2;
                fr[i] = i16::from_le_bytes([SPEECH[o], SPEECH[o + 1]]);
            }
            let t0 = prof::instret();
            fr8[f] = do_encode_1600(&mut e, &fr);
            let x = prof::instret().wrapping_sub(t0);
            if f > 0 { et += x as u64; emax = emax.max(x); }
            for &b in &fr8[f] { ck = ck.wrapping_mul(16777619).wrapping_add(b as u32); }
        }
        for f in 0..m {
            let t0 = prof::instret();
            let out = do_decode_1600(&mut d, &fr8[f]);
            let x = prof::instret().wrapping_sub(t0);
            if f > 0 { dt += x as u64; dmax = dmax.max(x); }
            for &s in &out { ck = ck.wrapping_mul(16777619).wrapping_add(s as u16 as u32); }
        }
        let _ = writeln!(u, "MODE1600 frames={} encode steady avg {} max {} instr/40ms frame; decode steady avg {} max {}", m, et / (m as u64 - 1), emax, dt / (m as u64 - 1), dmax);
        let _ = writeln!(u, "checksum1600 {:08x}", ck);
    }
    exit(0)
}

#[cfg(feature = "mode1600")]
#[inline(never)]
fn do_encode_1600(e: &mut ham_digital_modes::codec2_1600::EncoderFixed, fr: &[i16; 320]) -> [u8; 8] {
    e.encode(fr)
}
#[cfg(feature = "mode1600")]
#[inline(never)]
fn do_decode_1600(d: &mut ham_digital_modes::codec2_1600::DecoderFixed, b: &[u8; 8]) -> [i16; 320] {
    d.decode(b)
}

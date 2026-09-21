// SPDX-License-Identifier: LGPL-3.0-or-later
//! Host timing of the fixed-point Codec2 3200 path, per 20 ms frame.
//! Usage: `cargo run --release --example codec2_fixed_bench -- speech.wav [passes]`
//! Prints a checksum of all produced bytes/samples so before/after runs can be compared.
use ham_digital_modes::codec2_3200::{DecoderFixed, EncoderFixed, SAMPLES_PER_FRAME};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).expect("wav");
    let passes: usize = args.get(2).map_or(5, |s| s.parse().unwrap());
    let samples: Vec<i16> = data[44..]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    let n = samples.len() / SAMPLES_PER_FRAME;
    let mut best_enc = f64::MAX;
    let mut best_dec = f64::MAX;
    let mut sum = 0u64;
    for _ in 0..passes {
        let mut enc = EncoderFixed::new();
        let mut dec = DecoderFixed::new();
        let mut frames = Vec::with_capacity(n);
        sum = 0;
        let t = Instant::now();
        for f in 0..n {
            let fr: [i16; SAMPLES_PER_FRAME] = samples[f * SAMPLES_PER_FRAME..(f + 1) * SAMPLES_PER_FRAME]
                .try_into()
                .unwrap();
            frames.push(enc.encode(&fr));
        }
        best_enc = best_enc.min(t.elapsed().as_secs_f64() / n as f64);
        let t = Instant::now();
        let mut pcm = Vec::with_capacity(n);
        for b in &frames {
            pcm.push(dec.decode(b));
        }
        best_dec = best_dec.min(t.elapsed().as_secs_f64() / n as f64);
        for b in &frames {
            for &x in b {
                sum = sum.wrapping_mul(1099511628211).wrapping_add(x as u64);
            }
        }
        for p in &pcm {
            for &x in p {
                sum = sum.wrapping_mul(1099511628211).wrapping_add(x as u16 as u64);
            }
        }
    }
    // Cross-check value for the 32-bit RISC-V build (same 32-bit FNV-style hash the target
    // program prints, over the frames selected by the optional `skip_frames count` arguments).
    if let (Some(skip), Some(count)) = (args.get(3), args.get(4)) {
        let (skip, count): (usize, usize) = (skip.parse().unwrap(), count.parse().unwrap());
        let mut enc = EncoderFixed::new();
        let mut dec = DecoderFixed::new();
        let mut h = 0u32;
        let mut frames = Vec::new();
        for f in skip..skip + count {
            let fr: [i16; SAMPLES_PER_FRAME] = samples[f * SAMPLES_PER_FRAME..(f + 1) * SAMPLES_PER_FRAME]
                .try_into()
                .unwrap();
            let b = enc.encode(&fr);
            for &x in &b {
                h = h.wrapping_mul(16777619).wrapping_add(x as u32);
            }
            frames.push(b);
        }
        for b in &frames {
            for &x in &dec.decode(b) {
                h = h.wrapping_mul(16777619).wrapping_add(x as u16 as u32);
            }
        }
        println!("cross-check hash (frames {skip}..{}): {h:08x}", skip + count);
    }
    // Codec2 1600 (40 ms frames) over the same excerpt, for the 32-bit build's `mode1600` feature:
    // `... <wav> 1 100 200 1600` prints the same 32-bit hash the target program prints.
    if let (Some(skip), Some(count), Some("1600")) = (args.get(3), args.get(4), args.get(5).map(String::as_str)) {
        use ham_digital_modes::codec2_1600 as c;
        let (skip, count): (usize, usize) = (skip.parse().unwrap(), count.parse().unwrap());
        let start = skip * SAMPLES_PER_FRAME;
        let frames_1600 = count * SAMPLES_PER_FRAME / c::SAMPLES_PER_FRAME;
        let mut enc = c::EncoderFixed::new();
        let mut dec = c::DecoderFixed::new();
        let mut h = 0u32;
        let mut frames = Vec::new();
        for f in 0..frames_1600 {
            let o = start + f * c::SAMPLES_PER_FRAME;
            let fr: [i16; c::SAMPLES_PER_FRAME] = samples[o..o + c::SAMPLES_PER_FRAME].try_into().unwrap();
            let b = enc.encode(&fr);
            for &x in &b {
                h = h.wrapping_mul(16777619).wrapping_add(x as u32);
            }
            frames.push(b);
        }
        for b in &frames {
            for &x in &dec.decode(b) {
                h = h.wrapping_mul(16777619).wrapping_add(x as u16 as u32);
            }
        }
        println!("cross-check hash 1600 ({frames_1600} frames): {h:08x}");
    }
    println!("frames={n} encode {:.1} us/frame, decode {:.1} us/frame, checksum {sum:016x}", best_enc * 1e6, best_dec * 1e6);
}

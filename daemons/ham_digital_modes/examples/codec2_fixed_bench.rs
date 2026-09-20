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
    println!("frames={n} encode {:.1} us/frame, decode {:.1} us/frame, checksum {sum:016x}", best_enc * 1e6, best_dec * 1e6);
}

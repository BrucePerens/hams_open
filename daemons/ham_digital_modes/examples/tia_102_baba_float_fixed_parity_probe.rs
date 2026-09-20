// SPDX-License-Identifier: LGPL-3.0-or-later
//! Diagnostic for the float/fixed TIA-102.BABA encoder parity tests: encodes a WAV window with both encoders and
//! lists the frames whose code vectors differ, with each frame's RMS input level, so parity loss can be attributed
//! (quiet noise-floor frames versus speech).
//!
//! Usage: `cargo run --release --example tia_102_baba_float_fixed_parity_probe -- <wav> <first_frame> <frames>`

use ham_digital_modes::ambe::fixed::tia_102_baba::encoder::Encoder as FixedEncoder;
use ham_digital_modes::ambe::float::tia_102_baba::encoder::Encoder as FloatEncoder;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).expect("wav");
    let pcm: Vec<i16> = data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    let first: usize = args[2].parse().unwrap();
    let frames: usize = args[3].parse().unwrap();
    let window = &pcm[first * 160..((first + frames) * 160).min(pcm.len())];

    let mut fx = FixedEncoder::new();
    let mut fl = FloatEncoder::new();
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for chunk in window.chunks(97) {
        fx.push_samples(chunk);
        while let Some(f) = fx.next_frame() {
            a.push(f);
        }
        let c: Vec<f64> = chunk.iter().map(|&s| s as f64).collect();
        fl.push_samples(&c);
        while let Some(f) = fl.next_frame() {
            b.push(f);
        }
    }
    a.extend(fx.finish());
    b.extend(fl.finish());
    let mut differing = 0;
    for (k, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x != y {
            differing += 1;
            // Frame k is analysed around input sample 160 k (two frames of look-ahead delay in the output order).
            let lo = (160 * k).min(window.len());
            let hi = (lo + 160).min(window.len());
            let seg = &window[lo..hi];
            let rms = (seg.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / seg.len().max(1) as f64).sqrt();
            let bits: Vec<usize> = (0..8).filter(|&i| x[i] != y[i]).collect();
            println!("frame {k:4} rms {rms:8.1} differing vectors {bits:?}");
        }
    }
    println!("{} of {} frames differ", differing, a.len().min(b.len()));
}

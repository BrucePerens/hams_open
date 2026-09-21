// SPDX-License-Identifier: LGPL-3.0-or-later
//! Offline (no chip access): fits the real chip's D-STAR pitch-index-to-fundamental-frequency mapping from
//! previously captured data. Reads `<dir>/dstar_channel_payloads.hex` (one hex CHANNEL payload per line, as
//! written by `ambe_chip_pcm_vs_float_synthesis_half_rate`) and `<dir>/dstar_chip_decoded.wav`, decodes each
//! frame's `b0`, and for stable-pitch frames estimates the pitch actually present in the chip's decoded PCM
//! (a harmonic-comb search on a Hann-windowed 3-frame span, around this crate's own `f0_from_b0` guess).
//! Prints the median chip/guess ratio, and a least-squares line `log2(f_chip) = a + c*b0`.
//!
//! **Result (33 stable-pitch loud frames, b0 31-55, OSR_us_000_0010_8k.wav)**: chip/guess ratio p10 1.021,
//! median 1.030, p90 1.036, essentially independent of b0 (slope 0.00007/index). Control on this crate's own
//! float PCM (pitch exactly the guess): median 1.000, p10 0.987, p90 1.007 -- the estimator is unbiased, so the
//! chip's D-STAR pitch really is ~3% above mbelib's guessed formula. mbelib's alternative `AmbeW0table` sits
//! *below* the guess (ratio 0.985 at b0=31), so it is not the answer either.
//!
//! Usage: `cargo run --release --example dstar_fit_pitch_table -- [dir=/tmp] [wav_name=dstar_chip_decoded.wav]`
//! (run once with `dstar_float_decoded.wav` as a control: the float PCM's pitch is exactly the guess, so a ratio
//! near 1.000 there shows the estimator itself is unbiased.)

use ham_digital_modes::ambe::float::dstar::decode::{extract_raw_parameters, f0_from_b0, parse_frame};
use ham_digital_modes::ambe::float::dstar::interleave::wire_bytes_to_frame;
use rustfft::{num_complex::Complex64, FftPlanner};

const FFT_LEN: usize = 8192;

fn read_wav(path: &str) -> Vec<f64> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f64).collect()
}

fn spectrum(seg: &[f64]) -> Vec<f64> {
    let n = seg.len();
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(FFT_LEN);
    let mut buf: Vec<Complex64> = seg
        .iter()
        .enumerate()
        .map(|(i, &s)| Complex64::new(s * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n as f64 - 1.0)).cos()), 0.0))
        .collect();
    buf.resize(FFT_LEN, Complex64::new(0.0, 0.0));
    fft.process(&mut buf);
    buf[..FFT_LEN / 2].iter().map(|c| c.norm()).collect()
}

fn comb_score(mag: &[f64], f0: f64) -> f64 {
    let bin = |hz: f64| (hz * FFT_LEN as f64 / 8000.0).round() as usize;
    let mut score = 0.0;
    let mut k = 1;
    while (k as f64 * f0) < 2600.0 {
        let b = bin(k as f64 * f0);
        let peak = (b.saturating_sub(1)..=(b + 1).min(mag.len() - 1)).map(|i| mag[i]).fold(0.0, f64::max);
        score += (1.0 + peak).ln();
        k += 1;
    }
    score
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());
    let hex = std::fs::read_to_string(format!("{dir}/dstar_channel_payloads.hex")).unwrap();
    let wav_name = std::env::args().nth(2).unwrap_or_else(|| "dstar_chip_decoded.wav".to_string());
    let chip = read_wav(&format!("{dir}/{wav_name}"));
    let b0s: Vec<u32> = hex
        .lines()
        .map(|l| {
            let bytes: Vec<u8> = (0..l.len() / 2).map(|i| u8::from_str_radix(&l[2 * i..2 * i + 2], 16).unwrap()).collect();
            let mut wire = [0u8; 9];
            wire.copy_from_slice(&bytes[bytes.len() - 9..]);
            extract_raw_parameters(parse_frame(wire_bytes_to_frame(&wire)).d).b0
        })
        .collect();
    let b0_dump: String = b0s.iter().map(|b| format!("{b}\n")).collect();
    std::fs::write(format!("{dir}/dstar_b0.txt"), b0_dump).unwrap();
    let mut points: Vec<(u32, f64)> = Vec::new(); // (b0, chip/guess ratio)
    for i in 1..b0s.len() - 1 {
        let (a, b, c) = (b0s[i - 1] as i32, b0s[i] as i32, b0s[i + 1] as i32);
        if b0s[i] >= 120 || (a - b).abs() > 1 || (c - b).abs() > 1 {
            continue;
        }
        let seg = &chip[(i - 1) * 160..(i + 2) * 160];
        let rms = (seg.iter().map(|s| s * s).sum::<f64>() / seg.len() as f64).sqrt();
        if rms < 400.0 {
            continue;
        }
        let mag = spectrum(seg);
        let guess = f0_from_b0(b0s[i]) * 8000.0;
        let mut best = (f64::NEG_INFINITY, guess);
        let mut r = 0.90;
        while r <= 1.15 {
            let s = comb_score(&mag, guess * r);
            if s > best.0 {
                best = (s, guess * r);
            }
            r += 0.001;
        }
        // contrast: best comb score vs the average over the scan range, as a crude voicing check
        let mut sum = 0.0;
        let mut n = 0.0;
        let mut r = 0.90;
        while r <= 1.15 {
            sum += comb_score(&mag, guess * r);
            n += 1.0;
            r += 0.005;
        }
        if best.0 < 1.15 * sum / n {
            continue;
        }
        points.push((b0s[i], best.1 / guess));
    }
    println!("{} usable frames of {}", points.len(), b0s.len());
    let mut ratios: Vec<f64> = points.iter().map(|p| p.1).collect();
    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| ratios[((ratios.len() - 1) as f64 * p) as usize];
    println!("chip/guess ratio: p10 {:.4} median {:.4} p90 {:.4}", q(0.1), q(0.5), q(0.9));
    let n = points.len() as f64;
    let (sx, sy) = (points.iter().map(|p| p.0 as f64).sum::<f64>(), points.iter().map(|p| p.1.log2()).sum::<f64>());
    let sxx: f64 = points.iter().map(|p| (p.0 as f64).powi(2)).sum();
    let sxy: f64 = points.iter().map(|p| p.0 as f64 * p.1.log2()).sum();
    let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
    let intercept = (sy - slope * sx) / n;
    println!("log2(chip/guess) = {intercept:.5} + {slope:.6}*b0  (guess is 2^(-4.311767578125 - 0.021336*(b0+0.5)))");
    for (b0, r) in points.iter().take(40) {
        println!("  b0={b0:3} ratio={r:.4}");
    }
}

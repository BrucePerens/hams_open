// SPDX-License-Identifier: LGPL-3.0-or-later
//! Objective round-trip quality of every codec configuration (encode real speech, decode, compare with the input):
//! float and fixed-point D-STAR, AMBE+2 half-rate and TIA-102.BABA. No chip needed. For each configuration and speaker it
//! reports the log frame-energy correlation (after the best alignment, which absorbs the codec's delay), the mean
//! log-spectral distance over active frames (dB, 100-3800 Hz), and the mean level offset (output minus input, dB).
//!
//! `cargo run --release --features ambe_plus_2 --example ambe_roundtrip_quality_report [frames-per-speaker]`

use ham_digital_modes::ambe::{fixed, float};
use rustfft::{num_complex::Complex64, FftPlanner};

const FILES: [&str; 4] = [
    "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0011_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0030_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0031_8k.wav",
];

fn read_wav(path: &str) -> Vec<i16> {
    let bytes = std::fs::read(path).expect("wav");
    bytes[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

fn frame_db(x: &[f64]) -> f64 {
    10.0 * (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64 + 1e-3).log10()
}

fn spectrum_db(x: &[f64]) -> Vec<f64> {
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(256);
    let mut b: Vec<Complex64> = (0..256)
        .map(|i| Complex64::new(if i < x.len() { x[i] * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / 255.0).cos()) } else { 0.0 }, 0.0))
        .collect();
    fft.process(&mut b);
    (3..122).map(|k| 10.0 * (b[k].norm_sqr() + 1e-3).log10()).collect()
}

/// (log-energy correlation, log-spectral distance dB, level offset dB) after searching lags of up to +-400 samples.
fn metrics(input: &[f64], output: &[f64]) -> (f64, f64, f64) {
    let mut best = (-2.0, 0i32);
    for lag in (-400..=400).step_by(20) {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in (0..input.len().saturating_sub(160)).step_by(80) {
            let j = i as i32 + lag;
            if j < 0 || j as usize + 160 > output.len() {
                continue;
            }
            a.push(frame_db(&input[i..i + 160]));
            b.push(frame_db(&output[j as usize..j as usize + 160]));
        }
        if a.len() < 20 {
            continue;
        }
        let (ma, mb) = (a.iter().sum::<f64>() / a.len() as f64, b.iter().sum::<f64>() / b.len() as f64);
        let (mut c, mut va, mut vb) = (0.0, 0.0, 0.0);
        for (x, y) in a.iter().zip(&b) {
            c += (x - ma) * (y - mb);
            va += (x - ma).powi(2);
            vb += (y - mb).powi(2);
        }
        let r = c / (va * vb).sqrt().max(1e-12);
        if r > best.0 {
            best = (r, lag);
        }
    }
    let lag = best.1;
    let (mut lsd, mut off, mut n) = (0.0, 0.0, 0usize);
    for i in (0..input.len().saturating_sub(256)).step_by(160) {
        let j = i as i32 + lag;
        if j < 0 || j as usize + 256 > output.len() || frame_db(&input[i..i + 160]) < 30.0 {
            continue;
        }
        let (si, so) = (spectrum_db(&input[i..i + 256]), spectrum_db(&output[j as usize..j as usize + 256]));
        lsd += (si.iter().zip(&so).map(|(x, y)| (x - y).powi(2)).sum::<f64>() / si.len() as f64).sqrt();
        off += frame_db(&output[j as usize..j as usize + 160]) - frame_db(&input[i..i + 160]);
        n += 1;
    }
    (best.0, lsd / n.max(1) as f64, off / n.max(1) as f64)
}

fn q16(v: &[i64]) -> Vec<f64> {
    v.iter().map(|&x| x as f64 / 65536.0).collect()
}

type Codec = (&'static str, fn(&[i16]) -> Vec<f64>);

fn dstar_float(pcm: &[i16]) -> Vec<f64> {
    let mut e = float::dstar::encoder::Encoder::new();
    e.push_samples(&pcm.iter().map(|&s| s as f64).collect::<Vec<_>>());
    let mut frames = Vec::new();
    while let Some(f) = e.next_frame() {
        frames.push(f);
    }
    frames.extend(e.finish());
    let mut d = float::dstar::synthesis::DStarSynthesisDecoder::new();
    frames.iter().flat_map(|&f| d.decode_frame(f).unwrap_or([0.0; 160])).collect()
}
fn dstar_fixed(pcm: &[i16]) -> Vec<f64> {
    let mut e = fixed::dstar::encoder::Encoder::new();
    e.push_samples(pcm);
    let mut frames = Vec::new();
    while let Some(f) = e.next_frame() {
        frames.push(f);
    }
    frames.extend(e.finish());
    let mut d = fixed::dstar::synthesis::DStarSynthesisDecoder::new();
    frames.iter().flat_map(|&f| q16(&d.decode_frame(f).unwrap_or([0; 160]))).collect()
}
#[cfg(feature = "ambe_plus_2")]
fn a2_float(pcm: &[i16]) -> Vec<f64> {
    let mut e = float::ambe_plus_2::encoder::Encoder::new();
    e.push_samples(&pcm.iter().map(|&s| s as f64).collect::<Vec<_>>());
    let mut frames = Vec::new();
    while let Some(f) = e.next_frame() {
        frames.push(f);
    }
    frames.extend(e.finish());
    let mut d = float::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder::new();
    frames.iter().flat_map(|&f| d.decode_frame(f).unwrap_or([0.0; 160])).collect()
}
#[cfg(feature = "ambe_plus_2")]
fn a2_fixed(pcm: &[i16]) -> Vec<f64> {
    let mut e = fixed::ambe_plus_2::encoder::Encoder::new();
    e.push_samples(pcm);
    let mut frames = Vec::new();
    while let Some(f) = e.next_frame() {
        frames.push(f);
    }
    frames.extend(e.finish());
    let mut d = fixed::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder::new();
    frames.iter().flat_map(|&f| q16(&d.decode_frame(f).unwrap_or([0; 160]))).collect()
}
fn tia_float(pcm: &[i16]) -> Vec<f64> {
    let mut e = float::tia_102_baba::encoder::Encoder::new();
    e.push_samples(&pcm.iter().map(|&s| s as f64).collect::<Vec<_>>());
    let mut frames = Vec::new();
    while let Some(f) = e.next_frame() {
        frames.push(f);
    }
    frames.extend(e.finish());
    let mut d = float::tia_102_baba::decode::DecoderState::new();
    frames.iter().flat_map(|&c| d.decode_frame(c).unwrap_or([0.0; 160])).collect()
}
fn tia_fixed(pcm: &[i16]) -> Vec<f64> {
    let mut e = fixed::tia_102_baba::encoder::Encoder::new();
    e.push_samples(pcm);
    let mut frames = Vec::new();
    while let Some(f) = e.next_frame() {
        frames.push(f);
    }
    frames.extend(e.finish());
    let mut d = fixed::tia_102_baba::decode::DecoderState::new();
    frames.iter().flat_map(|&c| q16(&d.decode_frame(c).unwrap_or([0; 160]))).collect()
}

fn main() {
    let frames: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(600);
    #[allow(unused_mut)]
    let mut codecs: Vec<Codec> = vec![("D-STAR float", dstar_float), ("D-STAR fixed", dstar_fixed), ("TIA-102.BABA float", tia_float), ("TIA-102.BABA fixed", tia_fixed)];
    #[cfg(feature = "ambe_plus_2")]
    codecs.extend([("AMBE+2 float", a2_float as fn(&[i16]) -> Vec<f64>), ("AMBE+2 fixed", a2_fixed)]);
    println!("| codec | speaker | energy corr | spectral distance dB | level offset dB |\n|---|---|---|---|---|");
    for (name, f) in &codecs {
        let mut sums = (0.0, 0.0, 0.0);
        for (k, file) in FILES.iter().enumerate() {
            let pcm = read_wav(file);
            let pcm = &pcm[..(frames * 160).min(pcm.len())];
            let input: Vec<f64> = pcm.iter().map(|&s| s as f64).collect();
            let out = f(pcm);
            let (c, l, o) = metrics(&input, &out);
            println!("| {name} | {} | {c:.4} | {l:.2} | {o:+.2} |", ["0010", "0011", "0030", "0031"][k]);
            sums = (sums.0 + c, sums.1 + l, sums.2 + o);
        }
        println!("| **{name}** | mean | {:.4} | {:.2} | {:+.2} |", sums.0 / 4.0, sums.1 / 4.0, sums.2 / 4.0);
    }
}

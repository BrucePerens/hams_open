// SPDX-License-Identifier: LGPL-3.0-or-later
//! Shared helpers for the fixed-vs-float D-STAR / AMBE+2 synthesis tests.
#![allow(dead_code)]

pub const N: usize = 160;

pub fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[8..12], b"WAVE", "{path}: not a RIFF/WAVE file");
    assert_eq!(&data[36..40], b"data", "{path}: not a standard 44-byte-header PCM WAV");
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

/// `frames` frames' worth (plus encoder lookahead) of the fixture speech as f64 samples.
pub fn speech_samples(frames: usize) -> Vec<f64> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav");
    let wav = read_wav_mono_i16(path);
    wav.iter().take(frames * N + 800).map(|&s| s as f64).collect()
}

pub fn from_q16_i64(v: &[i64; N]) -> Vec<f64> {
    v.iter().map(|&s| s as f64 / 65536.0).collect()
}

pub fn snr_db(reference: &[f64], test: &[f64]) -> f64 {
    let signal: f64 = reference.iter().map(|&s| s * s).sum();
    let noise: f64 = reference.iter().zip(test).map(|(&f, &x)| (f - x) * (f - x)).sum();
    if noise <= 1e-12 {
        return f64::INFINITY;
    }
    10.0 * (signal / noise).log10()
}

pub fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|&s| s * s).sum::<f64>() / x.len() as f64).sqrt()
}

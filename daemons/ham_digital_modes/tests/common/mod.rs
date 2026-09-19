// SPDX-License-Identifier: LGPL-3.0-or-later
//! Shared helpers for the fixed-point-versus-float MBE analysis comparison tests.
#![allow(dead_code)]

pub const OSR_FILES: [&str; 4] = [
    "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0011_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0030_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0031_8k.wav",
];

/// Frame stride of the codec (20 ms at 8 kHz).
pub const FRAME_SAMPLES: usize = 160;
/// Leading margin before the first analysis centre (the window and its margins read +-160..200).
pub const LEAD: usize = 200;

pub fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let full = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), path);
    let data = std::fs::read(&full).unwrap_or_else(|e| panic!("{full}: {e}"));
    assert_eq!(&data[8..12], b"WAVE", "{full}: not a RIFF/WAVE file");
    assert_eq!(&data[36..40], b"data", "{full}: not a standard 44-byte-header PCM WAV");
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

/// Analysis centres `k*160 + 200` for every frame whose two look-ahead frames still fit with
/// `margin` samples to spare.
pub fn frame_centers(len: usize, margin: usize) -> Vec<usize> {
    let mut v = Vec::new();
    let mut k = 0;
    while LEAD + (k + 2) * FRAME_SAMPLES + margin < len {
        v.push(LEAD + k * FRAME_SAMPLES);
        k += 1;
    }
    v
}

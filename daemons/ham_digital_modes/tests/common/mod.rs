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

/// How many frames of each speech fixture the encoder parity tests use.
pub const PARITY_FRAMES: usize = 150;

/// [`PARITY_FRAMES`], or the `PARITY_MAX_FRAMES` environment variable when set (for wider one-off measurements).
pub fn parity_frames() -> usize {
    std::env::var("PARITY_MAX_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(PARITY_FRAMES)
}

/// What the parity comparison needs to know about a logical frame.
#[derive(Clone, Copy, Debug)]
pub struct FrameView {
    pub tone: bool,
    pub b0: u32,
    pub b1: u32,
}

#[derive(Default, Debug)]
pub struct Parity {
    pub frames: usize,
    pub count_mismatch: usize,
    pub tone_frames_fixed: usize,
    pub tone_frames_float: usize,
    pub identical: usize,
    pub b0_equal: usize,
    pub b0_within_1: usize,
    pub b1_equal: usize,
    /// Signal and error energy of `decode(fixed frames) - decode(float frames)` (PCM), summed over the files.
    pub signal_energy: f64,
    pub error_energy: f64,
    /// Pearson correlations of the per-frame dB RMS of the two decoded streams, one per file.
    pub envelope_corr: Vec<f64>,
}

impl Parity {
    pub fn frac(&self, n: usize) -> f64 {
        n as f64 / self.frames.max(1) as f64
    }
    pub fn snr_db(&self) -> f64 {
        10.0 * (self.signal_energy / self.error_energy.max(1e-9)).log10()
    }
    pub fn min_envelope_corr(&self) -> f64 {
        self.envelope_corr.iter().cloned().fold(f64::INFINITY, f64::min)
    }
    pub fn report(&self, name: &str) {
        println!(
            "{name}: frames {} count_mismatch {} identical {:.2}% b0 equal {:.2}% b0 within 1 {:.2}% b1 equal {:.2}% tone fixed/float {}/{} decoded SNR {:.2} dB envelope corr {:?}",
            self.frames,
            self.count_mismatch,
            100.0 * self.frac(self.identical),
            100.0 * self.frac(self.b0_equal),
            100.0 * self.frac(self.b0_within_1),
            100.0 * self.frac(self.b1_equal),
            self.tone_frames_fixed,
            self.tone_frames_float,
            self.snr_db(),
            self.envelope_corr
        );
    }
}

fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma) * (x - ma);
        sbb += (y - mb) * (y - mb);
    }
    sab / (saa * sbb).sqrt().max(1e-12)
}

/// Compares the fixed encoder's frames with the float encoder's for one file, decoding both with the float decoder
/// (`make_decoder` builds a fresh one per stream). Accumulates into `p`.
pub fn compare_streams<T: Copy + PartialEq>(
    p: &mut Parity,
    fixed: &[T],
    float: &[T],
    view: impl Fn(T) -> FrameView,
    make_decoder: impl Fn() -> Box<dyn FnMut(T) -> Option<[f64; 160]>>,
) {
    if fixed.len() != float.len() {
        p.count_mismatch += 1;
    }
    let n = fixed.len().min(float.len());
    let (mut dec_fx, mut dec_fl) = (make_decoder(), make_decoder());
    let (mut env_fx, mut env_fl) = (Vec::new(), Vec::new());
    for i in 0..n {
        let (a, b) = (view(fixed[i]), view(float[i]));
        p.frames += 1;
        p.tone_frames_fixed += usize::from(a.tone);
        p.tone_frames_float += usize::from(b.tone);
        p.identical += usize::from(fixed[i] == float[i]);
        if !a.tone && !b.tone {
            p.b0_equal += usize::from(a.b0 == b.b0);
            p.b0_within_1 += usize::from(a.b0.abs_diff(b.b0) <= 1);
            p.b1_equal += usize::from(a.b1 == b.b1);
        }
        let (x, y) = (dec_fx(fixed[i]).unwrap_or([0.0; 160]), dec_fl(float[i]).unwrap_or([0.0; 160]));
        for k in 0..160 {
            p.signal_energy += y[k] * y[k];
            p.error_energy += (x[k] - y[k]) * (x[k] - y[k]);
        }
        let db = |v: &[f64; 160]| 10.0 * (v.iter().map(|s| s * s).sum::<f64>() / 160.0 + 1.0).log10();
        env_fx.push(db(&x));
        env_fl.push(db(&y));
    }
    p.envelope_corr.push(pearson(&env_fx, &env_fl));
}

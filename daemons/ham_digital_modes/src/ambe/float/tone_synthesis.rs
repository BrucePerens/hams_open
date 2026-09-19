// SPDX-License-Identifier: LGPL-3.0-or-later
//! Tone-frame synthesis shared by D-STAR and AMBE+2 half-rate: turns a decoded tone frame (a single
//! tone, a DTMF digit, or a call-progress tone) into 20 ms of PCM sinusoids, with phase carried
//! across frames so a held tone is continuous.
//!
//! **Levels are provisional**: the frame formats carry an 8-bit `volume` (D-STAR) or none at all
//! (AMBE+2), and this crate has no chip-measured mapping from that field to PCM amplitude yet, so
//! [`ToneSynthesizer::synthesize`] takes an explicit peak amplitude and the per-mode wrappers pick a
//! default (`DEFAULT_TONE_PEAK`) until a live-chip level calibration replaces it.

use super::ratet27::unvoiced_synthesis::N;
use std::f64::consts::PI;

/// Peak amplitude (16-bit PCM units) used for each sinusoid of a tone frame until calibrated.
pub const DEFAULT_TONE_PEAK: f64 = 4000.0;

const SAMPLE_RATE_HZ: f64 = 8000.0;
/// DTMF row frequencies (Hz), row 0-3.
pub const DTMF_ROW_HZ: [f64; 4] = [697.0, 770.0, 852.0, 941.0];
/// DTMF column frequencies (Hz), column 0-3.
pub const DTMF_COL_HZ: [f64; 4] = [1209.0, 1336.0, 1477.0, 1633.0];
/// North American call-progress tone pairs (Hz): dial tone, ringback, busy.
pub const CALL_DIAL_HZ: [f64; 2] = [350.0, 440.0];
pub const CALL_RING_HZ: [f64; 2] = [440.0, 480.0];
pub const CALL_BUSY_HZ: [f64; 2] = [480.0, 620.0];

/// Two phase accumulators (radians); a tone uses one or both.
pub struct ToneSynthesizer {
    phase: [f64; 2],
}

impl ToneSynthesizer {
    pub fn new() -> Self {
        Self { phase: [0.0; 2] }
    }

    /// Synthesizes one frame of the given 1 or 2 frequencies, each at `peak` amplitude.
    pub fn synthesize(&mut self, freqs_hz: &[f64], peak: f64) -> [f64; N] {
        let mut out = [0.0; N];
        for (slot, &hz) in freqs_hz.iter().take(2).enumerate() {
            let step = 2.0 * PI * hz / SAMPLE_RATE_HZ;
            let mut phase = self.phase[slot];
            for sample in out.iter_mut() {
                *sample += peak * phase.sin();
                phase += step;
            }
            self.phase[slot] = phase.rem_euclid(2.0 * PI);
        }
        out
    }

    /// Resets phase (call when a non-tone frame intervenes so the next tone starts cleanly).
    pub fn reset(&mut self) {
        self.phase = [0.0; 2];
    }

    pub fn dtmf(&mut self, row: u8, col: u8, peak: f64) -> [f64; N] {
        self.synthesize(&[DTMF_ROW_HZ[(row & 3) as usize], DTMF_COL_HZ[(col & 3) as usize]], peak)
    }
}

impl Default for ToneSynthesizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dominant_hz(frame: &[f64; N]) -> f64 {
        // Goertzel-free brute force over 50..3500 Hz in 5 Hz steps.
        let mut best = (0.0, 0.0);
        let mut hz = 50.0;
        while hz < 3500.0 {
            let (mut re, mut im) = (0.0, 0.0);
            for (n, &s) in frame.iter().enumerate() {
                let a = 2.0 * PI * hz * n as f64 / SAMPLE_RATE_HZ;
                re += s * a.cos();
                im += s * a.sin();
            }
            let p = re * re + im * im;
            if p > best.1 {
                best = (hz, p);
            }
            hz += 5.0;
        }
        best.0
    }

    #[test]
    fn single_tone_lands_on_its_frequency_with_requested_peak() {
        let mut t = ToneSynthesizer::new();
        let frame = t.synthesize(&[1000.0], 1000.0);
        assert!((dominant_hz(&frame) - 1000.0).abs() <= 10.0);
        let peak = frame.iter().fold(0.0_f64, |m, &s| m.max(s.abs()));
        assert!((peak - 1000.0).abs() < 60.0, "peak {peak}");
    }

    #[test]
    fn phase_is_continuous_across_frames() {
        let mut t = ToneSynthesizer::new();
        let a = t.synthesize(&[500.0], 1000.0);
        let b = t.synthesize(&[500.0], 1000.0);
        let mut whole = ToneSynthesizer::new();
        let step = 2.0 * PI * 500.0 / SAMPLE_RATE_HZ;
        let expected_first_of_b = 1000.0 * (step * N as f64).sin();
        assert!((b[0] - expected_first_of_b).abs() < 1e-6);
        let _ = (a, &mut whole);
    }

    #[test]
    fn dtmf_digit_5_contains_770_and_1336() {
        let mut t = ToneSynthesizer::new();
        let frame = t.dtmf(1, 1, 1000.0);
        let power_at = |hz: f64| {
            let (mut re, mut im) = (0.0, 0.0);
            for (n, &s) in frame.iter().enumerate() {
                let a = 2.0 * PI * hz * n as f64 / SAMPLE_RATE_HZ;
                re += s * a.cos();
                im += s * a.sin();
            }
            re * re + im * im
        };
        assert!(power_at(770.0) > 20.0 * power_at(1000.0));
        assert!(power_at(1336.0) > 20.0 * power_at(1000.0));
    }
}

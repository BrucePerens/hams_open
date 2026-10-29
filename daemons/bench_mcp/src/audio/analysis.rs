// SPDX-License-Identifier: AGPL-3.0-or-later
//! Signal analysis on recorded samples (mono, full scale 1.0): level statistics and the level of one known tone. Pure functions, no hardware.
//!
//! Amplitudes are calibrated so that a sine of amplitude A reads `20 log10(A)` dBFS (peak, not RMS) at its frequency.

use std::f64::consts::PI;

pub const FLOOR_DB: f64 = -200.0;

pub fn db(amplitude: f64) -> f64 {
    if amplitude <= 1e-10 {
        FLOOR_DB
    } else {
        20.0 * amplitude.log10()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub samples: usize,
    pub peak_dbfs: f64,
    pub rms_dbfs: f64,
    /// Samples at or beyond 0.999 of full scale.
    pub clipped: usize,
    /// The fraction of 20 ms windows whose RMS is under the silence threshold.
    pub silence_fraction: f64,
    /// Every sample is exactly zero: a muted, disconnected or dead input, not a quiet room.
    pub all_zero: bool,
    pub dc_offset: f64,
}

pub fn rms(s: &[f32]) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    (s.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / s.len() as f64).sqrt()
}

pub fn stats(s: &[f32], rate: u32, silence_dbfs: f64) -> Stats {
    let peak = s.iter().fold(0f64, |m, x| m.max(f64::from(x.abs())));
    let win = ((f64::from(rate) * 0.02) as usize).max(1);
    let windows: Vec<&[f32]> = s.chunks(win).collect();
    let quiet = windows
        .iter()
        .filter(|w| db(rms(w) * std::f64::consts::SQRT_2) < silence_dbfs)
        .count();
    Stats {
        samples: s.len(),
        peak_dbfs: db(peak),
        // RMS of a sine reads 3 dB under its peak; report RMS as a plain RMS in dBFS (full-scale sine = -3 dBFS).
        rms_dbfs: db(rms(s)),
        clipped: s.iter().filter(|x| x.abs() >= 0.999).count(),
        silence_fraction: if windows.is_empty() {
            1.0
        } else {
            quiet as f64 / windows.len() as f64
        },
        all_zero: s.iter().all(|x| *x == 0.0),
        dc_offset: if s.is_empty() {
            0.0
        } else {
            s.iter().map(|x| f64::from(*x)).sum::<f64>() / s.len() as f64
        },
    }
}

/// The amplitude of one frequency in a block (Hann window, exact frequency, no binning): a sine of amplitude A reads A.
pub fn tone_amp(s: &[f32], rate: u32, hz: f64) -> f64 {
    let n = s.len();
    if n < 8 {
        return 0.0;
    }
    let (mut re, mut im, mut wsum) = (0.0, 0.0, 0.0);
    let step = 2.0 * PI * hz / f64::from(rate);
    for (i, x) in s.iter().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos();
        let a = step * i as f64;
        re += f64::from(*x) * w * a.cos();
        im -= f64::from(*x) * w * a.sin();
        wsum += w;
    }
    2.0 * (re * re + im * im).sqrt() / wsum
}

#[cfg(test)]
pub mod synth {
    //! Synthetic recordings for the analysis and decision tests.
    use std::f64::consts::PI;

    pub fn sine(rate: u32, secs: f64, hz: f64, amp: f64) -> Vec<f32> {
        (0..(secs * f64::from(rate)) as usize)
            .map(|i| (amp * (2.0 * PI * hz * i as f64 / f64::from(rate)).sin()) as f32)
            .collect()
    }

    /// Reproducible noise with the given RMS.
    pub fn noise(rate: u32, secs: f64, rms: f64, seed: u64) -> Vec<f32> {
        let mut s = seed | 1;
        (0..(secs * f64::from(rate)) as usize)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                // Uniform in [-1, 1) has an RMS of 1/sqrt(3).
                (((s >> 11) as f64 / (1u64 << 52) as f64 - 1.0) * rms * 3f64.sqrt()) as f32
            })
            .collect()
    }

    pub fn add(a: &[f32], b: &[f32]) -> Vec<f32> {
        let n = a.len().max(b.len());
        (0..n)
            .map(|i| a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0))
            .collect()
    }

    /// `tone` placed at `at_s` into `secs` of silence.
    pub fn at(rate: u32, secs: f64, at_s: f64, tone: &[f32]) -> Vec<f32> {
        let mut v = vec![0.0f32; (secs * f64::from(rate)) as usize];
        let start = (at_s * f64::from(rate)) as usize;
        for (i, x) in tone.iter().enumerate() {
            if start + i < v.len() {
                v[start + i] = *x;
            }
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::synth::*;
    use super::*;

    const R: u32 = 48000;

    // Tests [@ANCHOR: audio_analysis]
    #[test]
    fn a_sine_reads_its_amplitude_and_levels() {
        let s = sine(R, 1.0, 1000.0, 0.25);
        let st = stats(&s, R, -60.0);
        assert!((st.peak_dbfs - db(0.25)).abs() < 0.01, "{st:?}");
        assert!((st.rms_dbfs - db(0.25 / 2f64.sqrt())).abs() < 0.01);
        assert_eq!((st.clipped, st.all_zero), (0, false));
        assert!((db(tone_amp(&s, R, 1000.0)) - db(0.25)).abs() < 0.05);
    }

    #[test]
    fn digital_silence_clipping_and_silence_fraction_are_counted() {
        let z = stats(&vec![0.0; 4800], R, -60.0);
        assert!(z.all_zero && z.silence_fraction == 1.0 && z.peak_dbfs == FLOOR_DB);
        let loud = stats(
            &sine(R, 0.1, 500.0, 1.5)
                .iter()
                .map(|x| x.clamp(-1.0, 1.0))
                .collect::<Vec<_>>(),
            R,
            -60.0,
        );
        assert!(loud.clipped > 100, "{loud:?}");
        let half = at(R, 1.0, 0.5, &sine(R, 0.5, 800.0, 0.3));
        let st = stats(&half, R, -60.0);
        assert!((st.silence_fraction - 0.5).abs() < 0.03, "{st:?}");
        assert_eq!(stats(&[], R, -60.0).silence_fraction, 1.0);
    }
}

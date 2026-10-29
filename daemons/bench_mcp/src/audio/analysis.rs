// SPDX-License-Identifier: AGPL-3.0-or-later
//! Signal analysis on recorded samples (mono, full scale 1.0): level statistics, an averaged amplitude spectrum, the dominant
//! frequency, and the level of one known tone. Pure functions, no hardware.
//!
//! Amplitudes are calibrated so that a sine of amplitude A reads `20 log10(A)` dBFS (peak, not RMS) at its frequency.

use std::f64::consts::PI;

pub const FLOOR_DB: f64 = -200.0;

pub fn db(amplitude: f64) -> f64 {
    // [@ANCHOR: bench_mcp_audio_analysis_db]
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
    // [@ANCHOR: bench_mcp_audio_analysis_rms]
    if s.is_empty() {
        return 0.0;
    }
    (s.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / s.len() as f64).sqrt()
}

pub fn stats(s: &[f32], rate: u32, silence_dbfs: f64) -> Stats {
    // [@ANCHOR: bench_mcp_audio_analysis_stats]
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

/// In-place radix-2 FFT; the length must be a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    // [@ANCHOR: bench_mcp_audio_analysis_fft]
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

fn hann(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos())
        .collect()
}

#[derive(Clone, Debug)]
pub struct Spectrum {
    pub bin_hz: f64,
    /// Amplitude per bin from 0 Hz to Nyquist (linear, full scale 1.0).
    pub amp: Vec<f64>,
}

impl Spectrum {
    pub fn db_at(&self, i: usize) -> f64 {
        db(self.amp[i])
    }

    fn range(&self, lo_hz: f64, hi_hz: f64) -> std::ops::Range<usize> {
        // [@ANCHOR: bench_mcp_audio_analysis_spectrum_range]
        let lo = ((lo_hz / self.bin_hz).ceil() as usize).max(1);
        let hi = ((hi_hz / self.bin_hz).floor() as usize + 1).min(self.amp.len());
        lo..hi.max(lo)
    }
}

/// Welch's averaged amplitude spectrum: Hann-windowed blocks of `fft_len` (a power of two) with 50 % overlap. A signal shorter
/// than one block is zero-padded.
pub fn spectrum(s: &[f32], rate: u32, fft_len: usize) -> Result<Spectrum, String> {
    // [@ANCHOR: bench_mcp_audio_analysis_spectrum]
    if fft_len < 64 || !fft_len.is_power_of_two() {
        return Err(format!(
            "fft_len {fft_len} must be a power of two of at least 64"
        ));
    }
    let w = hann(fft_len);
    let wsum: f64 = w.iter().sum();
    let hop = fft_len / 2;
    let mut power = vec![0.0f64; fft_len / 2 + 1];
    let mut blocks = 0usize;
    let mut at = 0usize;
    loop {
        let mut re = vec![0.0f64; fft_len];
        let mut im = vec![0.0f64; fft_len];
        for i in 0..fft_len {
            re[i] = s.get(at + i).map_or(0.0, |x| f64::from(*x)) * w[i];
        }
        fft(&mut re, &mut im);
        for (k, p) in power.iter_mut().enumerate() {
            *p += re[k] * re[k] + im[k] * im[k];
        }
        blocks += 1;
        at += hop;
        if at + fft_len > s.len() {
            break;
        }
    }
    Ok(Spectrum {
        bin_hz: f64::from(rate) / fft_len as f64,
        amp: power
            .iter()
            .map(|p| 2.0 * (p / blocks as f64).sqrt() / wsum)
            .collect(),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Peak {
    pub hz: f64,
    pub dbfs: f64,
}

/// Local maxima of the spectrum between `lo_hz` and `hi_hz` at least `min_db` above the median of that range, loudest first,
/// frequencies refined by parabolic interpolation.
pub fn peaks(sp: &Spectrum, lo_hz: f64, hi_hz: f64, count: usize, min_db: f64) -> Vec<Peak> {
    // [@ANCHOR: bench_mcp_audio_analysis_peaks]
    let r = sp.range(lo_hz, hi_hz);
    if r.len() < 3 {
        return Vec::new();
    }
    let mut levels: Vec<f64> = r.clone().map(|i| sp.db_at(i)).collect();
    levels.sort_by(|a, b| a.total_cmp(b));
    let median = levels[levels.len() / 2];
    let mut found: Vec<Peak> = Vec::new();
    for i in r.start.max(1)..r.end.min(sp.amp.len() - 1) {
        let (a, b, c) = (sp.db_at(i - 1), sp.db_at(i), sp.db_at(i + 1));
        if b > a && b >= c && b >= median + min_db {
            let denom = a - 2.0 * b + c;
            let off = if denom.abs() < 1e-12 {
                0.0
            } else {
                0.5 * (a - c) / denom
            };
            found.push(Peak {
                hz: (i as f64 + off) * sp.bin_hz,
                dbfs: b - 0.25 * (a - c) * off,
            });
        }
    }
    found.sort_by(|a, b| b.dbfs.total_cmp(&a.dbfs));
    // Keep peaks at least 3 bins apart, so the skirt of one tone is not reported as another.
    let mut kept: Vec<Peak> = Vec::new();
    for p in found {
        if kept.iter().all(|k| (k.hz - p.hz).abs() > 3.0 * sp.bin_hz) {
            kept.push(p);
        }
        if kept.len() == count {
            break;
        }
    }
    kept
}

/// The loudest peak between `lo_hz` and `hi_hz`, if any is `min_db` above the median level of that range.
pub fn dominant(sp: &Spectrum, lo_hz: f64, hi_hz: f64, min_db: f64) -> Option<Peak> {
    peaks(sp, lo_hz, hi_hz, 1, min_db).into_iter().next()
}

/// The median spectrum level in dBFS over a range: a noise floor estimate that a few tones do not move.
pub fn median_db(sp: &Spectrum, lo_hz: f64, hi_hz: f64) -> f64 {
    // [@ANCHOR: bench_mcp_audio_analysis_median_db]
    let r = sp.range(lo_hz, hi_hz);
    let mut v: Vec<f64> = r.map(|i| sp.db_at(i)).collect();
    if v.is_empty() {
        return FLOOR_DB;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// The amplitude of one frequency in a block (Hann window, exact frequency, no binning): a sine of amplitude A reads A.
pub fn tone_amp(s: &[f32], rate: u32, hz: f64) -> f64 {
    // [@ANCHOR: bench_mcp_audio_analysis_tone_amp]
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

/// Pearson correlation of two spectra in dB over a frequency range, 1.0 for the same shape and about 0 for unrelated ones.
pub fn similarity(a: &Spectrum, b: &Spectrum, lo_hz: f64, hi_hz: f64) -> f64 {
    // [@ANCHOR: bench_mcp_audio_analysis_similarity]
    let (ra, rb) = (a.range(lo_hz, hi_hz), b.range(lo_hz, hi_hz));
    let n = ra.len().min(rb.len());
    if n < 3 || (a.bin_hz - b.bin_hz).abs() > 1e-9 {
        return 0.0;
    }
    let xa: Vec<f64> = ra.take(n).map(|i| a.db_at(i).max(-120.0)).collect();
    let xb: Vec<f64> = rb.take(n).map(|i| b.db_at(i).max(-120.0)).collect();
    let (ma, mb) = (
        xa.iter().sum::<f64>() / n as f64,
        xb.iter().sum::<f64>() / n as f64,
    );
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for i in 0..n {
        sab += (xa[i] - ma) * (xb[i] - mb);
        saa += (xa[i] - ma).powi(2);
        sbb += (xb[i] - mb).powi(2);
    }
    if saa < 1e-12 || sbb < 1e-12 {
        return 0.0;
    }
    sab / (saa * sbb).sqrt()
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
    // Tests [@ANCHOR: bench_mcp_audio_analysis_db]
    // Tests [@ANCHOR: bench_mcp_audio_analysis_fft]
    // Tests [@ANCHOR: bench_mcp_audio_analysis_spectrum]
    // Tests [@ANCHOR: bench_mcp_audio_analysis_stats]
    // Tests [@ANCHOR: bench_mcp_audio_analysis_tone_amp]
    #[test]
    fn a_sine_reads_its_amplitude_frequency_and_levels() {
        let s = sine(R, 1.0, 1000.0, 0.25);
        let st = stats(&s, R, -60.0);
        assert!((st.peak_dbfs - db(0.25)).abs() < 0.01, "{st:?}");
        assert!((st.rms_dbfs - db(0.25 / 2f64.sqrt())).abs() < 0.01);
        assert_eq!((st.clipped, st.all_zero), (0, false));
        let sp = spectrum(&s, R, 4096).unwrap();
        let p = dominant(&sp, 100.0, 8000.0, 20.0).unwrap();
        assert!((p.hz - 1000.0).abs() < 1.0, "{p:?}");
        assert!((p.dbfs - db(0.25)).abs() < 0.5, "{p:?}");
        assert!((db(tone_amp(&s, R, 1000.0)) - db(0.25)).abs() < 0.05);
    }

    // Tests [@ANCHOR: bench_mcp_audio_analysis_spectrum_range]
    // Tests [@ANCHOR: bench_mcp_audio_analysis_peaks]
    #[test]
    fn a_tone_between_bins_is_still_found_and_two_tones_are_separate_peaks() {
        let s = add(&sine(R, 1.0, 1234.5, 0.1), &sine(R, 1.0, 3000.0, 0.05));
        let sp = spectrum(&s, R, 4096).unwrap();
        let p = peaks(&sp, 100.0, 8000.0, 3, 20.0);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(
            (p[0].hz - 1234.5).abs() < 2.0 && (p[1].hz - 3000.0).abs() < 2.0,
            "{p:?}"
        );
        assert!(p[0].dbfs > p[1].dbfs);
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

    // Tests [@ANCHOR: bench_mcp_audio_analysis_median_db]
    // Tests [@ANCHOR: bench_mcp_audio_analysis_rms]
    #[test]
    fn noise_has_the_requested_rms_and_no_peak() {
        let n = noise(R, 1.0, 0.01, 7);
        assert!((db(rms(&n)) - db(0.01)).abs() < 0.3);
        let sp = spectrum(&n, R, 4096).unwrap();
        assert!(dominant(&sp, 100.0, 8000.0, 15.0).is_none());
        let med = median_db(&sp, 100.0, 8000.0);
        assert!(med < db(0.01) && med > -90.0, "{med}");
    }

    // Tests [@ANCHOR: bench_mcp_audio_analysis_similarity]
    #[test]
    fn similar_spectra_correlate_and_different_ones_do_not() {
        let a = spectrum(
            &add(&sine(R, 1.0, 1000.0, 0.1), &noise(R, 1.0, 0.001, 1)),
            R,
            4096,
        )
        .unwrap();
        let b = spectrum(
            &add(&sine(R, 1.0, 1000.0, 0.05), &noise(R, 1.0, 0.001, 2)),
            R,
            4096,
        )
        .unwrap();
        let c = spectrum(
            &add(&sine(R, 1.0, 2500.0, 0.1), &noise(R, 1.0, 0.001, 3)),
            R,
            4096,
        )
        .unwrap();
        assert!(similarity(&a, &b, 100.0, 6000.0) > 0.9);
        assert!(similarity(&a, &c, 100.0, 6000.0) < 0.5);
    }

    #[test]
    fn bad_fft_lengths_are_errors() {
        assert!(spectrum(&[0.0; 100], R, 100).is_err());
        assert!(spectrum(&[0.0; 100], R, 32).is_err());
        assert!(spectrum(&[0.1; 10], R, 64).is_ok());
    }
}

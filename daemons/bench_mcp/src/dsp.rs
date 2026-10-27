// SPDX-License-Identifier: AGPL-3.0-or-later
//! Signal processing for the SDR tools: an averaged power spectrum of complex baseband samples, peak picking, and the phase
//! between two receive channels that share one local oscillator. Pure functions on `(f32, f32)` I/Q pairs, no hardware.

use std::f64::consts::PI;

/// One complex sample, I then Q, as the radios deliver them (full scale is 1.0).
pub type Iq = (f32, f32);

/// An averaged power spectrum, with bin 0 at -rate/2 (the DC bin is at `len / 2`).
#[derive(Debug, Clone)]
pub struct Spectrum {
    pub bin_hz: f64,
    /// Power of each bin in dB relative to a full-scale complex tone (1.0 amplitude is 0 dBFS).
    pub power_db: Vec<f64>,
}

impl Spectrum {
    /// The offset of bin `i` from the centre frequency.
    pub fn offset_hz(&self, i: usize) -> f64 {
        (i as f64 - (self.power_db.len() / 2) as f64) * self.bin_hz
    }
}

/// In-place radix-2 FFT; the length must be a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
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

fn check_len(fft_len: usize) -> Result<(), String> {
    if fft_len < 16 || !fft_len.is_power_of_two() {
        return Err(format!(
            "fft_len {fft_len} must be a power of two of at least 16"
        ));
    }
    Ok(())
}

/// The windowed FFT of one block starting at `at`, with DC moved to the middle.
fn block_fft(samples: &[Iq], at: usize, w: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let n = w.len();
    let mut re: Vec<f64> = (0..n)
        .map(|i| f64::from(samples[at + i].0) * w[i])
        .collect();
    let mut im: Vec<f64> = (0..n)
        .map(|i| f64::from(samples[at + i].1) * w[i])
        .collect();
    fft(&mut re, &mut im);
    re.rotate_left(n / 2);
    im.rotate_left(n / 2);
    (re, im)
}

/// Welch's averaged power spectrum: Hann-windowed blocks with 50 % overlap, scaled so that a complex tone of amplitude A reads
/// 20 log10(A) dBFS at its bin (bin-centred tones; a tone between bins reads up to 1.4 dB low).
pub fn welch(samples: &[Iq], fft_len: usize, rate_hz: f64) -> Result<Spectrum, String> {
    // [@ANCHOR: bench_mcp_sdr_spectrum_math]
    check_len(fft_len)?;
    if samples.len() < fft_len {
        return Err(format!(
            "{} samples are fewer than one FFT block of {fft_len}",
            samples.len()
        ));
    }
    if !(rate_hz.is_finite() && rate_hz > 0.0) {
        return Err("sample rate must be a positive number".into());
    }
    let w = hann(fft_len);
    let wsum: f64 = w.iter().sum();
    let mut acc = vec![0.0f64; fft_len];
    let mut blocks = 0usize;
    let mut at = 0;
    while at + fft_len <= samples.len() {
        let (re, im) = block_fft(samples, at, &w);
        for k in 0..fft_len {
            acc[k] += re[k] * re[k] + im[k] * im[k];
        }
        blocks += 1;
        at += fft_len / 2;
    }
    let scale = 1.0 / (blocks as f64 * wsum * wsum);
    Ok(Spectrum {
        bin_hz: rate_hz / fft_len as f64,
        power_db: acc
            .iter()
            .map(|p| 10.0 * (p * scale).max(1e-30).log10())
            .collect(),
    })
}

/// The median power of the spectrum, the noise floor estimate (peaks do not move it).
pub fn noise_floor_db(s: &Spectrum) -> f64 {
    let mut v = s.power_db.clone();
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// A spectral peak: its offset from the centre frequency and its power.
#[derive(Debug, Clone, PartialEq)]
pub struct Peak {
    pub offset_hz: f64,
    pub power_db: f64,
}

/// The `count` strongest local maxima at least `min_above_floor_db` over the median floor, at least `min_sep_bins` apart, skipping
/// the DC bins (radios put a spike at the centre frequency) within `skip_dc_bins` of the middle.
pub fn peaks(
    s: &Spectrum,
    count: usize,
    min_above_floor_db: f64,
    min_sep_bins: usize,
    skip_dc_bins: usize,
) -> Vec<Peak> {
    let n = s.power_db.len();
    let floor = noise_floor_db(s);
    let mid = n / 2;
    let mut cand: Vec<usize> = (1..n.saturating_sub(1))
        .filter(|&i| {
            let p = s.power_db[i];
            p >= floor + min_above_floor_db
                && p >= s.power_db[i - 1]
                && p > s.power_db[i + 1]
                && i.abs_diff(mid) > skip_dc_bins
        })
        .collect();
    cand.sort_by(|&a, &b| s.power_db[b].total_cmp(&s.power_db[a]));
    let mut out: Vec<usize> = Vec::new();
    for i in cand {
        if out.iter().all(|&j| i.abs_diff(j) >= min_sep_bins) {
            out.push(i);
            if out.len() == count {
                break;
            }
        }
    }
    out.iter()
        .map(|&i| Peak {
            offset_hz: s.offset_hz(i),
            power_db: s.power_db[i],
        })
        .collect()
}

/// What two receive channels sharing one oscillator show at the strongest tone.
#[derive(Debug, Clone)]
pub struct PhaseReport {
    /// The offset of the analysed tone from the centre frequency.
    pub tone_offset_hz: f64,
    /// Circular mean of the phase of channel A minus channel B over the blocks, in degrees (-180 to 180).
    pub phase_deg: f64,
    /// Circular standard deviation of that phase over the blocks, in degrees: how stable the offset is.
    pub phase_std_deg: f64,
    /// Amplitude of A over amplitude of B at the tone, in dB.
    pub gain_db: f64,
    /// Magnitude-squared coherence at the tone (1.0 is perfectly related).
    pub coherence: f64,
    pub blocks: usize,
}

/// The phase and amplitude relation of two simultaneous captures at their strongest common tone, measured block by block.
pub fn phase_between(
    a: &[Iq],
    b: &[Iq],
    fft_len: usize,
    rate_hz: f64,
    skip_dc_bins: usize,
) -> Result<PhaseReport, String> {
    // [@ANCHOR: bench_mcp_sdr_phase_math]
    check_len(fft_len)?;
    let n = a.len().min(b.len());
    if n < fft_len * 2 {
        return Err(format!(
            "{n} samples are too few for a phase measurement (need {})",
            fft_len * 2
        ));
    }
    let (a, b) = (&a[..n], &b[..n]);
    let sa = welch(a, fft_len, rate_hz)?;
    let mid = fft_len / 2;
    let k = (1..fft_len - 1)
        .filter(|&i| i.abs_diff(mid) > skip_dc_bins)
        .max_by(|&x, &y| sa.power_db[x].total_cmp(&sa.power_db[y]))
        .ok_or("no bin to analyse")?;
    let w = hann(fft_len);
    let (mut sx, mut sy) = (0.0f64, 0.0f64);
    let (mut pa, mut pb) = (0.0f64, 0.0f64);
    let mut cross = (0.0f64, 0.0f64);
    let mut blocks = 0usize;
    let mut at = 0;
    while at + fft_len <= n {
        let (ar, ai) = block_fft(a, at, &w);
        let (br, bi) = block_fft(b, at, &w);
        // A times the conjugate of B at bin k.
        let (cr, ci) = (ar[k] * br[k] + ai[k] * bi[k], ai[k] * br[k] - ar[k] * bi[k]);
        let ang = ci.atan2(cr);
        sx += ang.cos();
        sy += ang.sin();
        cross.0 += cr;
        cross.1 += ci;
        pa += ar[k] * ar[k] + ai[k] * ai[k];
        pb += br[k] * br[k] + bi[k] * bi[k];
        blocks += 1;
        at += fft_len;
    }
    let r = (sx * sx + sy * sy).sqrt() / blocks as f64;
    Ok(PhaseReport {
        tone_offset_hz: sa.offset_hz(k),
        phase_deg: sy.atan2(sx).to_degrees(),
        phase_std_deg: (-2.0 * r.max(1e-12).ln()).max(0.0).sqrt().to_degrees(),
        gain_db: 10.0 * (pa / pb.max(1e-300)).max(1e-300).log10(),
        coherence: ((cross.0 * cross.0 + cross.1 * cross.1) / (pa * pb).max(1e-300)).min(1.0),
        blocks,
    })
}

/// Basic statistics of a capture: RMS and peak amplitude, the DC offset, and how many samples sit at full scale (clipping).
pub fn stats(samples: &[Iq]) -> (f64, f64, (f64, f64), usize) {
    let n = samples.len().max(1) as f64;
    let (mut p, mut peak, mut si, mut sq, mut clip) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0usize);
    for &(i, q) in samples {
        let (i, q) = (f64::from(i), f64::from(q));
        p += i * i + q * q;
        peak = peak.max(i.abs()).max(q.abs());
        si += i;
        sq += q;
        if i.abs() >= 0.999 || q.abs() >= 0.999 {
            clip += 1;
        }
    }
    ((p / n).sqrt(), peak, (si / n, sq / n), clip)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A complex tone plus a little deterministic noise.
    fn tone(n: usize, rate: f64, f: f64, amp: f64, phase: f64, noise: f64, seed: u64) -> Vec<Iq> {
        let mut s = seed;
        let mut rnd = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((s >> 33) as f64 / (1u64 << 31) as f64) - 0.5
        };
        (0..n)
            .map(|i| {
                let a = 2.0 * PI * f * i as f64 / rate + phase;
                (
                    (amp * a.cos() + noise * rnd()) as f32,
                    (amp * a.sin() + noise * rnd()) as f32,
                )
            })
            .collect()
    }

    // Tests [@ANCHOR: bench_mcp_sdr_spectrum_math]
    #[test]
    fn a_bin_centred_tone_reads_its_amplitude_and_offset() {
        let (rate, n) = (2.0e6, 1 << 16);
        let bin = rate / 4096.0;
        let f = 100.0 * bin;
        let s = welch(&tone(n, rate, f, 0.5, 0.0, 0.002, 1), 4096, rate).unwrap();
        let p = peaks(&s, 3, 20.0, 4, 2);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!((p[0].offset_hz - f).abs() < 1.0, "{p:?}");
        assert!((p[0].power_db - 20.0 * 0.5f64.log10()).abs() < 0.1, "{p:?}");
        // A negative-frequency tone shows on the other side of the centre.
        let s = welch(&tone(n, rate, -f, 0.5, 0.0, 0.002, 1), 4096, rate).unwrap();
        assert!((peaks(&s, 1, 20.0, 4, 2)[0].offset_hz + f).abs() < 1.0);
    }

    #[test]
    fn peaks_are_ordered_floor_is_the_median_and_dc_is_skipped() {
        let (rate, n) = (2.0e6, 1 << 16);
        let bin = rate / 4096.0;
        let mut x = tone(n, rate, 300.0 * bin, 0.3, 0.0, 0.001, 2);
        let y = tone(n, rate, -200.0 * bin, 0.1, 0.0, 0.0, 3);
        let dc = tone(n, rate, 0.0, 0.4, 0.0, 0.0, 4);
        for i in 0..n {
            x[i] = (x[i].0 + y[i].0 + dc[i].0, x[i].1 + y[i].1 + dc[i].1);
        }
        let s = welch(&x, 4096, rate).unwrap();
        let p = peaks(&s, 5, 20.0, 4, 3);
        assert_eq!(p.len(), 2, "the DC spike must be skipped: {p:?}");
        assert!(p[0].power_db > p[1].power_db);
        assert!((p[0].offset_hz - 300.0 * bin).abs() < 1.0);
        assert!(noise_floor_db(&s) < p[1].power_db - 30.0);
    }

    #[test]
    fn bad_fft_lengths_and_short_captures_are_refused() {
        assert!(welch(&[(0.0, 0.0); 100], 100, 1e6).is_err());
        assert!(welch(&[(0.0, 0.0); 100], 1024, 1e6).is_err());
        assert!(welch(&[(0.0, 0.0); 2048], 1024, 0.0).is_err());
    }

    // Tests [@ANCHOR: bench_mcp_sdr_phase_math]
    #[test]
    fn the_phase_between_two_channels_and_its_stability_are_measured() {
        let (rate, n) = (2.0e6, 1 << 16);
        let f = 123.0 * rate / 4096.0;
        let a = tone(n, rate, f, 0.5, 0.7, 0.01, 5);
        let b = tone(n, rate, f, 0.25, 0.0, 0.01, 6);
        let r = phase_between(&a, &b, 4096, rate, 2).unwrap();
        assert!((r.phase_deg - 0.7f64.to_degrees()).abs() < 1.0, "{r:?}");
        assert!((r.gain_db - 6.02).abs() < 0.2, "{r:?}");
        assert!(r.coherence > 0.99 && r.phase_std_deg < 1.0, "{r:?}");
        assert!((r.tone_offset_hz - f).abs() < 1.0);
        // Two unrelated noise captures are not coherent, and the phase wanders from block to block.
        let u = tone(n, rate, 0.0, 0.0, 0.0, 1.0, 7);
        let v = tone(n, rate, 0.0, 0.0, 0.0, 1.0, 8);
        let r = phase_between(&u, &v, 4096, rate, 2).unwrap();
        assert!(r.coherence < 0.5 && r.phase_std_deg > 20.0, "{r:?}");
        assert!(phase_between(&a[..100], &b[..100], 4096, rate, 2).is_err());
    }

    #[test]
    fn stats_report_rms_peak_dc_and_clipping() {
        let x: Vec<Iq> = vec![(1.0, 0.0), (0.0, 0.0), (0.5, 0.5), (0.0, -1.0)];
        let (rms, peak, dc, clip) = stats(&x);
        assert!((rms - ((1.0 + 0.5 + 1.0) / 4.0f64).sqrt()).abs() < 1e-9);
        assert_eq!((peak, clip), (1.0, 2));
        assert!((dc.0 - 0.375).abs() < 1e-9 && (dc.1 + 0.125).abs() < 1e-9);
    }
}

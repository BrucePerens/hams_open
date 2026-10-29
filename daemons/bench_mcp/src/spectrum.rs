// SPDX-License-Identifier: AGPL-3.0-or-later
//! The spectrum of a scope waveform, computed on the host: an FFT with a rectangular, Hanning, Hamming or Blackman window, the
//! peaks with parabolic interpolation, and the levels in dBV rms and dBm (into 50 ohms). The scope's 8-bit samples limit the
//! dynamic range to about 48 dB per bin before the window's processing gain, which is why this is a check of the strong
//! lines (the fundamental and its first harmonics), not a spectrum analyzer.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Window {
    Rect,
    Hanning,
    Hamming,
    Blackman,
}

impl Window {
    pub fn parse(s: &str) -> Result<Window, String> {
        match s.to_lowercase().as_str() {
            "rect" | "rectangular" | "none" => Ok(Window::Rect),
            "hann" | "hanning" => Ok(Window::Hanning),
            "hamming" => Ok(Window::Hamming),
            "blackman" => Ok(Window::Blackman),
            o => Err(format!(
                "unknown window {o:?} (rect, hanning, hamming, blackman)"
            )),
        }
    }

    fn weight(self, k: usize, n: usize) -> f64 {
        let x = 2.0 * PI * k as f64 / n as f64; // periodic form, for spectral analysis
        match self {
            Window::Rect => 1.0,
            Window::Hanning => 0.5 - 0.5 * x.cos(),
            Window::Hamming => 0.54 - 0.46 * x.cos(),
            Window::Blackman => 0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos(),
        }
    }
}

/// In-place iterative radix-2 FFT; `re.len()` must be a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for s in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (s + k, s + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Peak {
    pub freq_hz: f64,
    pub vrms: f64,
    pub dbv: f64,
    pub dbm_50: f64,
}

pub struct Spectrum {
    pub bin_hz: f64,
    /// rms amplitude of each bin from 0 to Nyquist (a sine's whole power is in the bin or bins around it; see `peaks`).
    pub vrms: Vec<f64>,
    /// The window's noise bandwidth in bins, for turning a bin sum into power.
    pub enbw_bins: f64,
}

/// The spectrum of `x` (its length must be a power of two, at least 16) sampled at `fs`.
pub fn spectrum(x: &[f32], fs: f64, w: Window) -> Result<Spectrum, String> {
    // [@ANCHOR: bench_mcp_spectrum]
    let n = x.len();
    if n < 16 || !n.is_power_of_two() {
        return Err(format!(
            "{n} samples: the FFT needs a power of two, at least 16"
        ));
    }
    let mean = x.iter().map(|v| *v as f64).sum::<f64>() / n as f64;
    let wts: Vec<f64> = (0..n).map(|k| w.weight(k, n)).collect();
    let sum_w: f64 = wts.iter().sum();
    let sum_w2: f64 = wts.iter().map(|v| v * v).sum();
    let mut re: Vec<f64> = x
        .iter()
        .zip(&wts)
        .map(|(v, w)| (*v as f64 - mean) * w)
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    // A sine of amplitude A gives |X| = A sum_w / 2 at its bin (coherent gain), so Vrms = |X| * 2 / sum_w / sqrt 2.
    let vrms: Vec<f64> = (0..=n / 2)
        .map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt() * 2.0 / sum_w / 2f64.sqrt())
        .collect();
    Ok(Spectrum {
        bin_hz: fs / n as f64,
        vrms,
        enbw_bins: n as f64 * sum_w2 / (sum_w * sum_w),
    })
}

/// The strongest local maxima, strongest first, at most `count`, at least `min_below_max_db` above... that is, no weaker than
/// that many dB below the strongest. Frequencies are interpolated with a parabola through the log magnitudes of the three bins.
/// The level of a peak is its interpolated bin value (exact for a tone centred on a bin, within the window's scalloping loss
/// otherwise: 0 dB rect to 1.4 dB for Hanning at worst).
pub fn peaks(sp: &Spectrum, count: usize, min_below_max_db: f64) -> Vec<Peak> {
    let v = &sp.vrms;
    let max = v[2..].iter().cloned().fold(0.0, f64::max); // skip DC and the first bin
    let mut found = Vec::new();
    for k in 2..v.len() - 1 {
        if v[k] > v[k - 1] && v[k] >= v[k + 1] && v[k] > 0.0 {
            let (a, b, c) = (
                v[k - 1].max(1e-300).ln(),
                v[k].ln(),
                v[k + 1].max(1e-300).ln(),
            );
            let denom = a - 2.0 * b + c;
            let d = if denom.abs() > 1e-12 {
                0.5 * (a - c) / denom
            } else {
                0.0
            };
            let peak_ln = b - 0.25 * (a - c) * d;
            let vr = peak_ln.exp();
            if 20.0 * (vr / max).log10() >= -min_below_max_db {
                let dbv = 20.0 * vr.log10();
                found.push(Peak {
                    freq_hz: (k as f64 + d) * sp.bin_hz,
                    vrms: vr,
                    dbv,
                    dbm_50: dbv + 13.0103,
                });
            }
        }
    }
    found.sort_by(|a, b| b.vrms.total_cmp(&a.vrms));
    found.truncate(count);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize, fs: f64, parts: &[(f64, f64)]) -> Vec<f32> {
        (0..n)
            .map(|i| {
                parts
                    .iter()
                    .map(|(f, a)| a * (2.0 * PI * f * i as f64 / fs).sin())
                    .sum::<f64>() as f32
            })
            .collect()
    }

    // Tests [@ANCHOR: bench_mcp_spectrum]
    #[test]
    fn a_tone_is_found_at_its_frequency_and_level_in_every_window() {
        let (n, fs) = (1 << 16, 500e6);
        // 10 MHz, 20 mV peak (14.14 mV rms), and a 20 MHz harmonic 40 dB down; neither is on a bin centre.
        let x = tone(n, fs, &[(10.0007e6, 0.020), (20.0014e6, 0.0002)]);
        for w in [
            Window::Rect,
            Window::Hanning,
            Window::Hamming,
            Window::Blackman,
        ] {
            let sp = spectrum(&x, fs, w).unwrap();
            let p = peaks(&sp, 5, 80.0);
            assert!(
                (p[0].freq_hz - 10.0007e6).abs() < 2e3,
                "{w:?}: {}",
                p[0].freq_hz
            );
            // within the window's scalloping loss (rect worst: 3.9 dB at half a bin; interpolation recovers most of it)
            assert!(
                (p[0].dbv - 20.0 * (0.020 / 2f64.sqrt()).log10()).abs() < 1.5,
                "{w:?}: {}",
                p[0].dbv
            );
            let h = p
                .iter()
                .find(|q| (q.freq_hz - 20.0014e6).abs() < 3e3)
                .expect("harmonic");
            assert!(
                (h.dbv - p[0].dbv + 40.0).abs() < 2.0,
                "{w:?}: {}",
                h.dbv - p[0].dbv
            );
            assert!((p[0].dbm_50 - p[0].dbv - 13.0103).abs() < 1e-9);
        }
        // A window lowers the leakage far from a tone: Blackman's far floor is far below the rectangular one.
        let far = |w| {
            let sp = spectrum(&x, fs, w).unwrap();
            sp.vrms[(sp.vrms.len() * 3) / 4]
        };
        assert!(far(Window::Blackman) < far(Window::Rect) / 10.0);
    }

    #[test]
    fn bad_sizes_and_windows_are_refused() {
        assert!(spectrum(&vec![0.0; 1000], 1e6, Window::Rect).is_err());
        assert!(spectrum(&[0.0; 8], 1e6, Window::Rect).is_err());
        assert_eq!(Window::parse("Hann").unwrap(), Window::Hanning);
        assert!(Window::parse("flattop").is_err());
    }
}

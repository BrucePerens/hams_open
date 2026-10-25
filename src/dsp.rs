//! Small DSP helpers for tests and examples: radix-2 FFT and averaged power spectra.

use crate::stream::IqI16;
use std::f64::consts::PI;

/// In-place radix-2 FFT over (re, im) pairs; `n` must be a power of two.
pub fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    assert!(n.is_power_of_two() && im.len() == n);
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
                let a = start + k;
                let b = a + len / 2;
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
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

/// Averaged power spectrum (Hann window, DC removed), FFT-shifted so that bin `n/2` is 0 Hz.
/// Returns power in dB relative to full scale (a full-scale complex tone reads about 0 dB).
pub fn power_spectrum_db(x: &[IqI16], n: usize) -> Vec<f64> {
    assert!(n.is_power_of_two() && x.len() >= n);
    let (mi, mq) = x.iter().fold((0.0, 0.0), |(a, b), s| (a + s.i as f64, b + s.q as f64));
    let (mi, mq) = (mi / x.len() as f64, mq / x.len() as f64);
    let win: Vec<f64> = (0..n).map(|k| 0.5 - 0.5 * (2.0 * PI * k as f64 / n as f64).cos()).collect();
    let wsum: f64 = win.iter().sum();
    let mut acc = vec![0.0f64; n];
    let segs = x.len() / n;
    let (mut re, mut im) = (vec![0.0; n], vec![0.0; n]);
    for s in 0..segs {
        for k in 0..n {
            let v = x[s * n + k];
            re[k] = (v.i as f64 - mi) / 32768.0 * win[k];
            im[k] = (v.q as f64 - mq) / 32768.0 * win[k];
        }
        fft(&mut re, &mut im);
        for k in 0..n {
            acc[k] += re[k] * re[k] + im[k] * im[k];
        }
    }
    (0..n)
        .map(|k| {
            let p = acc[(k + n / 2) % n] / segs as f64 / (wsum * wsum);
            10.0 * (p.max(1e-30)).log10()
        })
        .collect()
}

/// Median of a slice.
pub fn median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[s.len() / 2]
}

/// Strongest bin within `half_width_hz` of `offset_hz`, ignoring `exclude_dc_hz` around 0 Hz.
/// Returns (offset of the peak in Hz, level dB, level over the spectrum median dB).
pub fn peak_near(spec_db: &[f64], fs: f64, offset_hz: f64, half_width_hz: f64, exclude_dc_hz: f64) -> (f64, f64, f64) {
    let n = spec_db.len();
    let bin = fs / n as f64;
    let floor = median(spec_db);
    let mut best = (0.0, f64::NEG_INFINITY);
    for (k, &p) in spec_db.iter().enumerate() {
        let f = (k as f64 - (n / 2) as f64) * bin;
        if (f - offset_hz).abs() <= half_width_hz && f.abs() >= exclude_dc_hz && p > best.1 {
            best = (f, p);
        }
    }
    (best.0, best.1, best.1 - floor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_lands_in_right_bin() {
        let fs = 2e6;
        let f0 = 250_000.0;
        let x: Vec<IqI16> = (0..8192)
            .map(|k| {
                let ph = 2.0 * PI * f0 * k as f64 / fs;
                IqI16 { i: (8000.0 * ph.cos()) as i16, q: (8000.0 * ph.sin()) as i16 }
            })
            .collect();
        let s = power_spectrum_db(&x, 1024);
        let (f, _, over) = peak_near(&s, fs, 0.0, 1e6, 5e3);
        assert!((f - f0).abs() < 2.0 * fs / 1024.0, "{f}");
        assert!(over > 60.0);
        // A negative-frequency tone (Q inverted).
        let y: Vec<IqI16> = x.iter().map(|s| IqI16 { i: s.i, q: -s.q }).collect();
        let s = power_spectrum_db(&y, 1024);
        let (f, _, _) = peak_near(&s, fs, 0.0, 1e6, 5e3);
        assert!((f + f0).abs() < 2.0 * fs / 1024.0, "{f}");
    }
}

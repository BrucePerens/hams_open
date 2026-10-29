// SPDX-License-Identifier: AGPL-3.0-or-later
//! `tdr`: sweeps S21 over 2 to 200 MHz in 100 points (a grid that starts at its step, as low-pass mode needs), then reports the
//! delay of the path twice: from the slope of the unwrapped phase, and from the peak of the low-pass impulse response (Hamming
//! and rectangular windows) and band-pass response, so the two methods can be checked against each other.
use vnwa_open::sweep::{Config, Vna};
use vnwa_open::timedomain::{peak_time, transform, Mode, Response, Window};

fn main() -> Result<(), String> {
    let cfg = Config {
        if_hz: 4000.0,
        ..Config::default()
    };
    let mut vna = Vna::open(cfg)?;
    let points = vna.sweep(2e6, 200e6, 100)?;
    let f: Vec<f64> = points.iter().map(|p| p.freq_hz).collect();
    // The driver reports phase in the vendor program's convention, which rises with delay; the transform wants the standard one
    // (a delay tau gives e^(-j 2 pi f tau)), the complex conjugate.
    let s: Vec<_> = points
        .iter()
        .map(|p| vnwa_open::audio::Complex {
            re: p.s21.re,
            im: -p.s21.im,
        })
        .collect();
    // Unwrapped phase and a least-squares slope.
    let mut ph = Vec::new();
    let mut prev = s[0].phase_deg().to_radians();
    let mut acc = prev;
    ph.push(acc);
    for z in &s[1..] {
        let p = z.phase_deg().to_radians();
        let mut d = p - prev;
        while d > std::f64::consts::PI {
            d -= 2.0 * std::f64::consts::PI;
        }
        while d < -std::f64::consts::PI {
            d += 2.0 * std::f64::consts::PI;
        }
        acc += d;
        prev = p;
        ph.push(acc);
    }
    let n = f.len() as f64;
    let (mf, mp) = (f.iter().sum::<f64>() / n, ph.iter().sum::<f64>() / n);
    let slope = f
        .iter()
        .zip(&ph)
        .map(|(x, y)| (x - mf) * (y - mp))
        .sum::<f64>()
        / f.iter().map(|x| (x - mf) * (x - mf)).sum::<f64>();
    println!(
        "delay from the phase slope: {:.3} ns",
        -slope / (2.0 * std::f64::consts::PI) * 1e9
    );
    let t: Vec<f64> = (0..800).map(|i| i as f64 * 0.25e-9).collect();
    for (name, w, m) in [
        ("low-pass rectangular", Window::Rectangular, Mode::LowPass),
        ("low-pass Hamming", Window::Hamming, Mode::LowPass),
        ("band-pass Hamming", Window::Hamming, Mode::BandPass),
    ] {
        let h = transform(&f, &s, w, m, Response::Impulse, &t)?;
        println!(
            "{name}: peak at {:.3} ns",
            peak_time(&t, &h).ok_or("empty")? * 1e9
        );
    }
    Ok(())
}

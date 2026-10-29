// SPDX-License-Identifier: AGPL-3.0-or-later
//! Time-domain transforms of a measured frequency response, as the vendor's Help file describes them: the impulse response and the
//! step response reconstructed from S11 or S21, with the windowing functions rectangular, Hamming, Hanning, Blackman and
//! Blackman squared, in low-pass and band-pass mode, by a direct discrete Fourier transform at the requested times.
//!
//! The windows are the textbook definitions (the Help file shows only plots): over the used frequency span of `n` points,
//! `w(k) = a0 - a1 cos(2 pi k / (n - 1)) + a2 cos(4 pi k / (n - 1))` with Hamming (0.54, 0.46, 0), Hanning (0.5, 0.5, 0) and
//! Blackman (0.42, 0.5, 0.08); Blackman squared is the square of Blackman. Low-pass mode uses the response's symmetry
//! (`S(-f) = conj S(f)`) to give a real time response over the frequency span mirrored about zero, with the window peak at zero
//! frequency; the data must then reach (nearly) DC: a grid that starts at a multiple of its step is extended down to DC with the
//! first magnitude held and its phase taken linearly to zero (the Help file's "low frequency data extrapolated"). Band-pass mode uses the positive frequencies
//! only, so the result is complex (its magnitude is what is used), with the window peak at the centre of the span.

//! The data must be in the standard convention, where a delay `tau` gives `e^(-j 2 pi f tau)`. The driver reports phase in the vendor
//! program's convention, which is the complex conjugate (`audio::vendor_convention`), so conjugate sweep results first.

use crate::audio::Complex;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Window {
    Rectangular,
    Hamming,
    Hanning,
    Blackman,
    BlackmanSquared,
}

impl Window {
    pub fn from_name(s: &str) -> Result<Window, String> {
        match s.to_lowercase().as_str() {
            "rectangular" | "rect" | "none" => Ok(Window::Rectangular),
            "hamming" => Ok(Window::Hamming),
            "hanning" | "hann" => Ok(Window::Hanning),
            "blackman" => Ok(Window::Blackman),
            "blackman2" | "blackman_squared" => Ok(Window::BlackmanSquared),
            o => Err(format!(
                "unknown window {o:?} (rectangular, hamming, hanning, blackman, blackman_squared)"
            )),
        }
    }

    /// The weight of point `k` of `n` (0 to n-1), 0 at the first and last for Hanning and Blackman.
    pub fn weight(self, k: usize, n: usize) -> f64 {
        // [@ANCHOR: vnwa_open_window]
        if n < 2 {
            return 1.0;
        }
        let x = 2.0 * PI * k as f64 / (n - 1) as f64;
        match self {
            Window::Rectangular => 1.0,
            Window::Hamming => 0.54 - 0.46 * x.cos(),
            Window::Hanning => 0.5 - 0.5 * x.cos(),
            Window::Blackman => 0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos(),
            Window::BlackmanSquared => {
                let b = 0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos();
                b * b
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    LowPass,
    BandPass,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Response {
    Impulse,
    Step,
}

/// A uniform frequency grid: `f = start + k step`.
fn check_grid(freqs: &[f64]) -> Result<(f64, f64), String> {
    if freqs.len() < 4 {
        return Err("a time-domain transform needs at least four points".into());
    }
    let step = (freqs[freqs.len() - 1] - freqs[0]) / (freqs.len() - 1) as f64;
    if step.is_nan()
        || step <= 0.0
        || freqs
            .windows(2)
            .any(|w| ((w[1] - w[0]) - step).abs() > step * 1e-6)
    {
        return Err("the frequency grid must be uniform and rising".into());
    }
    Ok((freqs[0], step))
}

/// The time response at each of `times_s`. For `Response::Step` only low-pass mode applies (the Help file: band-pass mode cannot give
/// a step response); the step is the running integral of the impulse response over `times_s`, which must then be a uniform grid
/// starting at zero or earlier.
pub fn transform(
    freqs: &[f64],
    data: &[Complex],
    window: Window,
    mode: Mode,
    response: Response,
    times_s: &[f64],
) -> Result<Vec<Complex>, String> {
    // [@ANCHOR: vnwa_open_time_domain]
    if freqs.len() != data.len() {
        return Err("frequency and data lengths differ".into());
    }
    let (f0, step) = check_grid(freqs)?;
    if response == Response::Step && mode == Mode::BandPass {
        return Err("a step response needs low-pass mode".into());
    }
    // The spectrum used: (frequency, value, window index) for the positive side, extended to DC in low-pass mode.
    let mut pos: Vec<(f64, Complex)> = Vec::new();
    if mode == Mode::LowPass {
        let k0 = f0 / step;
        if (k0 - k0.round()).abs() > 1e-6 {
            return Err(format!(
                "low-pass mode needs the grid to start at a multiple of its {step} Hz step (starts at {f0} Hz)"
            ));
        }
        // Below the first point the magnitude is held and the phase falls linearly to zero at DC (exact for a delay line), so the
        // DC value is real as a real time response needs.
        let (mag, phase) = (data[0].abs(), data[0].im.atan2(data[0].re));
        for k in 0..(k0.round() as usize) {
            let p = phase * (k as f64 / k0);
            pos.push((
                k as f64 * step,
                Complex {
                    re: mag * p.cos(),
                    im: mag * p.sin(),
                },
            ));
        }
    }
    pos.extend(freqs.iter().copied().zip(data.iter().copied()));
    let n_pos = pos.len();
    // Window index: low-pass spans -fmax..fmax (2 n - 1 points, peak at DC); band-pass spans the positive points.
    let (n_win, offset) = match mode {
        Mode::LowPass => (2 * n_pos - 1, n_pos - 1),
        Mode::BandPass => (n_pos, 0),
    };
    let mut out = Vec::with_capacity(times_s.len());
    for &t in times_s {
        let mut sum = Complex { re: 0.0, im: 0.0 };
        for (i, (f, s)) in pos.iter().enumerate() {
            let w = window.weight(offset + i, n_win);
            let (c, sn) = ((2.0 * PI * f * t).cos(), (2.0 * PI * f * t).sin());
            let e = Complex { re: c, im: sn };
            sum = sum
                + Complex {
                    re: w * s.re,
                    im: w * s.im,
                } * e;
            if mode == Mode::LowPass && i > 0 {
                // the mirrored negative frequency: conj(S) e^(-j 2 pi f t) with the window of the mirrored index
                let wm = window.weight(offset - i, n_win);
                let em = Complex { re: c, im: -sn };
                let sc = Complex {
                    re: s.re,
                    im: -s.im,
                };
                sum = sum
                    + Complex {
                        re: wm * sc.re,
                        im: wm * sc.im,
                    } * em;
            }
        }
        // Scale so a flat response of 1 gives an impulse whose integral is 1 (the sum of the weights times the step).
        out.push(Complex {
            re: sum.re * step,
            im: sum.im * step,
        });
    }
    if response == Response::Step {
        if times_s.len() < 2 {
            return Err("a step response needs a grid of times".into());
        }
        let dt = times_s[1] - times_s[0];
        let mut acc = 0.0;
        for z in out.iter_mut() {
            acc += z.re * dt;
            *z = Complex { re: acc, im: 0.0 };
        }
    }
    Ok(out)
}

/// The delay of the largest peak of `|z|`, by parabolic interpolation around it.
pub fn peak_time(times_s: &[f64], z: &[Complex]) -> Option<f64> {
    let (i, _) = z
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))?;
    if i == 0 || i + 1 >= z.len() {
        return Some(times_s[i]);
    }
    let (a, b, c) = (z[i - 1].abs(), z[i].abs(), z[i + 1].abs());
    let denom = a - 2.0 * b + c;
    let shift = if denom.abs() > 1e-30 {
        0.5 * (a - c) / denom
    } else {
        0.0
    };
    Some(times_s[i] + shift * (times_s[1] - times_s[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(n: usize, step: f64, first: usize) -> Vec<f64> {
        (first..first + n).map(|k| k as f64 * step).collect()
    }

    fn times(n: usize, dt: f64, start: f64) -> Vec<f64> {
        (0..n).map(|i| start + i as f64 * dt).collect()
    }

    // Tests [@ANCHOR: vnwa_open_window]
    #[test]
    fn the_windows_have_their_textbook_shapes() {
        let n = 11;
        for w in [Window::Hanning, Window::Blackman, Window::BlackmanSquared] {
            assert!(
                w.weight(0, n).abs() < 1e-12 && w.weight(n - 1, n).abs() < 1e-12,
                "{w:?} ends at 0"
            );
            assert!((w.weight(5, n) - 1.0).abs() < 1e-12, "{w:?} peaks at 1");
        }
        assert!((Window::Hamming.weight(0, n) - 0.08).abs() < 1e-12);
        assert!((Window::Hamming.weight(5, n) - 1.0).abs() < 1e-12);
        assert_eq!(Window::Rectangular.weight(3, n), 1.0);
        assert!(
            (Window::BlackmanSquared.weight(2, n) - Window::Blackman.weight(2, n).powi(2)).abs()
                < 1e-15
        );
        assert_eq!(Window::from_name("Hann").unwrap(), Window::Hanning);
        assert!(Window::from_name("kaiser").is_err());
    }

    // Tests [@ANCHOR: vnwa_open_time_domain]
    #[test]
    fn a_delay_line_gives_a_peak_at_its_delay_in_both_modes_and_windows_lower_the_sidelobes() {
        let tau = 10.3e-9;
        let step = 2e6;
        let f = grid(100, step, 1); // 2 to 200 MHz, starts at one step: extended to DC
        let s: Vec<Complex> = f
            .iter()
            .map(|x| Complex {
                re: (2.0 * PI * x * tau).cos(),
                im: -(2.0 * PI * x * tau).sin(),
            })
            .collect();
        let t = times(400, 0.25e-9, 0.0);
        for mode in [Mode::LowPass, Mode::BandPass] {
            for w in [
                Window::Rectangular,
                Window::Hamming,
                Window::Hanning,
                Window::Blackman,
            ] {
                let h = transform(&f, &s, w, mode, Response::Impulse, &t).unwrap();
                let at = peak_time(&t, &h).unwrap();
                assert!((at - tau).abs() < 0.15e-9, "{mode:?} {w:?}: peak at {at}");
            }
        }
        // Sidelobe level 20 ns away from the peak (low pass): the window lowers it.
        let level = |w| {
            let h = transform(&f, &s, w, Mode::LowPass, Response::Impulse, &t).unwrap();
            let peak = h.iter().map(|z| z.abs()).fold(0.0, f64::max);
            let far: f64 = h[(40e-9 / 0.25e-9) as usize..]
                .iter()
                .map(|z| z.abs())
                .fold(0.0, f64::max);
            far / peak
        };
        assert!(
            level(Window::Blackman) < level(Window::Rectangular) / 5.0,
            "{} {}",
            level(Window::Blackman),
            level(Window::Rectangular)
        );
        // Low pass gives a real response; band pass does not.
        let h = transform(
            &f,
            &s,
            Window::Hamming,
            Mode::LowPass,
            Response::Impulse,
            &t,
        )
        .unwrap();
        let peak = h.iter().map(|z| z.abs()).fold(0.0, f64::max);
        assert!(h.iter().all(|z| z.im.abs() < 1e-9 * peak));
    }

    #[test]
    fn a_reflection_step_response_settles_at_the_reflection_coefficient() {
        // A constant reflection of 0.5 at 6 ns (a mismatch): the step response is 0 before and 0.5 after.
        let tau = 6e-9;
        let step = 2e6;
        let f = grid(250, step, 1);
        let s: Vec<Complex> = f
            .iter()
            .map(|x| Complex {
                re: 0.5 * (2.0 * PI * x * tau).cos(),
                im: -0.5 * (2.0 * PI * x * tau).sin(),
            })
            .collect();
        let t = times(500, 0.2e-9, 0.0);
        let h = transform(&f, &s, Window::Hamming, Mode::LowPass, Response::Step, &t).unwrap();
        assert!(h[10].re.abs() < 0.02, "before the reflection: {}", h[10].re);
        let late = h[200].re;
        assert!((late - 0.5).abs() < 0.03, "after it: {late}");
    }

    #[test]
    fn bad_input_is_refused() {
        let f = grid(10, 1e6, 1);
        let s = vec![Complex { re: 1.0, im: 0.0 }; 10];
        let t = times(10, 1e-9, 0.0);
        assert!(transform(
            &f[..3],
            &s[..3],
            Window::Hamming,
            Mode::LowPass,
            Response::Impulse,
            &t
        )
        .is_err());
        assert!(transform(
            &f,
            &s[..9],
            Window::Hamming,
            Mode::LowPass,
            Response::Impulse,
            &t
        )
        .is_err());
        assert!(
            transform(&f, &s, Window::Hamming, Mode::BandPass, Response::Step, &t)
                .unwrap_err()
                .contains("low-pass")
        );
        let skew = grid(10, 1e6, 1)
            .iter()
            .map(|x| x + 0.4e6)
            .collect::<Vec<_>>();
        assert!(transform(
            &skew,
            &s,
            Window::Hamming,
            Mode::LowPass,
            Response::Impulse,
            &t
        )
        .unwrap_err()
        .contains("multiple"));
        let mut bumpy = f.clone();
        bumpy[4] += 0.3e6;
        assert!(transform(
            &bumpy,
            &s,
            Window::Hamming,
            Mode::LowPass,
            Response::Impulse,
            &t
        )
        .unwrap_err()
        .contains("uniform"));
    }
}

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
        // [@ANCHOR: vnwa_open_window_from_name]
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
    // [@ANCHOR: vnwa_open_time_domain_check_grid]
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

/// Time-domain gating: keeps only what arrives between `t_start_s` and `t_stop_s` and returns the frequency response of that part, on the
/// same frequency grid. The impulse response is taken with a rectangular frequency window (so nothing needs undoing afterwards), on a
/// time grid that spans one whole period of the grid (`1 / step`, signed about zero), multiplied by a gate shaped by `shape` between the
/// two times (zero outside), and transformed back with the inverse of the same sum. With a rectangular gate of the whole period the
/// input comes back unchanged. `mode` is as for [`transform`]; low-pass mode gives a real response, so the gate times are signed
/// delays and a negative `t_start_s` keeps the part before zero.
pub fn gate(
    freqs: &[f64],
    data: &[Complex],
    mode: Mode,
    t_start_s: f64,
    t_stop_s: f64,
    shape: Window,
) -> Result<Vec<Complex>, String> {
    // [@ANCHOR: vnwa_open_time_domain_gate]
    if !(t_start_s.is_finite() && t_stop_s.is_finite() && t_start_s < t_stop_s) {
        return Err("the gate must start before it stops".into());
    }
    let (f0, step) = check_grid(freqs)?;
    // One whole period holds `m` samples, enough to hold the mirrored low-pass spectrum (or the band-pass one) without aliasing.
    let top = ((freqs[freqs.len() - 1] / step).round() as usize).max(1);
    let m = (2 * top + 2).max(freqs.len() * 2);
    let period = 1.0 / step;
    let dt = period / m as f64;
    let times: Vec<f64> = (0..m)
        .map(|i| {
            let k = if i > m / 2 {
                i as f64 - m as f64
            } else {
                i as f64
            };
            k * dt
        })
        .collect();
    let h = transform(
        freqs,
        data,
        Window::Rectangular,
        mode,
        Response::Impulse,
        &times,
    )?;
    let mut kept = vec![Complex { re: 0.0, im: 0.0 }; m];
    let span = t_stop_s - t_start_s;
    // Sample positions inside the gate get the shape's weight across the gate, found by scaling the window over a fine index range.
    const FINE: usize = 1 << 16;
    for (i, (&t, z)) in times.iter().zip(&h).enumerate() {
        if t >= t_start_s && t <= t_stop_s {
            let u = (t - t_start_s) / span;
            let g = shape.weight((u * (FINE - 1) as f64).round() as usize, FINE);
            kept[i] = Complex {
                re: z.re * g,
                im: z.im * g,
            };
        }
    }
    let _ = f0;
    Ok(freqs
        .iter()
        .map(|&f| {
            let mut sum = Complex { re: 0.0, im: 0.0 };
            for (&t, z) in times.iter().zip(&kept) {
                let e = Complex {
                    re: (2.0 * PI * f * t).cos(),
                    im: -(2.0 * PI * f * t).sin(),
                };
                sum = sum + *z * e;
            }
            Complex {
                re: sum.re * dt,
                im: sum.im * dt,
            }
        })
        .collect())
}

/// The delay of the largest peak of `|z|`, by parabolic interpolation around it.
pub fn peak_time(times_s: &[f64], z: &[Complex]) -> Option<f64> {
    // [@ANCHOR: vnwa_open_time_domain_peak_time]
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
    // Tests [@ANCHOR: vnwa_open_window_from_name]
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
    // Tests [@ANCHOR: vnwa_open_time_domain_peak_time]
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

    // Tests [@ANCHOR: vnwa_open_time_domain_gate]
    #[test]
    fn a_gate_around_the_near_reflection_removes_the_far_one_and_a_whole_period_gate_changes_nothing(
    ) {
        let step = 2e6;
        let f = grid(250, step, 1);
        let two = |x: f64| Complex {
            re: 0.5 * (2.0 * PI * x * 6e-9).cos() + 0.3 * (2.0 * PI * x * 40e-9).cos(),
            im: -0.5 * (2.0 * PI * x * 6e-9).sin() - 0.3 * (2.0 * PI * x * 40e-9).sin(),
        };
        let near = |x: f64| Complex {
            re: 0.5 * (2.0 * PI * x * 6e-9).cos(),
            im: -0.5 * (2.0 * PI * x * 6e-9).sin(),
        };
        let s: Vec<Complex> = f.iter().map(|x| two(*x)).collect();
        for mode in [Mode::LowPass, Mode::BandPass] {
            let g = gate(&f, &s, mode, 1e-9, 20e-9, Window::Rectangular).unwrap();
            // Away from the band edges (the gate and the band limit smear the ends) the gated response is the near reflection alone.
            let worst = f
                .iter()
                .zip(&g)
                .skip(20)
                .take(210)
                .map(|(x, z)| {
                    let w = near(*x);
                    ((z.re - w.re).powi(2) + (z.im - w.im).powi(2)).sqrt()
                })
                .fold(0.0, f64::max);
            assert!(worst < 0.1, "{mode:?}: worst error {worst}");
            // A Hanning gate centred on the reflection (peak weight 1 at 6 ns) keeps its size too.
            let h = gate(&f, &s, mode, 0.0, 12e-9, Window::Hanning).unwrap();
            let mid = h[100].abs();
            assert!(
                (mid - 0.5).abs() < 0.12,
                "{mode:?}: Hanning-gated size {mid}"
            );
            let ungated: f64 = s
                .iter()
                .skip(20)
                .take(210)
                .map(|z| (z.re - near(0.0).re).abs())
                .sum();
            assert!(ungated > 0.0);
        }
        let all = gate(
            &f,
            &s,
            Mode::LowPass,
            -1.0 / step,
            1.0 / step,
            Window::Rectangular,
        )
        .unwrap();
        let worst = s
            .iter()
            .zip(&all)
            .map(|(a, b)| ((a.re - b.re).powi(2) + (a.im - b.im).powi(2)).sqrt())
            .fold(0.0, f64::max);
        assert!(worst < 1e-6, "whole-period gate: {worst}");
        assert!(gate(&f, &s, Mode::LowPass, 5e-9, 5e-9, Window::Rectangular).is_err());
    }

    // Tests [@ANCHOR: vnwa_open_time_domain_check_grid]
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

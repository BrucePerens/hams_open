// SPDX-License-Identifier: AGPL-3.0-or-later
//! From codec samples to a complex measurement. At each frequency the VNWA mixes the signal down to an IF (12, 6 or 4
//! kHz seen, i.e. fs / (4 n) with n = 1, 2, 3 at 48 kHz) and the PC finds its amplitude and phase from whole IF periods
//! of the audio, skipping the presamples after a frequency change and the postsamples before the next one. The right
//! channel is the reference on one codec and the left channel on the other; the measurement is the other channel divided by it.
//! (The vendor's Help file says one IF period is the minimum; this code uses as many whole periods as fit, which for a
//! rectangular window and whole periods is the exact least-squares amplitude and phase of a tone.)

use std::f64::consts::PI;

/// A complex number (no dependency for this).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}

impl Complex {
    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }

    /// The angle in degrees, -180 to 180.
    pub fn phase_deg(self) -> f64 {
        self.im.atan2(self.re).to_degrees()
    }

    pub fn divided_by(self, o: Complex) -> Complex {
        let d = o.re * o.re + o.im * o.im;
        Complex {
            re: (self.re * o.re + self.im * o.im) / d,
            im: (self.im * o.re - self.re * o.im) / d,
        }
    }
}

/// The complex amplitude of the component at `if_hz` in `x` (a cosine of amplitude A and phase p gives `A e^(jp)`).
/// Exact when `x` holds a whole number of IF periods.
pub fn tone_at_if(x: &[f64], fs: f64, if_hz: f64) -> Complex {
    // [@ANCHOR: vnwa_open_tone_at_if]
    let w = 2.0 * PI * if_hz / fs;
    let (mut re, mut im) = (0.0, 0.0);
    for (n, v) in x.iter().enumerate() {
        let a = w * n as f64;
        re += v * a.cos();
        im -= v * a.sin();
    }
    let k = 2.0 / x.len() as f64;
    Complex {
        re: re * k,
        im: im * k,
    }
}

/// How many samples to analyse out of one point's `len` samples: whole IF periods between the presamples and
/// the postsamples. `None` when not even one period fits (the vendor program refuses to sweep in that case).
pub fn analysis_span(
    len: usize,
    fs: f64,
    if_hz: f64,
    presamples: usize,
    postsamples: usize,
) -> Option<(usize, usize)> {
    let per = fs / if_hz;
    let usable = len.checked_sub(presamples + postsamples)?;
    let periods = (usable as f64 / per).floor();
    if periods < 1.0 {
        None
    } else {
        Some((presamples, (periods * per).round() as usize))
    }
}

/// The vendor program's convention for the same measurement: the complex conjugate of `measure_point`'s value (its phase
/// has the opposite sign, as it would for a mixer whose LO is above the RF). Established against the vendor program's
/// own S21, see the reference test below.
pub fn vendor_convention(c: Complex) -> Complex {
    // [@ANCHOR: vnwa_open_vendor_convention]
    Complex {
        re: c.re,
        im: -c.im,
    }
}

/// Which channel of a codec carries the reference signal (the vendor program's "Reference = Right Channel" setting). On the
/// observed unit the second codec has it on the right and the first codec on the left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reference {
    Left,
    Right,
}

/// The measurement of one point: the measured channel's tone divided by the reference channel's, at the IF.
/// `min_reference` is the smallest reference amplitude (in sample counts) that is accepted as a signal.
#[allow(clippy::too_many_arguments)]
pub fn measure_point(
    left: &[f64],
    right: &[f64],
    fs: f64,
    if_hz: f64,
    presamples: usize,
    postsamples: usize,
    reference: Reference,
    min_reference: f64,
) -> Result<Complex, String> {
    // [@ANCHOR: vnwa_open_measure_point]
    if left.len() != right.len() {
        return Err("the two channels have different lengths".into());
    }
    let (start, n) = analysis_span(left.len(), fs, if_hz, presamples, postsamples).ok_or_else(|| {
        format!("{} samples do not hold one IF period plus {presamples} presamples and {postsamples} postsamples", left.len())
    })?;
    let l = tone_at_if(&left[start..start + n], fs, if_hz);
    let r = tone_at_if(&right[start..start + n], fs, if_hz);
    let (measured, reference_tone) = match reference {
        Reference::Right => (l, r),
        Reference::Left => (r, l),
    };
    if reference_tone.abs() < min_reference {
        return Err(format!(
            "reference amplitude {:.2} is below {min_reference}: no signal",
            reference_tone.abs()
        ));
    }
    Ok(measured.divided_by(reference_tone))
}

#[cfg(test)]
mod tests {
    use super::*;

    include!("audio_vectors.rs");
    include!("reference_vectors.rs");
    include!("if_reference_vectors.rs");
    include!("level_vectors.rs");

    fn tone(a: f64, phase_deg: f64, fs: f64, f: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| a * (2.0 * PI * f * i as f64 / fs + phase_deg.to_radians()).cos())
            .collect()
    }

    // Tests [@ANCHOR: vnwa_open_tone_at_if]
    #[test]
    fn amplitude_and_phase_of_a_synthetic_tone_are_exact() {
        for &(f, n) in &[(12_000.0, 48usize), (6_000.0, 96), (4_000.0, 96)] {
            let c = tone_at_if(&tone(0.8, 30.0, 48_000.0, f, n), 48_000.0, f);
            assert!((c.abs() - 0.8).abs() < 1e-12, "{f}");
            assert!((c.phase_deg() - 30.0).abs() < 1e-9, "{f}");
        }
    }

    // Tests [@ANCHOR: vnwa_open_measure_point]
    #[test]
    fn the_ratio_of_two_tones_is_their_amplitude_ratio_and_phase_difference() {
        let l = tone(0.5, 100.0, 48_000.0, 12_000.0, 40);
        let r = tone(0.25, 40.0, 48_000.0, 12_000.0, 40);
        // 40 samples, 2 pre and 2 post, leaves 36 = nine 4-sample periods.
        let m = measure_point(&l, &r, 48_000.0, 12_000.0, 2, 2, Reference::Right, 0.01).unwrap();
        assert!((m.abs() - 2.0).abs() < 1e-12);
        assert!((m.phase_deg() - 60.0).abs() < 1e-9);
    }

    fn complex_of(&(re, im): &(f64, f64)) -> Complex {
        Complex { re, im }
    }

    fn mine(chunk: &[[i16; 2]]) -> Complex {
        let l: Vec<f64> = chunk.iter().map(|f| f64::from(f[0])).collect();
        let r: Vec<f64> = chunk.iter().map(|f| f64::from(f[1])).collect();
        vendor_convention(
            measure_point(&l, &r, 48_000.0, 4_000.0, 0, 0, Reference::Right, 100.0).unwrap(),
        )
    }

    // Tests [@ANCHOR: vnwa_open_vendor_convention]
    #[test]
    fn against_the_vendor_programs_own_s21_in_a_sweep_laid_out_in_time() {
        // The loopback path is flat in frequency (the vendor's single-point S21 is the same from 1 to 30 MHz), and the
        // values drift with time over a sweep (settling). Laying my windows out at k * 60 ms like the vendor's points gives
        // a match to about 1.5 percent rms, scale 0.995, phase offset 0.8 degrees (measured when this was written).
        let m: Vec<Complex> = SWEEP_CHUNKS.iter().map(|c| mine(c)).collect();
        let v: Vec<Complex> = VENDOR_S21.iter().map(complex_of).collect();
        let (mut num_re, mut num_im, mut den) = (0.0, 0.0, 0.0);
        for (m, v) in m.iter().zip(&v) {
            num_re += m.re * v.re + m.im * v.im;
            num_im += m.re * v.im - m.im * v.re;
            den += m.re * m.re + m.im * m.im;
        }
        let a = Complex {
            re: num_re / den,
            im: num_im / den,
        };
        let (mut err, mut norm) = (0.0, 0.0);
        for (m, v) in m.iter().zip(&v) {
            let (re, im) = (a.re * m.re - a.im * m.im, a.re * m.im + a.im * m.re);
            err += (v.re - re).powi(2) + (v.im - im).powi(2);
            norm += v.re * v.re + v.im * v.im;
        }
        let residual = (err / norm).sqrt();
        assert!((0.98..1.02).contains(&a.abs()), "scale {}", a.abs());
        assert!(a.phase_deg().abs() < 2.0, "phase offset {}", a.phase_deg());
        assert!(residual < 0.03, "residual {residual}");
    }

    #[test]
    fn against_the_vendor_programs_own_s21_at_four_single_frequencies() {
        for p in POINTS.iter() {
            let (f, m, v) = (p.f_mhz, mine(&p.chunk), complex_of(&p.vendor));
            let diff = Complex {
                re: m.re - v.re,
                im: m.im - v.im,
            }
            .abs()
                / v.abs();
            // Measured 0.7 to 0.9 percent at 1, 10, 20 and 30 MHz.
            assert!(diff < 0.02, "{f} MHz: relative difference {diff}");
        }
    }

    // Tests [@ANCHOR: vnwa_open_vendor_convention]
    #[test]
    fn the_reference_channel_option_swaps_numerator_and_denominator() {
        let l = tone(0.5, 100.0, 48_000.0, 12_000.0, 40);
        let r = tone(0.25, 40.0, 48_000.0, 12_000.0, 40);
        let a = measure_point(&l, &r, 48_000.0, 12_000.0, 0, 0, Reference::Right, 0.01).unwrap();
        let b = measure_point(&l, &r, 48_000.0, 12_000.0, 0, 0, Reference::Left, 0.01).unwrap();
        assert!((a.abs() - 2.0).abs() < 1e-12 && (b.abs() - 0.5).abs() < 1e-12);
        assert!((a.phase_deg() + b.phase_deg()).abs() < 1e-9);
    }

    #[test]
    fn at_other_ifs_both_codecs_match_the_vendor_programs_s21_with_their_reference_channels() {
        // The unit's own response differs by IF (|S21| 0.874 at 172.3 degrees at 12 kHz, 0.799 at 174.7 at 6 kHz, 0.782 at
        // 176.2 at 4 kHz), so this is a check that is not flat. Measured when written: 0.1 to 0.3 percent (codec 1, reference
        // on the left) and 0.5 to 1.0 percent (codec 2, reference on the right).
        for r in IF_REFS.iter() {
            let v = complex_of(&r.vendor);
            for (chunk, reference, limit) in [
                (&r.codec1, Reference::Left, 0.01),
                (&r.codec2, Reference::Right, 0.02),
            ] {
                let l: Vec<f64> = chunk.iter().map(|f| f64::from(f[0])).collect();
                let rr: Vec<f64> = chunk.iter().map(|f| f64::from(f[1])).collect();
                let m = vendor_convention(
                    measure_point(&l, &rr, 48_000.0, r.if_hz, 0, 0, reference, 100.0).unwrap(),
                );
                let diff = Complex {
                    re: m.re - v.re,
                    im: m.im - v.im,
                }
                .abs()
                    / v.abs();
                assert!(
                    diff < limit,
                    "{} Hz {reference:?}: relative difference {diff}",
                    r.if_hz
                );
                assert!(
                    (m.phase_deg() - v.phase_deg()).abs() < 1.0,
                    "{} Hz {reference:?}: phase",
                    r.if_hz
                );
            }
        }
    }

    #[test]
    fn the_ratio_does_not_depend_on_the_signal_level_over_24_db() {
        // Real samples at five TX levels (reference amplitude about 17000 down to about 1000 counts): the ratio stays within 0.3
        // percent and 0.2 degrees (measured: 0.7876 to 0.7882, 176.13 to 176.18 degrees; the vendor program's own values scatter
        // by 1 to 2.5 percent at the lower levels).
        let m: Vec<Complex> = LEVEL_CHUNKS.iter().map(|c| mine(c)).collect();
        for z in &m {
            assert!(
                (z.abs() - m[0].abs()).abs() / m[0].abs() < 0.003,
                "{} against {}",
                z.abs(),
                m[0].abs()
            );
            assert!(
                (z.phase_deg() - m[0].phase_deg()).abs() < 0.2,
                "{} against {}",
                z.phase_deg(),
                m[0].phase_deg()
            );
        }
    }

    #[test]
    fn too_short_or_silent_points_are_refused() {
        let x = tone(1.0, 0.0, 48_000.0, 12_000.0, 8);
        assert!(measure_point(&x, &x, 48_000.0, 12_000.0, 2, 3, Reference::Right, 0.01).is_err()); // 8 - 5 = 3 < one 4-sample period
        assert!(measure_point(&x, &x, 48_000.0, 12_000.0, 2, 2, Reference::Right, 0.01).is_ok()); // exactly one period
        let silent = vec![0.0; 40];
        assert!(measure_point(
            &x.repeat(5),
            &silent,
            48_000.0,
            12_000.0,
            0,
            0,
            Reference::Right,
            0.5
        )
        .unwrap_err()
        .contains("no signal"));
        assert!(
            measure_point(&x, &x[..7], 48_000.0, 12_000.0, 0, 0, Reference::Right, 0.0).is_err()
        );
    }

    #[test]
    fn real_loopback_samples_give_the_independently_computed_ratio_and_the_expected_phase() {
        for e in EXCERPTS {
            let l: Vec<f64> = e.left.iter().map(|&v| f64::from(v)).collect();
            let r: Vec<f64> = e.right.iter().map(|&v| f64::from(v)).collect();
            let m = measure_point(&l, &r, 48_000.0, e.if_hz, 0, 0, Reference::Right, 10.0).unwrap();
            assert!(
                (m.re - e.ratio_re).abs() < 1e-9 && (m.im - e.ratio_im).abs() < 1e-9,
                "{}",
                e.name
            );
            // TX connected to RX: the two channels are about 180 degrees apart and within 30 percent in amplitude.
            assert!(m.phase_deg().abs() > 170.0, "{}: {}", e.name, m.phase_deg());
            assert!((0.8..1.4).contains(&m.abs()), "{}: {}", e.name, m.abs());
        }
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! The VNWA as a rudimentary spectrum analyzer (the vendor's "Spectrum Analyzer" mode). Sources: usbmon captures of the vendor program in that mode and
//! measurements on the unit (`docs/PROTOCOL.md`, "Spectrum analyzer mode"); the Help file for what the mode is.
//!
//! What a point is: the unit's receive mixer is fed by the LO chip, and the TX chip (the "tracking generator") is `IF` below it. The set-up block carries one
//! word, the TX frequency, and the LO is that word plus the IF (12 kHz in SA mode), so to analyse at `f` the word is `f - IF` and the LO sits at `f`: the
//! receiver is direct conversion, and what arrives at the RX port within the codec's band of `f` is in the audio of the S21 codec's left channel. With the TX
//! level 0 the tracking generator is silent and the audio is only what the RX input carries; with it on, a loopback shows the TX as a line at the IF. The
//! audio is real, so a signal at `f + d` and one at `f - d` fall on the same audio frequency: a point is the power in a band around the LO, not a one-sided line.
//!
//! Not verified: amplitude (the result is in codec counts, not dBm), the ghost lines and subharmonic responses the Help file warns of, and the dynamic range:
//! no calibrated external tone was available.

use std::time::Duration;

use crate::capture::SAMPLE_RATE;
use crate::setup::{setup_block, SetupParams};
use crate::sweep::{recover, Vna};

/// The IF of the vendor's SA mode (the TX chip sits this far below the LO).
pub const SA_IF_HZ: f64 = 12_000.0;
/// The audio below this is ignored (the codec is AC coupled: a notch at DC, and its offset).
pub const DC_GUARD_HZ: f64 = 100.0;
/// The widest band the audio carries (the codec's Nyquist is 24 kHz).
pub const MAX_BAND_HZ: f64 = 20_000.0;

/// One point of a spectrum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SaPoint {
    /// The LO frequency of the point.
    pub freq_hz: f64,
    /// The rms of the audio in the band from `DC_GUARD_HZ` to the requested band, in codec counts.
    pub rms: f64,
}

/// The rms of `x` (48 kHz audio) between `lo_hz` and `hi_hz`, from a Hann-windowed DFT (Parseval over the bins in the band).
pub fn band_rms(x: &[f64], lo_hz: f64, hi_hz: f64) -> f64 {
    // [@ANCHOR: vnwa_open_sa_band_rms]
    let n = x.len();
    let fs = f64::from(SAMPLE_RATE);
    let mean = x.iter().sum::<f64>() / n as f64;
    let win: Vec<f64> = (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos())
        .collect();
    let wpow = win.iter().map(|w| w * w).sum::<f64>() / n as f64;
    let (k0, k1) = (
        ((lo_hz * n as f64 / fs).ceil() as usize).max(1),
        ((hi_hz * n as f64 / fs).floor() as usize).min(n / 2 - 1),
    );
    let mut power = 0.0;
    for k in k0..=k1 {
        let w = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
        let (mut re, mut im) = (0.0, 0.0);
        for (i, v) in x.iter().enumerate() {
            let s = (v - mean) * win[i];
            re += s * (w * i as f64).cos();
            im -= s * (w * i as f64).sin();
        }
        // One-sided bin: the power of a real signal at this frequency is 2 |X|^2 / (N^2 * mean(w^2)).
        power += 2.0 * (re * re + im * im) / (n as f64 * n as f64 * wpow);
    }
    power.sqrt()
}

/// Checks the settings of a spectrum and returns the point spacing in hertz.
pub fn spectrum_plan(
    start_hz: f64,
    stop_hz: f64,
    points: usize,
    band_hz: f64,
    tracking_level: u16,
) -> Result<f64, String> {
    // [@ANCHOR: vnwa_open_sa_spectrum]
    if points < 2 {
        return Err("a spectrum needs at least 2 points".into());
    }
    if !(DC_GUARD_HZ * 2.0..=MAX_BAND_HZ).contains(&band_hz) {
        return Err(format!(
            "band_hz {band_hz} is outside {} to {MAX_BAND_HZ} Hz",
            DC_GUARD_HZ * 2.0
        ));
    }
    if tracking_level > 0x3fff {
        return Err("the tracking generator level is 14 bits".into());
    }
    if !(SA_IF_HZ + 1.0..=1.3e9).contains(&start_hz) || !(start_hz..=1.3e9).contains(&stop_hz) {
        return Err(format!(
            "the frequency range {start_hz} to {stop_hz} Hz is outside {} Hz to 1.3 GHz",
            SA_IF_HZ + 1.0
        ));
    }
    let step = (stop_hz - start_hz) / (points - 1) as f64;
    if step > band_hz {
        return Err(format!("the point spacing {step:.0} Hz is wider than the band {band_hz} Hz: the spectrum would have holes; use more points or a wider band"));
    }
    Ok(step)
}

impl Vna {
    /// The power in the audio band of the S21 codec at an LO frequency, tracking generator off (`tracking_level` 0) or at that DDS level.
    fn sa_once(&self, f_hz: f64, band_hz: f64, tracking_level: u16) -> Result<f64, String> {
        let c = &self.cfg;
        let (usb, cap21) = (&self.session.usb, &self.session.s21);
        let block = setup_block(&SetupParams {
            freq_hz: f_hz - SA_IF_HZ,
            previous_hz: f_hz - SA_IF_HZ,
            n_rf: 10,
            n_lo: 9,
            if_hz: SA_IF_HZ,
            point_ms: c.point_ms,
            counter: 3,
            mode: 0,
            rf_level: tracking_level,
        })?;
        usb.read(0x58, tracking_level, tracking_level, 64)?;
        usb.send_all(&block)?;
        usb.read_patient(0x0d, 0x0303, 0, 64, &c.stall_pauses_ms)?;
        let mark = cap21.mark();
        let frames = |ms: f64| (ms / 1000.0 * f64::from(SAMPLE_RATE)).round() as usize;
        let f = cap21.frames_after(
            mark,
            frames(c.settle()),
            frames(c.window()),
            Duration::from_secs(3),
        )?;
        let x: Vec<f64> = f.iter().map(|v| f64::from(v[0])).collect();
        Ok(band_rms(&x, DC_GUARD_HZ, band_hz))
    }

    /// A spectrum from `start_hz` to `stop_hz` in `points` points (at least 2): at each, the audio power within `band_hz` (at most `MAX_BAND_HZ`) of the LO.
    /// Space the points no wider than the band or the spectrum has holes. Each point is measured again if its audio had an overrun or a transfer stalled.
    pub fn spectrum(
        &mut self,
        start_hz: f64,
        stop_hz: f64,
        points: usize,
        band_hz: f64,
        tracking_level: u16,
    ) -> Result<Vec<SaPoint>, String> {
        let step = spectrum_plan(start_hz, stop_hz, points, band_hz, tracking_level)?;
        (0..points)
            .map(|i| {
                let f = start_hz + step * i as f64;
                let what = format!("spectrum point at {f} Hz");
                let rms = recover(self, &what, |v| v.sa_once(f, band_hz, tracking_level))?;
                Ok(SaPoint { freq_hz: f, rms })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(f: f64, a: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| a * (2.0 * std::f64::consts::PI * f * i as f64 / 48_000.0).sin())
            .collect()
    }

    // Tests [@ANCHOR: vnwa_open_sa_band_rms]
    #[test]
    fn the_band_rms_of_a_tone_is_its_rms_inside_the_band_and_nothing_outside_it() {
        let n = 5760;
        let x = tone(3_000.0, 1000.0, n);
        let inside = band_rms(&x, 100.0, 5_000.0);
        assert!(
            (inside / (1000.0 / 2f64.sqrt()) - 1.0).abs() < 0.02,
            "{inside}"
        );
        assert!(band_rms(&x, 5_500.0, 20_000.0) < 5.0);
        // Two tones add in power, and a DC offset is not counted.
        let mut y: Vec<f64> = tone(3_000.0, 1000.0, n)
            .iter()
            .zip(tone(7_000.0, 1000.0, n))
            .map(|(a, b)| a + b + 300.0)
            .collect();
        let both = band_rms(&y, 100.0, 10_000.0);
        assert!((both / 1000.0 - 1.0).abs() < 0.02, "{both}");
        y.iter_mut().for_each(|v| *v = 300.0);
        assert!(band_rms(&y, 100.0, 10_000.0) < 1.0);
    }

    // Tests [@ANCHOR: vnwa_open_sa_spectrum]
    #[test]
    fn a_spectrum_refuses_settings_that_would_leave_holes_or_leave_the_unit_range() {
        assert_eq!(spectrum_plan(10e6, 10.01e6, 5, 5_000.0, 0), Ok(2_500.0));
        let e = spectrum_plan(10e6, 11e6, 5, 5_000.0, 0).unwrap_err();
        assert!(e.contains("holes"), "{e}");
        assert!(spectrum_plan(10e6, 10.01e6, 1, 5_000.0, 0)
            .unwrap_err()
            .contains("at least 2"));
        assert!(spectrum_plan(10e6, 10.01e6, 5, 30_000.0, 0)
            .unwrap_err()
            .contains("band_hz"));
        assert!(spectrum_plan(5_000.0, 10e6, 5_000, 5_000.0, 0)
            .unwrap_err()
            .contains("outside"));
        assert!(spectrum_plan(10e6, 10.01e6, 5, 5_000.0, 0x4000)
            .unwrap_err()
            .contains("14 bits"));
    }
}

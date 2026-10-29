// SPDX-License-Identifier: AGPL-3.0-or-later
//! The frequency meter, as the vendor's "Precision Frequency Meter" mode works (Help file and the captured traffic, `docs/PROTOCOL.md`):
//! the unit sends the normal set-up block at one fixed frequency with the step counter 0x1f (31 stages instead of the 3 of a measurement
//! point), so it emits and listens for about 31 point times. A signal on the RX input that is not exactly at that frequency
//! comes out of the receiver mixer at the IF plus the difference, while the reference channel carries the IF exactly (both are
//! made by the same two DDS chips); the difference is the rate at which the phase of the measured channel advances against the
//! reference. So the meter fits a line to the unwrapped phase of (measured / reference) over about 1.7 s, in segments of whole IF
//! periods. The codec's own clock error (12 ppm) is in both channels and cancels. Accuracy is that of the unit's 12 MHz reference,
//! as the vendor states (about 0.3 ppm), because the unit's DDS clock is the yardstick.

use std::time::Duration;

use crate::audio::tone_at_if;
use crate::capture::SAMPLE_RATE;
use crate::protocol::{chip_tuning_word, sweep_word, INPUT_CLOCK_HZ};
use crate::setup::{setup_block, SetupParams};
use crate::sweep::{recover, Vna};

/// The step counter of the vendor's meter mode.
pub const METER_COUNTER: u8 = 31;
/// How long one phase estimate is (whole IF periods are used inside it).
const SEGMENT_MS: f64 = 20.0;

/// The measured tone's frequency minus the reference tone's, in Hz, from the two channels' samples, and the standard error of the
/// line fit. Segments of `SEGMENT_MS` are analysed over whole IF periods.
pub fn tone_offset(
    measured: &[f64],
    reference: &[f64],
    fs: f64,
    if_hz: f64,
) -> Result<(f64, f64), String> {
    // [@ANCHOR: vnwa_open_meter_offset]
    let periods = (SEGMENT_MS / 1000.0 * if_hz).floor().max(1.0);
    let seg = (periods * fs / if_hz).round() as usize;
    let n = measured.len().min(reference.len()) / seg;
    if n < 8 {
        return Err(format!(
            "only {n} segments: too little data for a frequency measurement"
        ));
    }
    let mut t = Vec::with_capacity(n);
    let mut ph = Vec::with_capacity(n);
    let mut last = 0.0f64;
    for k in 0..n {
        let r = k * seg..(k + 1) * seg;
        let m = tone_at_if(&measured[r.clone()], fs, if_hz);
        let rf = tone_at_if(&reference[r], fs, if_hz);
        if rf.abs() < 1e-9 || m.abs() < 1e-9 {
            return Err("no signal in a segment".into());
        }
        let z = m / rf;
        let mut p = z.im.atan2(z.re);
        if k > 0 {
            // Unwrap against the previous segment: the phase moves by less than half a turn per segment (below 25 Hz at 20 ms).
            while p - last > std::f64::consts::PI {
                p -= 2.0 * std::f64::consts::PI;
            }
            while p - last <= -std::f64::consts::PI {
                p += 2.0 * std::f64::consts::PI;
            }
        }
        last = p;
        t.push((k as f64 + 0.5) * seg as f64 / fs);
        ph.push(p);
    }
    let nf = n as f64;
    let (mt, mp) = (t.iter().sum::<f64>() / nf, ph.iter().sum::<f64>() / nf);
    let sxx: f64 = t.iter().map(|x| (x - mt) * (x - mt)).sum();
    let slope = t
        .iter()
        .zip(&ph)
        .map(|(x, y)| (x - mt) * (y - mp))
        .sum::<f64>()
        / sxx;
    let icpt = mp - slope * mt;
    let rss: f64 = t
        .iter()
        .zip(&ph)
        .map(|(x, y)| (y - icpt - slope * x).powi(2))
        .sum();
    let se_slope = (rss / (nf - 2.0) / sxx).sqrt();
    let two_pi = 2.0 * std::f64::consts::PI;
    Ok((slope / two_pi, se_slope / two_pi))
}

/// The frequency the unit's RF DDS actually produces for a requested frequency (its tuning word, on the nominal 360 MHz clock).
pub fn tx_actual_hz(freq_hz: f64) -> f64 {
    let w = chip_tuning_word(sweep_word(freq_hz, 10, 9), 9);
    f64::from(w) * (INPUT_CLOCK_HZ * 10.0) / 4_294_967_296.0
}

#[derive(Clone, Copy, Debug)]
pub struct Reading {
    /// The frequency the meter was centred on and the frequency the unit's TX DDS really produced for it (tuning word).
    pub center_hz: f64,
    pub tx_actual_hz: f64,
    /// Measured tone minus reference tone, in Hz, from codec 1 and codec 2 and their mean, with the standard error of the fit.
    pub offset_s11_hz: f64,
    pub offset_s21_hz: f64,
    pub offset_hz: f64,
    pub stderr_hz: f64,
    /// How long the fit ran, in seconds.
    pub seconds: f64,
}

impl Reading {
    /// The measured frequency: the unit's own TX frequency plus the offset. NOTE: the sign of the offset (whether a higher tone means a
    /// higher RX frequency) follows from the side of the LO and has NOT been verified on the unit with a known external signal;
    /// on a TX-to-RX loopback the offset is zero either way.
    pub fn frequency_hz(&self) -> f64 {
        self.tx_actual_hz + self.offset_hz
    }
}

impl Vna {
    /// Measures the frequency of the signal on the RX input near `center_hz`: the offset from it must stay under 25 Hz either way
    /// (the phase may not move more than half a turn in one 20 ms segment). `point_ms` sets the measurement time, about 31 point times.
    pub fn meter(&mut self, center_hz: f64, point_ms: f64) -> Result<Reading, String> {
        let what = format!("frequency meter at {center_hz} Hz");
        recover(self, &what, |v| v.meter_once(center_hz, point_ms))
    }

    fn meter_once(&mut self, center_hz: f64, point_ms: f64) -> Result<Reading, String> {
        let c = &self.cfg;
        let block = setup_block(&SetupParams {
            freq_hz: center_hz,
            previous_hz: center_hz,
            n_rf: 10,
            n_lo: 9,
            if_hz: c.if_hz,
            point_ms,
            counter: METER_COUNTER,
            mode: 0,
            rf_level: c.rf_level,
        })?;
        let (usb, cap11, cap21) = (&self.session.usb, &self.session.s11, &self.session.s21);
        usb.read(0x58, c.rf_level, c.rf_level, 64)?;
        let (m11, m21) = (cap11.mark(), cap21.mark());
        usb.send_all(&block)?;
        usb.read(0x0d, 0x0303, 0, 64)?;
        let frames = |ms: f64| (ms / 1000.0 * SAMPLE_RATE as f64).round() as usize;
        // The tone starts one stage in and runs to the end of stage 30.
        let (skip, n) = (
            frames(2.0 * point_ms),
            frames((METER_COUNTER as f64 - 3.0) * point_ms),
        );
        let timeout = Duration::from_secs(10);
        let f1 = cap11.frames_after(m11, skip, n, timeout)?;
        let f2 = cap21.frames_after(m21, skip, n, timeout)?;
        let fs = SAMPLE_RATE as f64;
        let col = |f: &[[i16; 2]], ch: usize| f.iter().map(|x| x[ch] as f64).collect::<Vec<_>>();
        // Codec 1 carries the reference on its left channel, codec 2 on its right.
        let (r1, x1) = (col(&f1, 0), col(&f1, 1));
        let (r2, x2) = (col(&f2, 1), col(&f2, 0));
        for (name, r) in [("codec 1", &r1), ("codec 2", &r2)] {
            let level = tone_at_if(r, fs, c.if_hz).abs();
            if level < c.min_reference {
                return Err(format!(
                    "no signal on the reference channel of {name} ({level:.1} counts)"
                ));
            }
        }
        let (o1, e1) = tone_offset(&x1, &r1, fs, c.if_hz)?;
        let (o2, e2) = tone_offset(&x2, &r2, fs, c.if_hz)?;
        let limit = 0.02 + 5.0 * (e1 + e2);
        if (o1 - o2).abs() > limit {
            return Err(format!(
                "the two codecs disagree: {o1:.4} Hz and {o2:.4} Hz (standard errors {e1:.4}, {e2:.4})"
            ));
        }
        Ok(Reading {
            center_hz,
            tx_actual_hz: tx_actual_hz(center_hz),
            offset_s11_hz: o1,
            offset_s21_hz: o2,
            offset_hz: (o1 + o2) / 2.0,
            stderr_hz: (e1 + e2) / 2.0,
            seconds: n as f64 / fs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tones(delta: f64, drift_hz: f64, noise: f64, secs: f64) -> (Vec<f64>, Vec<f64>) {
        // Reference at IF (+ the codec's drift), measured at IF + delta, with a fixed phase between them and some noise.
        let fs = 48_000.0;
        let n = (secs * fs) as usize;
        let mut seed = 12345u64;
        let mut rnd = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 31) as f64) - 0.5
        };
        let refc = (0..n)
            .map(|i| {
                5000.0 * (2.0 * std::f64::consts::PI * (4000.0 + drift_hz) * i as f64 / fs).cos()
                    + noise * rnd()
            })
            .collect();
        let meas = (0..n)
            .map(|i| {
                12000.0
                    * (2.0 * std::f64::consts::PI * (4000.0 + drift_hz + delta) * i as f64 / fs
                        + 0.7)
                        .cos()
                    + noise * rnd()
            })
            .collect();
        (meas, refc)
    }

    // Tests [@ANCHOR: vnwa_open_meter_offset]
    #[test]
    fn the_offset_between_the_two_tones_is_recovered_with_a_sign_and_the_codec_drift_cancels() {
        for delta in [0.0, 0.123, 5.0, -2.5, -0.01] {
            // 0.05 Hz is the codec's 12 ppm at 4 kHz; it is in both channels.
            let (m, r) = tones(delta, 0.05, 0.0, 1.68);
            let (got, se) = tone_offset(&m, &r, 48_000.0, 4000.0).unwrap();
            assert!((got - delta).abs() < 2e-4, "{delta}: {got} +- {se}");
        }
        // With noise (rms about 600 counts, 10 % of the reference), still within a few standard errors.
        let (m, r) = tones(0.3, 0.05, 2000.0, 1.68);
        let (got, se) = tone_offset(&m, &r, 48_000.0, 4000.0).unwrap();
        assert!(
            (got - 0.3).abs() < 6.0 * se.max(1e-4) && se < 0.01,
            "{got} +- {se}"
        );
    }

    #[test]
    fn too_little_data_or_silence_is_an_error() {
        let (m, r) = tones(0.1, 0.0, 0.0, 0.1);
        assert!(tone_offset(&m, &r, 48_000.0, 4000.0)
            .unwrap_err()
            .contains("too little"));
        let z = vec![0.0; 80_000];
        assert!(tone_offset(&z, &z, 48_000.0, 4000.0)
            .unwrap_err()
            .contains("no signal"));
    }

    #[test]
    fn the_tx_frequency_is_the_tuning_word_of_the_dds() {
        for f in [1e6, 10e6, 15.5e6, 100e6] {
            let a = tx_actual_hz(f);
            // Within one tuning-word step (360 MHz / 2^32 = 0.084 Hz) of the request.
            assert!((a - f).abs() < 0.09, "{f}: {a}");
        }
    }
}

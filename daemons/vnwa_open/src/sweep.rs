// SPDX-License-Identifier: AGPL-3.0-or-later
//! A measurement through the real unit: one set-up block per point, both codecs recorded from the moment the block is
//! sent, and the tone at the IF measured on whole IF periods after the unit has settled.
//!
//! What the unit does (observed, 10 MHz loopback, `docs/PROTOCOL.md`): after the block it emits for roughly 0.36 s, the level
//! changing in steps at the point time (the first plateau), at about 60 ms and at about 300 ms; the values measured between
//! 60 ms and 300 ms are steady to better than 0.1 %. So the analysis window sits inside that stretch.
//!
//! Results are uncalibrated ratios in the vendor program's convention (see `audio::vendor_convention`). Codec 1 has the
//! reference on its left channel and gives S11, codec 2 has it on the right and gives S21.

use std::time::Duration;

use crate::audio::{measure_point, vendor_convention, Complex, Reference};
use crate::capture::{Capture, SAMPLE_RATE};
use crate::setup::{setup_block, SetupParams};
use crate::usb::Vnwa;

/// Everything that is fixed for a session.
#[derive(Clone, Debug)]
pub struct Config {
    /// Part of the ALSA id of each codec (`hw:CARD=CODEC,` is the first, `hw:CARD=CODEC_1,` the second on the observed unit).
    pub codec_s11: String,
    pub codec_s21: String,
    /// The measurement IF; 48 kHz divided by it should be a whole number (12, 6, 4 kHz are).
    pub if_hz: f64,
    /// The time per point sent to the unit (0.2 to 100 ms).
    pub point_ms: f64,
    /// The RF level, 14 bits (0x3fff is the maximum).
    pub rf_level: u16,
    /// How long after the block is sent the analysis window starts, and how long it is.
    pub settle_ms: f64,
    pub window_ms: f64,
    /// The smallest reference amplitude (sample counts) accepted as a signal.
    pub min_reference: f64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            codec_s11: "hw:CARD=CODEC,".into(),
            codec_s21: "hw:CARD=CODEC_1,".into(),
            if_hz: 12_000.0,
            point_ms: 60.0,
            rf_level: 0x3fff,
            settle_ms: 80.0,
            window_ms: 100.0,
            min_reference: 200.0,
        }
    }
}

/// Refuses a configuration the unit or the analysis cannot use: an IF that does not divide 48 kHz into a whole number of
/// samples, a time per point outside the program's range, a level above 14 bits, a window that runs past the emission.
pub fn check(c: &Config) -> Result<(), String> {
    let per = SAMPLE_RATE as f64 / c.if_hz;
    if !(c.if_hz.is_finite() && c.if_hz > 0.0 && per.fract() == 0.0 && per >= 4.0) {
        return Err(format!(
            "if_hz {} does not divide 48 kHz into a whole number (at least 4) of samples",
            c.if_hz
        ));
    }
    crate::setup::point_time_fields(c.point_ms)?;
    if c.rf_level > 0x3fff {
        return Err(format!(
            "rf_level {} is above the 14-bit maximum 16383",
            c.rf_level
        ));
    }
    if !(c.settle_ms >= 0.0 && c.window_ms > 0.0 && c.settle_ms + c.window_ms <= 300.0) {
        return Err("settle_ms must be at least 0, window_ms above 0, and together at most 300 ms (the unit stops emitting near 360 ms)".into());
    }
    Ok(())
}

/// One measured point.
#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub freq_hz: f64,
    pub s11: Complex,
    pub s21: Complex,
}

pub struct Vna {
    usb: Vnwa,
    s11: Capture,
    s21: Capture,
    cfg: Config,
}

fn frames(ms: f64) -> usize {
    (ms / 1000.0 * SAMPLE_RATE as f64).round() as usize
}

impl Vna {
    pub fn open(cfg: Config) -> Result<Vna, String> {
        check(&cfg)?;
        Ok(Vna {
            usb: Vnwa::open()?,
            s11: Capture::open(&cfg.codec_s11)?,
            s21: Capture::open(&cfg.codec_s21)?,
            cfg,
        })
    }

    /// One point. `previous_hz` is the frequency before the sweep started (the unit sends the offset to it); a single point
    /// passes its own frequency.
    pub fn measure(&self, freq_hz: f64, previous_hz: f64) -> Result<Point, String> {
        let c = &self.cfg;
        let block = setup_block(&SetupParams {
            freq_hz,
            previous_hz,
            n_rf: 10,
            n_lo: 9,
            if_hz: c.if_hz,
            point_ms: c.point_ms,
            counter: 3,
            mode: 0,
            rf_level: c.rf_level,
        })?;
        let (m11, m21) = (self.s11.mark(), self.s21.mark());
        // The vendor program asks for the level (0x58) before each block and reads 0x0d (wValue 0x0303) after it; the unit
        // sends nothing without them. The replies are empty and are not used.
        let _ = self.usb.read(0x58, c.rf_level, c.rf_level, 64);
        self.usb.send_all(&block)?;
        let _ = self.usb.read(0x0d, 0x0303, 0, 64);
        let (skip, n) = (frames(c.settle_ms), frames(c.window_ms));
        let timeout = Duration::from_secs(3);
        let f11 = self.s11.frames_after(m11, skip, n, timeout)?;
        let f21 = self.s21.frames_after(m21, skip, n, timeout)?;
        let fs = SAMPLE_RATE as f64;
        let split = |f: &[[i16; 2]]| -> (Vec<f64>, Vec<f64>) {
            (
                f.iter().map(|x| x[0] as f64).collect(),
                f.iter().map(|x| x[1] as f64).collect(),
            )
        };
        let (l1, r1) = split(&f11);
        let (l2, r2) = split(&f21);
        let s11 = measure_point(
            &l1,
            &r1,
            fs,
            c.if_hz,
            0,
            0,
            Reference::Left,
            c.min_reference,
        )
        .map_err(|e| format!("S11 at {freq_hz} Hz: {e}"))?;
        let s21 = measure_point(
            &l2,
            &r2,
            fs,
            c.if_hz,
            0,
            0,
            Reference::Right,
            c.min_reference,
        )
        .map_err(|e| format!("S21 at {freq_hz} Hz: {e}"))?;
        Ok(Point {
            freq_hz,
            s11: vendor_convention(s11),
            s21: vendor_convention(s21),
        })
    }

    /// A linear sweep from `start_hz` to `stop_hz` in `points` points (at least 1).
    pub fn sweep(&self, start_hz: f64, stop_hz: f64, points: usize) -> Result<Vec<Point>, String> {
        if points == 0 {
            return Err("a sweep needs at least one point".into());
        }
        (0..points)
            .map(|i| {
                let f = if points == 1 {
                    start_hz
                } else {
                    start_hz + (stop_hz - start_hz) * i as f64 / (points - 1) as f64
                };
                self.measure(f, start_hz)
            })
            .collect()
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! A measurement through the real unit: one set-up block per point, both codecs recorded from the moment the block is
//! sent, and the tone at the IF measured on whole IF periods after the unit has settled.
//!
//! What the unit does (observed, 10 MHz loopback, `docs/PROTOCOL.md`): after the block it emits for six times the time per
//! point (360 ms at 60 ms, 180 ms at 30 ms, 600 ms at 100 ms), plus about 10 to 20 ms of audio latency, the level changing in
//! steps at 1, 5 and 6 point times; the values measured between 1 and 5 point times are steady to better than 0.1 %. So
//! the analysis window sits inside that stretch and scales with the time per point.
//!
//! Results are uncalibrated ratios in the vendor program's convention (see `audio::vendor_convention`). Codec 1 has the
//! reference on its left channel and gives S11, codec 2 has it on the right and gives S21.

use std::time::Duration;

use crate::audio::{measure_point, tone_at_if, vendor_convention, Complex, Reference};
use crate::capture::{Capture, SAMPLE_RATE};
use crate::power::HubPort;
use crate::setup::{setup_block, SetupParams};
use crate::usb::Vnwa;

/// Everything that is fixed for a session.
#[derive(Clone, Debug)]
pub struct Config {
    /// Part of the ALSA id of each codec. `None` (the default) finds them by their place on the USB tree (`topology`), which stays
    /// right when a power cycle changes the order the kernel names the cards in.
    pub codec_s11: Option<String>,
    pub codec_s21: Option<String>,
    /// The power-switching hub port the unit is on. With it, a point that still fails after its retries gets one cold reset
    /// of the unit; without it the failure is reported.
    pub hub: Option<HubPort>,
    /// How many times a failed point is measured again (the set-up block sent again) before a power cycle or the error.
    pub retries: u32,
    /// The measurement IF; 48 kHz divided by it should be a whole number (12, 6, 4 kHz are).
    pub if_hz: f64,
    /// The time per point sent to the unit (0.2 to 100 ms).
    pub point_ms: f64,
    /// The RF level, 14 bits (0x3fff is the maximum).
    pub rf_level: u16,
    /// How long after the block and the 0x0d read are done the analysis window starts, and how long it is; `None` takes one point time plus
    /// 11 ms and two point times.
    pub settle_ms: Option<f64>,
    pub window_ms: Option<f64>,
    /// The smallest reference amplitude (sample counts) accepted as a signal.
    pub min_reference: f64,
    /// How the two audio streams are opened (ALSA period).
    pub capture: crate::capture::Options,
    /// How many times a point is measured again because audio was lost to a buffer overrun inside its window. These retries are
    /// separate from `retries` and never lead to a power cycle (the unit is not at fault); when they are used up the point fails.
    pub overrun_retries: u32,
    /// Throw-away measurements at the start frequency before the first point of a sweep, and a wait before them (milliseconds). Both 0
    /// unless an experiment shows the first point of a sweep is worse than the rest (`examples/settle_probe.rs`).
    pub discard_points: u32,
    pub first_wait_ms: f64,
    /// Pauses (ms) between the tries of the 0x0d read that ends the set-up block, which the unit stalls in about 0.6 % of the points (a stall is
    /// an error of the read, not of the audio). Empty uses the default transfer retry.
    pub stall_pauses_ms: Vec<u64>,
    /// The shortest time (ms) from the end of one point's send to the start of the next one's; 0 sends as soon as the last point's
    /// analysis is done. The unit emits for 5 point times after each block (`docs/PROTOCOL.md`).
    pub min_gap_ms: f64,
}

/// Where the analysis window starts, after the first level step (one point time after the end of the block send): 11 ms, which is where it
/// started on average when it was timed from before the send (60 + 20 ms minus the median send of 9 ms).
pub const SETTLE_AFTER_STEP_MS: f64 = 11.0;

/// The pauses between the tries of a stalled 0x0d read (6 resends, 315 ms in all at worst). With the default transfer retry (3 resends, 5 ms apart)
/// 0.61 % of all attempts (25 of 4,105) failed on a stalled read; with these, 0.05 % (2 of 4,082). Waiting 360 ms between points instead did
/// not help (13 of 2,053) and made a sweep 80 % slower. See `docs/PROTOCOL.md`.
pub const STALL_PAUSES_MS: [u64; 6] = [5, 10, 20, 40, 80, 160];

impl Default for Config {
    fn default() -> Config {
        Config {
            codec_s11: None,
            codec_s21: None,
            hub: None,
            retries: 2,
            if_hz: 12_000.0,
            point_ms: 60.0,
            rf_level: 0x3fff,
            settle_ms: None,
            window_ms: None,
            min_reference: 200.0,
            capture: crate::capture::Options::default(),
            overrun_retries: 10,
            discard_points: 0,
            first_wait_ms: 0.0,
            stall_pauses_ms: STALL_PAUSES_MS.to_vec(),
            min_gap_ms: 0.0,
        }
    }
}

impl Config {
    /// The start of the analysis window after the block is sent, in milliseconds.
    pub fn settle(&self) -> f64 {
        self.settle_ms
            .unwrap_or(self.point_ms + SETTLE_AFTER_STEP_MS)
    }

    /// The length of the analysis window in milliseconds.
    pub fn window(&self) -> f64 {
        self.window_ms.unwrap_or(2.0 * self.point_ms)
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
    let (settle, window) = (c.settle(), c.window());
    let steady_end = 5.0 * c.point_ms;
    if !(settle >= 0.0 && window > 0.0 && settle + window <= steady_end + 15.0) {
        return Err(format!(
            "the analysis window ({settle} ms + {window} ms) must end by 5 point times plus the audio latency ({} ms): after that the unit's level steps",
            steady_end + 15.0
        ));
    }
    if window < 1000.0 / c.if_hz {
        return Err(format!(
            "the analysis window of {window} ms is shorter than one IF period"
        ));
    }
    Ok(())
}

/// The largest IF error (Hz) and level change between the two halves of the window that a point may show. The codec's clock is not
/// the unit's, so a healthy unit shows about 0.15 Hz of IF error (12 ppm); a DDS whose PLL has slipped, a retune inside the
/// window or a stage boundary shows far more.
pub const MAX_IF_ERROR_HZ: f64 = 2.0;
pub const MAX_LEVEL_CHANGE: f64 = 0.03;

/// How steady the tone at the IF is across a window, from the two halves of the reference channel: the frequency error in Hz
/// (from the phase advance between the halves) and the relative change of the amplitude. A healthy burst gives about 0.15 Hz and
/// under 0.1 %.
pub fn stability(x: &[f64], fs: f64, if_hz: f64) -> (f64, f64) {
    // [@ANCHOR: vnwa_open_stability]
    let half = x.len() / 2;
    let (a, b) = (
        tone_at_if(&x[..half], fs, if_hz),
        tone_at_if(&x[half..2 * half], fs, if_hz),
    );
    let mut dphi = b.phase_deg() - a.phase_deg();
    while dphi > 180.0 {
        dphi -= 360.0;
    }
    while dphi <= -180.0 {
        dphi += 360.0;
    }
    let df = dphi / 360.0 / (half as f64 / fs);
    let mean = (a.abs() + b.abs()) / 2.0;
    let change = if mean > 0.0 {
        (b.abs() - a.abs()).abs() / mean
    } else {
        f64::INFINITY
    };
    (df, change)
}

/// One measured point.
#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub freq_hz: f64,
    pub s11: Complex,
    pub s21: Complex,
    /// The larger IF error of the two reference channels (Hz) and the larger relative level change between the window's halves.
    pub if_error_hz: f64,
    pub level_change: f64,
}

/// How far a measurement is from a reference value of the same quantity (here: the vendor program's): the magnitude difference in
/// percent of the reference and the phase difference in degrees (wrapped to -180 to 180).
pub fn agreement(reference: Complex, measured: Complex) -> (f64, f64) {
    // [@ANCHOR: vnwa_open_vendor_agreement]
    let ratio = Complex {
        re: (measured.re * reference.re + measured.im * reference.im)
            / (reference.re * reference.re + reference.im * reference.im),
        im: (measured.im * reference.re - measured.re * reference.im)
            / (reference.re * reference.re + reference.im * reference.im),
    };
    ((ratio.abs() - 1.0) * 100.0, ratio.phase_deg())
}

/// What the unit needed to get a result, for the caller to report.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Recovery {
    /// Points measured again after a failed attempt.
    pub remeasured: u32,
    /// Control transfers sent again after a transient error (EPROTO and the like).
    pub transfer_resends: u32,
    /// Cold resets through the hub.
    pub power_cycles: u32,
    /// Points measured again because a buffer overrun fell inside their window (frames lost).
    pub overrun_retries: u32,
    /// Buffer overruns the audio layer reported on the two codecs since the handle was opened, wherever they fell; each one inside
    /// a window cost a retry or failed the point, so none passes silently.
    pub overruns_reported: u64,
}

/// The unit and its two codecs, opened together; all three are reopened after a power cycle.
pub(crate) struct Session {
    pub(crate) usb: Vnwa,
    pub(crate) s11: Capture,
    pub(crate) s21: Capture,
}

impl Session {
    /// Overruns reported on the two codecs since they were opened.
    pub(crate) fn overruns(&self) -> u64 {
        self.s11.overrun_count() + self.s21.overrun_count()
    }

    fn open(cfg: &Config) -> Result<Session, String> {
        let found = match (&cfg.codec_s11, &cfg.codec_s21) {
            (Some(a), Some(b)) => crate::topology::Codecs {
                s11: a.clone(),
                s21: b.clone(),
            },
            _ => crate::topology::codecs()?,
        };
        Ok(Session {
            usb: Vnwa::open()?,
            s11: Capture::open_with(cfg.codec_s11.as_deref().unwrap_or(&found.s11), cfg.capture)?,
            s21: Capture::open_with(cfg.codec_s21.as_deref().unwrap_or(&found.s21), cfg.capture)?,
        })
    }
}

/// What `recover` needs from a measuring unit.
pub trait Recoverable {
    fn retries(&self) -> u32;
    /// The budget of re-measurements after lost audio (separate from `retries`, no power cycle).
    fn overrun_retries(&self) -> u32;
    fn has_hub(&self) -> bool;
    fn tally(&mut self) -> &mut Recovery;
    /// Called when `recover` succeeded after at least one failed attempt, with what was measured and every failure, so that a
    /// recovery that worked still leaves its cause on record (the soak's three unexplained power cycles were lost this way).
    fn note_recovered(&mut self, _what: &str, _history: &[String]) {}
    /// Called by `recover` right after it cold reset the unit: why (the failures that led to it) and how long the reset took.
    fn note_power_cycle(&mut self, _reason: &str, _seconds: f64) {}
    /// Cold reset, then reopen everything that was open.
    fn cold_reset(&mut self) -> Result<(), String>;
}

/// Runs `attempt` up to `retries + 1` times; if all fail and a hub is configured, one cold reset and the same again; then a
/// clear error listing every attempt. Nothing that failed is ever returned as a result. An attempt that failed because audio was
/// lost to a buffer overrun (`capture::is_overrun_error`) is repeated from a separate budget (`overrun_retries`) and does not
/// count against the unit: it never leads to a power cycle, and when the budget is used up the error says so.
pub fn recover<R: Recoverable, T>(
    unit: &mut R,
    what: &str,
    mut attempt: impl FnMut(&mut R) -> Result<T, String>,
) -> Result<T, String> {
    // [@ANCHOR: vnwa_open_recovery]
    let mut history: Vec<String> = Vec::new();
    let mut overruns_left = unit.overrun_retries();
    for round in 0..2 {
        let tries = unit.retries();
        let mut n = 0;
        while n <= tries {
            match attempt(unit) {
                Ok(v) => {
                    if !history.is_empty() {
                        unit.note_recovered(what, &history);
                    }
                    return Ok(v);
                }
                Err(e) if crate::capture::is_overrun_error(&e) => {
                    history.push(format!("attempt {} of round {}: {e}", n + 1, round + 1));
                    if overruns_left == 0 {
                        return Err(format!(
                            "{what} failed: audio was lost to buffer overruns in every one of the last {} attempts (no power cycle: the unit is not at fault): {}",
                            unit.overrun_retries() + 1,
                            history.join("; ")
                        ));
                    }
                    overruns_left -= 1;
                    unit.tally().overrun_retries += 1;
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
                Err(e) => {
                    history.push(format!("attempt {} of round {}: {e}", n + 1, round + 1));
                    if n < tries {
                        unit.tally().remeasured += 1;
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }
            }
            n += 1;
        }
        if round == 1 || !unit.has_hub() {
            break;
        }
        unit.tally().power_cycles += 1;
        let reason = history.join("; ");
        let started = std::time::Instant::now();
        if let Err(e) = unit.cold_reset() {
            history.push(format!("power cycle: {e}"));
            break;
        }
        let seconds = started.elapsed().as_secs_f64();
        unit.note_power_cycle(&reason, seconds);
        history.push(format!(
            "power cycle by the recovery done ({seconds:.1} s from power-off to the unit and its codecs open)"
        ));
    }
    Err(format!(
        "{what} failed and was not recovered ({} power cycle(s)): {}",
        unit.tally().power_cycles,
        history.join("; ")
    ))
}

impl Recoverable for Vna {
    fn retries(&self) -> u32 {
        self.cfg.retries
    }
    fn overrun_retries(&self) -> u32 {
        self.cfg.overrun_retries
    }
    fn note_recovered(&mut self, what: &str, history: &[String]) {
        // [@ANCHOR: vnwa_open_failure_log]
        if self.failures.len() >= MAX_FAILURE_LOG {
            self.failures.remove(0);
        }
        self.failures.push(FailureRecord {
            epoch_s: epoch_s(),
            what: what.to_string(),
            attempts: history.to_vec(),
        });
    }
    fn note_power_cycle(&mut self, reason: &str, seconds: f64) {
        self.power_log.push(PowerRecord {
            epoch_s: epoch_s(),
            reason: reason.to_string(),
            seconds,
        });
    }
    fn has_hub(&self) -> bool {
        self.cfg.hub.is_some()
    }
    fn tally(&mut self) -> &mut Recovery {
        &mut self.recovery
    }
    fn cold_reset(&mut self) -> Result<(), String> {
        let hub = self.cfg.hub.clone().ok_or("no hub configured")?;
        self.recovery.transfer_resends += self.session.usb.resends();
        self.recovery.overruns_reported += self.session.overruns();
        // The audio streams are closed before the power goes: an open stream on a vanishing codec storms with overrun reports.
        self.session.s11.close();
        self.session.s21.close();
        crate::power::cold_reset(&hub)?;
        self.session = Session::open(&self.cfg)?;
        Ok(())
    }
}

pub struct Vna {
    pub(crate) session: Session,
    pub(crate) cfg: Config,
    recovery: Recovery,
    failures: Vec<FailureRecord>,
    power_log: Vec<PowerRecord>,
    /// The raw levels of the last attempt, for the evidence of a failure.
    last_levels: std::sync::Mutex<String>,
    /// How long the last attempt took to send its block and the 0x0d read (milliseconds); the analysis window is timed from before it.
    last_send_ms: std::sync::Mutex<f64>,
    last_send_end: std::sync::Mutex<Option<std::time::Instant>>,
    /// (block send time in ms, attempt succeeded) of the attempts since the handle was opened, the newest `ATTEMPT_LOG`.
    attempt_log: std::sync::Mutex<Vec<(f64, bool)>>,
}

const ATTEMPT_LOG: usize = 20_000;

/// How many recovered failures `Vna::failure_log` keeps (the newest).
const MAX_FAILURE_LOG: usize = 200;

/// A point that needed more than one attempt (or a power cycle) and was measured in the end: every failed attempt with its evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct FailureRecord {
    /// Seconds since the epoch when the point finally succeeded.
    pub epoch_s: u64,
    pub what: String,
    /// One entry per failed attempt (and per power cycle) in order, each with the error text and the evidence appended by `Vna`.
    pub attempts: Vec<String>,
}

/// A power cycle the recovery did by itself (a scheduled one is the caller's).
#[derive(Clone, Debug, PartialEq)]
pub struct PowerRecord {
    pub epoch_s: u64,
    pub reason: String,
    /// From power-off until the unit and both codecs were open again.
    pub seconds: f64,
}

/// Measures `count` points at the start frequency and throws the results away, to bring the unit to a steady state before the sweep.
/// A failure of a discarded point is still counted in the recovery tallies and the failure log by `measure` itself.
fn discard_points<T>(count: u32, mut measure: impl FnMut() -> Result<T, String>) {
    // [@ANCHOR: vnwa_open_discard_points]
    for _ in 0..count {
        let _ = measure();
    }
}

fn epoch_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn frames(ms: f64) -> usize {
    (ms / 1000.0 * SAMPLE_RATE as f64).round() as usize
}

impl Vna {
    pub fn open(cfg: Config) -> Result<Vna, String> {
        check(&cfg)?;
        Ok(Vna {
            session: Session::open(&cfg)?,
            cfg,
            recovery: Recovery::default(),
            failures: Vec::new(),
            power_log: Vec::new(),
            last_levels: std::sync::Mutex::new(String::new()),
            last_send_ms: std::sync::Mutex::new(0.0),
            last_send_end: std::sync::Mutex::new(None),
            attempt_log: std::sync::Mutex::new(Vec::new()),
        })
    }

    /// Every failed attempt of the points that were recovered (re-measured or after a power cycle), with the error text of each
    /// attempt, oldest first. A failure that was not recovered is the error `measure` returned.
    pub fn failure_log(&self) -> &[FailureRecord] {
        &self.failures
    }

    /// For every attempt since the handle was opened: how long its block and the 0x0d read took to send (ms; 0 if the attempt failed
    /// before the send finished) and whether it succeeded.
    pub fn attempt_timings(&self) -> Vec<(f64, bool)> {
        self.attempt_log.lock().unwrap().clone()
    }

    /// The power cycles the recovery did, oldest first, with their reasons.
    pub fn power_log(&self) -> &[PowerRecord] {
        &self.power_log
    }

    /// What recovery this handle has needed so far.
    pub fn recovery(&self) -> Recovery {
        Recovery {
            transfer_resends: self.recovery.transfer_resends + self.session.usb.resends(),
            overruns_reported: self.recovery.overruns_reported + self.session.overruns(),
            ..self.recovery
        }
    }

    /// One point, measured again when an attempt fails (a transfer error that outlasted its resends, no signal on the
    /// reference, an unsteady tone, missing audio), and, if the config names a hub port, after one cold reset of the unit when
    /// the retries are used up. The result is never a number from a failed attempt: the error lists every attempt.
    pub fn measure(&mut self, freq_hz: f64, previous_hz: f64) -> Result<Point, String> {
        let what = format!("point at {freq_hz} Hz");
        recover(self, &what, |v| {
            *v.last_levels.lock().unwrap() = String::new();
            *v.last_send_ms.lock().unwrap() = 0.0;
            let r = v.measure_once(freq_hz, previous_hz);
            {
                let mut log = v.attempt_log.lock().unwrap();
                if log.len() >= ATTEMPT_LOG {
                    log.remove(0);
                }
                log.push((*v.last_send_ms.lock().unwrap(), r.is_ok()));
            }
            r.map_err(|e| v.with_evidence(e))
        })
    }

    /// Appends the evidence of a failed attempt to its error text: the raw channel levels, the overruns reported so far and the
    /// last transfers with their outcomes (the 2026-10-29 soak lost all of this when a retry cured the failure).
    pub(crate) fn with_evidence(&self, e: String) -> String {
        // [@ANCHOR: vnwa_open_failure_evidence]
        let levels = self.last_levels.lock().unwrap().clone();
        let levels = if levels.is_empty() {
            "none (the attempt failed before the audio was analysed)".to_string()
        } else {
            levels
        };
        format!(
            "{e} [evidence: block send took {:.1} ms; levels {levels}; overruns reported so far {}; last transfers: {}]",
            *self.last_send_ms.lock().unwrap(),
            self.session.overruns(),
            self.session.usb.recent_transfers().join(" | ")
        )
    }

    /// One point. `previous_hz` is the frequency the offset in the block is taken from; every point of `sweep` passes its own
    /// frequency (no offset), because the later points of a block-per-point sweep with the sweep start as `previous_hz` were
    /// not repeatable (100 MHz measured 0.93, 1.20, 1.56 and 0.94 in four runs, against 0.998 every time as a single point).
    fn measure_once(&self, freq_hz: f64, previous_hz: f64) -> Result<Point, String> {
        let c = &self.cfg;
        let (usb, cap11, cap21) = (&self.session.usb, &self.session.s11, &self.session.s21);
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
        // The vendor program asks for the level (0x58) before each block and reads 0x0d (wValue 0x0303) after it; the unit
        // sends nothing without them. The replies are empty and are not used, but a failed transfer is not ignored: the 0x0d
        // read is stalled by the unit in about 7 % of tries (a resend clears it), and the other transfers must not fail silently.
        usb.read(0x58, c.rf_level, c.rf_level, 64)?;
        // The analysis window is placed relative to the instant the block is sent, so the mark is taken right before it.
        // Resent transfers wait only a few milliseconds (`usb::RETRY_PAUSE`) so they do not move the window.
        // [@ANCHOR: vnwa_open_window_from_send_end]
        if c.min_gap_ms > 0.0 {
            let last = *self.last_send_end.lock().unwrap();
            if let Some(t) = last {
                let want = Duration::from_secs_f64(c.min_gap_ms / 1000.0);
                if t.elapsed() < want {
                    std::thread::sleep(want - t.elapsed());
                }
            }
        }
        let sent_at = std::time::Instant::now();
        usb.send_all(&block)?;
        if c.stall_pauses_ms.is_empty() {
            usb.read(0x0d, 0x0303, 0, 64)?;
        } else {
            usb.read_patient(0x0d, 0x0303, 0, 64, &c.stall_pauses_ms)?;
        }
        *self.last_send_end.lock().unwrap() = Some(std::time::Instant::now());
        *self.last_send_ms.lock().unwrap() = sent_at.elapsed().as_secs_f64() * 1000.0;
        // The unit starts emitting when the block and the 0x0d read are done, so the window is timed from this instant. (It was timed from
        // before the send, and the send takes 7 to 27 ms: a send over about 20 ms put the window start on the 60 ms level step, which is
        // why 3 to 4 % of all attempts failed the steadiness check; measured 2026-10-29: every failed attempt had a send of 20.9 ms or more,
        // no succeeded attempt had more.)
        let (m11, m21) = (cap11.mark(), cap21.mark());
        let (skip, n) = (frames(c.settle()), frames(c.window()));
        let timeout = Duration::from_secs(3);
        let f11 = cap11.frames_after(m11, skip, n, timeout)?;
        let f21 = cap21.frames_after(m21, skip, n, timeout)?;
        let fs = SAMPLE_RATE as f64;
        let split = |f: &[[i16; 2]]| -> (Vec<f64>, Vec<f64>) {
            (
                f.iter().map(|x| x[0] as f64).collect(),
                f.iter().map(|x| x[1] as f64).collect(),
            )
        };
        let (l1, r1) = split(&f11);
        let (l2, r2) = split(&f21);
        // The reference channel of each codec (left on codec 1, right on codec 2) must carry the tone at all: when it does not
        // (the unit's reference inputs lost the signal while the measured channels still see it, seen 2026-10-26) the cause is
        // not the DDS lock and the steadiness check below would only report noise, so say what is missing.
        let level = |x: &[f64]| tone_at_if(x, fs, c.if_hz).abs();
        let (ref1, ref2) = (level(&l1), level(&r2));
        *self.last_levels.lock().unwrap() = format!(
            "(counts at the IF) codec1 reference {ref1:.1}, codec1 measured {:.1}, codec2 reference {ref2:.1}, codec2 measured {:.1}",
            level(&r1),
            level(&l2)
        );
        if ref1 < c.min_reference || ref2 < c.min_reference {
            return Err(format!(
                "point at {freq_hz} Hz: no signal on the reference channel(s) at the IF (codec 1 left {ref1:.1}, codec 2 right {ref2:.1} counts, minimum {}); the measured channels read codec 1 right {:.1}, codec 2 left {:.1}. The tone is reaching the receivers, so this is the unit's reference input or its cabling, not the DDS lock",
                c.min_reference,
                level(&r1),
                level(&l2)
            ));
        }
        // The reference channel of each codec (left on codec 1, right on codec 2) must hold a steady tone at the IF.
        let (e1, c1) = stability(&l1, fs, c.if_hz);
        let (e2, c2) = stability(&r2, fs, c.if_hz);
        let (if_error_hz, level_change) = (e1.abs().max(e2.abs()), c1.max(c2));
        if if_error_hz > MAX_IF_ERROR_HZ || level_change > MAX_LEVEL_CHANGE {
            return Err(format!(
                "point at {freq_hz} Hz is not steady: IF error {if_error_hz:.2} Hz (limit {MAX_IF_ERROR_HZ}), level change {:.1} % (limit {:.0} %): the DDS may be out of lock, or the window spans a change of the unit's stage",
                level_change * 100.0,
                MAX_LEVEL_CHANGE * 100.0
            ));
        }
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
            if_error_hz,
            level_change,
        })
    }

    /// A linear sweep from `start_hz` to `stop_hz` in `points` points (at least 1).
    pub fn sweep(
        &mut self,
        start_hz: f64,
        stop_hz: f64,
        points: usize,
    ) -> Result<Vec<Point>, String> {
        if points == 0 {
            return Err("a sweep needs at least one point".into());
        }
        if self.cfg.first_wait_ms > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(self.cfg.first_wait_ms / 1000.0));
        }
        discard_points(self.cfg.discard_points, || self.measure(start_hz, start_hz));
        (0..points)
            .map(|i| {
                let f = if points == 1 {
                    start_hz
                } else {
                    start_hz + (stop_hz - start_hz) * i as f64 / (points - 1) as f64
                };
                self.measure(f, f)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    include!("vendor_comparison_vectors.rs");

    fn tone(f: f64, fs: f64, n: usize, level: impl Fn(usize) -> f64) -> Vec<f64> {
        (0..n)
            .map(|i| level(i) * (2.0 * std::f64::consts::PI * f * i as f64 / fs).sin())
            .collect()
    }

    // Tests [@ANCHOR: vnwa_open_stability]
    #[test]
    fn a_steady_tone_at_the_if_is_stable_and_an_error_or_a_level_step_is_seen() {
        let fs = 48_000.0;
        let n = 5760; // 120 ms
        let (df, change) = stability(&tone(12_000.0, fs, n, |_| 8000.0), fs, 12_000.0);
        assert!(df.abs() < 0.01 && change < 0.001, "{df} {change}");
        // The unit's IF 5 Hz high (a slipped PLL, or a codec clock error of 400 ppm): the phase advance gives it back.
        let (df, _) = stability(&tone(12_005.0, fs, n, |_| 8000.0), fs, 12_000.0);
        assert!((df - 5.0).abs() < 0.2, "{df}");
        let (df, _) = stability(&tone(11_996.0, fs, n, |_| 8000.0), fs, 12_000.0);
        assert!((df + 4.0).abs() < 0.2, "{df}");
        // A step in level between the halves (a stage boundary): 8000 then 9800 is a change of about 20 %.
        let (_, change) = stability(
            &tone(12_000.0, fs, n, |i| if i < n / 2 { 8000.0 } else { 9800.0 }),
            fs,
            12_000.0,
        );
        assert!((change - 0.204).abs() < 0.01, "{change}");
        // No signal at all is not "stable".
        let (_, change) = stability(&vec![0.0; n], fs, 12_000.0);
        assert!(change.is_infinite());
    }

    struct Fake {
        tally: Recovery,
        hub: bool,
        resets: u32,
        reset_fails: bool,
    }
    impl Recoverable for Fake {
        fn retries(&self) -> u32 {
            2
        }
        fn overrun_retries(&self) -> u32 {
            3
        }
        fn has_hub(&self) -> bool {
            self.hub
        }
        fn tally(&mut self) -> &mut Recovery {
            &mut self.tally
        }
        fn cold_reset(&mut self) -> Result<(), String> {
            self.resets += 1;
            if self.reset_fails {
                Err("hub gone".into())
            } else {
                Ok(())
            }
        }
    }
    fn fake(hub: bool) -> Fake {
        Fake {
            tally: Recovery::default(),
            hub,
            resets: 0,
            reset_fails: false,
        }
    }

    // Tests [@ANCHOR: vnwa_open_discard_points]
    #[test]
    fn the_discarded_points_are_measured_all_of_them_and_a_failure_does_not_stop_the_rest() {
        let mut calls = 0;
        discard_points(3, || {
            calls += 1;
            if calls == 2 {
                Err::<u8, String>("silent".into())
            } else {
                Ok(1)
            }
        });
        assert_eq!(calls, 3);
        let mut none = 0;
        discard_points(0, || {
            none += 1;
            Ok::<u8, String>(1)
        });
        assert_eq!(none, 0);
    }

    // Tests [@ANCHOR: vnwa_open_recovery]
    #[test]
    fn a_failed_attempt_is_repeated_and_only_a_success_is_returned() {
        let mut f = fake(true);
        let mut calls = 0;
        let v = recover(&mut f, "p", |_| {
            calls += 1;
            if calls < 3 {
                Err("silent".to_string())
            } else {
                Ok(7)
            }
        });
        assert_eq!(v, Ok(7));
        assert_eq!(
            (f.tally.remeasured, f.resets, f.tally.power_cycles),
            (2, 0, 0)
        );
    }

    #[test]
    fn when_the_retries_are_used_up_the_unit_is_power_cycled_once_and_then_tried_again() {
        let mut f = fake(true);
        let mut calls = 0;
        let v = recover(&mut f, "p", |f: &mut Fake| {
            calls += 1;
            if f.resets == 0 {
                Err(format!("silent {calls}"))
            } else {
                Ok(calls)
            }
        });
        assert_eq!(v, Ok(4));
        assert_eq!((f.resets, f.tally.power_cycles), (1, 1));
    }

    #[test]
    fn without_a_hub_or_when_it_keeps_failing_the_error_lists_every_attempt() {
        let mut f = fake(false);
        let e = recover::<_, ()>(&mut f, "point at 1 Hz", |_| Err("no signal".into())).unwrap_err();
        assert_eq!(f.resets, 0);
        assert!(
            e.contains("attempt 3 of round 1")
                && e.contains("no signal")
                && e.contains("0 power cycle"),
            "{e}"
        );
        let mut f = fake(true);
        let e = recover::<_, ()>(&mut f, "p", |_| Err("no signal".into())).unwrap_err();
        assert_eq!(f.resets, 1);
        assert!(e.contains("attempt 3 of round 2"), "{e}");
        let mut f = Fake {
            reset_fails: true,
            ..fake(true)
        };
        let e = recover::<_, ()>(&mut f, "p", |_| Err("no signal".into())).unwrap_err();
        assert!(e.contains("power cycle: hub gone"), "{e}");
    }

    // Tests [@ANCHOR: vnwa_open_recovery]
    #[test]
    fn lost_audio_is_measured_again_from_its_own_budget_and_never_power_cycles_the_unit() {
        let mut f = fake(true);
        let mut calls = 0;
        let e = |n: u32| {
            format!(
                "{}: {n} buffer overrun(s) reported",
                crate::capture::OVERRUN_ERROR
            )
        };
        // Two overrun failures then a success: no normal retry is used, no power cycle.
        let v = recover(&mut f, "p", |_| {
            calls += 1;
            if calls < 3 {
                Err(e(calls))
            } else {
                Ok(9)
            }
        });
        assert_eq!(v, Ok(9));
        assert_eq!(
            (f.tally.overrun_retries, f.tally.remeasured, f.resets),
            (2, 0, 0)
        );
        // Overruns in every attempt: the budget (3) is used up and the error says so; still no power cycle.
        let mut f = fake(true);
        let err = recover::<_, ()>(&mut f, "point at 1 Hz", |_| Err(e(1))).unwrap_err();
        assert_eq!((f.tally.overrun_retries, f.resets), (3, 0));
        assert!(
            err.contains("lost to buffer overruns") && err.contains("no power cycle"),
            "{err}"
        );
        // A different failure between them still gets the normal path.
        let mut f = fake(true);
        let mut n = 0;
        let v = recover(&mut f, "p", |_| {
            n += 1;
            match n {
                1 => Err(e(1)),
                2 => Err("silent".to_string()),
                _ => Ok(1),
            }
        });
        assert_eq!(v, Ok(1));
        assert_eq!((f.tally.overrun_retries, f.tally.remeasured), (1, 1));
    }

    // Tests [@ANCHOR: vnwa_open_failure_log]
    #[test]
    fn a_recovery_that_worked_still_reports_every_failure_it_had() {
        struct Noting {
            f: Fake,
            noted: Vec<(String, Vec<String>)>,
        }
        impl Recoverable for Noting {
            fn retries(&self) -> u32 {
                self.f.retries()
            }
            fn overrun_retries(&self) -> u32 {
                3
            }
            fn has_hub(&self) -> bool {
                true
            }
            fn tally(&mut self) -> &mut Recovery {
                &mut self.f.tally
            }
            fn note_recovered(&mut self, what: &str, history: &[String]) {
                self.noted.push((what.to_string(), history.to_vec()));
            }
            fn cold_reset(&mut self) -> Result<(), String> {
                self.f.resets += 1;
                Ok(())
            }
        }
        let mut u = Noting {
            f: fake(true),
            noted: Vec::new(),
        };
        let v = recover(&mut u, "point at 5 Hz", |u: &mut Noting| {
            if u.f.resets == 0 {
                Err("no signal on the reference channel(s)".to_string())
            } else {
                Ok(1)
            }
        });
        assert_eq!(v, Ok(1));
        assert_eq!(u.noted.len(), 1);
        assert_eq!(u.noted[0].1.len(), 4);
        assert!(u.noted[0].1[0].contains("no signal on the reference"));
        // A first-try success notes nothing.
        let mut u = Noting {
            f: fake(true),
            noted: Vec::new(),
        };
        assert_eq!(recover(&mut u, "p", |_| Ok(2)), Ok(2));
        assert!(u.noted.is_empty());
    }

    // Tests [@ANCHOR: vnwa_open_vendor_agreement]
    #[test]
    fn this_driver_agrees_with_the_vendor_program_on_the_same_cables() {
        // 60 points, 100 kHz to 150 MHz including the lead stub's resonance near 52 MHz; see the vectors file.
        let c = |re: f64, im: f64| Complex { re, im };
        let (mut worst11, mut worst21, mut ph11, mut ph21) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let mut offsets21 = Vec::new();
        for r in VENDOR_COMPARISON {
            let (m11, p11) = agreement(c(r[1], r[2]), c(r[5], r[6]));
            let (m21, p21) = agreement(c(r[3], r[4]), c(r[7], r[8]));
            worst11 = worst11.max(m11.abs());
            worst21 = worst21.max(m21.abs());
            ph11 = ph11.max(p11.abs());
            ph21 = ph21.max(p21.abs());
            offsets21.push(m21);
        }
        // S11 (codec 1) agrees to 0.15 % and 0.5 degrees; S21 (codec 2) is a constant 0.8 % high, a gain difference between the two codecs that the vendor
        // program does not show (its S11 and S21 are equal on this loopback); a through calibration removes it.
        assert!(worst11 < 0.25 && ph11 < 0.6, "{worst11} {ph11}");
        assert!(worst21 < 1.0 && ph21 < 0.6, "{worst21} {ph21}");
        let spread = offsets21.iter().cloned().fold(f64::MIN, f64::max)
            - offsets21.iter().cloned().fold(f64::MAX, f64::min);
        assert!(spread < 0.3, "the S21 offset is not constant: {spread}");
        // The agreement function itself: 1 % high and 2 degrees round.
        let (m, p) = agreement(
            c(1.0, 0.0),
            Complex {
                re: 1.01 * 2f64.to_radians().cos(),
                im: 1.01 * 2f64.to_radians().sin(),
            },
        );
        assert!((m - 1.0).abs() < 1e-9 && (p - 2.0).abs() < 1e-9, "{m} {p}");
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! Capture from one of the VNWA's two USB codecs (PCM2900C: stereo, 16 bit, 48 kHz is the best they offer). The stream
//! runs continuously into a ring buffer with a running frame counter, so a caller can mark a moment (the instant it
//! sent a set-up block), skip the frames before the unit has settled and take the frames after.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub const SAMPLE_RATE: u32 = 48_000;
/// How many frames the ring keeps (eight seconds: a frequency-meter measurement runs for several seconds).
const KEEP_FRAMES: usize = 8 * SAMPLE_RATE as usize;

struct Ring {
    frames: std::collections::VecDeque<[i16; 2]>,
    /// Frames received since the stream started (not reduced when old frames are dropped).
    total: u64,
    /// The overruns the audio layer reported, newest last; those older than the oldest frame kept are dropped.
    overruns: std::collections::VecDeque<Overrun>,
    /// All overruns since the stream started.
    overrun_count: u64,
    /// When the audio layer last delivered a period, and its length in frames (0 before the first).
    last_delivery: Option<Instant>,
    period: u64,
    /// Overrun reports since the last delivery of audio; a stream that restarts and fails again reports one every millisecond or so.
    reports_since_delivery: u64,
    /// Set when the stream was closed because of a storm of reports.
    storm: bool,
}

/// True once this many overrun reports have come with no audio between them (see `STORM_REPORTS`).
fn is_storm(reports_since_delivery: u64) -> bool {
    reports_since_delivery >= STORM_REPORTS
}

/// This many overrun reports with no audio delivered between them mean the stream cannot run (its codec is gone), not that one buffer
/// overran: `frames_after` then closes the stream and fails.
pub const STORM_REPORTS: u64 = 100;

/// One buffer overrun reported by the audio layer: the capture ring lost frames at this point. `at_frame` is the running frame
/// count when it was reported (frames before it were delivered; the lost ones would have followed), `at` the instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Overrun {
    pub at: Instant,
    pub at_frame: u64,
}

/// An overrun is counted against a window if it was reported from this many frames before the mark (the callback delivers a period
/// at a time, so a report can trail the loss) up to the end of the window.
pub const OVERRUN_GUARD_FRAMES: u64 = 960;
/// The most overruns kept on record (the count is exact beyond it).
const MAX_OVERRUNS_KEPT: usize = 4096;

pub struct Capture {
    ring: Arc<(Mutex<Ring>, Condvar)>,
    /// `None` after `close`.
    stream: Arc<Mutex<Option<cpal::Stream>>>,
}

/// How a stream is opened.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Options {
    /// The ALSA period in frames (cpal sets the buffer to two periods); `None` leaves it to the driver.
    pub period_frames: Option<u32>,
}

/// The input devices' ids (for the ALSA host, for example `alsa:hw:CARD=CODEC,DEV=0`), in the host's order.
pub fn input_names() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let devices = host.input_devices().map_err(|e| format!("audio: {e}"))?;
    Ok(devices
        .filter_map(|d| d.id().ok())
        .map(|id| id.to_string())
        .collect())
}

/// The start of the error text for a window that contains overruns; `is_overrun_error` recognises it.
pub const OVERRUN_ERROR: &str = "audio overrun";

/// True if `e` is the error `frames_after` gives for a window that lost frames to an overrun.
pub fn is_overrun_error(e: &str) -> bool {
    e.contains(OVERRUN_ERROR)
}

impl Capture {
    /// Opens the input device whose id contains `needle` (case-insensitive, for example `hw:CARD=CODEC,`) at 48 kHz, stereo, 16 bit.
    pub fn open(needle: &str) -> Result<Capture, String> {
        Capture::open_with(needle, Options::default())
    }

    pub fn open_with(needle: &str, options: Options) -> Result<Capture, String> {
        // [@ANCHOR: vnwa_open_capture_open_with]
        let host = cpal::default_host();
        let wanted = needle.to_lowercase();
        let device = host
            .input_devices()
            .map_err(|e| format!("audio: {e}"))?
            .find(|d| {
                d.id()
                    .map(|id| id.to_string().to_lowercase().contains(&wanted))
                    .unwrap_or(false)
            })
            .ok_or_else(|| format!("no audio input device with \"{needle}\" in its id"))?;
        let config = cpal::StreamConfig {
            channels: 2,
            sample_rate: SAMPLE_RATE,
            buffer_size: match options.period_frames {
                Some(n) => cpal::BufferSize::Fixed(n),
                None => cpal::BufferSize::Default,
            },
        };
        let ring = Arc::new((
            Mutex::new(Ring {
                frames: std::collections::VecDeque::new(),
                total: 0,
                overruns: std::collections::VecDeque::new(),
                overrun_count: 0,
                last_delivery: None,
                period: 0,
                reports_since_delivery: 0,
                storm: false,
            }),
            Condvar::new(),
        ));
        let writer = ring.clone();
        let reporter = ring.clone();
        let slot: Arc<Mutex<Option<cpal::Stream>>> = Arc::new(Mutex::new(None));
        let storm_slot = slot.clone();
        let stream = device
            .build_input_stream(
                config,
                move |data: &[i16], _| {
                    let (lock, wake) = &*writer;
                    let mut ring = lock.lock().unwrap();
                    ring.reports_since_delivery = 0;
                    ring.last_delivery = Some(Instant::now());
                    ring.period = (data.len() / 2) as u64;
                    for f in data.as_chunks::<2>().0 {
                        ring.frames.push_back([f[0], f[1]]);
                        ring.total += 1;
                    }
                    while ring.frames.len() > KEEP_FRAMES {
                        ring.frames.pop_front();
                    }
                    wake.notify_all();
                },
                move |e| {
                    // Every report is kept with its instant and frame position; nothing is printed per event (the soak showed
                    // 50,000 lines), the callers count them.
                    let mut ring = reporter.0.lock().unwrap();
                    if e.kind() == cpal::ErrorKind::Xrun {
                        let at_frame = ring.total;
                        ring.overrun_count += 1;
                        ring.reports_since_delivery += 1;
                        if ring.reports_since_delivery == STORM_REPORTS {
                            ring.storm = true;
                            reporter.1.notify_all();
                            // The stream cannot run: close it now, from another thread (dropping it waits for this callback), so it does not
                            // report a thousand times a second until the next measurement finds out.
                            let s = storm_slot.clone();
                            std::thread::spawn(move || {
                                let stream = s.lock().unwrap().take();
                                drop(stream);
                            });
                        }
                        ring.overruns.push_back(Overrun {
                            at: Instant::now(),
                            at_frame,
                        });
                        // A stream that cannot restart reports about a thousand a second: keep the newest, the count stays exact.
                        if ring.overruns.len() > MAX_OVERRUNS_KEPT {
                            ring.overruns.pop_front();
                        }
                    } else {
                        // Anything else is a real fault of the stream (device gone, stream failed): say so, once per report.
                        eprintln!("audio stream error: {e}");
                    }
                },
                None,
            )
            .map_err(|e| format!("audio: {e}"))?;
        stream.play().map_err(|e| format!("audio: {e}"))?;
        *slot.lock().unwrap() = Some(stream);
        Ok(Capture { ring, stream: slot })
    }

    /// Stops the stream and releases the device. It must be done before the unit's power is cut: a stream left open on a codec that
    /// vanishes keeps restarting and failing, and the audio layer reports an overrun for each try, about a thousand a second until
    /// the stream is dropped (seen in the 2026-10-29 soak: three storms of about 16,700 reports, each during a power cycle).
    pub fn close(&self) {
        // [@ANCHOR: vnwa_open_capture_close]
        // Dropped outside every other lock: the stream's worker thread takes the ring lock in its callbacks and drop waits for it.
        let stream = self.stream.lock().unwrap().take();
        drop(stream);
    }

    /// True until `close`.
    pub fn is_open(&self) -> bool {
        self.stream.lock().unwrap().is_some()
    }

    /// The frame count at this instant: the "mark" for `frames_after`. The audio layer delivers a period at a time, so the frames
    /// delivered so far trail the real time by up to one period (5 ms at the driver's default, 85 ms at a 4096-frame period); the
    /// frames that have arrived since the last delivery are added from the clock, so the mark is as exact for any buffer size.
    pub fn mark(&self) -> u64 {
        // [@ANCHOR: vnwa_open_capture_mark]
        let ring = self.ring.0.lock().unwrap();
        ring.total + frames_since(ring.last_delivery, ring.period)
    }

    /// How many overruns have been reported since the stream started.
    pub fn overrun_count(&self) -> u64 {
        self.ring.0.lock().unwrap().overrun_count
    }

    /// The overruns still on record (those since about two seconds before now), oldest first.
    pub fn overruns(&self) -> Vec<Overrun> {
        self.ring
            .0
            .lock()
            .unwrap()
            .overruns
            .iter()
            .copied()
            .collect()
    }

    /// `n` frames starting `skip` frames after `mark`, waiting (up to `timeout`) until they have arrived. If the audio layer
    /// reported an overrun anywhere from `OVERRUN_GUARD_FRAMES` before the mark to the end of the frames, some audio was lost
    /// and the timing from the mark is wrong: the result is then an error that `is_overrun_error` recognises, never frames.
    pub fn frames_after(
        &self,
        mark: u64,
        skip: usize,
        n: usize,
        timeout: Duration,
    ) -> Result<Vec<[i16; 2]>, String> {
        // [@ANCHOR: vnwa_open_overrun_window]
        if !self.is_open() {
            let storm = self.ring.0.lock().unwrap().storm;
            return Err(if storm {
                "audio: the stream was closed because it reported an overrun every few milliseconds and delivered no audio (its codec is gone): the unit needs a power cycle or the codec a new stream".into()
            } else {
                "audio: the stream was closed".into()
            });
        }
        let (lock, wake) = &*self.ring;
        let first = mark + skip as u64;
        let last = first + n as u64;
        let deadline = Instant::now() + timeout;
        let mut ring = lock.lock().unwrap();
        while ring.total < last {
            if is_storm(ring.reports_since_delivery) {
                let reports = ring.overrun_count;
                drop(ring);
                self.close();
                return Err(format!(
                    "audio: the stream was closed because it reports an overrun every few milliseconds and delivers no audio (its codec is gone; {reports} reports): the unit needs a power cycle or the codec a new stream"
                ));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!(
                    "audio: {} of {} frames arrived in time ({} overrun reports so far: a stream that reports them at a high rate has lost its codec)",
                    ring.total.saturating_sub(first),
                    n,
                    ring.overrun_count
                ));
            }
            ring = wake.wait_timeout(ring, left).unwrap().0;
        }
        let oldest = ring.total - ring.frames.len() as u64;
        if first < oldest {
            return Err("audio: the frames asked for were already dropped from the ring".into());
        }
        let hit = overruns_in(&ring.overruns, mark, last);
        if hit > 0 {
            return Err(format!(
                "{OVERRUN_ERROR}: {hit} buffer overrun(s) reported inside the {} ms from the mark to the end of the window (frames lost)",
                (last - mark) * 1000 / SAMPLE_RATE as u64
            ));
        }
        let start = (first - oldest) as usize;
        let out = ring.frames.iter().skip(start).take(n).copied().collect();
        // Overruns older than the oldest frame kept can no longer touch any window.
        while ring
            .overruns
            .front()
            .is_some_and(|o| o.at_frame + OVERRUN_GUARD_FRAMES < oldest)
        {
            ring.overruns.pop_front();
        }
        Ok(out)
    }
}

/// The frames that have passed since the last delivery of a period (the clock times the rate), at most one period.
fn frames_since(last_delivery: Option<Instant>, period: u64) -> u64 {
    // [@ANCHOR: vnwa_open_capture_frames_since]
    match last_delivery {
        Some(t) => ((t.elapsed().as_secs_f64() * SAMPLE_RATE as f64) as u64).min(period),
        None => 0,
    }
}

/// How many of `overruns` fall in the span from `OVERRUN_GUARD_FRAMES` before `mark` to the frame `last`.
fn overruns_in(overruns: &std::collections::VecDeque<Overrun>, mark: u64, last: u64) -> usize {
    overruns
        .iter()
        .filter(|o| o.at_frame + OVERRUN_GUARD_FRAMES >= mark && o.at_frame <= last)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: vnwa_open_capture_close]
    #[test]
    fn a_storm_of_reports_is_told_from_single_overruns_by_the_count_without_audio_between() {
        // One overrun every few hundred ms (a loaded host) never reaches the bound; 100 in a row with no audio is a stream that cannot run.
        assert!(!is_storm(1) && !is_storm(STORM_REPORTS - 1) && is_storm(STORM_REPORTS));
        // The dry run of 2026-10-29: 72,337 reports in 26 s, about 2,800 a second, so the bound is reached in 36 ms.
        assert!(STORM_REPORTS as f64 / 2_800.0 < 0.05);
    }

    // Tests [@ANCHOR: vnwa_open_capture_mark]
    // Tests [@ANCHOR: vnwa_open_capture_frames_since]
    #[test]
    fn the_mark_adds_the_frames_since_the_last_delivery_up_to_one_period() {
        assert_eq!(frames_since(None, 4096), 0);
        let t = Instant::now() - Duration::from_millis(10);
        let n = frames_since(Some(t), 4096);
        assert!((480..=560).contains(&n), "{n}");
        // A stalled stream cannot push the mark past one period.
        let old = Instant::now() - Duration::from_secs(5);
        assert_eq!(frames_since(Some(old), 241), 241);
    }

    // Tests [@ANCHOR: vnwa_open_overrun_window]
    #[test]
    fn an_overrun_counts_against_a_window_only_from_just_before_the_mark_to_its_end() {
        let at = Instant::now();
        let o = |f: u64| Overrun { at, at_frame: f };
        let log: std::collections::VecDeque<Overrun> =
            [o(1_000), o(50_000), o(60_000)].into_iter().collect();
        // Window from mark 50_000 to 55_000: only the overrun at 50_000.
        assert_eq!(overruns_in(&log, 50_000, 55_000), 1);
        // An overrun reported 500 frames before the mark may be the loss just before it: counted; 2_000 before: not.
        assert_eq!(overruns_in(&log, 50_500, 55_000), 1);
        assert_eq!(overruns_in(&log, 52_000, 55_000), 0);
        // After the window's end: not counted.
        assert_eq!(overruns_in(&log, 51_000, 59_999), 0);
        assert_eq!(overruns_in(&log, 51_000, 60_000), 1);
        assert!(is_overrun_error(&format!(
            "{OVERRUN_ERROR}: 2 buffer overrun(s)"
        )));
        assert!(!is_overrun_error("audio: 3 of 5 frames arrived in time"));
    }
}

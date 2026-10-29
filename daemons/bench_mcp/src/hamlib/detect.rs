// SPDX-License-Identifier: AGPL-3.0-or-later
//! Finding a real signal in a receiver's audio: the tones and ticks that time-and-frequency stations carry, and the steady beat
//! note of a carrier heard in a single-sideband mode. Pure functions on samples (mono, full scale 1.0).
//!
//! A time station (WWV and WWVH are the stations this bench lists) sends a short 1 kHz (WWV) or 1.2 kHz (WWVH) tick every second
//! and, at known minutes, 440, 500 and 600 Hz tones; the module does not rely on the minute schedule. It looks for a narrow
//! spectral peak at any of those frequencies, and for the once-per-second rhythm of the ticks, which shows a station well below the
//! level at which the S-meter moves. Mains hum (60 and 50 Hz and their harmonics) is recognised as a comb of peaks and is not a
//! signal; the harmonics that coincide with a target tone (600 and 1200 Hz for 60 Hz mains) are then left out of the tone check.

use crate::audio::analysis::{db, peaks, rms, spectrum, tone_amp, Spectrum};
use serde_json::{json, Value};

/// Tones a time station carries, in Hz.
pub const TIME_TONES_HZ: [f64; 5] = [440.0, 500.0, 600.0, 1000.0, 1200.0];
/// Tick frequencies: WWV and WWVH.
pub const TICK_HZ: [f64; 2] = [1000.0, 1200.0];
const FFT_LEN: usize = 8192;
/// A tone must stand this far over the median level around it to count.
const MIN_OVER_FLOOR_DB: f64 = 15.0;
/// A tone must be narrow: this far over the level 35 Hz away on both sides.
const MIN_OVER_SKIRT_DB: f64 = 10.0;
/// Below this level (dBFS) a peak is numerical noise of a silent input, not a signal.
const MIN_LEVEL_DBFS: f64 = -85.0;
const TICK_WIN_S: f64 = 0.010;
const TICK_HOP_S: f64 = 0.0025;
const TICK_MIN_CONTRAST_DB: f64 = 8.0;
const TICK_MIN_CONSISTENT: f64 = 0.6;
const TICK_MIN_SECONDS: usize = 6;

#[derive(Clone, Debug, PartialEq)]
pub struct ToneHit {
    pub hz: f64,
    pub level_dbfs: f64,
    pub over_floor_db: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TickHit {
    pub hz: f64,
    /// Where in the second the tick falls.
    pub phase_s: f64,
    pub contrast_db: f64,
    /// The fraction of the seconds in which the tick fell at that phase.
    pub consistent: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Findings {
    pub all_zero: bool,
    pub rms_dbfs: f64,
    pub tones: Vec<ToneHit>,
    pub ticks: Vec<TickHit>,
    /// Narrow steady peaks that are not mains harmonics or station tones: carriers heard as beat notes, receiver birdies.
    pub beats_hz: Vec<f64>,
    pub mains_hum_hz: Option<f64>,
}

impl Findings {
    /// A time station is heard when a tone or the tick rhythm is found.
    pub fn time_station(&self) -> bool {
        !self.tones.is_empty() || !self.ticks.is_empty()
    }

    pub fn json(&self) -> Value {
        let r1 = |x: f64| (x * 10.0).round() / 10.0;
        json!({
            "input_all_zero": self.all_zero, "rms_dbfs": r1(self.rms_dbfs), "time_station_heard": self.time_station(),
            "tones": self.tones.iter().map(|t| json!({"hz": t.hz, "level_dbfs": r1(t.level_dbfs), "over_floor_db": r1(t.over_floor_db)})).collect::<Vec<_>>(),
            "ticks": self.ticks.iter().map(|t| json!({"hz": t.hz, "phase_s": (t.phase_s * 1000.0).round() / 1000.0, "contrast_db": r1(t.contrast_db), "consistent_fraction": (t.consistent * 100.0).round() / 100.0})).collect::<Vec<_>>(),
            "beat_notes_hz": self.beats_hz.iter().map(|b| b.round()).collect::<Vec<_>>(),
            "mains_hum_hz": self.mains_hum_hz,
        })
    }
}

fn level_near(sp: &Spectrum, hz: f64, bins: usize) -> f64 {
    let centre = (hz / sp.bin_hz).round() as usize;
    (centre.saturating_sub(bins)..=(centre + bins).min(sp.amp.len() - 1))
        .map(|i| sp.amp[i])
        .fold(0.0, f64::max)
}

/// The mains frequency (60 or 50 Hz) when its harmonics form a comb of spectral peaks: at least three of the first ten, and more of them than the other frequency's.
pub fn mains_comb(sp: &Spectrum) -> Option<f64> {
    // [@ANCHOR: hamlib_detect_hum]
    let found = peaks(sp, 40.0, 1500.0, 30, 10.0);
    let score = |base: f64| {
        (1..=10)
            .filter(|k| {
                found
                    .iter()
                    .any(|p| (p.hz - base * f64::from(*k)).abs() < 3.0)
            })
            .count()
    };
    let (s60, s50) = (score(60.0), score(50.0));
    match (s60 >= 3 || s50 >= 3, s60 >= s50) {
        (false, _) => None,
        (true, true) => Some(60.0),
        (true, false) => Some(50.0),
    }
}

fn near_mains(hz: f64, base: Option<f64>) -> bool {
    base.is_some_and(|b| {
        let k = (hz / b).round();
        k >= 1.0 && (hz - k * b).abs() < 6.0
    })
}

/// Narrow peaks at the time-station tones.
fn tone_hits(sp: &Spectrum, mains: Option<f64>) -> Vec<ToneHit> {
    // [@ANCHOR: hamlib_detect_tones]
    let mut out = vec![];
    for hz in TIME_TONES_HZ {
        if near_mains(hz, mains) {
            continue;
        }
        let peak = level_near(sp, hz, 2);
        let level = db(peak);
        // The floor: the median level around the tone, leaving out the tone and the other station tones.
        let mut around: Vec<f64> = (0..sp.amp.len())
            .filter(|i| {
                let f = *i as f64 * sp.bin_hz;
                (f - hz).abs() > 40.0
                    && (f - hz).abs() <= 300.0
                    && TIME_TONES_HZ.iter().all(|t| (f - t).abs() > 20.0)
            })
            .map(|i| sp.db_at(i))
            .collect();
        around.sort_by(|a, b| a.total_cmp(b));
        let Some(&floor) = around.get(around.len() / 2) else {
            continue;
        };
        let skirt = db(level_near(sp, hz - 35.0, 1)).max(db(level_near(sp, hz + 35.0, 1)));
        if level > MIN_LEVEL_DBFS
            && level - floor >= MIN_OVER_FLOOR_DB
            && level - skirt >= MIN_OVER_SKIRT_DB
        {
            out.push(ToneHit {
                hz,
                level_dbfs: level,
                over_floor_db: level - floor,
            });
        }
    }
    out
}

/// The once-per-second tick at `hz`: the envelope of the band around `hz` folded on one second.
pub fn tick_train(s: &[f32], rate: u32, hz: f64) -> Option<TickHit> {
    // [@ANCHOR: hamlib_detect_ticks]
    let r = f64::from(rate);
    let (win, hop) = ((TICK_WIN_S * r) as usize, (TICK_HOP_S * r) as usize);
    let per_second = (1.0 / TICK_HOP_S).round() as usize;
    let seconds = (s.len() as f64 / r).floor() as usize;
    if seconds < TICK_MIN_SECONDS || win == 0 || hop == 0 {
        return None;
    }
    let total = seconds * per_second;
    let env: Vec<f64> = (0..total)
        .map(|i| {
            let at = i * hop;
            tone_amp(&s[at..(at + win).min(s.len())], rate, hz)
        })
        .collect();
    let mut folded = vec![0.0; per_second];
    for (i, e) in env.iter().enumerate() {
        folded[i % per_second] += e / seconds as f64;
    }
    let mut sorted = folded.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = sorted[per_second / 2];
    let (best, &top) = folded
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))?;
    if db(top) < MIN_LEVEL_DBFS {
        return None;
    }
    let contrast = db(top) - db(median);
    let agree = (0..seconds)
        .filter(|sec| {
            let slice = &env[sec * per_second..(sec + 1) * per_second];
            let (p, _) = slice
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap_or((0, &0.0));
            let d = (p as i64 - best as i64).rem_euclid(per_second as i64);
            d.min(per_second as i64 - d) <= 3
        })
        .count() as f64
        / seconds as f64;
    (contrast >= TICK_MIN_CONTRAST_DB && agree >= TICK_MIN_CONSISTENT).then_some(TickHit {
        hz,
        phase_s: best as f64 * TICK_HOP_S,
        contrast_db: contrast,
        consistent: agree,
    })
}

/// Steady narrow peaks between 150 Hz and 3.5 kHz that are neither mains harmonics nor station tones.
fn beat_notes(sp: &Spectrum, mains: Option<f64>) -> Vec<f64> {
    // [@ANCHOR: hamlib_detect_beats]
    peaks(sp, 150.0, 3500.0, 8, 25.0)
        .into_iter()
        .filter(|p| {
            p.dbfs > MIN_LEVEL_DBFS
                && !near_mains(p.hz, mains)
                && TIME_TONES_HZ.iter().all(|t| (p.hz - t).abs() > 12.0)
                && db(level_near(sp, p.hz - 40.0, 1)).max(db(level_near(sp, p.hz + 40.0, 1)))
                    <= p.dbfs - MIN_OVER_SKIRT_DB
        })
        .map(|p| p.hz)
        .collect()
}

/// Everything the audio shows about a possible station.
pub fn analyze(s: &[f32], rate: u32) -> Result<Findings, String> {
    // [@ANCHOR: hamlib_detect_analyze]
    let all_zero = s.iter().all(|x| *x == 0.0);
    let rms_dbfs = db(rms(s));
    if all_zero || s.len() < FFT_LEN {
        return Ok(Findings {
            all_zero,
            rms_dbfs,
            tones: vec![],
            ticks: vec![],
            beats_hz: vec![],
            mains_hum_hz: None,
        });
    }
    let sp = spectrum(s, rate, FFT_LEN)?;
    let mains = mains_comb(&sp);
    Ok(Findings {
        all_zero,
        rms_dbfs,
        tones: tone_hits(&sp, mains),
        ticks: TICK_HZ
            .iter()
            .filter_map(|hz| tick_train(s, rate, *hz))
            .collect(),
        beats_hz: beat_notes(&sp, mains),
        mains_hum_hz: mains,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::analysis::synth::*;

    const R: u32 = 48000;

    /// A WWV-like second tick train: `ms` of `hz` at the start of every second, `secs` seconds long.
    fn ticks(secs: f64, hz: f64, ms: f64, amp: f64, offset_s: f64) -> Vec<f32> {
        let mut v = vec![0.0f32; (secs * f64::from(R)) as usize];
        let burst = sine(R, ms / 1000.0, hz, amp);
        let mut t = offset_s;
        while t + ms / 1000.0 < secs {
            let start = (t * f64::from(R)) as usize;
            for (i, x) in burst.iter().enumerate() {
                v[start + i] += *x;
            }
            t += 1.0;
        }
        v
    }

    fn hum(secs: f64, base: f64, amp: f64) -> Vec<f32> {
        (1..=10).fold(vec![0.0; (secs * f64::from(R)) as usize], |acc, k| {
            add(
                &acc,
                &sine(R, secs, base * f64::from(k), amp / f64::from(k)),
            )
        })
    }

    // Tests [@ANCHOR: hamlib_detect_ticks]
    #[test]
    fn a_one_second_tick_train_is_found_with_and_without_noise_and_silence_has_none() {
        let clean = ticks(10.0, 1000.0, 5.0, 0.3, 0.37);
        let t = tick_train(&clean, R, 1000.0).expect("ticks in clean audio");
        assert!((t.phase_s - 0.37).abs() < 0.012, "{t:?}");
        assert!(t.consistent > 0.9 && t.contrast_db > 15.0, "{t:?}");
        let noisy = add(&clean, &noise(R, 10.0, 0.05, 7));
        assert!(
            tick_train(&noisy, R, 1000.0).is_some(),
            "ticks 15 dB over the noise per sample are found"
        );
        let wwvh = add(
            &ticks(10.0, 1200.0, 5.0, 0.3, 0.1),
            &noise(R, 10.0, 0.05, 9),
        );
        assert!(tick_train(&wwvh, R, 1200.0).is_some());
        assert!(tick_train(&noise(R, 10.0, 0.05, 3), R, 1000.0).is_none());
        assert!(tick_train(&vec![0.0; R as usize * 10], R, 1000.0).is_none());
        assert!(
            tick_train(&clean[..R as usize * 3], R, 1000.0).is_none(),
            "too short to judge a rhythm"
        );
        // A steady 1 kHz tone has no rhythm.
        assert!(tick_train(&sine(R, 10.0, 1000.0, 0.2), R, 1000.0).is_none());
    }

    // Tests [@ANCHOR: hamlib_detect_tones]
    #[test]
    fn minute_tones_are_found_by_their_narrow_peak_and_broadband_noise_is_not_a_tone() {
        for hz in [440.0, 500.0, 600.0] {
            let s = add(&sine(R, 4.0, hz, 0.05), &noise(R, 4.0, 0.05, 11));
            let f = analyze(&s, R).unwrap();
            assert_eq!(
                f.tones.iter().map(|t| t.hz).collect::<Vec<_>>(),
                [hz],
                "{hz}: {f:?}"
            );
            assert!(f.time_station());
        }
        let f = analyze(&noise(R, 4.0, 0.1, 5), R).unwrap();
        assert!(
            f.tones.is_empty() && f.ticks.is_empty() && f.beats_hz.is_empty(),
            "{f:?}"
        );
        assert!(!f.time_station());
    }

    // Tests [@ANCHOR: hamlib_detect_hum]
    #[test]
    fn mains_hum_at_60_and_120_hertz_and_its_harmonics_is_not_a_station() {
        for base in [60.0, 50.0] {
            let s = add(&hum(4.0, base, 0.2), &noise(R, 4.0, 0.01, 2));
            let f = analyze(&s, R).unwrap();
            assert_eq!(f.mains_hum_hz, Some(base), "{f:?}");
            assert!(!f.time_station(), "hum at {base} Hz: {f:?}");
            assert!(
                f.beats_hz.is_empty(),
                "hum harmonics are not beat notes: {f:?}"
            );
        }
        // A real 500 Hz tone with hum at 60 Hz is still found: 500 Hz is not a 60 Hz harmonic.
        let both = add(
            &add(&hum(4.0, 60.0, 0.1), &sine(R, 4.0, 500.0, 0.05)),
            &noise(R, 4.0, 0.01, 4),
        );
        assert_eq!(analyze(&both, R).unwrap().tones.len(), 1);
    }

    // Tests [@ANCHOR: hamlib_detect_beats]
    #[test]
    fn a_steady_narrow_note_is_a_beat_note_and_not_a_time_station_tone() {
        let s = add(&sine(R, 4.0, 1500.0, 0.05), &noise(R, 4.0, 0.01, 6));
        let f = analyze(&s, R).unwrap();
        assert_eq!(f.beats_hz.len(), 1, "{f:?}");
        assert!((f.beats_hz[0] - 1500.0).abs() < 3.0);
        assert!(f.tones.is_empty() && !f.time_station());
    }

    // Tests [@ANCHOR: hamlib_detect_analyze]
    #[test]
    fn analysis_reports_silence_and_dead_inputs_and_a_full_station() {
        let dead = analyze(&vec![0.0; R as usize * 4], R).unwrap();
        assert!(dead.all_zero && !dead.time_station());
        let short = analyze(&sine(R, 0.05, 440.0, 0.1), R).unwrap();
        assert!(short.tones.is_empty(), "too short to judge");
        let station = add(
            &add(
                &ticks(10.0, 1000.0, 5.0, 0.3, 0.2),
                &sine(R, 10.0, 500.0, 0.05),
            ),
            &noise(R, 10.0, 0.03, 8),
        );
        let f = analyze(&station, R).unwrap();
        assert!(
            f.ticks.iter().any(|t| t.hz == 1000.0) && f.tones.iter().any(|t| t.hz == 500.0),
            "{f:?}"
        );
        assert!(f.json()["time_station_heard"] == true);
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! The decision "was this tone heard?" on a recording: the level of one frequency against the noise before it and against
//! neighbouring frequencies. Used by the loopback test, by sound verification and by path discovery. Pure functions.

use super::analysis::{db, tone_amp, FLOOR_DB};
use serde_json::{json, Value};

/// The analysis window and hop for tone tracking, in seconds.
const WIN_S: f64 = 0.03;
const HOP_S: f64 = 0.01;

#[derive(Clone, Debug, PartialEq)]
pub struct Heard {
    pub heard: bool,
    pub hz: f64,
    /// The strongest level of the tone in dBFS (peak amplitude).
    pub level_dbfs: f64,
    /// The level at the same frequency in the part of the recording before the tone was expected.
    pub noise_dbfs: f64,
    pub snr_db: f64,
    /// How far the tone stands above the loudest of two neighbouring frequencies during the tone.
    pub over_neighbours_db: f64,
    pub onset_s: Option<f64>,
    pub offset_s: Option<f64>,
    pub input_all_zero: bool,
    pub reason: String,
}

impl Heard {
    pub fn json(&self) -> Value {
        json!({
            "heard": self.heard, "freq_hz": self.hz, "level_dbfs": round1(self.level_dbfs), "noise_dbfs": round1(self.noise_dbfs),
            "snr_db": round1(self.snr_db), "over_neighbouring_frequencies_db": round1(self.over_neighbours_db),
            "onset_s": self.onset_s.map(round3), "offset_s": self.offset_s.map(round3),
            "input_all_zero": self.input_all_zero, "reason": self.reason,
        })
    }
}

pub fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

pub fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn tracks(s: &[f32], rate: u32, hz: f64, from: usize, to: usize) -> Vec<(f64, f64)> {
    let win = (WIN_S * f64::from(rate)) as usize;
    let hop = (HOP_S * f64::from(rate)) as usize;
    let mut v = Vec::new();
    let mut at = from;
    while at + win <= to.min(s.len()) {
        v.push((
            at as f64 / f64::from(rate),
            tone_amp(&s[at..at + win], rate, hz),
        ));
        at += hop;
    }
    v
}

/// Was a tone of `hz` heard in `s` (mono, `rate`)? The tone is expected from `expect_from_s` on; the recording before that is the
/// noise reference. `min_snr_db` is the least level over the pre-tone noise at the same frequency (and over the neighbouring
/// frequencies during the tone) that counts as heard.
pub fn hear_tone(s: &[f32], rate: u32, hz: f64, expect_from_s: f64, min_snr_db: f64) -> Heard {
    // [@ANCHOR: audio_heard_tone]
    let r = f64::from(rate);
    let split = ((expect_from_s * r) as usize).min(s.len());
    let all_zero = s.iter().all(|x| *x == 0.0);
    let mut out = Heard {
        heard: false,
        hz,
        level_dbfs: FLOOR_DB,
        noise_dbfs: FLOOR_DB,
        snr_db: 0.0,
        over_neighbours_db: 0.0,
        onset_s: None,
        offset_s: None,
        input_all_zero: all_zero,
        reason: String::new(),
    };
    if all_zero {
        out.reason =
            "the input recorded exact digital zeros: it is muted, disconnected or dead".into();
        return out;
    }
    let win = (WIN_S * r) as usize;
    if s.len() < split + win * 2 || split < win * 2 {
        out.reason =
            "the recording is too short to compare the tone with the noise before it".into();
        return out;
    }
    // Noise reference: the median level at the tone frequency in the pre-tone part.
    let mut pre: Vec<f64> = tracks(s, rate, hz, 0, split)
        .into_iter()
        .map(|t| t.1)
        .collect();
    pre.sort_by(|a, b| a.total_cmp(b));
    let noise = pre.get(pre.len() * 3 / 4).copied().unwrap_or(0.0);
    out.noise_dbfs = db(noise);
    let during = tracks(s, rate, hz, split, s.len());
    let Some(&(_, peak_amp)) = during.iter().max_by(|a, b| a.1.total_cmp(&b.1)) else {
        out.reason = "no recording after the tone start".into();
        return out;
    };
    out.level_dbfs = db(peak_amp);
    // The reference never goes below the 16-bit quantisation floor, so a digitally clean input does not give huge SNRs.
    out.snr_db = out.level_dbfs - out.noise_dbfs.max(-96.0);
    // Neighbours: the same window position, a quarter octave off on each side.
    let peak_at = during.iter().position(|t| t.1 == peak_amp).unwrap_or(0);
    let at = split + peak_at * (HOP_S * r) as usize;
    let end = (at + win).min(s.len());
    let nb = tone_amp(&s[at..end], rate, hz * 1.19).max(tone_amp(&s[at..end], rate, hz / 1.19));
    out.over_neighbours_db = out.level_dbfs - db(nb);
    // Onset and offset: where the track crosses 6 dB below its peak.
    let thr = peak_amp / 2.0;
    out.onset_s = during
        .iter()
        .find(|t| t.1 >= thr)
        .map(|t| t.0 + WIN_S / 2.0);
    out.offset_s = during
        .iter()
        .rev()
        .find(|t| t.1 >= thr)
        .map(|t| t.0 + WIN_S / 2.0);
    if out.level_dbfs < -100.0 {
        out.reason = format!(
            "nothing at {hz:.0} Hz: the level is {:.0} dBFS",
            out.level_dbfs
        );
    } else if out.snr_db < min_snr_db {
        out.reason = format!(
            "the tone is {:.1} dB over the noise at {hz:.0} Hz before it, under the {min_snr_db:.0} dB needed",
            out.snr_db
        );
    } else if out.over_neighbours_db < 10.0 {
        out.reason = format!(
            "the level at {hz:.0} Hz is only {:.1} dB over its neighbouring frequencies: broadband sound, not the tone",
            out.over_neighbours_db
        );
    } else {
        out.heard = true;
        out.reason = format!(
            "the {hz:.0} Hz tone reached {:.1} dBFS, {:.1} dB over the noise before it",
            out.level_dbfs, out.snr_db
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::analysis::synth::*;

    const R: u32 = 48000;

    fn rec(tone_db: Option<f64>, noise_rms: f64, hz: f64) -> Vec<f32> {
        let n = noise(R, 1.5, noise_rms, 11);
        match tone_db {
            None => n,
            Some(d) => {
                let t = sine(R, 0.4, hz, 10f64.powf(d / 20.0));
                add(&n, &at(R, 1.5, 0.6, &t))
            }
        }
    }

    // Tests [@ANCHOR: audio_heard_tone]
    #[test]
    fn a_clear_tone_over_room_noise_is_heard_with_its_times() {
        let h = hear_tone(&rec(Some(-30.0), 0.003, 1300.0), R, 1300.0, 0.5, 12.0);
        assert!(h.heard, "{h:?}");
        assert!((h.level_dbfs + 30.0).abs() < 1.5, "{h:?}");
        assert!(h.snr_db > 20.0 && h.over_neighbours_db > 10.0);
        assert!((h.onset_s.unwrap() - 0.6).abs() < 0.03, "{h:?}");
        assert!((h.offset_s.unwrap() - 1.0).abs() < 0.04, "{h:?}");
    }

    #[test]
    fn noise_alone_and_a_tone_at_another_frequency_are_not_heard() {
        let h = hear_tone(&rec(None, 0.003, 1300.0), R, 1300.0, 0.5, 12.0);
        assert!(!h.heard, "{h:?}");
        assert!(
            h.reason.contains("dB over the noise") || h.reason.contains("neighbour"),
            "{h:?}"
        );
        let other = hear_tone(&rec(Some(-30.0), 0.003, 2100.0), R, 1300.0, 0.5, 12.0);
        assert!(!other.heard, "{other:?}");
    }

    #[test]
    fn a_tone_barely_over_the_noise_is_refused_and_a_loud_noise_burst_is_not_a_tone() {
        let weak = hear_tone(&rec(Some(-76.0), 0.003, 1000.0), R, 1000.0, 0.5, 12.0);
        assert!(!weak.heard, "{weak:?}");
        let n = noise(R, 1.5, 0.003, 5);
        let burst = at(R, 1.5, 0.6, &noise(R, 0.4, 0.2, 9));
        let h = hear_tone(&add(&n, &burst), R, 1000.0, 0.5, 12.0);
        assert!(!h.heard, "{h:?}");
        assert!(h.reason.contains("broadband"), "{h:?}");
    }

    #[test]
    fn an_all_zero_input_is_reported_as_dead_and_a_short_recording_as_too_short() {
        let h = hear_tone(&vec![0.0; 72000], R, 1000.0, 0.5, 12.0);
        assert!(h.input_all_zero && !h.heard && h.reason.contains("digital zeros"));
        let short = hear_tone(&sine(R, 0.05, 1000.0, 0.1), R, 1000.0, 0.5, 12.0);
        assert!(
            !short.heard && short.reason.contains("too short"),
            "{short:?}"
        );
    }

    #[test]
    fn a_digitally_quiet_but_live_input_can_still_hear_a_tone() {
        // Quantised noise at about -80 dBFS: the reference floor is the 16-bit floor, not the nothing-at-all.
        let h = hear_tone(&rec(Some(-40.0), 0.0001, 1700.0), R, 1700.0, 0.5, 12.0);
        assert!(h.heard, "{h:?}");
        assert!(h.snr_db <= 60.0 + 1e-9);
    }
}

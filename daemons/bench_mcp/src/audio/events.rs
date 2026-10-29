// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sound events in a recording: when a sound starts and stops, how loud it is and what its pitch is. For relays clicking, a
//! rotor motor, an alarm tone, beeps, and keyed CW (with a simple Morse decoder). Pure functions.

use super::analysis::{db, dominant, rms, spectrum, FLOOR_DB};
use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct Params {
    pub frame_ms: f64,
    /// How far over the noise reference a frame must be to count as sound.
    pub margin_db: f64,
    /// A fixed threshold in dBFS (RMS of a frame), instead of noise reference plus margin.
    pub threshold_dbfs: Option<f64>,
    /// The noise reference in dBFS (RMS of a frame), when it was measured separately; otherwise the 20th percentile of the frames.
    pub noise_dbfs: Option<f64>,
    pub merge_gap_ms: f64,
    pub min_event_ms: f64,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            frame_ms: 5.0,
            margin_db: 10.0,
            threshold_dbfs: None,
            noise_dbfs: None,
            merge_gap_ms: 40.0,
            min_event_ms: 5.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub onset_s: f64,
    pub offset_s: f64,
    pub peak_dbfs: f64,
    pub rms_dbfs: f64,
    /// The pitch, when the event has a clear spectral peak.
    pub hz: Option<f64>,
}

impl Event {
    pub fn duration_s(&self) -> f64 {
        self.offset_s - self.onset_s
    }

    pub fn json(&self) -> Value {
        json!({
            "onset_s": r3(self.onset_s), "offset_s": r3(self.offset_s), "duration_s": r3(self.duration_s()),
            "peak_dbfs": r1(self.peak_dbfs), "rms_dbfs": r1(self.rms_dbfs), "dominant_hz": self.hz.map(r1),
        })
    }
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// RMS level in dBFS of consecutive frames of `frame_ms`.
pub fn frame_levels(s: &[f32], rate: u32, frame_ms: f64) -> Vec<f64> {
    let n = ((frame_ms / 1000.0) * f64::from(rate)).max(1.0) as usize;
    s.chunks(n)
        .filter(|c| c.len() == n)
        .map(|c| db(rms(c)))
        .collect()
}

pub fn percentile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return FLOOR_DB;
    }
    let mut x = v.to_vec();
    x.sort_by(|a, b| a.total_cmp(b));
    x[((x.len() - 1) as f64 * p).round() as usize]
}

pub struct Detection {
    pub noise_dbfs: f64,
    pub threshold_dbfs: f64,
    pub events: Vec<Event>,
}

pub fn detect(s: &[f32], rate: u32, p: &Params) -> Detection {
    // [@ANCHOR: audio_events_detect]
    let levels = frame_levels(s, rate, p.frame_ms);
    let noise = p.noise_dbfs.unwrap_or_else(|| percentile(&levels, 0.2));
    // The reference never goes below the 16-bit floor, so a digitally silent stretch does not make every sample an event.
    let threshold = p
        .threshold_dbfs
        .unwrap_or_else(|| noise.max(-96.0) + p.margin_db);
    let frame_s = p.frame_ms / 1000.0;
    let frame_n = ((frame_s * f64::from(rate)).max(1.0)) as usize;
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, l) in levels.iter().enumerate() {
        match start {
            None if *l >= threshold => start = Some(i),
            Some(st) if *l < threshold - 3.0 => {
                spans.push((st, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(st) = start {
        spans.push((st, levels.len()));
    }
    let gap = (p.merge_gap_ms / p.frame_ms).ceil() as usize;
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for sp in spans {
        match merged.last_mut() {
            Some(last) if sp.0 - last.1 <= gap => last.1 = sp.1,
            _ => merged.push(sp),
        }
    }
    let min_frames = (p.min_event_ms / p.frame_ms).ceil().max(1.0) as usize;
    let events = merged
        .into_iter()
        .filter(|(a, b)| b - a >= min_frames)
        .map(|(a, b)| {
            let seg = &s[a * frame_n..(b * frame_n).min(s.len())];
            let peak = seg.iter().fold(0f64, |m, x| m.max(f64::from(x.abs())));
            let nyq = f64::from(rate) / 2.0;
            let hz = spectrum(&seg[..seg.len().min(16384)], rate, 2048)
                .ok()
                .and_then(|sp| dominant(&sp, 100.0, nyq * 0.9, 15.0))
                .map(|pk| pk.hz);
            Event {
                onset_s: a as f64 * frame_s,
                offset_s: b as f64 * frame_s,
                peak_dbfs: db(peak),
                rms_dbfs: db(rms(seg)),
                hz,
            }
        })
        .collect();
    Detection {
        noise_dbfs: noise,
        threshold_dbfs: threshold,
        events,
    }
}

const MORSE: &[(&str, char)] = &[
    (".-", 'A'),
    ("-...", 'B'),
    ("-.-.", 'C'),
    ("-..", 'D'),
    (".", 'E'),
    ("..-.", 'F'),
    ("--.", 'G'),
    ("....", 'H'),
    ("..", 'I'),
    (".---", 'J'),
    ("-.-", 'K'),
    (".-..", 'L'),
    ("--", 'M'),
    ("-.", 'N'),
    ("---", 'O'),
    (".--.", 'P'),
    ("--.-", 'Q'),
    (".-.", 'R'),
    ("...", 'S'),
    ("-", 'T'),
    ("..-", 'U'),
    ("...-", 'V'),
    (".--", 'W'),
    ("-..-", 'X'),
    ("-.--", 'Y'),
    ("--..", 'Z'),
    ("-----", '0'),
    (".----", '1'),
    ("..---", '2'),
    ("...--", '3'),
    ("....-", '4'),
    (".....", '5'),
    ("-....", '6'),
    ("--...", '7'),
    ("---..", '8'),
    ("----.", '9'),
    (".-.-.-", '.'),
    ("--..--", ','),
    ("..--..", '?'),
    ("-..-.", '/'),
    ("-...-", '='),
    (".-.-.", '+'),
    ("-....-", '-'),
];

#[derive(Debug, PartialEq)]
pub struct Morse {
    pub text: String,
    pub dot_s: f64,
    pub wpm: f64,
    pub symbols: String,
}

/// Decodes keyed tone events as Morse. The dot length is the mean of the shorter cluster of mark lengths (marks are split at the
/// midpoint between the shortest and the longest when the longest is over twice the shortest; otherwise all are dots).
pub fn decode_morse(events: &[Event]) -> Option<Morse> {
    // [@ANCHOR: audio_morse]
    if events.is_empty() {
        return None;
    }
    let marks: Vec<f64> = events.iter().map(|e| e.duration_s()).collect();
    let (lo, hi) = marks
        .iter()
        .fold((f64::MAX, 0f64), |(l, h), m| (l.min(*m), h.max(*m)));
    let split = if hi > 2.0 * lo {
        (lo + hi) / 2.0
    } else {
        f64::MAX
    };
    let dots: Vec<f64> = marks.iter().copied().filter(|m| *m < split).collect();
    let dot = dots.iter().sum::<f64>() / dots.len().max(1) as f64;
    let mut symbols = String::new();
    let mut text = String::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, text: &mut String| {
        if !cur.is_empty() {
            text.push(
                MORSE
                    .iter()
                    .find(|(c, _)| *c == cur.as_str())
                    .map_or('?', |(_, ch)| *ch),
            );
            cur.clear();
        }
    };
    for (i, e) in events.iter().enumerate() {
        if i > 0 {
            let gap = e.onset_s - events[i - 1].offset_s;
            if gap > 5.0 * dot {
                flush(&mut cur, &mut text);
                text.push(' ');
                symbols.push_str(" / ");
            } else if gap > 2.0 * dot {
                flush(&mut cur, &mut text);
                symbols.push(' ');
            }
        }
        let c = if e.duration_s() < split { '.' } else { '-' };
        cur.push(c);
        symbols.push(c);
    }
    flush(&mut cur, &mut text);
    Some(Morse {
        text: text.trim().to_string(),
        dot_s: dot,
        wpm: 1.2 / dot,
        symbols,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::analysis::synth::*;

    const R: u32 = 48000;

    fn beeps(on: &[(f64, f64)], total: f64, hz: f64, amp: f64, noise_rms: f64) -> Vec<f32> {
        let mut v = noise(R, total, noise_rms, 3);
        for (at_s, len) in on {
            v = add(&v, &at(R, total, *at_s, &sine(R, *len, hz, amp)));
        }
        v
    }

    // Tests [@ANCHOR: audio_events_detect]
    #[test]
    fn three_beeps_are_found_with_their_times_levels_and_pitch() {
        let s = beeps(
            &[(0.5, 0.2), (1.0, 0.2), (1.5, 0.2)],
            2.5,
            1800.0,
            0.1,
            0.002,
        );
        let d = detect(&s, R, &Params::default());
        assert_eq!(d.events.len(), 3, "{:?}", d.events);
        for (e, at) in d.events.iter().zip([0.5, 1.0, 1.5]) {
            assert!(
                (e.onset_s - at).abs() < 0.015 && (e.duration_s() - 0.2).abs() < 0.02,
                "{e:?}"
            );
            assert!((e.peak_dbfs - db(0.1)).abs() < 1.0);
            assert!((e.hz.unwrap() - 1800.0).abs() < 25.0, "{e:?}");
        }
        assert!(d.noise_dbfs < -50.0 && d.threshold_dbfs > d.noise_dbfs);
    }

    #[test]
    fn short_clicks_are_events_without_a_pitch_and_a_quiet_room_has_none() {
        let mut v = noise(R, 2.0, 0.001, 8);
        for at_s in [0.4, 0.9, 1.4] {
            let click = noise(R, 0.004, 0.3, (at_s * 1000.0) as u64);
            v = add(&v, &at(R, 2.0, at_s, &click));
        }
        let d = detect(&v, R, &Params::default());
        assert_eq!(d.events.len(), 3, "{:?}", d.events);
        assert!(d
            .events
            .iter()
            .all(|e| e.hz.is_none() && e.duration_s() < 0.03));
        assert!(detect(&noise(R, 2.0, 0.001, 9), R, &Params::default())
            .events
            .is_empty());
    }

    #[test]
    fn digital_silence_gives_no_events_and_a_fixed_threshold_is_honoured() {
        assert!(detect(&vec![0.0; 48000], R, &Params::default())
            .events
            .is_empty());
        let s = beeps(&[(0.5, 0.2)], 1.5, 1000.0, 0.01, 0.0005);
        let hi = Params {
            threshold_dbfs: Some(-30.0),
            ..Params::default()
        };
        assert!(detect(&s, R, &hi).events.is_empty());
        let lo = Params {
            threshold_dbfs: Some(-45.0),
            ..Params::default()
        };
        assert_eq!(detect(&s, R, &lo).events.len(), 1);
    }

    #[test]
    fn a_gap_shorter_than_the_merge_gap_joins_two_bursts() {
        let s = beeps(&[(0.5, 0.2), (0.72, 0.2)], 1.5, 1000.0, 0.1, 0.001);
        assert_eq!(detect(&s, R, &Params::default()).events.len(), 1);
        let apart = Params {
            merge_gap_ms: 5.0,
            ..Params::default()
        };
        assert_eq!(detect(&s, R, &apart).events.len(), 2);
    }

    // Tests [@ANCHOR: audio_morse]
    #[test]
    fn keyed_cw_decodes_to_text_with_its_speed() {
        // "CQ DE K": 60 ms dots (20 wpm), dashes 3 dots, letter gap 3 dots, word gap 7 dots.
        let dot = 0.06;
        let word = "-.-. --.- / -.. . / -.-";
        let mut t = 0.3;
        let mut marks = Vec::new();
        for c in word.chars() {
            match c {
                '.' => {
                    marks.push((t, dot));
                    t += 2.0 * dot;
                }
                '-' => {
                    marks.push((t, 3.0 * dot));
                    t += 4.0 * dot;
                }
                ' ' => t += 2.0 * dot,
                _ => t += 4.0 * dot,
            }
        }
        let s = beeps(&marks, t + 0.5, 700.0, 0.2, 0.002);
        let d = detect(&s, R, &Params::default());
        let m = decode_morse(&d.events).unwrap();
        assert_eq!(m.text, "CQ DE K", "{m:?} {:?}", d.events.len());
        assert!((m.wpm - 20.0).abs() < 2.0, "{m:?}");
        assert!(decode_morse(&[]).is_none());
    }
}

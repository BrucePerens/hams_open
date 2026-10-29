// SPDX-License-Identifier: AGPL-3.0-or-later
//! The dummy-load plausibility check. With the rig in receive, on the path the owner says is a dummy load, it listens at the
//! transmit frequency and at reference frequencies that a real antenna hears (time stations, the AM band, weather radio) and
//! compares the readings with a BASELINE that was recorded when the owner attested the load. Signals or noise well above the
//! baseline mean an antenna-like or live path: the check refuses. Readings at the baseline are "consistent with a load".
//!
//! THIS IS EVIDENCE, NOT PROOF. An open or unplugged connection hears nothing and looks like a good load (the low-power SWR probe
//! of the transmit tools exists to catch that); an antenna on a dead band at low gain also hears nothing; the baseline is valid
//! only for the cabling and rig settings it was taken with and must be taken again after any change; and the owner's attestation
//! still gates transmitting. Any measurement error, an unreadable S-meter or a dead audio input refuses.

use super::attest::{LoadCheck, Transmit};
use super::audio_in::AudioSource;
use super::detect::{self, Findings};
use super::proto::Client;
use super::stations::{class_of, now_local, Station, Table};
use super::status::tolerate;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Everything measured at one frequency.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// The station's id, or `tx` for the transmit frequency.
    pub id: String,
    pub frequency_hz: f64,
    pub mode: String,
    /// Signal strength, dB relative to S9: the mean of the readings and their spread (highest minus lowest).
    pub mean_db: f64,
    pub spread_db: f64,
    pub squelch_open: Option<bool>,
    pub audio: Option<AudioObs>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioObs {
    pub rms_dbfs: f64,
    pub all_zero: bool,
    pub time_station: bool,
    pub beats_hz: Vec<f64>,
    pub detail: Value,
}

impl AudioObs {
    pub fn of(f: &Findings) -> AudioObs {
        AudioObs {
            rms_dbfs: f.rms_dbfs,
            all_zero: f.all_zero,
            time_station: f.time_station(),
            beats_hz: f.beats_hz.clone(),
            detail: f.json(),
        }
    }
}

impl Reading {
    pub fn json(&self) -> Value {
        let r1 = |x: f64| (x * 10.0).round() / 10.0;
        json!({
            "id": self.id, "frequency_hz": self.frequency_hz, "mode": self.mode, "strength_db_rel_s9": r1(self.mean_db), "spread_db": r1(self.spread_db),
            "squelch_open": self.squelch_open, "audio": self.audio.as_ref().map(|a| a.detail.clone()),
        })
    }

    fn from_json(v: &Value) -> Option<Reading> {
        Some(Reading {
            id: v["id"].as_str()?.into(),
            frequency_hz: v["frequency_hz"].as_f64()?,
            mode: v["mode"].as_str()?.into(),
            mean_db: v["mean_db"].as_f64()?,
            spread_db: v["spread_db"].as_f64()?,
            squelch_open: v["squelch_open"].as_bool(),
            audio: v.get("audio_obs").filter(|a| a.is_object()).and_then(|a| {
                Some(AudioObs {
                    rms_dbfs: a["rms_dbfs"].as_f64()?,
                    all_zero: a["all_zero"].as_bool()?,
                    time_station: a["time_station"].as_bool()?,
                    beats_hz: a["beats_hz"]
                        .as_array()?
                        .iter()
                        .filter_map(|b| b.as_f64())
                        .collect(),
                    detail: a["detail"].clone(),
                })
            }),
        })
    }

    /// The full record for the baseline file (the report form rounds).
    fn stored(&self) -> Value {
        json!({
            "id": self.id, "frequency_hz": self.frequency_hz, "mode": self.mode, "mean_db": self.mean_db, "spread_db": self.spread_db,
            "squelch_open": self.squelch_open,
            "audio_obs": self.audio.as_ref().map(|a| json!({"rms_dbfs": a.rms_dbfs, "all_zero": a.all_zero, "time_station": a.time_station, "beats_hz": a.beats_hz, "detail": a.detail})),
        })
    }
}

/// What was recorded when the owner attested the load.
#[derive(Clone, Debug, PartialEq)]
pub struct Baseline {
    pub taken_on: String,
    pub attested_by: String,
    pub cabling: String,
    pub settings: Value,
    pub readings: Vec<Reading>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// The readings match the baseline: consistent with a dummy load (not proof).
    Consistent,
    /// Something is above the baseline, a station was heard, or a measurement failed: do not transmit.
    Refuse,
    /// No baseline for this range: the readings are only reported; transmitting needs the owner to record one.
    NoBaseline,
}

impl Verdict {
    pub fn word(&self) -> &'static str {
        // [@ANCHOR: hamlib_verdict_word]
        match self {
            Verdict::Consistent => "consistent_with_a_load",
            Verdict::Refuse => "refuse",
            Verdict::NoBaseline => "no_baseline",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub verdict: Verdict,
    pub reasons: Vec<String>,
    /// How many readings were compared with the baseline.
    pub compared: usize,
}

/// A steady note in `now` that has no counterpart (within `tol` Hz) in the baseline.
fn new_beat(now: &[f64], base: &[f64], tol: f64) -> Option<f64> {
    now.iter()
        .copied()
        .find(|n| !base.iter().any(|b| (n - b).abs() <= tol))
}

/// The decision, on readings alone. `now` is what was just measured; `base` what was recorded at the attestation.
pub fn decide(
    base: Option<&Baseline>,
    now: &[Reading],
    th: &LoadCheck,
    settings: &Value,
) -> Decision {
    // [@ANCHOR: hamlib_load_decide]
    let mut reasons = vec![];
    if now.is_empty() {
        return Decision {
            verdict: Verdict::Refuse,
            reasons: vec!["no readings were taken".into()],
            compared: 0,
        };
    }
    for r in now {
        if !(r.mean_db.is_finite() && r.spread_db.is_finite()) {
            reasons.push(format!("{}: the S-meter reading is not a number", r.id));
        }
        if let Some(a) = &r.audio {
            if a.all_zero {
                reasons.push(format!("{}: the audio input recorded only zeros (muted, unplugged or dead), so the audio evidence is missing", r.id));
            }
            if a.time_station {
                reasons.push(format!("{} at {:.0} Hz: a time station's ticks or tones were heard through the supposed dummy-load path: the path is antenna-like", r.id, r.frequency_hz));
            }
        }
    }
    let Some(base) = base else {
        let verdict = if reasons.is_empty() {
            Verdict::NoBaseline
        } else {
            Verdict::Refuse
        };
        reasons.push("there is no baseline for this frequency range: the readings are reported only, and the transmit tools need the owner to record a baseline first".into());
        return Decision {
            verdict,
            reasons,
            compared: 0,
        };
    };
    if base.settings != *settings {
        reasons.push(
            "the baseline was recorded with other rig or check settings: record it again".into(),
        );
    }
    let mut compared = 0;
    for r in now {
        let Some(b) = base.readings.iter().find(|b| b.id == r.id) else {
            continue;
        };
        compared += 1;
        let spread = r.spread_db.max(b.spread_db);
        if spread > th.max_spread_db {
            reasons.push(format!("{}: the readings vary by {spread:.1} dB, more than {:.1} dB: too unsteady to tell a load from a signal", r.id, th.max_spread_db));
            continue;
        }
        let excess = r.mean_db - b.mean_db - th.margin_db - spread;
        if excess > 0.0 {
            reasons.push(format!("{} at {:.0} Hz: {:.1} dB over the baseline (allowed {:.1} dB plus the spread {:.1} dB): a signal or noise that a load would not give", r.id, r.frequency_hz, r.mean_db - b.mean_db, th.margin_db, spread));
        }
        if r.squelch_open == Some(true) && b.squelch_open != Some(true) {
            reasons.push(format!(
                "{}: the squelch is open and it was closed at the baseline",
                r.id
            ));
        }
        if let (Some(ra), Some(ba)) = (&r.audio, &b.audio) {
            if !ra.all_zero && ra.rms_dbfs - ba.rms_dbfs > th.audio_margin_db {
                reasons.push(format!(
                    "{}: the audio is {:.1} dB louder than at the baseline (allowed {:.1} dB)",
                    r.id,
                    ra.rms_dbfs - ba.rms_dbfs,
                    th.audio_margin_db
                ));
            }
            if let Some(n) = new_beat(&ra.beats_hz, &ba.beats_hz, 15.0) {
                reasons.push(format!("{}: a steady note at {n:.0} Hz that was not at the baseline: a carrier is being received", r.id));
            }
        }
    }
    if compared < 2 {
        reasons.push(format!("only {compared} of the {} readings are in the baseline: it does not cover this set of frequencies; record a new baseline", now.len()));
    }
    let verdict = if reasons.is_empty() {
        Verdict::Consistent
    } else {
        Verdict::Refuse
    };
    if verdict == Verdict::Consistent {
        reasons.push("the readings match the baseline; that is consistent with a dummy load and is not proof of one".into());
    }
    Decision {
        verdict,
        reasons,
        compared,
    }
}

/// The baseline file of a rig: bands keyed by the owner's frequency range.
pub fn baseline_file(dir: &Path, rig: &str) -> PathBuf {
    dir.join(format!("{}.baseline.json", super::safe_name(rig)))
}

fn band_key(range: (f64, f64)) -> String {
    format!("{:.0}-{:.0}", range.0, range.1)
}

pub fn load_baseline(dir: &Path, rig: &str, range: (f64, f64)) -> Result<Option<Baseline>, String> {
    // [@ANCHOR: hamlib_baseline_load]
    let path = baseline_file(dir, rig);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let v: Value = serde_json::from_str(&text).map_err(|e| {
        format!(
            "{}: {e}. Next: remove the file and record the baseline again.",
            path.display()
        )
    })?;
    let Some(b) = v.get("bands").and_then(|b| b.get(band_key(range))) else {
        return Ok(None);
    };
    let readings: Option<Vec<Reading>> = b["readings"]
        .as_array()
        .map(|a| a.iter().filter_map(Reading::from_json).collect());
    let readings = readings.filter(|r| !r.is_empty()).ok_or_else(|| {
        format!(
            "{}: the baseline for {} is damaged. Next: record it again.",
            path.display(),
            band_key(range)
        )
    })?;
    Ok(Some(Baseline {
        taken_on: b["taken_on"].as_str().unwrap_or("").into(),
        attested_by: b["attested_by"].as_str().unwrap_or("").into(),
        cabling: b["cabling"].as_str().unwrap_or("").into(),
        settings: b["settings"].clone(),
        readings,
    }))
}

pub fn save_baseline(dir: &Path, rig: &str, range: (f64, f64), b: &Baseline) -> Result<(), String> {
    // [@ANCHOR: hamlib_baseline_save]
    let path = baseline_file(dir, rig);
    let mut all: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({"bands": {}}));
    all["bands"][band_key(range)] = json!({
        "taken_on": b.taken_on, "attested_by": b.attested_by, "cabling": b.cabling, "settings": b.settings,
        "readings": b.readings.iter().map(Reading::stored).collect::<Vec<_>>(),
    });
    let tmp = path.with_extension("json.tmp");
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(&tmp, serde_json::to_string_pretty(&all).unwrap_or_default())
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The class default mode for the transmit frequency's own reading.
fn mode_for(hz: f64) -> &'static str {
    // [@ANCHOR: hamlib_load_mode_for]
    if class_of(hz) == "hf" {
        "AM"
    } else {
        "FM"
    }
}

/// What the rig was set to before the check, to put back.
struct Original {
    freq: f64,
    mode: Option<(String, f64)>,
    levels: Vec<(&'static str, f64)>,
}

fn read_original(c: &mut Client) -> Result<Original, String> {
    // [@ANCHOR: hamlib_load_read_original]
    let freq = c
        .read_number("get_freq", Some("Frequency"))
        .map_err(|e| e.to_string())?;
    let mode = tolerate(c.read("get_mode"))?
        .ok()
        .and_then(|r| Some((r.kv("Mode")?.to_string(), r.number(Some("Passband")).ok()?)));
    let mut levels = vec![];
    for n in ["AF", "PREAMP", "ATT"] {
        if let Ok(v) = tolerate(c.read_number(&format!("get_level {n}"), None))? {
            levels.push((n, v));
        }
    }
    Ok(Original { freq, mode, levels })
}

/// Puts the rig back. Returns what could not be restored.
fn restore(c: &mut Client, o: &Original) -> Vec<String> {
    // [@ANCHOR: hamlib_load_restore]
    let mut failed = vec![];
    if let Some((m, w)) = &o.mode {
        if c.command(&format!("set_mode {m} {w:.0}")).is_err() {
            failed.push(format!("mode {m}"));
        }
    }
    if c.command(&format!("set_freq {:.0}", o.freq)).is_err() {
        failed.push(format!("frequency {:.0} Hz", o.freq));
    }
    for (n, v) in &o.levels {
        if c.command(&format!("set_level {n} {v}")).is_err() {
            failed.push(format!("level {n}"));
        }
    }
    failed
}

/// One frequency: tune, wait, read the S-meter several times, the squelch, and listen.
#[allow(clippy::too_many_arguments)]
fn read_point(
    c: &mut Client,
    th: &LoadCheck,
    id: &str,
    hz: f64,
    mode: &str,
    audio: Option<&dyn AudioSource>,
    audio_s: f64,
) -> Result<Reading, String> {
    // [@ANCHOR: hamlib_load_read_point]
    c.command(&format!("set_freq {hz:.0}"))
        .map_err(|e| format!("{id}: tuning to {hz:.0} Hz: {e}"))?;
    c.command(&format!("set_mode {mode} 0"))
        .map_err(|e| format!("{id}: setting mode {mode}: {e}"))?;
    std::thread::sleep(Duration::from_secs_f64(th.dwell_ms / 1000.0));
    let mut v = vec![];
    for i in 0..th.readings {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(60));
        }
        v.push(c.read_number("get_level STRENGTH", None).map_err(|e| format!("{id}: the S-meter cannot be read ({e}); refusing, because a check without a reading proves nothing"))?);
    }
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let spread = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - v.iter().cloned().fold(f64::INFINITY, f64::min);
    let squelch_open = tolerate(c.read_number("get_dcd", Some("DCD")))?
        .ok()
        .map(|d| d != 0.0);
    let audio = match audio {
        None => None,
        Some(src) => {
            let samples = src.capture(audio_s).map_err(|e| format!("{id}: the rig's audio cannot be recorded ({e}); refusing, because the audio evidence is missing"))?;
            Some(AudioObs::of(&detect::analyze(&samples, src.rate())?))
        }
    };
    Ok(Reading {
        id: id.into(),
        frequency_hz: hz,
        mode: mode.into(),
        mean_db: mean,
        spread_db: spread,
        squelch_open,
        audio,
    })
}

/// Seconds of audio to record at a station: enough for the tick rhythm at a time station, less elsewhere.
fn audio_seconds(s: &Station) -> f64 {
    // [@ANCHOR: hamlib_load_audio_seconds]
    if s.audio == "wwv" || s.audio == "wwvh" {
        8.0
    } else {
        4.0
    }
}

/// The settings this check puts the rig in, as data for the baseline to compare.
fn settings_record(th: &LoadCheck, audio: bool) -> Value {
    json!({"af": th.af, "preamp_db": th.preamp_db, "attenuator_db": 0, "readings": th.readings, "dwell_ms": th.dwell_ms, "references": th.references, "audio": audio})
}

pub struct Measured {
    pub readings: Vec<Reading>,
    pub settings: Value,
    pub stations_used: Vec<Station>,
    pub restore_failed: Vec<String>,
}

/// Measures the transmit frequency and the reference frequencies with the rig in receive. The rig's frequency, mode, AF, preamp and
/// attenuator are put back afterwards on every path. A keyed rig is never touched.
pub fn measure(
    c: &mut Client,
    tx: &Transmit,
    tx_hz: f64,
    table: &Table,
    covers: &dyn Fn(f64) -> bool,
    audio: Option<&dyn AudioSource>,
) -> Result<Measured, String> {
    // [@ANCHOR: hamlib_load_measure]
    let th = &tx.load;
    if let Ok(r) = c.read("get_ptt") {
        if r.kv("PTT").is_some_and(|p| p != "0") {
            return Err("the rig reports PTT on; the load check listens with the rig in receive and does nothing while it transmits".into());
        }
    }
    let refs: Vec<Station> = table
        .references(tx_hz, now_local().1, covers, th.references)
        .into_iter()
        .cloned()
        .collect();
    let original = read_original(c)?;
    let mut run = || -> Result<Vec<Reading>, String> {
        // Listening conditions: audio gain fixed so levels compare, preamp as configured, attenuator off. A rig without them is
        // listened to as it is; the settings record says what was applied.
        let _ = c.command(&format!("set_level AF {}", th.af));
        if let Some(p) = th.preamp_db {
            let _ = c.command(&format!("set_level PREAMP {p}"));
        }
        let _ = c.command("set_level ATT 0");
        let mut out = vec![read_point(c, th, "tx", tx_hz, mode_for(tx_hz), audio, 4.0)?];
        for s in &refs {
            out.push(read_point(
                c,
                th,
                &s.id,
                s.frequency_hz,
                &s.mode,
                audio,
                audio_seconds(s),
            )?);
        }
        Ok(out)
    };
    let result = run();
    let restore_failed = restore(c, &original);
    Ok(Measured {
        readings: result?,
        settings: settings_record(th, audio.is_some()),
        stations_used: refs,
        restore_failed,
    })
}

/// Records which stations were heard (their audio showed their ticks or tones) in the table.
pub fn note_heard(table: &mut Table, m: &Measured) -> Vec<String> {
    // [@ANCHOR: hamlib_load_note_heard]
    let today = now_local().0;
    let mut heard = vec![];
    for (r, s) in m.readings.iter().skip(1).zip(&m.stations_used) {
        if r.audio.as_ref().is_some_and(|a| a.time_station)
            && (s.audio == "wwv" || s.audio == "wwvh")
            && table.mark_heard(&s.id, &today).is_ok()
        {
            heard.push(s.id.clone());
        }
    }
    heard
}

#[cfg(test)]
mod tests {
    use super::*;

    fn th() -> LoadCheck {
        LoadCheck::default()
    }

    fn r(id: &str, mean: f64, spread: f64) -> Reading {
        Reading {
            id: id.into(),
            frequency_hz: 7.1e6,
            mode: "AM".into(),
            mean_db: mean,
            spread_db: spread,
            squelch_open: Some(false),
            audio: None,
        }
    }

    fn audio(rms: f64, ts: bool, beats: &[f64]) -> Option<AudioObs> {
        Some(AudioObs {
            rms_dbfs: rms,
            all_zero: false,
            time_station: ts,
            beats_hz: beats.to_vec(),
            detail: json!({}),
        })
    }

    fn base(readings: Vec<Reading>) -> Baseline {
        Baseline {
            taken_on: "2026-10-29".into(),
            attested_by: "K6BP".into(),
            cabling: "load".into(),
            settings: json!({"s": 1}),
            readings,
        }
    }

    fn set() -> Value {
        json!({"s": 1})
    }

    // Tests [@ANCHOR: hamlib_load_decide]
    // Tests [@ANCHOR: hamlib_verdict_word]
    #[test]
    fn readings_at_the_baseline_are_consistent_and_each_kind_of_excess_refuses() {
        let b = base(vec![
            r("tx", -54.0, 1.0),
            r("wwv-10", -54.0, 2.0),
            r("wwv-5", -53.0, 1.0),
        ]);
        let ok = vec![
            r("tx", -53.0, 1.5),
            r("wwv-10", -55.0, 1.0),
            r("wwv-5", -52.0, 1.0),
        ];
        let d = decide(Some(&b), &ok, &th(), &set());
        assert_eq!(
            (d.verdict.clone(), d.compared),
            (Verdict::Consistent, 3),
            "{d:?}"
        );
        assert!(d.reasons[0].contains("not proof"));
        // 15 dB over the baseline at one reference: refuse, naming it.
        let hot = vec![
            r("tx", -53.0, 1.0),
            r("wwv-10", -39.0, 1.0),
            r("wwv-5", -52.0, 1.0),
        ];
        let d = decide(Some(&b), &hot, &th(), &set());
        assert_eq!(d.verdict, Verdict::Refuse);
        assert!(
            d.reasons
                .iter()
                .any(|x| x.starts_with("wwv-10") && x.contains("over the baseline")),
            "{d:?}"
        );
        // Just inside the margin plus the spread allowance: still consistent.
        let edge = vec![
            r("tx", -54.0, 1.0),
            r("wwv-10", -54.0 + 6.0 + 2.0, 2.0),
            r("wwv-5", -53.0, 1.0),
        ];
        assert_eq!(
            decide(Some(&b), &edge, &th(), &set()).verdict,
            Verdict::Consistent
        );
        let over = vec![
            r("tx", -54.0, 1.0),
            r("wwv-10", -54.0 + 6.0 + 2.0 + 0.5, 2.0),
            r("wwv-5", -53.0, 1.0),
        ];
        assert_eq!(
            decide(Some(&b), &over, &th(), &set()).verdict,
            Verdict::Refuse
        );
        // An open squelch that was closed.
        let mut sq = ok.clone();
        sq[1].squelch_open = Some(true);
        assert!(decide(Some(&b), &sq, &th(), &set())
            .reasons
            .iter()
            .any(|x| x.contains("squelch")));
    }

    #[test]
    fn unsteady_readings_a_missing_baseline_other_settings_and_a_thin_baseline_do_not_pass() {
        let b = base(vec![r("tx", -54.0, 1.0), r("wwv-10", -54.0, 1.0)]);
        let wobble = vec![r("tx", -54.0, 1.0), r("wwv-10", -54.0, 9.0)];
        let d = decide(Some(&b), &wobble, &th(), &set());
        assert_eq!(d.verdict, Verdict::Refuse);
        assert!(
            d.reasons.iter().any(|x| x.contains("too unsteady")),
            "{d:?}"
        );
        let ok = vec![r("tx", -54.0, 1.0), r("wwv-10", -54.0, 1.0)];
        let d = decide(None, &ok, &th(), &set());
        assert_eq!(d.verdict, Verdict::NoBaseline);
        assert!(d.reasons.iter().any(|x| x.contains("record a baseline")));
        let d = decide(Some(&b), &ok, &th(), &json!({"s": 2}));
        assert_eq!(d.verdict, Verdict::Refuse);
        assert!(d
            .reasons
            .iter()
            .any(|x| x.contains("other rig or check settings")));
        let thin = decide(
            Some(&b),
            &[r("tx", -54.0, 1.0), r("new-one", -54.0, 1.0)],
            &th(),
            &set(),
        );
        assert_eq!(thin.verdict, Verdict::Refuse);
        assert!(thin.reasons.iter().any(|x| x.contains("does not cover")));
        let empty = decide(Some(&b), &[], &th(), &set());
        assert_eq!(empty.verdict, Verdict::Refuse);
        let nan = decide(
            Some(&b),
            &[r("tx", f64::NAN, 0.0), r("wwv-10", -54.0, 1.0)],
            &th(),
            &set(),
        );
        assert_eq!(nan.verdict, Verdict::Refuse);
    }

    #[test]
    fn audio_evidence_a_station_heard_loud_audio_a_new_carrier_note_and_a_dead_input() {
        let mut b1 = r("tx", -54.0, 1.0);
        b1.audio = audio(-60.0, false, &[1200.0]);
        let mut b2 = r("wwv-10", -54.0, 1.0);
        b2.audio = audio(-60.0, false, &[]);
        let b = base(vec![b1.clone(), b2.clone()]);
        let same = vec![b1.clone(), b2.clone()];
        assert_eq!(
            decide(Some(&b), &same, &th(), &set()).verdict,
            Verdict::Consistent,
            "a receiver birdie in the baseline is not a signal"
        );
        // A time station heard: refuse whether or not a baseline exists, even with the S-meter at the baseline.
        let mut heard = same.clone();
        heard[1].audio = audio(-60.0, true, &[]);
        for base in [Some(&b), None] {
            let d = decide(base, &heard, &th(), &set());
            assert_eq!(d.verdict, Verdict::Refuse, "{d:?}");
            assert!(d.reasons.iter().any(|x| x.contains("time station")));
        }
        let mut loud = same.clone();
        loud[0].audio = audio(-45.0, false, &[1200.0]);
        assert!(decide(Some(&b), &loud, &th(), &set())
            .reasons
            .iter()
            .any(|x| x.contains("louder")));
        let mut beat = same.clone();
        beat[1].audio = audio(-60.0, false, &[1500.0]);
        assert!(decide(Some(&b), &beat, &th(), &set())
            .reasons
            .iter()
            .any(|x| x.contains("steady note at 1500")));
        let mut dead = same.clone();
        dead[0].audio = Some(AudioObs {
            rms_dbfs: -200.0,
            all_zero: true,
            time_station: false,
            beats_hz: vec![],
            detail: json!({}),
        });
        let d = decide(Some(&b), &dead, &th(), &set());
        assert_eq!(d.verdict, Verdict::Refuse);
        assert!(d.reasons.iter().any(|x| x.contains("only zeros")));
    }

    // Tests [@ANCHOR: hamlib_baseline_save]
    // Tests [@ANCHOR: hamlib_baseline_load]
    #[test]
    fn a_baseline_is_stored_per_range_and_read_back_exactly_and_damage_is_an_error() {
        let d = std::env::temp_dir().join(format!("bench_mcp-baseline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(load_baseline(&d, "rig", (7e6, 7.01e6)).unwrap(), None);
        let mut a = r("tx", -54.2, 1.5);
        a.audio = audio(-61.5, false, &[1200.0, 800.0]);
        let b = base(vec![a, r("wwv-10", -55.0, 0.5)]);
        save_baseline(&d, "rig", (7e6, 7.01e6), &b).unwrap();
        save_baseline(&d, "rig", (14e6, 14.35e6), &base(vec![r("tx", -50.0, 1.0)])).unwrap();
        let mut back = load_baseline(&d, "rig", (7e6, 7.01e6)).unwrap().unwrap();
        // The detail text of the audio is a report, not part of the comparison.
        back.readings[0].audio.as_mut().unwrap().detail = json!({});
        assert_eq!(back, b);
        assert_eq!(
            load_baseline(&d, "rig", (14e6, 14.35e6))
                .unwrap()
                .unwrap()
                .readings
                .len(),
            1
        );
        assert_eq!(
            load_baseline(&d, "rig", (3.5e6, 4e6)).unwrap(),
            None,
            "no baseline for another range"
        );
        assert_eq!(
            load_baseline(&d, "other", (7e6, 7.01e6)).unwrap(),
            None,
            "none for another rig"
        );
        std::fs::write(baseline_file(&d, "rig"), "{oops").unwrap();
        assert!(load_baseline(&d, "rig", (7e6, 7.01e6))
            .unwrap_err()
            .contains("Next:"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

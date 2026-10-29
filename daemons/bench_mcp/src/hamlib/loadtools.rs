// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tools around the load plausibility check: the station table, the check itself and the baseline. None of them transmits.

use super::attest::Transmit;
use super::audio_in::AudioSource;
use super::control::{check_freq, rx_ranges};
use super::load::{self, Baseline, Decision, Measured};
use super::proto::Client;
use super::stations::{now_local, Table};
use super::{rigd, Config};
use serde_json::{json, Value};
use std::path::PathBuf;

const SEPARATE_NOTE: &str = "This is evidence, not proof. An open or unplugged connection receives nothing and looks like a good load (the low-power SWR probe of the transmit tools exists to catch that); an antenna on a dead band at low gain also hears nothing; hearing no reference station proves nothing, because propagation can fail on all of them; the baseline is valid only for the cabling and rig settings it was recorded with and must be recorded again after any change; and the owner's attestation in the config still gates transmitting.";

fn transmit(cfg: &Config) -> Result<&Transmit, String> {
    cfg.transmit.as_ref().ok_or_else(|| "this rig has no transmit attestation in the bench config, so there is no dummy-load path to check. Nothing was done.".to_string())
}

/// `bench_hamlib_stations`: the table, with what was heard recently; for a frequency, the references that would be used now.
pub fn stations(cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: hamlib_stations_tool]
    let dir = rigd::dir(cfg);
    let table = Table::load(&dir)?;
    let (today, hour) = now_local();
    let include_retired = args
        .get("include_retired")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let list: Vec<Value> = table
        .stations
        .iter()
        .filter(|s| include_retired || !s.retired)
        .map(|s| s.json(&today))
        .collect();
    let mut out = json!({
        "today": today, "stations": list, "table_file": super::stations::file(&dir), "saved": super::stations::file(&dir).exists(),
        "note": "Never assume a station is on the air: silence is reported as not heard, not as a fault. Review this table; retire what you find off the air and add the stations you know are strong where you are. last_heard is filled from this bench's own detections.",
    });
    if let Some(f) = args.get("frequency_hz").and_then(|v| v.as_f64()) {
        let refs = table.references(f, hour, &|_| true, 12);
        out["references_for_frequency_hz"] = json!(f);
        out["references_now"] = json!(refs
            .iter()
            .map(|s| json!({"id": s.id, "frequency_hz": s.frequency_hz, "mode": s.mode}))
            .collect::<Vec<_>>());
        out["local_hour"] = json!(hour);
    }
    Ok(out)
}

/// `bench_hamlib_station_set`: the owner's edits of the table.
pub fn station_set(cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: hamlib_station_set_tool]
    let action = args
        .get("action")
        .and_then(|a| a.as_str())
        .ok_or("action is required: add, retire, restore, remove or note")?;
    let dir = rigd::dir(cfg);
    let mut table = Table::load(&dir)?;
    let done = table.apply(action, args)?;
    table.save(&dir)?;
    Ok(
        json!({"done": done, "stations_in_table": table.stations.len(), "table_file": super::stations::file(&dir)}),
    )
}

struct Prepared<'a> {
    tx: &'a Transmit,
    range: (f64, f64),
    dir: PathBuf,
}

fn prepare<'a>(cfg: &'a Config, args: &Value) -> Result<Prepared<'a>, String> {
    // [@ANCHOR: hamlib_loadtools_prepare]
    let tx = transmit(cfg)?;
    let f = super::status::bounded(args, "frequency_hz", f64::NAN, 1.0, 1.0e12).map_err(|_| {
        "frequency_hz is required (Hz): the transmit frequency the check is for".to_string()
    })?;
    let i = tx.range_of(f).ok_or_else(|| format!("{f:.0} Hz is outside the frequency ranges the owner listed in transmit.frequency_ranges ({}); the check is only for those", tx.frequency_ranges.iter().map(|r| format!("{:.0} to {:.0}", r.0, r.1)).collect::<Vec<_>>().join(", ")))?;
    Ok(Prepared {
        tx,
        range: tx.frequency_ranges[i],
        dir: rigd::dir(cfg),
    })
}

fn measure(
    c: &mut Client,
    cfg: &Config,
    p: &Prepared,
    f: f64,
    audio: Option<&dyn AudioSource>,
) -> Result<(Measured, Table), String> {
    // [@ANCHOR: hamlib_loadtools_measure]
    let mut table = Table::load(&p.dir)?;
    let (ranges, source) = rx_ranges(c, cfg)?;
    check_freq(f, &ranges, &source)?;
    let covers = |hz: f64| check_freq(hz, &ranges, &source).is_ok();
    let m = load::measure(c, p.tx, f, &table, &covers, audio)?;
    let heard = load::note_heard(&mut table, &m);
    if !heard.is_empty() {
        let _ = table.save(&p.dir);
    }
    Ok((m, table))
}

fn report(
    m: &Measured,
    d: &Decision,
    base: Option<&Baseline>,
    audio: Option<&dyn AudioSource>,
    heard: &[String],
) -> Value {
    json!({
        "verdict": d.verdict.word(), "reasons": d.reasons, "compared_with_baseline": d.compared,
        "readings": m.readings.iter().map(|r| r.json()).collect::<Vec<_>>(),
        "baseline": base.map(|b| json!({"taken_on": b.taken_on, "attested_by": b.attested_by, "cabling": b.cabling})),
        "audio": audio.map_or("none configured: the check has no audio evidence (set audio in the rig's config)".to_string(), |a| a.describe()),
        "stations_heard_now": heard, "rig_settings_not_restored": m.restore_failed,
        "note": SEPARATE_NOTE,
    })
}

/// `bench_hamlib_load_check`: receive-only readings compared with the baseline. Never keys.
pub fn load_check(
    c: &mut Client,
    cfg: &Config,
    audio: Option<&dyn AudioSource>,
    rig: &str,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: hamlib_load_check_tool]
    let p = prepare(cfg, args)?;
    let f = args["frequency_hz"].as_f64().unwrap_or_default();
    let base = load::load_baseline(&p.dir, rig, p.range)?;
    let (m, table) = measure(c, cfg, &p, f, audio)?;
    let d = load::decide(base.as_ref(), &m.readings, &p.tx.load, &m.settings);
    let heard: Vec<String> = table
        .stations
        .iter()
        .filter(|s| {
            s.last_heard.as_deref() == Some(now_local().0.as_str())
                && m.stations_used.iter().any(|u| u.id == s.id)
        })
        .map(|s| s.id.clone())
        .collect();
    Ok(report(&m, &d, base.as_ref(), audio, &heard))
}

/// What makes a set of readings unfit to be a baseline.
pub fn baseline_problems(m: &Measured, max_spread_db: f64) -> Vec<String> {
    // [@ANCHOR: hamlib_baseline_problems]
    let mut v: Vec<String> = load::decide(
        None,
        &m.readings,
        &super::attest::LoadCheck::default(),
        &m.settings,
    )
    .reasons
    .into_iter()
    .filter(|r| !r.contains("no baseline for this frequency range"))
    .collect();
    for r in &m.readings {
        if r.spread_db > max_spread_db {
            v.push(format!("{}: the readings vary by {:.1} dB, more than {max_spread_db:.1} dB: too unsteady to be a baseline", r.id, r.spread_db));
        }
    }
    v
}

/// `bench_hamlib_load_baseline`: records the readings through the load as the baseline for the frequency range. The owner attests
/// that the load is connected; the tool cannot know. It refuses to record a baseline in which a station is heard.
pub fn load_baseline(
    c: &mut Client,
    cfg: &Config,
    audio: Option<&dyn AudioSource>,
    rig: &str,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: hamlib_load_baseline_tool]
    let p = prepare(cfg, args)?;
    let f = args["frequency_hz"].as_f64().unwrap_or_default();
    let dry = args
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let confirm = args
        .get("confirm_dummy_load")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let table = Table::load(&p.dir)?;
    let (today, hour) = now_local();
    if dry {
        let refs = table.references(f, hour, &|_| true, p.tx.load.references);
        return Ok(json!({
            "dry_run": true, "sent": [], "frequency_range_hz": [p.range.0, p.range.1], "rig": rig,
            "would": [
                "record the rig's frequency, mode, AF, preamp and attenuator, and put them back afterwards".to_string(),
                format!("listen with the rig in receive at {f:.0} Hz and at {} reference frequencies: {}", refs.len(), refs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(", ")),
                format!("{} S-meter readings at each, with the audio from {}", p.tx.load.readings, audio.map_or("no audio input (none configured)".to_string(), |a| a.describe())),
                "refuse to record if a time station is heard or the readings are unsteady, else store them as the baseline for this range, with the attestation's name, date and cabling".to_string(),
            ],
            "attested_by": p.tx.attested_by, "attested_on": p.tx.attested_on, "cabling": p.tx.cabling,
            "note": "nothing was sent; the baseline is what later checks compare with, so it is only meaningful when the dummy load really is connected",
        }));
    }
    if !confirm {
        return Err(format!("recording a baseline states that the rig's antenna port is on the dummy load described as {:?} in the config; call it with confirm_dummy_load: true only when the person at the bench has said so (dry_run: true shows the plan)", p.tx.cabling));
    }
    let (m, _) = measure(c, cfg, &p, f, audio)?;
    let problems = baseline_problems(&m, p.tx.load.max_spread_db);
    if !problems.is_empty() {
        return Err(format!("no baseline was recorded: {}. Next: check what is connected to the rig's antenna port; a load hears nothing of these. {SEPARATE_NOTE}", problems.join("; ")));
    }
    let b = Baseline {
        taken_on: today,
        attested_by: p.tx.attested_by.clone(),
        cabling: p.tx.cabling.clone(),
        settings: m.settings.clone(),
        readings: m.readings.clone(),
    };
    load::save_baseline(&p.dir, rig, p.range, &b)?;
    Ok(json!({
        "recorded": true, "frequency_range_hz": [p.range.0, p.range.1], "taken_on": b.taken_on, "attested_by": b.attested_by, "cabling": b.cabling,
        "readings": m.readings.iter().map(|r| r.json()).collect::<Vec<_>>(), "rig_settings_not_restored": m.restore_failed,
        "audio": audio.map_or("none configured".to_string(), |a| a.describe()),
        "note": "Record it again after any change of cabling, load, rig settings or the audio path.",
    }))
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! `bench_hamlib_rx_check`: tunes the receiver, listens to its audio for a few seconds and reports the signal strength against the
//! audio level, the audio noise floor, the squelch state, the strongest steady tone and whether a time station is in the audio.
//! The audio comes from the sound input named in the rig's `audio` setting. The rig's frequency, mode and AF level are put back.

use super::attest::LoadCheck;
use super::audio_in::AudioSource;
use super::control::{check_freq, rx_ranges, MODES};
use super::detect;
use super::load::AudioObs;
use super::proto::Client;
use super::stations::class_of;
use super::status::{bounded, s_text, s_units, tolerate};
use super::Config;
use crate::audio::analysis::{db, dominant, median_db, spectrum};
use serde_json::{json, Value};
use std::time::Duration;

pub fn rx_check(
    c: &mut Client,
    cfg: &Config,
    audio: Option<&dyn AudioSource>,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: hamlib_rx_check]
    let f = bounded(args, "frequency_hz", f64::NAN, 1.0, 1.0e12)
        .map_err(|_| "frequency_hz is required (Hz)".to_string())?;
    let seconds = bounded(args, "seconds", 5.0, 3.0, 20.0)?;
    let expect = match args.get("expect_tone_hz") {
        None | Some(Value::Null) => None,
        Some(_) => Some(bounded(args, "expect_tone_hz", 0.0, 100.0, 5000.0)?),
    };
    let mode = match args.get("mode").and_then(|m| m.as_str()) {
        Some(m) => {
            let m = m.to_ascii_uppercase();
            MODES
                .contains(&m.as_str())
                .then_some(m)
                .ok_or("mode is not a Hamlib mode name")?
        }
        None => (if class_of(f) == "hf" { "AM" } else { "FM" }).to_string(),
    };
    let (ranges, source) = rx_ranges(c, cfg)?;
    check_freq(f, &ranges, &source)?;
    if tolerate(c.read("get_ptt"))?
        .ok()
        .is_some_and(|r| r.kv("PTT").is_some_and(|p| p != "0"))
    {
        return Err(
            "the rig reports PTT on; the receiver check does nothing while it transmits".into(),
        );
    }
    let had_freq = c
        .read_number("get_freq", Some("Frequency"))
        .map_err(|e| e.to_string())?;
    let had_mode = tolerate(c.read("get_mode"))?
        .ok()
        .and_then(|r| Some((r.kv("Mode")?.to_string(), r.number(Some("Passband")).ok()?)));
    let had_af = tolerate(c.read_number("get_level AF", None))?.ok();
    let result = listen(c, f, &mode, seconds, expect, audio);
    let mut failed = vec![];
    if let Some((m, w)) = &had_mode {
        if c.command(&format!("set_mode {m} {w:.0}")).is_err() {
            failed.push("mode");
        }
    }
    if c.command(&format!("set_freq {had_freq:.0}")).is_err() {
        failed.push("frequency");
    }
    if let Some(af) = had_af {
        if c.command(&format!("set_level AF {af}")).is_err() {
            failed.push("AF level");
        }
    }
    let mut out = result?;
    out["rig_settings_not_restored"] = json!(failed);
    Ok(out)
}

fn listen(
    c: &mut Client,
    f: f64,
    mode: &str,
    seconds: f64,
    expect: Option<f64>,
    audio: Option<&dyn AudioSource>,
) -> Result<Value, String> {
    // [@ANCHOR: hamlib_rx_listen]
    let th = LoadCheck::default();
    c.command(&format!("set_freq {f:.0}"))
        .map_err(|e| format!("tuning to {f:.0} Hz: {e}"))?;
    c.command(&format!("set_mode {mode} 0"))
        .map_err(|e| format!("setting mode {mode}: {e}"))?;
    if audio.is_some() {
        let _ = c.command(&format!("set_level AF {}", th.af));
    }
    std::thread::sleep(Duration::from_millis(300));
    let mut v = vec![];
    for i in 0..5 {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(80));
        }
        v.push(
            c.read_number("get_level STRENGTH", None)
                .map_err(|e| format!("the S-meter cannot be read: {e}"))?,
        );
    }
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let spread = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - v.iter().cloned().fold(f64::INFINITY, f64::min);
    let squelch = tolerate(c.read_number("get_dcd", Some("DCD")))?
        .ok()
        .map(|d| d != 0.0);
    let mut out = json!({
        "frequency_hz": f, "mode": mode,
        "signal": {"mean_db_relative_to_s9": mean, "spread_db": spread, "s_units": s_units(mean), "text": s_text(mean), "readings": v},
        "squelch_open": squelch,
    });
    let Some(src) = audio else {
        out["audio"] = json!("none configured: set audio in the rig's config to hear the receiver");
        return Ok(out);
    };
    let samples = src
        .capture(seconds)
        .map_err(|e| format!("the rig's audio cannot be recorded: {e}"))?;
    let findings = detect::analyze(&samples, src.rate())?;
    let a = AudioObs::of(&findings);
    let mut audio_json = json!({"source": src.describe(), "rms_dbfs": (a.rms_dbfs * 10.0).round() / 10.0, "input_all_zero": a.all_zero, "findings": findings.json()});
    if !a.all_zero && samples.len() >= 8192 {
        let sp = spectrum(&samples, src.rate(), 8192)?;
        audio_json["noise_floor_dbfs"] =
            json!((median_db(&sp, 300.0, 3000.0) * 10.0).round() / 10.0);
        if let Some(p) = dominant(&sp, 150.0, 3500.0, 20.0) {
            audio_json["dominant_tone"] = json!({"hz": (p.hz * 10.0).round() / 10.0, "level_dbfs": (p.dbfs * 10.0).round() / 10.0});
            if let Some(e) = expect {
                audio_json["tone_offset_hz"] = json!((p.hz - e).round());
                audio_json["tone_offset_note"] = json!("the offset of the strongest tone from the expected one: the receiver's tuning error when the tone is a carrier beat of a known frequency; the sound card's clock adds its own tiny error");
            }
        }
        audio_json["signal_vs_audio"] = json!(format!(
            "{} on the S-meter, {:.0} dBFS audio level over a {:.0} dBFS noise floor",
            s_text(mean),
            db(crate::audio::analysis::rms(&samples)),
            median_db(&sp, 300.0, 3000.0)
        ));
    }
    out["audio"] = audio_json;
    Ok(out)
}

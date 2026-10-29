// SPDX-License-Identifier: AGPL-3.0-or-later
//! Audio path discovery: find out by listening which outputs make sound and which inputs hear, instead of trusting device
//! names. Each output in turn plays a short quiet tone of its own frequency while every candidate input records; the
//! output-by-input matrix of heard / not heard gives the working outputs, the live inputs and the loopback pairs, from which
//! the alert output and the verification input are chosen.
//!
//! The decision logic (`conclude`) is pure and tested on synthetic observations; the measurement (`hear_tone`) is tested on
//! synthetic recordings; `run` is the part that needs sound hardware and is checked on the machine.

use super::devices::{Device, Dir};
use super::engine::{Playing, Recorder};
use super::heard::{hear_tone, round1};
use super::safety::{limits, VolumeGuard, Want};
use super::tones::{generate, Tone};
use super::tools::Ctx;
use super::{levels, store, wav};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

/// Tone frequencies, one per output in turn (no harmonics of each other).
pub const FREQS: [f64; 8] = [
    1000.0, 1300.0, 1700.0, 2200.0, 2900.0, 3700.0, 4700.0, 6000.0,
];
/// The identifying tone length in seconds.
pub const TONE_S: f64 = 0.4;
const RATE: u32 = 48000;
const LEAD_S: f64 = 0.4;
const TAIL_S: f64 = 0.5;
const MIN_SNR_DB: f64 = 12.0;
pub const FILE: &str = "audio_discovery.json";

/// One output heard (or not) by one input.
#[derive(Clone, Debug, PartialEq)]
pub struct Obs {
    pub output: String,
    pub input: String,
    pub freq: f64,
    /// The output volume at the step this observation was made at.
    pub volume: f64,
    pub heard: bool,
    pub level_dbfs: f64,
    pub snr_db: f64,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InState {
    pub name: String,
    pub is_default: bool,
    /// The stream could not be opened or delivered no samples.
    pub error: Option<String>,
    /// Every sample recorded was exactly zero.
    pub all_zero: bool,
    pub muted: Option<bool>,
    pub noise_rms_dbfs: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutState {
    pub name: String,
    pub is_default: bool,
    pub is_hdmi: bool,
    pub availability: Option<String>,
    pub freq: f64,
    /// The highest volume the tone was played at.
    pub max_volume: f64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Conclusion {
    pub inputs: Vec<Value>,
    pub outputs: Vec<Value>,
    pub pairs: Vec<Value>,
    pub alert_output: Option<String>,
    pub alert_output_reason: String,
    pub verify_input: Option<String>,
    pub verify_input_reason: String,
    pub operator_check: Option<String>,
}

fn best<'a>(obs: impl Iterator<Item = &'a Obs>) -> Option<&'a Obs> {
    obs.filter(|o| o.heard)
        .max_by(|a, b| a.snr_db.total_cmp(&b.snr_db))
}

/// The conclusions from the observations. A speaker is present only if a microphone heard it; a microphone is present only if it
/// heard a known tone or has the noise floor of a live capture. An output heard by nothing is "no evidence of sound", not a failure.
pub fn conclude(outs: &[OutState], ins: &[InState], obs: &[Obs]) -> Conclusion {
    // [@ANCHOR: audio_discover_conclude]
    let live: Vec<&InState> = ins
        .iter()
        .filter(|i| i.error.is_none() && !i.all_zero)
        .collect();
    let mut inputs = Vec::new();
    for i in ins {
        let heard_outputs: Vec<&str> = obs
            .iter()
            .filter(|o| o.input == i.name && o.heard)
            .map(|o| o.output.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let (status, detail) = if let Some(e) = &i.error {
            (
                "unavailable",
                format!("the stream could not be recorded: {e}"),
            )
        } else if !heard_outputs.is_empty() {
            (
                "microphone_heard_tone",
                format!(
                    "heard the tone of {} output(s); noise floor {:.1} dBFS",
                    heard_outputs.len(),
                    i.noise_rms_dbfs
                ),
            )
        } else if i.all_zero {
            (
                "dead",
                format!(
                    "recorded exact digital zeros: {}",
                    if i.muted == Some(true) {
                        "it is muted in the mixer"
                    } else {
                        "muted, disconnected or no capture hardware behind it"
                    }
                ),
            )
        } else {
            (
                "microphone_live_noise_floor",
                format!("no tone heard, but the noise floor of {:.1} dBFS is that of a live capture (no speaker in its range, or all outputs muted)", i.noise_rms_dbfs),
            )
        };
        inputs.push(json!({"name": i.name, "status": status, "detail": detail, "default": i.is_default, "heard_outputs": heard_outputs, "noise_floor_dbfs": round1(i.noise_rms_dbfs)}));
    }
    let mut outputs = Vec::new();
    for o in outs {
        let heard_by: Vec<&str> = obs
            .iter()
            .filter(|x| x.output == o.name && x.heard)
            .map(|x| x.input.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let jack = match o.availability.as_deref() {
            Some("no") => Some("the driver reports this port as unplugged or without a sink: supporting evidence only"),
            Some("yes") => Some("the driver reports something plugged into this port: supporting evidence only"),
            _ => None,
        };
        let (status, detail) = if let Some(e) = &o.error {
            ("error", format!("the tone could not be played: {e}"))
        } else if !heard_by.is_empty() {
            (
                "sound_heard",
                format!(
                    "speaker present: heard by {} input(s) at volume {:.2}",
                    heard_by.len(),
                    o.max_volume
                ),
            )
        } else if live.is_empty() {
            (
                "cannot_verify_no_microphone",
                "no live microphone: sound cannot be verified automatically; only the operator can confirm it by ear".to_string(),
            )
        } else {
            (
                "no_evidence_of_sound",
                format!("no evidence of sound: speaker absent, muted, or no microphone in range (tried up to volume {:.2})", o.max_volume),
            )
        };
        let shared: Vec<&str> = outs
            .iter()
            .filter(|x| x.name != o.name && x.freq == o.freq)
            .map(|x| x.name.as_str())
            .collect();
        outputs.push(json!({
            "name": o.name, "status": status, "detail": detail, "default": o.is_default, "frequency_hz": o.freq,
            "heard_by": heard_by, "port_evidence": jack, "shared_frequency_with": shared,
        }));
    }
    let mut pairs: Vec<&Obs> = obs.iter().filter(|o| o.heard).collect();
    pairs.sort_by(|a, b| b.snr_db.total_cmp(&a.snr_db));
    let pairs_json: Vec<Value> = pairs
        .iter()
        .map(|p| {
            let top = best(obs.iter().filter(|o| o.output == p.output)).map_or(p.level_dbfs, |b| b.level_dbfs);
            json!({
                "output": p.output, "input": p.input, "level_dbfs": round1(p.level_dbfs), "snr_db": round1(p.snr_db), "volume": p.volume,
                "crosstalk_suspect": p.level_dbfs < top - 15.0,
            })
        })
        .collect();
    // Alert output: a heard output, not HDMI unless nothing else was heard, the system default first, then the loudest.
    let mut heard_outs: Vec<&OutState> = outs
        .iter()
        .filter(|o| obs.iter().any(|x| x.output == o.name && x.heard))
        .collect();
    heard_outs.sort_by(|a, b| {
        let snr = |o: &OutState| {
            best(obs.iter().filter(|x| x.output == o.name)).map_or(0.0, |x| x.snr_db)
        };
        (a.is_hdmi, !a.is_default)
            .cmp(&(b.is_hdmi, !b.is_default))
            .then(snr(b).total_cmp(&snr(a)))
    });
    let (alert_output, alert_output_reason) = match heard_outs.first() {
        Some(o) => (
            Some(o.name.clone()),
            format!(
                "heard by a microphone{}{}",
                if o.is_default {
                    ", and the system default"
                } else {
                    ""
                },
                if o.is_hdmi {
                    " (HDMI, because nothing else was heard)"
                } else {
                    ""
                }
            ),
        ),
        None if live.is_empty() => (
            None,
            "no output could be verified because there is no live microphone".into(),
        ),
        None => (None, "no output was heard by any microphone".into()),
    };
    let mut good_ins: Vec<&InState> = ins
        .iter()
        .filter(|i| i.error.is_none() && !i.all_zero)
        .collect();
    good_ins.sort_by(|a, b| {
        let heard = |i: &InState| obs.iter().any(|x| x.input == i.name && x.heard);
        let snr =
            |i: &InState| best(obs.iter().filter(|x| x.input == i.name)).map_or(0.0, |x| x.snr_db);
        (!heard(a), !a.is_default)
            .cmp(&(!heard(b), !b.is_default))
            .then(snr(b).total_cmp(&snr(a)))
    });
    let (verify_input, verify_input_reason) = match good_ins.first() {
        Some(i) if obs.iter().any(|x| x.input == i.name && x.heard) => {
            (Some(i.name.clone()), "it heard a known tone".to_string())
        }
        Some(i) => (
            Some(i.name.clone()),
            "it has the noise floor of a live capture, but heard no tone".to_string(),
        ),
        None => (
            None,
            "no input recorded anything but digital zeros".to_string(),
        ),
    };
    let operator_check = if live.is_empty() {
        Some("No microphone works, so sound cannot be verified automatically. Ask the operator to listen: play a tone with bench_audio_play on the output in question and ask whether it was heard.".to_string())
    } else if alert_output.is_none() {
        Some("No output was heard. If an operator is present, play a tone with bench_audio_play on the output in question and ask whether it was heard.".to_string())
    } else {
        None
    };
    Conclusion {
        inputs,
        outputs,
        pairs: pairs_json,
        alert_output,
        alert_output_reason,
        verify_input,
        verify_input_reason,
        operator_check,
    }
}

fn is_hdmi(d: &Device) -> bool {
    d.port_type.as_deref() == Some("hdmi") || d.node_name.to_ascii_lowercase().contains("hdmi")
}

/// The volume steps for one output: from the quiet volume up to the limit.
pub fn ladder(quiet: f64, limit: f64) -> Vec<f64> {
    // [@ANCHOR: audio_discover_ladder]
    let start = quiet.min(limit);
    if limit - start < 0.03 {
        return vec![limit];
    }
    let mid = (start * limit).sqrt();
    let mut v = vec![start];
    if mid - start >= 0.03 && limit - mid >= 0.03 {
        v.push(mid);
    }
    v.push(limit);
    v.into_iter()
        .map(|x| (x * 1000.0).round() / 1000.0)
        .collect()
}

/// A stable description of the candidate devices: a change means a device came or went or a port changed state.
pub fn fingerprint(devs: &[Device]) -> Vec<String> {
    // [@ANCHOR: bench_mcp_audio_discover_fingerprint]
    let mut v: Vec<String> = devs
        .iter()
        .filter(|d| d.reserved.is_none() && d.transport() == "alsa")
        .map(|d| {
            format!(
                "{}|{}|{}",
                d.dir.word(),
                d.node_name,
                d.availability.as_deref().unwrap_or("-")
            )
        })
        .collect();
    v.sort();
    v
}

/// What is stored from the last run.
#[derive(Clone, Debug)]
pub struct Stored {
    pub at_epoch: u64,
    pub fingerprint: Vec<String>,
    pub alert_output: Option<String>,
    pub verify_input: Option<String>,
    pub raw: Value,
}

pub fn load(dir: &std::path::Path) -> Option<Stored> {
    // [@ANCHOR: bench_mcp_audio_discover_load]
    let text = std::fs::read_to_string(dir.join(FILE)).ok()?;
    let raw: Value = serde_json::from_str(&text).ok()?;
    let list = |k: &str| -> Vec<String> {
        raw.get(k)
            .and_then(|f| f.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(Stored {
        at_epoch: raw.get("at_epoch").and_then(|e| e.as_u64()).unwrap_or(0),
        fingerprint: list("fingerprint"),
        alert_output: raw
            .pointer("/selection/alert_output")
            .and_then(|x| x.as_str())
            .map(String::from),
        verify_input: raw
            .pointer("/selection/verify_input")
            .and_then(|x| x.as_str())
            .map(String::from),
        raw,
    })
}

impl Stored {
    /// The differences between the stored fingerprint and the devices now; empty when the result is fresh.
    pub fn changes(&self, devs: &[Device]) -> Vec<String> {
        // [@ANCHOR: audio_discover_stale]
        let now = fingerprint(devs);
        let mut out: Vec<String> = now
            .iter()
            .filter(|x| !self.fingerprint.contains(x))
            .map(|x| format!("new or changed: {x}"))
            .collect();
        out.extend(
            self.fingerprint
                .iter()
                .filter(|x| !now.contains(x))
                .map(|x| format!("gone or changed: {x}")),
        );
        out
    }
}

fn save(dir: &std::path::Path, v: &Value) -> Result<PathBuf, String> {
    // [@ANCHOR: bench_mcp_audio_discover_save]
    use std::os::unix::fs::PermissionsExt;
    store::new_path(dir, "disc", None)?; // makes the directory, owner only
    let p = dir.join(FILE);
    std::fs::write(
        &p,
        serde_json::to_string_pretty(v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("{}: {e}", p.display()))?;
    let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    Ok(p)
}

fn epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Runs the discovery: sound is made, so the caller has checked the tier. Returns the stored result.
pub fn run(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: audio_discover_tool]
    let urgent = args
        .get("urgent")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let include_bt = args
        .get("include_bluetooth")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let dry = args
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let ok_transport =
        |d: &&Device| d.reserved.is_none() && (include_bt || d.transport() == "alsa");
    // `output` and `input` restrict the test to one device each; such a partial run is reported but not stored.
    let only = |key: &str, dir: Dir| -> Result<Vec<&Device>, String> {
        match args.get(key).and_then(|v| v.as_str()) {
            Some(n) => Ok(vec![super::devices::resolve(ctx.devs, n, dir)?]),
            None => Ok(ctx
                .devs
                .iter()
                .filter(|d| d.dir == dir)
                .filter(ok_transport)
                .collect()),
        }
    };
    let partial = args.get("output").is_some() || args.get("input").is_some();
    let outs = only("output", Dir::Output)?;
    let ins = only("input", Dir::Input)?;
    if outs.is_empty() {
        return Err("there is no output device to test".into());
    }
    let quiet_now = ctx.cfg.quiet_hours.is_some_and(|q| q.contains(ctx.minute));
    if quiet_now && !urgent && !dry {
        return Err("quiet hours are in effect: audible discovery is not run now. Run it outside quiet hours, or pass urgent: true to run it at the quiet volume only".into());
    }
    let plan: Vec<Value> = outs
        .iter()
        .enumerate()
        .map(|(k, o)| {
            let lim = limits(ctx.cfg, o, false, ctx.minute);
            json!({"output": o.node_name, "frequency_hz": FREQS[k % FREQS.len()], "volume_steps": ladder(ctx.cfg.quiet_volume, lim.volume)})
        })
        .collect();
    if dry {
        return Ok(
            json!({"dry_run": true, "plan": plan, "inputs": ins.iter().map(|i| &i.node_name).collect::<Vec<_>>(),
            "tone_seconds": TONE_S, "quiet_hours_now": quiet_now}),
        );
    }
    let dir = ctx.cfg.capture_dir();
    // Every candidate input records throughout, so that each output is heard by all of them at once.
    let mut recs: Vec<(&Device, Result<Recorder, String>)> = ins
        .iter()
        .map(|d| (*d, Recorder::start(&d.node_name, RATE, false)))
        .collect();
    for (_, r) in recs.iter_mut() {
        if let Ok(rec) = r {
            if let Err(e) =
                rec.wait_samples((0.8 * f64::from(RATE)) as usize, Duration::from_secs(6))
            {
                *r = Err(e);
            }
        }
    }
    let mut obs: Vec<Obs> = Vec::new();
    let mut out_states: Vec<OutState> = Vec::new();
    let mut audible_s = 0.0;
    let tone_path = store::new_path(&dir, "tone", Some("discover"))?;
    let finish = (|| -> Result<(), String> {
        for (k, o) in outs.iter().enumerate() {
            let f = FREQS[k % FREQS.len()];
            let samples = generate(Tone::Sine { hz: f }, RATE, TONE_S, -12.0);
            wav::write(
                &tone_path,
                &wav::Wav {
                    rate: RATE,
                    channels: 1,
                    samples,
                },
            )?;
            let lim = limits(ctx.cfg, o, false, ctx.minute);
            let steps = ladder(ctx.cfg.quiet_volume, lim.volume);
            let mut state = OutState {
                name: o.node_name.clone(),
                is_default: o.is_default,
                is_hdmi: is_hdmi(o),
                availability: o.availability.clone(),
                freq: f,
                max_volume: 0.0,
                error: None,
            };
            let (mut guard, _) =
                match VolumeGuard::apply(ctx.run, o, &lim, Want::Level(steps[0]), true) {
                    Ok(g) => g,
                    Err(e) => {
                        state.error = Some(e);
                        out_states.push(state);
                        continue;
                    }
                };
            for v in &steps {
                let v = v.min(lim.volume);
                if let Err(e) = guard.set(v) {
                    state.error = Some(e);
                    break;
                }
                state.max_volume = v;
                std::thread::sleep(Duration::from_millis(250));
                let marks: Vec<usize> = recs
                    .iter()
                    .map(|(_, r)| r.as_ref().map_or(0, |x| x.len()))
                    .collect();
                let played = Playing::start(&o.node_name, &tone_path, 1.0)
                    .and_then(|p| p.wait(Duration::from_secs_f64(TONE_S + 4.0)));
                if let Err(e) = played {
                    state.error = Some(e);
                    break;
                }
                audible_s += TONE_S;
                std::thread::sleep(Duration::from_secs_f64(TAIL_S));
                let mut any = false;
                for ((d, r), mark) in recs.iter().zip(&marks) {
                    let Ok(rec) = r else { continue };
                    let from = mark.saturating_sub((LEAD_S * f64::from(RATE)) as usize);
                    let slice = rec.since(from);
                    let lead = (*mark - from) as f64 / f64::from(RATE);
                    let h = hear_tone(&slice, RATE, f, lead, MIN_SNR_DB);
                    any |= h.heard;
                    obs.retain(|x| !(x.output == o.node_name && x.input == d.node_name));
                    obs.push(Obs {
                        output: o.node_name.clone(),
                        input: d.node_name.clone(),
                        freq: f,
                        volume: v,
                        heard: h.heard,
                        level_dbfs: h.level_dbfs,
                        snr_db: h.snr_db,
                        reason: h.reason,
                    });
                }
                if any {
                    break;
                }
            }
            let restored = guard.restore();
            out_states.push(state);
            restored?;
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&tone_path);
    // Input states from everything each recorded.
    let mut in_states = Vec::new();
    for (d, r) in recs {
        let muted = levels::get(ctx.run, d.id).ok().map(|l| l.muted);
        match r {
            Ok(rec) => {
                let all = rec.stop();
                let st = super::analysis::stats(&all, RATE, -60.0);
                in_states.push(InState {
                    name: d.node_name.clone(),
                    is_default: d.is_default,
                    error: None,
                    all_zero: st.all_zero,
                    muted,
                    noise_rms_dbfs: st.rms_dbfs,
                });
            }
            Err(e) => in_states.push(InState {
                name: d.node_name.clone(),
                is_default: d.is_default,
                error: Some(e),
                all_zero: false,
                muted,
                noise_rms_dbfs: super::analysis::FLOOR_DB,
            }),
        }
    }
    finish?;
    let c = conclude(&out_states, &in_states, &obs);
    let result = json!({
        "at_epoch": epoch(), "fingerprint": fingerprint(ctx.devs), "urgent": urgent, "quiet_hours_at_run": quiet_now,
        "audible_seconds": super::heard::round3(audible_s), "include_bluetooth": include_bt,
        "inputs": c.inputs, "outputs": c.outputs, "loopback_pairs": c.pairs,
        "observations": obs.iter().map(|o| json!({"output": o.output, "input": o.input, "frequency_hz": o.freq, "volume": o.volume,
            "heard": o.heard, "level_dbfs": round1(o.level_dbfs), "snr_db": round1(o.snr_db), "reason": o.reason})).collect::<Vec<_>>(),
        "selection": {
            "alert_output": c.alert_output, "alert_output_reason": c.alert_output_reason,
            "verify_input": c.verify_input, "verify_input_reason": c.verify_input_reason,
        },
        "operator_check": c.operator_check,
        "method": "Each output played a 0.4 s tone of its own frequency while every candidate input recorded; a tone is heard when its level at that frequency is 12 dB over the same frequency before it and 10 dB over neighbouring frequencies. Volume rose from the quiet volume to the cap only while nothing was heard. Volumes and mutes were restored.",
    });
    let mut out = result;
    if partial {
        out["partial"] = json!("the run was limited to the named device(s), so it was not stored and does not change the selection");
    } else {
        out["stored_in"] = json!(save(&dir, &out)?);
    }
    Ok(out)
}

/// `bench_audio_discovery`: the stored result, whether it is stale, and what `auto` selects now.
pub fn report(ctx: &Ctx, _args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_audio_discover_report]
    let Some(st) = ctx.discovery else {
        return Ok(
            json!({"stored": false, "note": "no discovery has been run on this machine; bench_audio_discover makes the (short, quiet) test tones"}),
        );
    };
    let changes = st.changes(ctx.devs);
    let select = |dir: Dir, key: &str| {
        super::tools::pick(ctx, &json!({}), key, dir)
            .ok()
            .map(|d| d.node_name.clone())
    };
    Ok(json!({
        "stored": true, "stale": !changes.is_empty(), "changes_since": changes,
        "age_s": epoch().saturating_sub(st.at_epoch), "result": st.raw,
        "auto_selects_now": {"alert_output": select(Dir::Output, "output"), "verify_input": select(Dir::Input, "input")},
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::analysis::synth::*;
    use crate::audio::testkit::devbox_devices;

    fn out(name: &str, default: bool, hdmi: bool, f: f64) -> OutState {
        OutState {
            name: name.into(),
            is_default: default,
            is_hdmi: hdmi,
            availability: None,
            freq: f,
            max_volume: 0.25,
            error: None,
        }
    }

    fn inp(name: &str, default: bool, zero: bool) -> InState {
        InState {
            name: name.into(),
            is_default: default,
            error: None,
            all_zero: zero,
            muted: None,
            noise_rms_dbfs: if zero { -200.0 } else { -50.0 },
        }
    }

    fn heard(o: &str, i: &str, snr: f64, level: f64) -> Obs {
        Obs {
            output: o.into(),
            input: i.into(),
            freq: 1000.0,
            volume: 0.25,
            heard: true,
            level_dbfs: level,
            snr_db: snr,
            reason: String::new(),
        }
    }

    fn silent(o: &str, i: &str) -> Obs {
        Obs {
            heard: false,
            level_dbfs: -120.0,
            snr_db: 0.0,
            reason: "nothing".into(),
            ..heard(o, i, 0.0, 0.0)
        }
    }

    fn status<'a>(v: &'a [Value], name: &str) -> &'a str {
        v.iter().find(|x| x["name"] == name).unwrap()["status"]
            .as_str()
            .unwrap()
    }

    // Tests [@ANCHOR: audio_discover_conclude]
    #[test]
    fn a_speaker_and_a_microphone_are_present_only_because_one_heard_the_other() {
        let outs = [
            out("speaker", true, false, 1000.0),
            out("hdmi1", false, true, 1300.0),
        ];
        let ins = [inp("dmic", true, false), inp("line", false, false)];
        let obs = [
            heard("speaker", "dmic", 30.0, -40.0),
            silent("speaker", "line"),
            silent("hdmi1", "dmic"),
            silent("hdmi1", "line"),
        ];
        let c = conclude(&outs, &ins, &obs);
        assert_eq!(status(&c.outputs, "speaker"), "sound_heard");
        assert_eq!(status(&c.outputs, "hdmi1"), "no_evidence_of_sound");
        let hdmi = c.outputs.iter().find(|x| x["name"] == "hdmi1").unwrap();
        assert!(hdmi["detail"]
            .as_str()
            .unwrap()
            .contains("speaker absent, muted, or no microphone in range"));
        assert_eq!(status(&c.inputs, "dmic"), "microphone_heard_tone");
        assert_eq!(status(&c.inputs, "line"), "microphone_live_noise_floor");
        assert_eq!(c.alert_output.as_deref(), Some("speaker"));
        assert_eq!(c.verify_input.as_deref(), Some("dmic"));
        assert_eq!(c.pairs.len(), 1);
        assert!(c.operator_check.is_none());
    }

    #[test]
    fn a_muted_or_all_zero_input_is_dead_and_never_chosen_and_never_proves_a_speaker() {
        let outs = [out("speaker", true, false, 1000.0)];
        let mut muted = inp("mic", true, true);
        muted.muted = Some(true);
        let c = conclude(&outs, &[muted], &[silent("speaker", "mic")]);
        assert_eq!(status(&c.inputs, "mic"), "dead");
        assert!(c.inputs[0]["detail"]
            .as_str()
            .unwrap()
            .contains("muted in the mixer"));
        assert_eq!(status(&c.outputs, "speaker"), "cannot_verify_no_microphone");
        assert!(c.outputs[0]["detail"]
            .as_str()
            .unwrap()
            .contains("only the operator can confirm it by ear"));
        assert_eq!((c.alert_output, c.verify_input), (None, None));
        assert!(c.operator_check.unwrap().contains("Ask the operator"));
    }

    #[test]
    fn no_inputs_at_all_means_the_operator_must_listen() {
        let c = conclude(&[out("speaker", true, false, 1000.0)], &[], &[]);
        assert_eq!(status(&c.outputs, "speaker"), "cannot_verify_no_microphone");
        assert!(c.operator_check.is_some() && c.verify_input.is_none());
    }

    #[test]
    fn a_failed_stream_is_unavailable_and_an_output_that_could_not_play_is_an_error_not_silence() {
        let mut bad_in = inp("usb", false, false);
        bad_in.error = Some("pw-record ended early".into());
        let mut bad_out = out("hp", false, false, 1000.0);
        bad_out.error = Some("pw-play failed".into());
        let c = conclude(
            &[bad_out],
            &[bad_in, inp("dmic", true, false)],
            &[silent("hp", "dmic")],
        );
        assert_eq!(status(&c.inputs, "usb"), "unavailable");
        assert_eq!(status(&c.outputs, "hp"), "error");
    }

    #[test]
    fn hdmi_is_chosen_only_when_nothing_else_was_heard_and_the_default_comes_first() {
        let ins = [inp("dmic", true, false)];
        let both = [
            out("hdmi1", true, true, 1000.0),
            out("analog", false, false, 1300.0),
        ];
        let obs = [
            heard("hdmi1", "dmic", 40.0, -30.0),
            heard("analog", "dmic", 20.0, -45.0),
        ];
        assert_eq!(
            conclude(&both, &ins, &obs).alert_output.as_deref(),
            Some("analog")
        );
        let only_hdmi = [
            heard("hdmi1", "dmic", 40.0, -30.0),
            silent("analog", "dmic"),
        ];
        let c = conclude(&both, &ins, &only_hdmi);
        assert_eq!(c.alert_output.as_deref(), Some("hdmi1"));
        assert!(c.alert_output_reason.contains("HDMI"));
        let two = [
            out("a", false, false, 1000.0),
            out("b", true, false, 1300.0),
        ];
        let obs = [
            heard("a", "dmic", 40.0, -30.0),
            heard("b", "dmic", 20.0, -45.0),
        ];
        assert_eq!(
            conclude(&two, &ins, &obs).alert_output.as_deref(),
            Some("b"),
            "the system default wins over a louder output"
        );
    }

    #[test]
    fn the_quiet_pair_is_flagged_as_crosstalk_and_a_shared_tone_is_named() {
        let outs = [
            out("left", false, false, 1000.0),
            out("right", false, false, 1000.0),
        ];
        let ins = [inp("mic1", true, false), inp("mic2", false, false)];
        let obs = [
            heard("left", "mic1", 40.0, -30.0),
            heard("left", "mic2", 14.0, -62.0),
            silent("right", "mic1"),
            silent("right", "mic2"),
        ];
        let c = conclude(&outs, &ins, &obs);
        let weak = c.pairs.iter().find(|p| p["input"] == "mic2").unwrap();
        assert_eq!(weak["crosstalk_suspect"], true);
        assert_eq!(c.pairs[0]["crosstalk_suspect"], false);
        assert_eq!(c.outputs[0]["shared_frequency_with"][0], "right");
        assert_eq!(c.verify_input.as_deref(), Some("mic1"));
    }

    #[test]
    fn a_live_input_that_heard_nothing_is_still_a_microphone_with_the_reason() {
        let c = conclude(
            &[out("hdmi", true, true, 1000.0)],
            &[inp("dmic", true, false)],
            &[silent("hdmi", "dmic")],
        );
        assert_eq!(status(&c.inputs, "dmic"), "microphone_live_noise_floor");
        assert_eq!(c.verify_input.as_deref(), Some("dmic"));
        assert!(c.verify_input_reason.contains("heard no tone"));
        assert_eq!(c.alert_output, None);
        assert!(c.operator_check.unwrap().contains("No output was heard"));
    }

    #[test]
    fn the_jack_state_is_reported_as_supporting_evidence_only() {
        let mut o = out("hdmi", false, true, 1000.0);
        o.availability = Some("no".into());
        let c = conclude(&[o], &[inp("dmic", true, false)], &[silent("hdmi", "dmic")]);
        assert!(c.outputs[0]["port_evidence"]
            .as_str()
            .unwrap()
            .contains("supporting evidence only"));
        assert_eq!(c.outputs[0]["status"], "no_evidence_of_sound");
    }

    // Tests [@ANCHOR: audio_discover_ladder]
    #[test]
    fn the_volume_ladder_starts_quiet_and_never_passes_the_limit() {
        assert_eq!(ladder(0.15, 0.5), vec![0.15, 0.274, 0.5]);
        assert_eq!(ladder(0.15, 0.15), vec![0.15]);
        assert_eq!(ladder(0.15, 0.1), vec![0.1]);
        assert_eq!(ladder(0.15, 0.17), vec![0.17]);
        assert!(ladder(0.15, 0.5).windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn the_measurement_hears_each_outputs_own_tone_and_not_another_outputs() {
        // Output k plays FREQS[k]; the recording holds output 1's tone at 1300 Hz over room noise.
        let n = noise(48000, 1.6, 0.003, 21);
        let rec = add(&n, &at(48000, 1.6, 0.5, &sine(48000, 0.4, 1300.0, 0.02)));
        let own = hear_tone(&rec, 48000, 1300.0, LEAD_S, MIN_SNR_DB);
        assert!(own.heard, "{own:?}");
        for other in [1000.0, 1700.0, 2200.0] {
            assert!(
                !hear_tone(&rec, 48000, other, LEAD_S, MIN_SNR_DB).heard,
                "{other}"
            );
        }
        let zeros = hear_tone(&vec![0.0; 76800], 48000, 1300.0, LEAD_S, MIN_SNR_DB);
        assert!(zeros.input_all_zero && !zeros.heard);
    }

    // Tests [@ANCHOR: audio_discover_stale]
    // Tests [@ANCHOR: bench_mcp_audio_discover_fingerprint]
    #[test]
    fn a_changed_device_list_makes_the_result_stale_and_reserved_devices_do_not() {
        let devs = devbox_devices();
        let fp = fingerprint(&devs);
        assert!(
            !fp.iter().any(|l| l.contains("USB_AUDIO_CODEC")),
            "reserved devices are not in the fingerprint"
        );
        let st = Stored {
            at_epoch: 0,
            fingerprint: fp.clone(),
            alert_output: None,
            verify_input: None,
            raw: json!({}),
        };
        assert!(st.changes(&devs).is_empty());
        let fewer: Vec<Device> = devs
            .iter()
            .filter(|d| !d.description.ends_with("Speaker"))
            .cloned()
            .collect();
        let c = st.changes(&fewer);
        assert!(
            c.iter()
                .any(|x| x.starts_with("gone or changed") && x.contains("Speaker")),
            "{c:?}"
        );
        let mut plugged = devs.clone();
        for d in plugged
            .iter_mut()
            .filter(|d| d.port_type.as_deref() == Some("hdmi"))
        {
            d.availability = Some("yes".into());
        }
        assert!(
            !st.changes(&plugged).is_empty(),
            "plugging an HDMI monitor changes the fingerprint"
        );
        let mut renumbered = devs.clone();
        for (i, d) in renumbered.iter_mut().enumerate() {
            d.id = 900 + i as u32;
        }
        assert!(
            st.changes(&renumbered).is_empty(),
            "PipeWire ids are not part of the identity"
        );
    }

    // Tests [@ANCHOR: bench_mcp_audio_discover_load]
    // Tests [@ANCHOR: bench_mcp_audio_discover_save]
    #[test]
    fn the_stored_result_round_trips_with_its_selection() {
        let dir = std::env::temp_dir().join(format!("audio-disc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let v = json!({"at_epoch": 5, "fingerprint": ["output|a|-"], "selection": {"alert_output": "a", "verify_input": null}});
        save(&dir, &v).unwrap();
        let st = load(&dir).unwrap();
        assert_eq!(
            (st.at_epoch, st.alert_output.as_deref(), st.verify_input),
            (5, Some("a"), None)
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.join(FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(load(std::path::Path::new("/nonexistent/x")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Tests [@ANCHOR: bench_mcp_audio_discover_report]
    #[test]
    fn the_report_says_whether_a_discovery_is_stored_stale_and_what_auto_selects() {
        let devs = devbox_devices();
        let cfg = crate::audio::Config::default();
        let run = crate::audio::testkit::Scripted::new(&[]);
        fn mk<'a>(
            run: &'a crate::audio::testkit::Scripted,
            cfg: &'a crate::audio::Config,
            devs: &'a [Device],
            d: Option<&'a Stored>,
        ) -> Ctx<'a> {
            Ctx {
                run,
                cfg,
                devs,
                minute: 720,
                discovery: d,
            }
        }
        let none = report(&mk(&run, &cfg, &devs, None), &json!({})).unwrap();
        assert_eq!(none["stored"], false);
        assert!(none["note"].as_str().unwrap().contains("no discovery"));
        let mic = devs
            .iter()
            .find(|d| d.dir == Dir::Input && d.reserved.is_none() && !d.is_default)
            .unwrap();
        let mut st = Stored {
            at_epoch: epoch() - 90,
            fingerprint: fingerprint(&devs),
            alert_output: None,
            verify_input: Some(mic.node_name.clone()),
            raw: json!({"selection": "kept"}),
        };
        let fresh = report(&mk(&run, &cfg, &devs, Some(&st)), &json!({})).unwrap();
        assert_eq!(
            (fresh["stored"].clone(), fresh["stale"].clone()),
            (json!(true), json!(false))
        );
        assert!(fresh["changes_since"].as_array().unwrap().is_empty());
        assert!((90..95).contains(&fresh["age_s"].as_u64().unwrap()));
        assert_eq!(fresh["result"]["selection"], "kept");
        assert_eq!(
            fresh["auto_selects_now"]["verify_input"],
            json!(mic.node_name)
        );
        let default_out = devs
            .iter()
            .find(|d| d.dir == Dir::Output && d.is_default && d.reserved.is_none())
            .unwrap();
        assert_eq!(
            fresh["auto_selects_now"]["alert_output"],
            json!(default_out.node_name)
        );
        st.fingerprint.push("output|gone|-".into());
        let stale = report(&mk(&run, &cfg, &devs, Some(&st)), &json!({})).unwrap();
        assert_eq!(stale["stale"], true);
        assert!(stale["changes_since"][0]
            .as_str()
            .unwrap()
            .starts_with("gone or changed"));
    }
}

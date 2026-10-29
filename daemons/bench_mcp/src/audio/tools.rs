// SPDX-License-Identifier: AGPL-3.0-or-later
//! The record, meter, play, loopback, levels and volume tools.

use super::analysis::{self, db, stats};
use super::devices::{self, Device, Dir};
use super::engine::{Playing, Recorder};
use super::heard::{hear_tone, round1, round3};
use super::safety::{limits, VolumeGuard, Want};
use super::{levels, store, tones, wav, Config, Run};
use serde_json::{json, Value};
use std::time::Duration;

/// What every tool needs: the program runner, the config, the device list, and the minute of the local day.
pub struct Ctx<'a> {
    pub run: &'a dyn Run,
    pub cfg: &'a Config,
    pub devs: &'a [Device],
    pub minute: u32,
    /// The stored result of the last audio path discovery, which decides `auto` selections.
    pub discovery: Option<&'a super::discover::Stored>,
}

pub const NOTE_HEARS_ROOM: &str = "The input hears the room and a recording can contain speech. It is kept only in the managed capture directory (retention applies) and no tool uploads it.";

fn num(args: &Value, k: &str) -> Option<f64> {
    args.get(k).and_then(|v| v.as_f64())
}

fn flag(args: &Value, k: &str) -> bool {
    args.get(k).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn bounded(args: &Value, k: &str, default: f64, lo: f64, hi: f64) -> Result<f64, String> {
    // [@ANCHOR: bench_mcp_audio_tools_bounded]
    let v = num(args, k).unwrap_or(default);
    if !(lo..=hi).contains(&v) || !v.is_finite() {
        return Err(format!("{k} {v} is outside {lo} to {hi}"));
    }
    Ok(v)
}

fn rate_arg(args: &Value) -> Result<u32, String> {
    Ok(bounded(args, "sample_rate_hz", 48000.0, 8000.0, 96000.0)? as u32)
}

/// The device for a tool: the named one, else the configured alert output or verify input, else the system default.
pub fn pick<'a>(ctx: &Ctx<'a>, args: &Value, key: &str, dir: Dir) -> Result<&'a Device, String> {
    if let Some(n) = args.get(key).and_then(|n| n.as_str()) {
        return devices::resolve(ctx.devs, n, dir);
    }
    let setting = match dir {
        Dir::Output => &ctx.cfg.alert_output,
        Dir::Input => &ctx.cfg.verify_input,
    };
    if setting != "auto" {
        return devices::resolve(ctx.devs, setting, dir);
    }
    // [@ANCHOR: audio_select_auto]
    // `auto`: the device discovery found by listening, if it is still there; else the system default.
    let found = ctx.discovery.and_then(|d| match dir {
        Dir::Output => d.alert_output.as_deref(),
        Dir::Input => d.verify_input.as_deref(),
    });
    if let Some(d) = found.and_then(|n| {
        ctx.devs
            .iter()
            .find(|d| d.dir == dir && d.node_name == n && d.reserved.is_none())
    }) {
        return Ok(d);
    }
    ctx.devs
        .iter()
        .find(|d| d.dir == dir && d.is_default && d.reserved.is_none())
        .ok_or_else(|| {
            format!(
                "there is no usable default {}: name one with \"{key}\" (bench_audio_devices lists them)",
                dir.word()
            )
        })
}

pub fn stats_json(s: &analysis::Stats) -> Value {
    json!({
        "peak_dbfs": round1(s.peak_dbfs), "rms_dbfs": round1(s.rms_dbfs), "clipped_samples": s.clipped,
        "silence_fraction": (s.silence_fraction * 1000.0).round() / 1000.0, "all_zero": s.all_zero,
        "dc_offset": (s.dc_offset * 1e6).round() / 1e6,
    })
}

/// Records from the start of the stream until `need` samples are available, or, with a threshold, until the level passes it.
struct Taken {
    samples: Vec<f32>,
    triggered: Option<bool>,
    waited_s: f64,
}

fn take(
    rec: &mut Recorder,
    rate: u32,
    seconds: f64,
    trigger_dbfs: Option<f64>,
    max_wait_s: f64,
) -> Result<Taken, String> {
    // [@ANCHOR: bench_mcp_audio_tools_take]
    let r = f64::from(rate);
    rec.wait_samples(1, Duration::from_secs(5))?;
    let need = (seconds * r) as usize;
    let Some(thr) = trigger_dbfs else {
        rec.wait_samples(need, Duration::from_secs_f64(seconds + 5.0))?;
        let mut s = rec.since(0);
        s.truncate(need);
        return Ok(Taken {
            samples: s,
            triggered: None,
            waited_s: 0.0,
        });
    };
    let frame = (0.01 * r) as usize;
    let preroll = (0.1 * r) as usize;
    let begin = std::time::Instant::now();
    let mut scanned = 0usize;
    loop {
        let all = rec.since(0);
        while scanned + frame <= all.len() {
            let peak = all[scanned..scanned + frame]
                .iter()
                .fold(0f64, |m, x| m.max(f64::from(x.abs())));
            if db(peak) >= thr {
                let start = scanned.saturating_sub(preroll);
                rec.wait_samples(start + need, Duration::from_secs_f64(seconds + 5.0))?;
                let mut s = rec.since(start);
                s.truncate(need);
                return Ok(Taken {
                    samples: s,
                    triggered: Some(true),
                    waited_s: scanned as f64 / r,
                });
            }
            scanned += frame;
        }
        if begin.elapsed().as_secs_f64() > max_wait_s {
            return Ok(Taken {
                samples: all,
                triggered: Some(false),
                waited_s: max_wait_s,
            });
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn record(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: audio_record_tool]
    let monitor = flag(args, "monitor_of_output");
    let dev = if monitor {
        pick(ctx, args, "input", Dir::Output)?
    } else {
        pick(ctx, args, "input", Dir::Input)?
    };
    let seconds = bounded(args, "seconds", 3.0, 0.1, ctx.cfg.max_record_s)?;
    let rate = rate_arg(args)?;
    let thr = num(args, "start_on_threshold_dbfs");
    if thr.is_some_and(|t| !(-120.0..=0.0).contains(&t)) {
        return Err("start_on_threshold_dbfs must be -120 to 0".into());
    }
    let max_wait = bounded(args, "max_wait_s", 10.0, 0.1, 120.0)?;
    let silence_dbfs = num(args, "silence_threshold_dbfs").unwrap_or(-60.0);
    let mut rec = Recorder::start(&dev.node_name, rate, monitor)?;
    let t = take(&mut rec, rate, seconds, thr, max_wait)?;
    drop(rec);
    let st = stats(&t.samples, rate, silence_dbfs);
    let mut out = json!({
        "input": dev.node_name, "alias": dev.alias, "monitor_of_output": monitor, "sample_rate_hz": rate,
        "seconds": round3(t.samples.len() as f64 / f64::from(rate)), "levels": stats_json(&st), "note": NOTE_HEARS_ROOM,
    });
    if t.triggered == Some(false) {
        out["triggered"] = json!(false);
        out["result"] = json!(format!(
            "nothing reached {} dBFS in {max_wait:.1} s; no file was written",
            thr.unwrap_or_default()
        ));
        return Ok(out);
    }
    if let Some(tr) = t.triggered {
        out["triggered"] = json!(tr);
        out["trigger_after_s"] = json!(round3(t.waited_s));
    }
    let dir = ctx.cfg.capture_dir();
    let label = args
        .get("label")
        .and_then(|l| l.as_str())
        .or(dev.alias.as_deref());
    let path = store::new_path(&dir, "rec", label)?;
    wav::write(
        &path,
        &wav::Wav {
            rate,
            channels: 1,
            samples: t.samples,
        },
    )?;
    let pruned = store::prune(&dir, ctx.cfg.retention_files, ctx.cfg.retention_days);
    out["file"] = json!(path);
    out["retention_removed"] = json!(pruned);
    if st.all_zero {
        out["warning"] =
            json!("every sample is exactly zero: the input is muted, disconnected or dead");
    }
    Ok(out)
}

pub fn meter(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_audio_tools_meter]
    let dev = pick(ctx, args, "input", Dir::Input)?;
    let seconds = bounded(args, "seconds", 3.0, 0.2, 30.0)?;
    let interval = bounded(args, "interval_ms", 250.0, 50.0, 2000.0)? / 1000.0;
    let rate = 48000u32;
    let mut rec = Recorder::start(&dev.node_name, rate, false)?;
    let t = take(&mut rec, rate, seconds, None, 0.0)?;
    drop(rec);
    let step = (interval * f64::from(rate)) as usize;
    let readings: Vec<Value> = t
        .samples
        .chunks(step)
        .filter(|c| c.len() == step)
        .enumerate()
        .map(|(i, c)| {
            let s = stats(c, rate, -60.0);
            json!({"t_s": round3(i as f64 * interval), "peak_dbfs": round1(s.peak_dbfs), "rms_dbfs": round1(s.rms_dbfs)})
        })
        .collect();
    Ok(json!({
        "input": dev.node_name, "interval_ms": interval * 1000.0, "readings": readings,
        "overall": stats_json(&stats(&t.samples, rate, -60.0)), "note": NOTE_HEARS_ROOM,
    }))
}

/// A sound file or generated tone, ready to play: the file, its length, and whether the file is temporary.
struct Source {
    path: std::path::PathBuf,
    seconds: f64,
    temporary: bool,
    description: String,
}

fn source(ctx: &Ctx, args: &Value) -> Result<Source, String> {
    // [@ANCHOR: bench_mcp_audio_tools_source]
    let dir = ctx.cfg.capture_dir();
    let max = ctx.cfg.max_play_s;
    if let Some(name) = args.get("file").and_then(|f| f.as_str()) {
        let path = store::managed_file(&dir, name)?;
        let w = wav::read(&path)?;
        if w.seconds() > max {
            return Err(format!(
                "{name} is {:.1} s long and the limit is max_play_s {max}",
                w.seconds()
            ));
        }
        return Ok(Source {
            seconds: w.seconds(),
            description: format!("file {name}"),
            path,
            temporary: false,
        });
    }
    let rate = 48000;
    let tone = tones::parse(args, rate)?;
    let seconds = bounded(args, "seconds", 1.0, 0.05, max)?;
    let level = tones::level(args, -12.0)?;
    let samples = tones::generate(tone, rate, seconds, level);
    let path = store::new_path(&dir, "tone", None)?;
    wav::write(
        &path,
        &wav::Wav {
            rate,
            channels: 1,
            samples,
        },
    )?;
    Ok(Source {
        path,
        seconds,
        temporary: true,
        description: format!("{tone:?} at {level} dBFS"),
    })
}

/// Sets the output's volume within the limits, runs `sound` (which makes the noise and returns how long it took), and puts the
/// volume and mute back whether or not it succeeded.
pub fn play_guarded(
    ctx: &Ctx,
    dev: &Device,
    lim: &super::safety::Limits,
    want: Want,
    unmute: bool,
    sound: impl FnOnce() -> Result<Duration, String>,
) -> Result<(super::safety::Applied, Duration), String> {
    // [@ANCHOR: bench_mcp_audio_tools_play_guarded]
    let (mut guard, applied) = VolumeGuard::apply(ctx.run, dev, lim, want, unmute)?;
    let played = sound();
    let restored = guard.restore();
    let took = played?;
    restored?;
    Ok((applied, took))
}

pub fn play(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: audio_play_tool]
    let dev = pick(ctx, args, "output", Dir::Output)?;
    let lim = limits(ctx.cfg, dev, false, ctx.minute);
    let want = match num(args, "volume") {
        Some(v) if !(0.0..=1.0).contains(&v) => return Err("volume must be 0 to 1".into()),
        Some(v) => Want::Level(v),
        None => Want::Keep,
    };
    let src = source(ctx, args)?;
    let result = (|| {
        let (applied, took) = play_guarded(ctx, dev, &lim, want, flag(args, "unmute"), || {
            Playing::start(&dev.node_name, &src.path, 1.0)
                .and_then(|p| p.wait(Duration::from_secs_f64(src.seconds + 5.0)))
        })?;
        let mut out = json!({
            "output": dev.node_name, "alias": dev.alias, "played": src.description, "seconds": round3(src.seconds),
            "took_s": round3(took.as_secs_f64()), "volume_used": applied.volume, "volume_limit": lim.volume,
            "quiet_hours": lim.quiet, "volume_restored": applied.before.volume, "mute_restored": applied.before.muted,
        });
        if applied.muted {
            out["warning"] = json!("the output is muted, so nothing was audible; pass unmute: true to unmute it for the playback");
        }
        Ok(out)
    })();
    if src.temporary {
        let _ = std::fs::remove_file(&src.path);
    }
    result
}

pub fn loopback(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: audio_loopback_tool]
    let out_dev = pick(ctx, args, "output", Dir::Output)?;
    let in_dev = pick(ctx, args, "input", Dir::Input)?;
    let rate = 48000u32;
    let hz = bounded(args, "freq_hz", 1000.0, 100.0, 12000.0)?;
    let seconds = bounded(args, "seconds", 0.5, 0.2, ctx.cfg.max_play_s.min(5.0))?;
    let level = tones::level(args, -20.0)?;
    let lim = limits(ctx.cfg, out_dev, flag(args, "urgent"), ctx.minute);
    let want = match num(args, "volume") {
        Some(v) if !(0.0..=1.0).contains(&v) => return Err("volume must be 0 to 1".into()),
        Some(v) => Want::Level(v),
        None => Want::Level(lim.volume),
    };
    let samples = tones::generate(tones::Tone::Sine { hz }, rate, seconds, level);
    let path = store::new_path(&ctx.cfg.capture_dir(), "tone", None)?;
    wav::write(
        &path,
        &wav::Wav {
            rate,
            channels: 1,
            samples,
        },
    )?;
    let run = || -> Result<Value, String> {
        let (mut guard, applied) = VolumeGuard::apply(ctx.run, out_dev, &lim, want, true)?;
        let heard = (|| {
            let mut rec = Recorder::start(&in_dev.node_name, rate, false)?;
            let lead = (0.5 * f64::from(rate)) as usize;
            rec.wait_samples(lead, Duration::from_secs(5))?;
            let at = rec.len();
            let playing = Playing::start(&out_dev.node_name, &path, 1.0)?;
            let spawned = playing.started;
            playing.wait(Duration::from_secs_f64(seconds + 5.0))?;
            let tail = (0.4 * f64::from(rate)) as usize;
            let want_len = at + (seconds * f64::from(rate)) as usize + tail;
            rec.wait_samples(want_len, Duration::from_secs(5))?;
            let all = rec.stop();
            Ok::<_, String>((all, at, spawned))
        })();
        let restored = guard.restore();
        let (all, at, spawned) = heard?;
        restored?;
        let lead_s = at as f64 / f64::from(rate);
        let h = hear_tone(&all, rate, hz, lead_s, 12.0);
        let floor = stats(&all[..at.min(all.len())], rate, -60.0);
        let mut out = json!({
            "output": out_dev.node_name, "input": in_dev.node_name, "tone_hz": hz, "tone_level_dbfs": level,
            "volume_used": applied.volume, "volume_limit": lim.volume, "quiet_hours": lim.quiet,
            "result": h.json(), "noise_before": stats_json(&floor),
            "verdict": if h.heard { "working loopback: the input heard the output" } else { "not heard: see result.reason" },
        });
        if let Some(on) = h.onset_s {
            // Time from starting pw-play to the tone's half-amplitude point in the recording, minus the start offset: indicative.
            let _ = spawned;
            out["latency_ms_indicative"] = json!(round1((on - lead_s) * 1000.0));
            out["latency_note"] = json!("from the start of the pw-play process to the tone's half-amplitude point, so it includes process start-up and the graph's own buffering; it is not the device latency");
        }
        if h.input_all_zero {
            out["input_state"] = json!("dead: exact zeros");
        }
        Ok(out)
    };
    let r = run();
    let _ = std::fs::remove_file(&path);
    r
}

pub fn levels_get(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_audio_tools_levels_get]
    let dir = match args.get("direction").and_then(|d| d.as_str()) {
        Some("input") => Some(Dir::Input),
        Some("output") => Some(Dir::Output),
        Some(o) => return Err(format!("direction {o:?} is not output or input")),
        None => None,
    };
    let chosen: Vec<&Device> = match args.get("device").and_then(|d| d.as_str()) {
        Some(n) => vec![devices::resolve(ctx.devs, n, dir.unwrap_or(Dir::Output))?],
        None => ctx
            .devs
            .iter()
            .filter(|d| d.reserved.is_none() && dir.is_none_or(|x| x == d.dir))
            .collect(),
    };
    let mut out = Vec::new();
    for d in chosen {
        let l = levels::get(ctx.run, d.id)?;
        out.push(json!({
            "name": d.node_name, "alias": d.alias, "direction": d.dir.word(), "volume": l.volume, "muted": l.muted,
            "gain_db": round1(levels::gain_db(l.volume)),
            "volume_cap": if d.dir == Dir::Output { json!(ctx.cfg.cap_for(d)) } else { Value::Null },
        }));
    }
    Ok(
        json!({"levels": out, "volume_scale": "wpctl volume; the signal amplitude factor is the cube"}),
    )
}

pub fn set_volume(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: audio_set_volume_tool]
    let dir = match args.get("direction").and_then(|d| d.as_str()) {
        Some("input") => Dir::Input,
        _ => Dir::Output,
    };
    let name = args
        .get("device")
        .and_then(|d| d.as_str())
        .ok_or("device is required")?;
    let dev = devices::resolve(ctx.devs, name, dir)?;
    let vol = num(args, "volume");
    let mute = args.get("mute").and_then(|m| m.as_bool());
    if vol.is_none() && mute.is_none() {
        return Err("give volume, mute, or both".into());
    }
    let before = levels::get(ctx.run, dev.id)?;
    if let Some(v) = vol {
        let cap = if dir == Dir::Output {
            ctx.cfg.cap_for(dev)
        } else {
            1.0
        };
        if !(0.0..=cap).contains(&v) {
            return Err(format!(
                "volume {v} is outside 0 to {cap}, the cap for {}",
                dev.node_name
            ));
        }
        levels::set_volume(ctx.run, dev.id, v)?;
    }
    if let Some(m) = mute {
        levels::set_mute(ctx.run, dev.id, m)?;
    }
    let after = levels::get(ctx.run, dev.id)?;
    Ok(json!({
        "name": dev.node_name, "alias": dev.alias,
        "before": {"volume": before.volume, "muted": before.muted},
        "after": {"volume": after.volume, "muted": after.muted},
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::testkit::{devbox_devices, fake_programs, scratch, Scripted};
    use crate::audio::wav::Wav;

    fn f32_file(node: &str, samples: &[f32]) {
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        std::fs::write(fake_programs().join(format!("rec-{node}.f32")), bytes).unwrap();
    }

    fn sine(rate: u32, seconds: f64, amp: f32) -> Vec<f32> {
        (0..(f64::from(rate) * seconds) as usize)
            .map(|i| {
                amp * (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / f64::from(rate)).sin()
                    as f32
            })
            .collect()
    }

    // Tests [@ANCHOR: bench_mcp_audio_tools_bounded]
    #[test]
    fn arguments_are_taken_inside_their_bounds_or_refused() {
        let a = json!({"seconds": 2.5, "nan": null});
        assert_eq!(bounded(&a, "seconds", 1.0, 0.1, 5.0).unwrap(), 2.5);
        assert_eq!(bounded(&a, "missing", 1.0, 0.1, 5.0).unwrap(), 1.0);
        let e = bounded(&a, "seconds", 1.0, 0.1, 2.0).unwrap_err();
        assert!(e.contains("seconds 2.5 is outside 0.1 to 2"), "{e}");
        assert!(bounded(&json!({"s": 0.0}), "s", 1.0, 0.1, 2.0).is_err());
        assert!(bounded(&json!({}), "s", f64::NAN, 0.1, 2.0).is_err());
    }

    // Tests [@ANCHOR: bench_mcp_audio_tools_take]
    #[test]
    fn a_take_is_the_first_seconds_or_starts_when_the_level_passes_the_threshold() {
        let rate = 48000;
        let quiet = "bench-take-plain";
        let mut s = sine(rate, 0.6, 0.1);
        s[0] = 0.1;
        f32_file(quiet, &s);
        let mut rec = Recorder::start(quiet, rate, false).unwrap();
        let t = take(&mut rec, rate, 0.5, None, 0.0).unwrap();
        assert_eq!((t.samples.len(), t.triggered), (24000, None));
        assert_eq!(&t.samples[..], &s[..24000]);
        drop(rec);

        // 0.3 s of silence, then a loud tone: the take starts there, with 0.1 s of pre-roll.
        let late = "bench-take-trigger";
        let mut v = vec![0.0f32; (0.3 * f64::from(rate)) as usize];
        v.extend(sine(rate, 0.6, 0.5));
        f32_file(late, &v);
        let mut rec = Recorder::start(late, rate, false).unwrap();
        let t = take(&mut rec, rate, 0.2, Some(-20.0), 3.0).unwrap();
        assert_eq!(t.triggered, Some(true));
        assert_eq!(t.samples.len(), 9600);
        assert!((t.waited_s - 0.3).abs() < 0.03, "{}", t.waited_s);
        assert!(t.samples.iter().any(|x| x.abs() > 0.4));
        drop(rec);

        // Nothing passes the threshold within the wait: no trigger, and what was heard is returned.
        let hush = "bench-take-hush";
        f32_file(hush, &vec![0.001f32; 48000]);
        let mut rec = Recorder::start(hush, rate, false).unwrap();
        let t = take(&mut rec, rate, 0.2, Some(-20.0), 0.2).unwrap();
        assert_eq!(t.triggered, Some(false));
        assert!(t.waited_s == 0.2 && !t.samples.is_empty());
    }

    // Tests [@ANCHOR: bench_mcp_audio_tools_meter]
    #[test]
    fn the_meter_reports_peak_and_rms_per_interval() {
        let devs = devbox_devices();
        let mic = devs
            .iter()
            .find(|d| d.dir == Dir::Input && d.reserved.is_none())
            .unwrap();
        f32_file(&mic.node_name, &sine(48000, 1.0, 0.1));
        let cfg = Config::default();
        let run = Scripted::new(&[]);
        let ctx = Ctx {
            run: &run,
            cfg: &cfg,
            devs: &devs,
            minute: 720,
            discovery: None,
        };
        let v = meter(&ctx, &json!({"input": mic.node_name, "seconds": 0.5})).unwrap();
        let readings = v["readings"].as_array().unwrap();
        assert_eq!(readings.len(), 2);
        assert_eq!(readings[1]["t_s"], 0.25);
        assert!((readings[0]["peak_dbfs"].as_f64().unwrap() + 20.0).abs() < 0.6);
        assert!((readings[0]["rms_dbfs"].as_f64().unwrap() + 23.0).abs() < 0.6);
        assert_eq!(v["input"], json!(mic.node_name));
        assert!(meter(&ctx, &json!({"input": mic.node_name, "seconds": 0.1})).is_err());
        assert!(meter(&ctx, &json!({"input": mic.node_name, "interval_ms": 10})).is_err());
    }

    // Tests [@ANCHOR: bench_mcp_audio_tools_source]
    #[test]
    fn a_source_is_a_managed_file_within_the_limit_or_a_generated_tone() {
        let d = scratch("source");
        let devs = devbox_devices();
        let cfg = Config {
            capture_dir: Some(d.clone()),
            ..Config::default()
        };
        let run = Scripted::new(&[]);
        let ctx = Ctx {
            run: &run,
            cfg: &cfg,
            devs: &devs,
            minute: 720,
            discovery: None,
        };
        let w = |name: &str, seconds: usize| {
            crate::audio::wav::write(
                &d.join(name),
                &Wav {
                    rate: 8000,
                    channels: 1,
                    samples: vec![0.1; 8000 * seconds],
                },
            )
            .unwrap();
        };
        w("short.wav", 2);
        w("long.wav", 11);
        let s = source(&ctx, &json!({"file": "short.wav"})).unwrap();
        assert!((s.seconds - 2.0).abs() < 1e-9 && !s.temporary);
        assert_eq!(s.description, "file short.wav");
        assert!(s.path.exists());
        let e = source(&ctx, &json!({"file": "long.wav"})).err().unwrap();
        assert!(e.contains("limit is max_play_s"), "{e}");
        assert!(source(&ctx, &json!({"file": "../etc/passwd"})).is_err());
        let t = source(
            &ctx,
            &json!({"seconds": 0.5, "freq_hz": 800.0, "level_dbfs": -20.0}),
        )
        .unwrap();
        assert!(t.temporary && (t.seconds - 0.5).abs() < 1e-9);
        assert!(t.description.contains("-20"), "{}", t.description);
        let back = crate::audio::wav::read(&t.path).unwrap();
        assert_eq!(back.samples.len(), 48000 / 2);
        assert!(source(&ctx, &json!({"seconds": 11.0})).is_err());
        assert!(source(&ctx, &json!({"tone": "square"})).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    // Tests [@ANCHOR: bench_mcp_audio_tools_play_guarded]
    #[test]
    fn the_volume_is_set_for_the_sound_and_put_back_whether_it_works_or_not() {
        let devs = devbox_devices();
        let spk = devs
            .iter()
            .find(|d| d.description.ends_with("Speaker"))
            .unwrap();
        let cfg = Config::default();
        let run = Scripted::new(&[(spk.id, 0.9, true)]);
        let ctx = Ctx {
            run: &run,
            cfg: &cfg,
            devs: &devs,
            minute: 720,
            discovery: None,
        };
        let lim = limits(&cfg, spk, false, 720);
        let mut during = None;
        let (applied, took) = play_guarded(&ctx, spk, &lim, Want::Keep, true, || {
            during = Some(run.volumes.borrow()[0]);
            Ok(Duration::from_millis(7))
        })
        .unwrap();
        assert_eq!(during, Some((spk.id, 0.5, false)));
        assert_eq!((applied.volume, took), (0.5, Duration::from_millis(7)));
        assert_eq!(*run.volumes.borrow(), vec![(spk.id, 0.9, true)]);
        let e = play_guarded(&ctx, spk, &lim, Want::Level(0.2), true, || {
            Err("pw-play failed".into())
        })
        .unwrap_err();
        assert_eq!(e, "pw-play failed");
        assert_eq!(*run.volumes.borrow(), vec![(spk.id, 0.9, true)]);
        let mut reserved = spk.clone();
        reserved.reserved = Some("codec".into());
        let mut ran = false;
        assert!(play_guarded(&ctx, &reserved, &lim, Want::Keep, false, || {
            ran = true;
            Ok(Duration::ZERO)
        })
        .is_err());
        assert!(!ran, "nothing sounds on a reserved device");
    }

    // Tests [@ANCHOR: bench_mcp_audio_tools_levels_get]
    #[test]
    fn levels_are_listed_for_free_devices_and_filtered_by_direction() {
        let devs = devbox_devices();
        let table: Vec<(u32, f64, bool)> =
            devs.iter().map(|d| (d.id, 0.4, d.id % 2 == 0)).collect();
        let run = Scripted::new(&table);
        let cfg = Config::default();
        let ctx = Ctx {
            run: &run,
            cfg: &cfg,
            devs: &devs,
            minute: 720,
            discovery: None,
        };
        let all = levels_get(&ctx, &json!({})).unwrap();
        let free = devs.iter().filter(|d| d.reserved.is_none()).count();
        assert_eq!(all["levels"].as_array().unwrap().len(), free);
        let outs = levels_get(&ctx, &json!({"direction": "output"})).unwrap();
        for l in outs["levels"].as_array().unwrap() {
            assert_eq!(l["direction"], "output");
            assert_eq!(l["volume"], 0.4);
            assert_eq!(l["volume_cap"], 0.5);
            assert_eq!(l["gain_db"], round1(60.0 * 0.4f64.log10()));
        }
        let ins = levels_get(&ctx, &json!({"direction": "input"})).unwrap();
        assert!(ins["levels"]
            .as_array()
            .unwrap()
            .iter()
            .all(|l| l["direction"] == "input" && l["volume_cap"].is_null()));
        let one = levels_get(&ctx, &json!({"device": "Speaker"})).unwrap();
        assert_eq!(one["levels"].as_array().unwrap().len(), 1);
        assert!(levels_get(&ctx, &json!({"direction": "sideways"})).is_err());
    }
}

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
}

pub const NOTE_HEARS_ROOM: &str = "The input hears the room and a recording can contain speech. It is kept only in the managed capture directory (retention applies) and no tool uploads it.";

fn num(args: &Value, k: &str) -> Option<f64> {
    args.get(k).and_then(|v| v.as_f64())
}

fn flag(args: &Value, k: &str) -> bool {
    args.get(k).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn bounded(args: &Value, k: &str, default: f64, lo: f64, hi: f64) -> Result<f64, String> {
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

fn stats_json(s: &analysis::Stats) -> Value {
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
        let (mut guard, applied) =
            VolumeGuard::apply(ctx.run, dev, &lim, want, flag(args, "unmute"))?;
        let played = Playing::start(&dev.node_name, &src.path, 1.0)
            .and_then(|p| p.wait(Duration::from_secs_f64(src.seconds + 5.0)));
        let restored = guard.restore();
        let took = played?;
        restored?;
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

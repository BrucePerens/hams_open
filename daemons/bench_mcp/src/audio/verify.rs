// SPDX-License-Identifier: AGPL-3.0-or-later
//! "Did the device under control make a sound?" and "what sounds happened?": the tools `bench_audio_verify` and
//! `bench_audio_events`. Verification measures the noise floor first, then runs an action (a configured command or a delay for
//! another tool) while recording, and judges the recording against explicit checks. The answer is the evidence and the
//! reasons, never a bare boolean.

use super::analysis::{dominant, median_db, peaks, similarity, spectrum, stats};
use super::devices::Dir;
use super::engine::Recorder;
use super::events::{decode_morse, detect, frame_levels, percentile, Event, Params};
use super::heard::{round1, round3};
use super::tools::{pick, Ctx, NOTE_HEARS_ROOM};
use super::{store, wav};
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn num(args: &Value, k: &str) -> Option<f64> {
    args.get(k).and_then(|v| v.as_f64())
}

fn bounded(args: &Value, k: &str, default: f64, lo: f64, hi: f64) -> Result<f64, String> {
    let v = num(args, k).unwrap_or(default);
    if !v.is_finite() || !(lo..=hi).contains(&v) {
        return Err(format!("{k} {v} is outside {lo} to {hi}"));
    }
    Ok(v)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

impl Check {
    fn json(&self) -> Value {
        json!({"check": self.name, "ok": self.ok, "detail": self.detail})
    }
}

/// What the caller expects of the sound.
#[derive(Debug, Clone, Default)]
pub struct Expect {
    pub min_over_noise_db: f64,
    pub min_peak_dbfs: Option<f64>,
    pub hz: Option<f64>,
    pub tolerance_hz: f64,
    pub hz_min: Option<f64>,
    pub hz_max: Option<f64>,
    pub min_duration_s: Option<f64>,
    pub max_duration_s: Option<f64>,
    pub beeps: Option<usize>,
    pub min_beeps: Option<usize>,
    pub silence: bool,
    pub min_similarity: f64,
}

pub fn expectations(args: &Value) -> Result<Expect, String> {
    let hz = num(args, "expect_hz");
    let tol = match (num(args, "tolerance_hz"), hz) {
        (Some(t), _) => t,
        (None, Some(h)) => h * 0.05,
        (None, None) => 0.0,
    };
    let beeps = args
        .get("expect_beeps")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let min_beeps = args
        .get("min_beeps")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    Ok(Expect {
        min_over_noise_db: bounded(args, "min_over_noise_db", 10.0, 3.0, 80.0)?,
        min_peak_dbfs: num(args, "min_peak_dbfs"),
        hz,
        tolerance_hz: tol,
        hz_min: num(args, "freq_min_hz"),
        hz_max: num(args, "freq_max_hz"),
        min_duration_s: num(args, "min_duration_s"),
        max_duration_s: num(args, "max_duration_s"),
        beeps,
        min_beeps,
        silence: args
            .get("expect_silence")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        min_similarity: bounded(args, "min_similarity", 0.8, 0.0, 1.0)?,
    })
}

/// The evidence the judgment is made from.
pub struct Evidence<'a> {
    pub input_all_zero: bool,
    pub noise_dbfs: f64,
    pub threshold_dbfs: f64,
    pub events: &'a [Event],
    pub dominant_hz: Option<f64>,
    pub similarity: Option<f64>,
}

/// Judges the evidence against the expectations: one check for each thing asked, and a summary reason.
pub fn judge(e: &Expect, ev: &Evidence) -> (bool, Vec<Check>, String) {
    // [@ANCHOR: audio_verify_judge]
    let mut checks = Vec::new();
    if ev.input_all_zero {
        checks.push(Check {
            name: "input_alive",
            ok: false,
            detail: "the input recorded exact digital zeros: it is muted, disconnected or dead, so nothing can be concluded".into(),
        });
        return (false, checks, "the input is dead (exact zeros)".into());
    }
    let n = ev.events.len();
    if e.silence {
        let ok = n == 0;
        checks.push(Check {
            name: "silence",
            ok,
            detail: if ok {
                format!(
                    "no sound over {:.1} dB above the noise floor ({:.1} dBFS)",
                    e.min_over_noise_db, ev.noise_dbfs
                )
            } else {
                format!(
                    "{n} sound event(s), the first at {:.2} s",
                    ev.events[0].onset_s
                )
            },
        });
    } else {
        checks.push(Check {
            name: "sound_present",
            ok: n > 0,
            detail: if n > 0 {
                let loudest = ev.events.iter().map(|x| x.peak_dbfs).fold(f64::MIN, f64::max);
                format!(
                    "{n} event(s) over the threshold {:.1} dBFS ({:.1} dB over the noise floor {:.1} dBFS); loudest peak {:.1} dBFS",
                    ev.threshold_dbfs, e.min_over_noise_db, ev.noise_dbfs, loudest
                )
            } else {
                format!(
                    "nothing rose above {:.1} dBFS ({:.1} dB over the noise floor {:.1} dBFS)",
                    ev.threshold_dbfs, e.min_over_noise_db, ev.noise_dbfs
                )
            },
        });
    }
    if n > 0 && !e.silence {
        if let Some(min) = e.min_peak_dbfs {
            let peak = ev
                .events
                .iter()
                .map(|x| x.peak_dbfs)
                .fold(f64::MIN, f64::max);
            checks.push(Check {
                name: "level",
                ok: peak >= min,
                detail: format!("loudest peak {peak:.1} dBFS, wanted at least {min:.1}"),
            });
        }
        if let Some(h) = e.hz {
            match ev.dominant_hz {
                Some(d) => checks.push(Check {
                    name: "frequency",
                    ok: (d - h).abs() <= e.tolerance_hz,
                    detail: format!(
                        "dominant frequency {d:.1} Hz, wanted {h:.1} Hz within {:.1} Hz",
                        e.tolerance_hz
                    ),
                }),
                None => checks.push(Check {
                    name: "frequency",
                    ok: false,
                    detail: format!("no clear tone (broadband sound), wanted {h:.1} Hz"),
                }),
            }
        }
        if e.hz_min.is_some() || e.hz_max.is_some() {
            let (lo, hi) = (e.hz_min.unwrap_or(0.0), e.hz_max.unwrap_or(f64::MAX));
            match ev.dominant_hz {
                Some(d) => checks.push(Check {
                    name: "frequency_range",
                    ok: d >= lo && d <= hi,
                    detail: format!("dominant frequency {d:.1} Hz, wanted {lo:.0} to {hi:.0} Hz"),
                }),
                None => checks.push(Check {
                    name: "frequency_range",
                    ok: false,
                    detail: "no clear tone (broadband sound)".into(),
                }),
            }
        }
        let longest = ev.events.iter().map(|x| x.duration_s()).fold(0.0, f64::max);
        if let Some(min) = e.min_duration_s {
            checks.push(Check {
                name: "min_duration",
                ok: longest >= min,
                detail: format!("longest event {longest:.3} s, wanted at least {min:.3} s"),
            });
        }
        if let Some(max) = e.max_duration_s {
            checks.push(Check {
                name: "max_duration",
                ok: longest <= max,
                detail: format!("longest event {longest:.3} s, wanted at most {max:.3} s"),
            });
        }
    }
    if let Some(want) = e.beeps {
        checks.push(Check {
            name: "beep_count",
            ok: n == want,
            detail: format!("{n} beep(s), wanted exactly {want}"),
        });
    }
    if let Some(min) = e.min_beeps {
        checks.push(Check {
            name: "min_beeps",
            ok: n >= min,
            detail: format!("{n} beep(s), wanted at least {min}"),
        });
    }
    if let Some(sim) = ev.similarity {
        checks.push(Check {
            name: "reference_similarity",
            ok: sim >= e.min_similarity,
            detail: format!(
                "spectral similarity to the reference {sim:.2}, wanted at least {:.2}",
                e.min_similarity
            ),
        });
    }
    let failed: Vec<&Check> = checks.iter().filter(|c| !c.ok).collect();
    let ok = failed.is_empty();
    let reason = if ok {
        checks
            .iter()
            .map(|c| c.detail.clone())
            .collect::<Vec<_>>()
            .join("; ")
    } else {
        failed
            .iter()
            .map(|c| format!("{}: {}", c.name, c.detail))
            .collect::<Vec<_>>()
            .join("; ")
    };
    (ok, checks, reason)
}

/// The samples inside the events, joined: what the spectrum is taken of.
fn active_samples(s: &[f32], rate: u32, events: &[Event]) -> Vec<f32> {
    let r = f64::from(rate);
    events
        .iter()
        .flat_map(|e| {
            let a = (e.onset_s * r) as usize;
            let b = ((e.offset_s * r) as usize).min(s.len());
            s[a.min(b)..b].iter().copied()
        })
        .collect()
}

/// Runs a configured action and waits for it; the result says how it ended.
fn run_action(argv: &[String], timeout: Duration) -> Value {
    let started = Instant::now();
    let child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return json!({"ran": false, "error": format!("{}: {e}", argv[0])}),
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return json!({"ran": true, "exit": status.code(), "took_s": round3(started.elapsed().as_secs_f64())});
            }
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return json!({"ran": true, "timed_out": true, "took_s": round3(started.elapsed().as_secs_f64())});
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return json!({"ran": true, "error": e.to_string()}),
        }
    }
}

pub fn verify(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: audio_verify_tool]
    let dev = pick(ctx, args, "input", Dir::Input)?;
    let expect = expectations(args)?;
    let calibrate_s = bounded(args, "calibrate_s", 1.5, 0.5, 10.0)?;
    let max_window = (ctx.cfg.max_record_s - calibrate_s).max(1.0);
    let window_s = bounded(args, "window_s", 5.0_f64.min(max_window), 0.5, max_window)?;
    let delay_s = bounded(args, "delay_s", 0.0, 0.0, window_s)?;
    let action = match args.get("action").and_then(|a| a.as_str()) {
        Some(name) => Some((
            name,
            ctx.cfg.verify_actions.get(name).ok_or_else(|| {
                format!(
                    "no verify action named {name:?}; the config's verify_actions are: {}",
                    ctx.cfg
                        .verify_actions
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?,
        )),
        None => None,
    };
    let dir = ctx.cfg.capture_dir();
    let reference = match args.get("reference").and_then(|r| r.as_str()) {
        Some(name) => Some(wav::read(&store::managed_file(&dir, name)?)?),
        None => None,
    };
    let rate = match &reference {
        Some(r) => r.rate,
        None => bounded(args, "sample_rate_hz", 48000.0, 8000.0, 96000.0)? as u32,
    };
    let r = f64::from(rate);
    let mut rec = Recorder::start(&dev.node_name, rate, false)?;
    let cal_n = (calibrate_s * r) as usize;
    let total_n = cal_n + (window_s * r) as usize;
    rec.wait_samples(cal_n, Duration::from_secs_f64(calibrate_s + 5.0))?;
    let action_at = rec.len();
    if delay_s > 0.0 {
        std::thread::sleep(Duration::from_secs_f64(delay_s));
    }
    let action_result = action.map(|(name, argv)| {
        let timeout = Duration::from_secs_f64(window_s.max(1.0) + 5.0);
        let mut v = run_action(argv, timeout);
        v["action"] = json!(name);
        v["started_s_after_calibration"] =
            json!(round3(rec.len().saturating_sub(action_at) as f64 / r));
        v
    });
    rec.wait_samples(total_n, Duration::from_secs_f64(window_s + 10.0))?;
    let mut all = rec.stop();
    all.truncate(total_n);
    // The first 0.1 s of a stream can hold a start-up transient; the noise floor skips it.
    let skip = ((0.1 * r) as usize).min(cal_n / 2);
    let cal = &all[skip..action_at.min(all.len())];
    let win = &all[action_at.min(all.len())..];
    let input_all_zero = all.iter().all(|x| *x == 0.0);
    let cal_levels = frame_levels(cal, rate, 5.0);
    let noise = percentile(&cal_levels, 0.9);
    let params = Params {
        margin_db: expect.min_over_noise_db,
        noise_dbfs: Some(noise),
        ..Params::default()
    };
    let det = detect(win, rate, &params);
    let active = active_samples(win, rate, &det.events);
    let sp = if active.len() >= 256 {
        spectrum(
            &active,
            rate,
            4096.min(active.len().next_power_of_two()).max(256),
        )
        .ok()
    } else {
        None
    };
    let nyq = r / 2.0;
    let dominant_hz = sp
        .as_ref()
        .and_then(|s| dominant(s, 100.0, nyq * 0.9, 15.0))
        .map(|p| p.hz);
    let similarity_value = match (&reference, &sp) {
        (Some(refw), Some(mine)) => {
            let rs = refw.mono();
            let rdet = detect(
                &rs,
                rate,
                &Params {
                    margin_db: expect.min_over_noise_db,
                    ..Params::default()
                },
            );
            let ra = if rdet.events.is_empty() {
                rs.clone()
            } else {
                active_samples(&rs, rate, &rdet.events)
            };
            let rsp = spectrum(&ra, rate, 4096.min(ra.len().next_power_of_two()).max(256))
                .map_err(|e| format!("reference: {e}"))?;
            if (rsp.bin_hz - mine.bin_hz).abs() > 1e-9 {
                Some(0.0)
            } else {
                Some(similarity(mine, &rsp, 100.0, nyq * 0.9))
            }
        }
        (Some(_), None) => Some(0.0),
        _ => None,
    };
    let ev = Evidence {
        input_all_zero,
        noise_dbfs: noise,
        threshold_dbfs: det.threshold_dbfs,
        events: &det.events,
        dominant_hz,
        similarity: similarity_value,
    };
    let (pass, checks, reason) = judge(&expect, &ev);
    let mut out = json!({
        "input": dev.node_name, "alias": dev.alias, "pass": pass, "reason": reason,
        "checks": checks.iter().map(Check::json).collect::<Vec<_>>(),
        "evidence": {
            "noise_floor_dbfs": round1(noise), "threshold_dbfs": round1(det.threshold_dbfs),
            "calibration": super::tools::stats_json(&stats(cal, rate, -60.0)),
            "window": super::tools::stats_json(&stats(win, rate, -60.0)),
            "events": det.events.iter().map(Event::json).collect::<Vec<_>>(),
            "dominant_hz": dominant_hz.map(round1),
            "window_spectrum_median_dbfs": spectrum(win, rate, 4096).ok().map(|w| round1(median_db(&w, 100.0, nyq * 0.9))),
            "spectrum_peaks": sp.as_ref().map(|s| peaks(s, 100.0, nyq * 0.9, 5, 15.0).iter().map(|p| json!({"hz": round1(p.hz), "dbfs": round1(p.dbfs)})).collect::<Vec<_>>()),
            "reference_similarity": similarity_value.map(|x| (x * 100.0).round() / 100.0),
        },
        "action": action_result, "delay_s": delay_s, "note": NOTE_HEARS_ROOM,
    });
    let label = args
        .get("label")
        .and_then(|l| l.as_str())
        .or(dev.alias.as_deref());
    if let Some(name) = args.get("save_reference").and_then(|n| n.as_str()) {
        let path = dir.join(format!("ref-{}.wav", store::slug(name)));
        store::new_path(&dir, "ref", None)?; // creates the directory with owner-only permissions
        wav::write(
            &path,
            &wav::Wav {
                rate,
                channels: 1,
                samples: win.to_vec(),
            },
        )?;
        out["saved_reference"] = json!(path);
    } else {
        let path = store::new_path(&dir, "verify", label)?;
        wav::write(
            &path,
            &wav::Wav {
                rate,
                channels: 1,
                samples: all.clone(),
            },
        )?;
        store::prune(&dir, ctx.cfg.retention_files, ctx.cfg.retention_days);
        out["file"] = json!(path);
    }
    Ok(out)
}

/// `bench_audio_events`: listen for N seconds and report the sound events.
pub fn events(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: audio_events_tool]
    let dev = pick(ctx, args, "input", Dir::Input)?;
    let seconds = bounded(args, "seconds", 5.0, 0.5, ctx.cfg.max_record_s)?;
    let rate = bounded(args, "sample_rate_hz", 48000.0, 8000.0, 96000.0)? as u32;
    let p = Params {
        margin_db: bounded(args, "min_over_noise_db", 10.0, 3.0, 80.0)?,
        threshold_dbfs: num(args, "threshold_dbfs"),
        merge_gap_ms: bounded(args, "merge_gap_ms", 40.0, 0.0, 2000.0)?,
        min_event_ms: bounded(args, "min_event_ms", 5.0, 1.0, 5000.0)?,
        ..Params::default()
    };
    let mut rec = Recorder::start(&dev.node_name, rate, false)?;
    rec.wait_samples(
        (seconds * f64::from(rate)) as usize,
        Duration::from_secs_f64(seconds + 5.0),
    )?;
    let mut s = rec.stop();
    s.truncate((seconds * f64::from(rate)) as usize);
    let skip = ((0.1 * f64::from(rate)) as usize).min(s.len());
    let body = &s[skip..];
    let det = detect(body, rate, &p);
    let off = skip as f64 / f64::from(rate);
    let list: Vec<Event> = det
        .events
        .iter()
        .map(|e| Event {
            onset_s: e.onset_s + off,
            offset_s: e.offset_s + off,
            ..e.clone()
        })
        .collect();
    let mut out = json!({
        "input": dev.node_name, "seconds": seconds, "event_count": list.len(),
        "noise_floor_dbfs": round1(det.noise_dbfs), "threshold_dbfs": round1(det.threshold_dbfs),
        "events": list.iter().map(Event::json).collect::<Vec<_>>(),
        "levels": super::tools::stats_json(&stats(&s, rate, -60.0)), "note": NOTE_HEARS_ROOM,
    });
    if list.len() >= 2 {
        let onsets: Vec<f64> = list
            .windows(2)
            .map(|w| w[1].onset_s - w[0].onset_s)
            .collect();
        out["mean_interval_s"] = json!(round3(onsets.iter().sum::<f64>() / onsets.len() as f64));
    }
    if args.get("decode_morse").and_then(|v| v.as_bool()) == Some(true) {
        out["morse"] = match decode_morse(&list) {
            Some(m) => {
                json!({"text": m.text, "symbols": m.symbols, "dot_s": round3(m.dot_s), "wpm": round1(m.wpm)})
            }
            None => json!(null),
        };
    }
    if stats(&s, rate, -60.0).all_zero {
        out["warning"] =
            json!("every sample is exactly zero: the input is muted, disconnected or dead");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(on: f64, off: f64, peak: f64, hz: Option<f64>) -> Event {
        Event {
            onset_s: on,
            offset_s: off,
            peak_dbfs: peak,
            rms_dbfs: peak - 3.0,
            hz,
        }
    }

    fn evidence<'a>(events: &'a [Event], hz: Option<f64>) -> Evidence<'a> {
        Evidence {
            input_all_zero: false,
            noise_dbfs: -60.0,
            threshold_dbfs: -50.0,
            events,
            dominant_hz: hz,
            similarity: None,
        }
    }

    fn expect(args: Value) -> Expect {
        expectations(&args).unwrap()
    }

    // Tests [@ANCHOR: audio_verify_judge]
    #[test]
    fn a_beep_that_matches_every_expectation_passes_with_reasons() {
        let events = [ev(0.5, 0.7, -20.0, Some(1800.0))];
        let e = expect(
            json!({"expect_hz": 1800, "tolerance_hz": 50, "min_duration_s": 0.1, "max_duration_s": 0.5, "expect_beeps": 1, "min_peak_dbfs": -30}),
        );
        let (ok, checks, reason) = judge(&e, &evidence(&events, Some(1805.0)));
        assert!(ok, "{reason}");
        assert!(checks.len() >= 6 && checks.iter().all(|c| c.ok));
        assert!(reason.contains("1805.0 Hz"));
    }

    #[test]
    fn nothing_heard_fails_and_says_how_far_over_the_floor_it_looked() {
        let (ok, _, reason) = judge(&expect(json!({})), &evidence(&[], None));
        assert!(!ok);
        assert!(
            reason.contains("nothing rose above -50.0 dBFS") && reason.contains("-60.0 dBFS"),
            "{reason}"
        );
    }

    #[test]
    fn the_wrong_pitch_count_or_duration_each_fail_with_their_own_reason() {
        let events = [
            ev(0.5, 0.52, -20.0, Some(900.0)),
            ev(1.0, 1.02, -22.0, Some(900.0)),
        ];
        let (ok, checks, reason) = judge(
            &expect(json!({"expect_hz": 1800, "expect_beeps": 3, "min_duration_s": 0.1})),
            &evidence(&events, Some(900.0)),
        );
        assert!(!ok);
        let failed: Vec<&str> = checks.iter().filter(|c| !c.ok).map(|c| c.name).collect();
        assert_eq!(failed, vec!["frequency", "min_duration", "beep_count"]);
        assert!(reason.contains("2 beep(s), wanted exactly 3"));
        let (ok, _, reason) = judge(
            &expect(json!({"freq_min_hz": 1000, "freq_max_hz": 2000})),
            &evidence(&events, Some(900.0)),
        );
        assert!(!ok && reason.contains("frequency_range"), "{reason}");
        let (ok, _, reason) = judge(&expect(json!({"expect_hz": 900})), &evidence(&events, None));
        assert!(!ok && reason.contains("broadband"), "{reason}");
    }

    #[test]
    fn a_dead_input_cannot_pass_and_silence_can_be_the_expectation() {
        let mut dead = evidence(&[], None);
        dead.input_all_zero = true;
        let (ok, _, reason) = judge(&expect(json!({"expect_silence": true})), &dead);
        assert!(!ok && reason.contains("dead"), "{reason}");
        let (ok, _, _) = judge(
            &expect(json!({"expect_silence": true})),
            &evidence(&[], None),
        );
        assert!(ok);
        let events = [ev(0.5, 0.7, -20.0, None)];
        let (ok, _, reason) = judge(
            &expect(json!({"expect_silence": true})),
            &evidence(&events, None),
        );
        assert!(!ok && reason.contains("first at 0.50 s"), "{reason}");
    }

    #[test]
    fn reference_similarity_is_a_check() {
        let events = [ev(0.5, 0.7, -20.0, Some(1000.0))];
        let mut e = evidence(&events, Some(1000.0));
        e.similarity = Some(0.55);
        let (ok, _, reason) = judge(&expect(json!({})), &e);
        assert!(
            !ok && reason.contains("0.55") && reason.contains("0.80"),
            "{reason}"
        );
        e.similarity = Some(0.93);
        assert!(judge(&expect(json!({"min_similarity": 0.9})), &e).0);
    }

    #[test]
    fn expectation_arguments_are_validated_and_the_tolerance_defaults_to_five_percent() {
        assert_eq!(expect(json!({"expect_hz": 2000})).tolerance_hz, 100.0);
        assert!(expectations(&json!({"min_over_noise_db": 1})).is_err());
        assert!(expectations(&json!({"min_similarity": 2})).is_err());
    }

    #[test]
    fn a_configured_action_runs_and_reports_how_it_ended() {
        let ok = run_action(&["true".into()], Duration::from_secs(2));
        assert_eq!(
            (ok["ran"].clone(), ok["exit"].clone()),
            (json!(true), json!(0))
        );
        let bad = run_action(&["false".into()], Duration::from_secs(2));
        assert_eq!(bad["exit"], 1);
        let missing = run_action(&["/nonexistent/prog".into()], Duration::from_secs(2));
        assert_eq!(missing["ran"], false);
        let slow = run_action(&["sleep".into(), "5".into()], Duration::from_millis(200));
        assert_eq!(slow["timed_out"], true);
    }
}

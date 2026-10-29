// SPDX-License-Identifier: AGPL-3.0-or-later
//! Alerting the operator: a tone pattern by severity and/or a spoken message on the alert output, inside the volume cap and
//! quiet hours. The operator may be away, so the answer always carries the alert text to send by another channel, whether or not
//! the sound could be made.

use super::devices::Dir;
use super::heard::round3;
use super::safety::{limits, Want};
use super::speak;
use super::tones::{generate, Tone};
use super::tools::{pick, play_guarded, Ctx};
use super::{store, wav};
use serde_json::{json, Value};
use std::time::Duration;

/// The longest total sound one alert may make.
pub const MAX_ALERT_S: f64 = 30.0;
const RATE: u32 = 48000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Severity {
    Info,
    Warning,
    Urgent,
}

impl Severity {
    pub fn parse(s: &str) -> Result<Severity, String> {
        // [@ANCHOR: bench_mcp_audio_alert_severity_parse]
        match s {
            "info" => Ok(Severity::Info),
            "warning" => Ok(Severity::Warning),
            "urgent" => Ok(Severity::Urgent),
            o => Err(format!("severity {o:?} is not info, warning or urgent")),
        }
    }

    pub fn word(self) -> &'static str {
        // [@ANCHOR: bench_mcp_audio_alert_severity_word]
        match self {
            Severity::Info => "INFO",
            Severity::Warning => "WARNING",
            Severity::Urgent => "URGENT",
        }
    }

    /// The share of the volume limit used outside quiet hours.
    pub fn volume_share(self) -> f64 {
        // [@ANCHOR: bench_mcp_audio_alert_severity_volume_share]
        match self {
            Severity::Info => 0.6,
            Severity::Warning => 0.8,
            Severity::Urgent => 1.0,
        }
    }
}

/// The volume for an alert: in quiet hours the lowest volume (the quiet volume, which is the limit), except urgent alerts, which use the cap.
pub fn alert_volume(sev: Severity, limit: f64, quiet: bool) -> f64 {
    // [@ANCHOR: audio_alert_volume]
    if quiet && sev != Severity::Urgent {
        limit
    } else {
        limit * sev.volume_share()
    }
}

/// The tone pattern for a severity: one soft beep, two beeps, or a fast alternating two-tone siren.
pub fn pattern(sev: Severity) -> Vec<f32> {
    // [@ANCHOR: bench_mcp_audio_alert_pattern]
    let beep = |hz: f64, secs: f64| generate(Tone::Sine { hz }, RATE, secs, -12.0);
    let gap = |secs: f64| vec![0.0f32; (secs * f64::from(RATE)) as usize];
    let mut v = Vec::new();
    match sev {
        Severity::Info => v.extend(beep(880.0, 0.25)),
        Severity::Warning => {
            v.extend(beep(880.0, 0.2));
            v.extend(gap(0.12));
            v.extend(beep(880.0, 0.2));
        }
        Severity::Urgent => {
            for _ in 0..3 {
                v.extend(beep(1000.0, 0.15));
                v.extend(beep(1400.0, 0.15));
                v.extend(gap(0.08));
            }
        }
    }
    v
}

pub fn alert(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: audio_alert_tool]
    let sev = Severity::parse(
        args.get("severity")
            .and_then(|s| s.as_str())
            .unwrap_or("info"),
    )?;
    let message = args
        .get("message")
        .and_then(|m| m.as_str())
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());
    let want_tone = args.get("tone").and_then(|v| v.as_bool()).unwrap_or(true);
    let want_speech = args
        .get("speak")
        .and_then(|v| v.as_bool())
        .unwrap_or(message.is_some());
    if want_speech && message.is_none() {
        return Err("speak needs a message".into());
    }
    if !want_tone && !want_speech {
        return Err("nothing to do: tone and speak are both off".into());
    }
    let dry = args.get("dry_run").and_then(|v| v.as_bool()) == Some(true);
    let repeat = args.get("repeat").and_then(|v| v.as_u64()).unwrap_or(1);
    if !(1..=5).contains(&repeat) {
        return Err("repeat must be 1 to 5".into());
    }
    let text = match &message {
        Some(m) => format!("[{}] {m}", sev.word()),
        None => format!("[{}] alert", sev.word()),
    };
    // From here on a failure still returns the text, because the operator may be away.
    let sound = (|| -> Result<Value, String> {
        let dev = pick(ctx, args, "output", Dir::Output)?;
        let lim = limits(ctx.cfg, dev, sev == Severity::Urgent, ctx.minute);
        let volume = alert_volume(sev, lim.volume, lim.quiet);
        let req = if want_speech {
            let mut a = args.clone();
            a["text"] = json!(message.clone().unwrap_or_default());
            Some(speak::request(ctx, &a, dev)?)
        } else {
            None
        };
        if dry {
            let would = json!({"severity": sev.word(), "repeat": repeat, "tone": want_tone, "spoken": want_speech,
                "engine": req.as_ref().map(|r| r.engine.name()), "longest_total_s": MAX_ALERT_S});
            return super::tools::dry_plan(ctx, dev, &lim, Want::Level(volume), would);
        }
        let dir = ctx.cfg.capture_dir();
        let tone_path = if want_tone {
            let p = store::new_path(&dir, "tone", Some("alert"))?;
            wav::write(
                &p,
                &wav::Wav {
                    rate: RATE,
                    channels: 1,
                    samples: pattern(sev),
                },
            )?;
            Some(p)
        } else {
            None
        };
        let tone_s = if want_tone {
            pattern(sev).len() as f64 / f64::from(RATE)
        } else {
            0.0
        };
        let result = (|| {
            let mut spoken_s = 0.0;
            let (applied, took) = play_guarded(ctx, dev, &lim, Want::Level(volume), true, || {
                let begun = std::time::Instant::now();
                for i in 0..repeat {
                    if let Some(p) = &tone_path {
                        super::engine::Playing::start(&dev.node_name, p, 1.0)?
                            .wait(Duration::from_secs_f64(tone_s + 5.0))?;
                    }
                    if let Some(r) = &req {
                        spoken_s = speak::say(ctx, dev, r)?;
                    }
                    if begun.elapsed().as_secs_f64() > MAX_ALERT_S {
                        return Err(format!(
                            "the alert ran over {MAX_ALERT_S:.0} s and was stopped after repeat {}",
                            i + 1
                        ));
                    }
                    if i + 1 < repeat {
                        std::thread::sleep(Duration::from_millis(500));
                    }
                }
                Ok(begun.elapsed())
            })?;
            Ok(json!({
                "output": dev.node_name, "alias": dev.alias, "severity": sev.word(), "repeat": repeat,
                "tone": want_tone, "spoken": want_speech, "engine": req.as_ref().map(|r| r.engine.name()),
                "volume_used": applied.volume, "volume_limit": lim.volume, "quiet_hours": lim.quiet,
                "seconds": round3(took.as_secs_f64()), "speech_seconds": round3(spoken_s),
                "volume_restored": applied.before.volume, "mute_restored": applied.before.muted,
            }))
        })();
        if let Some(p) = tone_path {
            let _ = std::fs::remove_file(p);
        }
        result
    })();
    if dry {
        return Ok(match sound {
            Ok(mut v) => {
                v["text_to_send"] = json!(text);
                v
            }
            Err(e) => json!({"dry_run": true, "would_fail": e, "text_to_send": text}),
        });
    }
    Ok(match sound {
        Ok(v) => json!({"text_to_send": text, "audible": true, "sound": v,
            "note": "The operator may be away: send text_to_send by another channel as well if the alert matters."}),
        Err(e) => json!({"text_to_send": text, "audible": false, "error": e,
            "note": "No sound was made. The operator may be away: send text_to_send by another channel."}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: audio_alert_volume]
    // Tests [@ANCHOR: bench_mcp_audio_alert_severity_volume_share]
    #[test]
    fn quiet_hours_give_the_lowest_volume_except_for_urgent() {
        // Outside quiet hours the limit is the cap (0.5); inside, it is the quiet volume (0.15) unless urgent (cap).
        assert_eq!(alert_volume(Severity::Info, 0.5, false), 0.3);
        assert_eq!(alert_volume(Severity::Warning, 0.5, false), 0.4);
        assert_eq!(alert_volume(Severity::Urgent, 0.5, false), 0.5);
        assert_eq!(alert_volume(Severity::Info, 0.15, true), 0.15);
        assert_eq!(alert_volume(Severity::Warning, 0.15, true), 0.15);
        assert_eq!(alert_volume(Severity::Urgent, 0.5, true), 0.5);
    }

    // Tests [@ANCHOR: bench_mcp_audio_alert_severity_parse]
    // Tests [@ANCHOR: bench_mcp_audio_alert_severity_word]
    // Tests [@ANCHOR: bench_mcp_audio_alert_pattern]
    #[test]
    fn the_patterns_grow_with_severity_and_stay_short() {
        let secs = |s| pattern(s).len() as f64 / f64::from(RATE);
        assert!(secs(Severity::Info) < secs(Severity::Warning));
        assert!(secs(Severity::Warning) < secs(Severity::Urgent));
        assert!(secs(Severity::Urgent) < 2.0);
        assert!(pattern(Severity::Urgent).iter().all(|x| x.abs() < 0.26));
        assert!(Severity::parse("panic").is_err());
        assert_eq!(Severity::parse("urgent").unwrap().word(), "URGENT");
    }

    // Tests [@ANCHOR: audio_alert_tool]
    #[test]
    fn a_dry_run_of_an_alert_returns_the_text_and_the_plan_and_makes_no_sound() {
        use crate::audio::testkit::{devbox_devices, scratch, Scripted};
        let devs = devbox_devices();
        let spk = devs
            .iter()
            .find(|d| d.description.ends_with("Speaker"))
            .unwrap();
        let d = scratch("dry-alert");
        let cfg = crate::audio::Config {
            capture_dir: Some(d.clone()),
            ..crate::audio::Config::default()
        };
        let run = Scripted::new(&[(spk.id, 0.9, true)]);
        let ctx = Ctx {
            run: &run,
            cfg: &cfg,
            devs: &devs,
            minute: 720,
            discovery: None,
        };
        let v = alert(&ctx, &json!({"message": "rotor stuck", "severity": "warning", "tone": true, "output": "Speaker", "dry_run": true})).unwrap();
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["text_to_send"], "[WARNING] rotor stuck");
        assert_eq!(v["would"]["severity"], "WARNING");
        assert!((v["volume_would_be"].as_f64().unwrap() - 0.4).abs() < 1e-9);
        assert!(!run
            .calls
            .borrow()
            .iter()
            .any(|c| c.starts_with("wpctl set-")));
        assert!(std::fs::read_dir(&d).unwrap().next().is_none());
        // No output named and none configured: still no sound and the text comes back.
        let v = alert(&ctx, &json!({"message": "x", "dry_run": true})).unwrap();
        assert!(v["would_fail"].as_str().unwrap().contains("never used"));
        assert_eq!(v["text_to_send"], "[INFO] x");
        let _ = std::fs::remove_dir_all(&d);
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! Speech: which synthesizers are installed (speech-dispatcher, espeak-ng, Piper), their voices, and speaking text through a
//! chosen output or into a WAV file. Nothing is installed by these tools; a missing synthesizer is reported with what to install.
//!
//! espeak-ng and Piper write a WAV file that is played on the chosen node, so the output is selectable and the volume guard
//! applies. speech-dispatcher plays through its own audio path (the system default sink), so it is used only when the chosen
//! output is the system default and not reserved; `bench_audio_speak` with an explicit other output uses espeak-ng or Piper.

use super::devices::Device;
use super::engine::Playing;
use super::heard::round3;
use super::tools::{play_guarded, Ctx};
use super::{store, wav, Config};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MAX_TEXT: usize = 600;
pub const MAX_SPEECH_S: f64 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Engine {
    SpeechDispatcher,
    EspeakNg,
    Piper,
}

impl Engine {
    pub fn name(self) -> &'static str {
        // [@ANCHOR: bench_mcp_audio_speak_engine_name]
        match self {
            Engine::SpeechDispatcher => "speech-dispatcher",
            Engine::EspeakNg => "espeak-ng",
            Engine::Piper => "piper",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub espeak_ng: bool,
    pub spd_say: bool,
    /// The speech-dispatcher server socket exists, so a client does not have to start a server.
    pub speechd_running: bool,
    pub piper: bool,
    pub piper_model: Option<String>,
}

pub fn detect(cfg: &Config, runtime_dir: Option<&Path>) -> Found {
    Found {
        espeak_ng: super::on_path("espeak-ng").is_some(),
        spd_say: super::on_path("spd-say").is_some(),
        speechd_running: runtime_dir
            .is_some_and(|d| d.join("speech-dispatcher/speechd.sock").exists()),
        piper: super::on_path("piper").is_some(),
        piper_model: cfg.piper_model.clone(),
    }
}

/// The engine to use. `want` is `auto` or a name. `target` is the output the sound goes to.
pub fn choose(want: &str, f: &Found, target: &Device, cfg_default: &str) -> Result<Engine, String> {
    // [@ANCHOR: audio_speak_choose]
    let want = if want == "auto" { cfg_default } else { want };
    let missing = |what: &str, hint: &str| Err(format!("{what} is not available: {hint}"));
    match want {
        "speech-dispatcher" => {
            if !f.spd_say {
                return missing(
                    "speech-dispatcher",
                    "install the speech-dispatcher package (spd-say)",
                );
            }
            if !target.is_default || target.reserved.is_some() {
                return Err(format!(
                    "speech-dispatcher plays through the system default output only, and {} is not the usable default; use engine espeak-ng or piper for a chosen output",
                    target.node_name
                ));
            }
            Ok(Engine::SpeechDispatcher)
        }
        "espeak-ng" if f.espeak_ng => Ok(Engine::EspeakNg),
        "espeak-ng" => missing("espeak-ng", "install the espeak-ng package"),
        "piper" if f.piper && f.piper_model.is_some() => Ok(Engine::Piper),
        "piper" => missing(
            "piper",
            "install piper and set piper_model in the audio config",
        ),
        "auto" => {
            if f.spd_say && f.speechd_running && target.is_default && target.reserved.is_none() {
                Ok(Engine::SpeechDispatcher)
            } else if f.espeak_ng {
                Ok(Engine::EspeakNg)
            } else if f.piper && f.piper_model.is_some() {
                Ok(Engine::Piper)
            } else {
                Err("no speech synthesizer is installed: install espeak-ng (apt install espeak-ng); nothing was installed by this tool".into())
            }
        }
        o => Err(format!(
            "engine {o:?} is not auto, speech-dispatcher, espeak-ng or piper"
        )),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    pub language: String,
    pub name: String,
    pub gender: String,
}

/// Parses `espeak-ng --voices` output.
pub fn parse_voices(text: &str) -> Vec<Voice> {
    // [@ANCHOR: audio_speak_voices]
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() >= 4).then(|| Voice {
                language: f[1].to_string(),
                gender: f[2].to_string(),
                name: f[3].to_string(),
            })
        })
        .collect()
}

/// A voice or language code is passed as a command argument, so it is limited to the characters such codes use.
pub fn safe_code(s: &str) -> Result<&str, String> {
    // [@ANCHOR: bench_mcp_audio_speak_safe_code]
    let ok = !s.is_empty()
        && s.len() <= 40
        && !s.starts_with('-')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_+-".contains(&b));
    if ok {
        Ok(s)
    } else {
        Err(format!(
            "{s:?} is not a voice or language code (letters, digits, _ + - only)"
        ))
    }
}

pub fn espeak_args(voice: &str, rate_wpm: u32, out: &Path) -> Vec<String> {
    vec![
        "--stdin".into(),
        "-v".into(),
        voice.into(),
        "-s".into(),
        rate_wpm.to_string(),
        "-w".into(),
        out.display().to_string(),
    ]
}

pub fn piper_args(model: &str, out: &Path) -> Vec<String> {
    vec![
        "--model".into(),
        model.into(),
        "--output_file".into(),
        out.display().to_string(),
    ]
}

/// speech-dispatcher's rate is -100 to 100 around its default; espeak's words per minute map onto it around 160.
pub fn spd_rate(wpm: u32) -> i32 {
    (((f64::from(wpm) - 160.0) / 1.4).round() as i32).clamp(-100, 100)
}

pub fn spd_args(lang: &str, voice: Option<&str>, rate_wpm: u32) -> Vec<String> {
    // [@ANCHOR: bench_mcp_audio_speak_spd_args]
    let mut a: Vec<String> = vec![
        "--wait".into(),
        "-e".into(),
        "-l".into(),
        lang.into(),
        "-r".into(),
        spd_rate(rate_wpm).to_string(),
    ];
    if let Some(v) = voice {
        a.push("-t".into());
        a.push(v.into());
    }
    a
}

fn run_with_stdin(
    program: &str,
    args: &[String],
    input: &str,
    limit: Duration,
) -> Result<(), String> {
    // [@ANCHOR: bench_mcp_audio_speak_run_with_stdin]
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("no stdin")?
        .write_all(input.as_bytes())
        .map_err(|e| format!("{program}: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait().map_err(|e| format!("{program}: {e}"))? {
            Some(st) if st.success() => return Ok(()),
            Some(st) => {
                let mut msg = String::new();
                if let Some(mut e) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut e, &mut msg);
                }
                return Err(format!("{program} failed ({st}): {}", msg.trim()));
            }
            None if start.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{program} did not finish within {:.0} s",
                    limit.as_secs_f64()
                ));
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

/// What a speaking request asks for, after validation.
pub struct Request {
    pub text: String,
    pub engine: Engine,
    pub voice: String,
    pub language: String,
    pub rate_wpm: u32,
}

pub fn request(ctx: &Ctx, args: &Value, target: &Device) -> Result<Request, String> {
    // [@ANCHOR: bench_mcp_audio_speak_request]
    let text = args
        .get("text")
        .and_then(|t| t.as_str())
        .ok_or("text is required")?
        .trim()
        .to_string();
    if text.is_empty() || text.chars().count() > MAX_TEXT {
        return Err(format!("text must be 1 to {MAX_TEXT} characters"));
    }
    let language = args
        .get("language")
        .and_then(|v| v.as_str())
        .unwrap_or(&ctx.cfg.tts_language)
        .to_string();
    safe_code(&language)?;
    let voice = args
        .get("voice")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| ctx.cfg.tts_voice.clone())
        .unwrap_or_else(|| language.clone());
    safe_code(&voice)?;
    let rate_wpm = args
        .get("rate_wpm")
        .and_then(|v| v.as_u64())
        .unwrap_or(u64::from(ctx.cfg.tts_rate));
    if !(80..=450).contains(&rate_wpm) {
        return Err("rate_wpm must be 80 to 450".into());
    }
    let found = detect(ctx.cfg, super::runtime_dir().as_deref());
    let want = args
        .get("engine")
        .and_then(|v| v.as_str())
        .unwrap_or("auto");
    let engine = choose(want, &found, target, &ctx.cfg.tts_engine)?;
    Ok(Request {
        text,
        engine,
        voice,
        language,
        rate_wpm: rate_wpm as u32,
    })
}

/// Synthesizes the request into a WAV file with espeak-ng or Piper.
pub fn synthesize(ctx: &Ctx, r: &Request, out: &Path) -> Result<f64, String> {
    // [@ANCHOR: bench_mcp_audio_speak_synthesize]
    let (program, args) = match r.engine {
        Engine::EspeakNg => ("espeak-ng", espeak_args(&r.voice, r.rate_wpm, out)),
        Engine::Piper => (
            "piper",
            piper_args(
                ctx.cfg
                    .piper_model
                    .as_deref()
                    .ok_or("piper_model is not set")?,
                out,
            ),
        ),
        Engine::SpeechDispatcher => return Err("speech-dispatcher cannot write a WAV file".into()),
    };
    run_with_stdin(program, &args, &r.text, Duration::from_secs(30))?;
    let w = wav::read(out)?;
    if w.seconds() > MAX_SPEECH_S {
        return Err(format!(
            "the speech is {:.0} s long, over the {MAX_SPEECH_S:.0} s limit",
            w.seconds()
        ));
    }
    Ok(w.seconds())
}

/// Makes the sound for the request on `dev`: the caller holds the volume guard. Returns the seconds of speech.
pub fn say(ctx: &Ctx, dev: &Device, r: &Request) -> Result<f64, String> {
    // [@ANCHOR: bench_mcp_audio_speak_say]
    if r.engine == Engine::SpeechDispatcher {
        let started = Instant::now();
        let voice = (r.voice != r.language).then_some(r.voice.as_str());
        run_with_stdin(
            "spd-say",
            &spd_args(&r.language, voice, r.rate_wpm),
            &r.text,
            Duration::from_secs(40),
        )?;
        return Ok(started.elapsed().as_secs_f64());
    }
    let tmp = store::new_path(&ctx.cfg.capture_dir(), "speech", None)?;
    let result = synthesize(ctx, r, &tmp).and_then(|secs| {
        Playing::start(&dev.node_name, &tmp, 1.0)?.wait(Duration::from_secs_f64(secs + 5.0))?;
        Ok(secs)
    });
    let _ = std::fs::remove_file(&tmp);
    result
}

pub fn synthesizers(ctx: &Ctx, _args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_audio_speak_synthesizers]
    let f = detect(ctx.cfg, super::runtime_dir().as_deref());
    let voices = if f.espeak_ng {
        let (ok, text) = ctx.run.run("espeak-ng", &["--voices".into()])?;
        if ok {
            parse_voices(&text)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    let mut missing = Vec::new();
    if !f.espeak_ng {
        missing.push("espeak-ng (apt install espeak-ng)");
    }
    if !f.spd_say {
        missing.push("speech-dispatcher (apt install speech-dispatcher)");
    }
    Ok(json!({
        "engines": [
            {"engine": "speech-dispatcher", "installed": f.spd_say, "server_running": f.speechd_running,
             "note": "plays through the system default output only; started on demand by spd-say"},
            {"engine": "espeak-ng", "installed": f.espeak_ng, "note": "writes a WAV that is played on the chosen output"},
            {"engine": "piper", "installed": f.piper, "model": f.piper_model, "note": "needs piper_model in the config"},
        ],
        "configured_engine": ctx.cfg.tts_engine, "default_voice": ctx.cfg.tts_voice, "default_language": ctx.cfg.tts_language,
        "default_rate_wpm": ctx.cfg.tts_rate, "not_installed": missing,
        "espeak_voices": voices.iter().map(|v| json!({"language": v.language, "voice": v.name, "gender": v.gender})).collect::<Vec<_>>(),
    }))
}

pub fn speak(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: audio_speak_tool]
    let dev = super::tools::pick(ctx, args, "output", super::devices::Dir::Output)?;
    let r = request(ctx, args, dev)?;
    if args.get("dry_run").and_then(|v| v.as_bool()) == Some(true) {
        let lim = super::safety::limits(ctx.cfg, dev, false, ctx.minute);
        let would = json!({"engine": r.engine.name(), "voice": r.voice, "language": r.language, "rate_wpm": r.rate_wpm,
            "characters": r.text.chars().count(), "wav_file": args.get("wav_file")});
        return super::tools::dry_plan(ctx, dev, &lim, super::safety::Want::Keep, would);
    }
    if let Some(name) = args.get("wav_file").and_then(|n| n.as_str()) {
        if r.engine == Engine::SpeechDispatcher {
            return Err(
                "speech-dispatcher cannot write a WAV file; use engine espeak-ng or piper".into(),
            );
        }
        let dir = ctx.cfg.capture_dir();
        let path: PathBuf = store::new_path(&dir, "speech", Some(name))?;
        let secs = synthesize(ctx, &r, &path)?;
        store::prune(&dir, ctx.cfg.retention_files, ctx.cfg.retention_days);
        return Ok(
            json!({"engine": r.engine.name(), "voice": r.voice, "wav_file": path, "seconds": round3(secs), "audible": false}),
        );
    }
    let lim = super::safety::limits(ctx.cfg, dev, false, ctx.minute);
    let want = match args.get("volume").and_then(|v| v.as_f64()) {
        Some(v) if !(0.0..=1.0).contains(&v) => return Err("volume must be 0 to 1".into()),
        Some(v) => super::safety::Want::Level(v),
        None => super::safety::Want::Keep,
    };
    let unmute = args
        .get("unmute")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut secs = 0.0;
    let (applied, took) = play_guarded(ctx, dev, &lim, want, unmute, || {
        secs = say(ctx, dev, &r)?;
        Ok(Duration::from_secs_f64(secs))
    })?;
    let mut out = json!({
        "output": dev.node_name, "alias": dev.alias, "engine": r.engine.name(), "voice": r.voice, "language": r.language,
        "rate_wpm": r.rate_wpm, "seconds": round3(took.as_secs_f64()), "volume_used": applied.volume, "volume_limit": lim.volume,
        "quiet_hours": lim.quiet, "volume_restored": applied.before.volume, "mute_restored": applied.before.muted,
    });
    if applied.muted {
        out["warning"] = json!("the output is muted, so nothing was audible; pass unmute: true to unmute it for the speech");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::testkit::{devbox_devices, fake_programs, recorded, scratch, Scripted};
    use crate::audio::wav::Wav;

    fn dev(default: bool) -> Device {
        let mut d = devbox_devices()
            .into_iter()
            .find(|d| d.description.ends_with("Speaker"))
            .unwrap();
        d.is_default = default;
        d
    }

    fn found(spd: bool, running: bool, espeak: bool, piper: bool) -> Found {
        Found {
            espeak_ng: espeak,
            spd_say: spd,
            speechd_running: running,
            piper,
            piper_model: piper.then(|| "m.onnx".to_string()),
        }
    }

    // Tests [@ANCHOR: audio_speak_choose]
    #[test]
    fn auto_prefers_speech_dispatcher_only_for_a_running_server_and_the_default_output() {
        let all = found(true, true, true, true);
        assert_eq!(
            choose("auto", &all, &dev(true), "auto").unwrap(),
            Engine::SpeechDispatcher
        );
        assert_eq!(
            choose("auto", &all, &dev(false), "auto").unwrap(),
            Engine::EspeakNg
        );
        let idle = found(true, false, true, false);
        assert_eq!(
            choose("auto", &idle, &dev(true), "auto").unwrap(),
            Engine::EspeakNg
        );
        let mut reserved = dev(true);
        reserved.reserved = Some("x".into());
        assert_eq!(
            choose("auto", &all, &reserved, "auto").unwrap(),
            Engine::EspeakNg
        );
        assert_eq!(
            choose(
                "auto",
                &found(false, false, false, true),
                &dev(true),
                "auto"
            )
            .unwrap(),
            Engine::Piper
        );
    }

    #[test]
    fn a_missing_synthesizer_says_what_to_install_and_installs_nothing() {
        let e = choose(
            "auto",
            &found(false, false, false, false),
            &dev(true),
            "auto",
        )
        .unwrap_err();
        assert!(
            e.contains("apt install espeak-ng") && e.contains("nothing was installed"),
            "{e}"
        );
        assert!(choose(
            "espeak-ng",
            &found(true, true, false, false),
            &dev(true),
            "auto"
        )
        .unwrap_err()
        .contains("not available"));
        assert!(choose(
            "piper",
            &found(false, false, true, false),
            &dev(true),
            "auto"
        )
        .is_err());
        let e = choose(
            "speech-dispatcher",
            &found(true, true, true, false),
            &dev(false),
            "auto",
        )
        .unwrap_err();
        assert!(e.contains("system default"), "{e}");
        assert!(choose(
            "festival",
            &found(true, true, true, true),
            &dev(true),
            "auto"
        )
        .is_err());
    }

    #[test]
    fn the_configured_engine_replaces_auto() {
        let all = found(true, true, true, true);
        assert_eq!(
            choose("auto", &all, &dev(true), "espeak-ng").unwrap(),
            Engine::EspeakNg
        );
    }

    // Tests [@ANCHOR: audio_speak_voices]
    #[test]
    fn the_recorded_espeak_voice_list_parses() {
        let text = std::fs::read_to_string(recorded().join("espeak-voices.txt")).unwrap();
        let v = parse_voices(&text);
        assert!(v.len() > 100, "{}", v.len());
        assert!(v
            .iter()
            .any(|x| x.language == "en-us" && x.name == "English_(America)"));
        assert!(v.iter().all(|x| !x.name.is_empty()));
        assert!(parse_voices("").is_empty());
    }

    // Tests [@ANCHOR: bench_mcp_audio_speak_safe_code]
    // Tests [@ANCHOR: bench_mcp_audio_speak_spd_args]
    #[test]
    fn commands_carry_the_text_on_stdin_and_codes_are_checked() {
        let a = espeak_args("en-us", 150, Path::new("/x/y.wav"));
        assert_eq!(
            a,
            vec!["--stdin", "-v", "en-us", "-s", "150", "-w", "/x/y.wav"]
        );
        assert!(!a.iter().any(|x| x.contains("hello")));
        assert_eq!(
            piper_args("m.onnx", Path::new("o.wav")),
            vec!["--model", "m.onnx", "--output_file", "o.wav"]
        );
        assert_eq!(
            spd_args("en", Some("male1"), 160),
            vec!["--wait", "-e", "-l", "en", "-r", "0", "-t", "male1"]
        );
        assert_eq!((spd_rate(160), spd_rate(450), spd_rate(80)), (0, 100, -57));
        assert!(safe_code("en-us+f3").is_ok());
        for bad in ["", "-v", "a b", "en;rm", "x\n", "é"] {
            assert!(safe_code(bad).is_err(), "{bad:?}");
        }
    }

    fn tone_wav(path: &Path, rate: u32, seconds: f64) {
        wav::write(
            path,
            &Wav {
                rate,
                channels: 1,
                samples: vec![0.1; (f64::from(rate) * seconds) as usize],
            },
        )
        .unwrap();
    }

    fn request_for(engine: Engine, voice: &str) -> Request {
        Request {
            text: "net check".into(),
            engine,
            voice: voice.into(),
            language: "en-us".into(),
            rate_wpm: 160,
        }
    }

    // Tests [@ANCHOR: bench_mcp_audio_speak_engine_name]
    #[test]
    fn each_engine_has_the_name_the_tool_arguments_use() {
        assert_eq!(Engine::SpeechDispatcher.name(), "speech-dispatcher");
        assert_eq!(Engine::EspeakNg.name(), "espeak-ng");
        assert_eq!(Engine::Piper.name(), "piper");
    }

    // Tests [@ANCHOR: bench_mcp_audio_speak_run_with_stdin]
    #[test]
    fn a_program_gets_the_text_on_stdin_and_failure_and_overrun_are_errors() {
        let d = scratch("stdin");
        let got = d.join("got.txt");
        let script = format!("cat > '{}'", got.display());
        run_with_stdin(
            "sh",
            &["-c".into(), script],
            "hello relay",
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&got).unwrap(), "hello relay");
        let e = run_with_stdin(
            "sh",
            &["-c".into(), "cat > /dev/null; echo boom >&2; exit 3".into()],
            "x",
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(e.contains("failed") && e.contains("boom"), "{e}");
        let e =
            run_with_stdin("sleep", &["5".into()], "x", Duration::from_millis(100)).unwrap_err();
        assert!(e.contains("did not finish"), "{e}");
        let e =
            run_with_stdin("no-such-synth-program", &[], "x", Duration::from_secs(1)).unwrap_err();
        assert!(e.contains("no-such-synth-program"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // Tests [@ANCHOR: bench_mcp_audio_speak_request]
    #[test]
    fn a_request_is_validated_and_takes_its_defaults_from_the_config() {
        let fakes = fake_programs();
        tone_wav(&fakes.join("espeak-en-us.wav"), 22050, 1.0);
        let devs = devbox_devices();
        let target = dev(false);
        let cfg = Config::default();
        let run = Scripted::new(&[]);
        let ctx = Ctx {
            run: &run,
            cfg: &cfg,
            devs: &devs,
            minute: 720,
            discovery: None,
        };
        let r = request(&ctx, &json!({"text": "  net check  "}), &target).unwrap();
        assert_eq!(
            (
                r.text.as_str(),
                r.engine,
                r.voice.as_str(),
                r.language.as_str(),
                r.rate_wpm
            ),
            ("net check", Engine::EspeakNg, "en-us", "en-us", 160)
        );
        let r = request(
            &ctx,
            &json!({"text": "x", "voice": "en+f3", "language": "en", "rate_wpm": 200, "engine": "espeak-ng"}),
            &target,
        )
        .unwrap();
        assert_eq!(
            (r.voice.as_str(), r.language.as_str(), r.rate_wpm),
            ("en+f3", "en", 200)
        );
        for bad in [
            json!({}),
            json!({"text": "   "}),
            json!({"text": "x".repeat(MAX_TEXT + 1)}),
            json!({"text": "x", "language": "en;rm"}),
            json!({"text": "x", "voice": "-v"}),
            json!({"text": "x", "rate_wpm": 79}),
            json!({"text": "x", "rate_wpm": 451}),
            json!({"text": "x", "engine": "festival"}),
        ] {
            assert!(request(&ctx, &bad, &target).is_err(), "{bad}");
        }
        assert_eq!(
            request(&ctx, &json!({"text": "x".repeat(MAX_TEXT)}), &target)
                .unwrap()
                .text
                .len(),
            MAX_TEXT
        );
    }

    // Tests [@ANCHOR: bench_mcp_audio_speak_synthesize]
    #[test]
    fn synthesis_writes_a_wav_and_refuses_long_speech_and_missing_setup() {
        let fakes = fake_programs();
        tone_wav(&fakes.join("espeak-short.wav"), 22050, 1.0);
        tone_wav(&fakes.join("espeak-long.wav"), 8000, 31.0);
        let d = scratch("synth");
        let devs = devbox_devices();
        let cfg = Config::default();
        let run = Scripted::new(&[]);
        let ctx = Ctx {
            run: &run,
            cfg: &cfg,
            devs: &devs,
            minute: 720,
            discovery: None,
        };
        let out = d.join("o.wav");
        let secs = synthesize(&ctx, &request_for(Engine::EspeakNg, "short"), &out).unwrap();
        assert!((secs - 1.0).abs() < 0.01, "{secs}");
        assert!(out.exists());
        let e = synthesize(&ctx, &request_for(Engine::EspeakNg, "long"), &out).unwrap_err();
        assert!(e.contains("over the 30 s limit"), "{e}");
        let e = synthesize(&ctx, &request_for(Engine::Piper, "x"), &out).unwrap_err();
        assert!(e.contains("piper_model is not set"), "{e}");
        let e = synthesize(&ctx, &request_for(Engine::SpeechDispatcher, "x"), &out).unwrap_err();
        assert!(e.contains("cannot write a WAV"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // Tests [@ANCHOR: bench_mcp_audio_speak_say]
    #[test]
    fn saying_plays_the_synthesized_file_and_removes_it_afterwards() {
        let fakes = fake_programs();
        tone_wav(&fakes.join("espeak-say.wav"), 22050, 0.5);
        let d = scratch("say");
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
        let target = dev(true);
        let secs = say(&ctx, &target, &request_for(Engine::EspeakNg, "say")).unwrap();
        assert!((secs - 0.5).abs() < 0.01, "{secs}");
        let left: Vec<_> = std::fs::read_dir(&d).unwrap().collect();
        assert!(left.is_empty(), "the temporary speech file is removed");
        assert!(say(&ctx, &target, &request_for(Engine::Piper, "x")).is_err());
        let spoken = say(
            &ctx,
            &target,
            &request_for(Engine::SpeechDispatcher, "en-us"),
        )
        .unwrap();
        assert!(spoken >= 0.0);
        let _ = std::fs::remove_dir_all(&d);
    }

    // Tests [@ANCHOR: bench_mcp_audio_speak_synthesizers]
    #[test]
    fn the_synthesizer_report_lists_installed_engines_and_the_espeak_voices() {
        fake_programs();
        let devs = devbox_devices();
        let cfg = Config::default();
        let ctx = Ctx {
            run: &crate::audio::Process,
            cfg: &cfg,
            devs: &devs,
            minute: 720,
            discovery: None,
        };
        let v = synthesizers(&ctx, &json!({})).unwrap();
        let engines = v["engines"].as_array().unwrap();
        assert_eq!(engines[0]["installed"], true);
        assert_eq!(engines[1]["engine"], "espeak-ng");
        assert_eq!(engines[1]["installed"], true);
        let voices = v["espeak_voices"].as_array().unwrap();
        assert!(voices
            .iter()
            .any(|x| x["language"] == "en-us" && x["voice"] == "English_(America)"));
        assert_eq!(v["configured_engine"], "auto");
        assert!(v["not_installed"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| !m.as_str().unwrap().starts_with("espeak-ng")));
    }

    // Tests [@ANCHOR: audio_speak_tool]
    #[test]
    fn a_dry_run_of_speak_names_the_engine_and_the_volume_and_makes_no_sound() {
        let _fakes = fake_programs();
        let devs = devbox_devices();
        let spk = devs
            .iter()
            .find(|d| d.description.ends_with("Speaker"))
            .unwrap();
        let d = scratch("dry-speak");
        let cfg = crate::audio::Config {
            capture_dir: Some(d.clone()),
            ..crate::audio::Config::default()
        };
        let run = Scripted::new(&[(spk.id, 0.9, false)]);
        let ctx = Ctx {
            run: &run,
            cfg: &cfg,
            devs: &devs,
            minute: 720,
            discovery: None,
        };
        let v = speak(&ctx, &json!({"text": "net check", "output": "Speaker", "engine": "espeak-ng", "dry_run": true})).unwrap();
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["would"]["engine"], "espeak-ng");
        assert_eq!(v["volume_would_be"], 0.5);
        assert!(!run
            .calls
            .borrow()
            .iter()
            .any(|c| c.starts_with("wpctl set-")));
        // A dry run with a WAV label writes nothing either.
        let v = speak(&ctx, &json!({"text": "net check", "output": "Speaker", "engine": "espeak-ng", "wav_file": "x", "dry_run": true})).unwrap();
        assert_eq!(v["dry_run"], true);
        assert!(std::fs::read_dir(&d).unwrap().next().is_none());
        assert!(speak(
            &ctx,
            &json!({"text": "", "output": "Speaker", "dry_run": true})
        )
        .is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}

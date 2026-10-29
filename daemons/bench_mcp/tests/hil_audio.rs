// SPDX-License-Identifier: AGPL-3.0-or-later
//! Audio tools against the real PipeWire, without a sound in the room: a `pw-loopback` process makes a virtual sink whose output
//! is a virtual source, and the tools play into the sink and record from the source. Ignored by default (needs PipeWire for the
//! current user); run with `cargo test --test hil_audio -- --ignored --test-threads=1`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"hil","version":"1"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

struct Virtual(Child);

impl Virtual {
    /// A virtual sink and source joined by a loopback, at a priority that keeps them from becoming the defaults.
    fn start() -> Virtual {
        let child = Command::new("pw-loopback")
            .args([
                "-m",
                "[ FL FR ]",
                "--capture-props=media.class=Audio/Sink node.name=bench-lb-sink node.description=bench-lb-sink priority.session=1 priority.driver=1",
                "--playback-props=media.class=Audio/Source node.name=bench-lb-src node.description=bench-lb-src priority.session=1 priority.driver=1",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("pw-loopback");
        std::thread::sleep(std::time::Duration::from_millis(1500));
        Virtual(child)
    }
}

impl Drop for Virtual {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A 48 kHz mono 16-bit WAV: `bursts` beeps of `len_s` at `hz`, `gap_s` apart, after `lead_s` of silence.
fn beep_wav(path: &Path, hz: f64, bursts: usize, len_s: f64, gap_s: f64, lead_s: f64) {
    let rate = 48000.0;
    let total = lead_s + bursts as f64 * (len_s + gap_s) + 0.2;
    let samples: Vec<i16> = (0..(total * rate) as usize)
        .map(|i| {
            let t = i as f64 / rate;
            let k = ((t - lead_s) / (len_s + gap_s)).floor();
            let in_burst =
                t >= lead_s && k < bursts as f64 && (t - lead_s - k * (len_s + gap_s)) < len_s;
            if in_burst {
                ((2.0 * std::f64::consts::PI * hz * t).sin() * 0.1 * 32767.0) as i16
            } else {
                0
            }
        })
        .collect();
    let mut f = Vec::new();
    f.extend_from_slice(b"RIFF");
    f.extend_from_slice(&(36 + samples.len() as u32 * 2).to_le_bytes());
    f.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    f.extend_from_slice(&48000u32.to_le_bytes());
    f.extend_from_slice(&96000u32.to_le_bytes());
    f.extend_from_slice(&[2, 0, 16, 0]);
    f.extend_from_slice(b"data");
    f.extend_from_slice(&(samples.len() as u32 * 2).to_le_bytes());
    for s in samples {
        f.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, f).unwrap();
}

fn config(dir: &Path) -> PathBuf {
    let p = dir.join("bench.json");
    let beep = dir.join("beep.wav");
    beep_wav(&beep, 1800.0, 1, 0.3, 0.0, 0.3);
    let triple = dir.join("triple.wav");
    beep_wav(&triple, 1800.0, 3, 0.15, 0.25, 0.2);
    let wrong = dir.join("wrong.wav");
    beep_wav(&wrong, 600.0, 1, 0.3, 0.0, 0.3);
    std::fs::write(
        &p,
        format!(
            r#"{{"instruments":{{"snd":{{"driver":"audio","tier":"full","quiet_hours":false,
            "outputs":{{"virt":"bench-lb-sink"}},"inputs":{{"virt":"bench-lb-src"}},
            "alert_output":"virt","verify_input":"virt","capture_dir":"{d}/audio",
            "verify_actions":{{"beep":["pw-play","--target","bench-lb-sink","{b}"],
                               "triple":["pw-play","--target","bench-lb-sink","{t}"],
                               "wrong":["pw-play","--target","bench-lb-sink","{w}"],
                               "nothing":["true"]}}}}}}}}"#,
            d = dir.display(),
            b = beep.display(),
            t = triple.display(),
            w = wrong.display()
        ),
    )
    .unwrap();
    p
}

fn call(cfg: &Path, out: &Path, name: &str, args: &str) -> (bool, serde_json::Value) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bench_mcp"))
        .env("BENCH_MCP_CONFIG", cfg)
        .env("BENCH_MCP_OUT", out)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    writeln!(stdin, "{INIT}").unwrap();
    lines.next().unwrap().unwrap();
    writeln!(stdin, "{INITIALIZED}").unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{name}","arguments":{args}}}}}"#
    )
    .unwrap();
    let r: serde_json::Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    drop(stdin);
    let _ = child.wait();
    let text = r["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    let is_err = r["result"]["isError"].as_bool().unwrap_or(false);
    (
        !is_err,
        serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!(text)),
    )
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bench-audio-hil-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// Tests [@ANCHOR: audio_loopback_tool]
#[test]
#[ignore]
fn loopback_through_a_virtual_sink_and_source_is_heard() {
    let _v = Virtual::start();
    let d = scratch("loop");
    let cfg = config(&d);
    let (ok, r) = call(
        &cfg,
        &d,
        "bench_audio_loopback",
        r#"{"freq_hz":1500,"level_dbfs":-20}"#,
    );
    assert!(ok, "{r}");
    assert_eq!(r["result"]["heard"], true, "{r}");
    assert!(r["result"]["snr_db"].as_f64().unwrap() > 20.0, "{r}");
    // The output volume used scales the tone by its cube.
    let expected = -20.0 + 60.0 * r["volume_used"].as_f64().unwrap().log10();
    assert!(
        (r["result"]["level_dbfs"].as_f64().unwrap() - expected).abs() < 4.0,
        "{expected} {r}"
    );
    assert!(r["latency_ms_indicative"].as_f64().unwrap() > -50.0, "{r}");
    let _ = std::fs::remove_dir_all(&d);
}

// Tests [@ANCHOR: audio_record_tool] [@ANCHOR: audio_play_tool]
#[test]
#[ignore]
fn record_hears_a_tone_played_by_another_call_and_play_restores_the_volume() {
    let _v = Virtual::start();
    let d = scratch("rec");
    let cfg = config(&d);
    let (_, before) = call(&cfg, &d, "bench_audio_levels", r#"{"device":"virt"}"#);
    let (cfg2, d2) = (cfg.clone(), d.clone());
    let recorder = std::thread::spawn(move || {
        call(
            &cfg2,
            &d2,
            "bench_audio_record",
            r#"{"input":"virt","seconds":2.5,"label":"virt tone"}"#,
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(900));
    let (ok, p) = call(
        &cfg,
        &d,
        "bench_audio_play",
        r#"{"tone":"sine","freq_hz":1500,"seconds":0.6,"level_dbfs":-20,"unmute":true}"#,
    );
    assert!(ok, "{p}");
    let (ok, rec) = recorder.join().unwrap();
    assert!(ok, "{rec}");
    let peak = rec["levels"]["peak_dbfs"].as_f64().unwrap();
    assert!(peak > -40.0 && peak < -10.0, "{rec}");
    assert_eq!(rec["levels"]["all_zero"], false, "{rec}");
    let file = rec["file"].as_str().unwrap();
    assert!(Path::new(file).exists());
    assert!(file.contains("virt-tone"), "{file}");
    let (_, after) = call(&cfg, &d, "bench_audio_levels", r#"{"device":"virt"}"#);
    assert_eq!(
        before["levels"], after["levels"],
        "volume and mute are put back"
    );
    let (ok, m) = call(
        &cfg,
        &d,
        "bench_audio_meter",
        r#"{"seconds":0.6,"interval_ms":200}"#,
    );
    assert!(ok, "{m}");
    assert!(m["readings"].as_array().unwrap().len() >= 2);
    let _ = std::fs::remove_dir_all(&d);
}

// Tests [@ANCHOR: audio_set_volume_tool]
#[test]
#[ignore]
fn the_volume_cap_and_reserved_names_and_file_names_are_enforced_end_to_end() {
    let _v = Virtual::start();
    let d = scratch("cap");
    let cfg = config(&d);
    let (ok, e) = call(
        &cfg,
        &d,
        "bench_audio_set_volume",
        r#"{"device":"virt","volume":0.9}"#,
    );
    assert!(!ok && e.as_str().unwrap().contains("cap"), "{e}");
    let (ok, e) = call(
        &cfg,
        &d,
        "bench_audio_play",
        r#"{"output":"USB_AUDIO_CODEC","seconds":0.3}"#,
    );
    assert!(!ok && e.as_str().unwrap().contains("reserved"), "{e}");
    let (ok, e) = call(
        &cfg,
        &d,
        "bench_audio_record",
        r#"{"input":"USB_AUDIO_CODEC"}"#,
    );
    assert!(!ok && e.as_str().unwrap().contains("reserved"), "{e}");
    let (ok, e) = call(
        &cfg,
        &d,
        "bench_audio_play",
        r#"{"file":"../../etc/passwd"}"#,
    );
    assert!(!ok, "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

// Tests [@ANCHOR: audio_verify_tool]
#[test]
#[ignore]
fn verify_passes_a_beep_and_fails_a_wrong_pitch_or_a_missing_sound_with_reasons() {
    let _v = Virtual::start();
    let d = scratch("verify");
    let cfg = config(&d);
    let (ok, r) = call(
        &cfg,
        &d,
        "bench_audio_verify",
        r#"{"action":"beep","window_s":3,"expect_hz":1800,"expect_beeps":1,"min_duration_s":0.2,"save_reference":"beep"}"#,
    );
    assert!(ok, "{r}");
    assert_eq!(r["pass"], true, "{r}");
    let hz = r["evidence"]["dominant_hz"].as_f64().unwrap();
    assert!((hz - 1800.0).abs() < 40.0, "{r}");
    assert_eq!(r["action"]["exit"], 0, "{r}");
    assert!(Path::new(r["saved_reference"].as_str().unwrap()).exists());
    // The same sound against its own reference.
    let (_, same) = call(
        &cfg,
        &d,
        "bench_audio_verify",
        r#"{"action":"beep","window_s":3,"reference":"ref-beep.wav","min_similarity":0.8}"#,
    );
    assert_eq!(same["pass"], true, "{same}");
    assert!(
        same["evidence"]["reference_similarity"].as_f64().unwrap() > 0.8,
        "{same}"
    );
    // A 600 Hz beep is not 1800 Hz, and is not like the reference.
    let (_, wrong) = call(
        &cfg,
        &d,
        "bench_audio_verify",
        r#"{"action":"wrong","window_s":3,"expect_hz":1800,"reference":"ref-beep.wav"}"#,
    );
    assert_eq!(wrong["pass"], false, "{wrong}");
    assert!(
        wrong["reason"].as_str().unwrap().contains("frequency"),
        "{wrong}"
    );
    // No sound at all.
    let (_, none) = call(
        &cfg,
        &d,
        "bench_audio_verify",
        r#"{"action":"nothing","window_s":2}"#,
    );
    assert_eq!(none["pass"], false, "{none}");
    // A virtual source that carries no sound records exact zeros, which is how a dead input looks: it cannot confirm silence.
    let (_, quiet) = call(
        &cfg,
        &d,
        "bench_audio_verify",
        r#"{"action":"nothing","window_s":2,"expect_silence":true}"#,
    );
    assert_eq!(quiet["pass"], false, "{quiet}");
    assert!(
        quiet["reason"].as_str().unwrap().contains("dead"),
        "{quiet}"
    );
    // Only configured actions run.
    let (ok, e) = call(&cfg, &d, "bench_audio_verify", r#"{"action":"rm -rf /"}"#);
    assert!(
        !ok && e.as_str().unwrap().contains("no verify action"),
        "{e}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

// Tests [@ANCHOR: audio_events_tool]
#[test]
#[ignore]
fn events_lists_three_beeps_played_by_another_process() {
    let _v = Virtual::start();
    let d = scratch("events");
    let cfg = config(&d);
    let (cfg2, d2) = (cfg.clone(), d.clone());
    let listener =
        std::thread::spawn(move || call(&cfg2, &d2, "bench_audio_events", r#"{"seconds":4}"#));
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let status = Command::new("pw-play")
        .args(["--target", "bench-lb-sink"])
        .arg(d.join("triple.wav"))
        .status()
        .unwrap();
    assert!(status.success());
    let (ok, r) = listener.join().unwrap();
    assert!(ok, "{r}");
    assert_eq!(r["event_count"], 3, "{r}");
    let first = &r["events"][0];
    assert!(
        (first["dominant_hz"].as_f64().unwrap() - 1800.0).abs() < 60.0,
        "{r}"
    );
    assert!(
        (first["duration_s"].as_f64().unwrap() - 0.15).abs() < 0.05,
        "{r}"
    );
    assert!(
        (r["mean_interval_s"].as_f64().unwrap() - 0.4).abs() < 0.05,
        "{r}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

// Tests [@ANCHOR: audio_speak_tool]
#[test]
#[ignore]
fn speech_goes_to_a_wav_file_and_to_a_virtual_output_where_it_is_heard() {
    let _v = Virtual::start();
    let d = scratch("speak");
    let cfg = config(&d);
    let (ok, s) = call(&cfg, &d, "bench_audio_synthesizers", "{}");
    assert!(ok, "{s}");
    assert_eq!(s["engines"][1]["installed"], true, "{s}");
    assert!(s["espeak_voices"].as_array().unwrap().len() > 50);
    let (ok, w) = call(
        &cfg,
        &d,
        "bench_audio_speak",
        r#"{"text":"Antenna rotor stopped.","wav_file":"rotor note","engine":"espeak-ng"}"#,
    );
    assert!(ok, "{w}");
    assert_eq!(w["audible"], false);
    assert!(w["seconds"].as_f64().unwrap() > 0.5, "{w}");
    assert!(Path::new(w["wav_file"].as_str().unwrap()).exists());
    let (cfg2, d2) = (cfg.clone(), d.clone());
    let listener =
        std::thread::spawn(move || call(&cfg2, &d2, "bench_audio_events", r#"{"seconds":6}"#));
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let (ok, sp) = call(
        &cfg,
        &d,
        "bench_audio_speak",
        r#"{"text":"Rotor stopped.","engine":"espeak-ng","unmute":true}"#,
    );
    assert!(ok, "{sp}");
    assert_eq!(sp["engine"], "espeak-ng");
    let (ok, ev) = listener.join().unwrap();
    assert!(ok, "{ev}");
    assert!(ev["event_count"].as_u64().unwrap() >= 1, "{ev}");
    let (ok, e) = call(
        &cfg,
        &d,
        "bench_audio_speak",
        r#"{"text":"x","voice":"-v; rm"}"#,
    );
    assert!(
        !ok && e.as_str().unwrap().contains("voice or language"),
        "{e}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

// Tests [@ANCHOR: audio_alert_tool]
#[test]
#[ignore]
fn an_alert_makes_sound_and_always_returns_the_text_even_when_it_cannot() {
    let _v = Virtual::start();
    let d = scratch("alert");
    let cfg = config(&d);
    let (ok, a) = call(
        &cfg,
        &d,
        "bench_audio_alert",
        r#"{"message":"Rotor stopped","severity":"warning","engine":"espeak-ng"}"#,
    );
    assert!(ok, "{a}");
    assert_eq!(a["audible"], true, "{a}");
    assert_eq!(a["text_to_send"], "[WARNING] Rotor stopped");
    assert_eq!(a["sound"]["spoken"], true);
    let (ok, t) = call(&cfg, &d, "bench_audio_alert", r#"{"severity":"info"}"#);
    assert!(ok && t["audible"] == true, "{t}");
    assert_eq!(t["text_to_send"], "[INFO] alert");
    let (ok, n) = call(
        &cfg,
        &d,
        "bench_audio_alert",
        r#"{"message":"Door open","severity":"urgent","output":"no-such-output"}"#,
    );
    assert!(ok, "{n}");
    assert_eq!(n["audible"], false, "{n}");
    assert_eq!(n["text_to_send"], "[URGENT] Door open");
    assert!(n["error"].as_str().unwrap().contains("no output"), "{n}");
    let (ok, r) = call(&cfg, &d, "bench_audio_alert", r#"{"repeat":9}"#);
    assert!(!ok, "{r}");
    let _ = std::fs::remove_dir_all(&d);
}

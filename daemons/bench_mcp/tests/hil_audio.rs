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

fn config(dir: &Path) -> PathBuf {
    let p = dir.join("bench.json");
    std::fs::write(
        &p,
        format!(
            r#"{{"instruments":{{"snd":{{"driver":"audio","tier":"full","quiet_hours":false,
            "outputs":{{"virt":"bench-lb-sink"}},"inputs":{{"virt":"bench-lb-src"}},
            "alert_output":"virt","verify_input":"virt","capture_dir":"{}/audio"}}}}}}"#,
            dir.display()
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

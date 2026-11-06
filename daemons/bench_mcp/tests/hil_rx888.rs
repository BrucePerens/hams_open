// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware-in-the-loop check for the rx888 module: the OWNER runs it, on the real instrument, and only the READ tools.
//! Ignored by default. Run with
//! `BENCH_MCP_CONFIG=<your config with an "rx888" instrument> cargo test --test hil_rx888 -- --ignored --nocapture`.
//!
//! The RX-888 must be booted (bench_rx888_boot) and no other radiod may use the configured groups.
//! Everything here is receive only. The radiod this test starts is stopped at the end.
//!
//! Rules for this file in a real module: read-only calls first (identity, status); never call a tool that radiates,
//! drives or actuates without an `ARE_YOU_SURE_*` environment variable set by the owner for that one run; leave the
//! instrument in the state it was found in; print what was observed so the owner can paste it into the pull request.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"hil","version":"1"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

/// Starts the server on the config in `BENCH_MCP_CONFIG`, completes the handshake and returns the text of each tool call.
fn calls(requests: &[(&str, &str)]) -> Vec<String> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bench_mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut next =
        || serde_json::from_str::<serde_json::Value>(&lines.next().unwrap().unwrap()).unwrap();
    writeln!(stdin, "{INIT}").unwrap();
    assert_eq!(next()["result"]["serverInfo"]["name"], "bench_mcp");
    writeln!(stdin, "{INITIALIZED}").unwrap();
    let mut out = Vec::new();
    for (i, (name, args)) in requests.iter().enumerate() {
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"{name}","arguments":{args}}}}}"#,
            i + 2
        )
        .unwrap();
        let r = next();
        assert!(
            r["result"]["isError"] != true,
            "{name}: {}",
            r["result"]["content"][0]["text"]
        );
        out.push(
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        );
    }
    drop(stdin);
    let _ = child.wait();
    out
}

#[test]
#[ignore]
fn status_radiod_frontend_tune_spectrum_and_capture_on_the_real_rx888() {
    let r = calls(&[
        ("bench_rx888_status", "{}"),
        ("bench_rx888_radiod_start", "{}"),
        ("bench_rx888_frontend", "{}"),
        (
            "bench_rx888_tune",
            r#"{"freq_hz":10000000,"mode":"am","readings":20}"#,
        ),
        (
            "bench_rx888_tune",
            r#"{"freq_hz":7074000,"mode":"usb","bandwidth_hz":2400}"#,
        ),
        (
            "bench_rx888_spectrum",
            r#"{"bins":8192,"span_hz":32350000,"center_hz":16175000,"averaging":20}"#,
        ),
        (
            "bench_rx888_capture",
            r#"{"freq_hz":10000000,"kind":"iq","seconds":2}"#,
        ),
        (
            "bench_rx888_capture",
            r#"{"freq_hz":10000000,"kind":"audio","mode":"am","seconds":2}"#,
        ),
        ("bench_rx888_frontend", "{}"),
        ("bench_rx888_radiod_stop", "{}"),
    ]);
    for line in &r {
        println!("{line}\n");
    }
    let fe: serde_json::Value = serde_json::from_str(&r[2]).unwrap();
    assert_eq!(fe["input_sample_rate_hz"], 64_800_000);
    let tune: serde_json::Value = serde_json::from_str(&r[3]).unwrap();
    let rate = tune["input_sample_rate_measured_sps"].as_f64().unwrap();
    let resolution = tune["rate_resolution_fraction"].as_f64().unwrap();
    assert!(
        (rate / 64.8e6 - 1.0).abs() <= 1.5 * resolution,
        "measured {rate}, resolution {resolution}"
    );
    let spec: serde_json::Value = serde_json::from_str(&r[5]).unwrap();
    // The bin at exactly 0 Hz reads zero for this real front end; no other may.
    assert!(spec["zero_bins"].as_u64().unwrap() <= 1, "{spec}");
}

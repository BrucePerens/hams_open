// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware-in-the-loop check for an instrument module: the OWNER runs it, on the real instrument, and only the READ tools.
//! Ignored by default. Run with
//! `BENCH_MCP_CONFIG=<your config with an "elad" instrument> cargo test --test hil_elad -- --ignored --nocapture`.
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
fn identity_is_read_without_streaming_or_changing_anything() {
    // Tests [@ANCHOR: bench_mcp_elad_usb_open]
    let r = calls(&[
        ("bench_elad_list", "{}"),
        ("bench_elad_identify", "{}"),
        ("bench_elad_identify", "{}"),
    ]);
    println!("observed for the pull request:\n{}\n{}", r[0], r[1]);
    let first: serde_json::Value = serde_json::from_str(&r[1]).unwrap();
    let second: serde_json::Value = serde_json::from_str(&r[2]).unwrap();
    assert_eq!(first, second, "reading the identity twice changed it");
    assert!(
        first["firmware"].is_string(),
        "a unit that reports its firmware"
    );
}

/// Streams a short capture only when the owner sets `ARE_YOU_SURE_ELAD_STREAM=1` for the run (an FDM-S1/S2 also needs the
/// config's "fpga_loaded": true). Receive only; the antenna input stays as it is.
#[test]
#[ignore]
fn a_short_capture_when_the_owner_allows_it() {
    if std::env::var("ARE_YOU_SURE_ELAD_STREAM").as_deref() != Ok("1") {
        eprintln!("ARE_YOU_SURE_ELAD_STREAM is not 1; no stream started");
        return;
    }
    let name = format!("hil_{}.cf32", std::process::id());
    let r = calls(&[(
        "bench_elad_capture",
        &format!(r#"{{"freq_hz":7100000,"seconds":1.0,"name":"{name}"}}"#),
    )]);
    println!("observed for the pull request:\n{}", r[0]);
    let v: serde_json::Value = serde_json::from_str(&r[0]).unwrap();
    assert!(v["samples"].as_u64().unwrap() > 0);
}

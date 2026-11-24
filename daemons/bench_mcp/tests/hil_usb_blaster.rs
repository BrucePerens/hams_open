// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware-in-the-loop check for an instrument module: the OWNER runs it, on the real instrument, and only the READ tools.
//! Ignored by default. Run with
//! `BENCH_MCP_CONFIG=<your config with a "usb_blaster" instrument> cargo test --test hil_usb_blaster -- --ignored --nocapture`.
//!
//! Rules for this file in a real module: read-only calls first (the scan); never call a tool that radiates,
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
fn the_scan_reads_the_chain_and_loads_nothing() {
    let r = calls(&[("bench_jtag_scan", "{}")]);
    println!("observed for the pull request:\n{}", r[0]);
    let v: serde_json::Value = serde_json::from_str(&r[0]).unwrap();
    assert!(v["devices"].as_array().is_some_and(|d| !d.is_empty()));
}

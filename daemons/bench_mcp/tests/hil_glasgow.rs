// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware-in-the-loop check of the Glasgow module: the OWNER runs it, on the real device. Read tools only, unless the owner sets `ARE_YOU_SURE_GLASGOW_WIRED=1` for a run with the jumper wire of the bench-session page (manual instruments/glasgow/bench-session) on A0 and A1.
//! Ignored by default. Run with
//! `BENCH_MCP_CONFIG=<your config with a "glasgow" instrument at tier full> cargo test --test hil_glasgow -- --ignored --nocapture`.
//!
//! Rules: read-only calls first (identity, status); never call a tool that radiates,
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
fn identity_voltages_and_link_are_read_without_changing_anything() {
    let r = calls(&[
        ("bench_glasgow_list", "{}"),
        ("bench_glasgow_status", "{}"),
        ("bench_glasgow_safe_state", "{}"),
    ]);
    for (name, text) in ["list", "status", "safe_state"].iter().zip(&r) {
        println!("--- bench_glasgow_{name}\n{text}");
    }
    let list: serde_json::Value = serde_json::from_str(&r[0]).unwrap();
    assert_eq!(
        list["installed"], true,
        "run glasgow/install_glasgow.py first"
    );
    assert_eq!(list["usb"]["usb_id"], "20b7:9db1");
    let status: serde_json::Value = serde_json::from_str(&r[1]).unwrap();
    assert_eq!(
        status["outputs_enabled"], false,
        "the ports must start at 0 V"
    );
}

/// With the jumper wire from A0 to A1 and the owner's say-so: powers port A at 3.3 V, drives A0 and reads A1, and ends with every port off.
#[test]
#[ignore]
fn a_wired_loopback_follows_the_drive_and_ends_safe() {
    if std::env::var("ARE_YOU_SURE_GLASGOW_WIRED").as_deref() != Ok("1") {
        println!("skipped: set ARE_YOU_SURE_GLASGOW_WIRED=1 with a jumper from A0 to A1 and nothing else connected");
        return;
    }
    let r = calls(&[
        (
            "bench_glasgow_port_set",
            r#"{"port":"A","volts_v":3.3,"confirm":true}"#,
        ),
        (
            "bench_glasgow_gpio",
            r#"{"pins":"A0:1","actions":["A0=1","A1"],"confirm":true}"#,
        ),
        (
            "bench_glasgow_gpio",
            r#"{"pins":"A0:1","actions":["A0=0","A1"],"confirm":true}"#,
        ),
        ("bench_glasgow_safe_state", "{}"),
    ]);
    for t in &r {
        println!("{t}");
    }
    let high: serde_json::Value = serde_json::from_str(&r[1]).unwrap();
    let low: serde_json::Value = serde_json::from_str(&r[2]).unwrap();
    assert_eq!(high["reads"]["A1"], true);
    assert_eq!(low["reads"]["A1"], false);
}

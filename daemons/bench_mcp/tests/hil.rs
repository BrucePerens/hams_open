// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware-in-the-loop checks against the real bench: a Siglent SDS1000X-E and an ATM6301 supply,
//! both at tier read_config. Ignored by default; run with
//! `BENCH_MCP_CONFIG=<config.json> cargo test -- --ignored`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"hil","version":"1"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

/// Complete handshake, then each request in turn; returns the responses after the initialize one.
fn rpc(requests: &[&str]) -> Vec<serde_json::Value> {
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
    for r in requests {
        writeln!(stdin, "{r}").unwrap();
        out.push(next());
    }
    drop(stdin);
    let _ = child.wait();
    out
}

fn text(r: &serde_json::Value) -> String {
    r["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn call(name: &str, args: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{name}","arguments":{args}}}}}"#
    )
}

#[test]
#[ignore]
fn list_and_identify_both_instruments() {
    let r = rpc(&[
        &call("bench_list", "{}"),
        &call("bench_identify", r#"{"instrument":"scope"}"#),
        &call("bench_identify", r#"{"instrument":"psu"}"#),
    ]);
    assert!(
        text(&r[0]).contains("siglent_sds") && text(&r[0]).contains("atm6301"),
        "{}",
        text(&r[0])
    );
    assert!(text(&r[1]).contains("Siglent"), "{}", text(&r[1]));
    assert!(text(&r[2]).contains("ATM6301"), "{}", text(&r[2]));
}

#[test]
#[ignore]
fn dangerous_calls_are_refused_at_read_config() {
    let r = rpc(&[
        &call(
            "bench_scope_self_cal",
            r#"{"confirm_inputs_disconnected":true}"#,
        ),
        &call("bench_psu_output", r#"{"on":true}"#),
        &call("bench_set", r#"{"instrument":"psu","voltage_v":30}"#),
    ]);
    for x in &r {
        assert_eq!(x["result"]["isError"], true, "{x}");
    }
    assert!(text(&r[1]).contains("needs tier Full"), "{}", text(&r[1]));
    assert!(text(&r[2]).contains("outside"), "{}", text(&r[2]));
}

#[test]
#[ignore]
fn supply_state_is_readable_with_the_output_off() {
    let r = rpc(&[&call("bench_status", r#"{"instrument":"psu"}"#)]);
    let s = text(&r[0]);
    assert!(
        s.contains("set_voltage_v") && s.contains("measured_voltage_v"),
        "{s}"
    );
}

/// Needs a `vna` instrument (driver `vnwa_open`, tier read_config) and a cable from the TX port to the RX port.
#[test]
#[ignore]
fn vna_sweeps_the_loopback_and_saves_a_csv() {
    let r = rpc(&[
        &call("bench_status", r#"{"instrument":"vna"}"#),
        &call(
            "bench_vna_sweep",
            r#"{"start_hz":10000000,"stop_hz":30000000,"points":3}"#,
        ),
    ]);
    let status: serde_json::Value = serde_json::from_str(&text(&r[0])).unwrap();
    assert_eq!(status["usb"], "present", "{status}");
    let sweep: serde_json::Value = serde_json::from_str(&text(&r[1])).unwrap();
    assert_eq!(sweep["points"], 3, "{sweep}");
    let rows = sweep["rows"].as_array().unwrap();
    for row in rows {
        // A cable between the ports: well within a few dB of through, on both channels.
        for key in ["s21_db", "s11_db"] {
            let db = row[key].as_f64().unwrap();
            assert!((-6.0..6.0).contains(&db), "{key} {db} dB in {row}");
        }
    }
    // The same sweep again: the points repeat (a block per point, each its own previous frequency).
    let again = rpc(&[&call(
        "bench_vna_sweep",
        r#"{"start_hz":10000000,"stop_hz":30000000,"points":3}"#,
    )]);
    let again: serde_json::Value = serde_json::from_str(&text(&again[0])).unwrap();
    for (a, b) in rows.iter().zip(again["rows"].as_array().unwrap()) {
        let d = (a["s21_db"].as_f64().unwrap() - b["s21_db"].as_f64().unwrap()).abs();
        assert!(
            d < 0.05,
            "S21 changed by {d} dB between two sweeps: {a} then {b}"
        );
    }
    let file = sweep["file"].as_str().unwrap();
    assert_eq!(std::fs::read_to_string(file).unwrap().lines().count(), 4);
}

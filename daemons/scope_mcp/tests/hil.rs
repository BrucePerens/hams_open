// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware-in-the-loop checks against a real Siglent SDS1000X-E on the LAN. Ignored by default;
//! run with `SCOPE_MCP_HOST=<ip> cargo test --release -- --ignored` (the probe on the calibrator,
//! scope running).

use std::io::Write;
use std::process::{Command, Stdio};

fn rpc(requests: &[&str]) -> Vec<serde_json::Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_scope_mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for r in requests {
        writeln!(stdin, "{r}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
#[ignore]
fn identify_and_status_on_a_real_scope() {
    let r = rpc(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"scope_identify","arguments":{}}}"#,
    ]);
    let text = r[1]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Siglent"), "{text}");
}

#[test]
#[ignore]
fn self_cal_is_refused_at_the_default_tier() {
    let r = rpc(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"scope_self_cal","arguments":{"confirm_inputs_disconnected":true}}}"#,
    ]);
    assert_eq!(r[0]["result"]["isError"], true);
}

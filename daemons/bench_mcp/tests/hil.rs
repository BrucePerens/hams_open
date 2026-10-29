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
    rpc_cfg(None, requests)
}

/// As `rpc`, with the server reading the given config file instead of `BENCH_MCP_CONFIG`.
fn rpc_cfg(config: Option<&str>, requests: &[&str]) -> Vec<serde_json::Value> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_bench_mcp"));
    if let Some(c) = config {
        cmd.env("BENCH_MCP_CONFIG", c);
    }
    let mut child = cmd
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

// The SDR checks need `BENCH_MCP_SDR_CONFIG=<file>` naming a config with a bladeRF instrument "blade" (with its XB-200) and a
// LimeSDR instrument "lime" at read_config, both plugged in and with the antennas disconnected or not (nothing here needs a signal).
fn sdr_rpc(requests: &[&str]) -> Vec<serde_json::Value> {
    let cfg = std::env::var("BENCH_MCP_SDR_CONFIG").expect("BENCH_MCP_SDR_CONFIG");
    rpc_cfg(Some(&cfg), requests)
}

#[test]
#[ignore]
fn sdr_identify_reports_firmware_and_channels() {
    let r = sdr_rpc(&[
        &call("bench_identify", r#"{"instrument":"blade"}"#),
        &call("bench_identify", r#"{"instrument":"lime"}"#),
    ]);
    let (b, l) = (text(&r[0]), text(&r[1]));
    assert!(
        b.contains("fw_version") && b.contains("\"rx_channels\": 1"),
        "{b}"
    );
    assert!(
        l.contains("gatewareVersion") && l.contains("\"rx_channels\": 2"),
        "{l}"
    );
}

#[test]
#[ignore]
fn sdr_captures_are_continuous_and_have_a_floor() {
    let r = sdr_rpc(&[
        &call(
            "bench_sdr_capture",
            r#"{"instrument":"blade","freq_hz":433920000,"samples":1048576,"sample_rate_hz":4000000}"#,
        ),
        &call(
            "bench_sdr_capture",
            r#"{"instrument":"blade","freq_hz":7100000,"samples":262144}"#,
        ),
        &call(
            "bench_sdr_capture",
            r#"{"instrument":"lime","freq_hz":433920000,"samples":1048576,"sample_rate_hz":4000000}"#,
        ),
        &call(
            "bench_sdr_capture",
            r#"{"instrument":"lime","freq_hz":433920000,"channel":1}"#,
        ),
    ]);
    for x in &r {
        let t = text(x);
        assert_ne!(x["result"]["isError"], true, "{t}");
        assert!(t.contains("\"continuous\": true"), "samples were lost: {t}");
        assert!(
            t.contains("noise_floor_dbfs") && t.contains("iq_file"),
            "{t}"
        );
    }
}

#[test]
#[ignore]
fn sdr_survey_steps_and_writes_a_csv() {
    let r = sdr_rpc(&[&call(
        "bench_sdr_survey",
        r#"{"instrument":"lime","start_hz":88000000,"stop_hz":108000000}"#,
    )]);
    let t = text(&r[0]);
    assert!(t.contains("\"steps\": 3") && t.contains("csv_file"), "{t}");
    let v: serde_json::Value = serde_json::from_str(&t).unwrap();
    let csv = std::fs::read_to_string(v["csv_file"].as_str().unwrap()).unwrap();
    assert!(csv.starts_with("freq_hz,power_dbfs\n") && csv.lines().count() > 1000);
}

#[test]
#[ignore]
fn sdr_coherence_needs_two_receivers_and_runs_on_the_lime() {
    let r = sdr_rpc(&[
        &call(
            "bench_sdr_coherence",
            r#"{"instrument":"blade","freq_hz":433920000}"#,
        ),
        &call(
            "bench_sdr_coherence",
            r#"{"instrument":"lime","freq_hz":433920000,"repeats":2}"#,
        ),
    ]);
    assert_eq!(r[0]["result"]["isError"], true);
    assert!(
        text(&r[0]).contains("coherence needs two"),
        "{}",
        text(&r[0])
    );
    assert!(
        text(&r[1]).contains("phase_std_across_retunes_deg"),
        "{}",
        text(&r[1])
    );
}

/// Needs `vna` (with `hub` and `hub_port`) and `scope` instruments at read_config. Cold-resets the unit through the hub while a
/// scope measurement is requested on the same server: the scope answers first (its lock is its own), the unit is back with both
/// codecs and a good one-point sweep, and the status reports the cycle.
#[test]
#[ignore]
fn vna_power_cycle_leaves_the_scope_usable() {
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
    next();
    writeln!(stdin, "{INITIALIZED}").unwrap();
    let with_id = |id: u32, name: &str, args: &str| {
        call(name, args).replace(r#""id":2"#, &format!(r#""id":{id}"#))
    };
    writeln!(stdin, "{}", with_id(21, "bench_vna_power_cycle", "{}")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    writeln!(
        stdin,
        "{}",
        with_id(22, "bench_measure", r#"{"instrument":"scope","channel":1}"#)
    )
    .unwrap();
    let (first, second) = (next(), next());
    assert_eq!(first["id"], 22, "the scope must answer first: {first}");
    assert_eq!(second["id"], 21, "{second}");
    assert!(first["result"]["isError"] != true, "{first}");
    let cycle: serde_json::Value = serde_json::from_str(&text(&second)).unwrap();
    assert_eq!(cycle["power_cycled"], true, "{cycle}");
    let s21 = cycle["verified"]["s21_db"].as_f64().unwrap();
    assert!(s21.is_finite(), "{cycle}");
    writeln!(
        stdin,
        "{}",
        with_id(23, "bench_status", r#"{"instrument":"vna"}"#)
    )
    .unwrap();
    let status: serde_json::Value = serde_json::from_str(&text(&next())).unwrap();
    assert_eq!(status["usb"], "present", "{status}");
    assert_eq!(status["power_cycles_by_this_server"], 1, "{status}");
    drop(stdin);
    let _ = child.wait();
}

/// Needs `vna` (read_config) and `scope` instruments, the TX port through the pad and tee to the scope and the RX input, and nothing else using the unit.
/// Each VNA function through its own tool: the generator's level steps read on the scope with the host FFT, the meter on the loopback, a sweep.
#[test]
#[ignore]
fn each_vna_function_works_through_its_tool() {
    let tool = |name: &str, args: &str| -> serde_json::Value {
        let r = rpc(&[&call(name, args)]);
        assert!(r[0]["result"]["isError"] != true, "{name}: {}", text(&r[0]));
        serde_json::from_str(&text(&r[0])).unwrap()
    };
    tool(
        "bench_vna_generator_on",
        r#"{"freq_hz":10000000,"level_db":0}"#,
    );
    let mut reads = Vec::new();
    for db in [0.0f64, -10.0, -20.0] {
        tool(
            "bench_vna_generator_level",
            &format!(r#"{{"level_db":{db}}}"#),
        );
        std::thread::sleep(std::time::Duration::from_secs(2));
        let s = tool(
            "bench_scope_spectrum",
            r#"{"instrument":"scope","channel":1,"timebase_s":0.0002,"memory_depth":"14M","points":262144,"window":"blackman","peaks":1}"#,
        );
        let p = &s["peaks"][0];
        assert!((p["freq_hz"].as_f64().unwrap() - 1e7).abs() < 5e4, "{p}");
        reads.push(p["vrms"].as_f64().unwrap());
    }
    tool("bench_vna_generator_off", "{}");
    // Steps of 10 dB are a factor 3.162 in voltage, to 5 % (the scope's vertical scale must not clip the 0 dB line: 10 mV per division).
    assert!(
        (reads[0] / reads[1] / 3.162 - 1.0).abs() < 0.05
            && (reads[1] / reads[2] / 3.162 - 1.0).abs() < 0.05,
        "{reads:?}"
    );
    std::thread::sleep(std::time::Duration::from_secs(3));
    let m = tool("bench_vna_meter", r#"{"freq_hz":10000000}"#);
    assert!(m["offset_hz"].as_f64().unwrap().abs() < 0.01, "{m}");
    let sw = tool(
        "bench_vna_sweep",
        r#"{"start_hz":1000000,"stop_hz":100000000,"points":5,"if_hz":4000}"#,
    );
    assert_eq!(sw["points"], 5, "{sw}");
}

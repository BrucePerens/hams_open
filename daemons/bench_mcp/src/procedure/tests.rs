// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests of the procedure runner. The mock bench is the production configuration path with `example` instruments whose
//! far end is a fake device on a local socket (`crate::example::testkit`); every call goes through `crate::call`, the
//! dispatcher an MCP client reaches, and every procedure is a real `python3 -I` process.

use super::*;
use crate::config;
use crate::example::testkit::{FakeDevice, Sim};
use serde_json::json;
use std::time::{Duration, Instant};

pub struct Bench {
    pub insts: Vec<Instrument>,
    pub dir: PathBuf,
    _devices: Vec<FakeDevice>,
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A bench with a generator-like instrument `gen` (full tier), a read-only one `dmm`, and the runner `procs` (settings from
/// `procs_extra`, a JSON object body such as `"max_runs":1`).
pub fn bench_with(runner_tier: &str, procs_extra: &str) -> Bench {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("bench_proc_test_{}_{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let gen = FakeDevice::sim(Sim::default());
    let dmm = FakeDevice::sim(Sim::default());
    let sep = if procs_extra.is_empty() { "" } else { "," };
    let text = format!(
        r#"{{"instruments":{{
        "gen":{{"driver":"example","tier":"full","host":"127.0.0.1","port":{}}},
        "dmm":{{"driver":"example","tier":"read_only","host":"127.0.0.1","port":{}}},
        "procs":{{"driver":"procedure","tier":"{runner_tier}","dir":"{}","grace_s":1.0{sep}{procs_extra}}}}}}}"#,
        gen.port(),
        dmm.port(),
        dir.display()
    );
    Bench {
        insts: config::parse(&text).expect("the mock bench configuration parses"),
        dir,
        _devices: vec![gen, dmm],
    }
}

pub fn bench() -> Bench {
    bench_with("full", "")
}

pub fn tool(b: &Bench, name: &str, args: Value) -> Result<Value, String> {
    crate::call(&b.insts, name, &args)
}

/// Starts a run of `source`; `extra` is merged into the arguments of `bench_proc_run`.
pub fn start(b: &Bench, source: &str, extra: Value) -> String {
    let mut args = json!({"source": source});
    if let Value::Object(m) = extra {
        args.as_object_mut().unwrap().extend(m);
    }
    let r = tool(b, "bench_proc_run", args).expect("bench_proc_run");
    r["run"].as_str().unwrap().to_string()
}

/// Waits until the run has ended and returns its result.
pub fn finish(b: &Bench, id: &str) -> Value {
    let t = Instant::now();
    loop {
        let st = tool(b, "bench_proc_status", json!({"run": id})).unwrap();
        if st["finished"] == json!(true) {
            return tool(b, "bench_proc_result", json!({"run": id})).unwrap();
        }
        assert!(
            t.elapsed() < Duration::from_secs(60),
            "run {id} did not end: {st}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Starts, waits, returns the result.
pub fn run(b: &Bench, source: &str, extra: Value) -> Value {
    let id = start(b, source, extra);
    finish(b, &id)
}

// Tests [@ANCHOR: proc_config]
#[test]
fn the_runner_entry_is_read_with_defaults_and_bad_values_are_refused() {
    let c = parse_config(&json!({})).unwrap();
    assert_eq!(c, Config::default());
    assert_eq!(
        (c.max_runs, c.grace_s, c.wrapper.as_str()),
        (2, 5.0, "none")
    );
    let c = parse_config(&json!({"dir": "/var/tmp/p", "max_runs": 3, "retention_days": 1, "wrapper": "systemd-run", "python": "/usr/bin/python3"})).unwrap();
    assert_eq!(
        (c.max_runs, c.retention_days, c.wrapper.as_str()),
        (3, 1.0, "systemd-run")
    );
    for bad in [
        json!({"dir": "relative"}),
        json!({"max_runs": 0}),
        json!({"max_runs": 99}),
        json!({"grace_s": "x"}),
        json!({"wrapper": "docker"}),
        json!({"python": "python3"}),
        json!({"retention_days": -1}),
    ] {
        assert!(parse_config(&bad).is_err(), "{bad}");
    }
    let v =
        config::parse(r#"{"instruments":{"p":{"driver":"procedure","tier":"full","max_runs":5}}}"#)
            .unwrap();
    assert_eq!((v[0].kind, v[0].procedure.max_runs), (Kind::Procedure, 5));
    assert!(
        config::parse(r#"{"instruments":{"p":{"driver":"procedure","wrapper":"x"}}}"#).is_err()
    );
}

// Tests [@ANCHOR: proc_tool_defs]
// Tests [@ANCHOR: proc_instrument]
// Tests [@ANCHOR: proc_call]
// Tests [@ANCHOR: proc_flag_arg]
// Tests [@ANCHOR: proc_number_arg]
#[test]
fn tools_are_listed_by_tier_and_refused_above_it() {
    let names = |b: &Bench| {
        tool_defs(&b.insts)
            .iter()
            .map(|t| t.name.to_string())
            .collect::<Vec<_>>()
    };
    let full = bench();
    assert_eq!(names(&full).len(), SPECS.len());
    let low = bench_with("read_only", "");
    let n = names(&low);
    assert!(
        n.contains(&"bench_proc_status".to_string())
            && n.contains(&"bench_proc_cancel".to_string())
    );
    assert!(!n.contains(&"bench_proc_run".to_string()));
    let e = tool(&low, "bench_proc_run", json!({"source": "pass"})).unwrap_err();
    assert!(e.contains("needs tier"), "{e}");
    let none =
        config::parse(r#"{"instruments":{"x":{"driver":"example","host":"127.0.0.1"}}}"#).unwrap();
    assert!(call(&none, "bench_proc_list", &json!({}))
        .unwrap_err()
        .contains("no procedure runner"));
    let e = tool(
        &full,
        "bench_proc_status",
        json!({"instrument": "gen", "run": "r"}),
    )
    .unwrap_err();
    assert!(e.contains("no procedure runner named"), "{e}");
    assert!(tool(&full, "bench_proc_status", json!({}))
        .unwrap_err()
        .contains("run is required"));
    let e = tool(
        &full,
        "bench_proc_run",
        json!({"source": "pass", "wall_s": "soon"}),
    )
    .unwrap_err();
    assert!(e.contains("wall_s must be a number"), "{e}");
    let e = tool(
        &full,
        "bench_proc_run",
        json!({"source": "pass", "trace": "yes"}),
    )
    .unwrap_err();
    assert!(e.contains("trace must be true or false"), "{e}");
    let e = tool(
        &full,
        "bench_proc_run",
        json!({"source": "pass", "tier": "full"}),
    )
    .unwrap_err();
    assert!(e.contains("confirm_full_tier"), "{e}");
    let two =
        config::parse(r#"{"instruments":{"a":{"driver":"procedure"},"b":{"driver":"procedure"}}}"#)
            .unwrap();
    assert!(call(&two, "bench_proc_list", &json!({}))
        .unwrap_err()
        .contains("name the instrument"));
}

// Tests [@ANCHOR: proc_run_params]
#[test]
fn run_arguments_are_checked_against_the_runner_before_anything_starts() {
    let b = bench_with("read_config", "");
    let inst = b.insts.iter().find(|i| i.name == "procs").unwrap();
    let p = run_params(inst, &json!({"source": "pass"})).unwrap();
    assert_eq!(
        (p.cap, p.wall_s, p.max_calls, p.limits.memory_mb),
        (Tier::ReadConfig, 600.0, 1000, 2048)
    );
    let bad = |a: Value| run_params(inst, &a).err().unwrap();
    assert!(bad(json!({})).contains("source is required"));
    assert!(bad(json!({"source": "  "})).contains("1 byte"));
    assert!(bad(json!({"source": "pass", "language": "lisp"})).contains("not available"));
    assert!(bad(json!({"source": "pass", "args": [1]})).contains("JSON object"));
    assert!(
        bad(json!({"source": "pass", "tier": "full", "confirm_full_tier": true})).contains("above")
    );
    assert!(bad(json!({"source": "pass", "tier": "root"})).contains("not one of"));
    assert!(bad(json!({"source": "pass", "memory_mb": 10})).contains("memory_mb"));
    assert!(bad(json!({"source": "pass", "max_calls": 0})).contains("max_calls"));
    let p = run_params(
        inst,
        &json!({"source": "pass", "tier": "read_only", "wall_s": 5, "label": "x"}),
    )
    .unwrap();
    assert_eq!(
        (p.cap, p.wall_s, p.label.as_str(), p.limits.cpu_s),
        (Tier::ReadOnly, 5.0, "x", 15)
    );
}

// Tests [@ANCHOR: proc_tier_name]
// Tests [@ANCHOR: proc_tier_parse]
#[test]
fn tier_names_round_trip() {
    for t in [Tier::ReadOnly, Tier::ReadConfig, Tier::Full] {
        assert_eq!(tiers::parse_tier(tiers::tier_name(t)).unwrap(), t);
    }
    assert!(tiers::parse_tier("").is_err());
}

// Tests [@ANCHOR: proc_run_tool]
// Tests [@ANCHOR: proc_run_active_in]
// Tests [@ANCHOR: proc_run_count_error]
// Tests [@ANCHOR: proc_view_list]
// Tests [@ANCHOR: proc_store_event_dropped]
// Tests [@ANCHOR: proc_run_start]
// Tests [@ANCHOR: proc_run_supervise]
// Tests [@ANCHOR: proc_run_handle]
// Tests [@ANCHOR: proc_run_exec_call]
// Tests [@ANCHOR: proc_run_finish]
// Tests [@ANCHOR: proc_run_event]
// Tests [@ANCHOR: proc_run_persist]
// Tests [@ANCHOR: proc_run_set_state]
// Tests [@ANCHOR: proc_run_registry]
// Tests [@ANCHOR: proc_run_lookup]
// Tests [@ANCHOR: proc_run_is_active]
// Tests [@ANCHOR: proc_run_clip]
// Tests [@ANCHOR: proc_backend_for]
// Tests [@ANCHOR: proc_cpython_start]
// Tests [@ANCHOR: proc_cpython_send]
// Tests [@ANCHOR: proc_cpython_recv]
// Tests [@ANCHOR: proc_cpython_pump]
// Tests [@ANCHOR: proc_cpython_env]
// Tests [@ANCHOR: proc_cpython_pid]
// Tests [@ANCHOR: proc_cpython_exit_status]
// Tests [@ANCHOR: proc_cpython_command_line]
// Tests [@ANCHOR: proc_tool_index]
// Tests [@ANCHOR: proc_tool_index_json]
// Tests [@ANCHOR: proc_py_main]
// Tests [@ANCHOR: proc_py_bench_error_init]
// Tests [@ANCHOR: proc_py_failure_init]
// Tests [@ANCHOR: proc_py_wire_init]
// Tests [@ANCHOR: proc_py_output_init]
// Tests [@ANCHOR: proc_py_wire_send]
// Tests [@ANCHOR: proc_py_wire_request]
// Tests [@ANCHOR: proc_py_wire_reader]
// Tests [@ANCHOR: proc_py_call]
// Tests [@ANCHOR: proc_py_emit]
// Tests [@ANCHOR: proc_py_log]
// Tests [@ANCHOR: proc_py_step]
// Tests [@ANCHOR: proc_py_result]
// Tests [@ANCHOR: proc_py_execute]
// Tests [@ANCHOR: proc_py_make_bench]
// Tests [@ANCHOR: proc_py_output_write]
// Tests [@ANCHOR: proc_py_output_flush]
// Tests [@ANCHOR: proc_py_output_isatty]
// Tests [@ANCHOR: proc_py_jsonable]
// Tests [@ANCHOR: proc_py_check_cancel]
// Tests [@ANCHOR: proc_py_tools]
// Tests [@ANCHOR: proc_py_deadline]
// Tests [@ANCHOR: proc_store_new_run_dir]
// Tests [@ANCHOR: proc_store_run_dir]
// Tests [@ANCHOR: proc_store_write_json]
// Tests [@ANCHOR: proc_store_read_json]
// Tests [@ANCHOR: proc_store_root]
// Tests [@ANCHOR: proc_store_now]
// Tests [@ANCHOR: proc_store_event_open]
// Tests [@ANCHOR: proc_store_event_append]
// Tests [@ANCHOR: proc_store_event_count]
// Tests [@ANCHOR: proc_store_read_events]
// Tests [@ANCHOR: proc_store_append_stream]
// Tests [@ANCHOR: proc_store_read_stream]
// Tests [@ANCHOR: proc_view_status]
// Tests [@ANCHOR: proc_view_output]
// Tests [@ANCHOR: proc_view_result]
// Tests [@ANCHOR: proc_view_terminal]
#[test]
fn a_procedure_runs_to_the_end_and_everything_it_did_can_be_read_back() {
    let b = bench();
    let source = r#"
import numpy as np
bench.step("measure")
reading = bench.call("bench_example_measure", instrument="gen", count=3)
bench.emit("reading", reading)
bench.log("hello from the procedure")
print("printed " + str(bench.args["n"]))
assert bench.deadline() > 0
assert "bench_example_measure" in bench.tools()
bench.emit("array", np.arange(3) * 1.5)
bench.result({"mean": float(np.mean([1, 2, 3])), "n": bench.args["n"], "tools": len(bench.tools()) > 3})
"#;
    let id = start(&b, source, json!({"args": {"n": 7}, "label": "first"}));
    let r = finish(&b, &id);
    assert_eq!(r["state"], "succeeded", "{r}");
    assert_eq!(r["ok"], json!(true));
    assert_eq!(r["value"]["mean"], json!(2.0));
    assert_eq!(r["value"]["n"], json!(7));
    assert_eq!(r["emitted"]["reading"]["readings"], json!(3));
    assert_eq!(r["emitted"]["array"], json!([0.0, 1.5, 3.0]));
    assert_eq!(r["calls"], json!(1));
    let st = tool(&b, "bench_proc_status", json!({"run": id})).unwrap();
    assert_eq!(
        (
            st["state"].as_str().unwrap(),
            st["label"].as_str().unwrap(),
            st["step"].as_str().unwrap()
        ),
        ("succeeded", "first", "measure")
    );
    assert_eq!(st["last_call"]["tool"], "bench_example_measure");
    assert_eq!(st["events_dropped"], json!(0));
    assert!(st["elapsed_s"].as_f64().unwrap() > 0.0 && st["pid"].as_u64().unwrap() > 1);
    let out = tool(&b, "bench_proc_output", json!({"run": id})).unwrap();
    assert!(
        out["stdout"]["text"]
            .as_str()
            .unwrap()
            .contains("printed 7"),
        "{out}"
    );
    let kinds: Vec<&str> = out["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    for want in ["state", "step", "call", "emit", "log"] {
        assert!(kinds.contains(&want), "{want} in {kinds:?}");
    }
    // The cursor continues where the last read stopped, and a large output is bounded.
    let cursor = out["cursor"].as_u64().unwrap();
    let again = tool(
        &b,
        "bench_proc_output",
        json!({"run": id, "cursor": cursor}),
    )
    .unwrap();
    assert_eq!(again["events"], json!([]));
    let small = tool(
        &b,
        "bench_proc_output",
        json!({"run": id, "max_bytes": 100}),
    )
    .unwrap();
    assert_eq!(small["more_events"], json!(true));
    assert!(small["events"].as_array().unwrap().len() < out["events"].as_array().unwrap().len());
    let l = tool(&b, "bench_proc_list", json!({})).unwrap();
    assert_eq!(l["runs"][0]["run"], json!(id));
    assert_eq!(l["active_here"], json!(0));
    // The measure call really reached the fake device through the dispatcher.
    assert!(tool(&b, "bench_proc_status", json!({"run": "../etc"}))
        .unwrap_err()
        .contains("not a run id"));
    assert!(
        tool(&b, "bench_proc_status", json!({"run": "r00000000000000"}))
            .unwrap_err()
            .contains("no run")
    );
}

// Tests [@ANCHOR: proc_tool_allow]
#[test]
fn a_procedure_cannot_exceed_its_tier_cap_or_the_instruments_tier_or_start_procedures() {
    let b = bench();
    let source = r#"
seen = {}
def attempt(label, tool, **args):
    try:
        bench.call(tool, **args)
        seen[label] = "ok"
    except bench.BenchError as error:
        seen[label] = [error.kind, error.message]
attempt("read", "bench_example_status", instrument="gen")
attempt("full_tool", "bench_example_pulse", instrument="gen", level_v=0.1, pulse_s=0.1, dry_run=True)
attempt("safe_state", "bench_example_safe_state", instrument="gen")
attempt("nested", "bench_proc_run", source="pass")
attempt("unknown", "bench_does_not_exist")
bench.result(seen)
"#;
    let r = run(&b, source, json!({"tier": "read_only"}));
    let seen = &r["value"];
    assert_eq!(seen["read"], "ok", "{r}");
    assert_eq!(seen["full_tool"][0], "tier", "{r}");
    assert!(seen["full_tool"][1]
        .as_str()
        .unwrap()
        .contains("needs tier full; this run's tier cap is read_only"));
    assert_eq!(seen["safe_state"][0], "tier");
    assert_eq!(seen["nested"][0], "policy");
    assert_eq!(seen["unknown"][0], "tool");
    // A cap of read_config lets the read_config tool through to the dispatcher, which applies the instrument's own tier.
    let source = r#"
seen = {}
for instrument in ("gen", "dmm"):
    try:
        bench.call("bench_example_safe_state", instrument=instrument)
        seen[instrument] = "ok"
    except bench.BenchError as error:
        seen[instrument] = error.message
bench.result(seen)
"#;
    let r = run(&b, source, json!({"tier": "read_config"}));
    assert_eq!(r["value"]["gen"], "ok", "{r}");
    assert!(
        r["value"]["dmm"]
            .as_str()
            .unwrap()
            .contains("configured at ReadOnly"),
        "{r}"
    );
    // With confirm_full_tier the full tool is reached, and its own dry_run and confirm rules apply to the procedure too.
    let source = r#"
plan = bench.call("bench_example_pulse", instrument="gen", level_v=0.1, pulse_s=0.1, dry_run=True)
try:
    bench.call("bench_example_pulse", instrument="gen", level_v=0.1, pulse_s=0.1)
    refused = None
except bench.BenchError as error:
    refused = error.message
bench.result({"dry": plan["dry_run"], "refused": refused})
"#;
    let r = run(
        &b,
        source,
        json!({"tier": "full", "confirm_full_tier": true}),
    );
    assert_eq!(r["value"]["dry"], json!(true), "{r}");
    assert!(
        r["value"]["refused"].as_str().unwrap().contains("confirm"),
        "{r}"
    );
}

// Tests [@ANCHOR: proc_py_failure_report]
// Tests [@ANCHOR: proc_py_frame_report]
// Tests [@ANCHOR: proc_py_short]
// Tests [@ANCHOR: proc_py_expect]
#[test]
fn an_uncaught_error_gives_a_report_with_the_locals_of_the_procedures_own_frames_and_the_last_calls(
) {
    let b = bench();
    let source = r#"
def inner(level, big):
    shaped = [level] * 3
    secret_count = len(big)
    bench.expect(level < 1, "level too high", level=level, shaped=shaped)

def outer():
    first = bench.call("bench_example_status", instrument="gen")
    bench.emit("seen", "status")
    bench.call("bench_example_measure", instrument="gen")
    inner(2.5, "x" * 5000)

outer()
"#;
    let r = run(&b, source, json!({}));
    assert_eq!(
        (r["state"].as_str().unwrap(), r["ok"].clone()),
        ("failed", json!(false)),
        "{r}"
    );
    let f = &r["failure"];
    assert_eq!(f["kind"], "exception");
    assert_eq!(f["type"], "ProcedureFailure");
    assert_eq!(f["message"], "level too high");
    assert_eq!(f["context"]["level"], json!(2.5));
    assert!(
        f["traceback"]
            .as_str()
            .unwrap()
            .contains("bench.expect(level < 1"),
        "source line in the traceback: {f}"
    );
    let frames = f["frames"].as_array().unwrap();
    let names: Vec<&str> = frames
        .iter()
        .map(|x| x["function"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["<module>", "outer", "inner"],
        "only the procedure's own frames"
    );
    let inner = &frames[2];
    assert_eq!(inner["locals"]["level"], "2.5");
    assert_eq!(inner["locals"]["shaped"], "[2.5, 2.5, 2.5]");
    assert_eq!(inner["locals"]["secret_count"], "5000");
    assert!(
        inner["locals"]["big"]
            .as_str()
            .unwrap()
            .contains("(5002 characters)"),
        "long values are cut: {inner}"
    );
    assert!(frames[1]["locals"]["first"]
        .as_str()
        .unwrap()
        .contains("EXAMPLE-1000"));
    assert!(!frames[1]["locals"]
        .as_object()
        .unwrap()
        .contains_key("bench"));
    let calls = f["last_calls"].as_array().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["tool"], "bench_example_status");
    assert_eq!(calls[1]["args"]["instrument"], "gen");
    assert_eq!(f["emitted"]["seen"], "status");
    assert_eq!(
        r["emitted"]["seen"], "status",
        "emitted values survive a failure"
    );
    // A syntax error is reported with its line, before anything runs.
    let r = run(&b, "x = 1\nif x:\ny = 2\n", json!({}));
    assert_eq!(r["state"], "failed", "{r}");
    assert_eq!(r["failure"]["type"], "IndentationError");
    assert_eq!(r["failure"]["line"], json!(3));
    // A failed tool call surfaces as a BenchError with the tool and the reason.
    let r = run(
        &b,
        "bench.call('bench_example_measure', instrument='gen', count=99)",
        json!({}),
    );
    assert_eq!(r["failure"]["type"], "BenchError", "{r}");
    assert_eq!(r["failure"]["tool"], "bench_example_measure");
    assert!(
        r["failure"]["message"].as_str().unwrap().contains("count"),
        "{r}"
    );
    assert_eq!(r["call_errors"], json!(1));
}

// Tests [@ANCHOR: proc_py_sleep]
// Tests [@ANCHOR: proc_py_cleanup]
// Tests [@ANCHOR: proc_py_run_cleanups]
// Tests [@ANCHOR: proc_run_request_cancel]
// Tests [@ANCHOR: proc_run_cancel_reason]
// Tests [@ANCHOR: proc_view_cancel]
// Tests [@ANCHOR: proc_cpython_kill]
// Tests [@ANCHOR: proc_cpython_drop]
#[test]
fn cancel_ends_a_run_after_its_cleanup_handlers_have_made_their_tool_calls() {
    let b = bench();
    let source = r#"
def leave_safe():
    bench.call("bench_example_safe_state", instrument="gen")
    bench.emit("cleanup", "ran")
bench.cleanup(leave_safe)
bench.emit("started", True)
while True:
    bench.sleep(0.05)
"#;
    let id = start(&b, source, json!({"tier": "read_config"}));
    let t = Instant::now();
    while tool(&b, "bench_proc_output", json!({"run": id})).unwrap()["emitted"]["started"]
        != json!(true)
    {
        assert!(t.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(20));
    }
    let st = tool(&b, "bench_proc_cancel", json!({"run": id, "wait_s": 20})).unwrap();
    assert_eq!(st["state"], "cancelled", "{st}");
    let r = tool(&b, "bench_proc_result", json!({"run": id})).unwrap();
    assert_eq!(r["emitted"]["cleanup"], "ran", "{r}");
    assert_eq!(r["failure"]["kind"], "cancelled");
    assert_eq!(r["failure"]["reason"], "cancel");
    assert_eq!(
        r["calls"],
        json!(1),
        "the cleanup handler's call went through the dispatcher"
    );
    // A second cancel of a finished run changes nothing.
    assert_eq!(
        tool(&b, "bench_proc_cancel", json!({"run": id})).unwrap()["state"],
        "cancelled"
    );
}

#[test]
fn the_wall_clock_budget_stops_a_run_and_a_program_that_ignores_the_request_is_killed_after_the_grace_time(
) {
    let b = bench();
    let polite = run(
        &b,
        "while True:\n    bench.sleep(0.05)\n",
        json!({"wall_s": 1}),
    );
    assert_eq!(polite["state"], "timed_out", "{polite}");
    assert_eq!(polite["failure"]["reason"], "timeout");
    let t = Instant::now();
    let stubborn = run(
        &b,
        "bench.emit('alive', 1)\nwhile True:\n    pass\n",
        json!({"wall_s": 1}),
    );
    assert_eq!(stubborn["state"], "timed_out", "{stubborn}");
    assert!(
        stubborn["failure"]["message"]
            .as_str()
            .unwrap()
            .contains("killed"),
        "{stubborn}"
    );
    assert_eq!(
        stubborn["emitted"]["alive"],
        json!(1),
        "partial results survive the kill"
    );
    assert!(t.elapsed() < Duration::from_secs(20));
    let st = tool(&b, "bench_proc_status", json!({"run": stubborn["run"]})).unwrap();
    let pid = st["pid"].as_i64().unwrap() as i32;
    // SAFETY: signal 0 only tests whether the process exists.
    assert_ne!(
        unsafe { libc::kill(pid, 0) },
        0,
        "the killed program is gone"
    );
}

// Tests [@ANCHOR: proc_py_make_hook]
// Tests [@ANCHOR: proc_py_open_refusal]
// Tests [@ANCHOR: proc_py_path_inside]
#[test]
fn the_audit_hook_blocks_processes_sockets_devices_and_writes_outside_scratch_and_allows_bench_call(
) {
    let b = bench();
    let outside = std::env::temp_dir().join(format!("proc_hook_outside_{}", std::process::id()));
    let source = format!(
        r#"
import os, socket, subprocess
seen = {{}}
def attempt(label, function):
    try:
        function()
        seen[label] = "allowed"
    except bench.PolicyError as error:
        seen[label] = "blocked"
    except Exception as error:
        seen[label] = "other: " + repr(error)
attempt("subprocess", lambda: subprocess.run(["true"]))
attempt("system", lambda: os.system("true"))
attempt("fork", lambda: os.fork())
attempt("socket", lambda: socket.create_connection(("127.0.0.1", 9)))
attempt("dns", lambda: socket.getaddrinfo("example.invalid", 80))
attempt("device", lambda: open("/dev/ptmx", "rb"))
attempt("write_outside", lambda: open({outside:?}, "w"))
attempt("remove_outside", lambda: os.remove("/etc/hostname"))
attempt("rename_outside", lambda: os.rename("/etc/hostname", "/etc/hostname2"))
attempt("kill", lambda: os.kill(1, 0))
attempt("ctypes", lambda: __import__("ctypes").CDLL("libc.so.6"))
attempt("write_scratch", lambda: open("note.txt", "w").write("x"))
attempt("mkdir_scratch", lambda: os.mkdir("sub"))
attempt("dev_null", lambda: open("/dev/null", "w").write("x"))
attempt("urandom", lambda: open("/dev/urandom", "rb").read(1))
attempt("read_elsewhere", lambda: open("/etc/hostname").read())
attempt("bench_call", lambda: bench.call("bench_example_status", instrument="gen"))
bench.result(seen)
"#
    );
    let r = run(&b, &source, json!({}));
    assert_eq!(r["state"], "succeeded", "{r}");
    let seen = r["value"].as_object().unwrap();
    for blocked in [
        "subprocess",
        "system",
        "fork",
        "socket",
        "dns",
        "device",
        "write_outside",
        "remove_outside",
        "rename_outside",
        "kill",
        "ctypes",
    ] {
        assert_eq!(seen[blocked], "blocked", "{blocked}: {seen:?}");
    }
    for allowed in [
        "write_scratch",
        "mkdir_scratch",
        "dev_null",
        "urandom",
        "read_elsewhere",
        "bench_call",
    ] {
        assert_eq!(seen[allowed], "allowed", "{allowed}: {seen:?}");
    }
    assert!(!outside.exists(), "the blocked write created nothing");
    assert_eq!(r["hook_violations"].as_array().unwrap().len(), 11);
    assert!(r["hook_violations"][0]["msg"]
        .as_str()
        .unwrap()
        .contains("bench.call"));
    let st = tool(&b, "bench_proc_status", json!({"run": r["run"]})).unwrap();
    assert_eq!(st["hook_violations"], json!(11));
    let out = tool(
        &b,
        "bench_proc_output",
        json!({"run": r["run"], "max_bytes": 200000}),
    )
    .unwrap();
    let violations = out["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "violation")
        .count();
    assert_eq!(violations, 11, "each attempt is in the run log");
}

// Tests [@ANCHOR: proc_cpython_limit]
// Tests [@ANCHOR: proc_cpython_user_tasks]
// Tests [@ANCHOR: proc_cpython_no_limits]
#[test]
fn limits_stop_runaway_memory_calls_and_output_and_numpy_scipy_work_inside_them() {
    let b = bench();
    let r = run(
        &b,
        "block = bytearray(3 << 30)\n",
        json!({"memory_mb": 512}),
    );
    assert_eq!(r["state"], "failed", "{r}");
    assert_eq!(r["failure"]["type"], "MemoryError", "{r}");
    let source = r#"
done = 0
try:
    for _ in range(5):
        bench.call("bench_example_status", instrument="gen")
        done += 1
except bench.BenchError as error:
    bench.result({"done": done, "kind": error.kind, "message": error.message})
"#;
    let r = run(&b, source, json!({"max_calls": 3}));
    assert_eq!(r["value"]["done"], json!(3), "{r}");
    assert_eq!(r["value"]["kind"], "budget");
    assert!(r["value"]["message"]
        .as_str()
        .unwrap()
        .contains("budget of 3"));
    let r = run(&b, "print('x' * 2000000)\nimport sys\nprint('e' * 2000000, file=sys.stderr)\nbench.result(1)\n", json!({}));
    assert_eq!(r["state"], "succeeded");
    let dir = b.dir.join("runs").join(r["run"].as_str().unwrap());
    for f in ["stdout.txt", "stderr.txt"] {
        let len = std::fs::metadata(dir.join(f)).unwrap().len();
        assert!(len < store::STREAM_CAP + 200, "{f} is bounded: {len}");
    }
    let st = tool(&b, "bench_proc_status", json!({"run": r["run"]})).unwrap();
    assert!(st["note"].as_str().unwrap().contains("size limit"), "{st}");
    let source = r#"
import numpy as np
from scipy import optimize, signal
t = np.linspace(0, 1, 2000)
y = 3.0 * np.sin(2 * np.pi * 50 * t) + 0.1 * np.random.default_rng(1).standard_normal(t.size)
spectrum = np.abs(np.fft.rfft(y))
peak = float(np.argmax(spectrum))
fit, _ = optimize.curve_fit(lambda x, a: a * np.sin(2 * np.pi * 50 * x), t, y, p0=[1.0])
b_, a_ = signal.butter(4, 0.2)
filtered = signal.lfilter(b_, a_, y)
bench.result({"peak_hz": peak, "amplitude": round(float(fit[0]), 1), "filtered": len(filtered)})
"#;
    let r = run(&b, source, json!({}));
    assert_eq!(r["value"]["peak_hz"], json!(50.0), "{r}");
    assert_eq!(r["value"]["amplitude"], json!(3.0));
    assert_eq!(r["value"]["filtered"], json!(2000));
}

#[test]
fn only_the_configured_number_of_runs_are_active_and_old_runs_are_purged() {
    let b = bench_with("full", r#""max_runs":1,"keep_runs":2"#);
    let first = start(&b, "while True:\n    bench.sleep(0.05)\n", json!({}));
    let e = tool(&b, "bench_proc_run", json!({"source": "pass"})).unwrap_err();
    assert!(
        e.contains("1 runs are already active") && e.contains("limit"),
        "{e}"
    );
    assert_eq!(
        tool(&b, "bench_proc_list", json!({})).unwrap()["active_here"],
        json!(1)
    );
    tool(&b, "bench_proc_cancel", json!({"run": first, "wait_s": 20})).unwrap();
    let mut ids = vec![first.clone()];
    for _ in 0..3 {
        ids.push(
            run(&b, "bench.result(1)", json!({}))["run"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    let listed = tool(&b, "bench_proc_list", json!({"limit": 50})).unwrap();
    let have: Vec<&str> = listed["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["run"].as_str().unwrap())
        .collect();
    assert!(have.len() <= 3, "{have:?}");
    assert!(
        !have.contains(&first.as_str()),
        "the oldest finished run is gone: {have:?}"
    );
    assert!(have.contains(&ids[3].as_str()));
    assert!(tool(&b, "bench_proc_status", json!({"run": first}))
        .unwrap_err()
        .contains("no run"));
}

// Tests [@ANCHOR: proc_store_purge]
// Tests [@ANCHOR: proc_store_list_runs]
#[test]
fn finished_runs_older_than_the_retention_time_are_deleted_when_the_next_run_starts() {
    let b = bench_with("full", r#""retention_days":0.00001"#);
    let old = run(&b, "bench.result(1)", json!({}))["run"]
        .as_str()
        .unwrap()
        .to_string();
    std::thread::sleep(Duration::from_millis(1500));
    let newer = run(&b, "bench.result(2)", json!({}))["run"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(tool(&b, "bench_proc_status", json!({"run": old})).is_err());
    assert_eq!(
        tool(&b, "bench_proc_status", json!({"run": newer})).unwrap()["state"],
        "succeeded"
    );
}

// Tests [@ANCHOR: proc_py_retry]
// Tests [@ANCHOR: proc_py_stats]
// Tests [@ANCHOR: proc_py_series]
// Tests [@ANCHOR: proc_py_checkpoint]
// Tests [@ANCHOR: proc_py_flush_trace]
// Tests [@ANCHOR: proc_py_tracer]
#[test]
fn the_helpers_retry_stats_series_checkpoint_and_trace_do_what_the_manual_says() {
    let b = bench();
    let source = r#"
tries = []
def flaky():
    tries.append(1)
    if len(tries) < 3:
        raise bench.BenchError("t", "not yet")
    return "ok"
got = bench.retry(flaky, tries=3, delay=0.01)
try:
    bench.retry(lambda: bench.call("bench_nope"), tries=2, delay=0.01)
    exhausted = None
except bench.BenchError as error:
    exhausted = error.kind
stats = bench.stats([1, 2, 3, 4])
failures = {"left": 3}
cycled = []
def measure(i):
    if failures["left"] > 0:
        failures["left"] -= 1
        raise bench.BenchError("sweep", "no answer")
    return 10.0 + i
series = bench.series("sweep", 4, measure, retries=1, cycle=lambda: cycled.append(1), cycle_after=2)
bench.checkpoint("after-series", {"values": series["values"]})
try:
    bench.series("bad", 3, lambda i: (_ for _ in ()).throw(bench.BenchError("x", "y")), retries=0, max_failed=1)
except bench.ProcedureFailure as error:
    too_many = [str(error), len(error.context["failed"])]
bench.result({"got": got, "exhausted": exhausted, "stats": stats, "series": series, "cycled": len(cycled), "too_many": too_many})
"#;
    let r = run(&b, source, json!({"trace": true}));
    assert_eq!(r["state"], "succeeded", "{r}");
    let v = &r["value"];
    assert_eq!(v["got"], "ok");
    assert_eq!(v["exhausted"], "tool");
    assert_eq!(v["stats"]["n"], json!(4));
    assert_eq!(v["stats"]["mean"], json!(2.5));
    assert!((v["stats"]["std"].as_f64().unwrap() - 1.2909944487358056).abs() < 1e-12);
    assert_eq!(v["stats"]["spread"], json!(3.0));
    // Three failures in a row: the first point is tried twice (fail, fail -> a power cycle), then fails a third time and is
    // listed as failed; the next three points measure.
    assert_eq!(v["cycled"], json!(1), "{v}");
    assert_eq!(v["series"]["failed"].as_array().unwrap().len(), 1, "{v}");
    assert_eq!(v["series"]["values"], json!([11.0, 12.0, 13.0]));
    assert_eq!(v["series"]["stats"]["n"], json!(3));
    assert_eq!(v["too_many"][1], json!(2));
    assert!(v["too_many"][0]
        .as_str()
        .unwrap()
        .contains("more than 1 points failed"));
    let progress = &r["emitted"]["sweep.progress"];
    assert_eq!(
        (
            progress["done"].clone(),
            progress["of"].clone(),
            progress["cycles"].clone()
        ),
        (json!(4), json!(4), json!(1))
    );
    let cp = b
        .dir
        .join("runs")
        .join(r["run"].as_str().unwrap())
        .join("checkpoints/after-series.json");
    assert_eq!(
        store::read_json(&cp).unwrap()["values"],
        json!([11.0, 12.0, 13.0])
    );
    let st = tool(&b, "bench_proc_status", json!({"run": r["run"]})).unwrap();
    assert!(st["line"].as_u64().unwrap() > 0, "trace: {st}");
    let out = tool(
        &b,
        "bench_proc_output",
        json!({"run": r["run"], "max_bytes": 200000}),
    )
    .unwrap();
    assert!(out["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "trace"));
    assert!(out["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "log"
            && e["msg"]
                .as_str()
                .unwrap()
                .contains("cycling after 2 failures")));
}

// Tests [@ANCHOR: proc_py_check_source]
// Tests [@ANCHOR: proc_cpython_check]
#[test]
fn dry_run_checks_the_source_and_lists_the_tools_with_their_tiers_and_starts_nothing() {
    let b = bench_with("read_config", "");
    let source = "bench.call('bench_example_status')\nbench.call('bench_example_pulse')\nbench.call('bench_nope')\nbench.call(name)\n";
    let r = tool(
        &b,
        "bench_proc_run",
        json!({"source": source, "dry_run": true}),
    )
    .unwrap();
    assert_eq!(
        (r["dry_run"].clone(), r["started"].clone()),
        (json!(true), json!(false)),
        "{r}"
    );
    let calls = r["tool_calls"].as_array().unwrap();
    assert_eq!(calls.len(), 3, "only literal names are listed: {calls:?}");
    let by = |n: &str| calls.iter().find(|c| c["tool"] == n).unwrap();
    assert_eq!(by("bench_example_status")["allowed"], json!(true));
    assert_eq!(by("bench_example_status")["tier"], "read_only");
    assert_eq!(by("bench_example_pulse")["allowed"], json!(false));
    assert_eq!(by("bench_nope")["allowed"], json!(false));
    assert_eq!(
        tool(&b, "bench_proc_list", json!({})).unwrap()["runs"],
        json!([])
    );
    let r = tool(
        &b,
        "bench_proc_run",
        json!({"source": "def (:\n", "dry_run": true}),
    )
    .unwrap();
    assert_eq!(r["source"]["ok"], json!(false), "{r}");
    assert_eq!(r["source"]["line"], json!(1));
}

// Tests [@ANCHOR: proc_cpython_command_line]
#[test]
fn the_command_line_has_the_interpreter_flags_and_the_optional_systemd_wrapper() {
    let limits = backend::Limits {
        memory_mb: 512,
        cpu_s: 5,
        files: 64,
        file_mb: 1,
    };
    let plain = cpython::CPython {
        python: "/usr/bin/python3".into(),
        wrapper: "none".into(),
    };
    let v = cpython::command_line(
        &plain,
        std::path::Path::new("/r/bench_runtime.py"),
        &["/r"],
        &limits,
    );
    assert_eq!(v[..3], ["/usr/bin/python3", "-I", "-B"]);
    assert_eq!(v.last().unwrap(), "/r");
    let wrapped = cpython::CPython {
        wrapper: "systemd-run".into(),
        ..plain
    };
    let v = cpython::command_line(
        &wrapped,
        std::path::Path::new("/r/bench_runtime.py"),
        &["/r"],
        &limits,
    );
    assert_eq!(v[0], "systemd-run");
    assert!(v.contains(&"MemoryMax=512M".to_string()) && v.contains(&"--scope".to_string()));
    assert!(v.iter().position(|a| a == "/usr/bin/python3") > v.iter().position(|a| a == "--scope"));
}

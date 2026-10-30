// SPDX-License-Identifier: AGPL-3.0-or-later
//! THE PROCEDURE RUNNER. A procedure is a program, written by the agent or saved earlier, that calls bench tools in loops
//! (a sweep series with retries and a power cycle after failures, a soak round, a calibration with operator steps) and
//! runs next to the server, so a long job does not pass through the agent's context step by step. The agent starts it
//! with `bench_proc_run`, gets a run id at once and polls `bench_proc_status`, `bench_proc_output` and `bench_proc_result`.
//!
//! Every tool call a procedure makes goes through the server's normal dispatcher (`crate::call`): the instrument's tier,
//! the tool's confirmation and attestation arguments, reserved-device rules and logging apply exactly as for an agent's
//! call, and a procedure can never call a tool above the tier cap it was started with. The first language is CPython
//! (`cpython.rs`, with numpy and scipy); the language interface is `backend.rs`.
//!
//! THE LIMITS AND THE AUDIT HOOK ARE NOT A SANDBOX (see `cpython.rs`). The entry in the bench config:
//! `{"driver": "procedure", "tier": "read_config"}`; the instrument's tier is the highest tier cap a run may have.

mod backend;
mod cpython;
mod run;
mod store;
mod tiers;
mod view;

#[cfg(test)]
mod tests;

use crate::config::{Instrument, Kind, Tier};
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

/// The `procedure` settings of an instrument entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The managed directory (runs, saved procedures); `procedures` under the bench output directory when omitted.
    pub dir: Option<PathBuf>,
    /// Runs at once.
    pub max_runs: usize,
    /// Finished runs are deleted after this many days.
    pub retention_days: f64,
    /// At most this many finished runs are kept.
    pub keep_runs: usize,
    /// The interpreter of the Python backend.
    pub python: String,
    /// Seconds a cancelled program has for its cleanup handlers before it is killed.
    pub grace_s: f64,
    /// The longest `wall_s` a run may ask for.
    pub max_wall_s: f64,
    /// The largest `memory_mb` a run may ask for.
    pub max_memory_mb: u64,
    /// `none` or `systemd-run` (each program in a transient user scope with memory and task limits).
    pub wrapper: String,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            dir: None,
            max_runs: 2,
            retention_days: 14.0,
            keep_runs: 200,
            python: "/usr/bin/python3".into(),
            grace_s: 5.0,
            max_wall_s: 86400.0,
            max_memory_mb: 16384,
            wrapper: "none".into(),
        }
    }
}

/// Reads the settings of a `procedure` entry; a bad value is refused when the config file is read.
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: proc_config]
    let mut cfg = Config::default();
    let num = |k: &str, lo: f64, hi: f64| -> Result<Option<f64>, String> {
        match c.get(k) {
            None => Ok(None),
            Some(v) => {
                let x = v
                    .as_f64()
                    .filter(|x| x.is_finite() && (lo..=hi).contains(x));
                x.map(Some)
                    .ok_or_else(|| format!("{k} must be a number from {lo} to {hi}"))
            }
        }
    };
    if let Some(d) = c.get("dir") {
        let s = d
            .as_str()
            .filter(|s| s.starts_with('/'))
            .ok_or("dir must be an absolute path")?;
        cfg.dir = Some(PathBuf::from(s));
    }
    if let Some(x) = num("max_runs", 1.0, 16.0)? {
        cfg.max_runs = x as usize;
    }
    if let Some(x) = num("retention_days", 0.0, 3650.0)? {
        cfg.retention_days = x;
    }
    if let Some(x) = num("keep_runs", 1.0, 100000.0)? {
        cfg.keep_runs = x as usize;
    }
    if let Some(x) = num("grace_s", 0.5, 120.0)? {
        cfg.grace_s = x;
    }
    if let Some(x) = num("max_wall_s", 1.0, 31_536_000.0)? {
        cfg.max_wall_s = x;
    }
    if let Some(x) = num("max_memory_mb", 256.0, 1_048_576.0)? {
        cfg.max_memory_mb = x as u64;
    }
    if let Some(p) = c.get("python") {
        cfg.python = p
            .as_str()
            .filter(|s| s.starts_with('/'))
            .ok_or("python must be an absolute path")?
            .into();
    }
    if let Some(w) = c.get("wrapper") {
        match w.as_str() {
            Some(s @ ("none" | "systemd-run")) => cfg.wrapper = s.into(),
            _ => return Err("wrapper must be \"none\" or \"systemd-run\"".into()),
        }
    }
    Ok(cfg)
}

pub struct Spec {
    pub name: &'static str,
    pub tier: Tier,
    pub desc: &'static str,
    pub props: &'static str,
    pub required: &'static [&'static str],
}

/// Every tool of the runner.
pub const SPECS: &[Spec] = &[
    Spec {
        name: "bench_proc_run",
        tier: Tier::ReadConfig,
        desc: "PROCEDURE RUNNER: start a program that calls bench tools in loops and returns a run id at once; poll bench_proc_status, bench_proc_output and bench_proc_result. `source` is Python (numpy and scipy are importable) using the injected `bench` module: bench.call(tool, **args) goes through this server's normal dispatcher, so tiers, confirmations and attestations apply as for your own calls, and never above the `tier` cap of the run (default read_config; full needs confirm_full_tier true from the person at the bench). The run has a wall-clock budget, a tool-call budget and a memory limit; it is killed at the end of them or on bench_proc_cancel, after its cleanup handlers had a grace time. THE AUDIT HOOK THAT BLOCKS SUBPROCESS, SOCKET, DEVICE FILES AND WRITES OUTSIDE ITS SCRATCH DIRECTORY IS NOT A SANDBOX. dry_run compiles the source and lists the tools it calls, and starts nothing.",
        props: r#""instrument":{"type":"string","description":"the procedure runner entry of the bench config; may be left out when there is one"},"source":{"type":"string","description":"the Python source of the procedure (at most 256 KiB)"},"args":{"type":"object","description":"JSON arguments, available to the program as bench.args"},"language":{"type":"string","enum":["python"],"description":"default python; the only language so far"},"label":{"type":"string","description":"a short name for the run, shown in lists"},"tier":{"type":"string","enum":["read_only","read_config","full"],"description":"the highest tier of tool the run may call; default read_config, never above the runner entry's own tier"},"confirm_full_tier":{"type":"boolean","description":"true: the person at the bench agrees that this run may call full-tier tools"},"wall_s":{"type":"number","description":"wall-clock budget in seconds, default 600"},"max_calls":{"type":"integer","description":"tool-call budget, default 1000"},"memory_mb":{"type":"integer","description":"address-space limit in MiB, default 2048"},"cpu_s":{"type":"integer","description":"CPU-time limit in seconds, default the wall budget plus 10"},"trace":{"type":"boolean","description":"record the line numbers the program reaches (slower)"},"dry_run":{"type":"boolean","description":"check the source and list the tools it calls; start nothing"}"#,
        required: &["source"],
    },
    Spec {
        name: "bench_proc_status",
        tier: Tier::ReadOnly,
        desc: "PROCEDURE RUNNER: the state of a run (starting, running, cancelling, succeeded, failed, cancelled, timed_out, lost), elapsed time, the current step or last tool call, the counters (calls, errors, hook violations) and the limits. Changes nothing.",
        props: r#""instrument":{"type":"string","description":"the procedure runner entry of the bench config; may be left out when there is one"},"run":{"type":"string","description":"the run id returned by bench_proc_run"}"#,
        required: &["run"],
    },
    Spec {
        name: "bench_proc_output",
        tier: Tier::ReadOnly,
        desc: "PROCEDURE RUNNER: the progress events of a run after `cursor` (emitted values, log lines, tool calls, steps), the stdout and stderr text after the given byte offsets, and the latest emitted values. Each part is bounded by max_bytes; pass back the returned cursor and offsets to continue. Changes nothing.",
        props: r#""instrument":{"type":"string","description":"the procedure runner entry of the bench config; may be left out when there is one"},"run":{"type":"string","description":"the run id"},"cursor":{"type":"integer","description":"the event number returned last time; default 0 (from the start)"},"stdout_from":{"type":"integer","description":"byte offset in the standard output, default 0"},"stderr_from":{"type":"integer","description":"byte offset in the error output, default 0"},"max_bytes":{"type":"integer","description":"bound for each part, default 20000, at most 200000"}"#,
        required: &["run"],
    },
    Spec {
        name: "bench_proc_result",
        tier: Tier::ReadOnly,
        desc: "PROCEDURE RUNNER: the final value of a finished run, or its structured failure report (error, traceback, the local variables of every frame in the procedure's own code, the last tool calls with arguments and results, the emitted values). For a run still going it returns what has been emitted so far. Changes nothing.",
        props: r#""instrument":{"type":"string","description":"the procedure runner entry of the bench config; may be left out when there is one"},"run":{"type":"string","description":"the run id"}"#,
        required: &["run"],
    },
    Spec {
        name: "bench_proc_cancel",
        tier: Tier::ReadOnly,
        desc: "PROCEDURE RUNNER: ask a run to stop. Its cleanup handlers run for the grace time, then it is killed. Optionally wait for it to end. Needs no more than the lowest tier, because it only stops work.",
        props: r#""instrument":{"type":"string","description":"the procedure runner entry of the bench config; may be left out when there is one"},"run":{"type":"string","description":"the run id"},"wait_s":{"type":"number","description":"seconds to wait for the run to end, default 0, at most 60"}"#,
        required: &["run"],
    },
    Spec {
        name: "bench_proc_list",
        tier: Tier::ReadOnly,
        desc: "PROCEDURE RUNNER: the runs in the managed directory, newest first, with their states, and how many are active. Changes nothing.",
        props: r#""instrument":{"type":"string","description":"the procedure runner entry of the bench config; may be left out when there is one"},"limit":{"type":"integer","description":"at most this many runs, default 20"}"#,
        required: &[],
    },
];

/// The runner entry a call is for: the named one, or the only one.
fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    // [@ANCHOR: proc_instrument]
    let ours: Vec<&Instrument> = insts.iter().filter(|i| i.kind == Kind::Procedure).collect();
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return ours
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no procedure runner named {n:?}"));
    }
    match ours.as_slice() {
        [one] => Ok(one),
        [] => Err("no procedure runner is configured".into()),
        many => Err(format!(
            "name the instrument: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The tools to list: an entry of `SPECS` appears when a runner is configured at or above its tier.
pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    // [@ANCHOR: proc_tool_defs]
    SPECS
        .iter()
        .filter(|s| insts.iter().any(|i| i.kind == Kind::Procedure && i.tier >= s.tier))
        .map(|s| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(&format!("{{{}}}", s.props)).unwrap(), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[{:?}] {}", s.tier, s.desc), Arc::new(schema))
        })
        .collect()
}

/// A number argument: absent gives `default`; present it must be finite and in `lo..=hi`.
fn number(args: &Value, key: &str, default: f64, lo: f64, hi: f64) -> Result<f64, String> {
    // [@ANCHOR: proc_number_arg]
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => match v.as_f64() {
            Some(x) if x.is_finite() && (lo..=hi).contains(&x) => Ok(x),
            _ => Err(format!("{key} must be a number from {lo} to {hi}")),
        },
    }
}

/// A true/false argument that may be absent (false).
fn flag(args: &Value, key: &str) -> Result<bool, String> {
    // [@ANCHOR: proc_flag_arg]
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{key} must be true or false")),
    }
}

/// The parameters of a run from the arguments of `bench_proc_run`, every one checked against the runner's limits.
fn run_params(inst: &Instrument, args: &Value) -> Result<run::Params, String> {
    // [@ANCHOR: proc_run_params]
    let cfg = &inst.procedure;
    let source = args
        .get("source")
        .and_then(|s| s.as_str())
        .ok_or("source is required and must be text")?;
    if source.trim().is_empty() || source.len() > 262_144 {
        return Err("source must be 1 byte to 256 KiB of Python".into());
    }
    match args.get("language").and_then(|l| l.as_str()) {
        None | Some("python") => {}
        Some(o) => return Err(format!("language {o:?} is not available (python)")),
    }
    let arguments = match args.get("args") {
        None | Some(Value::Null) => json!({}),
        Some(v @ Value::Object(_)) if v.to_string().len() <= 65536 => v.clone(),
        Some(_) => return Err("args must be a JSON object of at most 64 KiB".into()),
    };
    let cap = match args.get("tier").and_then(|t| t.as_str()) {
        Some(t) => tiers::parse_tier(t)?,
        None if inst.tier < Tier::ReadConfig => inst.tier,
        None => Tier::ReadConfig,
    };
    if cap > inst.tier {
        return Err(format!(
            "tier {} is above the {:?} tier of the procedure runner {:?}",
            tiers::tier_name(cap),
            inst.tier,
            inst.name
        ));
    }
    if cap == Tier::Full && !flag(args, "confirm_full_tier")? {
        return Err("a run with tier full needs confirm_full_tier true, and that must come from the person at the bench: the run may call tools that drive outputs".into());
    }
    let wall_s = number(
        args,
        "wall_s",
        600.0f64.min(cfg.max_wall_s),
        1.0,
        cfg.max_wall_s,
    )?;
    let memory_mb = number(
        args,
        "memory_mb",
        2048.0f64.min(cfg.max_memory_mb as f64),
        256.0,
        cfg.max_memory_mb as f64,
    )? as u64;
    let label = args
        .get("label")
        .and_then(|l| l.as_str())
        .unwrap_or("")
        .chars()
        .take(100)
        .collect();
    Ok(run::Params {
        source: source.to_string(),
        args: arguments,
        label,
        cap,
        wall_s,
        max_calls: number(args, "max_calls", 1000.0, 1.0, 1_000_000.0)? as u64,
        limits: backend::Limits {
            memory_mb,
            cpu_s: number(
                args,
                "cpu_s",
                wall_s.ceil() + 10.0,
                1.0,
                cfg.max_wall_s + 10.0,
            )? as u64,
            files: 256,
            file_mb: 64,
        },
        trace: flag(args, "trace")?,
    })
}

/// The backend for a language.
fn backend_for(cfg: &Config) -> Arc<dyn backend::Backend> {
    // [@ANCHOR: proc_backend_for]
    Arc::new(cpython::CPython {
        python: cfg.python.clone(),
        wrapper: cfg.wrapper.clone(),
    })
}

/// `bench_proc_run`: checks, then either reports the plan (`dry_run`) or starts the run.
fn run_tool(insts: &[Instrument], inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: proc_run_tool]
    let params = run_params(inst, args)?;
    let cfg = &inst.procedure;
    let backend = backend_for(cfg);
    if flag(args, "dry_run")? {
        let tmp = store::root(cfg).join("check");
        std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let checked = backend.check(&params.source, &tmp);
        let _ = std::fs::remove_dir_all(&tmp);
        let checked = checked?;
        let index = tiers::ToolIndex::build(insts, params.cap);
        let calls: Vec<Value> = checked["calls"]
            .as_array()
            .map(|a| a.iter().filter_map(|n| n.as_str()).collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .map(|n| match index.allow(n, params.cap, insts) {
                Ok(t) => json!({"tool": n, "tier": tiers::tier_name(t), "allowed": true}),
                Err((_, why)) => json!({"tool": n, "allowed": false, "why": why}),
            })
            .collect();
        return Ok(
            json!({"dry_run": true, "started": false, "source": checked, "tool_calls": calls,
            "tier_cap": tiers::tier_name(params.cap), "wall_s": params.wall_s, "max_calls": params.max_calls,
            "memory_mb": params.limits.memory_mb}),
        );
    }
    if run::active_in(&store::root(cfg)) >= cfg.max_runs {
        return Err(format!(
            "{} runs are already active (the limit of {:?} is {}); wait for one or cancel it with bench_proc_cancel",
            run::active_in(&store::root(cfg)),
            inst.name,
            cfg.max_runs
        ));
    }
    let cap = tiers::tier_name(params.cap);
    let (wall_s, max_calls) = (params.wall_s, params.max_calls);
    let id = run::start(cfg, insts, backend, params)?;
    Ok(
        json!({"run": id, "state": "running", "tier_cap": cap, "wall_s": wall_s, "max_calls": max_calls,
        "next": "poll bench_proc_status, then bench_proc_output with the returned cursor, then bench_proc_result"}),
    )
}

/// Runs a procedure tool.
pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: proc_call]
    let spec = SPECS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("unknown tool {name}"))?;
    let inst = instrument(insts, args)?;
    if inst.tier < spec.tier {
        return Err(format!(
            "{name} needs tier {:?}; {:?} is configured at {:?}",
            spec.tier, inst.name, inst.tier
        ));
    }
    let cfg = &inst.procedure;
    let run_id = || {
        args.get("run")
            .and_then(|r| r.as_str())
            .ok_or("run is required (the id from bench_proc_run)".to_string())
    };
    match name {
        "bench_proc_run" => run_tool(insts, inst, args),
        "bench_proc_status" => view::status(cfg, run_id()?),
        "bench_proc_output" => {
            let max = number(args, "max_bytes", 20000.0, 100.0, 200_000.0)? as usize;
            let cursor = number(args, "cursor", 0.0, 0.0, 1e12)? as u64;
            let (out, err) = (
                number(args, "stdout_from", 0.0, 0.0, 1e12)? as u64,
                number(args, "stderr_from", 0.0, 0.0, 1e12)? as u64,
            );
            view::output(cfg, run_id()?, cursor, out, err, max)
        }
        "bench_proc_result" => view::result(cfg, run_id()?),
        "bench_proc_cancel" => {
            view::cancel(cfg, run_id()?, number(args, "wait_s", 0.0, 0.0, 60.0)?)
        }
        "bench_proc_list" => Ok(view::list(
            cfg,
            number(args, "limit", 20.0, 1.0, 1000.0)? as usize,
        )),
        _ => Err(format!("{name} is not implemented")),
    }
}

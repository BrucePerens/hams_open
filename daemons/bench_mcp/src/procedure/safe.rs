// SPDX-License-Identifier: AGPL-3.0-or-later
//! The safe-state routine. When a run ends other than by success (cancel, time-out, an error, a crash, a server shutdown, or a
//! server that died and was found lost by the next one) the instruments it changed are put in a safe state through the normal
//! dispatcher: generator off, PTT released at minimum power, rotator stopped, scope running, supply output off.
//!
//! A tool registers its safe-state action by having a row in `RULES`: the tools that make it necessary (with the argument
//! condition that makes a call actually change something) and the tool that undoes it. The calls a run makes are recorded
//! BEFORE they are dispatched (`touched.json` in the run's directory), so the routine knows what to undo even after a crash of
//! the server. The supply output is switched off only if the run switched it on and the runner's `safe_state_psu_off` allows it.
//! A safe state that cannot be confirmed is reported loudly in the result and never silently dropped.

use super::{store, Config};
use crate::config::Instrument;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

pub(super) struct Rule {
    /// Tools whose call makes the safe state necessary.
    pub triggers: &'static [&'static str],
    /// The call only counts when this boolean argument is true.
    arg_true: Option<&'static str>,
    /// The call only counts when this argument has one of these values.
    arg_in: Option<(&'static str, &'static [&'static str])>,
    /// The tool that puts the instrument in its safe state, and an argument it needs.
    pub safe_tool: &'static str,
    safe_arg: Option<(&'static str, &'static str)>,
    /// The supply rule: applies only when the runner's config allows it.
    psu_gate: bool,
}

pub(super) const RULES: &[Rule] = &[
    Rule {
        triggers: &["bench_psu_output"],
        arg_true: Some("on"),
        arg_in: None,
        safe_tool: "bench_psu_safe_state",
        safe_arg: None,
        psu_gate: true,
    },
    Rule {
        triggers: &[
            "bench_vna_generator_on",
            "bench_vna_generator_am",
            "bench_vna_generator_fm",
        ],
        arg_true: None,
        arg_in: None,
        safe_tool: "bench_vna_generator_off",
        safe_arg: None,
        psu_gate: false,
    },
    Rule {
        triggers: &["bench_example_pulse"],
        arg_true: None,
        arg_in: None,
        safe_tool: "bench_example_safe_state",
        safe_arg: None,
        psu_gate: false,
    },
    Rule {
        triggers: &["bench_hamlib_tx_key_test", "bench_hamlib_tx_power_sweep"],
        arg_true: None,
        arg_in: None,
        safe_tool: "bench_hamlib_safe_state",
        safe_arg: None,
        psu_gate: false,
    },
    Rule {
        triggers: &["bench_rotator_point", "bench_rotator_park"],
        arg_true: None,
        arg_in: None,
        safe_tool: "bench_rotator_stop",
        safe_arg: None,
        psu_gate: false,
    },
    Rule {
        triggers: &["bench_set"],
        arg_true: None,
        arg_in: Some(("trigger_mode", &["STOP", "SINGLE"])),
        safe_tool: "bench_set",
        safe_arg: Some(("trigger_mode", "AUTO")),
        psu_gate: false,
    },
];

/// The safe-state call a tool call makes necessary, if any: `{"tool", "args", "trigger"}`. A `dry_run` call changes nothing.
pub fn action_for(tool: &str, args: &Value, cfg: &Config) -> Option<Value> {
    // [@ANCHOR: proc_safe_action_for]
    if args.get("dry_run").and_then(|d| d.as_bool()) == Some(true) {
        return None;
    }
    let rule = RULES.iter().find(|r| r.triggers.contains(&tool))?;
    if rule.psu_gate && !cfg.safe_state_psu_off {
        return None;
    }
    if let Some(key) = rule.arg_true {
        if args.get(key).and_then(|v| v.as_bool()) != Some(true) {
            return None;
        }
    }
    if let Some((key, values)) = rule.arg_in {
        if !args
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|v| values.contains(&v))
        {
            return None;
        }
    }
    let mut safe_args = serde_json::Map::new();
    if let Some(i) = args.get("instrument") {
        safe_args.insert("instrument".into(), i.clone());
    }
    if let Some((k, v)) = rule.safe_arg {
        safe_args.insert(k.into(), json!(v));
    }
    Some(json!({"tool": rule.safe_tool, "args": Value::Object(safe_args), "trigger": tool}))
}

/// Adds `action` to the run's record of what to undo (one entry per safe-state call and instrument) and saves it.
pub fn record(dir: &Path, touched: &mut Vec<Value>, action: Value) {
    // [@ANCHOR: proc_safe_record]
    let same = |a: &Value| a["tool"] == action["tool"] && a["args"] == action["args"];
    if !touched.iter().any(same) {
        touched.push(action);
        let _ = store::write_json(&dir.join("touched.json"), &Value::Array(touched.clone()));
    }
}

/// Runs the recorded safe-state calls through the dispatcher (each tried up to three times) and returns what happened to each.
pub fn run(insts: &[Instrument], touched: &[Value]) -> Vec<Value> {
    // [@ANCHOR: proc_safe_run]
    let mut results = Vec::new();
    for action in touched.iter().rev() {
        let tool = action["tool"].as_str().unwrap_or("");
        let mut outcome =
            json!({"tool": tool, "args": action["args"], "trigger": action["trigger"]});
        for attempt in 1..=3 {
            let r = crate::call(insts, tool, &action["args"]);
            outcome["attempts"] = json!(attempt);
            match r {
                Ok(v) => {
                    outcome["ok"] = json!(true);
                    outcome["result"] = super::run::clip(&v);
                    break;
                }
                Err(e) => {
                    outcome["ok"] = json!(false);
                    outcome["error"] = json!(e);
                    if attempt < 3 {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            }
        }
        results.push(outcome);
    }
    results
}

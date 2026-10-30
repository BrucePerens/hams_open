// SPDX-License-Identifier: AGPL-3.0-or-later
//! The read side of the procedure tools: status, output since a cursor, the final result, the list, and cancel. They read
//! the managed directory, so they answer the same for a live run, a finished one and one left by a server that restarted.

use super::run::{self, lookup};
use super::store;
use super::Config;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn terminal(state: &str) -> bool {
    // [@ANCHOR: proc_view_terminal]
    !matches!(
        state,
        "starting" | "running" | "waiting_operator" | "cancelling"
    )
}

/// The record of a run with the elapsed time filled in.
pub fn status(cfg: &Config, id: &str) -> Result<Value, String> {
    // [@ANCHOR: proc_view_status]
    let dir = store::run_dir(cfg, id)?;
    let mut st = store::read_json(&dir.join("state.json"))?;
    let started = st["started_unix"].as_f64().unwrap_or(0.0);
    let end = st["ended_unix"].as_f64().unwrap_or_else(store::now_unix);
    st["elapsed_s"] = json!(((end - started) * 1000.0).round() / 1000.0);
    st["finished"] = json!(terminal(st["state"].as_str().unwrap_or("")));
    Ok(st)
}

/// Events after `cursor`, and the new stdout and stderr text after the given byte offsets, each within `max_bytes`.
pub fn output(
    cfg: &Config,
    id: &str,
    cursor: u64,
    out_from: u64,
    err_from: u64,
    max_bytes: usize,
) -> Result<Value, String> {
    // [@ANCHOR: proc_view_output]
    let dir = store::run_dir(cfg, id)?;
    let st = status(cfg, id)?;
    let (events, next, more) = store::read_events(&dir, cursor, max_bytes)?;
    let (out, out_next, out_more) =
        store::read_stream(&dir.join("stdout.txt"), out_from, max_bytes);
    let (err, err_next, err_more) =
        store::read_stream(&dir.join("stderr.txt"), err_from, max_bytes);
    Ok(json!({
        "run": id, "state": st["state"], "finished": st["finished"],
        "events": events, "cursor": next, "more_events": more,
        "stdout": {"text": out, "next": out_next, "more": out_more},
        "stderr": {"text": err, "next": err_next, "more": err_more},
        "emitted": store::read_json(&dir.join("emitted.json")).unwrap_or_else(|_| json!({})),
    }))
}

/// The final value and failure report of a finished run; for a run still going, its state and what it has emitted so far.
pub fn result(cfg: &Config, id: &str) -> Result<Value, String> {
    // [@ANCHOR: proc_view_result]
    let dir = store::run_dir(cfg, id)?;
    let st = status(cfg, id)?;
    if let Ok(mut r) = store::read_json(&dir.join("result.json")) {
        r["finished"] = json!(true);
        r["stderr_tail"] = json!(store::read_stream(&dir.join("stderr.txt"), 0, 2000).0);
        return Ok(r);
    }
    Ok(json!({
        "run": id, "state": st["state"], "finished": st["finished"], "ok": false, "value": null,
        "failure": if st["finished"] == json!(true) { json!({"kind": st["state"], "message": st["note"]}) } else { Value::Null },
        "emitted": store::read_json(&dir.join("emitted.json")).unwrap_or_else(|_| json!({})),
        "note": "no result yet: the run has not ended, or it ended before a result could be written",
    }))
}

/// Asks a live run to stop and optionally waits (up to `wait_s`) for it to end.
pub fn cancel(cfg: &Config, id: &str, wait_s: f64) -> Result<Value, String> {
    // [@ANCHOR: proc_view_cancel]
    store::run_dir(cfg, id)?;
    if let Some(ctx) = lookup(id) {
        ctx.request_cancel("cancel");
    }
    let t = Instant::now();
    loop {
        let st = status(cfg, id)?;
        if st["finished"] == json!(true) || t.elapsed() >= Duration::from_secs_f64(wait_s) {
            return Ok(st);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Runs on disk, newest first.
pub fn list(cfg: &Config, limit: usize) -> Value {
    // [@ANCHOR: proc_view_list]
    json!({"runs": store::list_runs(cfg, limit), "active_here": run::active_in(&store::root(cfg))})
}

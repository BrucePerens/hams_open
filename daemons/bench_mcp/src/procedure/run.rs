// SPDX-License-Identifier: AGPL-3.0-or-later
//! One run: its record, its supervisor thread and the host side of the run protocol. The supervisor owns the backend's
//! session. It forwards the program's tool calls to bench_mcp's normal dispatcher (`crate::call`) after the policy checks
//! (not a procedure tool, within the tier cap, within the call budget), keeps the record and the event list, enforces the
//! wall-clock budget and the cancel request (ask the program to stop, give its cleanup handlers the grace time, then kill),
//! and writes the final state and result.

use super::backend::{Backend, Incoming, Limits, Session, SpawnSpec};
use super::store::{self, EventLog};
use super::tiers::{tier_name, ToolIndex};
use super::Config;
use super::{locks, safe};
use crate::config::{Instrument, Tier};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs::File;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Tool calls a cleanup handler may still make after the budget is used up.
const CLEANUP_CALLS: u64 = 25;
/// Calls kept for the failure report.
const LAST_CALLS: usize = 10;
/// Longest text of a value in an event or in the failure report, characters.
const CLIP: usize = 2000;
/// The largest reply to a tool call that is passed to a program, bytes.
const MAX_REPLY: usize = 8 << 20;

/// The parameters of a run after validation.
pub struct Params {
    pub source: String,
    pub args: Value,
    pub label: String,
    pub cap: Tier,
    pub wall_s: f64,
    pub max_calls: u64,
    pub limits: Limits,
    pub trace: bool,
}

/// The persisted record of a run (`state.json`).
struct Rec {
    state: String,
    label: String,
    cap: Tier,
    started_unix: f64,
    ended_unix: Option<f64>,
    step: String,
    line: Option<u64>,
    last_call: Option<Value>,
    calls: u64,
    call_errors: u64,
    violations: u64,
    wall_s: f64,
    max_calls: u64,
    pid: u32,
    note: String,
    server_pid: u32,
    server_start: u64,
    paused_s: f64,
    awaiting: Option<Value>,
}

pub struct RunCtx {
    pub id: String,
    pub dir: PathBuf,
    cfg: Config,
    insts: Arc<Vec<Instrument>>,
    index: ToolIndex,
    rec: Mutex<Rec>,
    log: Mutex<EventLog>,
    emitted: Mutex<Map<String, Value>>,
    last_calls: Mutex<VecDeque<Value>>,
    hook_violations: Mutex<Vec<Value>>,
    cancel: Mutex<Option<String>>,
    touched: Mutex<Vec<Value>>,
    held: Mutex<Vec<(String, File)>>,
    ask: Mutex<Option<Ask>>,
    answer: Mutex<Option<String>>,
}

/// An operator prompt a run is waiting on.
struct Ask {
    id: Value,
    since: Instant,
    limit: Duration,
}

/// What a worker thread tells the supervisor.
enum Internal {
    Reply(Value),
}

fn registry() -> &'static Mutex<HashMap<String, Arc<RunCtx>>> {
    // [@ANCHOR: proc_run_registry]
    static MAP: std::sync::OnceLock<Mutex<HashMap<String, Arc<RunCtx>>>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The live run with this id, if this server process has it.
pub fn lookup(id: &str) -> Option<Arc<RunCtx>> {
    // [@ANCHOR: proc_run_lookup]
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .cloned()
}

/// Number of live runs that use the managed directory `root`.
pub fn active_in(root: &std::path::Path) -> usize {
    // [@ANCHOR: proc_run_active_in]
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    // A run whose final state is written but which the supervisor has not yet taken out of the table is not active.
    reg.values()
        .filter(|c| super::store::root(&c.cfg) == root)
        .filter(|c| is_active(&c.rec.lock().unwrap_or_else(|e| e.into_inner()).state))
        .count()
}

/// A value cut to at most `CLIP` characters of its JSON text (as a string when it had to be cut).
pub fn clip(v: &Value) -> Value {
    // [@ANCHOR: proc_run_clip]
    let text = v.to_string();
    if text.chars().count() <= CLIP {
        return v.clone();
    }
    let head: String = text.chars().take(CLIP).collect();
    json!(format!(
        "{head}... ({} characters in all)",
        text.chars().count()
    ))
}

fn is_active(state: &str) -> bool {
    // [@ANCHOR: proc_run_is_active]
    matches!(
        state,
        "starting" | "running" | "waiting_operator" | "cancelling"
    )
}

impl RunCtx {
    /// Appends an event and counts it.
    fn event(&self, kind: &str, data: Value) {
        // [@ANCHOR: proc_run_event]
        self.log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append(kind, data);
    }

    /// Rewrites `state.json` from the record.
    fn persist(&self) {
        // [@ANCHOR: proc_run_persist]
        let (v, dir) = {
            let r = self.rec.lock().unwrap_or_else(|e| e.into_inner());
            let (events, dropped) = {
                let l = self.log.lock().unwrap_or_else(|e| e.into_inner());
                (l.count(), l.dropped())
            };
            (
                json!({
                    "run": self.id, "state": r.state, "label": r.label, "tier_cap": tier_name(r.cap),
                    "started_unix": r.started_unix, "ended_unix": r.ended_unix, "step": r.step, "line": r.line,
                    "last_call": r.last_call, "calls": r.calls, "call_errors": r.call_errors,
                    "hook_violations": r.violations, "wall_s": r.wall_s, "max_calls": r.max_calls,
                    "pid": r.pid, "note": r.note, "server_pid": r.server_pid, "server_start": r.server_start, "paused_s": r.paused_s, "awaiting": r.awaiting, "cancel_reason": self.cancel_reason(), "events": events, "events_dropped": dropped,
                    "language": "python",
                }),
                self.dir.clone(),
            )
        };
        let _ = store::write_json(&dir.join("state.json"), &v);
    }

    /// Changes the state (and records it as an event).
    fn set_state(&self, state: &str, note: &str) {
        // [@ANCHOR: proc_run_set_state]
        {
            let mut r = self.rec.lock().unwrap_or_else(|e| e.into_inner());
            r.state = state.to_string();
            if !note.is_empty() {
                r.note = note.to_string();
            }
            if !is_active(state) {
                r.ended_unix = Some(store::now_unix());
            }
        }
        self.event("state", json!({"state": state, "note": note}));
        self.persist();
    }

    /// Asks the run to stop: the first reason wins.
    pub fn request_cancel(&self, reason: &str) {
        // [@ANCHOR: proc_run_request_cancel]
        let mut c = self.cancel.lock().unwrap_or_else(|e| e.into_inner());
        if c.is_none() {
            *c = Some(reason.to_string());
        }
    }

    /// Takes the reservations of `names` that this run does not hold yet, waiting up to `wait`.
    fn hold(&self, names: &[String], wait: Duration) -> Result<(), String> {
        // [@ANCHOR: proc_run_hold]
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let need: Vec<String> = names
            .iter()
            .filter(|n| !held.iter().any(|(h, _)| h == *n))
            .cloned()
            .collect();
        if need.is_empty() {
            return Ok(());
        }
        let label = self
            .rec
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .label
            .clone();
        let root = store::root(&self.cfg);
        let cancelled = || self.cancel_reason().is_some();
        let got = locks::reserve(&root, &self.id, &label, &need, wait, &cancelled)?;
        held.extend(got);
        Ok(())
    }

    /// Seconds spent waiting for the operator so far, finished waits and the one in progress.
    fn paused(&self) -> f64 {
        // [@ANCHOR: proc_run_paused]
        let done = self.rec.lock().unwrap_or_else(|e| e.into_inner()).paused_s;
        let now = self
            .ask
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map_or(0.0, |a| a.since.elapsed().as_secs_f64());
        done + now
    }

    /// The reply to a pending operator prompt once it was answered; asks the run to stop when the answer takes too long.
    fn poll_ask(&self) -> Option<Value> {
        // [@ANCHOR: proc_run_poll_ask]
        let mut ask = self.ask.lock().unwrap_or_else(|e| e.into_inner());
        let a = ask.as_ref()?;
        let answer = self.answer.lock().unwrap_or_else(|e| e.into_inner()).take();
        let Some(text) = answer else {
            if a.since.elapsed() > a.limit {
                self.request_cancel("operator_timeout");
            }
            return None;
        };
        let reply = json!({"op": "reply", "id": a.id, "ok": true, "value": text});
        let waited = a.since.elapsed().as_secs_f64();
        *ask = None;
        drop(ask);
        {
            let mut r = self.rec.lock().unwrap_or_else(|e| e.into_inner());
            r.paused_s += waited;
            r.awaiting = None;
        }
        self.event("answer", json!({"answer": text, "waited_s": waited}));
        self.set_state("running", "");
        Some(reply)
    }

    /// Drops a pending prompt (the run is being stopped); the wait still counts as paused time.
    fn clear_ask(&self) {
        // [@ANCHOR: proc_run_clear_ask]
        let mut ask = self.ask.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(a) = ask.take() {
            let mut r = self.rec.lock().unwrap_or_else(|e| e.into_inner());
            r.paused_s += a.since.elapsed().as_secs_f64();
            r.awaiting = None;
        }
    }

    fn cancel_reason(&self) -> Option<String> {
        // [@ANCHOR: proc_run_cancel_reason]
        self.cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// Runs one tool call for the program and returns the reply message (without its id).
fn exec_call(ctx: &RunCtx, tool: &str, args: &Value) -> Value {
    // [@ANCHOR: proc_run_exec_call]
    let fail =
        |kind: &str, msg: String| json!({"op": "reply", "ok": false, "kind": kind, "error": msg});
    let (cap, limit) = {
        let r = ctx.rec.lock().unwrap_or_else(|e| e.into_inner());
        let extra = if ctx.cancel_reason().is_some() {
            CLEANUP_CALLS
        } else {
            0
        };
        (r.cap, r.max_calls + extra)
    };
    {
        let mut r = ctx.rec.lock().unwrap_or_else(|e| e.into_inner());
        if r.calls >= limit {
            return fail(
                "budget",
                format!("the tool-call budget of {} calls is used up", r.max_calls),
            );
        }
        r.calls += 1;
    }
    let tier = match ctx.index.allow(tool, cap, &ctx.insts) {
        Ok(t) => t,
        Err((kind, msg)) => {
            count_error(ctx);
            return fail(kind, msg);
        }
    };
    if let Some(action) = safe::action_for(tool, args, &ctx.cfg) {
        let mut t = ctx.touched.lock().unwrap_or_else(|e| e.into_inner());
        safe::record(&ctx.dir, &mut t, action);
    }
    if tier > Tier::ReadOnly {
        if let Some(name) = locks::instrument_of(&ctx.insts, tool, args) {
            let wait = Duration::from_secs_f64(ctx.cfg.lock_wait_s);
            if let Err(e) = ctx.hold(&[name], wait) {
                count_error(ctx);
                return fail("lock", e);
            }
        }
    }
    let started = Instant::now();
    let out = crate::call(&ctx.insts, tool, args);
    let ms = started.elapsed().as_millis() as u64;
    let (entry, reply) = match &out {
        Ok(v) => {
            let big = v.to_string().len() > MAX_REPLY;
            let reply = if big {
                fail(
                    "tool",
                    format!("the reply of {tool} is larger than {MAX_REPLY} bytes"),
                )
            } else {
                json!({"op": "reply", "ok": true, "value": v})
            };
            (
                json!({"tool": tool, "args": clip(args), "ok": !big, "result": clip(v), "ms": ms}),
                reply,
            )
        }
        Err(e) => (
            json!({"tool": tool, "args": clip(args), "ok": false, "error": clip(&json!(e)), "ms": ms}),
            fail("tool", e.clone()),
        ),
    };
    if !entry["ok"].as_bool().unwrap_or(false) {
        count_error(ctx);
    }
    {
        let mut l = ctx.last_calls.lock().unwrap_or_else(|e| e.into_inner());
        if l.len() >= LAST_CALLS {
            l.pop_front();
        }
        l.push_back(entry.clone());
    }
    ctx.rec.lock().unwrap_or_else(|e| e.into_inner()).last_call = Some(entry.clone());
    ctx.event("call", entry);
    ctx.persist();
    reply
}

fn count_error(ctx: &RunCtx) {
    // [@ANCHOR: proc_run_count_error]
    ctx.rec
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .call_errors += 1;
}

/// `reserve` and `release` for the program: the names must be configured instruments.
fn reserve_op(ctx: &RunCtx, op: &str, names: &[String], wait: f64) -> Value {
    // [@ANCHOR: proc_run_reserve_op]
    let fail = |msg: String| json!({"op": "reply", "ok": false, "kind": "lock", "error": msg});
    if names.is_empty() {
        return fail("name at least one instrument".into());
    }
    if let Some(bad) = names
        .iter()
        .find(|n| !ctx.insts.iter().any(|i| &i.name == *n))
    {
        return fail(format!("no instrument named {bad:?} (see bench_list)"));
    }
    if op == "release" {
        let mut held = ctx.held.lock().unwrap_or_else(|e| e.into_inner());
        for n in names {
            locks::release(&store::root(&ctx.cfg), &mut held, n);
        }
        drop(held);
        ctx.event("release", json!({"instruments": names}));
        return json!({"op": "reply", "ok": true, "value": null});
    }
    match ctx.hold(names, Duration::from_secs_f64(wait)) {
        Ok(()) => {
            ctx.event("reserve", json!({"instruments": names}));
            json!({"op": "reply", "ok": true, "value": null})
        }
        Err(e) => fail(e),
    }
}

/// Handles one message from the program. Returns its final message (`done`, `failed`, `cancelled`) when it is one.
fn handle(ctx: &Arc<RunCtx>, msg: Value, tx: &Sender<Internal>) -> Option<(String, Value)> {
    // [@ANCHOR: proc_run_handle]
    let op = msg
        .get("op")
        .and_then(|o| o.as_str())
        .unwrap_or("")
        .to_string();
    let text = |k: &str| {
        msg.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    match op.as_str() {
        "call" => {
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            let tool = text("tool");
            let args = msg.get("args").cloned().unwrap_or_else(|| json!({}));
            let (ctx, tx) = (ctx.clone(), tx.clone());
            std::thread::spawn(move || {
                let mut reply = exec_call(&ctx, &tool, &args);
                reply["id"] = id;
                let _ = tx.send(Internal::Reply(reply));
            });
        }
        "emit" => {
            let key: String = text("key").chars().take(100).collect();
            let value = msg.get("value").cloned().unwrap_or(Value::Null);
            let mut em = ctx.emitted.lock().unwrap_or_else(|e| e.into_inner());
            if value.to_string().len() > 65536 || (!em.contains_key(&key) && em.len() >= 500) {
                drop(em);
                ctx.event(
                    "log",
                    json!({"msg": format!("emit {key:?} dropped: too large or too many keys")}),
                );
            } else {
                em.insert(key.clone(), value.clone());
                let _ =
                    store::write_json(&ctx.dir.join("emitted.json"), &Value::Object(em.clone()));
                drop(em);
                ctx.event("emit", json!({"key": key, "value": value}));
            }
        }
        "reserve" | "release" => {
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            let names: Vec<String> = msg
                .get("names")
                .and_then(|n| n.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|n| n.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let wait = msg
                .get("timeout")
                .and_then(|t| t.as_f64())
                .unwrap_or(30.0)
                .clamp(0.0, 3600.0);
            let (ctx, tx) = (ctx.clone(), tx.clone());
            std::thread::spawn(move || {
                let mut reply = reserve_op(&ctx, &op, &names, wait);
                reply["id"] = id;
                let _ = tx.send(Internal::Reply(reply));
            });
        }
        "ask" => {
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            let message: String = text("message").chars().take(1000).collect();
            let limit = msg
                .get("timeout")
                .and_then(|t| t.as_f64())
                .unwrap_or(f64::MAX)
                .clamp(1.0, ctx.cfg.operator_timeout_s);
            let mut ask = ctx.ask.lock().unwrap_or_else(|e| e.into_inner());
            if ask.is_some() || ctx.cancel_reason().is_some() {
                let why = "another operator prompt is pending, or the run is being stopped";
                let _ = tx.send(Internal::Reply(
                    json!({"op": "reply", "id": id, "ok": false, "kind": "policy", "error": why}),
                ));
            } else {
                *ask = Some(Ask {
                    id,
                    since: Instant::now(),
                    limit: Duration::from_secs_f64(limit),
                });
                drop(ask);
                ctx.rec.lock().unwrap_or_else(|e| e.into_inner()).awaiting = Some(
                    json!({"message": message, "since_unix": store::now_unix(), "answer_with": "bench_proc_input"}),
                );
                ctx.event("ask", json!({"message": message}));
                ctx.set_state("waiting_operator", "waiting for the operator");
            }
        }
        "log" => ctx.event("log", json!({"msg": text("msg")})),
        "step" => {
            ctx.rec.lock().unwrap_or_else(|e| e.into_inner()).step = text("name");
            ctx.event("step", json!({"name": text("name")}));
            ctx.persist();
        }
        "checkpoint" => {
            let name: String = text("name")
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .take(60)
                .collect();
            let dir = ctx.dir.join("checkpoints");
            let data = msg.get("data").cloned().unwrap_or(Value::Null);
            if name.is_empty()
                || data.to_string().len() > 262_144
                || std::fs::create_dir_all(&dir).is_err()
            {
                ctx.event(
                    "log",
                    json!({"msg": "checkpoint dropped: bad name or larger than 256 KiB"}),
                );
            } else {
                let _ = store::write_json(&dir.join(format!("{name}.json")), &data);
                ctx.event("checkpoint", json!({"name": name}));
            }
        }
        "stdout" | "stderr" => {
            let dropped = store::append_stream(&ctx.dir.join(format!("{op}.txt")), &text("text"));
            if dropped > 0 {
                ctx.rec.lock().unwrap_or_else(|e| e.into_inner()).note =
                    format!("{op} was cut at the size limit");
            }
        }
        "trace" => {
            let lines = msg.get("lines").cloned().unwrap_or_else(|| json!([]));
            let last = lines
                .as_array()
                .and_then(|a| a.last())
                .and_then(|l| l.as_u64());
            ctx.rec.lock().unwrap_or_else(|e| e.into_inner()).line = last;
            ctx.event("trace", json!({"lines": lines}));
        }
        "violation" => {
            let v = json!({"event": text("event"), "msg": text("msg")});
            ctx.hook_violations
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(v.clone());
            ctx.rec.lock().unwrap_or_else(|e| e.into_inner()).violations += 1;
            ctx.event("violation", v);
            ctx.persist();
        }
        "ready" => ctx.event(
            "log",
            json!({"msg": format!("python {} ready", text("python"))}),
        ),
        "protocol_error" => ctx.event("log", json!({"msg": text("msg")})),
        "done" | "failed" | "cancelled" => {
            return Some((op, msg.get("report").cloned().unwrap_or_else(|| json!({}))));
        }
        other => ctx.event(
            "log",
            json!({"msg": format!("unknown message {other:?} from the program")}),
        ),
    }
    None
}

/// The supervisor: runs on its own thread for the life of the run.
fn supervise(
    ctx: Arc<RunCtx>,
    backend: Arc<dyn Backend>,
    spec: SpawnSpec,
    started: Sender<Result<(), String>>,
) {
    // [@ANCHOR: proc_run_supervise]
    let mut session: Box<dyn Session> = match backend.start(&spec) {
        Ok(s) => s,
        Err(e) => {
            let _ = started.send(Err(e));
            return;
        }
    };
    ctx.rec.lock().unwrap_or_else(|e| e.into_inner()).pid = session.pid();
    let _ = started.send(Ok(()));
    ctx.set_state("running", "");
    let (tx, rx) = channel::<Internal>();
    let t0 = Instant::now();
    let (wall, grace) = {
        let r = ctx.rec.lock().unwrap_or_else(|e| e.into_inner());
        (
            Duration::from_secs_f64(r.wall_s),
            Duration::from_secs_f64(ctx.cfg.grace_s),
        )
    };
    let mut cancel_sent: Option<Instant> = None;
    let mut ended: Option<(String, Value)> = None;
    let mut ended_at: Option<Instant> = None;
    let mut killed = false;
    loop {
        while let Ok(Internal::Reply(r)) = rx.try_recv() {
            let _ = session.send(&r);
        }
        if ended.is_none() && cancel_sent.is_none() {
            if let Some(reply) = ctx.poll_ask() {
                let _ = session.send(&reply);
            }
            if t0.elapsed().as_secs_f64() - ctx.paused() > wall.as_secs_f64() {
                ctx.request_cancel("timeout");
            }
        }
        if let (Some(reason), None, None) = (ctx.cancel_reason(), &cancel_sent, &ended) {
            let _ = session.send(&json!({"op": "cancel", "reason": reason}));
            cancel_sent = Some(Instant::now());
            ctx.clear_ask();
            ctx.set_state("cancelling", &reason);
        }
        if let Some(at) = cancel_sent {
            if ended.is_none() && at.elapsed() > grace {
                session.kill();
                killed = true;
                break;
            }
        }
        if let Some(at) = ended_at {
            if at.elapsed() > Duration::from_secs(3) {
                session.kill();
                break;
            }
        }
        match session.recv(Duration::from_millis(20)) {
            Incoming::Message(m) => {
                if let Some(fin) = handle(&ctx, m, &tx) {
                    ended = Some(fin);
                    ended_at = Some(Instant::now());
                }
            }
            Incoming::Timeout => {}
            Incoming::Closed => break,
        }
    }
    let exit = session.exit_status();
    drop(session);
    finish(&ctx, ended, killed, &exit);
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&ctx.id);
}

/// The final state, from what the program reported and how it ended; writes `result.json`.
fn finish(ctx: &RunCtx, ended: Option<(String, Value)>, killed: bool, exit: &str) {
    // [@ANCHOR: proc_run_finish]
    let reason = ctx.cancel_reason();
    let timed_out = matches!(
        reason.as_deref(),
        Some("timeout") | Some("operator_timeout")
    );
    let stopped = if timed_out { "timed_out" } else { "cancelled" };
    let (state, value, mut failure) = match ended {
        Some((op, report)) if op == "done" => (
            "succeeded",
            report.get("value").cloned().unwrap_or(Value::Null),
            Value::Null,
        ),
        Some((op, mut report)) => {
            let st = if op == "cancelled" || reason.is_some() {
                stopped
            } else {
                "failed"
            };
            report["kind"] = json!(if st == "failed" { "exception" } else { stopped });
            (st, Value::Null, report)
        }
        None if killed => (
            stopped,
            Value::Null,
            json!({"kind": stopped, "message": format!("the program did not stop within the {} s grace time and was killed", ctx.cfg.grace_s)}),
        ),
        None => {
            let tail = store::read_stream(&ctx.dir.join("stderr.txt"), 0, 4000).0;
            let why = if exit.contains("SIGKILL") || exit.contains("SIGXCPU") {
                " (killed by the system: a memory or CPU-time limit of the run, or the out-of-memory killer)"
            } else {
                ""
            };
            (
                "failed",
                Value::Null,
                json!({"kind": "crash", "message": format!("the program ended without a result ({exit}){why}"), "stderr": tail}),
            )
        }
    };
    let mut safe_results = Vec::new();
    if state != "succeeded" {
        let touched = ctx
            .touched
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        safe_results = safe::run(&ctx.insts, &touched);
        for r in &safe_results {
            ctx.event("safe_state", r.clone());
        }
    }
    let safe_failed = safe_results.iter().any(|r| r["ok"] != json!(true));
    if safe_failed {
        failure["safe_state_failed"] =
            json!("SAFE STATE NOT CONFIRMED: see safe_state; check the instruments by hand");
    }
    if !failure.is_null() {
        failure["last_calls"] = Value::Array(
            ctx.last_calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .cloned()
                .collect(),
        );
        failure["emitted"] = Value::Object(
            ctx.emitted
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        );
    }
    let mut r = json!({
        "run": ctx.id, "state": state, "ok": state == "succeeded", "value": value, "failure": failure,
        "emitted": Value::Object(ctx.emitted.lock().unwrap_or_else(|e| e.into_inner()).clone()),
        "hook_violations": ctx.hook_violations.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        "exit": exit, "safe_state": safe_results, "safe_state_ok": !safe_failed,
    });
    {
        let rec = ctx.rec.lock().unwrap_or_else(|e| e.into_inner());
        r["calls"] = json!(rec.calls);
        r["call_errors"] = json!(rec.call_errors);
        r["elapsed_s"] = json!(store::now_unix() - rec.started_unix);
    }
    let _ = store::write_json(&ctx.dir.join("result.json"), &r);
    {
        let mut held = ctx.held.lock().unwrap_or_else(|e| e.into_inner());
        let names: Vec<String> = held.iter().map(|(n, _)| n.clone()).collect();
        for n in names {
            locks::release(&store::root(&ctx.cfg), &mut held, &n);
        }
    }
    ctx.set_state(
        state,
        if safe_failed {
            "SAFE STATE NOT CONFIRMED"
        } else {
            ""
        },
    );
}

/// Validates nothing (the caller did) and starts the run: directory, files, supervisor. Returns the run id.
pub fn start(
    cfg: &Config,
    insts: &[Instrument],
    backend: Arc<dyn Backend>,
    p: Params,
) -> Result<String, String> {
    // [@ANCHOR: proc_run_start]
    store::purge(cfg);
    let (id, dir) = store::new_run_dir(cfg)?;
    let started = store::now_unix();
    let index = ToolIndex::build(insts, p.cap);
    let scratch = dir.join("scratch");
    let write = |name: &str, text: String| {
        std::fs::write(dir.join(name), text).map_err(|e| format!("{name}: {e}"))
    };
    write("source.py", p.source.clone())?;
    write("args.json", p.args.to_string())?;
    write("tools.json", index.to_json().to_string())?;
    let conf = json!({"args": p.args, "wall_s": p.wall_s, "trace": p.trace, "scratch": scratch});
    write("config.json", conf.to_string())?;
    let log = EventLog::open(&dir, started)?;
    let ctx = Arc::new(RunCtx {
        id: id.clone(),
        dir: dir.clone(),
        cfg: cfg.clone(),
        insts: Arc::new(insts.to_vec()),
        index,
        rec: Mutex::new(Rec {
            state: "starting".into(),
            label: p.label.clone(),
            cap: p.cap,
            started_unix: started,
            ended_unix: None,
            step: String::new(),
            line: None,
            last_call: None,
            calls: 0,
            call_errors: 0,
            violations: 0,
            wall_s: p.wall_s,
            max_calls: p.max_calls,
            pid: 0,
            note: String::new(),
            server_pid: std::process::id(),
            server_start: start_ticks(std::process::id()).unwrap_or(0),
            paused_s: 0.0,
            awaiting: None,
        }),
        log: Mutex::new(log),
        emitted: Mutex::new(Map::new()),
        last_calls: Mutex::new(VecDeque::new()),
        hook_violations: Mutex::new(Vec::new()),
        cancel: Mutex::new(None),
        touched: Mutex::new(Vec::new()),
        held: Mutex::new(Vec::new()),
        ask: Mutex::new(None),
        answer: Mutex::new(None),
    });
    ctx.persist();
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.clone(), ctx.clone());
    let spec = SpawnSpec {
        run_dir: dir,
        scratch,
        limits: p.limits,
    };
    let (tx, rx) = channel();
    let c2 = ctx.clone();
    std::thread::spawn(move || supervise(c2, backend, spec, tx));
    match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(Ok(())) => Ok(id),
        Ok(Err(e)) => {
            registry()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            ctx.set_state("failed", &e);
            Err(e)
        }
        Err(_) => Err(format!("run {id} did not start within 20 s")),
    }
}

/// The start time of a process in clock ticks since boot (field 22 of its stat), which with the pid names one process for good.
pub fn start_ticks(pid: u32) -> Option<u64> {
    // [@ANCHOR: proc_run_start_ticks]
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after = text.rsplit_once(')')?.1;
    after.split_whitespace().nth(19)?.parse().ok()
}

/// Stops every live run of this server process, asking each to cancel with the reason `shutdown` and waiting for its cleanup and
/// safe state, so the instruments are left safe when the server ends.
pub fn shutdown() {
    // [@ANCHOR: proc_run_shutdown]
    stop_all(None);
}

/// `shutdown`, for the runs that use the managed directory `only` (all runs when `None`).
pub fn stop_all(only: Option<&std::path::Path>) {
    // [@ANCHOR: proc_run_stop_all]
    let mine = |c: &Arc<RunCtx>| only.is_none_or(|o| store::root(&c.cfg) == o);
    let runs: Vec<Arc<RunCtx>> = registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .filter(|c| mine(c))
        .cloned()
        .collect();
    for c in &runs {
        c.request_cancel("shutdown");
    }
    let limit = runs.iter().map(|c| c.cfg.grace_s).fold(0.0, f64::max) + 60.0;
    let t = Instant::now();
    while t.elapsed().as_secs_f64() < limit
        && registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .any(&mine)
    {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Finds runs of a server process that no longer exists (they were active when it died), marks them lost and puts the
/// instruments they touched in a safe state. Returns one entry per run found.
pub fn recover_now(insts: &[Instrument]) -> Vec<Value> {
    // [@ANCHOR: proc_run_recover_now]
    let mut found = Vec::new();
    let mut roots: BTreeMap<PathBuf, Config> = BTreeMap::new();
    for i in insts
        .iter()
        .filter(|i| i.kind == crate::config::Kind::Procedure)
    {
        roots.insert(store::root(&i.procedure), i.procedure.clone());
    }
    for (root, cfg) in roots {
        let Ok(rd) = std::fs::read_dir(root.join("runs")) else {
            continue;
        };
        for e in rd.flatten() {
            let dir = e.path();
            let Ok(mut st) = store::read_json(&dir.join("state.json")) else {
                continue;
            };
            let id = e.file_name().to_string_lossy().to_string();
            let active = is_active(st["state"].as_str().unwrap_or(""));
            let pid = st["server_pid"].as_u64().unwrap_or(0) as u32;
            let same = start_ticks(pid)
                .is_some_and(|t| t == st["server_start"].as_u64().unwrap_or(u64::MAX));
            if !active || same || lookup(&id).is_some() {
                continue;
            }
            let touched: Vec<Value> = store::read_json(&dir.join("touched.json"))
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            let results = safe::run(insts, &touched);
            let ok = results.iter().all(|r| r["ok"] == json!(true));
            let note = if ok {
                "the server that ran it ended"
            } else {
                "SAFE STATE NOT CONFIRMED"
            };
            st["state"] = json!("lost");
            st["ended_unix"] = json!(store::now_unix());
            st["note"] = json!(note);
            let _ = store::write_json(&dir.join("state.json"), &st);
            let emitted = store::read_json(&dir.join("emitted.json")).unwrap_or_else(|_| json!({}));
            let failure = json!({"kind": "lost", "message": "the server process that ran this run ended while it was active; its instruments were put in their safe state", "pid": pid});
            let result = json!({"run": id, "state": "lost", "ok": false, "value": null, "failure": failure, "emitted": emitted,
                "safe_state": results, "safe_state_ok": ok, "calls": st["calls"], "hook_violations": []});
            let _ = store::write_json(&dir.join("result.json"), &result);
            found.push(
                json!({"run": id, "safe_state": results, "retention_days": cfg.retention_days}),
            );
        }
    }
    found
}

/// Runs `recover_now` on a thread of its own, so a safe-state call that waits on hardware does not hold up the server's start.
pub fn recover(insts: &[Instrument]) {
    // [@ANCHOR: proc_run_recover]
    let insts = insts.to_vec();
    std::thread::spawn(move || {
        recover_now(&insts);
    });
}

/// Answers the operator prompt a live run is waiting on; returns the run's record.
pub fn answer(id: &str, text: &str) -> Result<Value, String> {
    // [@ANCHOR: proc_run_answer]
    if text.chars().count() > 4096 {
        return Err("answer is longer than 4096 characters".into());
    }
    let ctx = lookup(id)
        .ok_or_else(|| format!("run {id} is not running in this server (see bench_proc_status)"))?;
    if ctx.ask.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
        let state = ctx
            .rec
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .state
            .clone();
        return Err(format!(
            "run {id} is not waiting for the operator (state {state})"
        ));
    }
    *ctx.answer.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.to_string());
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5)
        && ctx.ask.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(json!({"run": id, "answered": true}))
}

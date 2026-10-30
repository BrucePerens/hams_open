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
use crate::config::{Instrument, Tier};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, VecDeque};
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
    reg.values()
        .filter(|c| super::store::root(&c.cfg) == root)
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
                    "pid": r.pid, "note": r.note, "cancel_reason": self.cancel_reason(), "events": events, "events_dropped": dropped,
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
    if let Err((kind, msg)) = ctx.index.allow(tool, cap, &ctx.insts) {
        count_error(ctx);
        return fail(kind, msg);
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
        if ended.is_none() && cancel_sent.is_none() && t0.elapsed() > wall {
            ctx.request_cancel("timeout");
        }
        if let (Some(reason), None, None) = (ctx.cancel_reason(), &cancel_sent, &ended) {
            let _ = session.send(&json!({"op": "cancel", "reason": reason}));
            cancel_sent = Some(Instant::now());
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
        "exit": exit,
    });
    {
        let rec = ctx.rec.lock().unwrap_or_else(|e| e.into_inner());
        r["calls"] = json!(rec.calls);
        r["call_errors"] = json!(rec.call_errors);
        r["elapsed_s"] = json!(store::now_unix() - rec.started_unix);
    }
    let _ = store::write_json(&ctx.dir.join("result.json"), &r);
    ctx.set_state(state, "");
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
        }),
        log: Mutex::new(log),
        emitted: Mutex::new(Map::new()),
        last_calls: Mutex::new(VecDeque::new()),
        hook_violations: Mutex::new(Vec::new()),
        cancel: Mutex::new(None),
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

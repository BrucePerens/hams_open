// SPDX-License-Identifier: AGPL-3.0-or-later
//! The interface of a language backend of the procedure runner. A backend starts one program in a process of its own and
//! speaks the run protocol with it: newline-delimited JSON objects, `op` naming the message. The host asks nothing of the
//! language beyond that: another language (a Lisp-like engine, MicroPython) implements these two traits and the runner
//! (`run.rs`) drives it unchanged.
//!
//! Program to host: `ready`, `call` (tool, args, id), `emit`, `log`, `step`, `checkpoint`, `stdout`, `stderr`, `trace`,
//! `violation`, and one of `done`, `failed`, `cancelled` (with a `report`). Host to program: `reply` (id, ok, value or
//! error and kind) and `cancel` (reason). The CPython implementation is `cpython.rs`.

use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

/// The operating-system limits a program runs under.
#[derive(Clone, Debug, PartialEq)]
pub struct Limits {
    /// Address space, MiB.
    pub memory_mb: u64,
    /// CPU time, seconds.
    pub cpu_s: u64,
    /// Open files.
    pub files: u64,
    /// Largest file the program may write, MiB.
    pub file_mb: u64,
}

/// What a backend needs to start one run.
#[derive(Clone, Debug)]
pub struct SpawnSpec {
    /// The run's directory in the managed store: the backend finds `source.py`, `config.json` and `tools.json` there.
    pub run_dir: PathBuf,
    /// The program's working directory and the only place it may write.
    pub scratch: PathBuf,
    pub limits: Limits,
}

/// What `Session::recv` found.
#[derive(Debug, PartialEq)]
pub enum Incoming {
    Message(Value),
    Timeout,
    /// The program closed its end: it has ended or is about to.
    Closed,
}

/// One running program.
pub trait Session: Send {
    /// Sends one message to the program.
    fn send(&mut self, msg: &Value) -> Result<(), String>;
    /// Waits up to `wait` for the next message.
    fn recv(&mut self, wait: Duration) -> Incoming;
    /// Ends the program and everything it started, at once, and reaps it.
    fn kill(&mut self);
    /// How the program ended, once it has (waits for it when it has closed its pipes).
    fn exit_status(&mut self) -> String;
    /// The operating-system process id, for the run record.
    fn pid(&self) -> u32;
}

/// A language the runner can start programs in.
pub trait Backend: Send + Sync {
    /// Checks a source without running it: `{"ok": true, "calls": [tool names it calls literally]}` or `{"ok": false, ...}`.
    fn check(&self, source: &str, scratch: &std::path::Path) -> Result<Value, String>;
    /// Starts the program of `spec`.
    fn start(&self, spec: &SpawnSpec) -> Result<Box<dyn Session>, String>;
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! The CPython backend: each run is a `python3 -I` process of its own, in a fresh scratch directory with a stripped
//! environment, speaking the run protocol (`backend.rs`) on its stdin and a duplicate of its stdout. `bench_runtime.py`
//! (embedded, written to the run directory at start) installs the audit hook, builds the `bench` module and runs the source.
//!
//! The limits are `setrlimit` ones set in the child before it executes (address space, CPU time, open files, file size,
//! number of processes), a process group so that a kill reaches everything the program started, and a parent-death
//! signal so that no program outlives the server. THE AUDIT HOOK AND THE LIMITS ARE NOT A SANDBOX: they keep the tier model
//! honest and stop accidents and runaways; code that tries can get around them.

use super::backend::{Backend, Incoming, Limits, Session, SpawnSpec};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// The runtime written next to each run's source.
pub const RUNTIME: &str = include_str!("bench_runtime.py");
/// Processes and threads the program may add to the ones the user has when it starts. `RLIMIT_NPROC` counts every task of the
/// user, other programs' included, and a busy machine (parallel builds) can add hundreds in the moment between the count and the
/// program's first thread; a small margin made runs die with `can't start new thread` (found by repeating the tests under load).
/// The margin still stops a runaway fork loop at about this many tasks.
const TASK_MARGIN: u64 = 2048;
/// The longest protocol line accepted from the program, bytes.
const MAX_LINE: u64 = 16 << 20;

pub struct CPython {
    pub python: String,
    /// `none`, or `systemd-run` to start the program inside a transient user scope with the memory and task limits.
    pub wrapper: String,
}

struct PySession {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Value>,
}

/// The environment of a program: nothing of the server's, a PATH for the standard tools, and single-threaded numerics.
fn stripped_env(scratch: &Path) -> Vec<(String, String)> {
    // [@ANCHOR: proc_cpython_env]
    let s = scratch.to_string_lossy().to_string();
    [
        ("PATH", "/usr/bin:/bin"),
        ("LANG", "C.UTF-8"),
        ("OPENBLAS_NUM_THREADS", "1"),
        ("OMP_NUM_THREADS", "1"),
        ("MKL_NUM_THREADS", "1"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .chain([("HOME".into(), s.clone()), ("TMPDIR".into(), s)])
    .collect()
}

/// Threads and processes of the current user, now. `RLIMIT_NPROC` counts all of them, so the program's limit is this plus
/// `TASK_MARGIN`: it can start the threads it needs (numpy) and a runaway fork loop is stopped.
fn user_tasks() -> u64 {
    // [@ANCHOR: proc_cpython_user_tasks]
    // SAFETY: getuid has no preconditions.
    let me = unsafe { libc::getuid() }.to_string();
    let mut total = 0u64;
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return 4096;
    };
    for e in rd.flatten() {
        if !e
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(e.path().join("status")) else {
            continue;
        };
        let uid = text.lines().find_map(|l| l.strip_prefix("Uid:"));
        if uid.and_then(|u| u.split_whitespace().next()) != Some(me.as_str()) {
            continue;
        }
        let threads = text.lines().find_map(|l| l.strip_prefix("Threads:"));
        total += threads.and_then(|t| t.trim().parse().ok()).unwrap_or(1);
    }
    total
}

fn limit(resource: libc::__rlimit_resource_t, soft: u64, hard: u64) {
    // [@ANCHOR: proc_cpython_limit]
    let r = libc::rlimit {
        rlim_cur: soft as libc::rlim_t,
        rlim_max: hard as libc::rlim_t,
    };
    // SAFETY: setrlimit reads a valid struct; called in the child between fork and exec.
    unsafe { libc::setrlimit(resource, &r) };
}

/// The command line of a run: the interpreter flags, the wrapper if configured, the runtime and the run directory.
pub fn command_line(cfg: &CPython, runtime: &Path, tail: &[&str], limits: &Limits) -> Vec<String> {
    // [@ANCHOR: proc_cpython_command_line]
    let mut v: Vec<String> = Vec::new();
    if cfg.wrapper == "systemd-run" {
        v.extend(
            [
                "systemd-run",
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                "-p",
                &format!("MemoryMax={}M", limits.memory_mb),
                "-p",
                "TasksMax=64",
            ]
            .map(String::from),
        );
    }
    v.push(cfg.python.clone());
    v.extend(["-I", "-B", "-u", "-W", "ignore:A NumPy version:UserWarning"].map(String::from));
    v.push(runtime.to_string_lossy().to_string());
    v.extend(tail.iter().map(|s| s.to_string()));
    v
}

/// Reader threads: protocol lines from stdout and raw text from stderr, into one channel.
fn pump(child: &mut Child) -> Receiver<Value> {
    // [@ANCHOR: proc_cpython_pump]
    let (tx, rx) = channel::<Value>();
    let out = child.stdout.take();
    let err = child.stderr.take();
    let tx_out = tx.clone();
    std::thread::spawn(move || {
        let Some(out) = out else { return };
        let mut r = BufReader::new(out);
        loop {
            let mut line = Vec::new();
            let n = r
                .by_ref()
                .take(MAX_LINE)
                .read_until(b'\n', &mut line)
                .unwrap_or(0);
            if n == 0 {
                break;
            }
            let msg = match serde_json::from_slice::<Value>(&line) {
                Ok(v) => v,
                Err(e) => {
                    json!({"op": "protocol_error", "msg": format!("unreadable line from the program: {e}")})
                }
            };
            if tx_out.send(msg).is_err() {
                break;
            }
        }
    });
    std::thread::spawn(move || {
        let Some(mut err) = err else { return };
        let mut buf = [0u8; 8192];
        while let Ok(n) = err.read(&mut buf) {
            if n == 0 {
                break;
            }
            let text = String::from_utf8_lossy(&buf[..n]).to_string();
            if tx.send(json!({"op": "stderr", "text": text})).is_err() {
                break;
            }
        }
    });
    rx
}

impl Backend for CPython {
    fn check(&self, source: &str, scratch: &Path) -> Result<Value, String> {
        // [@ANCHOR: proc_cpython_check]
        let runtime = scratch.join("bench_runtime.py");
        let file = scratch.join("check_source.py");
        std::fs::write(&runtime, RUNTIME).map_err(|e| format!("{}: {e}", runtime.display()))?;
        std::fs::write(&file, source).map_err(|e| format!("{}: {e}", file.display()))?;
        let plain = CPython {
            python: self.python.clone(),
            wrapper: "none".into(),
        };
        let argv = command_line(
            &plain,
            &runtime,
            &["--check", &file.to_string_lossy()],
            &no_limits(),
        );
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .env_clear()
            .envs(stripped_env(scratch))
            .current_dir(scratch)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", self.python))?;
        let t = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if t.elapsed() < Duration::from_secs(20) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("the source check did not finish in 20 s".into());
                }
            }
        }
        let mut text = String::new();
        if let Some(mut o) = child.stdout.take() {
            let _ = o.read_to_string(&mut text);
        }
        serde_json::from_str(text.trim())
            .map_err(|e| format!("the source check gave no result ({e})"))
    }

    fn start(&self, spec: &SpawnSpec) -> Result<Box<dyn Session>, String> {
        // [@ANCHOR: proc_cpython_start]
        let runtime = spec.run_dir.join("bench_runtime.py");
        std::fs::write(&runtime, RUNTIME).map_err(|e| format!("{}: {e}", runtime.display()))?;
        let argv = command_line(
            self,
            &runtime,
            &[&spec.run_dir.to_string_lossy()],
            &spec.limits,
        );
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .env_clear()
            .envs(stripped_env(&spec.scratch))
            .current_dir(&spec.scratch)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let l = spec.limits.clone();
        let tasks = user_tasks() + TASK_MARGIN;
        // SAFETY: the closure only calls async-signal-safe libc functions (setrlimit, prctl) between fork and exec.
        unsafe {
            cmd.pre_exec(move || {
                limit(libc::RLIMIT_AS, l.memory_mb << 20, l.memory_mb << 20);
                limit(libc::RLIMIT_CPU, l.cpu_s, l.cpu_s + 2);
                limit(libc::RLIMIT_NOFILE, l.files, l.files);
                limit(libc::RLIMIT_FSIZE, l.file_mb << 20, l.file_mb << 20);
                limit(libc::RLIMIT_NPROC, tasks, tasks);
                limit(libc::RLIMIT_CORE, 0, 0);
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", self.python))?;
        let stdin = child.stdin.take();
        let rx = pump(&mut child);
        Ok(Box::new(PySession { child, stdin, rx }))
    }
}

/// Limits for the source check, which runs no procedure code.
fn no_limits() -> Limits {
    // [@ANCHOR: proc_cpython_no_limits]
    Limits {
        memory_mb: 1024,
        cpu_s: 20,
        files: 64,
        file_mb: 1,
    }
}

impl Session for PySession {
    fn send(&mut self, msg: &Value) -> Result<(), String> {
        // [@ANCHOR: proc_cpython_send]
        let Some(w) = self.stdin.as_mut() else {
            return Err("the program has closed its input".into());
        };
        let mut line = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
        line.push(b'\n');
        w.write_all(&line)
            .and_then(|_| w.flush())
            .map_err(|e| format!("cannot write to the program: {e}"))
    }

    fn recv(&mut self, wait: Duration) -> Incoming {
        // [@ANCHOR: proc_cpython_recv]
        match self.rx.recv_timeout(wait) {
            Ok(v) => Incoming::Message(v),
            Err(RecvTimeoutError::Timeout) => Incoming::Timeout,
            Err(RecvTimeoutError::Disconnected) => Incoming::Closed,
        }
    }

    fn kill(&mut self) {
        // [@ANCHOR: proc_cpython_kill]
        let pid = self.child.id() as i32;
        // SAFETY: kill with a negative pid signals the program's own process group.
        unsafe { libc::kill(-pid, libc::SIGKILL) };
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.stdin = None;
    }

    fn exit_status(&mut self) -> String {
        // [@ANCHOR: proc_cpython_exit_status]
        let t = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(s)) => return s.to_string(),
                Ok(None) if t.elapsed() < Duration::from_secs(3) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => return "still running".into(),
            }
        }
    }

    fn pid(&self) -> u32 {
        // [@ANCHOR: proc_cpython_pid]
        self.child.id()
    }
}

impl Drop for PySession {
    fn drop(&mut self) {
        // [@ANCHOR: proc_cpython_drop]
        if matches!(self.child.try_wait(), Ok(None)) {
            self.kill();
        }
    }
}

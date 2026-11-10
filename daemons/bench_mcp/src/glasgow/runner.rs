// SPDX-License-Identifier: AGPL-3.0-or-later
//! How the Glasgow module reaches the `glasgow` command line: one trait, so the tests put a simulated device behind the same
//! code path the real process uses. The real runner starts the pinned venv's `glasgow` as a child of this server, reads its
//! output on two threads, and ends it by the ladder SIGINT (what Ctrl+C does: Glasgow closes its files and stops cleanly), then
//! SIGTERM. It never sends SIGKILL, and it only ever signals the child it started.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One command line to run.
#[derive(Clone, Debug)]
pub struct Req {
    /// Arguments after the program name (the `glasgow` binary is the runner's business).
    pub args: Vec<String>,
    /// The whole call ends after this long whatever happens (SIGINT, then SIGTERM).
    pub timeout: Duration,
    /// For a capture that runs until told to stop: SIGINT after this long. The exit is then a success, not a timeout.
    pub stop_after: Option<Duration>,
    /// A file the program writes; SIGINT is sent when it grows past `max_bytes` (a capture of a busy bus must not fill the disk).
    pub watch: Option<(PathBuf, u64)>,
    /// Run at low priority (bitstream builds use every core for ten seconds).
    pub nice: bool,
    /// Full-speed fallback: run this launcher script with the install's Python instead of the `glasgow` command (see `glasgow_fs.py`).
    pub full_speed: Option<PathBuf>,
}

/// What came back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Out {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Ended by this runner because `timeout` passed.
    pub timed_out: bool,
    /// Ended by this runner on purpose (`stop_after` or the size limit): the output is complete up to there.
    pub stopped: bool,
    /// The child still ran after SIGINT and SIGTERM. Its pid is named in `stderr`; nothing else was done to it.
    pub stuck: bool,
}

pub trait Runner: Send + Sync {
    fn run(&self, req: &Req) -> Result<Out, String>;
}

/// The runner that starts the real `glasgow`.
pub struct RealRunner {
    pub program: PathBuf,
    pub cache_dir: PathBuf,
}

impl RealRunner {
    fn command(&self, req: &Req) -> Command {
        // [@ANCHOR: glasgow_runner_command]
        // Full-speed fallback: the install's Python runs the launcher script, which then behaves as `glasgow`.
        let mut argv = vec![self.program.clone().into_os_string()];
        if let Some(launcher) = &req.full_speed {
            argv = vec![
                self.program.with_file_name("python").into_os_string(),
                launcher.clone().into_os_string(),
            ];
        }
        let mut c = if req.nice {
            let mut c = Command::new("nice");
            c.args(["-n", "10"]).args(&argv);
            c
        } else {
            let mut c = Command::new(&argv[0]);
            c.args(&argv[1..]);
            c
        };
        c.args(&req.args)
            // The bitstream cache of the pinned install, never the user's own ~/.cache.
            .env("XDG_CACHE_HOME", &self.cache_dir)
            .env("PYTHONNOUSERSITE", "1")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        c
    }
}

fn drain(mut r: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = r.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    })
}

impl Runner for RealRunner {
    fn run(&self, req: &Req) -> Result<Out, String> {
        // [@ANCHOR: glasgow_real_runner]
        let mut child = self.command(req).spawn().map_err(|e| {
            format!(
                "cannot start {}: {e}. Next: run daemons/bench_mcp/glasgow/install_glasgow.py (see manual instruments/glasgow/setup) or fix the config's \"venv\".",
                self.program.display()
            )
        })?;
        let pid = child.id();
        let pid_t = libc::pid_t::try_from(pid).map_err(|_| "pid out of range".to_string())?;
        let out_t = drain(child.stdout.take().ok_or("no stdout pipe")?);
        let err_t = drain(child.stderr.take().ok_or("no stderr pipe")?);
        // SAFETY: kill(2) only with the pid of the child this function started and still holds (not yet reaped).
        let signal = |sig: libc::c_int| unsafe { libc::kill(pid_t, sig) };
        let started = Instant::now();
        let mut out = Out::default();
        let mut sent_int: Option<Instant> = None;
        let mut sent_term: Option<Instant> = None;
        let status = loop {
            if let Some(s) = child.try_wait().map_err(|e| format!("wait: {e}"))? {
                break Some(s);
            }
            let now = Instant::now();
            let over_size = req.watch.as_ref().is_some_and(|(p, max)| {
                std::fs::metadata(p)
                    .map(|m| m.len() > *max)
                    .unwrap_or(false)
            });
            if sent_int.is_none()
                && (over_size || req.stop_after.is_some_and(|d| now - started >= d))
            {
                out.stopped = true;
                signal(libc::SIGINT);
                sent_int = Some(now);
            } else if sent_int.is_none() && now - started >= req.timeout {
                out.timed_out = true;
                signal(libc::SIGINT);
                sent_int = Some(now);
            }
            if let Some(t) = sent_int {
                if sent_term.is_none() && now - t >= Duration::from_secs(10) {
                    signal(libc::SIGTERM);
                    sent_term = Some(now);
                }
            }
            if let Some(t) = sent_term {
                if now - t >= Duration::from_secs(10) {
                    out.stuck = true;
                    break None;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        if let Some(s) = status {
            out.code = s.code();
            out.stdout = out_t.join().unwrap_or_default();
            out.stderr = err_t.join().unwrap_or_default();
        } else {
            // The child is still running and holds the pipes; do not wait for it. A person decides (the pid is in the message).
            out.stderr = format!(
                "glasgow (pid {pid}) did not end after SIGINT and SIGTERM and was left running; no SIGKILL is ever sent. Next: look at it (ps -p {pid}) and end it by hand, then call bench_glasgow_safe_state."
            );
        }
        Ok(out)
    }
}

/// The program and cache of an install directory: `<dir>/venv/bin/glasgow` and `<dir>/cache`.
pub fn install_paths(dir: &Path) -> (PathBuf, PathBuf) {
    // [@ANCHOR: glasgow_install_paths]
    (dir.join("venv/bin/glasgow"), dir.join("cache"))
}

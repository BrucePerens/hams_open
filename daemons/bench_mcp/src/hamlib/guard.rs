// SPDX-License-Identifier: AGPL-3.0-or-later
//! The transmit watchdog: a separate process that un-keys the rig when the transmit tool cannot.
//!
//! Before any tool keys the rig it starts `bench_mcp --hamlib-guard <host> <port> <deadline_ms> <min_power>` (this program run
//! again, as the USB tools run their root helper) and waits until the guard says it is armed. The guard has its own connection to
//! the rig's rigctld and the tool's end of a pipe on its standard input. It un-keys the rig, and then sets the power to the
//! minimum, when ANY of these happens: the deadline passes (the tool should have finished by then); the pipe closes without the
//! word `done` (the server crashed, was killed, panicked, or exited); or the tool tells it `abort`. The tool tells it `done` only
//! after it has un-keyed the rig and read the PTT state back. A killed server therefore does not leave the rig keyed: the pipe
//! closes with the process. The guard is in its own process group, so a Ctrl-C aimed at the server does not take it down first.
//!
//! What this cannot cover: the guard itself being killed at the same time as the server (a SIGKILL to both), the rigctld or the
//! computer dying, and the rig ignoring `T 0`. The next start of the module checks for a stuck PTT.

use super::proto::Client;
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The command-line flag that makes `bench_mcp` run as the watchdog.
pub const FLAG: &str = "--hamlib-guard";
/// How long after the planned end the guard waits before it acts alone.
pub const MARGIN: Duration = Duration::from_secs(3);
const ATTEMPTS: u32 = 8;

/// What the guard does to the rig. Returns whether the PTT was read back as off (or could not be read but `set_ptt 0` was accepted).
pub fn unkey(host: &str, port: u16, min_power: Option<f64>) -> Result<bool, String> {
    // [@ANCHOR: hamlib_guard_unkey]
    let mut last = String::from("no attempt");
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(250));
        }
        let mut c = match Client::connect(host, port, Duration::from_secs(2)) {
            Ok(c) => c,
            Err(e) => {
                last = e.to_string();
                continue;
            }
        };
        if let Err(e) = c.command("set_ptt 0") {
            last = e.to_string();
            continue;
        }
        if let Some(p) = min_power {
            // Best effort: some rigs refuse a level change while they are not ready; the PTT is what matters.
            let _ = c.command(&format!("set_level RFPOWER {p}"));
        }
        match c.read("get_ptt") {
            Ok(r) if r.kv("PTT") == Some("0") => return Ok(true),
            Ok(r) => last = format!("the rig still reports PTT {:?}", r.kv("PTT")),
            // A rig that cannot report PTT: the accepted `set_ptt 0` is all there is to go on.
            Err(e) if e.unsupported() => return Ok(false),
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!(
        "could not un-key the rig after {ATTEMPTS} attempts ({last})"
    ))
}

/// Lines from the tool's side of the pipe, or the end of the pipe.
pub enum Word {
    Line(String),
    Closed,
}

/// The watchdog. `words` is the standard input of the process, as a channel; `armed` is told when the guard has reached the rig.
/// Returns the exit code: 0 when the tool said `done` or the guard un-keyed the rig and read it back, 2 when it could not.
pub fn run(
    host: &str,
    port: u16,
    deadline: Duration,
    min_power: Option<f64>,
    words: &mpsc::Receiver<Word>,
    armed: &mut dyn Write,
    report: &mut dyn Write,
) -> i32 {
    // [@ANCHOR: hamlib_guard_run]
    let until = Instant::now() + deadline;
    // Prove the way to the rig works before saying so: a guard that cannot reach it must not let the tool key.
    match Client::connect(host, port, Duration::from_secs(2))
        .and_then(|mut c| c.command("get_freq"))
    {
        Ok(_) => {}
        Err(e) => {
            let _ = writeln!(report, "guard: cannot reach rigctld at {host}:{port}: {e}");
            return 3;
        }
    }
    let _ = writeln!(armed, "armed");
    let _ = armed.flush();
    let why = loop {
        let left = until.saturating_duration_since(Instant::now());
        match words.recv_timeout(left) {
            Ok(Word::Line(l)) if l.trim() == "done" => return 0,
            Ok(Word::Line(l)) if l.trim() == "abort" => break "the tool asked for an abort",
            Ok(Word::Line(_)) => continue,
            Ok(Word::Closed) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                break "the server's pipe closed without done: it exited or died"
            }
            Err(mpsc::RecvTimeoutError::Timeout) => break "the deadline passed without done",
        }
    };
    let _ = writeln!(
        report,
        "guard: {why}; un-keying the rig and setting the minimum power"
    );
    match unkey(host, port, min_power) {
        Ok(_) => {
            let _ = writeln!(report, "guard: rig un-keyed");
            0
        }
        Err(e) => {
            let _ = writeln!(
                report,
                "guard: FAILED: {e}. The rig may be transmitting: switch it off at the rig now."
            );
            2
        }
    }
}

/// `bench_mcp --hamlib-guard <host> <port> <deadline_ms> <min_power|none>`: the process entry. Reads its standard input in a thread.
pub fn main(args: &[String]) -> i32 {
    // [@ANCHOR: hamlib_guard_main]
    let parsed = (|| -> Result<(String, u16, Duration, Option<f64>), String> {
        let [host, port, ms, min] = args else {
            return Err(
                "usage: --hamlib-guard <host> <port> <deadline_ms> <min_power|none>".into(),
            );
        };
        let ms: u64 = ms.parse().map_err(|_| "deadline_ms is not a number")?;
        if !(500..=120_000).contains(&ms) {
            return Err("deadline_ms must be 500 to 120000".into());
        }
        let min = if min == "none" {
            None
        } else {
            Some(
                min.parse::<f64>()
                    .ok()
                    .filter(|m| (0.0..=1.0).contains(m))
                    .ok_or("min_power must be 0 to 1 or none")?,
            )
        };
        Ok((
            host.clone(),
            port.parse().map_err(|_| "port is not a number")?,
            Duration::from_millis(ms),
            min,
        ))
    })();
    let (host, port, deadline, min) = match parsed {
        Ok(p) => p,
        Err(e) => {
            eprintln!("guard: {e}");
            return 64;
        }
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(std::io::stdin()).lines() {
            match line {
                Ok(l) => {
                    if tx.send(Word::Line(l)).is_err() {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(Word::Closed);
    });
    run(
        &host,
        port,
        deadline,
        min,
        &rx,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

/// What the transmit tool holds while the watchdog runs.
pub trait Watch {
    /// The tool un-keyed the rig and read it back: the guard stands down.
    fn stand_down(self: Box<Self>) -> Result<(), String>;
    /// The rig could not be shown to be un-keyed: leave it to the guard (close the pipe without `done`).
    fn leave_to_guard(self: Box<Self>);
}

/// Starts watchdogs. Production starts a process; the tests of this module start the same `run` in a thread, because a test binary
/// is not `bench_mcp`. `tests/hamlib_guard.rs` runs the real process.
pub trait Spawner {
    fn spawn(
        &self,
        host: &str,
        port: u16,
        deadline: Duration,
        min_power: Option<f64>,
        log: &std::path::Path,
    ) -> Result<Box<dyn Watch>, String>;
}

pub struct ProcessSpawner;

struct ProcessWatch {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
}

impl Spawner for ProcessSpawner {
    fn spawn(
        &self,
        host: &str,
        port: u16,
        deadline: Duration,
        min_power: Option<f64>,
        log: &std::path::Path,
    ) -> Result<Box<dyn Watch>, String> {
        // [@ANCHOR: hamlib_guard_spawn]
        use std::os::unix::process::CommandExt;
        let exe = std::env::current_exe()
            .map_err(|e| format!("cannot find this program to start the transmit watchdog: {e}"))?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map_err(|e| format!("watchdog log {}: {e}", log.display()))?;
        let mut child = std::process::Command::new(exe)
            .args([
                FLAG,
                host,
                &port.to_string(),
                &deadline.as_millis().to_string(),
                &min_power.map_or("none".to_string(), |p| p.to_string()),
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::from(log))
            .process_group(0)
            .spawn()
            .map_err(|e| format!("cannot start the transmit watchdog: {e}"))?;
        let stdin = child.stdin.take();
        let out = child.stdout.take().ok_or("no watchdog output")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = tx.send(BufReader::new(out).read_line(&mut line).map(|_| line));
        });
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(l)) if l.trim() == "armed" => Ok(Box::new(ProcessWatch { child, stdin })),
            other => {
                let _ = child.kill();
                let _ = child.wait();
                Err(format!("the transmit watchdog did not report armed ({other:?}); not keying. Next: bench_hamlib_diagnose, and check the watchdog log in the managed directory."))
            }
        }
    }
}

impl Watch for ProcessWatch {
    fn stand_down(mut self: Box<Self>) -> Result<(), String> {
        // [@ANCHOR: hamlib_guard_stand_down]
        if let Some(mut s) = self.stdin.take() {
            let _ = s.write_all(b"done\n");
        }
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            if let Ok(Some(_)) = self.child.try_wait() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        Err("the watchdog did not exit after done; it was killed".into())
    }

    fn leave_to_guard(mut self: Box<Self>) {
        // Closing the pipe without `done` makes the guard un-key the rig by itself; it is not waited for here.
        drop(self.stdin.take());
        std::thread::spawn(move || {
            let _ = self.child.wait();
        });
    }
}

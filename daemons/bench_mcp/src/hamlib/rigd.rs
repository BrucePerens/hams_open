// SPDX-License-Identifier: AGPL-3.0-or-later
//! The managed `rigctld`: one per configured rig, started by this server on a free localhost port, watched, restarted within a
//! limit, and stopped when the server stops. It listens on 127.0.0.1 only (`-T 127.0.0.1`); there is no option to change that.
//!
//! A small state file in the managed directory records the server's pid and the daemon's pid. If this server was killed, the next
//! start finds the file, sees that its owner is gone, and stops the leftover daemon before starting its own; a live owner is never touched.

use super::{Config, Managed, Source};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How many starts of one rig's daemon within `WINDOW` are allowed without the owner asking again.
const MAX_STARTS: usize = 4;
const WINDOW: Duration = Duration::from_secs(600);
/// How long a new daemon gets to open the rig and accept connections.
const READY_WAIT: Duration = Duration::from_secs(10);

struct Slot {
    child: Option<Child>,
    port: u16,
    starts: Vec<Instant>,
    last_exit: Option<String>,
}

static STATE: Mutex<BTreeMap<String, Slot>> = Mutex::new(BTreeMap::new());
static REAPED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The directory for the state files, logs, the keyed-interval log, baselines and the station table.
pub fn dir(cfg: &Config) -> PathBuf {
    let d = cfg
        .managed_dir
        .clone()
        .unwrap_or_else(|| crate::out_dir().join("hamlib"));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// The command line of the daemon for a managed rig. Nothing here comes from a tool argument: model, device and settings are the owner's config.
pub fn args(m: &Managed, port: u16) -> Vec<String> {
    // [@ANCHOR: hamlib_rigctld_args]
    let mut a = vec!["-m".to_string(), m.model.to_string()];
    if let Some(d) = &m.device {
        a.extend(["-r".to_string(), d.clone()]);
    }
    if let Some(s) = m.speed {
        a.extend(["-s".to_string(), s.to_string()]);
    }
    a.extend([
        "-t".to_string(),
        port.to_string(),
        "-T".to_string(),
        "127.0.0.1".to_string(),
    ]);
    if !m.conf.is_empty() {
        let conf: Vec<String> = m.conf.iter().map(|(k, v)| format!("{k}={v}")).collect();
        a.push(format!("--set-conf={}", conf.join(",")));
    }
    a
}

fn free_port() -> Result<u16, String> {
    let l = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("no free localhost port: {e}"))?;
    l.local_addr()
        .map(|a| a.port())
        .map_err(|e| format!("no free localhost port: {e}"))
}

fn pid_alive(pid: i64) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    pid > 1 && unsafe { libc::kill(pid as libc::pid_t, 0) } == 0
}

fn cmdline_has(pid: i64, word: &str) -> bool {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|b| String::from_utf8_lossy(&b).contains(word))
        .unwrap_or(false)
}

fn state_file(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{}.rigctld.json", super::safe_name(name)))
}

/// Stops a leftover daemon of a dead server. A live owner means another bench_mcp manages this rig: an error, not a takeover.
pub fn reap_stale(name: &str, dir: &Path) -> Result<Option<u16>, String> {
    // [@ANCHOR: hamlib_reap_stale]
    let path = state_file(dir, name);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let owner = v["owner_pid"].as_i64().unwrap_or(0);
    let daemon = v["rigctld_pid"].as_i64().unwrap_or(0);
    let port = v["port"].as_u64().map(|p| p as u16);
    if owner != std::process::id() as i64 && pid_alive(owner) && cmdline_has(owner, "bench_mcp") {
        return Err(format!("rig {name:?} is managed by another bench_mcp process (pid {owner}); two servers must not share a rig. Next: use that server, or stop it."));
    }
    if daemon > 1 && pid_alive(daemon) && cmdline_has(daemon, "rigctld") {
        // The previous server died. The rig may be left transmitting; the transmit module releases it first.
        if let Some(p) = port {
            super::tx::release_after_crash(name, p);
        }
        // SAFETY: the pid was checked to be a rigctld of ours just above.
        unsafe { libc::kill(daemon as libc::pid_t, libc::SIGTERM) };
        let until = Instant::now() + Duration::from_secs(3);
        while pid_alive(daemon) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
        if pid_alive(daemon) {
            // SAFETY: as above.
            unsafe { libc::kill(daemon as libc::pid_t, libc::SIGKILL) };
        }
    }
    let _ = std::fs::remove_file(&path);
    Ok(port)
}

fn log_tail(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().rev().take(8).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join(" | ")
}

/// A serial device that is missing or not ours to open is reported before a daemon is started: rigctld itself keeps running after
/// it fails to open the rig, which would look like a rig that does not answer.
pub fn check_device(path: &str) -> Result<(), String> {
    // [@ANCHOR: hamlib_device_check]
    let meta = std::fs::metadata(path).map_err(|e| {
        format!("the rig's device {path} cannot be used ({e}). Next: check the cable and the path (bench_usb_find shows which tty belongs to which USB device; /dev/serial/by-id paths stay stable); then bench_hamlib_diagnose.")
    })?;
    let c = std::ffi::CString::new(path).map_err(|_| "device path has a NUL".to_string())?;
    // SAFETY: c is a valid C string for the call.
    if unsafe { libc::access(c.as_ptr(), libc::R_OK | libc::W_OK) } != 0 {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let kind = if meta.file_type().is_char_device() {
            "serial device"
        } else {
            "file"
        };
        return Err(format!("this user may not read and write the {kind} {path} (group id {}, mode {:o}). Next: add the user to that group (on Debian, dialout) and log in again; bench_mcp does not change permissions.", meta.gid(), meta.mode() & 0o777));
    }
    Ok(())
}

fn spawn(name: &str, m: &Managed, dir: &Path) -> Result<(Child, u16), String> {
    // [@ANCHOR: hamlib_rigctld_start]
    let log_path = dir.join(format!("{}.rigctld.log", super::safe_name(name)));
    if let Some(d) = m.device.as_deref().filter(|d| d.starts_with("/dev/")) {
        check_device(d)?;
    }
    let mut last = String::new();
    for _ in 0..3 {
        let port = match m.port {
            Some(p) => p,
            None => free_port()?,
        };
        let log =
            std::fs::File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
        let err = log
            .try_clone()
            .map_err(|e| format!("{}: {e}", log_path.display()))?;
        let mut child = Command::new("rigctld")
            .args(args(m, port))
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(err))
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "rigctld is not installed. Next: install the Hamlib utilities (Debian: apt install libhamlib-utils).".to_string()
                } else {
                    format!("cannot start rigctld: {e}")
                }
            })?;
        let until = Instant::now() + READY_WAIT;
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                last = format!(
                    "rigctld exited at start ({status}); its log says: {}",
                    log_tail(&log_path)
                );
                break;
            }
            if std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_millis(200),
            )
            .is_ok()
            {
                std::fs::write(
                    state_file(dir, name),
                    json!({"owner_pid": std::process::id(), "rigctld_pid": child.id(), "port": port}).to_string(),
                )
                .map_err(|e| format!("state file: {e}"))?;
                return Ok((child, port));
            }
            if Instant::now() > until {
                let _ = child.kill();
                let _ = child.wait();
                last = format!(
                    "rigctld did not accept connections within {} s; its log says: {}",
                    READY_WAIT.as_secs(),
                    log_tail(&log_path)
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if m.port.is_some() {
            break;
        }
    }
    Err(format!("{last}. Next: run bench_hamlib_diagnose (permissions on the device, a busy port, the model number and baud rate)."))
}

/// Starts the daemon if it is not running, within the restart limit. Returns its port.
fn ensure(name: &str, m: &Managed, cfg: &Config) -> Result<u16, String> {
    // [@ANCHOR: hamlib_rigctld_ensure]
    let dir = dir(cfg);
    let first = lock(&REAPED).insert(name.to_string());
    if first {
        reap_stale(name, &dir)?;
    }
    let mut st = lock(&STATE);
    let slot = st.entry(name.to_string()).or_insert(Slot {
        child: None,
        port: 0,
        starts: vec![],
        last_exit: None,
    });
    if let Some(child) = slot.child.as_mut() {
        match child.try_wait() {
            Ok(None) => return Ok(slot.port),
            Ok(Some(status)) => {
                slot.last_exit = Some(format!("exited with {status}"));
                slot.child = None;
            }
            Err(e) => return Err(format!("cannot check rigctld: {e}")),
        }
    }
    slot.starts.retain(|t| t.elapsed() < WINDOW);
    if slot.starts.len() >= MAX_STARTS {
        return Err(format!(
            "rigctld for {name:?} has been started {MAX_STARTS} times in {} minutes (last exit: {}); not restarting it again by itself. Next: bench_hamlib_diagnose to find the cause, then bench_hamlib_rigctld with action start.",
            WINDOW.as_secs() / 60,
            slot.last_exit.as_deref().unwrap_or("unknown")
        ));
    }
    let (child, port) = spawn(name, m, &dir)?;
    slot.starts.push(Instant::now());
    slot.child = Some(child);
    slot.port = port;
    Ok(port)
}

/// Where the commands for this rig go: an existing rigctld as configured, or the managed one, started if needed.
pub fn endpoint(name: &str, cfg: &Config) -> Result<(String, u16), String> {
    match &cfg.source {
        Source::Existing { host, port } => Ok((host.clone(), *port)),
        Source::Managed(m) => ensure(name, m, cfg).map(|p| ("127.0.0.1".to_string(), p)),
    }
}

/// Explicit start: forgets earlier failures so the restart limit starts again.
pub fn start(name: &str, cfg: &Config) -> Result<Value, String> {
    let Source::Managed(m) = &cfg.source else {
        return Err(format!(
            "{name:?} uses an existing rigctld; this server does not start or stop it"
        ));
    };
    if let Some(slot) = lock(&STATE).get_mut(name) {
        slot.starts.clear();
    }
    let port = ensure(name, m, cfg)?;
    Ok(status(name, cfg).tap_port(port))
}

trait TapPort {
    fn tap_port(self, port: u16) -> Value;
}
impl TapPort for Value {
    fn tap_port(mut self, port: u16) -> Value {
        self["port"] = json!(port);
        self
    }
}

/// Stops the daemon of a rig and removes its state file. Stopping is never refused.
pub fn stop(name: &str, cfg: &Config) -> Result<Value, String> {
    let Source::Managed(_) = &cfg.source else {
        return Err(format!(
            "{name:?} uses an existing rigctld; this server does not start or stop it"
        ));
    };
    let was = kill_slot(name);
    let _ = std::fs::remove_file(state_file(&dir(cfg), name));
    Ok(json!({"rigctld": "stopped", "was_running": was}))
}

pub fn kill_slot(name: &str) -> bool {
    let mut st = lock(&STATE);
    let Some(slot) = st.get_mut(name) else {
        return false;
    };
    let Some(mut child) = slot.child.take() else {
        return false;
    };
    let running = matches!(child.try_wait(), Ok(None));
    if running {
        // SAFETY: the pid is our own live child.
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
        let until = Instant::now() + Duration::from_secs(2);
        while matches!(child.try_wait(), Ok(None)) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
    }
    let _ = child.wait();
    running
}

/// Stops every managed daemon: called when the server stops.
pub fn shutdown() {
    let names: Vec<String> = lock(&STATE).keys().cloned().collect();
    for n in names {
        kill_slot(&n);
    }
}

/// What this server knows about the daemon of a rig, without starting it.
pub fn status(name: &str, cfg: &Config) -> Value {
    match &cfg.source {
        Source::Existing { host, port } => {
            json!({"source": "existing rigctld", "host": host, "port": port, "managed": false})
        }
        Source::Managed(m) => {
            let mut st = lock(&STATE);
            let mut out = json!({"source": "managed rigctld", "managed": true, "model": m.model, "listens_on": "127.0.0.1"});
            match st.get_mut(name) {
                Some(slot) => {
                    let running = slot
                        .child
                        .as_mut()
                        .is_some_and(|c| matches!(c.try_wait(), Ok(None)));
                    out["running"] = json!(running);
                    if running {
                        out["port"] = json!(slot.port);
                        out["pid"] = json!(slot.child.as_ref().map(|c| c.id()));
                    }
                    out["starts_in_last_10_min"] =
                        json!(slot.starts.iter().filter(|t| t.elapsed() < WINDOW).count());
                    out["last_exit"] = json!(slot.last_exit);
                }
                None => out["running"] = json!(false),
            }
            out
        }
    }
}

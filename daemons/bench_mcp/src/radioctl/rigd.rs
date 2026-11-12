// SPDX-License-Identifier: AGPL-3.0-or-later
//! The managed `radioctld`: one per configured rig, started by this server on a free localhost port, watched, restarted within a
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

// [@ANCHOR: bench_mcp_radioctl_rigd_slot]
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
    // [@ANCHOR: radioctl_managed_dir]
    let d = cfg
        .managed_dir
        .clone()
        .unwrap_or_else(|| crate::out_dir().join("radioctl"));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// The directory of the rig description files: the config's `rigs_dir`, else `RADIOCTL_RIGS` (radioctld's own variable).
pub fn rigs_dir(m: &Managed) -> Result<PathBuf, String> {
    // [@ANCHOR: radioctl_rigs_dir]
    m.rigs_dir
        .clone()
        .or_else(|| {
            std::env::var_os("RADIOCTL_RIGS")
                .filter(|d| !d.is_empty())
                .map(PathBuf::from)
        })
        .ok_or_else(|| "no rig description directory: set rigs_dir in the rig's config entry or RADIOCTL_RIGS (the rigs/ directory of radioctl, which holds <rig>.toml files). Next: bench_radioctl_models lists the rigs once it is set.".to_string())
}

/// The program that serves the rig: `BENCH_MCP_RADIOCTLD` if set, else `radioctld` from the search path.
pub fn program() -> std::ffi::OsString {
    // [@ANCHOR: bench_mcp_rigd_program]
    // The unit tests build radioctld from this repository when no program is named.
    #[cfg(test)]
    if std::env::var_os("BENCH_MCP_RADIOCTLD").is_none_or(|p| p.is_empty()) {
        return super::testkit::binary::radioctld().into_os_string();
    }
    std::env::var_os("BENCH_MCP_RADIOCTLD")
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "radioctld".into())
}

/// The radioctld configuration file text for a managed rig. Nothing here comes from a tool argument: rig, device and speed are the
/// owner's config. The daemon listens on 127.0.0.1 only. `transmit` opens the daemon's own transmit gate: it is true only for a rig
/// that has the owner's dummy-load attestation (the tools check the attestation and every limit before they key). Reads are never
/// served from the daemon's cache (`cache_max_age_ms = 0`): scans and the load check need what the rig says now. `tx_power_mw` is the
/// owner's `full_power_w`: it is the full scale the tools turn watts into a power setting with.
pub fn config_text(name: &str, m: &Managed, port: u16, rigs: &Path, transmit: bool) -> String {
    // [@ANCHOR: radioctl_daemon_args]
    let transport = match (&m.device, m.speed) {
        (Some(d), Some(s)) => format!("kind = \"serial\"\npath = \"{d}\"\nbaud = {s}\n"),
        _ => "kind = \"virtual\"\n".to_string(),
    };
    let power = m.full_power_w.map_or(String::new(), |w| {
        format!("tx_power_mw = {:.0}\n", w * 1000.0)
    });
    format!(
        "[server]\ncache_max_age_ms = 0\n\n[[rig]]\nname = \"{}\"\ndescription = \"{}\"\nlisten = \"127.0.0.1:{port}\"\ntransmit = {transmit}\n{power}\n[rig.transport]\n{transport}",
        super::safe_name(name),
        rigs.join(format!("{}.toml", m.rig)).display().to_string().replace('\\', "\\\\").replace('"', "\\\"")
    )
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
    dir.join(format!("{}.radioctld.json", super::safe_name(name)))
}

/// Stops a leftover daemon of a dead server. A live owner means another bench_mcp manages this rig: an error, not a takeover.
pub fn reap_stale(name: &str, dir: &Path) -> Result<Option<u16>, String> {
    // [@ANCHOR: radioctl_reap_stale]
    let path = state_file(dir, name);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let owner = v["owner_pid"].as_i64().unwrap_or(0);
    let daemon = v["radioctld_pid"].as_i64().unwrap_or(0);
    let port = v["port"].as_u64().map(|p| p as u16);
    if owner != std::process::id() as i64 && pid_alive(owner) && cmdline_has(owner, "bench_mcp") {
        return Err(format!("rig {name:?} is managed by another bench_mcp process (pid {owner}); two servers must not share a rig. Next: use that server, or stop it."));
    }
    if daemon > 1 && pid_alive(daemon) && cmdline_has(daemon, "radioctld") {
        // The previous server died. The rig may be left transmitting; the transmit module releases it first.
        if let Some(p) = port {
            super::tx::release_after_crash(name, p);
        }
        // SAFETY: the pid was checked to be a radioctld of ours just above.
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
    // [@ANCHOR: radioctl_log_tail]
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().rev().take(8).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join(" | ")
}

/// A serial device that is missing or not ours to open is reported before a daemon is started: radioctld itself keeps running after
/// it fails to open the rig, which would look like a rig that does not answer.
pub fn check_device(path: &str) -> Result<(), String> {
    // [@ANCHOR: radioctl_device_check]
    let meta = std::fs::metadata(path).map_err(|e| {
        format!("the rig's device {path} cannot be used ({e}). Next: check the cable and the path (bench_usb_find shows which tty belongs to which USB device; /dev/serial/by-id paths stay stable); then bench_radioctl_diagnose.")
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

fn spawn(name: &str, m: &Managed, dir: &Path, transmit: bool) -> Result<(Child, u16), String> {
    // [@ANCHOR: radioctl_daemon_start]
    let log_path = dir.join(format!("{}.radioctld.log", super::safe_name(name)));
    if let Some(d) = m.device.as_deref().filter(|d| d.starts_with("/dev/")) {
        check_device(d)?;
    }
    let rigs = rigs_dir(m)?;
    let desc = rigs.join(format!("{}.toml", m.rig));
    if !desc.is_file() {
        return Err(format!("no rig description {} for rig {:?}. Next: bench_radioctl_models lists the rigs; check rig and rigs_dir.", desc.display(), m.rig));
    }
    let conf_path = dir.join(format!("{}.radioctld.toml", super::safe_name(name)));
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
        std::fs::write(&conf_path, config_text(name, m, port, &rigs, transmit))
            .map_err(|e| format!("{}: {e}", conf_path.display()))?;
        let mut child = Command::new(program())
            .arg("--config")
            .arg(&conf_path)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(err))
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "radioctld is not installed. Next: build it from daemons/radioctl (cargo build --release -p radioctld) and put it on the search path, or set BENCH_MCP_RADIOCTLD to its path.".to_string()
                } else {
                    format!("cannot start radioctld: {e}")
                }
            })?;
        let until = Instant::now() + READY_WAIT;
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                last = format!(
                    "radioctld exited at start ({status}); its log says: {}",
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
                // Something accepted. It must be this daemon: if another program took the port in the meantime, ours fails to bind and exits.
                std::thread::sleep(Duration::from_millis(250));
                if let Ok(Some(status)) = child.try_wait() {
                    last = format!(
                        "radioctld exited just after start ({status}), probably because port {port} was taken; its log says: {}",
                        log_tail(&log_path)
                    );
                    break;
                }
                std::fs::write(
                    state_file(dir, name),
                    json!({"owner_pid": std::process::id(), "radioctld_pid": child.id(), "port": port}).to_string(),
                )
                .map_err(|e| format!("state file: {e}"))?;
                return Ok((child, port));
            }
            if Instant::now() > until {
                let _ = child.kill();
                let _ = child.wait();
                last = format!(
                    "radioctld did not accept connections within {} s; its log says: {}",
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
    Err(format!("{last}. Next: run bench_radioctl_diagnose (permissions on the device, a busy port, the model number and baud rate)."))
}

/// Starts the daemon if it is not running, within the restart limit. Returns its port.
fn ensure(name: &str, m: &Managed, cfg: &Config) -> Result<u16, String> {
    // [@ANCHOR: radioctl_daemon_ensure]
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
            Err(e) => return Err(format!("cannot check radioctld: {e}")),
        }
    }
    slot.starts.retain(|t| t.elapsed() < WINDOW);
    if slot.starts.len() >= MAX_STARTS {
        return Err(format!(
            "radioctld for {name:?} has been started {MAX_STARTS} times in {} minutes (last exit: {}); not restarting it again by itself. Next: bench_radioctl_diagnose to find the cause, then bench_radioctl_daemon with action start.",
            WINDOW.as_secs() / 60,
            slot.last_exit.as_deref().unwrap_or("unknown")
        ));
    }
    let (child, port) = spawn(name, m, &dir, cfg.transmit.is_some())?;
    slot.starts.push(Instant::now());
    slot.child = Some(child);
    slot.port = port;
    Ok(port)
}

/// Where the commands for this rig go: an existing radioctld as configured, or the managed one, started if needed.
pub fn endpoint(name: &str, cfg: &Config) -> Result<(String, u16), String> {
    // [@ANCHOR: radioctl_endpoint]
    match &cfg.source {
        Source::Existing { host, port } => Ok((host.clone(), *port)),
        Source::Managed(m) => ensure(name, m, cfg).map(|p| ("127.0.0.1".to_string(), p)),
    }
}

/// Explicit start: forgets earlier failures so the restart limit starts again.
pub fn start(name: &str, cfg: &Config) -> Result<Value, String> {
    // [@ANCHOR: radioctl_daemon_explicit_start]
    let Source::Managed(m) = &cfg.source else {
        return Err(format!(
            "{name:?} uses an existing radioctld; this server does not start or stop it"
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
    // [@ANCHOR: radioctl_daemon_stop]
    let Source::Managed(_) = &cfg.source else {
        return Err(format!(
            "{name:?} uses an existing radioctld; this server does not start or stop it"
        ));
    };
    let was = kill_slot(name);
    let _ = std::fs::remove_file(state_file(&dir(cfg), name));
    Ok(json!({"radioctld": "stopped", "was_running": was}))
}

pub fn kill_slot(name: &str) -> bool {
    // [@ANCHOR: radioctl_kill_slot]
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

/// Stops the managed daemons of the rigs whose names start with `prefix` (all of them for an empty prefix).
pub fn shutdown_prefix(prefix: &str) {
    // [@ANCHOR: radioctl_shutdown]
    let names: Vec<String> = lock(&STATE)
        .keys()
        .filter(|n| n.starts_with(prefix))
        .cloned()
        .collect();
    for n in names {
        kill_slot(&n);
    }
}

/// What this server knows about the daemon of a rig, without starting it.
pub fn status(name: &str, cfg: &Config) -> Value {
    // [@ANCHOR: radioctl_daemon_status]
    match &cfg.source {
        Source::Existing { host, port } => {
            json!({"source": "existing radioctld", "host": host, "port": port, "managed": false})
        }
        Source::Managed(m) => {
            let mut st = lock(&STATE);
            let mut out = json!({"source": "managed radioctld", "managed": true, "rig": m.rig, "listens_on": "127.0.0.1"});
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

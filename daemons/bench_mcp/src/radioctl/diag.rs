// SPDX-License-Identifier: AGPL-3.0-or-later
//! `bench_radioctl_diagnose`: why a rig does not answer. It reads the configuration and the system and never opens the serial port
//! itself and never starts the daemon: opening a serial port can change the DTR and RTS lines, and some interfaces key the
//! transmitter from them. It does talk to a daemon that is already running.

use super::proto::Client;
use super::{rigd, Config, Source};
use crate::audio::Run;
use serde_json::{json, Value};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::time::{Duration, Instant};

/// Processes (pid and name) that hold `target` open, found from /proc/*/fd. Our own pid is left out.
pub fn holders(target: &std::path::Path) -> Vec<String> {
    // [@ANCHOR: radioctl_device_holders]
    let Ok(real) = std::fs::canonicalize(target) else {
        return vec![];
    };
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return out;
    };
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if pid == std::process::id() {
            continue;
        }
        let Ok(fds) = std::fs::read_dir(e.path().join("fd")) else {
            continue;
        };
        if fds
            .flatten()
            .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|l| l == real))
        {
            let comm = std::fs::read_to_string(e.path().join("comm")).unwrap_or_default();
            out.push(format!("{pid} {}", comm.trim()));
        }
    }
    out
}

fn check(name: &str, ok: Option<bool>, detail: impl Into<String>, next: &str) -> Value {
    json!({"check": name, "ok": ok, "detail": detail.into(), "next": next})
}

/// What radioctld does with the DTR and RTS lines: it opens the port with both inactive and never toggles them. Nothing to configure.
fn lines_note(cfg: &Config) -> Value {
    // [@ANCHOR: radioctl_lines_note]
    let detail = match &cfg.source {
        Source::Managed(_) => {
            "radioctld opens the port 8N1 with DTR and RTS inactive and does not change them"
        }
        Source::Existing { .. } => "an existing radioctld owns the port; its own settings apply",
    };
    check("DTR and RTS lines", None, detail, "")
}

pub fn diagnose(run: &dyn Run, name: &str, cfg: &Config) -> Result<Value, String> {
    // [@ANCHOR: radioctl_diagnose]
    let mut checks = vec![];
    let status = rigd::status(name, cfg);
    let own_pid = status["pid"].as_u64().map(|p| p.to_string());
    let prog = rigd::program();
    let daemon_present = run.run(&prog.to_string_lossy(), &["--help".into()]);
    checks.push(match &daemon_present {
        Ok((_, v)) => check(
            "radioctld program",
            Some(true),
            v.lines().next().unwrap_or("").trim().to_string(),
            "",
        ),
        Err(e) => check(
            "radioctld program",
            Some(false),
            e.clone(),
            "build radioctld from daemons/radioctl and put it on the search path, or set BENCH_MCP_RADIOCTLD",
        ),
    });
    match &cfg.source {
        Source::Managed(m) => {
            checks.push(match rigd::rigs_dir(m) {
                Err(e) => check(
                    "rig description",
                    Some(false),
                    e,
                    "set rigs_dir or RADIOCTL_RIGS",
                ),
                Ok(dir) => {
                    let file = dir.join(format!("{}.toml", m.rig));
                    match super::models::description_name(&file) {
                        Ok(name) => check(
                            "rig description",
                            Some(true),
                            format!("{} ({})", name, file.display()),
                            "",
                        ),
                        Err(e) => check(
                            "rig description",
                            Some(false),
                            e,
                            "search with bench_radioctl_models",
                        ),
                    }
                }
            });
            if let Some(d) = &m.device {
                checks.push(match std::fs::metadata(d) {
                    Err(e) => check("device exists", Some(false), format!("{d}: {e}"), "check the cable and power; bench_usb_find shows the USB devices; use a /dev/serial/by-id path"),
                    Ok(meta) => {
                        let readable = rigd::check_device(d);
                        let kind = if meta.file_type().is_char_device() { "character device" } else { "not a character device" };
                        check("device access", Some(readable.is_ok()), format!("{d}: {kind}, group id {}, mode {:o}", meta.gid(), meta.mode() & 0o777), readable.err().as_deref().unwrap_or(""))
                    }
                });
                let others: Vec<String> = holders(std::path::Path::new(d))
                    .into_iter()
                    .filter(|h| Some(h.split(' ').next().unwrap_or("").to_string()) != own_pid)
                    .collect();
                checks.push(check("device busy", Some(others.is_empty()), if others.is_empty() { "no other process has it open".to_string() } else { format!("held open by: {}", others.join(", ")) }, if others.is_empty() { "" } else { "close the other program (another radioctld, a logger, a terminal program) or stop the process; two programs on one serial port garble both conversations" }));
            }
            checks.push(lines_note(cfg));
        }
        Source::Existing { .. } => checks.push(check(
            "source",
            None,
            "an existing radioctld is configured; its serial port is not this server's to check",
            "",
        )),
    }
    let endpoint = match (
        &cfg.source,
        status["running"].as_bool(),
        status["port"].as_u64(),
    ) {
        (Source::Existing { host, port }, _, _) => Some((host.clone(), *port)),
        (_, Some(true), Some(p)) => Some(("127.0.0.1".to_string(), p as u16)),
        _ => None,
    };
    match endpoint {
        None => checks.push(check(
            "radioctld running",
            Some(false),
            "the managed daemon is not running",
            "bench_radioctl_daemon start (this opens the serial port)",
        )),
        Some((host, port)) => {
            let began = Instant::now();
            match Client::connect(&host, port, Duration::from_secs(cfg.reply_timeout_s)).and_then(|mut c| c.read_number("get_freq", Some("Frequency"))) {
                Ok(f) => checks.push(check("the rig answers", Some(true), format!("frequency {f:.0} Hz in {} ms", began.elapsed().as_millis()), "")),
                Err(e) => checks.push(check("the rig answers", Some(false), e.to_string(), "see instruments/radioctl/troubleshooting: cable, power, baud rate and model, then restart radioctld, then power-cycle the USB port")),
            }
        }
    }
    if let Source::Managed(_) = &cfg.source {
        let log = rigd::dir(cfg).join(format!("{}.radioctld.log", super::safe_name(name)));
        if let Ok(text) = std::fs::read_to_string(&log) {
            let tail: Vec<&str> = text.lines().rev().take(6).collect();
            if !tail.is_empty() {
                checks.push(check(
                    "radioctld log",
                    None,
                    tail.into_iter().rev().collect::<Vec<_>>().join(" | "),
                    "",
                ));
            }
        }
    }
    let failing = checks.iter().filter(|c| c["ok"] == false).count();
    Ok(json!({"rig": name, "checks": checks, "failing": failing,
        "note": "This tool never opens the serial port and never starts the daemon; it reads the configuration and the system."}))
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! `bench_hamlib_diagnose`: why a rig does not answer. It reads the configuration and the system and never opens the serial port
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
    // [@ANCHOR: hamlib_device_holders]
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

/// Reads the serial line settings that the config sets, and says what the owner should set for a rig that can be keyed by DTR or RTS.
fn lines_note(cfg: &Config) -> Value {
    // [@ANCHOR: hamlib_lines_note]
    let Source::Managed(m) = &cfg.source else {
        return check(
            "DTR and RTS lines",
            None,
            "an existing rigctld owns the port; its own settings apply",
            "",
        );
    };
    let get = |k: &str| {
        m.conf
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
    };
    let (dtr, rts, ptt) = (get("dtr_state"), get("rts_state"), get("ptt_type"));
    let detail = format!(
        "dtr_state={}, rts_state={}, ptt_type={}",
        dtr.as_deref().unwrap_or("(Hamlib default)"),
        rts.as_deref().unwrap_or("(Hamlib default)"),
        ptt.as_deref().unwrap_or("(the backend's default)")
    );
    let set = dtr.is_some() && rts.is_some();
    check(
        "DTR and RTS lines",
        Some(set),
        detail,
        if set {
            ""
        } else {
            "Opening the port sets DTR and RTS. If the rig's interface keys the transmitter from either, set dtr_state and rts_state to OFF (and ptt_type) in set_conf before the first start; bench_mcp does not toggle the lines itself."
        },
    )
}

pub fn diagnose(run: &dyn Run, name: &str, cfg: &Config) -> Result<Value, String> {
    // [@ANCHOR: hamlib_diagnose]
    let mut checks = vec![];
    let status = rigd::status(name, cfg);
    let own_pid = status["pid"].as_u64().map(|p| p.to_string());
    let daemon_present = run.run("rigctld", &["--version".into()]);
    checks.push(match &daemon_present {
        Ok((_, v)) => check(
            "rigctld program",
            Some(true),
            v.lines().next().unwrap_or("").trim().to_string(),
            "",
        ),
        Err(e) => check(
            "rigctld program",
            Some(false),
            e.clone(),
            "install the Hamlib utilities (Debian: apt install libhamlib-utils)",
        ),
    });
    match &cfg.source {
        Source::Managed(m) => {
            match run.run("rigctl", &["-m".into(), m.model.to_string(), "-u".into()]) {
                Ok((_, text)) => match super::models::parse_caps(&text) {
                    Ok(c) => {
                        checks.push(check(
                            "model",
                            Some(true),
                            format!(
                                "{} {} ({}), port type {}, PTT type {}",
                                c.manufacturer, c.model_name, c.status, c.port_type, c.ptt_type
                            ),
                            "",
                        ));
                        if let Some(l) = text.lines().find(|l| l.starts_with("Serial speed")) {
                            let fits = m.speed.is_none_or(|s| speed_in(l, s));
                            checks.push(check(
                                "serial speed",
                                Some(fits),
                                format!(
                                    "backend: {l}; config speed: {}",
                                    m.speed
                                        .map_or("(backend default)".into(), |s| s.to_string())
                                ),
                                if fits {
                                    ""
                                } else {
                                    "set speed to a rate the backend allows"
                                },
                            ));
                        }
                    }
                    Err(_) => checks.push(check(
                        "model",
                        Some(false),
                        format!("rigctl does not know model {}", m.model),
                        "search with bench_hamlib_models",
                    )),
                },
                Err(e) => checks.push(check("model", None, e, "install the Hamlib utilities")),
            }
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
                checks.push(check("device busy", Some(others.is_empty()), if others.is_empty() { "no other process has it open".to_string() } else { format!("held open by: {}", others.join(", ")) }, if others.is_empty() { "" } else { "close the other program (another rigctld, a logger, a terminal program) or stop the process; two programs on one serial port garble both conversations" }));
            }
            checks.push(lines_note(cfg));
        }
        Source::Existing { .. } => checks.push(check(
            "source",
            None,
            "an existing rigctld is configured; its serial port is not this server's to check",
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
            "rigctld running",
            Some(false),
            "the managed daemon is not running",
            "bench_hamlib_rigctld start (this opens the serial port)",
        )),
        Some((host, port)) => {
            let began = Instant::now();
            match Client::connect(&host, port, Duration::from_secs(cfg.reply_timeout_s)).and_then(|mut c| c.read_number("get_freq", Some("Frequency"))) {
                Ok(f) => checks.push(check("the rig answers", Some(true), format!("frequency {f:.0} Hz in {} ms", began.elapsed().as_millis()), "")),
                Err(e) => checks.push(check("the rig answers", Some(false), e.to_string(), "see instruments/hamlib/troubleshooting: cable, power, baud rate and model, then restart rigctld, then power-cycle the USB port")),
            }
        }
    }
    if let Source::Managed(_) = &cfg.source {
        let log = rigd::dir(cfg).join(format!("{}.rigctld.log", super::safe_name(name)));
        if let Ok(text) = std::fs::read_to_string(&log) {
            let tail: Vec<&str> = text.lines().rev().take(6).collect();
            if !tail.is_empty() {
                checks.push(check(
                    "rigctld log",
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

/// True when `speed` lies in the "A..B baud" range of a `Serial speed:` line of `rigctl -u`.
pub fn speed_in(line: &str, speed: u32) -> bool {
    // [@ANCHOR: hamlib_speed_in]
    let range = line.split_whitespace().nth(2).unwrap_or("");
    match range.split_once("..") {
        Some((lo, hi)) => lo
            .parse::<u32>()
            .ok()
            .zip(hi.parse::<u32>().ok())
            .is_none_or(|(lo, hi)| (lo..=hi).contains(&speed)),
        None => true,
    }
}

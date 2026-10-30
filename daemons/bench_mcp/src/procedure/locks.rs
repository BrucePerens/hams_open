// SPDX-License-Identifier: AGPL-3.0-or-later
//! Instrument reservations between procedure runs. A tool that changes an instrument (any tier above read_only) makes the
//! run reserve that instrument; a second run that tries to drive it is refused with a clear error naming the run that holds
//! it. Reads share. A program may also reserve instruments for a stretch of code (`bench.reserve`), waiting up to a timeout.
//!
//! A reservation is a `flock` on a file in the managed directory, so it is released by the operating system when the run's
//! holder ends, even if the server was killed, and it holds between several server processes that share the directory.
//! Several instruments are taken in sorted order, all or none, and with a timeout, so two runs cannot deadlock each other.
//! Only procedure runs reserve: a direct call by an agent does not look at the reservations, which is the caller's business.

use crate::config::{Instrument, Kind};
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The file of an instrument's reservation: the name made safe for a file name.
fn lock_path(root: &Path, instrument: &str) -> PathBuf {
    // [@ANCHOR: proc_locks_path]
    let safe: String = instrument
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    root.join("locks").join(format!("{safe}.lock"))
}

/// The instrument a tool call is for, if it can be told: the `instrument` argument, the only instrument that fits a
/// generic tool, or the only instrument of the module's kind.
pub fn instrument_of(insts: &[Instrument], tool: &str, args: &Value) -> Option<String> {
    // [@ANCHOR: proc_locks_instrument_of]
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return Some(n.to_string());
    }
    if let Some(spec) = crate::TOOLS.iter().find(|t| t.name == tool) {
        return crate::pick(insts, spec, args).ok().map(|i| i.name.clone());
    }
    let kind = [
        ("bench_audio_", Kind::Audio),
        ("bench_example_", Kind::Example),
        ("bench_hamlib_", Kind::Hamlib),
        ("bench_rotator_", Kind::Rotator),
        ("bench_usb_", Kind::UsbHub),
    ]
    .iter()
    .find(|(p, _)| tool.starts_with(p))
    .map(|(_, k)| *k)?;
    let mut of_kind = insts.iter().filter(|i| i.kind == kind);
    match (of_kind.next(), of_kind.next()) {
        (Some(one), None) => Some(one.name.clone()),
        _ => None,
    }
}

/// Who holds a reservation, from its owner file, for the error message.
fn owner_text(root: &Path, instrument: &str) -> String {
    // [@ANCHOR: proc_locks_owner_text]
    let path = lock_path(root, instrument).with_extension("owner");
    let Ok(text) = std::fs::read_to_string(path) else {
        return "another procedure run".into();
    };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let since = v["since_unix"]
        .as_f64()
        .map(|t| (super::store::now_unix() - t).max(0.0) as u64);
    format!(
        "run {} ({:?}{})",
        v["run"].as_str().unwrap_or("?"),
        v["label"].as_str().unwrap_or(""),
        since
            .map(|s| format!(", holding it for {s} s"))
            .unwrap_or_default()
    )
}

/// Takes the reservations of `names` for run `run`, all or none, waiting up to `wait`. `cancelled` is polled while waiting.
/// Returns the held lock files (dropping them releases the reservations).
pub fn reserve(
    root: &Path,
    run: &str,
    label: &str,
    names: &[String],
    wait: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<(String, File)>, String> {
    // [@ANCHOR: proc_locks_reserve]
    let mut sorted: Vec<String> = names.to_vec();
    sorted.sort();
    sorted.dedup();
    std::fs::create_dir_all(root.join("locks")).map_err(|e| format!("locks: {e}"))?;
    let started = Instant::now();
    let mut held: Vec<(String, File)> = Vec::new();
    for name in sorted {
        let path = lock_path(root, &name);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        loop {
            // SAFETY: flock on a file descriptor this function owns.
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if rc == 0 {
                break;
            }
            if started.elapsed() >= wait || cancelled() {
                let who = owner_text(root, &name);
                drop(held);
                return Err(format!(
                    "instrument {name:?} is reserved by {who}. Wait for it to end (bench_proc_status), or call bench.reserve with a timeout to wait for it"
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let owner = json!({"run": run, "label": label, "pid": std::process::id(), "since_unix": super::store::now_unix()});
        let _ = std::fs::write(path.with_extension("owner"), owner.to_string());
        held.push((name, file));
    }
    Ok(held)
}

/// Releases one reservation: the owner note is removed, then the lock with the file.
pub fn release(root: &Path, held: &mut Vec<(String, File)>, name: &str) {
    // [@ANCHOR: proc_locks_release]
    if let Some(i) = held.iter().position(|(n, _)| n == name) {
        let _ = std::fs::remove_file(lock_path(root, name).with_extension("owner"));
        let (_, mut f) = held.remove(i);
        let _ = f.flush();
    }
}

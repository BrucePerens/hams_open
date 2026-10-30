// SPDX-License-Identifier: AGPL-3.0-or-later
//! The managed directory of the procedure runner. Everything a run produces is kept in `runs/<id>/` so that polling works
//! after the run ended and after a restart of the server: `state.json` (rewritten atomically at every change), `events.jsonl`
//! (appended), `stdout.txt` and `stderr.txt` (size-capped), `emitted.json`, `result.json`, `touched.json` (the safe-state
//! actions the run made necessary), `checkpoints/`, the source and arguments, and the program's scratch directory.

use super::Config;
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The largest events file; past it events are counted and dropped, with a final marker.
pub const EVENTS_CAP: u64 = 8 << 20;
/// The largest stdout or stderr file.
pub const STREAM_CAP: u64 = 1 << 20;

/// Seconds since the epoch.
pub fn now_unix() -> f64 {
    // [@ANCHOR: proc_store_now]
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// The managed directory: the `dir` of the config, or `procedures` under the bench output directory.
pub fn root(cfg: &Config) -> PathBuf {
    // [@ANCHOR: proc_store_root]
    cfg.dir
        .clone()
        .unwrap_or_else(|| crate::out_dir().join("procedures"))
}

/// Writes `v` to `path` so that a reader sees the old or the new file, never half of one.
pub fn write_json(path: &Path, v: &Value) -> Result<(), String> {
    // [@ANCHOR: proc_store_write_json]
    let tmp = path.with_extension("tmp");
    let text = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn read_json(path: &Path) -> Result<Value, String> {
    // [@ANCHOR: proc_store_read_json]
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// A new run directory: the id (sortable by start time, unique) and the path, created.
pub fn new_run_dir(cfg: &Config) -> Result<(String, PathBuf), String> {
    // [@ANCHOR: proc_store_new_run_dir]
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let runs = root(cfg).join("runs");
    std::fs::create_dir_all(&runs).map_err(|e| format!("{}: {e}", runs.display()))?;
    loop {
        let ms = (now_unix() * 1000.0) as u64;
        let n = COUNTER.fetch_add(1, Ordering::SeqCst) % 0x1000;
        let id = format!("r{ms:011x}{n:03x}");
        let dir = runs.join(&id);
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                std::fs::create_dir(dir.join("scratch")).map_err(|e| e.to_string())?;
                return Ok((id, dir));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", dir.display())),
        }
    }
}

/// The directory of a run id, refused unless the id has exactly the shape `new_run_dir` makes (no path tricks).
pub fn run_dir(cfg: &Config, id: &str) -> Result<PathBuf, String> {
    // [@ANCHOR: proc_store_run_dir]
    let ok =
        id.len() == 15 && id.starts_with('r') && id[1..].bytes().all(|b| b.is_ascii_hexdigit());
    if !ok {
        return Err(format!("{id:?} is not a run id (see bench_proc_list)"));
    }
    let d = root(cfg).join("runs").join(id);
    if d.is_dir() {
        Ok(d)
    } else {
        Err(format!(
            "no run {id} (see bench_proc_list; finished runs are deleted after the retention time)"
        ))
    }
}

/// The append-only list of a run's events. Each line is `{"seq", "t", "kind", ...}`; `seq` counts from 1 and is the cursor of
/// `bench_proc_output`.
pub struct EventLog {
    file: File,
    seq: u64,
    bytes: u64,
    started: f64,
    dropped: u64,
}

impl EventLog {
    pub fn open(dir: &Path, started: f64) -> Result<EventLog, String> {
        // [@ANCHOR: proc_store_event_open]
        let path = dir.join("events.jsonl");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(EventLog {
            file,
            seq: 0,
            bytes: 0,
            started,
            dropped: 0,
        })
    }

    /// Appends an event and returns its number (0 when the file is full and the event was dropped).
    pub fn append(&mut self, kind: &str, data: Value) -> u64 {
        // [@ANCHOR: proc_store_event_append]
        if self.bytes >= EVENTS_CAP {
            self.dropped += 1;
            return 0;
        }
        self.seq += 1;
        let mut obj = serde_json::Map::new();
        obj.insert("seq".into(), json!(self.seq));
        obj.insert(
            "t".into(),
            json!(((now_unix() - self.started) * 1000.0).round() / 1000.0),
        );
        obj.insert("kind".into(), json!(kind));
        match data {
            Value::Object(m) => obj.extend(m),
            other => {
                obj.insert("data".into(), other);
            }
        }
        let mut line = Value::Object(obj).to_string();
        if self.bytes + line.len() as u64 >= EVENTS_CAP {
            line = json!({"seq": self.seq, "kind": "truncated", "msg": "the event list is full; later events are counted, not stored"}).to_string();
        }
        line.push('\n');
        self.bytes += line.len() as u64;
        let _ = self.file.write_all(line.as_bytes());
        self.seq
    }

    pub fn count(&self) -> u64 {
        // [@ANCHOR: proc_store_event_count]
        self.seq
    }

    pub fn dropped(&self) -> u64 {
        // [@ANCHOR: proc_store_event_dropped]
        self.dropped
    }
}

/// The events after `cursor`, as many as fit in `max_bytes` of JSON text: (events, last seq returned, more left).
pub fn read_events(
    dir: &Path,
    cursor: u64,
    max_bytes: usize,
) -> Result<(Vec<Value>, u64, bool), String> {
    // [@ANCHOR: proc_store_read_events]
    let text = match std::fs::read_to_string(dir.join("events.jsonl")) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };
    let mut out = Vec::new();
    let mut used = 0usize;
    let mut last = cursor;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let seq = v.get("seq").and_then(|s| s.as_u64()).unwrap_or(0);
        if seq <= cursor {
            continue;
        }
        if used + line.len() > max_bytes && !out.is_empty() {
            return Ok((out, last, true));
        }
        used += line.len();
        last = seq;
        out.push(v);
    }
    Ok((out, last, false))
}

/// Appends `text` to a stream file (stdout or stderr) up to `STREAM_CAP`; returns the bytes dropped.
pub fn append_stream(path: &Path, text: &str) -> u64 {
    // [@ANCHOR: proc_store_append_stream]
    let have = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if have >= STREAM_CAP {
        return text.len() as u64;
    }
    let room = (STREAM_CAP - have) as usize;
    let mut cut = text.len().min(room);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(&text.as_bytes()[..cut]);
        if cut < text.len() {
            let _ = f.write_all(b"\n[output limit reached; the rest was dropped]\n");
        }
    }
    (text.len() - cut) as u64
}

/// Up to `max` bytes of a stream file from byte offset `from`: (text, next offset, more left).
pub fn read_stream(path: &Path, from: u64, max: usize) -> (String, u64, bool) {
    // [@ANCHOR: proc_store_read_stream]
    let Ok(mut f) = File::open(path) else {
        return (String::new(), from, false);
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if from >= len || f.seek(SeekFrom::Start(from)).is_err() {
        return (String::new(), from.min(len), false);
    }
    let want = ((len - from) as usize).min(max);
    let mut buf = vec![0u8; want];
    let n = f.read(&mut buf).unwrap_or(0);
    buf.truncate(n);
    (
        String::from_utf8_lossy(&buf).to_string(),
        from + n as u64,
        from + (n as u64) < len,
    )
}

/// Deletes finished runs older than the retention time and, beyond `keep_runs`, the oldest finished ones.
pub fn purge(cfg: &Config) -> usize {
    // [@ANCHOR: proc_store_purge]
    let runs = root(cfg).join("runs");
    let Ok(rd) = std::fs::read_dir(&runs) else {
        return 0;
    };
    let mut finished: Vec<(f64, PathBuf)> = Vec::new();
    for e in rd.flatten() {
        let Ok(st) = read_json(&e.path().join("state.json")) else {
            continue;
        };
        let state = st.get("state").and_then(|s| s.as_str()).unwrap_or("");
        if matches!(
            state,
            "starting" | "running" | "waiting_operator" | "cancelling"
        ) {
            continue;
        }
        let ended = st.get("ended_unix").and_then(|t| t.as_f64()).unwrap_or(0.0);
        finished.push((ended, e.path()));
    }
    finished.sort_by(|a, b| b.0.total_cmp(&a.0));
    let cutoff = now_unix() - cfg.retention_days * 86400.0;
    let mut removed = 0;
    for (i, (ended, path)) in finished.iter().enumerate() {
        if (*ended < cutoff || i >= cfg.keep_runs) && std::fs::remove_dir_all(path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Summaries of the runs on disk, newest first (up to `limit`).
pub fn list_runs(cfg: &Config, limit: usize) -> Vec<Value> {
    // [@ANCHOR: proc_store_list_runs]
    let runs = root(cfg).join("runs");
    let mut ids: Vec<String> = std::fs::read_dir(&runs)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.reverse();
    let mut out = Vec::new();
    for id in ids.into_iter().take(limit) {
        if let Ok(st) = read_json(&runs.join(&id).join("state.json")) {
            out.push(json!({
                "run": id,
                "state": st.get("state"),
                "label": st.get("label"),
                "started_unix": st.get("started_unix"),
                "ended_unix": st.get("ended_unix"),
                "calls": st.get("calls"),
            }));
        }
    }
    out
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! The audit log: one JSON object per line (`events.jsonl` style), append-only, written for
//! every request the broker decides on, allowed or refused, and for every peer it turns away.
//! Hand-formatted so there is no serialisation dependency; the only escaping needed is for the
//! short reason strings.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The log is moved to `<path>.1` (replacing an older one) when it reaches this size, so the log
/// directory never holds more than about twice this.
pub const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;
/// A peer turned away (`peer_rejected`) is logged once per UID per this many seconds.
pub const REJECTION_LOG_SECS: u64 = 60;
/// The most UIDs remembered for that throttle; past it the memory is cleared, which only means a
/// few extra lines.
const MAX_REMEMBERED_UIDS: usize = 1024;

/// One audit record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    /// Peer UID (`u32::MAX` when unknown).
    pub uid: u32,
    /// Peer PID.
    pub pid: i32,
    /// The action name (`open_device`, `list_devices`, `grant_group`, `provision_groups`,
    /// `peer_rejected`, `malformed`).
    pub action: &'static str,
    /// `allowed`, `denied`, `dry_run`, `applied`, `rate_limited`, `failed`, `rejected`.
    pub outcome: &'static str,
    /// Free text, bounded by the caller (a path, a reason).
    pub detail: String,
}

/// An open append-only log.
#[derive(Debug)]
pub struct AuditLog {
    file: Option<File>,
    path: Option<PathBuf>,
    size: u64,
    max_bytes: u64,
    last_rejection: HashMap<u32, u64>,
}

// [@ANCHOR: relay_broker_json_escape]
/// Escapes a string for a JSON literal (quotes, backslash, control characters).
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len().saturating_add(2));
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

impl AuditEvent {
    // [@ANCHOR: relay_broker_audit_event_to_json_line]
    /// The record as one JSON line (with the trailing newline). `ts` is Unix seconds.
    pub fn to_json_line(&self, ts: u64) -> String {
        format!(
            "{{\"ts\":{ts},\"uid\":{},\"pid\":{},\"action\":\"{}\",\"outcome\":\"{}\",\"detail\":\"{}\"}}\n",
            self.uid,
            self.pid,
            json_escape(self.action),
            json_escape(self.outcome),
            json_escape(&self.detail)
        )
    }
}

impl AuditLog {
    // [@ANCHOR: relay_broker_audit_log_open]
    /// Opens (creating, mode 0600) the log at `path`; `None` disables logging to a file, which
    /// the tests use. A log that cannot be opened is an error: the broker refuses to start
    /// unauditable.
    pub fn open(path: Option<&Path>) -> io::Result<AuditLog> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = match path {
            Some(p) => Some(
                OpenOptions::new()
                    .append(true)
                    .create(true)
                    .mode(0o600)
                    .open(p)?,
            ),
            None => None,
        };
        let size = match &file {
            Some(f) => f.metadata()?.len(),
            None => 0,
        };
        Ok(AuditLog {
            file,
            path: path.map(Path::to_path_buf),
            size,
            max_bytes: MAX_LOG_BYTES,
            last_rejection: HashMap::new(),
        })
    }

    // [@ANCHOR: relay_broker_audit_log_record]
    /// Appends one event. A write failure is reported but never stops the broker. A peer turned
    /// away is logged once per UID per minute (a group member not on the allow-list can connect as
    /// fast as it likes), and the file is rotated to `<path>.1` at the size cap.
    pub fn record(&mut self, event: &AuditEvent) -> io::Result<()> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.record_at(event, ts)
    }

    fn record_at(&mut self, event: &AuditEvent, ts: u64) -> io::Result<()> {
        if event.action == "peer_rejected" {
            if let Some(last) = self.last_rejection.get(&event.uid) {
                if ts.saturating_sub(*last) < REJECTION_LOG_SECS {
                    return Ok(());
                }
            }
            if self.last_rejection.len() >= MAX_REMEMBERED_UIDS {
                self.last_rejection.clear();
            }
            self.last_rejection.insert(event.uid, ts);
        }
        let line = event.to_json_line(ts);
        if self.size.saturating_add(line.len() as u64) > self.max_bytes {
            self.rotate()?;
        }
        match self.file.as_mut() {
            Some(f) => {
                f.write_all(line.as_bytes())?;
                self.size = self.size.saturating_add(line.len() as u64);
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// Moves the full log to `<path>.1` and starts an empty one (mode 0600).
    fn rotate(&mut self) -> io::Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        let mut old = path.clone().into_os_string();
        old.push(".1");
        std::fs::rename(&path, PathBuf::from(old))?;
        self.file = Some(
            OpenOptions::new()
                .append(true)
                .create(true)
                .mode(0o600)
                .open(&path)?,
        );
        self.size = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: relay_broker_json_escape]
    // Tests [@ANCHOR: relay_broker_audit_event_to_json_line]
    #[test]
    fn lines_are_valid_json_with_escaped_detail() {
        assert_eq!(json_escape("a\"b\\c\n\u{1}"), "a\\\"b\\\\c\\n\\u0001");
        let e = AuditEvent {
            uid: 1000,
            pid: 42,
            action: "open_device",
            outcome: "denied",
            detail: "/dev/\"x\"".into(),
        };
        assert_eq!(
            e.to_json_line(7),
            "{\"ts\":7,\"uid\":1000,\"pid\":42,\"action\":\"open_device\",\"outcome\":\"denied\",\"detail\":\"/dev/\\\"x\\\"\"}\n"
        );
    }

    // Tests [@ANCHOR: relay_broker_audit_log_open]
    // Tests [@ANCHOR: relay_broker_audit_log_record]
    #[test]
    fn records_append_to_a_private_file_and_none_is_a_sink() {
        let dir = std::env::temp_dir().join(format!("relay_broker_audit_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        let _ = std::fs::remove_file(&path);
        let e = AuditEvent {
            uid: 1,
            pid: 2,
            action: "list_devices",
            outcome: "allowed",
            detail: String::new(),
        };
        let mut log = AuditLog::open(Some(&path)).unwrap();
        log.record(&e).unwrap();
        log.record(&e).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("\"action\":\"list_devices\""));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(AuditLog::open(None).unwrap().record(&e).is_ok());
        assert!(AuditLog::open(Some(&dir.join("no/such/dir/x"))).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // Tests [@ANCHOR: relay_broker_audit_log_record]
    #[test]
    fn a_rejected_peer_is_logged_once_per_minute_and_the_log_rotates_at_the_cap() {
        let dir =
            std::env::temp_dir().join(format!("relay_broker_audit_rot_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(dir.join("events.jsonl.1"));
        let rejected = |uid| AuditEvent {
            uid,
            pid: 2,
            action: "peer_rejected",
            outcome: "rejected",
            detail: "gid 5".into(),
        };
        let mut log = AuditLog::open(Some(&path)).unwrap();
        log.record_at(&rejected(7), 1000).unwrap();
        log.record_at(&rejected(7), 1030).unwrap();
        log.record_at(&rejected(8), 1030).unwrap();
        log.record_at(&rejected(7), 1060).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 3);

        log.max_bytes = 300;
        let other = AuditEvent {
            uid: 1,
            pid: 2,
            action: "list_devices",
            outcome: "allowed",
            detail: String::new(),
        };
        for _ in 0..20 {
            log.record_at(&other, 2000).unwrap();
        }
        let current = std::fs::metadata(&path).unwrap().len();
        assert!(current <= 300, "current log is {current} bytes");
        assert!(dir.join("events.jsonl.1").exists());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

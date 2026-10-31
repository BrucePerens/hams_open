// SPDX-License-Identifier: AGPL-3.0-or-later
//! The audit log: one JSON object per line (`events.jsonl` style), append-only, written for
//! every request the broker decides on, allowed or refused, and for every peer it turns away.
//! Hand-formatted so there is no serialisation dependency; the only escaping needed is for the
//! short reason strings.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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
        Ok(AuditLog { file })
    }

    // [@ANCHOR: relay_broker_audit_log_record]
    /// Appends one event. A write failure is reported but never stops the broker.
    pub fn record(&mut self, event: &AuditEvent) -> io::Result<()> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = event.to_json_line(ts);
        match self.file.as_mut() {
            Some(f) => f.write_all(line.as_bytes()),
            None => Ok(()),
        }
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
}

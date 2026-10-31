// SPDX-License-Identifier: AGPL-3.0-or-later
//! The broker's configuration: a root-owned `key = value` file. Hand-parsed (no TOML crate) so
//! the input surface stays in this crate. Unknown keys are errors, not warnings: a typo in a
//! security file must not silently fall back to a default.
//!
//! Keys: `allow_uid`, `allow_gid` (repeatable), `apply_groups` (`true`/`false`, default false:
//! provisioning only reports), `audit_log` (path), `rate_per_minute`, `device_root`,
//! `serial_major`, `audio_major`, `deny_usb` (`vvvv:pppp`, repeatable), `deny_path` (repeatable),
//! `idle_exit_secs`. Setting any `serial_major`/`audio_major`/`deny_usb`/`deny_path` replaces the
//! default list for that key.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::auth::AllowList;
use crate::policy::Policy;

/// The parsed configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Who may talk to the broker.
    pub allow: AllowList,
    /// The open policy.
    pub policy: Policy,
    /// Whether `grant_group`/`provision_groups` change anything (default: report only).
    pub apply_groups: bool,
    /// Where the audit log goes; `None` logs nowhere (tests only).
    pub audit_log: Option<PathBuf>,
    /// Requests per minute per peer.
    pub rate_per_minute: u32,
    /// Exit after this many idle seconds (socket activation restarts the broker on demand).
    pub idle_exit_secs: u64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            allow: AllowList::default(),
            policy: Policy::default(),
            apply_groups: false,
            audit_log: Some(PathBuf::from("/var/log/hams-relay-broker/events.jsonl")),
            rate_per_minute: 60,
            idle_exit_secs: 60,
        }
    }
}

/// A configuration error, with the line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// 1-based line, 0 for file-level problems.
    pub line: usize,
    /// What was wrong.
    pub message: String,
}

// [@ANCHOR: relay_broker_parse_usb_pair]
/// Parses `vvvv:pppp` (hex) into a vendor/product pair.
fn parse_usb_pair(s: &str) -> Option<(u16, u16)> {
    let (v, p) = s.split_once(':')?;
    Some((
        u16::from_str_radix(v, 16).ok()?,
        u16::from_str_radix(p, 16).ok()?,
    ))
}

impl Config {
    // [@ANCHOR: relay_broker_config_parse]
    /// Parses the file text. Comments start with `#`; blank lines are skipped.
    pub fn parse(text: &str) -> Result<Config, ConfigError> {
        let mut cfg = Config::default();
        let mut seen_serial = false;
        let mut seen_audio = false;
        let mut seen_usb = false;
        let mut seen_path = false;
        for (idx, raw) in text.lines().enumerate() {
            let lineno = idx.saturating_add(1);
            let err = |m: &str| ConfigError {
                line: lineno,
                message: m.to_string(),
            };
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| err("expected key = value"))?;
            let (key, value) = (key.trim(), value.trim());
            match key {
                "allow_uid" => cfg
                    .allow
                    .uids
                    .push(value.parse().map_err(|_| err("bad uid"))?),
                "allow_gid" => cfg
                    .allow
                    .gids
                    .push(value.parse().map_err(|_| err("bad gid"))?),
                "apply_groups" => {
                    cfg.apply_groups = match value {
                        "true" => true,
                        "false" => false,
                        _ => return Err(err("expected true or false")),
                    }
                }
                "audit_log" => cfg.audit_log = Some(PathBuf::from(value)),
                "rate_per_minute" => {
                    cfg.rate_per_minute = value.parse().map_err(|_| err("bad number"))?
                }
                "idle_exit_secs" => {
                    cfg.idle_exit_secs = value.parse().map_err(|_| err("bad number"))?
                }
                "device_root" => {
                    if !value.starts_with('/') {
                        return Err(err("device_root must be absolute"));
                    }
                    cfg.policy.device_root = PathBuf::from(value);
                }
                "serial_major" => {
                    if !seen_serial {
                        cfg.policy.serial_majors.clear();
                        seen_serial = true;
                    }
                    cfg.policy
                        .serial_majors
                        .push(value.parse().map_err(|_| err("bad major"))?);
                }
                "audio_major" => {
                    if !seen_audio {
                        cfg.policy.audio_majors.clear();
                        seen_audio = true;
                    }
                    cfg.policy
                        .audio_majors
                        .push(value.parse().map_err(|_| err("bad major"))?);
                }
                "deny_usb" => {
                    if !seen_usb {
                        cfg.policy.deny_usb.clear();
                        seen_usb = true;
                    }
                    cfg.policy
                        .deny_usb
                        .push(parse_usb_pair(value).ok_or_else(|| err("expected vvvv:pppp"))?);
                }
                "deny_path" => {
                    if !seen_path {
                        cfg.policy.deny_paths.clear();
                        seen_path = true;
                    }
                    cfg.policy.deny_paths.push(PathBuf::from(value));
                }
                other => return Err(err(&format!("unknown key {other}"))),
            }
        }
        Ok(cfg)
    }

    // [@ANCHOR: relay_broker_config_load]
    /// Reads and parses `path`, first requiring it to be owned by `owner_uid` (0 in production)
    /// and writable by nobody else. The owner is a parameter so the tests can load a file they
    /// own; the binary passes 0.
    pub fn load(path: &Path, owner_uid: u32) -> Result<Config, ConfigError> {
        let file_err = |m: String| ConfigError {
            line: 0,
            message: m,
        };
        let meta = fs::metadata(path).map_err(|e: io::Error| file_err(e.to_string()))?;
        if meta.uid() != owner_uid {
            return Err(file_err(format!(
                "config must be owned by uid {owner_uid}, is owned by {}",
                meta.uid()
            )));
        }
        if meta.mode() & 0o022 != 0 {
            return Err(file_err(
                "config must not be group- or world-writable".into(),
            ));
        }
        let text = fs::read_to_string(path).map_err(|e| file_err(e.to_string()))?;
        Config::parse(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: relay_broker_config_parse]
    // Tests [@ANCHOR: relay_broker_parse_usb_pair]
    #[test]
    fn keys_are_closed_and_lists_replace_defaults() {
        let cfg = Config::parse(
            "# comment\nallow_uid = 1000\nallow_uid=1001\nallow_gid = 20 # trailing\napply_groups = true\n\
             rate_per_minute = 5\ndeny_usb = 08bb:29c0\ndeny_usb = 1234:abcd\nserial_major = 136\n\
             device_root = /dev\nidle_exit_secs = 5\naudit_log = /tmp/x.jsonl\n",
        )
        .unwrap();
        assert_eq!(cfg.allow.uids, vec![1000, 1001]);
        assert_eq!(cfg.allow.gids, vec![20]);
        assert!(cfg.apply_groups);
        assert_eq!(cfg.rate_per_minute, 5);
        assert_eq!(
            cfg.policy.deny_usb,
            vec![(0x08bb, 0x29c0), (0x1234, 0xabcd)]
        );
        assert_eq!(cfg.policy.serial_majors, vec![136]);
        assert_eq!(
            cfg.policy.audio_majors,
            vec![116],
            "untouched list keeps its default"
        );
        assert_eq!(cfg.idle_exit_secs, 5);
        assert!(!Config::default().apply_groups, "dry run by default");
        assert_eq!(Config::parse("bogus = 1").unwrap_err().line, 1);
        assert_eq!(Config::parse("\nallow_uid = x").unwrap_err().line, 2);
        assert!(Config::parse("apply_groups = yes").is_err());
        assert!(Config::parse("deny_usb = 08bb").is_err());
        assert!(Config::parse("device_root = dev").is_err());
        assert!(Config::parse("no equals here").is_err());
        assert_eq!(parse_usb_pair("08BB:29C0"), Some((0x08bb, 0x29c0)));
        assert_eq!(parse_usb_pair("zz:1"), None);
    }

    // Tests [@ANCHOR: relay_broker_config_load]
    #[test]
    fn load_requires_the_expected_owner_and_private_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("relay_broker_cfg_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broker.conf");
        fs::write(&path, "allow_uid = 7\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let me = nix::unistd::getuid().as_raw();
        assert_eq!(Config::load(&path, me).unwrap().allow.uids, vec![7]);
        // Root ownership cannot be tested unprivileged: a file owned by this uid is refused when
        // uid 0 is required, which is the same code path.
        assert!(Config::load(&path, me.wrapping_add(1))
            .unwrap_err()
            .message
            .contains("owned"));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(Config::load(&path, me)
            .unwrap_err()
            .message
            .contains("writable"));
        assert!(Config::load(&dir.join("missing"), me).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}

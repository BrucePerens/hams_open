// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `hams-relay-broker` program run as a process: its exit codes and its idle exit, with a
//! throwaway config and socket in a scratch directory. No privileges and no hardware.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::os::unix::fs::MetadataExt;
use std::process::Command;

struct ScratchDir(std::path::PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn broker() -> Command {
    Command::new(env!("CARGO_BIN_EXE_hams-relay-broker"))
}

// Tests [@ANCHOR: relay_broker_main]
#[test]
fn the_program_exits_2_on_bad_options_or_config_and_0_after_an_idle_run() {
    // temp-dir-helper: `ScratchDir` removes the directory in its Drop, also while a failing test unwinds
    let dir = std::env::temp_dir().join(format!("relay_broker_cli_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let _cleanup = ScratchDir(dir.clone());

    // An unknown option.
    let out = broker().arg("--bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown option"));

    // A config file that is not there.
    let missing = dir.join("none.conf");
    let out = broker().arg("--config").arg(&missing).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("none.conf"));

    // A good config but neither an activated socket nor --socket.
    let conf = dir.join("broker.conf");
    let audit = dir.join("audit.jsonl");
    std::fs::write(
        &conf,
        format!(
            "idle_exit_secs = 1\nallow_uid = 1000\naudit_log = {}\n",
            audit.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&conf, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    let uid = std::fs::metadata(&conf).unwrap().uid().to_string();
    let out = broker()
        .args(["--config-owner", &uid, "--config"])
        .arg(&conf)
        .env_remove("LISTEN_PID")
        .env_remove("LISTEN_FDS")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--socket"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // With a socket path it serves, hears nobody, and exits 0 after the idle time.
    let sock = dir.join("broker.sock");
    let out = broker()
        .args(["--config-owner", &uid, "--config"])
        .arg(&conf)
        .arg("--socket")
        .arg(&sock)
        .env_remove("LISTEN_PID")
        .env_remove("LISTEN_FDS")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

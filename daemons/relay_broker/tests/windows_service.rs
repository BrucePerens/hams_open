// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Windows broker as a program: started by hand (console) and started by the service control
//! manager. Both listen on the fixed pipe `\\.\pipe\hams-relay-broker`, so the two tests run one
//! after the other, and neither works while the real `HamsRelayBroker` service is running. The
//! service test creates and deletes its own service, so it needs an elevated account.
#![cfg(windows)]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use relay_broker::windows::{call, current_user_sid, PIPE_NAME};
use relay_broker::wire::{Request, Response};

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn exe() -> &'static str {
    env!("CARGO_BIN_EXE_hams-relay-broker")
}

fn write_config(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("relay_broker_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("broker.conf");
    let sid = current_user_sid().expect("own sid");
    std::fs::write(&path, format!("set_time = false\nallow_sid = {sid}\n")).expect("config");
    path
}

fn status_when_up() -> Response {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match call(PIPE_NAME, &Request::TimeStatus) {
            Ok(r) => return r,
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => panic!("the broker never answered: {e}"),
        }
    }
}

fn powershell(script: &str) -> String {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .expect("powershell");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

// Tests [@ANCHOR: relay_broker_windows_run_main]
// Tests [@ANCHOR: relay_broker_windows_serve_forever]
// Tests [@ANCHOR: relay_broker_windows_run_as_service]
#[test]
fn started_by_hand_the_broker_serves_the_pipe_and_a_second_copy_cannot_take_it() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let config = write_config("console");
    let mut child = Command::new(exe())
        .arg("--config")
        .arg(&config)
        .spawn()
        .expect("start the broker");
    let answer = status_when_up();
    // A second broker must not become a second server on the same name.
    let second = Command::new(exe())
        .arg("--config")
        .arg(&config)
        .output()
        .expect("second broker");
    child.kill().expect("stop the broker this test started");
    child.wait().expect("reap it");
    assert!(
        matches!(
            answer,
            Response::TimeStatus {
                steps_last_hour: 0,
                breaker_open: false,
                ..
            }
        ),
        "{answer:?}"
    );
    assert!(
        !second.status.success(),
        "a second broker took the same pipe"
    );
}

// Tests [@ANCHOR: relay_broker_windows_service_main]
// Tests [@ANCHOR: relay_broker_windows_control_handler]
// Tests [@ANCHOR: relay_broker_windows_report]
#[test]
fn started_and_stopped_by_the_service_control_manager_it_answers_and_stops_cleanly() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let config = write_config("service");
    let name = "HamsRelayBrokerTest";
    let script = format!(
        "New-Service -Name {name} -BinaryPathName '\"{exe}\" --config \"{config}\"' -StartupType Manual | Out-Null; \
         Start-Service {name}; (Get-Service {name}).Status",
        exe = exe(),
        config = config.display()
    );
    let started = powershell(&script);
    let answer = if started.trim().ends_with("Running") {
        Some(status_when_up())
    } else {
        None
    };
    let stopped = powershell(&format!(
        "Stop-Service {name}; (Get-Service {name}).Status; sc.exe delete {name} | Out-Null"
    ));
    assert!(
        started.trim().ends_with("Running"),
        "the service did not start (run elevated): {started}"
    );
    assert!(
        matches!(answer, Some(Response::TimeStatus { .. })),
        "{answer:?}"
    );
    assert!(
        stopped.trim().ends_with("Stopped"),
        "the service did not stop cleanly: {stopped}"
    );
}

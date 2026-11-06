// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctld` stops in order (and so unkeys every rig) on SIGTERM and SIGHUP as well as on
//! SIGINT. Before 2026-11-05 only SIGINT was caught: a `kill` or `systemctl stop` ended the
//! process at once and left a keyed KX3 keyed. Each run keys a virtual rig through the rigctld
//! port, sends the signal and expects the orderly stop (its log line and exit status 0, not death
//! by the signal). The unkey that the stop performs is covered by the service tests.
#![cfg(unix)]
#![allow(clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

fn config(dir: &PathBuf, port: u16) -> PathBuf {
    let rigs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../rigs/elecraft-kx3.toml");
    let text = format!(
        "[[rig]]\nname = \"kx3\"\ndescription = {:?}\nlisten = \"127.0.0.1:{port}\"\n\
         transmit = true\nmax_transmit_s = 30\n[rig.transport]\nkind = \"virtual\"\n",
        rigs.display().to_string()
    );
    let path = dir.join("radioctld.toml");
    std::fs::write(&path, text).expect("write config");
    path
}

fn key_then_signal(signal: &str) {
    let dir = std::env::temp_dir().join(format!(
        "radioctld-stop-{signal}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("dir");
    let port = free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_radioctld"))
        .arg("--config")
        .arg(config(&dir, port))
        .stderr(Stdio::piped())
        .spawn()
        .expect("start radioctld");
    let mut stderr = BufReader::new(child.stderr.take().expect("stderr"));
    let mut line = String::new();
    stderr.read_line(&mut line).expect("first line");
    assert!(line.contains("listening"), "{line:?}");

    let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    client.write_all(b"T 1\nt\n").expect("key");
    let mut reply = String::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut buf = [0u8; 256];
    while !reply.contains("RPRT 0\n1\n") && Instant::now() < deadline {
        let n = client.read(&mut buf).expect("read");
        reply.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    assert!(reply.contains("RPRT 0\n1\n"), "keyed: {reply:?}");

    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(child.id().to_string())
        .status()
        .expect("kill");
    assert!(status.success());
    let mut rest = String::new();
    stderr.read_to_string(&mut rest).expect("rest of stderr");
    let exit = child.wait().expect("wait");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(exit.code(), Some(0), "{signal}: {exit:?} {rest:?}");
    assert!(rest.contains("radioctld: stopping ("), "{signal}: {rest:?}");
}

// Tests [@ANCHOR: radioctld_main_stop_signal]
// Tests [@ANCHOR: radioctld_main_run]
#[test]
fn sigterm_sighup_and_sigint_all_stop_the_daemon_in_order() {
    for signal in ["TERM", "HUP", "INT"] {
        key_then_signal(signal);
    }
}

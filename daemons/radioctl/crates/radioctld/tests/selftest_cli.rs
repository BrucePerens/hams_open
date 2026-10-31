// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctl-selftest` end to end on pseudo-terminals that hold virtual radios of different
//! models: discovery of several rigs in one run, the ambiguous and not-a-radio cases, one upload
//! per rig to a loopback server, and the first-run prompt. Nothing leaves this machine.
#![allow(clippy::expect_used, clippy::panic)]

mod common;

use std::io::{Cursor, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use common::rigs_dir;
use radioctl_core::usb::PortInfo;
use radioctl_protocols::virtual_rig::VirtualRig;
use radioctld::rigs::load_description;
use radioctld::selftest_cli::{parse_args, run, Host};
use radioctld::upload::Contact;
use serialport::{SerialPort, TTYPort};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A pty. With `Some(id)` a virtual radio of that description answers on it; with `None` nothing
/// answers. `received` counts the bytes written to the slave side.
struct Pty {
    path: String,
    received: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Pty {
    fn new(id: Option<&str>) -> Pty {
        let rig = id.map(|id| {
            let d = load_description(&rigs_dir().join(format!("{id}.toml"))).expect("description");
            Mutex::new(VirtualRig::from_description(&d).expect("virtual rig"))
        });
        let (mut master, slave) = TTYPort::pair().expect("pty pair");
        let path = slave.name().expect("slave name");
        drop(slave);
        let received = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (r, s) = (Arc::clone(&received), Arc::clone(&stop));
        let thread = thread::spawn(move || {
            master
                .set_timeout(Duration::from_millis(20))
                .expect("timeout");
            let started = Instant::now();
            let mut buffer = [0u8; 256];
            while !s.load(Ordering::Acquire) {
                let Ok(n) = master.read(&mut buffer) else {
                    continue;
                };
                r.fetch_add(n, Ordering::AcqRel);
                if let Some(rig) = &rig {
                    let reply = rig
                        .lock()
                        .expect("lock")
                        .feed(buffer.get(..n).unwrap_or(&[]), started.elapsed());
                    if !reply.is_empty() {
                        let _ = master.write_all(&reply);
                    }
                }
            }
        });
        Pty {
            path,
            received,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn port(path: &str, vid: u16, product: &str) -> PortInfo {
    PortInfo {
        path: PathBuf::from(path),
        vid: Some(vid),
        pid: Some(0xea60),
        serial: Some("SERIAL-PRIVATE".into()),
        product: Some(product.into()),
        cdc_acm: false,
    }
}

fn host(ports: Vec<PortInfo>) -> Host {
    Host {
        ports,
        interactive: false,
        contact_file: None,
        env_contact: Contact::default(),
        lan_window: Duration::ZERO,
        update: None,
        restart_after_update: false,
        enter_pressed: None,
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("selftest-cli-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

/// A loopback server that answers 204 to every POST and keeps the bodies.
async fn collector() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!(
        "http://{}/api/rig-reports",
        listener.local_addr().expect("addr")
    );
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&bodies);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let kept = Arc::clone(&kept);
            tokio::spawn(async move {
                let mut got = Vec::new();
                let mut buf = [0u8; 8192];
                loop {
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    got.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&got).to_string();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let want: usize = head
                            .lines()
                            .find_map(|l| l.strip_prefix("Content-Length: "))
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                        if body.len() >= want {
                            kept.lock().expect("lock").push(body.to_string());
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                let _ = socket
                    .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
            });
        }
    });
    (url, bodies)
}

// Tests [@ANCHOR: radioctld_cli_run]
// Tests [@ANCHOR: radioctld_discovery_discover_ports]
// Tests [@ANCHOR: radioctld_upload_post_report]
// Tests [@ANCHOR: radioctld_upload_build_payload]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_run_tests_every_identified_rig_and_uploads_one_report_each() {
    let (ic, ts) = (
        Pty::new(Some("icom-ic-7300")),
        Pty::new(Some("kenwood-ts-2000")),
    );
    let (gps, generic, nobody) = (Pty::new(None), Pty::new(None), Pty::new(None));
    let ports = vec![
        port(&ic.path, 0x10c4, "IC-7300"),
        port(&ts.path, 0x0403, "TS-2000 CAT"),
        port(&gps.path, 0x1546, "u-blox GPS receiver"),
        port(
            &generic.path,
            0x10c4,
            "CP2102 USB to UART Bridge Controller",
        ),
        port(&nobody.path, 0x10c4, "TS-590SG"),
    ];
    let dir = temp_dir("multi");
    let (url, bodies) = collector().await;
    let mut o = parse_args(&[
        "--unmanaged".into(),
        "--out-dir".into(),
        dir.display().to_string(),
        "--server".into(),
        url,
        "--name".into(),
        "Ann Ham".into(),
        "--email".into(),
        "ann@example.org".into(),
    ])
    .expect("args");
    o.baud = radioctld::selftest_cli::BaudArg::Fixed(19_200);
    let mut out = Vec::new();
    let status = run(o, host(ports), &mut Cursor::new(""), &mut out).await;
    let shown = String::from_utf8_lossy(&out).to_string();
    assert_eq!(status, 0, "{shown}");
    assert!(
        shown.contains("ambiguous: candidates kenwood-ts-590s, kenwood-ts-590sg"),
        "{shown}"
    );
    assert!(
        shown.contains("skipped, not a radio") && shown.contains("unidentified"),
        "{shown}"
    );
    // Nothing was written to the GPS or to the unidentified bridge.
    assert_eq!(gps.received.load(Ordering::Acquire), 0);
    assert_eq!(generic.received.load(Ordering::Acquire), 0);
    for file in [
        "radioctl-selftest-icom-ic-7300.txt",
        "radioctl-selftest-kenwood-ts-2000.txt",
        "radioctl-selftest-summary.txt",
    ] {
        assert!(dir.join(file).is_file(), "{file}\n{shown}");
    }
    assert_eq!(shown.matches("Thank You!").count(), 2, "{shown}");
    assert!(
        shown.contains("Tested 2 radios; 2 reports uploaded."),
        "{shown}"
    );
    let ic_report =
        std::fs::read_to_string(dir.join("radioctl-selftest-icom-ic-7300.txt")).expect("report");
    assert!(
        ic_report.contains("unsolicited status (turn the VFO knob)")
            && ic_report.contains("skipped: no terminal"),
        "{ic_report}"
    );
    assert!(!shown.contains("Please turn the VFO knob"), "{shown}");
    let bodies = bodies.lock().expect("lock").clone();
    assert_eq!(bodies.len(), 2, "{shown}");
    let docs: Vec<serde_json::Value> = bodies
        .iter()
        .map(|b| serde_json::from_str(b).expect("json"))
        .collect();
    assert_ne!(docs[0]["report_id"], docs[1]["report_id"]);
    let mut ids: Vec<&str> = docs
        .iter()
        .filter_map(|d| d["description_id"].as_str())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, ["icom-ic-7300", "kenwood-ts-2000"]);
    for (body, doc) in bodies.iter().zip(&docs) {
        assert_eq!(doc["contact"]["email"], "ann@example.org");
        assert_eq!(
            doc["link"]["usb_vid_pid"].as_str().map(|s| s.len()),
            Some(9)
        );
        for private in ["SERIAL-PRIVATE", "/dev/", "pts"] {
            assert!(!body.contains(private), "{private}");
        }
    }
    // The raw bytes of a rig reach the payload.
    assert!(docs.iter().any(|d| d["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .any(|c| c["raw_hex"].is_string())));
    std::fs::remove_dir_all(&dir).ok();
}

// Tests [@ANCHOR: radioctld_cli_run]
// Tests [@ANCHOR: radioctld_cli_thanks]
// Tests [@ANCHOR: radioctld_upload_post_report]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_upload_never_changes_the_exit_status() {
    let closed = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}/x", closed.local_addr().expect("addr"));
    drop(closed);
    let dir = temp_dir("offline");
    let o = parse_args(&[
        "--description".into(),
        "icom-ic-7300".into(),
        "--port".into(),
        "virtual".into(),
        "--out".into(),
        dir.join("r.txt").display().to_string(),
        "--server".into(),
        url,
    ])
    .expect("args");
    let mut out = Vec::new();
    assert_eq!(
        run(o, host(vec![]), &mut Cursor::new(""), &mut out).await,
        0
    );
    let shown = String::from_utf8_lossy(&out);
    assert!(
        shown.contains("upload failed") && !shown.contains("Thank You"),
        "{shown}"
    );
    assert!(dir.join("r.txt").is_file());
    let report = std::fs::read_to_string(dir.join("r.txt")).expect("report");
    assert!(report.contains("nothing was transmitted") && report.contains("PASS read frequency A"));
    std::fs::remove_dir_all(&dir).ok();
}

// Tests [@ANCHOR: radioctld_cli_run]
// Tests [@ANCHOR: radioctld_upload_prompt_contact]
// Tests [@ANCHOR: radioctld_upload_resolve_contact]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_interactive_run_asks_once_and_a_scripted_run_never_asks() {
    let dir = temp_dir("prompt");
    let file = dir.join("config/selftest-contact.json");
    let (url, bodies) = collector().await;
    let args = |extra: &[&str]| {
        let mut a: Vec<String> = [
            "--description",
            "icom-ic-7300",
            "--port",
            "virtual",
            "--out",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        a.push(dir.join("r.txt").display().to_string());
        a.extend(["--server".to_string(), url.clone()]);
        a.extend(extra.iter().map(|s| s.to_string()));
        parse_args(&a).expect("args")
    };
    let mut interactive = host(vec![]);
    interactive.interactive = true;
    interactive.contact_file = Some(file.clone());
    let mut out = Vec::new();
    run(
        args(&[]),
        interactive,
        &mut Cursor::new("Ann Ham\nann@example.org\n"),
        &mut out,
    )
    .await;
    assert!(String::from_utf8_lossy(&out).contains("contact you about your rig"));
    assert!(file.is_file());
    let mut again = host(vec![]);
    again.interactive = true;
    again.contact_file = Some(file.clone());
    let mut out = Vec::new();
    run(args(&[]), again, &mut Cursor::new(""), &mut out).await;
    assert!(!String::from_utf8_lossy(&out).contains("what is your name"));
    // A scripted run (not interactive) with no saved file asks nothing and blocks on nothing.
    let mut out = Vec::new();
    run(args(&[]), host(vec![]), &mut Cursor::new(""), &mut out).await;
    assert!(!String::from_utf8_lossy(&out).contains("what is your name"));
    let bodies = bodies.lock().expect("lock").clone();
    assert_eq!(bodies.len(), 3);
    let contact = |i: usize| {
        serde_json::from_str::<serde_json::Value>(&bodies[i]).expect("json")["contact"].clone()
    };
    assert_eq!(contact(0)["name"], "Ann Ham");
    assert_eq!(contact(1)["email"], "ann@example.org");
    assert_eq!(contact(2), serde_json::json!({}));
    std::fs::remove_dir_all(&dir).ok();
}

// Tests [@ANCHOR: radioctld_cli_main_from_env]
// Tests [@ANCHOR: radioctld_cli_main_with_bundle]
#[test]
fn the_binary_reports_its_version_lists_embedded_rigs_and_rejects_bad_usage() {
    let bin = env!("CARGO_BIN_EXE_radioctl-selftest");
    let run = |args: &[&str]| {
        std::process::Command::new(bin)
            .args(args)
            .env_remove("RADIOCTL_RIGS")
            .output()
            .expect("runs")
    };
    let version = run(&["--version"]);
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("radioctl-selftest "));
    let list = run(&["--list"]);
    assert!(String::from_utf8_lossy(&list.stdout).contains("icom-ic-7300"));
    assert_eq!(run(&["--bogus"]).status.code(), Some(2));
}

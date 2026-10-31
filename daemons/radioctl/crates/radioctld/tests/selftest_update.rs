// SPDX-License-Identifier: AGPL-3.0-or-later
//! The self-update of `radioctl-selftest` against a release channel on loopback: newer, same,
//! older, a bad signature or hash, offline, non-interactive, and the atomic replace. No test
//! contacts the real hams.com.
#![allow(clippy::expect_used, clippy::panic)]

use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::time::Duration;

use radioctld::selftest_cli::{parse_args, run, update_flow, Host};
use radioctld::update::UpdateConfig;
use radioctld::upload::Contact;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use zipsign_api::SigningKey;

fn entry() -> &'static str {
    if cfg!(windows) {
        "radioctl-selftest.exe"
    } else {
        "radioctl-selftest"
    }
}

fn bundle(seed: u8, content: &[u8]) -> (Vec<u8>, [u8; 32], String) {
    let mut raw = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut raw));
        zip.start_file(entry(), zip::write::SimpleFileOptions::default())
            .expect("start");
        zip.write_all(content).expect("write");
        zip.finish().expect("finish");
    }
    let key = SigningKey::from_bytes(&[seed; 32]);
    let public = key.verifying_key().to_bytes();
    let mut out = Cursor::new(Vec::new());
    zipsign_api::sign::copy_and_sign_zip(&mut Cursor::new(raw), &mut out, &[key], None)
        .expect("sign");
    let bytes = out.into_inner();
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    (bytes, public, sha)
}

async fn channel(version: &str, sha: &str, bundle: Vec<u8>) -> String {
    let info =
        serde_json::json!({"result": {"status": "success", "version": version, "sha256": sha}})
            .to_string();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (info, bundle) = (info.clone(), bundle.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let body = if head.contains("latest_info") {
                    info.into_bytes()
                } else {
                    bundle
                };
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            });
        }
    });
    base
}

struct Setup {
    dir: PathBuf,
    exe: PathBuf,
}

impl Setup {
    fn new(name: &str) -> Setup {
        let dir =
            std::env::temp_dir().join(format!("selftest-update-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let exe = dir.join(entry());
        std::fs::write(&exe, b"OLD PROGRAM").expect("exe");
        Setup { dir, exe }
    }

    fn host(&self, base: String, keys: Vec<[u8; 32]>, interactive: bool) -> Host {
        Host {
            ports: vec![],
            interactive,
            contact_file: None,
            env_contact: Contact::default(),
            lan_window: Duration::ZERO,
            update: Some(UpdateConfig {
                base_url: base,
                current_version: "0.3.3".into(),
                keys,
                exe: self.exe.clone(),
            }),
            restart_after_update: false,
        }
    }

    fn content(&self) -> Vec<u8> {
        std::fs::read(&self.exe).expect("exe")
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

async fn flow(args: &[&str], host: &Host, answer: &str) -> (Option<i32>, String) {
    let o = parse_args(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).expect("args");
    let mut out = Vec::new();
    let status = update_flow(
        &o,
        host,
        host.update.as_ref().expect("cfg"),
        &mut Cursor::new(answer.to_string()),
        &mut out,
    )
    .await;
    (status, String::from_utf8_lossy(&out).to_string())
}

// Tests [@ANCHOR: radioctld_cli_update_flow]
// Tests [@ANCHOR: radioctld_update_replace_exe]
#[tokio::test]
async fn a_newer_version_is_installed_only_on_a_yes_or_update_and_keeps_the_old_program() {
    let (zip, public, sha) = bundle(7, b"NEW PROGRAM");
    let base = channel("0.4.0", &sha, zip).await;
    // Not interactive and no --update: a notice only, never silently.
    let s = Setup::new("notice");
    let (status, shown) = flow(&[], &s.host(base.clone(), vec![public], false), "").await;
    assert_eq!(status, None);
    assert!(
        shown.contains("newer radioctl-selftest (0.4.0) is available")
            && shown.contains("--update"),
        "{shown}"
    );
    assert_eq!(s.content(), b"OLD PROGRAM");
    // Interactive no.
    let (status, shown) = flow(&[], &s.host(base.clone(), vec![public], true), "n\n").await;
    assert_eq!(status, None);
    assert!(shown.contains("update now? [y/N]"), "{shown}");
    assert_eq!(s.content(), b"OLD PROGRAM");
    // Interactive yes.
    let (status, shown) = flow(&[], &s.host(base.clone(), vec![public], true), "y\n").await;
    assert_eq!(status, Some(0), "{shown}");
    assert_eq!(s.content(), b"NEW PROGRAM");
    assert_eq!(
        std::fs::read(s.dir.join(format!("{}.prev", entry()))).expect("prev"),
        b"OLD PROGRAM"
    );
    // --update without a terminal.
    let s2 = Setup::new("forced");
    let (status, _) = flow(&["--update"], &s2.host(base, vec![public], false), "").await;
    assert_eq!(status, Some(0));
    assert_eq!(s2.content(), b"NEW PROGRAM");
}

// Tests [@ANCHOR: radioctld_cli_update_flow]
#[tokio::test]
async fn the_same_or_an_older_version_says_nothing_and_changes_nothing() {
    let (zip, public, sha) = bundle(7, b"NEW PROGRAM");
    for version in ["0.3.3", "0.2.0"] {
        let base = channel(version, &sha, zip.clone()).await;
        let s = Setup::new("same");
        let (status, shown) = flow(&["--update"], &s.host(base, vec![public], true), "y\n").await;
        assert_eq!((status, shown.as_str()), (None, ""));
        assert_eq!(s.content(), b"OLD PROGRAM");
    }
}

// Tests [@ANCHOR: radioctld_cli_update_flow]
// Tests [@ANCHOR: radioctld_update_verify_and_extract]
#[tokio::test]
async fn a_bad_signature_or_hash_leaves_the_old_program_untouched() {
    let (zip, public, sha) = bundle(7, b"NEW PROGRAM");
    let (other_zip, _, other_sha) = bundle(8, b"EVIL PROGRAM");
    // Signed by a key that is not the release key.
    let base = channel("0.4.0", &other_sha, other_zip).await;
    let s = Setup::new("badsig");
    let (status, shown) = flow(&["--update"], &s.host(base, vec![public], false), "").await;
    assert_eq!(status, None);
    assert!(
        shown.contains("the existing program is unchanged"),
        "{shown}"
    );
    assert_eq!(s.content(), b"OLD PROGRAM");
    // The published hash does not match the download.
    let base = channel("0.4.0", &"0".repeat(64), zip).await;
    let (status, shown) = flow(&["--update"], &s.host(base, vec![public], false), "").await;
    assert_eq!(status, None);
    assert!(
        shown.contains("sha256") && shown.contains("unchanged"),
        "{shown}"
    );
    assert_eq!(s.content(), b"OLD PROGRAM");
    assert!(!s.dir.join(format!("{}.prev", entry())).exists());
    let _ = sha;
}

// Tests [@ANCHOR: radioctld_cli_update_flow]
// Tests [@ANCHOR: radioctld_cli_run]
#[tokio::test]
async fn offline_is_one_line_and_the_test_still_runs() {
    let closed = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base = format!("http://{}", closed.local_addr().expect("addr"));
    drop(closed);
    let s = Setup::new("offline");
    let host = s.host(base, vec![], true);
    let (status, shown) = flow(&[], &host, "").await;
    assert_eq!(status, None);
    assert_eq!(shown.lines().count(), 1, "{shown}");
    assert!(
        shown.starts_with("Could not check for a newer radioctl-selftest"),
        "{shown}"
    );
    let o = parse_args(&[
        "--description".into(),
        "icom-ic-7300".into(),
        "--port".into(),
        "virtual".into(),
        "--no-upload".into(),
        "--out".into(),
        s.dir.join("r.txt").display().to_string(),
    ])
    .expect("args");
    let mut out = Vec::new();
    assert_eq!(
        run(o, host, &mut Cursor::new(String::new()), &mut out).await,
        0
    );
    assert!(String::from_utf8_lossy(&out).contains("Could not check"));
    assert!(s.dir.join("r.txt").is_file());
    // --no-update-check does not even look.
    let o = parse_args(&["--no-update-check".into(), "--list".into()]).expect("args");
    let mut out = Vec::new();
    run(
        o,
        s.host("http://127.0.0.1:9".into(), vec![], false),
        &mut Cursor::new(String::new()),
        &mut out,
    )
    .await;
    assert!(!String::from_utf8_lossy(&out).contains("Could not check"));
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! A TS-890S on its LAN port served by radioctld: the production path (configuration text with
//! `framing = "ts890-lan"`, the password in a secret file, `daemon::start`, real TCP) against a
//! fake radio on loopback that demands `##CN;` and the `##ID` login before it speaks CAT, counts
//! the keep-alives, refuses a wrong password, and can drop its connections so the link reconnects
//! and logs in again. The radio behind the login is the virtual TS-890S. Nothing leaves the
//! machine; nothing is keyed; no real radio is involved.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{quiet_options, rigs_dir, Client};
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::scope::ScopeAxis;
use radioctl_core::units::Hz;
use radioctl_protocols::virtual_rig::VirtualRig;
use radioctld::config::Config;
use radioctld::daemon;
use radioctld::rigs::load_description;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

#[derive(Default)]
struct Counts {
    connections: AtomicU32,
    logins: AtomicU32,
    refused: AtomicU32,
    keepalives: AtomicU32,
}

/// A fake TS-890S LAN port: `##CN;` then `##ID00705kenwoodadmin;` (the guide's example account),
/// then the virtual TS-890S. `connections` holds the live connection tasks so a test can drop
/// them all, as a radio that was power-cycled would.
struct Radio {
    addr: std::net::SocketAddr,
    counts: Arc<Counts>,
    connections: Arc<Mutex<Vec<JoinHandle<()>>>>,
    /// The virtual rig behind the login, so a test can make it send a sweep.
    rig: Arc<Mutex<VirtualRig>>,
    /// Bytes the rig sends on its own, written to every logged-in connection.
    pushes: broadcast::Sender<Vec<u8>>,
}

async fn radio() -> Radio {
    let file = std::fs::read_to_string(rigs_dir().join("kenwood-ts-890s.toml")).expect("file");
    let description = rigfile::parse(&file).expect("ts890 file").description;
    let rig = Arc::new(Mutex::new(
        VirtualRig::from_description(&description).expect("rig"),
    ));
    let counts = Arc::new(Counts::default());
    let connections = Arc::new(Mutex::new(Vec::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (pushes, _) = broadcast::channel::<Vec<u8>>(16);
    let (rig2, counts2, connections2, pushes2) = (
        Arc::clone(&rig),
        Arc::clone(&counts),
        Arc::clone(&connections),
        pushes.clone(),
    );
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut pushed = pushes2.subscribe();
            counts2.connections.fetch_add(1, Ordering::SeqCst);
            let (rig, counts) = (Arc::clone(&rig2), Arc::clone(&counts2));
            let task = tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 512];
                let mut stage = 0;
                loop {
                    // The guide's rule: no communication for 10 seconds closes the connection.
                    let n = tokio::select! {
                        bytes = pushed.recv() => {
                            match bytes {
                                Ok(bytes) if stage == 2 => {
                                    if stream.write_all(&bytes).await.is_err() {
                                        return;
                                    }
                                }
                                Err(broadcast::error::RecvError::Closed) => return,
                                _ => {}
                            }
                            continue;
                        }
                        read = timeout(Duration::from_secs(10), stream.read(&mut chunk)) => {
                            match read {
                                Ok(Ok(n)) if n > 0 => n,
                                _ => return,
                            }
                        }
                    };
                    buf.extend_from_slice(&chunk[..n]);
                    while let Some(end) = buf.iter().position(|b| *b == b';') {
                        let frame: Vec<u8> = buf.drain(..=end).collect();
                        let text = String::from_utf8_lossy(&frame).into_owned();
                        let reply: Vec<u8> = match stage {
                            0 if text == "##CN;" => {
                                stage = 1;
                                b"##CN1;".to_vec()
                            }
                            1 if text.starts_with("##ID") => {
                                if text == "##ID00705kenwoodadmin;" {
                                    counts.logins.fetch_add(1, Ordering::SeqCst);
                                    stage = 2;
                                    b"##ID1;".to_vec()
                                } else {
                                    counts.refused.fetch_add(1, Ordering::SeqCst);
                                    let _ = stream.write_all(b"##ID0;").await;
                                    return;
                                }
                            }
                            2 => {
                                if text == "ID;" {
                                    counts.keepalives.fetch_add(1, Ordering::SeqCst);
                                }
                                rig.lock().expect("rig").feed(&frame, Duration::ZERO)
                            }
                            _ => {
                                counts.refused.fetch_add(1, Ordering::SeqCst);
                                Vec::new()
                            }
                        };
                        if !reply.is_empty() && stream.write_all(&reply).await.is_err() {
                            return;
                        }
                    }
                }
            });
            connections2.lock().expect("tasks").push(task);
        }
    });
    Radio {
        addr,
        counts,
        connections,
        rig,
        pushes,
    }
}

impl Radio {
    /// The rig sends one bandscope sweep (`##DD2`) and one filter-scope sweep (`##DD3`) on its own,
    /// as a TS-890S with its waveform output on LAN does.
    fn sweep(&self) {
        let mut rig = self.rig.lock().expect("rig");
        let mut bytes = rig.scope_sweep();
        bytes.extend(rig.filter_scope_sweep());
        let _ = self.pushes.send(bytes);
    }

    /// Drops every live connection, as a power cycle would. The listener stays.
    fn drop_connections(&self) {
        for task in self.connections.lock().expect("tasks").drain(..) {
            task.abort();
        }
    }
}

/// A secret directory of its own for one test, with `ts890-password` holding `password`, mode
/// 0600 (the store refuses anything others can read).
fn secrets(tag: &str, password: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("radioctld-ts890-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("ts890-password");
    std::fs::write(&file, format!("{password}\n")).expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    }
    dir
}

/// Configuration text for a TS-890S at `addr` over the LAN session, the password from `dir`.
fn text(addr: std::net::SocketAddr, dir: &Path, extra: &str) -> String {
    format!(
        "[server]\nsecrets_dir = {:?}\n[[rig]]\nname = \"ts890\"\ndescription = {:?}\n\
         listen = \"127.0.0.1:0\"\n[rig.transport]\nkind = \"tcp\"\naddress = \"{addr}\"\n\
         framing = \"ts890-lan\"\nlan_account = \"kenwood\"\nlan_administrator = true\n\
         lan_password_secret = \"ts890-password\"\n{extra}",
        dir.display().to_string(),
        rigs_dir()
            .join("kenwood-ts-890s.toml")
            .display()
            .to_string()
    )
}

async fn start(text: &str) -> Result<daemon::Daemon, String> {
    let config = Config::parse(text, Path::new("."))?;
    let descriptions = config
        .rigs
        .iter()
        .map(|r| load_description(&r.description).expect("description"))
        .collect();
    daemon::start(&Handle::current(), &config, descriptions, quiet_options).await
}

async fn until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        sleep(Duration::from_millis(25)).await;
    }
}

// Tests [@ANCHOR: radioctld_rigs_tcp_framing]
// Tests [@ANCHOR: radioctld_rigs_build_rig]
// Tests [@ANCHOR: radioctld_config_tcp_framing]
// Tests [@ANCHOR: radioctl_ts890lan_new]
// Tests [@ANCHOR: radioctl_ts890lan_keepalive]
// Tests [@ANCHOR: radioctl_core_net_open_link]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_daemon_logs_in_serves_cat_keeps_the_line_alive_and_logs_in_again_after_a_drop() {
    let radio = radio().await;
    let dir = secrets("good", "admin");
    let daemon = start(&text(radio.addr, &dir, "")).await.expect("daemon");
    assert_eq!(radio.counts.logins.load(Ordering::SeqCst), 1);
    assert!(daemon.rigs[0].net_link.is_some());
    let mut client = Client::connect(&daemon).await;
    assert_eq!(client.ask("F 14074000").await, "RPRT 0\n");
    assert_eq!(client.ask("f").await, "14074000\n");
    // The keep-alive (ID; every 4 s) reaches the radio and its answer never reaches the client.
    until("a keep-alive", || {
        radio.counts.keepalives.load(Ordering::SeqCst) >= 1
    })
    .await;
    assert_eq!(client.ask("f").await, "14074000\n");
    assert_eq!(radio.counts.connections.load(Ordering::SeqCst), 1);
    // The radio drops the connection: the link reconnects and logs in again, and the rig is
    // served again. The radio's state survived (it is the same virtual rig), the session did not.
    radio.drop_connections();
    until("a second login", || {
        radio.counts.logins.load(Ordering::SeqCst) >= 2
    })
    .await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let reply = client.ask("f").await;
        if reply == "14074000\n" {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "after the drop: {reply:?}"
        );
        sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(radio.counts.refused.load(Ordering::SeqCst), 0);
    daemon.stop().await;
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

// Tests [@ANCHOR: radioctld_rigs_tcp_framing]
// Tests [@ANCHOR: radioctl_station_ha_filesecrets_secret]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_password_a_missing_secret_or_a_readable_one_fails_closed_without_showing_it() {
    let radio = radio().await;
    // Wrong password: the radio answers ##ID0; and the daemon does not start. The error names
    // the rig and says the login was refused; the password is in no message.
    let dir = secrets("wrong", "hunter2-wrong");
    let err = start(&text(radio.addr, &dir, ""))
        .await
        .err()
        .expect("refused");
    assert!(err.starts_with("rig ts890:"), "{err}");
    assert!(err.contains("wrong account or password"), "{err}");
    assert!(!err.contains("hunter2"), "{err}");
    assert_eq!(radio.counts.refused.load(Ordering::SeqCst), 1);
    std::fs::remove_dir_all(&dir).expect("cleanup");
    // A missing secret: nothing is connected at all.
    let before = radio.counts.connections.load(Ordering::SeqCst);
    let dir = secrets("missing", "admin");
    std::fs::remove_file(dir.join("ts890-password")).expect("remove");
    let err = start(&text(radio.addr, &dir, ""))
        .await
        .err()
        .expect("missing");
    assert!(err.contains("ts890-password"), "{err}");
    assert_eq!(radio.counts.connections.load(Ordering::SeqCst), before);
    std::fs::remove_dir_all(&dir).expect("cleanup");
    // A secret others can read is refused before it is used; the value stays out of the error.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = secrets("readable", "s3cret-value");
        std::fs::set_permissions(
            dir.join("ts890-password"),
            std::fs::Permissions::from_mode(0o644),
        )
        .expect("mode");
        let err = start(&text(radio.addr, &dir, ""))
            .await
            .err()
            .expect("readable");
        assert!(err.contains("0600"), "{err}");
        assert!(!err.contains("s3cret"), "{err}");
        assert_eq!(radio.counts.connections.load(Ordering::SeqCst), before);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
    // A password the login frame cannot carry (a `;`) never reaches the wire either.
    let dir = secrets("unsendable", "ad;min");
    let err = start(&text(radio.addr, &dir, ""))
        .await
        .err()
        .expect("unsendable");
    assert!(
        err.contains("cannot be sent") && !err.contains("ad;min"),
        "{err}"
    );
    assert_eq!(radio.counts.connections.load(Ordering::SeqCst), before);
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

// Tests [@ANCHOR: radioctld_rigs_build_rig]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_raw_tcp_rig_is_on_the_reconnecting_link_too() {
    // A plain CAT stream over TCP: the fake here is the virtual TS-890S without a login gate.
    let file = std::fs::read_to_string(rigs_dir().join("kenwood-ts-890s.toml")).expect("file");
    let description = rigfile::parse(&file).expect("ts890 file").description;
    let rig = Arc::new(Mutex::new(
        VirtualRig::from_description(&description).expect("rig"),
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let connections = Arc::new(AtomicU32::new(0));
    let (rig2, connections2) = (Arc::clone(&rig), Arc::clone(&connections));
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let n = connections2.fetch_add(1, Ordering::SeqCst) + 1;
            let rig = Arc::clone(&rig2);
            tokio::spawn(async move {
                let mut chunk = [0u8; 512];
                let mut served = 0;
                while let Ok(k) = stream.read(&mut chunk).await {
                    if k == 0 {
                        return;
                    }
                    let reply = rig.lock().expect("rig").feed(&chunk[..k], Duration::ZERO);
                    if !reply.is_empty() && stream.write_all(&reply).await.is_err() {
                        return;
                    }
                    served += 1;
                    // The first connection hangs up after a few commands; later ones stay.
                    if n == 1 && served == 3 {
                        return;
                    }
                }
            });
        }
    });
    let text = format!(
        "[[rig]]\nname = \"ts890\"\ndescription = {:?}\nlisten = \"127.0.0.1:0\"\n\
         [rig.transport]\nkind = \"tcp\"\naddress = \"{addr}\"\n",
        rigs_dir()
            .join("kenwood-ts-890s.toml")
            .display()
            .to_string()
    );
    let daemon = start(&text).await.expect("daemon");
    let mut client = Client::connect(&daemon).await;
    assert_eq!(client.ask("F 7074000").await, "RPRT 0\n");
    assert_eq!(client.ask("f").await, "7074000\n");
    // The radio hung up; the link comes back and the rig is served again.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let reply = client.ask("f").await;
        if reply == "7074000\n" && connections.load(Ordering::SeqCst) >= 2 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "after the drop: {reply:?}"
        );
        sleep(Duration::from_millis(100)).await;
    }
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_rigs_tcp_framing]
// Tests [@ANCHOR: radioctl_core_handle_scope_frames]
// Tests [@ANCHOR: radioctl_core_scope_stream_next]
// Tests [@ANCHOR: radioctl_core_handle_filter_scope_frames]
// Tests [@ANCHOR: radioctl_core_filter_scope_stream_next]
// Tests [@ANCHOR: radioctl_ts890_engine_scope_frame]
// Tests [@ANCHOR: radioctl_ts890_engine_filter_scope_frame]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_lan_sweeps_reach_the_scope_streams_through_the_daemon() {
    let radio = radio().await;
    let dir = secrets("sweeps", "admin");
    let daemon = start(&text(radio.addr, &dir, "")).await.expect("daemon");
    let rig = daemon.rigs[0].service.rig();
    let mut scope = rig.scope_frames();
    let mut filter = rig.filter_scope_frames();
    rig.execute(Op::SetFrequency(
        radioctl_core::units::Vfo::A,
        Hz(14_074_000),
    ))
    .await
    .expect("frequency");
    rig.execute(Op::SetTransceive(true))
        .await
        .expect("transceive");
    rig.execute(Op::SetScopeOutput(true))
        .await
        .expect("output on");
    assert_eq!(
        rig.execute(Op::GetScopeAxis(0)).await.expect("axis"),
        Some(Observation::ScopeAxis(0, ScopeAxis::Center))
    );
    radio.sweep();
    let frame = timeout(Duration::from_secs(10), scope.next())
        .await
        .expect("a bandscope frame arrives")
        .expect("stream open");
    assert_eq!((frame.seq, frame.samples.len()), (1, 640));
    assert_eq!((frame.low, frame.high), (Hz(14_049_000), Hz(14_099_000)));
    let filter_frame = timeout(Duration::from_secs(10), filter.next())
        .await
        .expect("a filter-scope frame arrives")
        .expect("stream open");
    assert_eq!((filter_frame.seq, filter_frame.samples.len()), (1, 213));
    let stats = rig.stats();
    assert_eq!((stats.scope_frames, stats.filter_scope_frames), (1, 1));
    assert_eq!(stats.unrecognised, 0);
    // The CAT side still works while the sweeps flow.
    let mut client = Client::connect(&daemon).await;
    assert_eq!(client.ask("f").await, "14074000\n");
    daemon.stop().await;
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

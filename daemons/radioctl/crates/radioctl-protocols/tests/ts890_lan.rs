// SPDX-License-Identifier: AGPL-3.0-or-later
//! The TS-890S LAN session (`##CN`, `##ID`, the 10-second idle rule) over the real network link
//! against a fake radio on loopback: the radio demands the connection request and the login before
//! it speaks CAT, refuses a wrong password, denies a second connection, and hangs up after an idle
//! limit (scaled down from the guide's 10 seconds) unless the keep-alive keeps the line busy. The
//! radio behind it is the virtual TS-890S. Nothing leaves the machine; nothing is keyed.
#![allow(clippy::expect_used, clippy::panic)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use radioctl_core::backoff::Backoff;
use radioctl_core::error::ErrorKind;
use radioctl_core::net::{open_link, NetConfig};
use radioctl_core::op::{Alarm, Observation, Op};
use radioctl_core::rig::{spawn, Freshness, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::serial::LinkStatus;
use radioctl_core::units::{Hz, Vfo};
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::kenwood_ts890_lan::{LanCredentials, Ts890LanFraming};
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::time::{sleep, timeout};

const FILE: &str = include_str!("../../../rigs/kenwood-ts-890s.toml");

struct Counts {
    connections: AtomicU32,
    logins: AtomicU32,
    idle_drops: AtomicU32,
    refused: AtomicU32,
    /// `AI2;` frames the radio received (counted after the login, over every connection).
    ai2: AtomicU32,
    /// Bumped to make the radio hang up on every open connection at once.
    kill: tokio::sync::watch::Sender<u32>,
}

impl Default for Counts {
    fn default() -> Counts {
        Counts {
            connections: AtomicU32::new(0),
            logins: AtomicU32::new(0),
            idle_drops: AtomicU32::new(0),
            refused: AtomicU32::new(0),
            ai2: AtomicU32::new(0),
            kill: tokio::sync::watch::channel(0).0,
        }
    }
}

/// A TS-890S on its LAN port. `idle` is the no-communication limit (10 seconds in the guide).
async fn radio(
    idle: Duration,
    deny: bool,
) -> (std::net::SocketAddr, Arc<Counts>, Arc<Mutex<VirtualRig>>) {
    let description = rigfile::parse(FILE).expect("ts890 file").description;
    let rig = Arc::new(Mutex::new(
        VirtualRig::from_description(&description).expect("rig"),
    ));
    let counts = Arc::new(Counts::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (rig2, counts2) = (Arc::clone(&rig), Arc::clone(&counts));
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            counts2.connections.fetch_add(1, Ordering::SeqCst);
            let (rig, counts) = (Arc::clone(&rig2), Arc::clone(&counts2));
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 512];
                let mut stage = 0;
                let mut killed = counts.kill.subscribe();
                loop {
                    let read = tokio::select! {
                        read = timeout(idle, stream.read(&mut chunk)) => read,
                        _ = killed.changed() => return,
                    };
                    let n = match read {
                        Err(_) => {
                            counts.idle_drops.fetch_add(1, Ordering::SeqCst);
                            return;
                        }
                        Ok(Ok(n)) if n > 0 => n,
                        _ => return,
                    };
                    buf.extend_from_slice(&chunk[..n]);
                    while let Some(end) = buf.iter().position(|b| *b == b';') {
                        let frame: Vec<u8> = buf.drain(..=end).collect();
                        let text = String::from_utf8_lossy(&frame).into_owned();
                        let reply: Vec<u8> = match stage {
                            0 if text == "##CN;" => {
                                stage = 1;
                                if deny {
                                    b"##CN0;".to_vec()
                                } else {
                                    b"##CN1;".to_vec()
                                }
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
                                if text == "AI2;" {
                                    counts.ai2.fetch_add(1, Ordering::SeqCst);
                                }
                                rig.lock().expect("rig").feed(&frame, Duration::ZERO)
                            }
                            _ => {
                                // CAT before the login: the radio says nothing.
                                counts.refused.fetch_add(1, Ordering::SeqCst);
                                Vec::new()
                            }
                        };
                        if !reply.is_empty() && stream.write_all(&reply).await.is_err() {
                            return;
                        }
                        if deny {
                            return;
                        }
                    }
                }
            });
        }
    });
    (addr, counts, rig)
}

fn config(addr: std::net::SocketAddr) -> NetConfig {
    let mut c = NetConfig::new(addr.to_string());
    c.connect_timeout = Duration::from_millis(500);
    c.handshake_timeout = Duration::from_millis(500);
    c.write_timeout = Duration::from_millis(500);
    c.backoff = Backoff {
        initial: Duration::from_millis(20),
        max: Duration::from_millis(80),
        factor: 2,
    };
    c
}

fn session(password: &str, keepalive: Duration) -> Box<Ts890LanFraming> {
    Box::new(
        Ts890LanFraming::new(LanCredentials::new(true, "kenwood", password).expect("credentials"))
            .with_keepalive(keepalive),
    )
}

// Tests [@ANCHOR: radioctl_ts890lan_decode]
// Tests [@ANCHOR: radioctl_ts890lan_greeting]
// Tests [@ANCHOR: radioctl_ts890lan_keepalive_sent]
// Tests [@ANCHOR: radioctl_ts890_lan_connect_request]
// Tests [@ANCHOR: radioctl_ts890_lan_login]
// Tests [@ANCHOR: radioctl_ts890_lan_reply]
#[tokio::test]
async fn the_login_then_cat_then_keepalive_keeps_the_line_up_past_the_idle_limit() {
    let (addr, counts, _rig) = radio(Duration::from_millis(600), false).await;
    let link = open_link(
        &Handle::current(),
        config(addr),
        session("admin", Duration::from_millis(150)),
    )
    .await
    .expect("link");
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.default_timeout = Duration::from_secs(2);
    options.link_watch = Some(link.watch());
    let description = rigfile::parse(FILE).expect("file").description;
    let engine = engine_for(&description).expect("engine");
    struct Boxed(Box<dyn radioctl_core::engine::ProtocolEngine>);
    impl radioctl_core::engine::ProtocolEngine for Boxed {
        fn description(&self) -> &radioctl_core::description::RigDescription {
            self.0.description()
        }
        fn encode(
            &mut self,
            op: &radioctl_core::op::Op,
        ) -> Result<radioctl_core::engine::Request, radioctl_core::error::RigError> {
            self.0.encode(op)
        }
        fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<radioctl_core::engine::Inbound> {
            self.0.feed(bytes, now)
        }
    }
    let (transport, monitor) = link.into_parts();
    let rig = spawn(&Handle::current(), Boxed(engine), transport, options).expect("spawn");
    rig.set_frequency(Vfo::A, Hz(14_074_000))
        .await
        .expect("set");
    assert_eq!(
        rig.frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect("read")
            .value,
        Hz(14_074_000)
    );
    // Three idle limits pass; only the keep-alive is on the wire. The line stays up, the radio
    // never hangs up, and the engine never sees the keep-alive's answers.
    sleep(Duration::from_millis(1800)).await;
    assert_eq!(counts.idle_drops.load(Ordering::SeqCst), 0);
    assert_eq!(counts.connections.load(Ordering::SeqCst), 1);
    assert!(matches!(
        monitor.status(),
        LinkStatus::Connected { generation: 1, .. }
    ));
    assert_eq!(
        rig.frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect("read")
            .value,
        Hz(14_074_000)
    );
    assert_eq!(counts.logins.load(Ordering::SeqCst), 1);
    assert_eq!(counts.refused.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn without_a_keepalive_the_radio_hangs_up_and_the_link_logs_in_again() {
    let (addr, counts, _rig) = radio(Duration::from_millis(400), false).await;
    let link = open_link(
        &Handle::current(),
        config(addr),
        session("admin", Duration::from_secs(60)),
    )
    .await
    .expect("link");
    let mut watch = link.watch();
    timeout(Duration::from_secs(5), async {
        loop {
            if matches!(&*watch.borrow_and_update(), LinkStatus::Connected { generation, .. } if *generation >= 2)
            {
                return;
            }
            watch.changed().await.expect("status");
        }
    })
    .await
    .expect("reconnected after the idle hang-up");
    assert!(counts.idle_drops.load(Ordering::SeqCst) >= 1);
    assert!(
        counts.logins.load(Ordering::SeqCst) >= 2,
        "it logged in again"
    );
}

#[tokio::test]
async fn a_wrong_password_and_a_denied_connection_fail_the_open() {
    let (addr, counts, _rig) = radio(Duration::from_secs(5), false).await;
    let error = match open_link(
        &Handle::current(),
        config(addr),
        session("wrong", Duration::from_secs(4)),
    )
    .await
    {
        Ok(_) => panic!("a wrong password must not connect"),
        Err(e) => e,
    };
    assert_eq!(error.kind, ErrorKind::LinkDown);
    assert!(
        error.detail.contains("wrong account or password"),
        "{}",
        error.detail
    );
    assert!(
        !error.detail.contains("wrong\n"),
        "the password is not in the error"
    );
    assert_eq!(counts.refused.load(Ordering::SeqCst), 1);

    let (addr, _counts, _rig) = radio(Duration::from_secs(5), true).await;
    let error = match open_link(
        &Handle::current(),
        config(addr),
        session("admin", Duration::from_secs(4)),
    )
    .await
    {
        Ok(_) => panic!("a denied connection must not connect"),
        Err(e) => e,
    };
    assert!(error.detail.contains("denied"), "{}", error.detail);
}

// Tests [@ANCHOR: radioctl_ts890_engine_link_restored]
// Tests [@ANCHOR: radioctl_core_actor_restore_session]
// Tests [@ANCHOR: radioctl_core_actor_link_status_changed]
#[tokio::test]
async fn a_dropped_lan_session_logs_in_again_and_the_engine_turns_auto_information_back_on() {
    let (addr, counts, _rig) = radio(Duration::from_secs(60), false).await;
    let link = open_link(
        &Handle::current(),
        config(addr),
        session("admin", Duration::from_secs(60)),
    )
    .await
    .expect("link");
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.default_timeout = Duration::from_secs(2);
    options.link_watch = Some(link.watch());
    let description = rigfile::parse(FILE).expect("file").description;
    let engine = engine_for(&description).expect("engine");
    let (transport, monitor) = link.into_parts();
    let rig = spawn(&Handle::current(), engine, transport, options).expect("spawn");
    let mut events = rig.events();
    // A client that never asked for auto-information gets none after a reconnect.
    rig.frequency(Vfo::A, Freshness::Fresh).await.expect("read");
    rig.execute(Op::SetTransceive(true)).await.expect("ai2");
    rig.frequency(Vfo::A, Freshness::Fresh).await.expect("read");
    assert_eq!(counts.ai2.load(Ordering::SeqCst), 1);
    assert_eq!(counts.logins.load(Ordering::SeqCst), 1);

    counts.kill.send_modify(|n| *n += 1);
    timeout(Duration::from_secs(5), async {
        while !matches!(
            monitor.status(),
            LinkStatus::Connected { generation, .. } if generation >= 2
        ) {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("reconnected");
    // The restore step is complete when the event arrives: unkey first, session next.
    timeout(Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            if event
                .changes
                .iter()
                .any(|c| c.new == Observation::Alarm(Alarm::LinkRestored))
            {
                return;
            }
        }
    })
    .await
    .expect("link restored event");
    // The radio's task reads what the actor wrote after the event: wait for it, then for a
    // possible repeat.
    timeout(Duration::from_secs(5), async {
        while counts.ai2.load(Ordering::SeqCst) < 2 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("AI2; sent again");
    sleep(Duration::from_millis(200)).await;
    assert_eq!(counts.logins.load(Ordering::SeqCst), 2, "logged in again");
    assert_eq!(
        counts.ai2.load(Ordering::SeqCst),
        2,
        "AI2; sent again, once"
    );
    // Still gated, still unkeyed.
    assert_eq!(
        rig.set_ptt(true).await.expect_err("gate").kind,
        ErrorKind::TransmitGateClosed
    );
    assert_eq!(counts.refused.load(Ordering::SeqCst), 0);
}

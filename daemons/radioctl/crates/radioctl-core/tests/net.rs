// SPDX-License-Identifier: AGPL-3.0-or-later
//! The network link against loopback fake radios: a virtual FlexRadio on a TCP listener, a virtual
//! Kenwood served by a killable server (so the connection can be cut and the rig must be unkeyed
//! when it comes back), a login handshake, keep-alive and silence detection, length framing and a
//! UDP echo radio. No hardware, nothing leaves the machine, nothing is keyed.
#![allow(clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use radioctl_core::backoff::Backoff;
use radioctl_core::description::RigDescription;
use radioctl_core::error::ErrorKind;
use radioctl_core::net::{
    open_link, Decoded, Framing, LengthFraming, NetConfig, NetProtocol, RawFraming,
};
use radioctl_core::op::{Alarm, Field, Observation};
use radioctl_core::rig::{spawn, Freshness, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::serial::LinkStatus;
use radioctl_core::units::{Hz, Vfo};
use radioctl_protocols::flex::FlexEngine;
use radioctl_protocols::flex_io::serve_virtual_flex;
use radioctl_protocols::kenwood::KenwoodEngine;
use radioctl_protocols::virtual_flex::VirtualFlex;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

fn fast(address: String) -> NetConfig {
    let mut config = NetConfig::new(address);
    config.connect_timeout = Duration::from_millis(500);
    config.handshake_timeout = Duration::from_millis(500);
    config.write_timeout = Duration::from_millis(500);
    config.backoff = Backoff {
        initial: Duration::from_millis(20),
        max: Duration::from_millis(80),
        factor: 2,
    };
    config
}

async fn wait_status(
    watch: &mut watch::Receiver<LinkStatus>,
    what: &str,
    mut want: impl FnMut(&LinkStatus) -> bool,
) {
    timeout(Duration::from_secs(5), async {
        loop {
            if want(&watch.borrow_and_update()) {
                return;
            }
            watch.changed().await.expect("status sender alive");
        }
    })
    .await
    .unwrap_or_else(|_| panic!("status never became: {what}"));
}

/// A virtual rig on a TCP listener that can be killed (listener and connection together) and
/// started again on the same port with the same rig state behind it.
struct Server {
    rig: Arc<Mutex<VirtualRig>>,
    task: Option<JoinHandle<()>>,
    addr: std::net::SocketAddr,
}

impl Server {
    async fn start(desc: &RigDescription) -> Server {
        let rig = Arc::new(Mutex::new(VirtualRig::from_description(desc).expect("rig")));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let mut server = Server {
            rig,
            task: None,
            addr,
        };
        server.serve(listener);
        server
    }

    fn serve(&mut self, listener: TcpListener) {
        let rig = Arc::clone(&self.rig);
        self.task = Some(tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = [0u8; 256];
                while let Ok(n) = stream.read(&mut buffer).await {
                    if n == 0 {
                        break;
                    }
                    let reply = rig
                        .lock()
                        .expect("rig")
                        .feed(buffer.get(..n).unwrap_or(&[]), Duration::ZERO);
                    if !reply.is_empty() && stream.write_all(&reply).await.is_err() {
                        break;
                    }
                }
            }
        }));
    }

    async fn kill(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }

    async fn restart(&mut self) {
        let listener = TcpListener::bind(self.addr).await.expect("rebind");
        self.serve(listener);
    }

    fn keyed(&self) -> bool {
        self.rig.lock().expect("rig").state().ptt
    }
}

// Tests [@ANCHOR: radioctl_core_net_open_link]
// Tests [@ANCHOR: radioctl_core_net_supervisor_run]
// Tests [@ANCHOR: radioctl_core_net_attempt]
// Tests [@ANCHOR: radioctl_core_net_open_and_handshake]
// Tests [@ANCHOR: radioctl_core_net_wire_open]
// Tests [@ANCHOR: radioctl_core_net_wire_recv]
// Tests [@ANCHOR: radioctl_core_net_wire_send]
// Tests [@ANCHOR: radioctl_core_net_send]
// Tests [@ANCHOR: radioctl_core_net_session]
// Tests [@ANCHOR: radioctl_core_net_deliver]
// Tests [@ANCHOR: radioctl_core_net_link_status]
// Tests [@ANCHOR: radioctl_core_net_link_watch]
// Tests [@ANCHOR: radioctl_core_net_link_stats]
// Tests [@ANCHOR: radioctl_core_net_link_into_parts]
// Tests [@ANCHOR: radioctl_core_net_monitor_status]
// Tests [@ANCHOR: radioctl_core_net_monitor_watch]
// Tests [@ANCHOR: radioctl_core_net_monitor_stats]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_virtual_flexradio_is_read_through_the_network_link() {
    let desc = rigfile::parse(include_str!("../../../rigs/flex-6300.toml"))
        .expect("flex file")
        .description;
    let rig = VirtualFlex::from_description(&desc).expect("virtual flex");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let _control = serve_virtual_flex(&Handle::current(), listener, rig);
    let link = open_link(
        &Handle::current(),
        fast(addr.to_string()),
        Box::new(RawFraming),
    )
    .await
    .expect("link");
    assert!(matches!(
        link.status(),
        LinkStatus::Connected { generation: 1, .. }
    ));
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.default_timeout = Duration::from_secs(3);
    options.link_watch = Some(link.watch());
    let (transport, monitor) = link.into_parts();
    let handle = spawn(
        &Handle::current(),
        FlexEngine::new(desc).expect("engine"),
        transport,
        options,
    )
    .expect("spawn");
    let before = handle
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("read");
    handle
        .set_frequency(Vfo::A, Hz(7_030_000))
        .await
        .expect("set");
    let after = handle
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("read");
    assert_eq!(after.value, Hz(7_030_000));
    assert_ne!(before.value, after.value);
    let stats = monitor.stats();
    assert!(stats.bytes_in > 0 && stats.bytes_out > 0);
    assert_eq!(stats.reconnects, 0);
    assert!(matches!(
        monitor.status(),
        LinkStatus::Connected { generation: 1, .. }
    ));
    let mut watch = monitor.watch();
    drop(handle);
    wait_status(&mut watch, "closed after the rig is dropped", |s| {
        *s == LinkStatus::Closed
    })
    .await;
}

// Tests [@ANCHOR: radioctl_core_net_sleep_dropping]
// Tests [@ANCHOR: radioctl_core_net_sleep_until]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cut_connection_is_reconnected_and_the_rig_is_unkeyed_first() {
    let desc = RigDescription::ts2000();
    let mut server = Server::start(&desc).await;
    let link = open_link(
        &Handle::current(),
        fast(server.addr.to_string()),
        Box::new(RawFraming),
    )
    .await
    .expect("link");
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.default_timeout = Duration::from_secs(2);
    options.link_watch = Some(link.watch());
    let (transport, monitor) = link.into_parts();
    let rig = spawn(
        &Handle::current(),
        KenwoodEngine::new(desc).expect("engine"),
        transport,
        options,
    )
    .expect("spawn");
    let mut events = rig.events();
    rig.open_transmit_gate(TransmitPermit::new("net test"));
    rig.set_ptt(true).await.expect("key the virtual rig");
    timeout(Duration::from_secs(2), async {
        while !server.keyed() {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the virtual rig is keyed");

    // The network dies with the rig keyed.
    let mut watch = monitor.watch();
    server.kill().await;
    wait_status(&mut watch, "reconnecting", |s| {
        matches!(s, LinkStatus::Reconnecting { .. })
    })
    .await;
    timeout(Duration::from_secs(3), async {
        while rig.link_up() {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the actor learns the link is down");
    assert_eq!(
        rig.frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect_err("down")
            .kind,
        ErrorKind::LinkDown
    );
    assert!(server.keyed(), "nothing could unkey it while it was away");

    // It comes back: the actor unkeys before any other command.
    server.restart().await;
    wait_status(&mut watch, "connected again", |s| {
        matches!(s, LinkStatus::Connected { generation: 2, .. })
    })
    .await;
    timeout(Duration::from_secs(3), async {
        while server.keyed() {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the reconnect unkeyed the rig");
    assert!(rig.link_up());
    assert_eq!(monitor.stats().reconnects, 1);
    rig.frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("served after the reconnect");
    let mut alarms = Vec::new();
    while let Ok(Some(event)) = timeout(Duration::from_millis(100), events.next()).await {
        for change in event.changes {
            if let Observation::Alarm(alarm) = change.new {
                alarms.push(alarm);
            }
        }
    }
    assert!(alarms.contains(&Alarm::LinkDown), "{alarms:?}");
    assert!(alarms.contains(&Alarm::LinkRestored), "{alarms:?}");
    assert_eq!(
        rig.cached(Field::Ptt).map(|c| c.value),
        Some(Observation::Ptt(false))
    );
    server.kill().await;
}

#[tokio::test]
async fn the_first_connection_gives_up_after_its_attempts() {
    let dead = {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        listener.local_addr().expect("addr")
    };
    let mut config = fast(dead.to_string());
    config.first_attempts = 3;
    let started = std::time::Instant::now();
    let error = match open_link(&Handle::current(), config, Box::new(RawFraming)).await {
        Ok(_) => panic!("nothing listens there"),
        Err(error) => error,
    };
    assert_eq!(error.kind, ErrorKind::LinkDown);
    assert!(error.detail.contains("3 attempts"), "{}", error.detail);
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// A login handshake: the greeting is `LOGIN <secret>\n`, the radio answers `OK\n` or `NO\n`.
struct Login {
    secret: &'static str,
    heard: Vec<u8>,
    every: Option<Duration>,
    up: bool,
}

impl Framing for Login {
    fn reset(&mut self) {
        self.heard.clear();
        self.up = false;
    }
    fn greeting(&mut self) -> Vec<u8> {
        format!("LOGIN {}\n", self.secret).into_bytes()
    }
    fn needs_handshake(&self) -> bool {
        true
    }
    fn encode(&mut self, payload: &[u8]) -> Vec<u8> {
        payload.to_vec()
    }
    fn decode(&mut self, wire: &[u8]) -> Decoded {
        let mut out = Decoded::default();
        if self.up {
            out.payload = wire.to_vec();
            return out;
        }
        self.heard.extend_from_slice(wire);
        if let Some(end) = self.heard.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.heard.drain(..=end).collect();
            if line == b"NO\n" {
                out.failure = Some("login refused".into());
            } else if line == b"OK\n" {
                out.ready = true;
                self.up = true;
                out.payload = std::mem::take(&mut self.heard);
            }
        }
        out
    }
    fn keepalive(&self) -> Option<(Duration, Vec<u8>)> {
        self.every.map(|e| (e, b"PING\n".to_vec()))
    }
}

/// A radio with a login: answers `OK` to the right secret and `NO` to any other, then says
/// `HELLO` and echoes what it hears. Counts connections and keep-alive probes.
async fn login_radio(
    secret: &'static str,
    answer_pings: bool,
) -> (std::net::SocketAddr, Arc<Mutex<(u32, u32)>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let counts = Arc::new(Mutex::new((0u32, 0u32)));
    let shared = Arc::clone(&counts);
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            shared.lock().expect("counts").0 += 1;
            let shared = Arc::clone(&shared);
            tokio::spawn(async move {
                let mut heard = Vec::new();
                let mut buffer = [0u8; 128];
                let mut in_session = false;
                while let Ok(n) = stream.read(&mut buffer).await {
                    if n == 0 {
                        break;
                    }
                    heard.extend_from_slice(buffer.get(..n).unwrap_or(&[]));
                    while let Some(end) = heard.iter().position(|b| *b == b'\n') {
                        let line: Vec<u8> = heard.drain(..=end).collect();
                        if line.starts_with(b"LOGIN ") {
                            let ok = line == format!("LOGIN {secret}\n").as_bytes();
                            let reply: &[u8] = if ok { b"OK\nHELLO" } else { b"NO\n" };
                            in_session = ok;
                            let _ = stream.write_all(reply).await;
                        } else if line == b"PING\n" {
                            shared.lock().expect("counts").1 += 1;
                            if answer_pings && in_session {
                                let _ = stream.write_all(b"PONG").await;
                            }
                        } else if in_session {
                            let _ = stream.write_all(&line).await;
                        }
                    }
                }
            });
        }
    });
    (addr, counts)
}

// Tests [@ANCHOR: radioctl_core_net_open_and_handshake]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_login_handshake_gates_the_link_and_a_refusal_fails_it() {
    let (addr, counts) = login_radio("s3cret", true).await;
    let login = Login {
        secret: "s3cret",
        heard: Vec::new(),
        every: None,
        up: false,
    };
    let mut link = open_link(&Handle::current(), fast(addr.to_string()), Box::new(login))
        .await
        .expect("login works");
    // The payload that rode in with the handshake reaches the actor's stream first.
    let mut hello = [0u8; 5];
    timeout(
        Duration::from_secs(2),
        link.transport.read_exact(&mut hello),
    )
    .await
    .expect("in time")
    .expect("read");
    assert_eq!(&hello, b"HELLO");
    link.transport.write_all(b"FA;\n").await.expect("write");
    let mut echoed = [0u8; 4];
    timeout(
        Duration::from_secs(2),
        link.transport.read_exact(&mut echoed),
    )
    .await
    .expect("in time")
    .expect("read");
    assert_eq!(&echoed, b"FA;\n");

    let wrong = Login {
        secret: "wrong",
        heard: Vec::new(),
        every: None,
        up: false,
    };
    let error = match open_link(&Handle::current(), fast(addr.to_string()), Box::new(wrong)).await {
        Ok(_) => panic!("a wrong secret must not connect"),
        Err(error) => error,
    };
    assert_eq!(error.kind, ErrorKind::LinkDown);
    assert!(error.detail.contains("login refused"), "{}", error.detail);
    assert_eq!(counts.lock().expect("counts").0, 2);
}

// Tests [@ANCHOR: radioctl_core_net_attempt]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_radio_that_never_answers_the_login_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        // Accepts and says nothing, holding every connection open.
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
        }
    });
    let mut config = fast(addr.to_string());
    config.handshake_timeout = Duration::from_millis(100);
    let login = Login {
        secret: "x",
        heard: Vec::new(),
        every: None,
        up: false,
    };
    let error = match open_link(&Handle::current(), config, Box::new(login)).await {
        Ok(_) => panic!("no answer means no link"),
        Err(error) => error,
    };
    assert!(
        error.detail.contains("handshake timed out"),
        "{}",
        error.detail
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keepalive_is_sent_and_silence_drops_the_link() {
    // A radio that answers pings keeps its link; one that goes quiet loses it.
    let (addr, counts) = login_radio("k", true).await;
    let mut config = fast(addr.to_string());
    config.idle_timeout = Some(Duration::from_millis(400));
    let login = Login {
        secret: "k",
        heard: Vec::new(),
        every: Some(Duration::from_millis(50)),
        up: false,
    };
    let link = open_link(&Handle::current(), config, Box::new(login))
        .await
        .expect("link");
    sleep(Duration::from_millis(900)).await;
    assert!(
        matches!(link.status(), LinkStatus::Connected { generation: 1, .. }),
        "a chatty radio keeps its link: {:?}",
        link.status()
    );
    assert!(counts.lock().expect("counts").1 >= 5);

    let (quiet, _) = login_radio("k", false).await;
    let mut config = fast(quiet.to_string());
    config.idle_timeout = Some(Duration::from_millis(300));
    let login = Login {
        secret: "k",
        heard: Vec::new(),
        every: Some(Duration::from_millis(50)),
        up: false,
    };
    let link = open_link(&Handle::current(), config, Box::new(login))
        .await
        .expect("link");
    let mut watch = link.watch();
    wait_status(&mut watch, "reconnecting after silence", |s| {
        matches!(s, LinkStatus::Reconnecting { last_error, .. } if last_error.contains("nothing heard"))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn length_framing_wraps_both_directions_over_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = [0u8; 64];
            if let Ok(n) = stream.read(&mut buffer).await {
                log.lock()
                    .expect("log")
                    .extend_from_slice(buffer.get(..n).unwrap_or(&[]));
            }
            // Two frames in one write, the second split across two writes.
            let _ = stream
                .write_all(&[0, 3, b'F', b'A', b';', 0, 2, b'I'])
                .await;
            sleep(Duration::from_millis(30)).await;
            let _ = stream.write_all(b"F").await;
            sleep(Duration::from_millis(300)).await;
        }
    });
    let mut link = open_link(
        &Handle::current(),
        fast(addr.to_string()),
        Box::new(LengthFraming::new(2)),
    )
    .await
    .expect("link");
    link.transport.write_all(b"ID;").await.expect("write");
    let mut got = [0u8; 5];
    timeout(Duration::from_secs(2), link.transport.read_exact(&mut got))
        .await
        .expect("in time")
        .expect("read");
    assert_eq!(&got, b"FA;IF");
    assert_eq!(
        seen.lock().expect("log").as_slice(),
        &[0, 3, b'I', b'D', b';']
    );
}

// Tests [@ANCHOR: radioctl_core_net_wire_open]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_carries_one_datagram_per_write_and_detects_silence() {
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
    let addr = socket.local_addr().expect("addr");
    let heard = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let log = Arc::clone(&heard);
    let radio = tokio::spawn(async move {
        let mut buffer = [0u8; 2048];
        // Answers the first datagram only, then goes quiet.
        if let Ok((n, from)) = socket.recv_from(&mut buffer).await {
            log.lock()
                .expect("log")
                .push(buffer.get(..n).unwrap_or(&[]).to_vec());
            let _ = socket.send_to(b"FA00014074000;", from).await;
        }
        while let Ok((n, _)) = socket.recv_from(&mut buffer).await {
            log.lock()
                .expect("log")
                .push(buffer.get(..n).unwrap_or(&[]).to_vec());
        }
    });
    let mut config = fast(addr.to_string());
    config.protocol = NetProtocol::Udp;
    config.idle_timeout = Some(Duration::from_millis(400));
    let mut link = open_link(&Handle::current(), config, Box::new(RawFraming))
        .await
        .expect("udp link");
    link.transport.write_all(b"FA;").await.expect("write");
    let mut got = [0u8; 14];
    timeout(Duration::from_secs(2), link.transport.read_exact(&mut got))
        .await
        .expect("in time")
        .expect("read");
    assert_eq!(&got, b"FA00014074000;");
    assert_eq!(heard.lock().expect("log")[0], b"FA;");
    // The radio is now silent: after the idle limit the link reopens (a new socket, a higher
    // generation), which is what makes the actor unkey.
    let mut watch = link.watch();
    wait_status(
        &mut watch,
        "udp reconnect after silence",
        |s| matches!(s, LinkStatus::Connected { generation, .. } if *generation >= 2),
    )
    .await;
    assert!(link.stats().reconnects >= 1);
    radio.abort();
}

// Tests [@ANCHOR: radioctl_core_net_config_new]
#[test]
fn defaults_never_wait_forever() {
    let config = NetConfig::new("127.0.0.1:1");
    assert!(config.connect_timeout <= Duration::from_secs(10));
    assert!(config.handshake_timeout <= Duration::from_secs(10));
    assert!(config.write_timeout <= Duration::from_secs(10));
}

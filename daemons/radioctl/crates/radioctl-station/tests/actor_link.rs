// SPDX-License-Identifier: AGPL-3.0-or-later
//! The radio manager driving real rig actors over loopback TCP to a killable virtual Kenwood:
//! power on spawns the actor, a dead actor is respawned with the manager's backoff, the radio is
//! unkeyed before the link is reported up, and an actor whose transport reconnects by itself is not
//! respawned. No hardware; the virtual bank stands in for the power switch; nothing is keyed except
//! the virtual rig, by the test, to prove the unkey pass.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use radioctl_core::backoff::Backoff;
use radioctl_core::description::RigDescription;
use radioctl_core::net::{open_link, NetConfig, RawFraming};
use radioctl_core::rig::{spawn, RigOptions};
use radioctl_core::serial::LinkStatus;
use radioctl_core::tcp::{connect_tcp, TcpConfig};
use radioctl_protocols::kenwood::KenwoodEngine;
use radioctl_protocols::virtual_rig::{PanelChange, VirtualRig};
use radioctl_station::actor_link::{ActorLinks, RigConnection, RigSpawner};
use radioctl_station::channel::{BoxFuture, Switchboard, VirtualBank};
use radioctl_station::error::{StationError, StationErrorKind};
use radioctl_station::lifecycle::{LifecycleTimes, RadioState};
use radioctl_station::manager::{Button, RadioLink, RadioManager};
use radioctl_station::model::{DeviceId, StationDef};
use radioctl_station::record::RecordStore;
use radioctl_station::station::Station;
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

const DEF: &str = r#"
[[channels]]
id = "radio_pwr"
type = "power"
driver = "bank"
line = 0
safe = "off"
min_off_ms = 500
[[devices]]
id = "ts2000"
kind = "radio"
power = { source = "channel", channel = "radio_pwr" }
"#;

fn dev(s: &str) -> DeviceId {
    DeviceId(s.into())
}

/// A virtual Kenwood on a TCP listener that can be killed and restarted with its state intact.
struct Server {
    rig: Arc<Mutex<VirtualRig>>,
    task: Option<JoinHandle<()>>,
    addr: std::net::SocketAddr,
}

impl Server {
    async fn start() -> Server {
        let rig = VirtualRig::from_description(&RigDescription::ts2000()).expect("rig");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let mut server = Server {
            rig: Arc::new(Mutex::new(rig)),
            task: None,
            addr: listener.local_addr().expect("addr"),
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

    fn key(&self, on: bool) {
        let _ = self
            .rig
            .lock()
            .expect("rig")
            .front_panel(PanelChange::Ptt(on));
    }

    fn keyed(&self) -> bool {
        self.rig.lock().expect("rig").state().ptt
    }
}

fn options() -> RigOptions {
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.default_timeout = Duration::from_secs(2);
    options
}

/// One plain TCP connection per spawn: the actor cannot reconnect, so a lost link means a
/// respawn.
struct PlainSpawner {
    addr: std::net::SocketAddr,
}

impl RigSpawner for PlainSpawner {
    fn spawn(&self, _radio: &DeviceId) -> BoxFuture<'_, Result<RigConnection, StationError>> {
        Box::pin(async move {
            let config = TcpConfig {
                connect_timeout: Duration::from_millis(300),
                attempts: 1,
                backoff: Backoff::default(),
            };
            let stream = connect_tcp(self.addr, &config)
                .await
                .map_err(|e| StationError::new(StationErrorKind::Driver, e.detail))?;
            let engine = KenwoodEngine::new(RigDescription::ts2000()).expect("engine");
            let handle = spawn(&Handle::current(), engine, stream, options())
                .map_err(|e| StationError::new(StationErrorKind::Driver, e.detail))?;
            Ok(RigConnection {
                handle,
                watch: None,
            })
        })
    }
}

/// A transport that reconnects by itself: the actor stays and watches the status.
struct LinkSpawner {
    addr: std::net::SocketAddr,
}

impl RigSpawner for LinkSpawner {
    fn spawn(&self, _radio: &DeviceId) -> BoxFuture<'_, Result<RigConnection, StationError>> {
        Box::pin(async move {
            let mut config = NetConfig::new(self.addr.to_string());
            config.connect_timeout = Duration::from_millis(300);
            config.backoff = Backoff {
                initial: Duration::from_millis(20),
                max: Duration::from_millis(80),
                factor: 2,
            };
            let link = open_link(&Handle::current(), config, Box::new(RawFraming))
                .await
                .map_err(|e| StationError::new(StationErrorKind::Driver, e.detail))?;
            let mut opts = options();
            let status = link.watch();
            opts.link_watch = Some(status.clone());
            let (transport, monitor) = link.into_parts();
            // The monitor must live as long as the rig; leaking one small struct per spawn in a
            // test is fine, a real spawner keeps it with the rig.
            std::mem::forget(monitor);
            let engine = KenwoodEngine::new(RigDescription::ts2000()).expect("engine");
            let handle = spawn(&Handle::current(), engine, transport, opts)
                .map_err(|e| StationError::new(StationErrorKind::Driver, e.detail))?;
            Ok(RigConnection {
                handle,
                watch: Some(status),
            })
        })
    }
}

struct World {
    mgr: Arc<RadioManager>,
    links: Arc<ActorLinks>,
    bank: VirtualBank,
    monitor: JoinHandle<()>,
    path: std::path::PathBuf,
}

impl Drop for World {
    fn drop(&mut self) {
        self.monitor.abort();
        let _ = std::fs::remove_file(&self.path);
    }
}

fn world(name: &str, spawner: Arc<dyn RigSpawner>) -> World {
    let path = std::env::temp_dir().join(format!("rs-actor-{name}-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let def = StationDef::from_toml(DEF).expect("def");
    let bank = VirtualBank::new();
    let mut board = Switchboard::new();
    for c in def.channels.clone() {
        board.add_channel(c, Arc::new(bank.clone())).expect("chan");
    }
    let links = Arc::new(ActorLinks::new(spawner));
    let station = Arc::new(
        Station::new(
            def,
            Arc::new(board),
            vec![links.key_source(&dev("ts2000"))],
            None,
        )
        .expect("station"),
    );
    let mgr = Arc::new(RadioManager::new(
        station,
        Arc::clone(&links) as Arc<dyn RadioLink>,
        RecordStore::new(&path),
        Arc::new(|| 1_000),
        LifecycleTimes {
            boot_time: Duration::from_secs(5),
            boot_grace: Duration::from_secs(5),
            power_on_timeout: Duration::from_secs(5),
            degraded_timeout: Duration::from_secs(60),
        },
    ));
    let monitor = mgr.spawn_monitor(&Handle::current(), Duration::from_millis(40));
    World {
        mgr,
        links,
        bank,
        monitor,
        path,
    }
}

async fn until(what: &str, mut check: impl FnMut() -> bool) {
    timeout(Duration::from_secs(15), async {
        while !check() {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("never happened: {what}"));
}

// Tests [@ANCHOR: radioctl_station_actorlink_new]
// Tests [@ANCHOR: radioctl_station_actorlink_handle]
// Tests [@ANCHOR: radioctl_station_actorlink_key_source]
// Tests [@ANCHOR: radioctl_station_actorlink_spawns]
// Tests [@ANCHOR: radioctl_station_actorlink_journal]
// Tests [@ANCHOR: radioctl_station_actorlink_note]
// Tests [@ANCHOR: radioctl_station_actorlink_state]
// Tests [@ANCHOR: radioctl_station_actorlink_unkey_pass]
// Tests [@ANCHOR: radioctl_station_actorlink_link_up]
// Tests [@ANCHOR: radioctl_station_actorlink_present]
// Tests [@ANCHOR: radioctl_station_actorlink_connect]
// Tests [@ANCHOR: radioctl_station_livekey_name]
// Tests [@ANCHOR: radioctl_station_livekey_key_state]
// Tests [@ANCHOR: radioctl_station_actorlink_lock]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dead_actor_is_respawned_and_the_radio_unkeyed_before_the_link_is_up() {
    let mut server = Server::start().await;
    // The radio is keyed (a stuck microphone) when the software first reaches it.
    server.key(true);
    let w = world("respawn", Arc::new(PlainSpawner { addr: server.addr }));
    let id = dev("ts2000");
    assert!(w.links.handle(&id).is_none());
    assert!(!w.links.link_up(&id).await);
    assert_eq!(w.links.present(&id).await, None);

    // Power button: the switch goes on, the manager connects, the first actor is spawned and
    // unkeys the radio before the state is reported.
    let report = w.mgr.press(&id, Button::On).await.expect("press");
    assert_eq!(report.state, RadioState::Ready);
    assert_eq!(w.links.spawns(&id), 1);
    assert!(!server.keyed(), "the first connection unkeyed the radio");
    assert!(w.links.handle(&id).is_some());
    assert!(w.links.link_up(&id).await);
    // Only the power line was written.
    assert_eq!(w.bank.log(), vec![(0, true)]);

    // The network dies with the radio keyed. The actor learns the link is down (end of stream).
    server.key(true);
    server.kill().await;
    until("the manager sees the link down", || {
        w.mgr.state(&id) == Some(RadioState::Degraded)
    })
    .await;
    assert!(!w.links.link_up(&id).await);
    assert!(server.keyed(), "nothing could unkey it while it was away");

    // It comes back: the manager respawns the actor with backoff and the new actor unkeys first.
    server.restart().await;
    until("the manager reconnects", || {
        w.mgr.state(&id) == Some(RadioState::Ready)
    })
    .await;
    assert_eq!(w.links.spawns(&id), 2);
    assert!(!server.keyed(), "the respawn unkeyed the radio");
    let unkeys = w
        .links
        .journal()
        .iter()
        .filter(|l| l.contains("unkey pass done"))
        .count();
    assert_eq!(unkeys, 2, "{:?}", w.links.journal());
    // Reconnecting wrote nothing to the switch.
    assert_eq!(w.bank.log(), vec![(0, true)]);
    // The key source follows the new actor.
    let key = w.links.key_source(&id);
    assert_eq!(key.name(), "ts2000");
    assert_eq!(
        key.key_state().await,
        radioctl_station::station::KeyState::Unkeyed
    );
    server.kill().await;
}

// Tests [@ANCHOR: radioctl_station_actorlink_connect]
// Tests [@ANCHOR: radioctl_station_actorlink_state]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_transport_that_reconnects_by_itself_is_not_respawned() {
    let mut server = Server::start().await;
    let w = world("selfheal", Arc::new(LinkSpawner { addr: server.addr }));
    let id = dev("ts2000");
    let report = w.mgr.press(&id, Button::On).await.expect("press");
    assert_eq!(report.state, RadioState::Ready);
    assert_eq!(w.links.spawns(&id), 1);

    server.key(true);
    server.kill().await;
    until("the link is seen down", || {
        w.mgr.state(&id) == Some(RadioState::Degraded)
    })
    .await;
    // Give the manager several ticks to be tempted.
    sleep(Duration::from_millis(600)).await;
    assert_eq!(w.links.spawns(&id), 1, "no second actor on the same radio");
    assert!(
        w.links
            .journal()
            .iter()
            .any(|l| l.contains("not respawning")),
        "{:?}",
        w.links.journal()
    );

    server.restart().await;
    until("ready again", || {
        w.mgr.state(&id) == Some(RadioState::Ready)
    })
    .await;
    assert_eq!(w.links.spawns(&id), 1);
    // The actor itself ran the unkey pass when its transport came back.
    until("unkeyed by the actor", || !server.keyed()).await;
    server.kill().await;
}

// Tests [@ANCHOR: radioctl_station_actorlink_with_unkey_timeout]
// Tests [@ANCHOR: radioctl_station_actorlink_unkey_pass]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_unkey_pass_is_a_failed_connection() {
    // The actor is given a transport that reports itself down, so the unkey cannot be sent.
    struct DownSpawner(Mutex<Vec<watch::Sender<LinkStatus>>>);
    impl RigSpawner for DownSpawner {
        fn spawn(&self, _radio: &DeviceId) -> BoxFuture<'_, Result<RigConnection, StationError>> {
            Box::pin(async move {
                let (tx, rx) = watch::channel(LinkStatus::Reconnecting {
                    attempt: 1,
                    last_error: "down".into(),
                });
                self.0.lock().expect("senders").push(tx);
                let (near, _far) = duplex(1024);
                let mut opts = options();
                opts.link_watch = Some(rx.clone());
                let engine = KenwoodEngine::new(RigDescription::ts2000()).expect("engine");
                let handle = spawn(&Handle::current(), engine, near, opts)
                    .map_err(|e| StationError::new(StationErrorKind::Driver, e.detail))?;
                Ok(RigConnection {
                    handle,
                    watch: Some(rx),
                })
            })
        }
    }
    let links = ActorLinks::new(Arc::new(DownSpawner(Mutex::new(Vec::new()))))
        .with_unkey_timeout(Duration::from_millis(500));
    let id = dev("ts2000");
    let error = links.connect(&id).await.expect_err("unkey cannot be sent");
    assert_eq!(error.kind, StationErrorKind::Driver);
    assert!(error.detail.contains("unkey pass"), "{}", error.detail);
    assert!(
        links.handle(&id).is_none(),
        "a rig that was not unkeyed is not adopted"
    );
    assert!(!links.link_up(&id).await);
}

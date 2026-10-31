// SPDX-License-Identifier: AGPL-3.0-or-later
//! The radio manager, `ActorLinks` and the ready-made `DescriptionSpawner` together, against
//! virtual radios on loopback: a plain Kenwood CAT port whose network drops and comes back (the
//! actor stays and unkeys itself), and a TS-890S with its LAN login and keep-alive. The power
//! switch is a virtual bank; nothing is keyed except the virtual rigs, by the test.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use radioctl_core::backoff::Backoff;
use radioctl_core::description::RigDescription;
use radioctl_core::net::NetConfig;
use radioctl_core::rigfile;
use radioctl_protocols::kenwood_ts890_lan::{LanCredentials, Ts890LanFraming};
use radioctl_protocols::virtual_rig::{PanelChange, VirtualRig};
use radioctl_station::actor_link::{ActorLinks, RigSpawner};
use radioctl_station::channel::{Switchboard, VirtualBank};
use radioctl_station::error::StationErrorKind;
use radioctl_station::lifecycle::{LifecycleTimes, RadioState};
use radioctl_station::manager::{Button, RadioLink, RadioManager};
use radioctl_station::model::{DeviceId, StationDef};
use radioctl_station::record::RecordStore;
use radioctl_station::spawner::{DescriptionSpawner, RigEndpoint, RigSpec};
use radioctl_station::station::Station;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

const DEF: &str = r#"
[[channels]]
id = "pwr"
type = "power"
driver = "bank"
line = 0
safe = "off"
[[devices]]
id = "radio"
kind = "radio"
power = { source = "channel", channel = "pwr" }
"#;

fn dev(s: &str) -> DeviceId {
    DeviceId(s.into())
}

/// A virtual radio on a killable TCP listener. With `lan`, it demands the TS-890S login first.
struct Radio {
    rig: Arc<Mutex<VirtualRig>>,
    task: Option<JoinHandle<()>>,
    addr: std::net::SocketAddr,
    lan: bool,
}

impl Radio {
    async fn start(desc: &RigDescription, lan: bool) -> Radio {
        let rig = VirtualRig::from_description(desc).expect("rig");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let mut radio = Radio {
            rig: Arc::new(Mutex::new(rig)),
            task: None,
            addr: listener.local_addr().expect("addr"),
            lan,
        };
        radio.serve(listener);
        radio
    }

    fn serve(&mut self, listener: TcpListener) {
        let (rig, lan) = (Arc::clone(&self.rig), self.lan);
        self.task = Some(tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let rig = Arc::clone(&rig);
                // Inline, so aborting the task drops the connection too.
                async {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 256];
                    let mut up = !lan;
                    let mut step = 0;
                    while let Ok(n) = stream.read(&mut chunk).await {
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        while let Some(end) = buf.iter().position(|b| *b == b';') {
                            let frame: Vec<u8> = buf.drain(..=end).collect();
                            let reply = if up {
                                rig.lock().unwrap().feed(&frame, Duration::ZERO)
                            } else if step == 0 && frame == b"##CN;" {
                                step = 1;
                                b"##CN1;".to_vec()
                            } else if step == 1 && frame == b"##ID00705kenwoodadmin;" {
                                up = true;
                                b"##ID1;".to_vec()
                            } else {
                                return;
                            };
                            if !reply.is_empty() && stream.write_all(&reply).await.is_err() {
                                return;
                            }
                        }
                    }
                }
                .await;
            }
        }));
    }

    async fn kill(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
            let _ = t.await;
        }
    }

    async fn restart(&mut self) {
        let listener = TcpListener::bind(self.addr).await.expect("rebind");
        self.serve(listener);
    }

    fn key(&self, on: bool) {
        let _ = self.rig.lock().unwrap().front_panel(PanelChange::Ptt(on));
    }

    fn keyed(&self) -> bool {
        self.rig.lock().unwrap().state().ptt
    }
}

struct World {
    mgr: Arc<RadioManager>,
    links: Arc<ActorLinks>,
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
    let path = std::env::temp_dir().join(format!("rs-spawner-{name}-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let def = StationDef::from_toml(DEF).unwrap();
    let bank = VirtualBank::new();
    let mut board = Switchboard::new();
    for c in def.channels.clone() {
        board.add_channel(c, Arc::new(bank.clone())).unwrap();
    }
    let links = Arc::new(ActorLinks::new(spawner));
    let station = Arc::new(
        Station::new(
            def,
            Arc::new(board),
            vec![links.key_source(&dev("radio"))],
            None,
        )
        .unwrap(),
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
    let monitor = mgr.spawn_monitor(
        &tokio::runtime::Handle::current(),
        Duration::from_millis(40),
    );
    World {
        mgr,
        links,
        monitor,
        path,
    }
}

async fn until(what: &str, mut check: impl FnMut() -> bool) {
    timeout(Duration::from_secs(15), async {
        while !check() {
            sleep(Duration::from_millis(15)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("never happened: {what}"));
}

fn quick(address: String) -> NetConfig {
    let mut c = NetConfig::new(address);
    c.connect_timeout = Duration::from_millis(500);
    c.handshake_timeout = Duration::from_millis(500);
    c.backoff = Backoff {
        initial: Duration::from_millis(20),
        max: Duration::from_millis(80),
        factor: 2,
    };
    c
}

// Tests [@ANCHOR: radioctl_station_dspawner_new]
// Tests [@ANCHOR: radioctl_station_dspawner_add]
// Tests [@ANCHOR: radioctl_station_dspawner_spawn]
// Tests [@ANCHOR: radioctl_station_dspawner_fail]
// Tests [@ANCHOR: radioctl_station_rigspec_tcp]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_network_radio_keeps_its_actor_across_a_network_drop_and_is_unkeyed_by_it() {
    let desc = RigDescription::ts2000();
    let mut radio = Radio::start(&desc, false).await;
    radio.key(true); // keyed when first reached
    let mut spawner = DescriptionSpawner::new();
    let mut spec = RigSpec::tcp(desc, radio.addr.to_string());
    spec.options.poll.enabled = false;
    spec.options.default_timeout = Duration::from_secs(2);
    if let RigEndpoint::Tcp { config, .. } = &mut spec.endpoint {
        *config = quick(radio.addr.to_string());
    }
    spawner.add(dev("radio"), spec);
    let w = world("net", Arc::new(spawner));

    let report = w.mgr.press(&dev("radio"), Button::On).await.unwrap();
    assert_eq!(report.state, RadioState::Ready);
    assert!(
        !radio.keyed(),
        "the unkey pass ran before the link was reported up"
    );
    assert_eq!(w.links.spawns(&dev("radio")), 1);
    // The transmit gate of a spawned rig is closed.
    let handle = w.links.handle(&dev("radio")).unwrap();
    assert!(handle.transmit_permit().is_none());

    radio.key(true);
    radio.kill().await;
    until("degraded", || {
        w.mgr.state(&dev("radio")) == Some(RadioState::Degraded)
    })
    .await;
    radio.restart().await;
    until("ready again", || {
        w.mgr.state(&dev("radio")) == Some(RadioState::Ready)
    })
    .await;
    until("unkeyed again", || !radio.keyed()).await;
    assert_eq!(w.links.spawns(&dev("radio")), 1, "the same actor came back");
    radio.kill().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_ts890_is_logged_in_and_unkeyed_through_the_manager() {
    let desc = rigfile::parse(include_str!("../../../rigs/kenwood-ts-890s.toml"))
        .unwrap()
        .description;
    let mut radio = Radio::start(&desc, true).await;
    radio.key(true);
    let mut spawner = DescriptionSpawner::new();
    let mut spec = RigSpec::tcp(desc, radio.addr.to_string());
    spec.options.poll.enabled = false;
    spec.options.default_timeout = Duration::from_secs(2);
    let credentials = LanCredentials::new(true, "kenwood", "admin").unwrap();
    spec.endpoint = RigEndpoint::Tcp {
        address: radio.addr.to_string(),
        framing: Some(Arc::new(move || {
            Box::new(Ts890LanFraming::new(credentials.clone()))
        })),
        config: quick(radio.addr.to_string()),
    };
    spawner.add(dev("radio"), spec);
    let w = world("lan", Arc::new(spawner));
    let report = w.mgr.press(&dev("radio"), Button::On).await.unwrap();
    assert_eq!(report.state, RadioState::Ready);
    assert!(!radio.keyed(), "unkeyed after the LAN login");
    radio.kill().await;
}

// Tests [@ANCHOR: radioctl_station_rigspec_serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_radio_and_an_unreachable_one_are_clean_errors() {
    let mut spawner = DescriptionSpawner::new();
    spawner.add(
        dev("serial"),
        RigSpec::serial(RigDescription::ts2000(), "/dev/null/not-a-port", 9600),
    );
    let dead = {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap()
    };
    let mut spec = RigSpec::tcp(RigDescription::ts2000(), dead.to_string());
    if let RigEndpoint::Tcp { config, .. } = &mut spec.endpoint {
        *config = quick(dead.to_string());
    }
    spawner.add(dev("tcp"), spec);
    let unknown = match spawner.spawn(&dev("nobody")).await {
        Ok(_) => panic!("no spec"),
        Err(e) => e,
    };
    assert_eq!(unknown.kind, StationErrorKind::Unknown);
    for id in ["serial", "tcp"] {
        let e = match spawner.spawn(&dev(id)).await {
            Ok(_) => panic!("{id} cannot connect"),
            Err(e) => e,
        };
        assert_eq!(e.kind, StationErrorKind::Driver, "{id}");
    }
}

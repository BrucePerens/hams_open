// SPDX-License-Identifier: AGPL-3.0-or-later
//! A selector follows the transmit frequency of a virtual TS-590SG: the receive VFO while simplex,
//! the other VFO in split, and nothing switches while the rig is keyed.
#![allow(clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::op::Op;
use radioctl_core::rig::{spawn_current, RigHandle, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Vfo};
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::kenwood::KenwoodEngine;
use radioctl_protocols::virtual_rig::VirtualRig;
use radioctl_station::band_follow::{follow_configured, follow_tx_frequency, route_for, BandRoute};
use radioctl_station::channel::{Switchboard, VirtualBank};
use radioctl_station::model::{ChannelId, StationDef, Value};
use radioctl_station::station::{KeyedSource, Station};
use tokio::runtime::Handle;

fn routes() -> Vec<BandRoute> {
    let ant = ChannelId("ant".into());
    vec![
        BandRoute {
            low: Hz(14_000_000),
            high: Hz(14_350_000),
            channel: ant.clone(),
            port: 1,
        },
        BandRoute {
            low: Hz(7_000_000),
            high: Hz(7_300_000),
            channel: ant,
            port: 2,
        },
    ]
}

fn rig_and_station() -> (RigHandle, Arc<Station>) {
    rig_and_station_from(
        "[[channels]]\nid = \"ant\"\ntype = \"selector\"\nways = 2\ndriver = \"bank\"\nline = 0\n",
    )
}

fn rig_and_station_from(def_toml: &str) -> (RigHandle, Arc<Station>) {
    let text = std::fs::read_to_string(format!(
        "{}/../../rigs/kenwood-ts-590sg.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("rig file");
    let desc: RigDescription = rigfile::parse(&text).expect("description").description;
    let rig = VirtualRig::from_description(&desc).expect("rig");
    let (stream, _control) = spawn_virtual_rig(&Handle::current(), rig, Duration::ZERO);
    let handle = spawn_current(
        KenwoodEngine::new(desc).expect("engine"),
        stream,
        RigOptions::default(),
    )
    .expect("spawn");
    let def = StationDef::from_toml(def_toml).expect("def");
    let bank = VirtualBank::new();
    let mut board = Switchboard::new();
    for c in def.channels.clone() {
        board
            .add_channel(c, Arc::new(bank.clone()))
            .expect("channel");
    }
    let src: Arc<dyn KeyedSource> = Arc::new(handle.clone());
    let station = Station::new(def, Arc::new(board), vec![src], None).expect("station");
    (handle, Arc::new(station))
}

async fn way(station: &Station) -> Option<Value> {
    station
        .board()
        .known(&ChannelId("ant".into()))
        .expect("known")
}

async fn wait_for_way(station: &Station, port: u32) {
    for _ in 0..250 {
        if way(station).await == Some(Value::Port(port)) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "selector never reached way {port}, is {:?}",
        way(station).await
    );
}

// Tests [@ANCHOR: radioctl_station_band_route_for]
#[test]
fn a_route_is_found_by_band_and_outside_every_band_there_is_none() {
    let r = routes();
    assert_eq!(route_for(&r, Hz(14_074_000)).map(|x| x.port), Some(1));
    assert_eq!(route_for(&r, Hz(7_000_000)).map(|x| x.port), Some(2));
    assert_eq!(route_for(&r, Hz(7_300_000)).map(|x| x.port), Some(2));
    assert!(route_for(&r, Hz(10_100_000)).is_none());
}

// Tests [@ANCHOR: radioctl_station_band_follow_tx_frequency]
#[tokio::test]
async fn the_selector_follows_the_transmit_vfo_in_split_and_waits_while_keyed() {
    let (rig, station) = rig_and_station();
    rig.set_frequency(Vfo::A, Hz(14_100_000)).await.expect("A");
    rig.set_frequency(Vfo::B, Hz(7_100_000)).await.expect("B");
    let results = Arc::new(std::sync::Mutex::new(Vec::new()));
    let task = {
        let (rig, station, results) = (rig.clone(), station.clone(), results.clone());
        tokio::spawn(async move {
            follow_tx_frequency(&station, &rig, &routes(), |r, res| {
                results.lock().expect("lock").push((r.port, res.is_ok()));
            })
            .await;
        })
    };
    wait_for_way(&station, 1).await;

    // Keyed in simplex on 14.1 MHz, then split to the 40 m VFO: the interlock refuses the switch.
    rig.open_transmit_gate(TransmitPermit::new("band follow test"));
    rig.set_ptt(true).await.expect("key");
    rig.execute(Op::SetSplit(true)).await.expect("split");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(way(&station).await, Some(Value::Port(1)));

    rig.set_ptt(false).await.expect("unkey");
    wait_for_way(&station, 2).await;
    assert!(results.lock().expect("lock").contains(&(2, true)));
    task.abort();
}

const CONFIG: &str = r#"
[[channels]]
id = "ant"
type = "selector"
ways = 2
driver = "bank"
line = 0
rig = "ts590"

[[channels]]
id = "amp"
type = "selector"
ways = 2
driver = "bank"
line = 2
band_follow = false

[[channels]]
id = "dec"
type = "selector"
ways = 2
driver = "bank"
line = 4
rig = "kx3"

[[channels]]
id = "any"
type = "selector"
ways = 2
driver = "bank"
line = 6

[[band_routes]]
channel = "ant"
low_hz = 14000000
high_hz = 14350000
port = 1
[[band_routes]]
channel = "amp"
low_hz = 14000000
high_hz = 14350000
port = 1
[[band_routes]]
channel = "dec"
low_hz = 14000000
high_hz = 14350000
port = 1
[[band_routes]]
channel = "any"
low_hz = 7000000
high_hz = 7300000
port = 2
"#;

// Tests [@ANCHOR: radioctl_station_def_band_routes_for]
#[test]
fn a_channel_follows_only_its_own_rig_and_only_without_the_opt_out() {
    let def = StationDef::from_toml(CONFIG).expect("def");
    let names = |rig: &str| -> Vec<String> {
        def.band_routes_for(rig)
            .into_iter()
            .map(|r| r.channel.0)
            .collect()
    };
    // "amp" opted out; "dec" belongs to the other rig; "any" belongs to none in particular.
    assert_eq!(names("ts590"), ["ant", "any"]);
    assert_eq!(names("kx3"), ["dec", "any"]);
    assert_eq!(names("ic7300"), ["any"]);
    let r = &def.band_routes_for("ts590")[0];
    assert_eq!((r.low, r.high, r.port), (Hz(14_000_000), Hz(14_350_000), 1));
}

// Tests [@ANCHOR: radioctl_station_def_validate]
#[test]
fn a_band_route_must_name_a_selector_way_and_a_sane_band() {
    let with = |route: &str| format!("{CONFIG}\n[[band_routes]]\n{route}");
    for bad in [
        "channel = \"nope\"\nlow_hz = 1\nhigh_hz = 2\nport = 1\n",
        "channel = \"ant\"\nlow_hz = 3\nhigh_hz = 2\nport = 1\n",
        "channel = \"ant\"\nlow_hz = 1\nhigh_hz = 2\nport = 3\n",
        "channel = \"ant\"\nlow_hz = 1\nhigh_hz = 2\nport = 0\n",
    ] {
        assert!(StationDef::from_toml(&with(bad)).is_err(), "{bad}");
    }
}

// Tests [@ANCHOR: radioctl_station_follow_configured]
#[tokio::test]
async fn the_configured_follower_switches_only_the_channels_of_the_named_rig() {
    let (rig, station) = rig_and_station_from(CONFIG);
    rig.set_frequency(Vfo::A, Hz(14_100_000)).await.expect("A");
    let task = {
        let (rig, station) = (rig.clone(), station.clone());
        tokio::spawn(async move {
            follow_configured(&station, "ts590", &rig, |_, _| {}).await;
        })
    };
    wait_for_way(&station, 1).await;
    // The opted-out channel and the other rig's channel were left alone.
    let known = |id: &str| station.board().known(&ChannelId(id.into())).expect("known");
    assert_ne!(known("amp"), Some(Value::Port(1)));
    assert_ne!(known("dec"), Some(Value::Port(1)));
    task.abort();
}

#[tokio::test]
async fn a_rig_with_no_routes_returns_at_once() {
    let (rig, station) = rig_and_station();
    follow_configured(&station, "ts590", &rig, |_, _| panic!("no route")).await;
    assert_eq!(way(&station).await, None);
}

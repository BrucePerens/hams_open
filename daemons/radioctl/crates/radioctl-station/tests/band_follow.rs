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
use radioctl_station::band_follow::{follow_tx_frequency, route_for, BandRoute};
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
    let def = StationDef::from_toml(
        "[[channels]]\nid = \"ant\"\ntype = \"selector\"\nways = 2\ndriver = \"bank\"\nline = 0\n",
    )
    .expect("def");
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

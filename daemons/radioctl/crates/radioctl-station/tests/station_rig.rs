// SPDX-License-Identifier: AGPL-3.0-or-later
//! The interlock fed by a real radioctl rig handle: a virtual IC-7300 keyed through the open
//! transmit gate makes the station refuse to switch; unkeyed it lets the command through.
#![allow(clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use radioctl_core::description::RigDescription;
use radioctl_core::rig::{spawn_current, RigOptions, TransmitPermit};
use radioctl_protocols::civ::CivEngine;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use radioctl_station::channel::{Switchboard, VirtualBank};
use radioctl_station::error::StationErrorKind;
use radioctl_station::manager::{RadioLink, RigLinks};
use radioctl_station::model::{ChannelId, DeviceId, StationDef, Value};
use radioctl_station::station::{KeyState, KeyedSource, Station};
use tokio::runtime::Handle;

// Tests [@ANCHOR: radioctl_station_rig_name]
// Tests [@ANCHOR: radioctl_station_rig_key_state]
#[tokio::test]
async fn a_keyed_virtual_rig_blocks_switching() {
    let desc = RigDescription::ic7300();
    let id = desc.id.clone();
    let rig = VirtualRig::from_description(&desc).expect("rig");
    let (stream, _control) = spawn_virtual_rig(&Handle::current(), rig, std::time::Duration::ZERO);
    let handle = spawn_current(
        CivEngine::new(desc).expect("engine"),
        stream,
        RigOptions::default(),
    )
    .expect("spawn");
    assert_eq!(handle.name(), id);
    assert_eq!(handle.key_state().await, KeyState::Unkeyed);

    let def = StationDef::from_toml(
        r#"
[[channels]]
id = "pwr"
type = "power"
driver = "bank"
line = 0
safe = "off"
"#,
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

    handle.open_transmit_gate(TransmitPermit::new("station interlock test"));
    handle.set_ptt(true).await.expect("key");
    assert_eq!(handle.key_state().await, KeyState::Keyed);
    let err = station
        .set_channel(&ChannelId("pwr".into()), Value::On)
        .await
        .expect_err("keyed");
    assert_eq!(err.kind, StationErrorKind::Refused);
    assert!(bank.log().is_empty());

    handle.set_ptt(false).await.expect("unkey");
    station
        .set_channel(&ChannelId("pwr".into()), Value::On)
        .await
        .expect("unkeyed");
    assert!(bank.line(0));
}

// Tests [@ANCHOR: radioctl_station_riglinks_new]
// Tests [@ANCHOR: radioctl_station_riglinks_set]
// Tests [@ANCHOR: radioctl_station_riglinks_link_up]
// Tests [@ANCHOR: radioctl_station_riglinks_present]
// Tests [@ANCHOR: radioctl_station_riglinks_connect]
#[tokio::test]
async fn rig_links_report_a_radio_without_a_handle_as_down_and_a_live_handle_as_up() {
    let links = RigLinks::new();
    let radio = DeviceId("ic7300".to_string());
    assert!(!links.link_up(&radio).await);
    assert_eq!(links.present(&radio).await, None);
    assert!(links.connect(&radio).await.is_err());

    let desc = RigDescription::ic7300();
    let rig = VirtualRig::from_description(&desc).expect("rig");
    let (stream, _control) = spawn_virtual_rig(&Handle::current(), rig, std::time::Duration::ZERO);
    let handle = spawn_current(
        CivEngine::new(desc).expect("engine"),
        stream,
        RigOptions::default(),
    )
    .expect("spawn");
    links.set(radio.clone(), handle);
    assert!(links.link_up(&radio).await);
    assert_eq!(links.present(&radio).await, None);
    assert!(matches!(links.connect(&radio).await, Ok(None)));
}

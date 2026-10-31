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

const KEYED_STATION: &str = r#"
[[channels]]
id = "radio_pwr"
type = "power"
driver = "bank"
line = 0
safe = "off"
[[channels]]
id = "amp_pwr"
type = "power"
driver = "bank"
line = 1
safe = "off"
[[channels]]
id = "ant"
type = "selector"
ways = 2
driver = "bank"
line = 2
[[channels]]
id = "band"
type = "output"
driver = "bank"
line = 4
[[devices]]
id = "ic7300"
kind = "radio"
power = { source = "channel", channel = "radio_pwr" }
[[devices]]
id = "amp"
kind = "amplifier"
power = { source = "channel", channel = "amp_pwr" }
depends_on = ["ic7300"]
"#;

fn keyed_station(src: Arc<dyn KeyedSource>) -> (Station, VirtualBank) {
    let def = StationDef::from_toml(KEYED_STATION).expect("def");
    let bank = VirtualBank::new();
    let mut board = Switchboard::new();
    for c in def.channels.clone() {
        board
            .add_channel(c, Arc::new(bank.clone()))
            .expect("channel");
    }
    let station = Station::new(def, Arc::new(board), vec![src], None).expect("station");
    (station, bank)
}

fn ch(s: &str) -> ChannelId {
    ChannelId(s.into())
}

fn dev(s: &str) -> DeviceId {
    DeviceId(s.into())
}

// Tests [@ANCHOR: radioctl_station_rig_key_state]
// Tests [@ANCHOR: radioctl_station_station_guard_action]
// Tests [@ANCHOR: radioctl_station_station_power_off]
#[tokio::test]
async fn a_known_keyed_rig_lets_power_off_and_radio_power_on_through_and_the_gate_is_not_touched() {
    // Bruce, 2026-10-31: power-off of the amplifier or the rig, and the rig's own power-on, pass
    // while a rig is KNOWN keyed; amplifier power-on, antenna, band and every other output stay
    // refused.
    let desc = RigDescription::ic7300();
    let rig = VirtualRig::from_description(&desc).expect("rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), rig, std::time::Duration::ZERO);
    let handle = spawn_current(
        CivEngine::new(desc).expect("engine"),
        stream,
        RigOptions::default(),
    )
    .expect("spawn");
    let (station, bank) = keyed_station(Arc::new(handle.clone()));
    station.power_on(&dev("ic7300")).await.expect("radio on");
    station.power_on(&dev("amp")).await.expect("amp on");
    station.select(&ch("ant"), 1).await.expect("antenna");
    station
        .set_channel(&ch("band"), Value::On)
        .await
        .expect("band");

    handle.open_transmit_gate(TransmitPermit::new("station interlock test"));
    handle.set_ptt(true).await.expect("key");
    assert_eq!(handle.key_state().await, KeyState::Keyed);
    for refused in [
        station.select(&ch("ant"), 2).await,
        station.set_channel(&ch("band"), Value::Off).await,
        station.set_channel(&ch("amp_pwr"), Value::On).await,
    ] {
        assert_eq!(refused.expect_err("keyed").kind, StationErrorKind::Refused);
    }
    assert!(bank.line(1) && bank.line(4));
    // the amplifier's power-off passes, then the rig's; neither asks for anything
    assert_eq!(
        station.power_off(&dev("amp")).await.expect("amp off"),
        vec![dev("amp")]
    );
    assert!(!bank.line(1));
    assert_eq!(
        station.power_off(&dev("ic7300")).await.expect("rig off"),
        vec![dev("ic7300")]
    );
    assert!(!bank.line(0));
    // the rig's power-on passes while keyed (an off radio cannot be keyed); the amplifier's,
    // the antenna and the band do not
    station.power_on(&dev("ic7300")).await.expect("rig on");
    assert!(bank.line(0));
    for refused in [
        station.power_on(&dev("amp")).await.map(|_| ()),
        station.select(&ch("ant"), 2).await,
        station.set_channel(&ch("band"), Value::On).await,
    ] {
        assert_eq!(refused.expect_err("keyed").kind, StationErrorKind::Refused);
    }
    assert!(!bank.line(1) && bank.line(4), "nothing switched");
    // the station never touched the rig: still keyed, by the test's own permit
    assert_eq!(handle.key_state().await, KeyState::Keyed);
    assert!(control.state().ptt);
    handle.set_ptt(false).await.expect("unkey");
    handle.close_transmit_gate();
    // a power-off with the gate closed does not open it
    station
        .set_channel(&ch("radio_pwr"), Value::Off)
        .await
        .expect("off is always allowed");
    assert_eq!(
        handle.set_ptt(true).await.expect_err("gate").kind,
        radioctl_core::error::ErrorKind::TransmitGateClosed
    );
}

// Tests [@ANCHOR: radioctl_station_rig_key_state]
// Tests [@ANCHOR: radioctl_station_station_guard_action]
// Tests [@ANCHOR: radioctl_station_station_classify_on]
#[tokio::test(start_paused = true)]
async fn a_dead_link_is_an_unknown_keyed_state_that_allows_power_off_and_radio_power_on_only() {
    // Bruce, 2026-10-31: while the keyed state is UNKNOWN, power-off of any device and power-on
    // of a down radio pass; amplifier power-on, antenna, band and other outputs are refused; the
    // transmit gate stays closed throughout.
    let (near, far) = tokio::io::duplex(64);
    let handle = spawn_current(
        CivEngine::new(RigDescription::ic7300()).expect("engine"),
        near,
        RigOptions {
            poll: radioctl_core::poll::PollConfig {
                enabled: false,
                ..radioctl_core::poll::PollConfig::default()
            },
            ..RigOptions::default()
        },
    )
    .expect("spawn");
    let (station, bank) = keyed_station(Arc::new(handle.clone()));
    // the link is up but nothing has been read: unknown, so the RF path is already closed
    assert_eq!(handle.key_state().await, KeyState::Unknown);
    station
        .power_on(&dev("ic7300"))
        .await
        .expect("down radio on");
    assert!(bank.line(0));
    drop(far);
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    assert!(!handle.link_up());
    assert_eq!(handle.key_state().await, KeyState::Unknown);
    for refused in [
        station.power_on(&dev("amp")).await.map(|_| ()),
        station.set_channel(&ch("amp_pwr"), Value::On).await,
        station.select(&ch("ant"), 1).await,
        station.set_channel(&ch("band"), Value::On).await,
    ] {
        assert_eq!(
            refused.expect_err("unknown").kind,
            StationErrorKind::KeyedUnknown
        );
    }
    assert!(!bank.line(1) && !bank.line(4));
    // the power cycle of the radio whose link dropped (the amplifier's power state was never
    // confirmed, so the orderly power-off takes it first)
    assert_eq!(
        station.power_off(&dev("ic7300")).await.expect("off"),
        vec![dev("amp"), dev("ic7300")]
    );
    assert!(!bank.line(0));
    station.power_on(&dev("ic7300")).await.expect("on again");
    assert!(bank.line(0));
    // the gate was never opened by any of it
    assert_eq!(
        handle.set_ptt(true).await.expect_err("gate").kind,
        radioctl_core::error::ErrorKind::TransmitGateClosed
    );
}

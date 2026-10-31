// SPDX-License-Identifier: AGPL-3.0-or-later
//! The amplifier and tuner channel type: operate and standby behind the interlocks, the fault
//! line, the tune pulse, standby before power-off and in the emergency all-off, and the checks on
//! the station definition. A virtual bank stands in for the relay board; nothing is keyed.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use radioctl_station::channel::{BoxFuture, Switchboard, VirtualBank};
use radioctl_station::error::StationErrorKind;
use radioctl_station::model::{ChannelId, DeviceId, StationDef, Value};
use radioctl_station::station::{KeyState, KeyedSource, Station};

const DEF: &str = r#"
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
id = "amp_operate"
type = "amplifier"
device = "amp"
function = "operate"
fault_input = "amp_fault"
driver = "bank"
line = 2
safe = "off"
[[channels]]
id = "amp_fault"
type = "input"
driver = "bank"
line = 3
[[channels]]
id = "tuner_start"
type = "amplifier"
device = "tuner"
function = "tune"
pulse_ms = 200
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
[[devices]]
id = "tuner"
kind = "tuner"
power = { source = "always_on" }
"#;

struct Key(AtomicU8);
impl KeyedSource for Key {
    fn name(&self) -> String {
        "rig".into()
    }
    fn key_state(&self) -> BoxFuture<'_, KeyState> {
        Box::pin(async move {
            match self.0.load(Ordering::SeqCst) {
                0 => KeyState::Unkeyed,
                1 => KeyState::Keyed,
                _ => KeyState::Unknown,
            }
        })
    }
}

fn ch(s: &str) -> ChannelId {
    ChannelId(s.into())
}
fn dev(s: &str) -> DeviceId {
    DeviceId(s.into())
}

fn world() -> (Station, VirtualBank, Arc<Key>) {
    let def = StationDef::from_toml(DEF).expect("def");
    let bank = VirtualBank::new();
    let mut b = Switchboard::new();
    for c in def.channels.clone() {
        b.add_channel(c, Arc::new(bank.clone())).expect("chan");
    }
    let key = Arc::new(Key(AtomicU8::new(0)));
    let station = Station::new(def, Arc::new(b), vec![key.clone()], None).expect("station");
    (station, bank, key)
}

/// Radio and amplifier powered, fault line read as clear.
async fn powered(station: &Station) {
    station.power_on(&dev("ic7300")).await.expect("radio");
    station.power_on(&dev("amp")).await.expect("amp");
    station
        .board()
        .read_back(&ch("amp_fault"))
        .await
        .expect("fault read");
}

// Tests [@ANCHOR: radioctl_station_station_check_operate]
// Tests [@ANCHOR: radioctl_station_station_check_amp_powered]
// Tests [@ANCHOR: radioctl_station_station_set_channel]
#[tokio::test(start_paused = true)]
async fn operate_needs_the_amplifier_powered_and_the_fault_line_clear_and_known() {
    let (station, bank, _key) = world();
    // Unpowered: refused, nothing written to the operate line.
    let e = station
        .set_channel(&ch("amp_operate"), Value::On)
        .await
        .unwrap_err();
    assert_eq!(e.kind, StationErrorKind::Refused);
    assert!(bank.log().iter().all(|(line, _)| *line != 2));

    station.power_on(&dev("ic7300")).await.unwrap();
    station.power_on(&dev("amp")).await.unwrap();
    // The fault line has never been read: unknown refuses (fail closed).
    let e = station
        .set_channel(&ch("amp_operate"), Value::On)
        .await
        .unwrap_err();
    assert!(e.detail.contains("unknown"), "{}", e.detail);
    // Tripped refuses.
    bank.set_physical(3, true);
    station.board().read_back(&ch("amp_fault")).await.unwrap();
    let e = station
        .set_channel(&ch("amp_operate"), Value::On)
        .await
        .unwrap_err();
    assert!(e.detail.contains("tripped"), "{}", e.detail);
    // Clear: operate.
    bank.set_physical(3, false);
    station.board().read_back(&ch("amp_fault")).await.unwrap();
    station
        .set_channel(&ch("amp_operate"), Value::On)
        .await
        .unwrap();
    assert!(bank.line(2));
    assert_eq!(
        station
            .board()
            .status(&ch("amp_operate"))
            .unwrap()
            .confirmed,
        Some(Value::On)
    );
    station
        .set_channel(&ch("amp_operate"), Value::Off)
        .await
        .unwrap();
    assert!(!bank.line(2));
}

// Tests [@ANCHOR: radioctl_station_station_classify_channel]
// Tests [@ANCHOR: radioctl_station_station_guard_action]
#[tokio::test(start_paused = true)]
async fn operate_is_an_rf_path_change_and_standby_is_the_safe_direction() {
    let (station, bank, key) = world();
    powered(&station).await;
    station
        .set_channel(&ch("amp_operate"), Value::On)
        .await
        .unwrap();

    // Keyed (known): standby passes like a power-off (it is the first step of one, Bruce
    // 2026-10-31); operate does not.
    key.0.store(1, Ordering::SeqCst);
    assert_eq!(
        station
            .set_channel(&ch("amp_operate"), Value::On)
            .await
            .unwrap_err()
            .kind,
        StationErrorKind::Refused
    );
    station
        .set_channel(&ch("amp_operate"), Value::Off)
        .await
        .unwrap();
    assert!(!bank.line(2));
    assert_eq!(
        station
            .set_channel(&ch("amp_operate"), Value::On)
            .await
            .unwrap_err()
            .kind,
        StationErrorKind::Refused
    );
    assert!(!bank.line(2));
    // Unknown keyed state: standby passes (it can only stop RF), operate does not.
    key.0.store(0, Ordering::SeqCst);
    station
        .set_channel(&ch("amp_operate"), Value::On)
        .await
        .unwrap();
    key.0.store(2, Ordering::SeqCst);
    station
        .set_channel(&ch("amp_operate"), Value::Off)
        .await
        .unwrap();
    assert!(!bank.line(2));
    let e = station
        .set_channel(&ch("amp_operate"), Value::On)
        .await
        .unwrap_err();
    assert_eq!(e.kind, StationErrorKind::KeyedUnknown);
}

// Tests [@ANCHOR: radioctl_station_station_standby]
// Tests [@ANCHOR: radioctl_station_station_power_off]
// Tests [@ANCHOR: radioctl_station_station_all_off]
#[tokio::test(start_paused = true)]
async fn standby_comes_before_power_off_and_in_the_emergency_all_off() {
    let (station, bank, _key) = world();
    powered(&station).await;
    station
        .set_channel(&ch("amp_operate"), Value::On)
        .await
        .unwrap();
    let before = bank.log().len();
    let off = station.power_off(&dev("amp")).await.unwrap();
    assert_eq!(off, vec![dev("amp")]);
    let log = bank.log();
    let tail = &log[before..];
    let standby = tail
        .iter()
        .position(|w| *w == (2, false))
        .expect("standby written");
    let power = tail
        .iter()
        .position(|w| *w == (1, false))
        .expect("power removed");
    assert!(standby < power, "standby first: {tail:?}");
}

// Tests [@ANCHOR: radioctl_station_station_all_off]
#[tokio::test(start_paused = true)]
async fn all_off_puts_the_amplifier_in_standby_before_anything_else() {
    let (station, bank, key) = world();
    powered(&station).await;
    station
        .set_channel(&ch("amp_operate"), Value::On)
        .await
        .unwrap();
    key.0.store(1, Ordering::SeqCst); // keyed: all-off is never blocked
    let before = bank.log().len();
    let report = station.all_off().await;
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let log = bank.log();
    assert_eq!(
        log[before],
        (2, false),
        "standby is the first write: {:?}",
        &log[before..]
    );
    assert!(!bank.line(2) && !bank.line(1) && !bank.line(0));
}

// Tests [@ANCHOR: radioctl_station_station_pulse]
// Tests [@ANCHOR: radioctl_station_board_pulse]
#[tokio::test(start_paused = true)]
async fn the_tune_pulse_is_an_rf_path_command_with_the_configured_width() {
    let (station, bank, key) = world();
    // The tuner is always on: it can be pulsed.
    key.0.store(1, Ordering::SeqCst);
    assert!(
        station.pulse(&ch("tuner_start")).await.is_err(),
        "not while keyed"
    );
    key.0.store(0, Ordering::SeqCst);
    let start = tokio::time::Instant::now();
    station.pulse(&ch("tuner_start")).await.unwrap();
    assert!(start.elapsed() >= std::time::Duration::from_millis(200));
    assert_eq!(bank.log(), vec![(4, true), (4, false)]);
    assert!(!bank.line(4));
    // A pulse on an operate channel is not a pulse.
    assert!(station.pulse(&ch("amp_operate")).await.is_err());
}

// Tests [@ANCHOR: radioctl_station_channel_validate]
// Tests [@ANCHOR: radioctl_station_def_validate]
#[test]
fn the_definition_checks_amplifier_channels() {
    let cases = [
        (
            DEF.replacen(
                "safe = \"off\"\n[[channels]]\nid = \"amp_fault\"",
                "[[channels]]\nid = \"amp_fault\"",
                1,
            ),
            "needs safe",
        ),
        (DEF.replace("pulse_ms = 200", "pulse_ms = 0"), "tune pulse"),
        (
            DEF.replace(
                "device = \"amp\"\nfunction",
                "device = \"ic7300\"\nfunction",
            ),
            "not an amplifier or tuner",
        ),
        (
            DEF.replace("fault_input = \"amp_fault\"", "fault_input = \"amp_pwr\""),
            "not an input",
        ),
        (
            DEF.replace(
                "device = \"tuner\"\nfunction = \"tune\"\npulse_ms = 200",
                "device = \"amp\"\nfunction = \"operate\"\nsafe = \"off\"",
            ),
            "two operate",
        ),
        (
            DEF.replace("device = \"amp\"\nfunction", "device = \"nope\"\nfunction"),
            "not an amplifier or tuner",
        ),
    ];
    for (text, want) in cases {
        let e = StationDef::from_toml(&text).expect_err(want);
        assert!(e.detail.contains(want), "wanted {want:?}: {}", e.detail);
    }
    assert!(StationDef::from_toml(DEF).is_ok());
}

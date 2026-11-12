// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Elecraft engine and the virtual KX3 behind the real rig actor (no transport yet but the
//! in-memory stream): reads, sets, the closed transmit gate, and auto-information.
#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::rig::{spawn, Freshness, RigHandle, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Level, Mode, Vfo};
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::io::{spawn_virtual_rig, VirtualRigControl};
use radioctl_protocols::virtual_rig::{PanelChange, VirtualRig};
use tokio::runtime::Handle;
use tokio::time::sleep;

fn kx3_description() -> RigDescription {
    rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))
        .expect("kx3 file")
        .description
}

fn kx3(options: RigOptions) -> (RigHandle, VirtualRigControl) {
    let desc = kx3_description();
    let rig = VirtualRig::from_description(&desc).expect("virtual rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), rig, Duration::ZERO);
    let engine = ElecraftEngine::new(desc).expect("engine");
    let handle = spawn(&Handle::current(), engine, stream, options).expect("spawn");
    (handle, control)
}

fn quiet() -> RigOptions {
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options
}

// Tests [@ANCHOR: radioctl_elecraft_engine_encode]
// Tests [@ANCHOR: radioctl_elecraft_engine_feed]
// Tests [@ANCHOR: radioctl_core_handle_execute]
#[tokio::test(start_paused = true)]
async fn the_actor_drives_a_virtual_kx3_through_its_whole_vocabulary() {
    let (rig, control) = kx3(quiet());
    let get = |op: Op| {
        let rig = rig.clone();
        async move { rig.execute(op).await.expect("read").expect("value") }
    };
    assert_eq!(
        get(Op::GetFrequency(Vfo::A)).await,
        Observation::Frequency(Vfo::A, Hz(14_060_000))
    );
    rig.set_frequency(Vfo::B, Hz(14_062_000))
        .await
        .expect("set");
    rig.execute(Op::SetSplit(true)).await.expect("split");
    rig.execute(Op::SetRit(true)).await.expect("rit");
    rig.execute(Op::SetRitOffset(-120)).await.expect("offset");
    rig.execute(Op::SetFilterWidth(Hz(400))).await.expect("bw");
    rig.execute(Op::SetLevel(Level::KeyerSpeed, 22))
        .await
        .expect("ks");
    let state = control.state();
    assert!(state.split && state.rit);
    assert_eq!(state.rit_offset, -120);
    assert_eq!(state.filter_width, Hz(400));
    assert_eq!(state.freq_b, Hz(14_062_000));
    assert_eq!(get(Op::GetSplit).await, Observation::Split(true));
    assert_eq!(get(Op::GetRitOffset).await, Observation::RitOffset(-120));
    assert_eq!(
        get(Op::GetFilterWidth).await,
        Observation::FilterWidth(Hz(400))
    );
    assert_eq!(
        get(Op::GetLevel(Level::KeyerSpeed)).await,
        Observation::Level(Level::KeyerSpeed, 22)
    );
    assert_eq!(get(Op::GetModelId).await, Observation::ModelId(17));
    assert_eq!(get(Op::GetFirmware).await, Observation::Firmware(2, 37));
    // Unkeyed, a KX3 has no SWR reading and answers SW000 (MCU 3.02, 2026-11-05).
    assert_eq!(get(Op::GetSwr).await, Observation::Swr(0));
    assert_eq!(get(Op::GetPtt).await, Observation::Ptt(false));
    assert_eq!(rig.stats().commands_failed, 0);
}

// Tests [@ANCHOR: radioctl_core_handle_open_gate]
// Tests [@ANCHOR: radioctl_core_handle_set_ptt]
// Tests [@ANCHOR: radioctl_elecraft_encode_set]
#[tokio::test(start_paused = true)]
async fn keying_and_cw_are_refused_while_the_gate_is_closed_and_nothing_reaches_the_rig() {
    let (rig, control) = kx3(quiet());
    let before = control.bytes_received();
    let ptt = rig.set_ptt(true).await.expect_err("gate closed");
    assert_eq!(ptt.kind, ErrorKind::TransmitGateClosed);
    let cw = rig
        .execute(Op::SendCw("CQ".into()))
        .await
        .expect_err("gate closed");
    assert_eq!(cw.kind, ErrorKind::TransmitGateClosed);
    sleep(Duration::from_millis(50)).await;
    assert_eq!(control.bytes_received(), before);
    assert!(!control.state().ptt);
    assert!(control.state().cw_sent.is_empty());
    rig.set_ptt(false)
        .await
        .expect("unkeying is always allowed");
}

// Tests [@ANCHOR: radioctl_elecraft_decode]
// Tests [@ANCHOR: radioctl_elecraft_decode_if]
// Tests [@ANCHOR: radioctl_core_actor_observe]
#[tokio::test(start_paused = true)]
async fn auto_info_from_the_panel_reaches_the_cache_and_the_event_stream() {
    let (rig, control) = kx3(quiet());
    let mut events = rig.events();
    rig.execute(Op::SetTransceive(true)).await.expect("ai2");
    control.front_panel(PanelChange::Frequency(Hz(7_100_000)));
    control.front_panel(PanelChange::Mode(Mode::Lsb));
    control.front_panel(PanelChange::Split(true));
    sleep(Duration::from_millis(100)).await;
    let cached = |field| rig.cached(field).map(|c| c.value);
    assert_eq!(
        cached(Field::Frequency(Vfo::A)),
        Some(Observation::Frequency(Vfo::A, Hz(7_100_000)))
    );
    assert_eq!(cached(Field::Mode), Some(Observation::Mode(Mode::Lsb)));
    assert_eq!(cached(Field::Split), Some(Observation::Split(true)));
    let first = events.next().await.expect("an event");
    assert!(first
        .changes
        .iter()
        .any(|c| c.field == Field::Frequency(Vfo::A)));
    assert!(rig.stats().unsolicited >= 3);
    let fresh = rig.mode(Freshness::Fresh).await.expect("mode");
    assert_eq!(fresh.value, Mode::Lsb);
}

// Tests [@ANCHOR: radioctl_core_actor_apply_set]
#[tokio::test(start_paused = true)]
async fn the_transmit_frequency_moves_with_the_split_set_not_with_the_next_poll() {
    // KX3 bench check 2026-11-05: `S 1 VFOB` then `I 14065000`, then `\get_active_tx_freq` read
    // the receive VFO's frequency until a poll learned the transmit VFO two seconds later.
    // The `FT` poll that ran before the split was set cached "transmit on A".
    let (rig, _control) = kx3(quiet());
    rig.execute(Op::GetFrequency(Vfo::A)).await.expect("a");
    rig.execute(Op::GetFrequency(Vfo::B)).await.expect("b");
    rig.execute(Op::GetTxVfo).await.expect("tx vfo");
    let active = || rig.active_tx_frequency(Freshness::Cached);
    assert_eq!(active().await.expect("simplex"), Hz(14_060_000));
    rig.execute(Op::SetSplit(true)).await.expect("split on");
    rig.set_frequency(Vfo::B, Hz(14_065_000)).await.expect("b");
    assert_eq!(active().await.expect("split"), Hz(14_065_000));
    rig.execute(Op::SetSplit(false)).await.expect("split off");
    assert_eq!(active().await.expect("simplex again"), Hz(14_060_000));
}

// Tests [@ANCHOR: radioctl_core_actor_apply_set]
#[tokio::test(start_paused = true)]
async fn the_unkey_updates_the_cached_ptt_at_once() {
    // KX3 bench check 2026-11-05: `t` read 1 about 0.85 s after `T 0`. The actor runs polls and
    // sets one at a time, so a poll cannot overwrite the unkey with an older answer; this pins
    // that the cache says "receiving" the moment the unkey returns.
    let (rig, control) = kx3(quiet());
    rig.open_transmit_gate(TransmitPermit::new("kx3 test"));
    rig.set_ptt(true).await.expect("keyed");
    assert_eq!(
        rig.execute(Op::GetPtt).await.expect("read"),
        Some(Observation::Ptt(true))
    );
    rig.set_ptt(false).await.expect("unkeyed");
    assert_eq!(
        rig.cached(Field::Ptt).map(|c| c.value),
        Some(Observation::Ptt(false))
    );
    sleep(Duration::from_secs(1)).await;
    assert_eq!(
        rig.cached(Field::Ptt).map(|c| c.value),
        Some(Observation::Ptt(false))
    );
    assert!(!control.state().ptt);
}

// Tests [@ANCHOR: radioctl_elecraft_engine_encode]
// Tests [@ANCHOR: radioctl_elecraft_engine_feed]
#[tokio::test(start_paused = true)]
async fn a_k3_antenna_jack_is_set_and_read_through_the_actor() {
    let desc = rigfile::parse(include_str!("../../../rigs/elecraft-k3.toml"))
        .expect("k3 file")
        .description;
    let rig = VirtualRig::from_description(&desc).expect("virtual rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), rig, Duration::ZERO);
    let engine = ElecraftEngine::new(desc).expect("engine");
    let handle = spawn(&Handle::current(), engine, stream, quiet()).expect("spawn");
    handle
        .execute(Op::SetLevel(Level::Antenna, 2))
        .await
        .expect("antenna 2");
    assert_eq!(control.state().levels.get(&Level::Antenna), Some(&2));
    let refused = handle
        .execute(Op::SetLevel(Level::Antenna, 3))
        .await
        .expect_err("no third jack");
    assert_eq!(refused.kind, ErrorKind::Unsupported);
}

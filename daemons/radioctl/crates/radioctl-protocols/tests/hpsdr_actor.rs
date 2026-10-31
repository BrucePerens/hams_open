// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `hpsdr-network` engine behind the real rig actor, bridged to the `hpsdr` crate's Protocol 1 and
//! Protocol 2 sessions on loopback emulators (a virtual ANAN): frequency, power and SWR, the attenuator
//! on the control handle, and the closed transmit gate checked at the emulator.
#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use hpsdr::discovery::discover;
use hpsdr::emulator::{Emulator, EmulatorConfig, EmulatorCounters};
use hpsdr::emulator_p2::{loopback_ports, EmulatorP2, P2Counters, P2EmulatorConfig};
use hpsdr::types::HardwareStatus;
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::rig::{spawn, Freshness, RigHandle, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Level, Vfo};
use radioctl_protocols::hpsdr_io::{serve_hpsdr, start_session, HpsdrControl, HpsdrOptions};
use radioctl_protocols::hpsdr_net::HpsdrEngine;
use tokio::runtime::Handle;
use tokio::time::sleep;

const STATUS: HardwareStatus = HardwareStatus {
    adc_overload: [false; 4],
    adc_max_magnitude: [0; 2],
    ptt_input: false,
    dot_input: false,
    dash_input: false,
    exciter_raw: 0,
    forward_raw: 1000,
    reverse_raw: 100,
    supply_raw: 3500,
    firmware: 0,
    pll_locked: None,
};

async fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..200 {
        if check() {
            return;
        }
        sleep(Duration::from_millis(20)).await;
    }
    panic!("never happened: {what}");
}

async fn rig_for(
    link: radioctl_protocols::hpsdr_io::HpsdrLink,
    gate: hpsdr::gate::TxGate,
) -> (RigHandle, HpsdrControl) {
    let desc = rigfile::parse(include_str!("../../../rigs/anan-100.toml"))
        .expect("file")
        .description;
    let opts = HpsdrOptions {
        status_every: Duration::from_millis(50),
        ..HpsdrOptions::default()
    };
    let (stream, control) = serve_hpsdr(&Handle::current(), link, gate, 7_100_000, opts);
    let mut ro = RigOptions::default();
    ro.poll.enabled = false;
    ro.default_timeout = Duration::from_secs(3);
    let engine = HpsdrEngine::new(desc).expect("engine");
    (
        spawn(&Handle::current(), engine, stream, ro).expect("spawn"),
        control,
    )
}

// Tests [@ANCHOR: radioctl_hpsdr_serve]
// Tests [@ANCHOR: radioctl_hpsdr_status_of]
// Tests [@ANCHOR: radioctl_hpsdr_handle_line]
// Tests [@ANCHOR: radioctl_hpsdr_virtual_anan]
// Tests [@ANCHOR: radioctl_hpsdr_set_frequency]
// Tests [@ANCHOR: radioctl_hpsdr_request_ptt]
// Tests [@ANCHOR: radioctl_hpsdr_swr_tenths]
// Tests [@ANCHOR: radioctl_hpsdr_options_default]
// Tests [@ANCHOR: radioctl_hpsdr_control_status]
// Tests [@ANCHOR: radioctl_hpsdr_control_set_attenuator]
// Tests [@ANCHOR: radioctl_hpsdr_set_attenuator]
// Tests [@ANCHOR: radioctl_hpsdr_control_take_streams]
#[tokio::test]
async fn protocol_1_virtual_anan_tunes_reports_power_and_keeps_the_gate_closed() {
    let emu = Emulator::start_p1(
        &Handle::current(),
        0,
        EmulatorConfig {
            status: STATUS,
            ..EmulatorConfig::default()
        },
    )
    .expect("emulator");
    let found = discover(
        &Handle::current(),
        &[emu.addr()],
        Duration::from_millis(300),
    )
    .await
    .expect("discover");
    let (link, gate) = start_session(&Handle::current(), &found[0], None, 7_100_000)
        .await
        .expect("session");
    let (rig, control) = rig_for(link, gate).await;
    common(&rig, &control).await;
    let c = emu.counters();
    // Closed gate: the actor refuses, and nothing that can key reaches the radio.
    let err = rig.set_ptt(true).await.expect_err("gate closed");
    assert_eq!(err.kind, ErrorKind::TransmitGateClosed);
    sleep(Duration::from_millis(200)).await;
    assert_eq!(EmulatorCounters::get(&c.mox_frames), 0);
    assert_eq!(EmulatorCounters::get(&c.drive_nonzero_frames), 0);
    assert_eq!(EmulatorCounters::get(&c.tx_iq_packets), 0);
    // Open: the same request keys; closing unkeys.
    rig.open_transmit_gate(TransmitPermit::new("hpsdr test"));
    rig.set_ptt(true).await.expect("keys");
    eventually("mox", || EmulatorCounters::get(&c.mox_frames) > 0).await;
    rig.set_ptt(false).await.expect("unkeys");
    sleep(Duration::from_millis(100)).await;
    let frames = EmulatorCounters::get(&c.mox_frames);
    sleep(Duration::from_millis(200)).await;
    assert_eq!(
        EmulatorCounters::get(&c.mox_frames),
        frames,
        "unkeyed for good"
    );
    emu.stop();
}

/// Frequency, power and SWR through the actor, and the attenuator through the control handle.
async fn common(rig: &RigHandle, control: &HpsdrControl) {
    let f = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("frequency");
    assert_eq!(f.value, Hz(7_100_000));
    rig.set_frequency(Vfo::A, Hz(14_200_000))
        .await
        .expect("set");
    let f = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("frequency");
    assert_eq!(f.value, Hz(14_200_000));
    eventually("power", || {
        matches!(
            rig.cached(Field::Power).map(|c| c.value),
            Some(Observation::Power(1000))
        )
    })
    .await;
    // Power goes as counts squared: 1000 and 100 counts reflect 0.1 in voltage, SWR 1.22.
    eventually("swr", || {
        matches!(rig.cached(Field::Swr).map(|c| c.value), Some(Observation::Swr(s)) if (11..=14).contains(&s))
    })
    .await;
    assert!((control.status().supply_volts() - 2.82).abs() < 0.05);
    // The attenuator as a core level through the actor, then the bridge handle's own bounds.
    rig.execute(Op::SetLevel(Level::Attenuator, 20))
        .await
        .expect("attenuator set");
    let read = rig
        .execute(Op::GetLevel(Level::Attenuator))
        .await
        .expect("attenuator read");
    assert_eq!(read, Some(Observation::Level(Level::Attenuator, 20)));
    let refused = rig
        .execute(Op::SetLevel(Level::Attenuator, 32))
        .await
        .expect_err("beyond 31 dB");
    assert_eq!(refused.kind, ErrorKind::Unsupported);
    let refused = rig
        .execute(Op::SetLevel(Level::Preamp, 1))
        .await
        .expect_err("no preamplifier");
    assert_eq!(refused.kind, ErrorKind::Unsupported);
    rig.execute(Op::SetLevel(Level::Attenuator, 12))
        .await
        .expect("attenuator set");
    assert!(control.set_attenuator(32).is_err());
    assert!(!control.take_streams().is_empty());
}

#[tokio::test]
async fn protocol_2_virtual_anan_tunes_and_keeps_the_gate_closed() {
    let ports = loopback_ports(1).expect("ports");
    let emu = EmulatorP2::start(
        &Handle::current(),
        0,
        P2EmulatorConfig {
            ports,
            status: STATUS,
            ..P2EmulatorConfig::default()
        },
    )
    .expect("emulator");
    let found = discover(
        &Handle::current(),
        &[emu.addr()],
        Duration::from_millis(300),
    )
    .await
    .expect("discover");
    let (link, gate) = start_session(&Handle::current(), &found[0], Some(ports), 7_100_000)
        .await
        .expect("session");
    let (rig, control) = rig_for(link, gate).await;
    common(&rig, &control).await;
    eventually("attenuator reached the radio", || {
        P2Counters::get(&emu.counters().last_atten_adc0) == 12
    })
    .await;
    let c = emu.counters();
    let err = rig.set_ptt(true).await.expect_err("gate closed");
    assert_eq!(err.kind, ErrorKind::TransmitGateClosed);
    sleep(Duration::from_millis(200)).await;
    assert_eq!(P2Counters::get(&c.ptt_frames), 0);
    assert_eq!(P2Counters::get(&c.drive_nonzero_frames), 0);
    assert_eq!(P2Counters::get(&c.duc_packets), 0);
    assert_eq!(P2Counters::get(&c.tx_freq_nonzero_frames), 0);
    rig.open_transmit_gate(TransmitPermit::new("hpsdr test"));
    rig.set_ptt(true).await.expect("keys");
    eventually("ptt", || P2Counters::get(&c.ptt_frames) > 0).await;
    rig.set_ptt(false).await.expect("unkeys");
    eventually("unkeyed", || P2Counters::get(&c.ptt_now) == 0).await;
    emu.stop();
}

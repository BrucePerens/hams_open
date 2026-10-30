// SPDX-License-Identifier: AGPL-3.0-or-later
//! Each engine against the virtual rig generated from the same description, with no I/O: the
//! driver and its test double cannot drift apart without one of these failing.
#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind};
use radioctl_core::op::{Observation, Op};
use radioctl_core::units::{Hz, Mode, Vfo};
use radioctl_protocols::civ::CivEngine;
use radioctl_protocols::kenwood::KenwoodEngine;
use radioctl_protocols::virtual_rig::{PanelChange, VirtualRig};

const NOW: Duration = Duration::from_secs(10);

/// Sends one op through engine and rig; returns what the engine decoded from the answer.
fn exchange(engine: &mut dyn ProtocolEngine, rig: &mut VirtualRig, op: Op) -> Vec<Inbound> {
    let request = engine.encode(&op).expect("encode");
    let answer = rig.feed(&request.bytes, NOW);
    match request.reply {
        ReplyKind::None => assert!(answer.is_empty(), "unexpected answer to {op:?}"),
        _ => assert!(!answer.is_empty(), "no answer to {op:?}"),
    }
    engine.feed(&answer, NOW)
}

fn value(inbound: Vec<Inbound>) -> Observation {
    match inbound.as_slice() {
        [Inbound::Observation(observation)] => *observation,
        other => panic!("expected one observation, got {other:?}"),
    }
}

// Tests [@ANCHOR: radioctl_civ_engine_encode]
// Tests [@ANCHOR: radioctl_civ_engine_feed]
// Tests [@ANCHOR: radioctl_virtual_feed]
#[test]
fn civ_engine_and_virtual_ic7300_agree_on_every_operation() {
    let desc = RigDescription::ic7300();
    let mut engine = CivEngine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");

    let read = value(exchange(&mut engine, &mut rig, Op::GetFrequency(Vfo::A)));
    assert_eq!(read, Observation::Frequency(Vfo::A, Hz(14_074_000)));
    let set = exchange(
        &mut engine,
        &mut rig,
        Op::SetFrequency(Vfo::A, Hz(3_573_000)),
    );
    assert_eq!(set, vec![Inbound::Ack]);
    let read = value(exchange(&mut engine, &mut rig, Op::GetFrequency(Vfo::A)));
    assert_eq!(read, Observation::Frequency(Vfo::A, Hz(3_573_000)));

    assert_eq!(
        exchange(&mut engine, &mut rig, Op::SetMode(Mode::Cw)),
        vec![Inbound::Ack]
    );
    assert_eq!(
        value(exchange(&mut engine, &mut rig, Op::GetMode)),
        Observation::Mode(Mode::Cw)
    );

    assert_eq!(
        value(exchange(&mut engine, &mut rig, Op::GetPtt)),
        Observation::Ptt(false)
    );
    assert_eq!(
        exchange(&mut engine, &mut rig, Op::SetPtt(true)),
        vec![Inbound::Ack]
    );
    assert_eq!(
        value(exchange(&mut engine, &mut rig, Op::GetPtt)),
        Observation::Ptt(true)
    );
    assert_eq!(
        value(exchange(&mut engine, &mut rig, Op::GetPower)),
        Observation::Power(128)
    );
    assert_eq!(
        exchange(&mut engine, &mut rig, Op::SetPtt(false)),
        vec![Inbound::Ack]
    );
    assert_eq!(
        value(exchange(&mut engine, &mut rig, Op::GetPower)),
        Observation::Power(0)
    );
    assert_eq!(
        value(exchange(&mut engine, &mut rig, Op::GetSignal)),
        Observation::Signal(30)
    );

    let cw = exchange(&mut engine, &mut rig, Op::SendCw("CQ TEST".into()));
    assert_eq!(cw, vec![Inbound::Ack]);
    assert_eq!(rig.state().cw_sent, vec!["CQ TEST".to_string()]);
}

// Tests [@ANCHOR: radioctl_civ_engine_feed]
#[test]
fn civ_engine_reads_unsolicited_transceive_and_skips_bus_echo() {
    let mut desc = RigDescription::ic7300();
    if let Some(civ) = desc.civ.as_mut() {
        civ.echo = true;
    }
    let mut engine = CivEngine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    // With echo on, the answer holds our own frame first; the engine must skip it.
    let read = value(exchange(&mut engine, &mut rig, Op::GetFrequency(Vfo::A)));
    assert_eq!(read, Observation::Frequency(Vfo::A, Hz(14_074_000)));
    let knob = rig.front_panel(PanelChange::Frequency(Hz(14_075_500)));
    assert_eq!(
        engine.feed(&knob, NOW),
        vec![Inbound::Observation(Observation::Frequency(
            Vfo::A,
            Hz(14_075_500)
        ))]
    );
    let mode = rig.front_panel(PanelChange::Mode(Mode::Lsb));
    assert_eq!(
        engine.feed(&mode, NOW),
        vec![Inbound::Observation(Observation::Mode(Mode::Lsb))]
    );
}

// Tests [@ANCHOR: radioctl_kenwood_engine_encode]
// Tests [@ANCHOR: radioctl_kenwood_engine_feed]
// Tests [@ANCHOR: radioctl_virtual_feed]
#[test]
fn kenwood_engine_and_virtual_ts2000_agree_on_every_operation() {
    let desc = RigDescription::ts2000();
    let mut engine = KenwoodEngine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");

    let read = value(exchange(&mut engine, &mut rig, Op::GetFrequency(Vfo::A)));
    assert_eq!(read, Observation::Frequency(Vfo::A, Hz(14_074_000)));
    assert!(exchange(
        &mut engine,
        &mut rig,
        Op::SetFrequency(Vfo::B, Hz(7_040_000))
    )
    .is_empty());
    let read = value(exchange(&mut engine, &mut rig, Op::GetFrequency(Vfo::B)));
    assert_eq!(read, Observation::Frequency(Vfo::B, Hz(7_040_000)));
    let read = value(exchange(&mut engine, &mut rig, Op::GetFrequency(Vfo::A)));
    assert_eq!(read, Observation::Frequency(Vfo::A, Hz(14_074_000)));

    assert!(exchange(&mut engine, &mut rig, Op::SetMode(Mode::Lsb)).is_empty());
    assert_eq!(
        value(exchange(&mut engine, &mut rig, Op::GetMode)),
        Observation::Mode(Mode::Lsb)
    );
    assert!(exchange(&mut engine, &mut rig, Op::SetPtt(true)).is_empty());
    assert!(rig.state().ptt);
    assert!(exchange(&mut engine, &mut rig, Op::SetPtt(false)).is_empty());
    assert!(!rig.state().ptt);
    assert_eq!(
        value(exchange(&mut engine, &mut rig, Op::GetSignal)),
        Observation::Signal(30)
    );
}

// Tests [@ANCHOR: radioctl_kenwood_engine_feed]
#[test]
fn kenwood_auto_information_reports_the_knob() {
    let desc = RigDescription::ts2000();
    let mut engine = KenwoodEngine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    assert!(exchange(&mut engine, &mut rig, Op::SetTransceive(true)).is_empty());
    let knob = rig.front_panel(PanelChange::Frequency(Hz(14_080_000)));
    assert_eq!(
        engine.feed(&knob, NOW),
        vec![Inbound::Observation(Observation::Frequency(
            Vfo::A,
            Hz(14_080_000)
        ))]
    );
}

// Tests [@ANCHOR: radioctl_virtual_from_description]
#[test]
fn rigs_made_from_one_description_are_independent() {
    let desc = RigDescription::ic7300();
    let mut engine_a = CivEngine::new(desc.clone()).expect("engine");
    let mut rig_a = VirtualRig::from_description(&desc).expect("rig");
    let rig_b = VirtualRig::from_description(&desc).expect("rig");
    exchange(
        &mut engine_a,
        &mut rig_a,
        Op::SetFrequency(Vfo::A, Hz(21_074_000)),
    );
    exchange(&mut engine_a, &mut rig_a, Op::SetPtt(true));
    assert_eq!(rig_b.state().freq_a, Hz(14_074_000));
    assert!(!rig_b.state().ptt);
    assert_eq!(rig_a.state().freq_a, Hz(21_074_000));
}

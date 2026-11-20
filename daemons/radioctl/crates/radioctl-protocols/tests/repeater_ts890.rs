// SPDX-License-Identifier: AGPL-3.0-or-later
//! The TS-890S tone ops (`TO`, `TN`, `CN`) against the virtual TS-890S of the same description.
//! The guide has no DCS, repeater shift or offset command: those must be refused. A description
//! without the `repeater` capability refuses everything. No radio is involved.
#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use radioctl_core::capability::{Capability, CapabilitySet};
use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine};
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, RepeaterShift, ToneMode};
use radioctl_protocols::kenwood_ts890::Ts890Engine;
use radioctl_protocols::virtual_rig::VirtualRig;

const NOW: Duration = Duration::from_secs(3);

fn description() -> RigDescription {
    rigfile::parse(include_str!("../../../rigs/kenwood-ts-890s.toml"))
        .expect("loads")
        .description
}

fn exchange(engine: &mut Ts890Engine, rig: &mut VirtualRig, op: Op) -> Vec<Inbound> {
    let request = engine.encode(&op).expect("encode");
    let answer = rig.feed(&request.bytes, NOW);
    engine.feed(&answer, NOW)
}

fn observed(inbound: Vec<Inbound>) -> Observation {
    match inbound.as_slice() {
        [Inbound::Observation(o)] => *o,
        other => panic!("expected one observation, got {other:?}"),
    }
}

// Tests [@ANCHOR: radioctl_ts890_engine_encode]
// Tests [@ANCHOR: radioctl_ts890_tone_number]
#[test]
fn the_ts890s_encodes_the_tone_ops_as_its_guide_says() {
    let mut e = Ts890Engine::new(description()).expect("engine");
    let b = |e: &mut Ts890Engine, op: Op| e.encode(&op).expect("encodes").bytes;
    assert_eq!(b(&mut e, Op::GetToneMode), b"TO;");
    assert_eq!(b(&mut e, Op::SetToneMode(ToneMode::Off)), b"TO0;");
    assert_eq!(b(&mut e, Op::SetToneMode(ToneMode::Tone)), b"TO1;");
    assert_eq!(b(&mut e, Op::SetToneMode(ToneMode::Tsql)), b"TO2;");
    assert_eq!(b(&mut e, Op::GetCtcssTone), b"TN;");
    // 67.0 Hz is number 00, 88.5 Hz 08, 159.8 Hz 26 (the TS-890S chart has it), 254.1 Hz 49, 1750 Hz 50.
    assert_eq!(b(&mut e, Op::SetCtcssTone(670)), b"TN00;CN00;");
    assert_eq!(b(&mut e, Op::SetCtcssTone(885)), b"TN08;CN08;");
    assert_eq!(b(&mut e, Op::SetCtcssTone(1598)), b"TN26;CN26;");
    assert_eq!(b(&mut e, Op::SetCtcssTone(2541)), b"TN49;CN49;");
    assert_eq!(b(&mut e, Op::SetCtcssTone(17500)), b"TN50;");
    for op in [
        Op::SetToneMode(ToneMode::Dcs),
        Op::SetCtcssTone(886),
        Op::GetDcsCode,
        Op::SetDcsCode(23),
        Op::GetRepeaterShift,
        Op::SetRepeaterShift(RepeaterShift::Minus),
        Op::GetRepeaterOffset,
        Op::SetRepeaterOffset(Hz(600_000)),
    ] {
        assert_eq!(
            e.encode(&op).expect_err("refused").kind,
            ErrorKind::Unsupported,
            "{op:?}"
        );
    }
}

#[test]
fn a_description_without_the_capability_refuses_the_tone_ops() {
    let mut d = description();
    d.capabilities = CapabilitySet::of(&[Capability::ReadFrequency]);
    let mut e = Ts890Engine::new(d).expect("engine");
    for op in [
        Op::GetToneMode,
        Op::SetToneMode(ToneMode::Tone),
        Op::SetCtcssTone(885),
    ] {
        assert_eq!(
            e.encode(&op).expect_err("refused").kind,
            ErrorKind::Unsupported,
            "{op:?}"
        );
    }
}

// Tests [@ANCHOR: radioctl_ts890_engine_decode]
// Tests [@ANCHOR: radioctl_ts890_tone_value]
// Tests [@ANCHOR: radioctl_ts890_tone_number]
#[test]
fn the_ts890s_round_trips_against_its_virtual_twin() {
    let desc = description();
    let mut engine = Ts890Engine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    let mut ask = |op: Op| exchange(&mut engine, &mut rig, op);
    for mode in [
        ToneMode::Tone,
        ToneMode::Tsql,
        ToneMode::Off,
        ToneMode::Tone,
    ] {
        assert_eq!(ask(Op::SetToneMode(mode)), vec![]);
        assert_eq!(observed(ask(Op::GetToneMode)), Observation::ToneMode(mode));
    }
    for tenths in [670, 885, 1598, 2541, 17500] {
        assert_eq!(ask(Op::SetCtcssTone(tenths)), vec![]);
        assert_eq!(
            observed(ask(Op::GetCtcssTone)),
            Observation::CtcssTone(tenths)
        );
    }
}

#[test]
fn the_virtual_rig_refuses_a_tone_number_past_the_chart() {
    let mut rig = VirtualRig::from_description(&description()).expect("rig");
    assert_eq!(rig.feed(b"TN51;", NOW), b"?;");
    assert_eq!(rig.feed(b"CN50;", NOW), b"?;");
    assert_eq!(rig.feed(b"TN50;", NOW), b"");
}

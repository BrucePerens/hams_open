// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Kenwood tone ops (`TO`, `CT`, `TN`, `CN`) of the TS-590S, the one classic-dialect
//! description that lists the `repeater` capability, against the virtual Kenwood of the same
//! description. Models without a chart, or without the capability, must refuse them. No radio is
//! involved.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::time::Duration;

use radioctl_core::capability::{Capability, CapabilitySet};
use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind};
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, RepeaterShift, ToneMode};
use radioctl_protocols::kenwood::KenwoodEngine;
use radioctl_protocols::virtual_rig::VirtualRig;

const NOW: Duration = Duration::from_secs(5);

fn shipped(id: &str) -> RigDescription {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../rigs")
        .join(format!("{id}.toml"));
    let text = std::fs::read_to_string(&path).expect("read rig file");
    rigfile::parse(&text).expect("parse").description
}

fn exchange(engine: &mut KenwoodEngine, rig: &mut VirtualRig, op: Op) -> Vec<Inbound> {
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

// Tests [@ANCHOR: radioctl_kenwood_engine_encode_repeater]
// Tests [@ANCHOR: radioctl_kenwood_engine_tone_chart]
#[test]
fn the_ts590s_encodes_the_tone_ops_as_its_guide_says() {
    let mut e = KenwoodEngine::new(shipped("kenwood-ts-590s")).expect("engine");
    let b = |e: &mut KenwoodEngine, op: Op| e.encode(&op).expect("encodes").bytes;
    assert_eq!(b(&mut e, Op::GetToneMode), b"TO;CT;");
    assert_eq!(b(&mut e, Op::SetToneMode(ToneMode::Off)), b"TO0;CT0;");
    assert_eq!(b(&mut e, Op::SetToneMode(ToneMode::Tone)), b"TO1;CT0;");
    assert_eq!(b(&mut e, Op::SetToneMode(ToneMode::Tsql)), b"TO0;CT1;");
    assert_eq!(b(&mut e, Op::GetCtcssTone), b"TN;");
    // 67.0 Hz is number 00, 88.5 Hz 08, 254.1 Hz 41 (the last CN takes), 1750 Hz 42 (TN only).
    assert_eq!(b(&mut e, Op::SetCtcssTone(670)), b"TN00;CN00;");
    assert_eq!(b(&mut e, Op::SetCtcssTone(885)), b"TN08;CN08;");
    assert_eq!(b(&mut e, Op::SetCtcssTone(2541)), b"TN41;CN41;");
    assert_eq!(b(&mut e, Op::SetCtcssTone(17500)), b"TN42;");
    for op in [
        Op::SetToneMode(ToneMode::Dcs),
        Op::SetCtcssTone(886),
        Op::GetDcsCode,
        Op::SetDcsCode(23),
        Op::GetRepeaterShift,
        Op::SetRepeaterShift(RepeaterShift::Plus),
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
fn a_rig_without_the_capability_or_without_a_chart_refuses_the_tone_ops() {
    let mut d = shipped("kenwood-ts-590s");
    d.capabilities = CapabilitySet::of(&[Capability::ReadFrequency]);
    let mut without_capability = KenwoodEngine::new(d).expect("engine");
    let mut d = shipped("kenwood-ts-480");
    d.capabilities = CapabilitySet::of(&[Capability::Repeater]);
    let mut without_chart = KenwoodEngine::new(d).expect("engine");
    for engine in [&mut without_capability, &mut without_chart] {
        for op in [
            Op::GetToneMode,
            Op::SetToneMode(ToneMode::Tone),
            Op::SetCtcssTone(885),
        ] {
            assert_eq!(
                engine.encode(&op).expect_err("refused").kind,
                ErrorKind::Unsupported,
                "{op:?}"
            );
        }
    }
}

// Tests [@ANCHOR: radioctl_kenwood_engine_decode_tone_flag]
// Tests [@ANCHOR: radioctl_kenwood_ts590_tone_number]
// Tests [@ANCHOR: radioctl_kenwood_ts590_tone_value]
#[test]
fn the_ts590s_round_trips_against_its_virtual_twin() {
    let desc = shipped("kenwood-ts-590s");
    let mut engine = KenwoodEngine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    let mut ask = |op: Op| exchange(&mut engine, &mut rig, op);
    for mode in [
        ToneMode::Tone,
        ToneMode::Tsql,
        ToneMode::Off,
        ToneMode::Tsql,
    ] {
        assert_eq!(ask(Op::SetToneMode(mode)), vec![]);
        assert_eq!(value(ask(Op::GetToneMode)), Observation::ToneMode(mode));
    }
    for tenths in [670, 885, 1000, 1318, 2541, 17500] {
        assert_eq!(ask(Op::SetCtcssTone(tenths)), vec![]);
        assert_eq!(value(ask(Op::GetCtcssTone)), Observation::CtcssTone(tenths));
    }
}

#[test]
fn the_virtual_rig_refuses_a_tone_number_past_the_chart() {
    let desc = shipped("kenwood-ts-590s");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    assert_eq!(rig.feed(b"TN43;", NOW), b"?;");
    assert_eq!(rig.feed(b"CN42;", NOW), b"?;");
    assert_eq!(rig.feed(b"TN42;", NOW), b"");
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Yaesu newcat repeater and tone ops (`OS0`, `CT0`, `CN00`, `CN01`) for the FT-991A and the
//! FT-891, the two descriptions that list the `repeater` capability, against the virtual Yaesu of
//! the same description. A rig without the capability must refuse them. No radio is involved.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::time::Duration;

use radioctl_core::capability::{Capability, CapabilitySet};
use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind};
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, RepeaterShift, ToneMode};
use radioctl_protocols::virtual_rig::VirtualRig;
use radioctl_protocols::yaesu::YaesuEngine;

const NOW: Duration = Duration::from_secs(5);
const RIGS: [&str; 2] = ["yaesu-ft-991a", "yaesu-ft-891"];

fn shipped(id: &str) -> RigDescription {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../rigs")
        .join(format!("{id}.toml"));
    let text = std::fs::read_to_string(&path).expect("read rig file");
    rigfile::parse(&text).expect("parse").description
}

fn exchange(engine: &mut YaesuEngine, rig: &mut VirtualRig, op: Op) -> Vec<Inbound> {
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

// Tests [@ANCHOR: radioctl_yaesu_engine_encode]
// Tests [@ANCHOR: radioctl_yaesu_tone_number]
#[test]
fn the_ft991a_encodes_the_repeater_ops_as_the_cat_book_says() {
    let mut e = YaesuEngine::new(shipped("yaesu-ft-991a")).expect("engine");
    let b = |e: &mut YaesuEngine, op: Op| e.encode(&op).expect("encodes").bytes;
    assert_eq!(b(&mut e, Op::GetRepeaterShift), b"OS0;");
    assert_eq!(
        b(&mut e, Op::SetRepeaterShift(RepeaterShift::Plus)),
        b"OS01;"
    );
    assert_eq!(
        b(&mut e, Op::SetRepeaterShift(RepeaterShift::Minus)),
        b"OS02;"
    );
    assert_eq!(
        b(&mut e, Op::SetRepeaterShift(RepeaterShift::Simplex)),
        b"OS00;"
    );
    assert_eq!(b(&mut e, Op::GetToneMode), b"CT0;");
    assert_eq!(b(&mut e, Op::SetToneMode(ToneMode::Off)), b"CT00;");
    assert_eq!(b(&mut e, Op::SetToneMode(ToneMode::Tsql)), b"CT01;");
    assert_eq!(b(&mut e, Op::SetToneMode(ToneMode::Tone)), b"CT02;");
    assert_eq!(b(&mut e, Op::SetToneMode(ToneMode::Dcs)), b"CT03;");
    assert_eq!(b(&mut e, Op::GetCtcssTone), b"CN00;");
    // 67.0 Hz is number 000, 88.5 Hz number 008, 254.1 Hz number 049 (Table 1).
    assert_eq!(b(&mut e, Op::SetCtcssTone(670)), b"CN00000;");
    assert_eq!(b(&mut e, Op::SetCtcssTone(885)), b"CN00008;");
    assert_eq!(b(&mut e, Op::SetCtcssTone(2541)), b"CN00049;");
    assert_eq!(b(&mut e, Op::GetDcsCode), b"CN01;");
    // Code 023 is number 000, 074 number 015, 754 number 103 (Table 2).
    assert_eq!(b(&mut e, Op::SetDcsCode(23)), b"CN01000;");
    assert_eq!(b(&mut e, Op::SetDcsCode(74)), b"CN01015;");
    assert_eq!(b(&mut e, Op::SetDcsCode(754)), b"CN01103;");
    for op in [
        Op::SetCtcssTone(886),
        Op::SetDcsCode(24),
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
fn a_rig_without_the_capability_refuses_every_repeater_op() {
    let mut d = shipped("yaesu-ft-991a");
    d.capabilities = CapabilitySet::of(&[Capability::ReadFrequency]);
    let mut e = YaesuEngine::new(d).expect("engine");
    for op in [
        Op::GetRepeaterShift,
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

// Tests [@ANCHOR: radioctl_yaesu_engine_decode]
// Tests [@ANCHOR: radioctl_yaesu_tone_value]
// Tests [@ANCHOR: radioctl_yaesu_tone_number]
#[test]
fn both_rigs_round_trip_against_their_virtual_twins_and_every_chart_entry() {
    for id in RIGS {
        let desc = shipped(id);
        assert_eq!(desc.family, Family::YaesuNewcat, "{id}");
        let mut engine = YaesuEngine::new(desc.clone()).expect("engine");
        let mut rig = VirtualRig::from_description(&desc).expect("rig");
        let mut ask = |op: Op| exchange(&mut engine, &mut rig, op);
        for shift in [
            RepeaterShift::Plus,
            RepeaterShift::Minus,
            RepeaterShift::Simplex,
        ] {
            assert_eq!(ask(Op::SetRepeaterShift(shift)), vec![], "{id}");
            assert_eq!(
                value(ask(Op::GetRepeaterShift)),
                Observation::RepeaterShift(shift),
                "{id}"
            );
        }
        for mode in [ToneMode::Tone, ToneMode::Tsql, ToneMode::Dcs, ToneMode::Off] {
            assert_eq!(ask(Op::SetToneMode(mode)), vec![], "{id}");
            assert_eq!(
                value(ask(Op::GetToneMode)),
                Observation::ToneMode(mode),
                "{id}"
            );
        }
        for tenths in [670, 885, 1000, 1318, 2541] {
            assert_eq!(ask(Op::SetCtcssTone(tenths)), vec![], "{id}");
            assert_eq!(
                value(ask(Op::GetCtcssTone)),
                Observation::CtcssTone(tenths),
                "{id}"
            );
        }
        for code in [23, 74, 223, 465, 754] {
            assert_eq!(ask(Op::SetDcsCode(code)), vec![], "{id}");
            assert_eq!(
                value(ask(Op::GetDcsCode)),
                Observation::DcsCode(code),
                "{id}"
            );
        }
    }
}

#[test]
fn the_virtual_rig_refuses_a_tone_number_past_the_chart() {
    let desc = shipped("yaesu-ft-991a");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    assert_eq!(rig.feed(b"CN00050;", NOW), b"?;");
    assert_eq!(rig.feed(b"CN01104;", NOW), b"?;");
    assert_eq!(rig.feed(b"CN00049;", NOW), b"");
}

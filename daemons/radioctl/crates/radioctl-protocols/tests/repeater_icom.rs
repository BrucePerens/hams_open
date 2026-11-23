// SPDX-License-Identifier: AGPL-3.0-or-later
//! The CI-V repeater and tone ops (`0F 10..12`, `0C`/`0D`, `1B 00`, `1B 02`, `16 42/43/4B`)
//! against the virtual Icom of the same description, for every Icom description that lists the
//! `repeater` capability. A rig without the capability must refuse them. No radio is involved.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::time::Duration;

use radioctl_core::capability::Capability;
use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind};
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, RepeaterShift, ToneMode};
use radioctl_protocols::civ::{
    decode_repeater_offset, decode_tone_digits, encode_repeater_offset, encode_tone_digits,
    CivEngine,
};
use radioctl_protocols::virtual_rig::VirtualRig;

const NOW: Duration = Duration::from_secs(5);

fn shipped(id: &str) -> RigDescription {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../rigs")
        .join(format!("{id}.toml"));
    let text = std::fs::read_to_string(&path).expect("read rig file");
    rigfile::parse(&text).expect("parse").description
}

fn exchange(engine: &mut CivEngine, rig: &mut VirtualRig, op: Op) -> Vec<Inbound> {
    let request = engine.encode(&op).expect("encode");
    let answer = rig.feed(&request.bytes, NOW);
    assert!(
        request.reply == ReplyKind::None || !answer.is_empty(),
        "no answer to {op:?}"
    );
    engine.feed(&answer, NOW)
}

fn value(inbound: Vec<Inbound>) -> Observation {
    match inbound.as_slice() {
        [Inbound::Observation(observation)] => *observation,
        other => panic!("expected one observation, got {other:?}"),
    }
}

fn repeater_rigs() -> Vec<RigDescription> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("rigs") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "toml") {
            let text = std::fs::read_to_string(&path).expect("read");
            let desc = rigfile::parse(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
                .description;
            if desc.family == Family::IcomCiv && desc.capabilities.has(Capability::Repeater) {
                out.push(desc);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

// Tests [@ANCHOR: radioctl_civ_encode_repeater_offset]
// Tests [@ANCHOR: radioctl_civ_decode_repeater_offset]
// Tests [@ANCHOR: radioctl_civ_encode_tone_digits]
// Tests [@ANCHOR: radioctl_civ_decode_tone_digits]
#[test]
fn the_bcd_fields_have_the_documented_byte_order() {
    // 600 kHz is 6000 units of 100 Hz: 006000, least-significant pair first.
    assert_eq!(
        encode_repeater_offset(Hz(600_000)),
        Some([0x00, 0x60, 0x00])
    );
    assert_eq!(
        encode_repeater_offset(Hz(5_000_000)),
        Some([0x00, 0x00, 0x05])
    );
    assert_eq!(
        decode_repeater_offset(&[0x00, 0x60, 0x00]),
        Some(Hz(600_000))
    );
    assert_eq!(
        encode_repeater_offset(Hz(600_050)),
        None,
        "not a 100 Hz step"
    );
    assert_eq!(
        encode_repeater_offset(Hz(100_000_000)),
        None,
        "seven digits"
    );
    assert_eq!(decode_repeater_offset(&[0x00, 0x60]), None);
    // 88.5 Hz is 885 tenths: 000885, most-significant pair first.
    assert_eq!(encode_tone_digits(885), Some([0x00, 0x08, 0x85]));
    assert_eq!(decode_tone_digits(&[0x00, 0x08, 0x85]), Some(885));
    assert_eq!(decode_tone_digits(&[0x00, 0x0A, 0x85]), None, "not BCD");
}

// Tests [@ANCHOR: radioctl_civ_engine_encode_repeater]
// Tests [@ANCHOR: radioctl_civ_engine_decode_tone_flag]
#[test]
fn the_ic7000_encodes_the_repeater_ops_as_read_from_the_facts() {
    let mut e = CivEngine::new(shipped("icom-ic-7000")).expect("engine");
    let b = |e: &mut CivEngine, op: Op| e.encode(&op).expect("encodes").bytes;
    let with = |body: &[u8]| [&[0xFE, 0xFE, 0x70, 0xE0][..], body, &[0xFD]].concat();
    assert_eq!(b(&mut e, Op::GetRepeaterShift), with(&[0x0F]));
    assert_eq!(
        b(&mut e, Op::SetRepeaterShift(RepeaterShift::Plus)),
        with(&[0x0F, 0x12])
    );
    assert_eq!(
        b(&mut e, Op::SetRepeaterShift(RepeaterShift::Minus)),
        with(&[0x0F, 0x11])
    );
    assert_eq!(
        b(&mut e, Op::SetRepeaterShift(RepeaterShift::Simplex)),
        with(&[0x0F, 0x10])
    );
    assert_eq!(b(&mut e, Op::GetRepeaterOffset), with(&[0x0C]));
    assert_eq!(
        b(&mut e, Op::SetRepeaterOffset(Hz(600_000))),
        with(&[0x0D, 0x00, 0x60, 0x00])
    );
    assert_eq!(b(&mut e, Op::GetCtcssTone), with(&[0x1B, 0x00]));
    assert_eq!(
        b(&mut e, Op::SetCtcssTone(885)),
        with(&[0x1B, 0x00, 0x00, 0x08, 0x85])
    );
    assert_eq!(b(&mut e, Op::GetDcsCode), with(&[0x1B, 0x02]));
    assert_eq!(
        b(&mut e, Op::SetDcsCode(123)),
        with(&[0x1B, 0x02, 0x00, 0x01, 0x23])
    );
    assert_eq!(
        b(&mut e, Op::SetToneMode(ToneMode::Tsql)),
        [
            with(&[0x16, 0x42, 0x00]),
            with(&[0x16, 0x43, 0x01]),
            with(&[0x16, 0x4B, 0x00])
        ]
        .concat()
    );
    assert_eq!(
        b(&mut e, Op::GetToneMode),
        [
            with(&[0x16, 0x42]),
            with(&[0x16, 0x43]),
            with(&[0x16, 0x4B])
        ]
        .concat()
    );
    assert_eq!(
        e.encode(&Op::SetRepeaterOffset(Hz(600_050)))
            .expect_err("not a 100 Hz step")
            .kind,
        ErrorKind::Unsupported
    );
    assert!(e.encode(&Op::SetDcsCode(10_000)).is_err());
}

#[test]
fn a_rig_without_the_capability_refuses_every_repeater_op() {
    let mut d = shipped("icom-ic-7000");
    d.capabilities = radioctl_core::capability::CapabilitySet::of(&[Capability::ReadFrequency]);
    let mut e = CivEngine::new(d).expect("engine");
    for op in [
        Op::GetRepeaterShift,
        Op::SetRepeaterShift(RepeaterShift::Plus),
        Op::GetToneMode,
        Op::SetCtcssTone(885),
    ] {
        assert_eq!(
            e.encode(&op).expect_err("refused").kind,
            ErrorKind::Unsupported,
            "{op:?}"
        );
    }
}

// Tests [@ANCHOR: radioctl_civ_engine_decode]
#[test]
fn split_and_shift_reads_do_not_answer_each_other_on_the_virtual_icom() {
    let desc = repeater_rigs().into_iter().next().expect("a repeater rig");
    let mut engine = CivEngine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    let mut ask = |op: Op| exchange(&mut engine, &mut rig, op);
    assert_eq!(
        ask(Op::SetRepeaterShift(RepeaterShift::Minus)),
        vec![Inbound::Ack]
    );
    // The rig has one 0F answer, the shift code: the split read is a clean Nak, not a wrong value.
    assert_eq!(ask(Op::GetSplit), vec![Inbound::Nak]);
    assert_eq!(
        value(ask(Op::GetRepeaterShift)),
        Observation::RepeaterShift(RepeaterShift::Minus)
    );
    assert_eq!(ask(Op::SetSplit(true)), vec![Inbound::Ack]);
    assert_eq!(value(ask(Op::GetSplit)), Observation::Split(true));
    assert_eq!(ask(Op::GetRepeaterShift), vec![Inbound::Nak]);
}

// Tests [@ANCHOR: radioctl_civ_engine_decode]
// Tests [@ANCHOR: radioctl_virtual_tone_flag_index]
// Tests [@ANCHOR: radioctl_civ_engine_encode_repeater]
// Tests [@ANCHOR: radioctl_civ_engine_decode_tone_flag]
#[test]
fn every_icom_repeater_rig_round_trips_against_its_virtual_twin() {
    let rigs = repeater_rigs();
    assert!(
        rigs.len() >= 20,
        "the repeater capability is listed on the Hamlib models"
    );
    for desc in rigs {
        let id = desc.id.clone();
        let mut engine = CivEngine::new(desc.clone()).expect("engine");
        let mut rig = VirtualRig::from_description(&desc).expect("rig");
        let mut ask = |op: Op| exchange(&mut engine, &mut rig, op);

        assert_eq!(
            ask(Op::SetRepeaterShift(RepeaterShift::Minus)),
            vec![Inbound::Ack],
            "{id}"
        );
        assert_eq!(
            value(ask(Op::GetRepeaterShift)),
            Observation::RepeaterShift(RepeaterShift::Minus),
            "{id}"
        );
        assert_eq!(
            ask(Op::SetRepeaterShift(RepeaterShift::Plus)),
            vec![Inbound::Ack],
            "{id}"
        );
        assert_eq!(
            value(ask(Op::GetRepeaterShift)),
            Observation::RepeaterShift(RepeaterShift::Plus),
            "{id}"
        );
        assert_eq!(
            ask(Op::SetRepeaterOffset(Hz(5_000_000))),
            vec![Inbound::Ack],
            "{id}"
        );
        assert_eq!(
            value(ask(Op::GetRepeaterOffset)),
            Observation::RepeaterOffset(Hz(5_000_000)),
            "{id}"
        );
        assert_eq!(ask(Op::SetCtcssTone(1318)), vec![Inbound::Ack], "{id}");
        assert_eq!(
            value(ask(Op::GetCtcssTone)),
            Observation::CtcssTone(1318),
            "{id}"
        );
        assert_eq!(ask(Op::SetDcsCode(754)), vec![Inbound::Ack], "{id}");
        assert_eq!(
            value(ask(Op::GetDcsCode)),
            Observation::DcsCode(754),
            "{id}"
        );
        for mode in [ToneMode::Tone, ToneMode::Tsql, ToneMode::Dcs, ToneMode::Off] {
            assert_eq!(
                ask(Op::SetToneMode(mode)),
                vec![Inbound::Ack],
                "{id} {mode:?}"
            );
            assert_eq!(
                value(ask(Op::GetToneMode)),
                Observation::ToneMode(mode),
                "{id} {mode:?}"
            );
        }
    }
}

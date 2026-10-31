// SPDX-License-Identifier: AGPL-3.0-or-later
//! The receive attenuator and preamplifier through every shipped engine and its virtual rig:
//! every step the description lists round-trips, every other value is refused by the engine, and a
//! model without a `[levels]` block refuses both levels.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Duration;

use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine};
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::Level;
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::virtual_rig::VirtualRig;

const NOW: Duration = Duration::from_secs(5);

fn shipped() -> Vec<RigDescription> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("rigs directory") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "toml") {
            let text = std::fs::read_to_string(&path).expect("read");
            out.push(rigfile::parse(&text).expect("loads").description);
        }
    }
    out
}

fn has_virtual_rig(desc: &RigDescription) -> bool {
    !matches!(desc.family, Family::Flex | Family::Hpsdr)
}

fn set_and_read(
    engine: &mut dyn ProtocolEngine,
    rig: &mut VirtualRig,
    level: Level,
    value: u16,
) -> Option<u16> {
    let set = engine
        .encode(&Op::SetLevel(level, value))
        .expect("encode set");
    let answer = rig.feed(&set.bytes, NOW);
    let acked = engine.feed(&answer, NOW);
    assert!(
        acked.is_empty() || acked == vec![Inbound::Ack],
        "set answered {acked:?}"
    );
    let get = engine.encode(&Op::GetLevel(level)).expect("encode get");
    let answer = rig.feed(&get.bytes, NOW);
    match engine.feed(&answer, NOW).as_slice() {
        [Inbound::Observation(Observation::Level(got, v))] if *got == level => Some(*v),
        _ => None,
    }
}

// Tests [@ANCHOR: radioctl_civ_engine_encode_front_end]
// Tests [@ANCHOR: radioctl_civ_decode_bcd_byte]
// Tests [@ANCHOR: radioctl_virtual_civ_front_end_get]
// Tests [@ANCHOR: radioctl_virtual_civ_front_end_set]
// Tests [@ANCHOR: radioctl_kenwood_engine_front_end_set]
// Tests [@ANCHOR: radioctl_kenwood_engine_decode_front_end]
// Tests [@ANCHOR: radioctl_yaesu_engine_decode_front_end]
#[test]
fn every_listed_step_round_trips_and_nothing_else_is_accepted() {
    let mut checked = 0;
    for desc in shipped().into_iter().filter(has_virtual_rig) {
        if desc.levels.max_attenuation_db().is_none() && desc.levels.preamp_steps == 0 {
            continue;
        }
        let mut engine = engine_for(&desc).expect("engine");
        let mut rig = VirtualRig::from_description(&desc).expect("rig");
        // A model may have only one of the two controls (the IC-R9000 has an attenuator only).
        let has_att = desc.levels.max_attenuation_db().is_some();
        for db in std::iter::once(0)
            .chain(desc.levels.attenuator_db.iter().copied().map(u16::from))
            .filter(|_| has_att)
        {
            assert_eq!(
                set_and_read(engine.as_mut(), &mut rig, Level::Attenuator, db),
                Some(db),
                "{} attenuator {db}",
                desc.id
            );
            checked += 1;
        }
        for step in
            (0..=u16::from(desc.levels.preamp_steps)).filter(|_| desc.levels.preamp_steps > 0)
        {
            assert_eq!(
                set_and_read(engine.as_mut(), &mut rig, Level::Preamp, step),
                Some(step),
                "{} preamp {step}",
                desc.id
            );
            checked += 1;
        }
        // Values the description does not list never reach the wire.
        let past = u16::from(desc.levels.preamp_steps) + 1;
        let err = engine
            .encode(&Op::SetLevel(Level::Preamp, past))
            .expect_err("preamp step beyond the list");
        assert_eq!(err.kind, ErrorKind::Unsupported, "{}", desc.id);
        let odd = engine.encode(&Op::SetLevel(Level::Attenuator, 7));
        assert!(odd.is_err(), "{} takes 7 dB", desc.id);
    }
    assert!(checked >= 30, "only {checked} cases ran");
}

#[test]
fn a_model_without_a_levels_block_refuses_both_levels() {
    let mut refused = 0;
    for desc in shipped() {
        if desc.levels.max_attenuation_db().is_some() || desc.levels.preamp_steps > 0 {
            continue;
        }
        let mut engine = engine_for(&desc).expect("engine");
        for op in [
            Op::GetLevel(Level::Attenuator),
            Op::SetLevel(Level::Attenuator, 0),
            Op::GetLevel(Level::Preamp),
            Op::SetLevel(Level::Preamp, 0),
        ] {
            assert!(engine.encode(&op).is_err(), "{} accepted {op:?}", desc.id);
            refused += 1;
        }
    }
    assert!(refused >= 40, "only {refused} refusals checked");
}

#[test]
fn a_virtual_rig_answers_an_unlisted_front_end_command_with_its_refusal() {
    let desc = RigDescription::ic7300();
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    let reply = rig.feed(&[0xFE, 0xFE, 0x94, 0xE0, 0x11, 0xFD], NOW);
    assert_eq!(reply, vec![0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD]);
    let reply = rig.feed(&[0xFE, 0xFE, 0x94, 0xE0, 0x16, 0x02, 0xFD], NOW);
    assert_eq!(reply, vec![0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD]);
}

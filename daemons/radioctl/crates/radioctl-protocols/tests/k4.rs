// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Elecraft K4 forms of the RIT/XIT offset and the S-meter, from the K4 Programmer's Reference
//! (rev. D12), against the virtual K4 built from `rigs/elecraft-k4.toml`. No radio is connected.
#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use radioctl_core::capability::Capability;
use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine};
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::virtual_rig::VirtualRig;

const NOW: Duration = Duration::from_secs(3);

fn description() -> RigDescription {
    rigfile::parse(include_str!("../../../rigs/elecraft-k4.toml"))
        .expect("loads")
        .description
}

fn exchange(engine: &mut ElecraftEngine, rig: &mut VirtualRig, op: Op) -> Vec<Inbound> {
    let request = engine.encode(&op).expect("encode");
    let answer = rig.feed(&request.bytes, NOW);
    engine.feed(&answer, NOW)
}

fn seen(inbound: Vec<Inbound>, expected: Observation) {
    let all: Vec<Observation> = inbound
        .into_iter()
        .filter_map(|i| match i {
            Inbound::Observation(o) => Some(o),
            _ => None,
        })
        .collect();
    assert!(all.contains(&expected), "{expected:?} not in {all:?}");
}

// Tests [@ANCHOR: radioctl_elecraft_engine_encode]
// Tests [@ANCHOR: radioctl_elecraft_engine_feed]
// Tests [@ANCHOR: radioctl_virtual_elecraft_command]
#[test]
fn the_k4_rit_offset_is_rc_then_ru_or_rd_and_is_read_from_if() {
    // RC clears the offset; RU/RD n (RU$nnnnn, 000001-09999 Hz) change it; there is no RO (D12:
    // "RC$", "RD$", "RU$"); the offset is read from IF (the K3-compatible IF, D12 "IF").
    let desc = description();
    assert!(desc.capabilities.has(Capability::RitXit));
    assert!(desc.capabilities.has(Capability::ReadSignal));
    let mut engine = ElecraftEngine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    let wire = |e: &mut ElecraftEngine, op: Op| e.encode(&op).expect("encodes").bytes;
    assert_eq!(wire(&mut engine, Op::SetRitOffset(150)), b"RC;RU00150;");
    assert_eq!(wire(&mut engine, Op::SetRitOffset(-50)), b"RC;RD00050;");
    assert_eq!(wire(&mut engine, Op::SetRitOffset(0)), b"RC;");
    assert_eq!(wire(&mut engine, Op::GetRitOffset), b"IF;");
    assert_eq!(wire(&mut engine, Op::SetRit(true)), b"RT1;");
    assert_eq!(wire(&mut engine, Op::SetXit(false)), b"XT0;");
    assert!(engine.encode(&Op::SetRitOffset(10_000)).is_err());

    exchange(&mut engine, &mut rig, Op::SetRitOffset(-200));
    seen(
        exchange(&mut engine, &mut rig, Op::GetRitOffset),
        Observation::RitOffset(-200),
    );
    exchange(&mut engine, &mut rig, Op::SetRitOffset(1234));
    seen(
        exchange(&mut engine, &mut rig, Op::GetRitOffset),
        Observation::RitOffset(1234),
    );
    exchange(&mut engine, &mut rig, Op::SetRit(true));
    seen(
        exchange(&mut engine, &mut rig, Op::GetRit),
        Observation::Rit(true),
    );
    exchange(&mut engine, &mut rig, Op::SetXit(true));
    seen(
        exchange(&mut engine, &mut rig, Op::GetXit),
        Observation::Xit(true),
    );
    // The K4 has no RO; the virtual K4 refuses it.
    assert_eq!(rig.feed(b"RO+0050;", NOW), b"?;");
    assert_eq!(rig.feed(b"RU00000;", NOW), b"?;");
}

#[test]
fn the_k4_s_meter_is_the_k3_compatible_sm_in_k40_mode() {
    // D12 "SM$": without K41 (K4 meta mode) the answer is SMnnnn, 0000-0015 in K30.
    let desc = description();
    let mut engine = ElecraftEngine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    assert_eq!(
        engine.encode(&Op::GetSignal).expect("encodes").bytes,
        b"SM;"
    );
    assert!(matches!(
        exchange(&mut engine, &mut rig, Op::GetSignal).as_slice(),
        [Inbound::Observation(Observation::Signal(_))]
    ));
}

// Tests [@ANCHOR: radioctl_elecraft_engine_encode]
// Tests [@ANCHOR: radioctl_elecraft_engine_feed]
// Tests [@ANCHOR: radioctl_elecraft_engine_link_restored]
// Tests [@ANCHOR: radioctl_elecraft_k4_front_end]
// Tests [@ANCHOR: radioctl_virtual_elecraft_command]
#[test]
fn the_k4_attenuator_and_preamp_use_k41_and_put_the_meta_mode_back() {
    use radioctl_core::units::Level;
    // RA$nnm and PA$nm exist in K41 (K4 meta) mode only (PDF pp. 25 and 27; K41 list p. 18), so
    // each request switches to K41, does the command and restores the mode the rig was in.
    let desc = description();
    let mut engine = ElecraftEngine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    let wire = |e: &mut ElecraftEngine, op: Op| e.encode(&op).expect("encodes").bytes;
    assert_eq!(
        wire(&mut engine, Op::SetLevel(Level::Attenuator, 12)),
        b"K41;RA121;K40;"
    );
    assert_eq!(
        wire(&mut engine, Op::SetLevel(Level::Attenuator, 0)),
        b"K41;RA000;K40;"
    );
    assert_eq!(
        wire(&mut engine, Op::SetLevel(Level::Preamp, 2)),
        b"K41;PA21;K40;"
    );
    assert_eq!(
        wire(&mut engine, Op::GetLevel(Level::Preamp)),
        b"K41;PA;K40;"
    );
    assert!(engine.encode(&Op::SetLevel(Level::Attenuator, 5)).is_err());
    assert!(engine.encode(&Op::SetLevel(Level::Preamp, 4)).is_err());

    // Round trip against the virtual K4, in K40: the mode is K40 again afterwards.
    exchange(&mut engine, &mut rig, Op::SetLevel(Level::Attenuator, 12));
    assert_eq!(rig.feed(b"K4;", NOW), b"K40;");
    seen(
        exchange(&mut engine, &mut rig, Op::GetLevel(Level::Attenuator)),
        Observation::Level(Level::Attenuator, 12),
    );
    exchange(&mut engine, &mut rig, Op::SetLevel(Level::Preamp, 3));
    seen(
        exchange(&mut engine, &mut rig, Op::GetLevel(Level::Preamp)),
        Observation::Level(Level::Preamp, 3),
    );
    exchange(&mut engine, &mut rig, Op::SetLevel(Level::Attenuator, 0));
    seen(
        exchange(&mut engine, &mut rig, Op::GetLevel(Level::Attenuator)),
        Observation::Level(Level::Attenuator, 0),
    );
    // Outside K41 the rig has no such forms.
    assert_eq!(rig.feed(b"RA;", NOW), b"?;");

    // A rig that was in K41 is put back in K41: the engine learns the mode from `K4;`.
    rig.feed(b"K41;", NOW);
    let probe = engine.link_restored();
    assert_eq!(probe.last().map(|r| r.bytes.clone()), Some(b"K4;".to_vec()));
    let answer = rig.feed(b"K4;", NOW);
    assert_eq!(answer, b"K41;");
    engine.feed(&answer, NOW);
    assert_eq!(
        wire(&mut engine, Op::GetLevel(Level::Preamp)),
        b"K41;PA;K41;"
    );
    exchange(&mut engine, &mut rig, Op::SetLevel(Level::Preamp, 1));
    assert_eq!(rig.feed(b"K4;", NOW), b"K41;");
}

#[test]
fn a_k41_id_text_is_not_a_model_number() {
    // In K41 the ID answer is text (PDF p. 16), not `ID017;`: it must not decode as a model id.
    let mut engine = ElecraftEngine::new(description()).expect("engine");
    for bad in [&b"IDWA1ABC;"[..], b"ID0;"] {
        assert!(
            matches!(engine.feed(bad, NOW).as_slice(), [Inbound::Unrecognised(_)]),
            "{bad:?}"
        );
    }
}

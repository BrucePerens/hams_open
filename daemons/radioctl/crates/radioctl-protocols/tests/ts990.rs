// SPDX-License-Identifier: AGPL-3.0-or-later
//! The TS-990S dialect (`kenwood.dialect = "ts990"`): the classic engine against a virtual TS-990S
//! built from `rigs/kenwood-ts-990s.toml` (Kenwood's PC command reference, March 2013). Nothing is
//! keyed and no radio or LAN connection is involved.
#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use radioctl_core::capability::Capability;
use radioctl_core::description::{KenwoodDialect, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine};
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Level, Mode, Vfo};
use radioctl_protocols::kenwood::KenwoodEngine;
use radioctl_protocols::virtual_rig::VirtualRig;

const NOW: Duration = Duration::from_secs(3);

fn description() -> RigDescription {
    rigfile::parse(include_str!("../../../rigs/kenwood-ts-990s.toml"))
        .expect("loads")
        .description
}

fn exchange(engine: &mut KenwoodEngine, rig: &mut VirtualRig, op: Op) -> Vec<Inbound> {
    let request = engine.encode(&op).expect("encode");
    let answer = rig.feed(&request.bytes, NOW);
    engine.feed(&answer, NOW)
}

fn assert_seen(inbound: Vec<Inbound>, expected: Observation) {
    let seen: Vec<Observation> = inbound
        .into_iter()
        .map(|i| match i {
            Inbound::Observation(o) => o,
            other => panic!("expected an observation, got {other:?}"),
        })
        .collect();
    assert!(seen.contains(&expected), "{expected:?} not in {seen:?}");
}

// Tests [@ANCHOR: radioctl_kenwood_engine_encode_ts990]
// Tests [@ANCHOR: radioctl_kenwood_engine_encode]
// Tests [@ANCHOR: radioctl_kenwood_engine_decode]
// Tests [@ANCHOR: radioctl_kenwood_engine_decode_if]
#[test]
fn the_wire_forms_are_those_of_the_ts990s_reference() {
    let desc = description();
    assert_eq!(
        desc.kenwood.as_ref().map(|k| k.dialect),
        Some(KenwoodDialect::Ts990)
    );
    let mut engine = KenwoodEngine::new(desc).expect("engine");
    let wire = |e: &mut KenwoodEngine, op: Op| e.encode(&op).expect("encodes").bytes;
    // The mode is OM0 (PDF p. 45), there is no MD; the main band is FA; the meter SM0; AI2 is on.
    assert_eq!(wire(&mut engine, Op::GetMode), b"OM0;");
    assert_eq!(wire(&mut engine, Op::SetMode(Mode::Usb)), b"OM02;");
    assert_eq!(wire(&mut engine, Op::SetMode(Mode::Lsb)), b"OM01;");
    assert_eq!(wire(&mut engine, Op::SetMode(Mode::Data)), b"OM0D;");
    assert_eq!(
        wire(&mut engine, Op::SetFrequency(Vfo::A, Hz(14_195_000))),
        b"FA00014195000;"
    );
    assert_eq!(wire(&mut engine, Op::GetSignal), b"SM0;");
    assert_eq!(wire(&mut engine, Op::SetTransceive(true)), b"AI2;");
    assert_eq!(wire(&mut engine, Op::GetModelId), b"ID;");
    assert_eq!(wire(&mut engine, Op::SetPtt(true)), b"TX;");
    assert_eq!(wire(&mut engine, Op::SetRitOffset(150)), b"RC;RU00150;");
    // What the reference does not give in a form the engine can use is refused, not guessed.
    for op in [
        Op::GetPtt,
        Op::GetSplit,
        Op::SetSplit(true),
        Op::GetTxVfo,
        Op::GetLevel(Level::AfGain),
        Op::SetLevel(Level::MicGain, 100),
        Op::StopCw,
    ] {
        assert_eq!(
            engine.encode(&op).expect_err("not in this dialect").kind,
            ErrorKind::Unsupported,
            "{op:?}"
        );
    }
    // Answers: OM0<m>, and an IF whose P8 and P10 are not read. The sub band is not a VFO B.
    assert_seen(engine.feed(b"OM02;", NOW), Observation::Mode(Mode::Usb));
    for bad in [&b"MD2;"[..], b"FR0;", b"AG0128;", b"OM0Z;", b"OM2;"] {
        assert!(
            matches!(engine.feed(bad, NOW).as_slice(), [Inbound::Unrecognised(_)]),
            "{bad:?}"
        );
    }
    let answer = [
        "IF",
        "00014195000",
        "     ",
        "-0120",
        "1",
        "0",
        "000",
        "0",
        "2",
        "0",
        "0",
        "0",
        "0000",
        ";",
    ]
    .concat();
    assert_eq!(answer.len(), 38);
    let seen = engine.feed(answer.as_bytes(), NOW);
    assert_eq!(
        seen,
        vec![
            Inbound::Observation(Observation::Frequency(Vfo::A, Hz(14_195_000))),
            Inbound::Observation(Observation::Mode(Mode::Usb)),
            Inbound::Observation(Observation::Rit(true)),
            Inbound::Observation(Observation::Xit(false)),
            Inbound::Observation(Observation::RitOffset(-120)),
        ]
    );
}

// Tests [@ANCHOR: radioctl_kenwood_engine_encode_ts990]
// Tests [@ANCHOR: radioctl_kenwood_engine_decode_if]
// Tests [@ANCHOR: radioctl_virtual_ts990_is]
// Tests [@ANCHOR: radioctl_virtual_ts990_command]
// Tests [@ANCHOR: radioctl_virtual_kenwood_if]
#[test]
fn the_engine_and_the_virtual_ts990s_agree() {
    let desc = description();
    assert!(!desc.capabilities.has(Capability::ReadPtt));
    assert!(!desc.capabilities.has(Capability::Split));
    assert!(!desc.capabilities.has(Capability::Levels));
    let mut engine = KenwoodEngine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetModelId),
        Observation::ModelId(22),
    );
    exchange(
        &mut engine,
        &mut rig,
        Op::SetFrequency(Vfo::A, Hz(7_074_000)),
    );
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetFrequency(Vfo::A)),
        Observation::Frequency(Vfo::A, Hz(7_074_000)),
    );
    exchange(&mut engine, &mut rig, Op::SetMode(Mode::Cw));
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetMode),
        Observation::Mode(Mode::Cw),
    );
    exchange(&mut engine, &mut rig, Op::SetRit(true));
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetRit),
        Observation::Rit(true),
    );
    exchange(&mut engine, &mut rig, Op::SetRitOffset(-200));
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetRitOffset),
        Observation::RitOffset(-200),
    );
    exchange(&mut engine, &mut rig, Op::SetXit(true));
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetXit),
        Observation::Xit(true),
    );
    assert!(matches!(
        exchange(&mut engine, &mut rig, Op::GetSignal).as_slice(),
        [Inbound::Observation(Observation::Signal(_))]
    ));
    // Keying changes nothing a read can see: P8 of IF is always 0 on this model.
    exchange(&mut engine, &mut rig, Op::SetPtt(true));
    assert!(rig.state().ptt);
    let answer = rig.feed(b"IF;", NOW);
    assert_eq!(answer.get(28), Some(&b'0'), "{answer:?}");
    exchange(&mut engine, &mut rig, Op::SetPtt(false));
    assert!(!rig.state().ptt);
    // The forms the TS-990S does not have are refused by the virtual rig.
    for bad in [&b"MD;"[..], b"FR0;", b"FT1;", b"AG0;", b"RA;", b"SP1;"] {
        assert_eq!(rig.feed(bad, NOW), b"?;", "{bad:?}");
    }
}

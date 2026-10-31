// SPDX-License-Identifier: AGPL-3.0-or-later
//! The older Kenwood dialect (`kenwood.dialect = "ts440"`): the classic engine against a virtual
//! TS-440S built from `rigs/kenwood-ts-440s.toml` (Kenwood IC-10 manual). Nothing is keyed.
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
    rigfile::parse(include_str!("../../../rigs/kenwood-ts-440s.toml"))
        .expect("loads")
        .description
}

fn exchange(engine: &mut KenwoodEngine, rig: &mut VirtualRig, op: Op) -> Vec<Inbound> {
    let request = engine.encode(&op).expect("encode");
    let answer = rig.feed(&request.bytes, NOW);
    engine.feed(&answer, NOW)
}

/// The observations in an answer; an `IF` answer carries several, the engine hands all of them on.
fn observed(inbound: Vec<Inbound>) -> Vec<Observation> {
    inbound
        .into_iter()
        .map(|i| match i {
            Inbound::Observation(o) => o,
            other => panic!("expected an observation, got {other:?}"),
        })
        .collect()
}

fn assert_seen(inbound: Vec<Inbound>, expected: Observation) {
    let seen = observed(inbound);
    assert!(seen.contains(&expected), "{expected:?} not in {seen:?}");
}

// Tests [@ANCHOR: radioctl_kenwood_engine_encode_ts440]
// Tests [@ANCHOR: radioctl_kenwood_engine_encode]
// Tests [@ANCHOR: radioctl_kenwood_engine_decode]
// Tests [@ANCHOR: radioctl_kenwood_engine_decode_if]
// Tests [@ANCHOR: radioctl_virtual_ts440_is]
// Tests [@ANCHOR: radioctl_virtual_ts440_command]
// Tests [@ANCHOR: radioctl_virtual_kenwood_if]
#[test]
fn the_wire_forms_are_those_of_the_ic10_manual() {
    let desc = description();
    assert_eq!(
        desc.kenwood.as_ref().map(|k| k.dialect),
        Some(KenwoodDialect::Ts440)
    );
    let mut engine = KenwoodEngine::new(desc.clone()).expect("engine");
    let wire = |e: &mut KenwoodEngine, op: Op| e.encode(&op).expect("encodes").bytes;
    // The mode is read through IF (MD has no read form), and set with MD.
    assert_eq!(wire(&mut engine, Op::GetMode), b"IF;");
    assert_eq!(wire(&mut engine, Op::SetMode(Mode::Usb)), b"MD2;");
    // FN and SP replace FR and FT; AI has one digit; RU and RD are not used for an offset.
    assert_eq!(wire(&mut engine, Op::GetTxVfo), b"IF;");
    assert_eq!(wire(&mut engine, Op::SetSplit(true)), b"FN0;SP1;");
    assert_eq!(wire(&mut engine, Op::SetSplit(false)), b"FN0;SP0;");
    assert_eq!(wire(&mut engine, Op::SetTransceive(true)), b"AI1;");
    assert_eq!(wire(&mut engine, Op::SetTransceive(false)), b"AI0;");
    assert_eq!(wire(&mut engine, Op::GetModelId), b"ID;");
    for op in [
        Op::SetRitOffset(100),
        Op::GetSignal,
        Op::GetLevel(Level::AfGain),
        Op::SetLevel(Level::AfGain, 10),
        Op::StopCw,
    ] {
        assert_eq!(
            engine.encode(&op).expect_err("not in this dialect").kind,
            ErrorKind::Unsupported,
            "{op:?}"
        );
    }
    // Answers the dialect never gives are not accepted.
    for bad in [&b"MD2;"[..], b"FR0;", b"FT1;", b"SM00012;", b"AG0128;"] {
        assert!(
            matches!(engine.feed(bad, NOW).as_slice(), [Inbound::Unrecognised(_)]),
            "{bad:?}"
        );
    }
    // The IF layout is the classic one: 38 characters, P3 a sign and four digits.
    let answer = [
        "IF",
        "00007074000",
        "00000",
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
    let seen = observed(engine.feed(answer.as_bytes(), NOW));
    assert!(seen.contains(&Observation::Frequency(Vfo::A, Hz(7_074_000))));
    assert!(seen.contains(&Observation::RitOffset(-120)));
    assert!(seen.contains(&Observation::Rit(true)));
    assert!(seen.contains(&Observation::Mode(Mode::Usb)));
    assert!(seen.contains(&Observation::Split(false)));
}

// Tests [@ANCHOR: radioctl_kenwood_engine_encode_ts440]
// Tests [@ANCHOR: radioctl_kenwood_engine_decode_if]
// Tests [@ANCHOR: radioctl_virtual_ts440_command]
// Tests [@ANCHOR: radioctl_virtual_ts440_is_fn_sp]
// Tests [@ANCHOR: radioctl_virtual_kenwood_if]
#[test]
fn the_engine_and_the_virtual_ts440_agree() {
    let desc = description();
    let mut engine = KenwoodEngine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetModelId),
        Observation::ModelId(4),
    );
    exchange(
        &mut engine,
        &mut rig,
        Op::SetFrequency(Vfo::A, Hz(7_040_000)),
    );
    exchange(
        &mut engine,
        &mut rig,
        Op::SetFrequency(Vfo::B, Hz(14_200_000)),
    );
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetFrequency(Vfo::B)),
        Observation::Frequency(Vfo::B, Hz(14_200_000)),
    );
    exchange(&mut engine, &mut rig, Op::SetMode(Mode::Cw));
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetMode),
        Observation::Mode(Mode::Cw),
    );
    exchange(&mut engine, &mut rig, Op::SetSplit(true));
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetSplit),
        Observation::Split(true),
    );
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetTxVfo),
        Observation::TxVfo(Vfo::B),
    );
    exchange(&mut engine, &mut rig, Op::SetSplit(false));
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetTxVfo),
        Observation::TxVfo(Vfo::A),
    );
    exchange(&mut engine, &mut rig, Op::SetRit(true));
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetRit),
        Observation::Rit(true),
    );
    exchange(&mut engine, &mut rig, Op::SetXit(true));
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetXit),
        Observation::Xit(true),
    );
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetRitOffset),
        Observation::RitOffset(0),
    );
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetPtt),
        Observation::Ptt(false),
    );
    // The rig refuses what the dialect does not have (a read of MD, an FR, a level).
    for bad in [&b"MD;"[..], b"FR0;", b"AG0;", b"SM0;", b"RU00010;"] {
        assert_eq!(rig.feed(bad, NOW), b"?;", "{bad:?}");
    }
    // RU and RD take no parameter and move the RIT; RC clears it.
    assert!(rig.feed(b"RU;RU;", NOW).is_empty());
    assert_seen(
        exchange(&mut engine, &mut rig, Op::GetRitOffset),
        Observation::RitOffset(20),
    );
    assert!(rig.feed(b"RC;", NOW).is_empty());
}

fn shipped(id: &str) -> RigDescription {
    let text = std::fs::read_to_string(format!(
        "{}/../../rigs/{id}.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("rig file");
    rigfile::parse(&text).expect("loads").description
}

// Tests [@ANCHOR: radioctl_kenwood_engine_encode_ts440]
// Tests [@ANCHOR: radioctl_kenwood_engine_decode]
// Tests [@ANCHOR: radioctl_virtual_ts440_command]
// Tests [@ANCHOR: radioctl_virtual_ts440_is_fn_sp]
// Tests [@ANCHOR: radioctl_virtual_kenwood_select_vfo]
#[test]
fn the_ts450_dialect_splits_with_fr_and_ft_and_reads_the_mode_from_if() {
    for (id, model) in [("kenwood-ts-450s", 10), ("kenwood-ts-690s", 11)] {
        let desc = shipped(id);
        assert_eq!(
            desc.kenwood.as_ref().map(|k| k.dialect),
            Some(KenwoodDialect::Ts450),
            "{id}"
        );
        let mut engine = KenwoodEngine::new(desc.clone()).expect("engine");
        let wire = |e: &mut KenwoodEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        // MD and FT have no read form in the manual (pages 13 and 10): both come from IF.
        assert_eq!(wire(&mut engine, Op::GetMode), b"IF;");
        assert_eq!(wire(&mut engine, Op::GetTxVfo), b"IF;");
        assert_eq!(wire(&mut engine, Op::SetMode(Mode::Usb)), b"MD2;");
        assert_eq!(wire(&mut engine, Op::SetSplit(true)), b"FR0;FT1;");
        assert_eq!(wire(&mut engine, Op::SetSplit(false)), b"FR0;FT0;");
        // AI has one digit (page 8); RU and RD take no parameter (page 16); SM has no receiver digit.
        assert_eq!(wire(&mut engine, Op::SetTransceive(true)), b"AI1;");
        for op in [
            Op::SetRitOffset(100),
            Op::GetSignal,
            Op::GetLevel(Level::RfGain),
        ] {
            assert_eq!(
                engine.encode(&op).expect_err("not in this dialect").kind,
                ErrorKind::Unsupported,
                "{id} {op:?}"
            );
        }
        let mut rig = VirtualRig::from_description(&desc).expect("rig");
        assert_seen(
            exchange(&mut engine, &mut rig, Op::GetModelId),
            Observation::ModelId(model),
        );
        exchange(&mut engine, &mut rig, Op::SetMode(Mode::Lsb));
        assert_seen(
            exchange(&mut engine, &mut rig, Op::GetMode),
            Observation::Mode(Mode::Lsb),
        );
        exchange(&mut engine, &mut rig, Op::SetSplit(true));
        assert_seen(
            exchange(&mut engine, &mut rig, Op::GetSplit),
            Observation::Split(true),
        );
        assert_seen(
            exchange(&mut engine, &mut rig, Op::GetTxVfo),
            Observation::TxVfo(Vfo::B),
        );
        // FN and SP are the TS-440S's, not this dialect's; FR and FT cannot be read.
        for bad in [&b"FN0;"[..], b"SP1;", b"FR;", b"FT;", b"MD;"] {
            assert_eq!(rig.feed(bad, NOW), b"?;", "{id} {bad:?}");
        }
    }
}

// Tests [@ANCHOR: radioctl_kenwood_engine_encode]
// Tests [@ANCHOR: radioctl_kenwood_level_wire]
#[test]
fn the_ts570_files_follow_their_manual_and_pin_what_the_engine_cannot_do() {
    for (id, model) in [("kenwood-ts-570d", 17), ("kenwood-ts-570s", 18)] {
        let desc = shipped(id);
        let mut engine = KenwoodEngine::new(desc.clone()).expect("engine");
        let mut rig = VirtualRig::from_description(&desc).expect("rig");
        assert_seen(
            exchange(&mut engine, &mut rig, Op::GetModelId),
            Observation::ModelId(model),
        );
        // The manual's modes 7 (CW-R) and 9 (FSK-R) are in the file.
        assert_eq!(desc.mode_code(Mode::CwR), Some(b'7'), "{id}");
        assert_eq!(desc.mode_code(Mode::RttyR), Some(b'9'), "{id}");
        // RU and RD take no parameter (printed page 79): the engine's RUnnnnn; cannot be sent, so no
        // RIT offset. AG is AGnnn with no leading 0 (the engine writes AG0nnn;), SM is SMnnnn: so no
        // levels and no signal. These pins fail if a later edit claims what the manual contradicts.
        for cap in [
            Capability::RitXit,
            Capability::Levels,
            Capability::ReadSignal,
        ] {
            assert!(!desc.capabilities.has(cap), "{id} claims {cap:?}");
        }
        assert_eq!(
            engine
                .encode(&Op::SetRitOffset(100))
                .expect_err("no rit")
                .kind,
            ErrorKind::Unsupported
        );
        assert_eq!(
            engine
                .encode(&Op::GetLevel(Level::AfGain))
                .expect_err("no levels")
                .kind,
            ErrorKind::Unsupported
        );
    }
}

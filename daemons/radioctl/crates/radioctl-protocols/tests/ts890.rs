// SPDX-License-Identifier: AGPL-3.0-or-later
//! The TS-890S dialect: the engine against the virtual rig built from `rigs/kenwood-ts-890s.toml`,
//! the LAN login frames, and the checks that keep the classic engine away from it.
#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use radioctl_core::engine::{Inbound, ProtocolEngine};
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Level, Mode, Vfo};
use radioctl_protocols::kenwood::KenwoodEngine;
use radioctl_protocols::kenwood_ts890::{lan_connect_request, lan_login, lan_reply, Ts890Engine};
use radioctl_protocols::virtual_rig::VirtualRig;

const NOW: Duration = Duration::from_secs(3);

fn description() -> radioctl_core::description::RigDescription {
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

// Tests [@ANCHOR: radioctl_ts890_engine_new]
// Tests [@ANCHOR: radioctl_ts890_engine_encode]
// Tests [@ANCHOR: radioctl_ts890_engine_feed]
// Tests [@ANCHOR: radioctl_ts890_engine_decode]
// Tests [@ANCHOR: radioctl_ts890_engine_decode_step]
// Tests [@ANCHOR: radioctl_ts890_vfo_of]
// Tests [@ANCHOR: radioctl_ts890_decode_offset]
// Tests [@ANCHOR: radioctl_ts890_decode_level]
// Tests [@ANCHOR: radioctl_ts890_level_wire]
// Tests [@ANCHOR: radioctl_virtual_ts890_is]
// Tests [@ANCHOR: radioctl_virtual_ts890_command]
// Tests [@ANCHOR: radioctl_virtual_ts890_level]
#[test]
fn the_engine_and_the_virtual_ts890_agree_on_every_operation() {
    let desc = description();
    let mut engine = Ts890Engine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");

    assert_eq!(
        observed(exchange(&mut engine, &mut rig, Op::GetModelId)),
        Observation::ModelId(24)
    );
    exchange(
        &mut engine,
        &mut rig,
        Op::SetFrequency(Vfo::B, Hz(3_573_000)),
    );
    assert_eq!(
        observed(exchange(&mut engine, &mut rig, Op::GetFrequency(Vfo::B))),
        Observation::Frequency(Vfo::B, Hz(3_573_000))
    );
    for mode in [
        Mode::Lsb,
        Mode::Cw,
        Mode::CwR,
        Mode::Rtty,
        Mode::RttyR,
        Mode::Data,
        Mode::DataR,
        Mode::Fm,
        Mode::Am,
    ] {
        exchange(&mut engine, &mut rig, Op::SetMode(mode));
        assert_eq!(
            observed(exchange(&mut engine, &mut rig, Op::GetMode)),
            Observation::Mode(mode)
        );
    }
    // Split is TB; the transmit VFO follows it.
    exchange(&mut engine, &mut rig, Op::SetSplit(true));
    assert_eq!(
        observed(exchange(&mut engine, &mut rig, Op::GetSplit)),
        Observation::Split(true)
    );
    assert_eq!(
        observed(exchange(&mut engine, &mut rig, Op::GetTxVfo)),
        Observation::TxVfo(Vfo::B)
    );
    exchange(&mut engine, &mut rig, Op::SetRit(true));
    exchange(&mut engine, &mut rig, Op::SetXit(true));
    assert_eq!(
        observed(exchange(&mut engine, &mut rig, Op::GetRit)),
        Observation::Rit(true)
    );
    assert_eq!(
        observed(exchange(&mut engine, &mut rig, Op::GetXit)),
        Observation::Xit(true)
    );
    for hz in [250, -9_999, 0] {
        exchange(&mut engine, &mut rig, Op::SetRitOffset(hz));
        assert_eq!(
            observed(exchange(&mut engine, &mut rig, Op::GetRitOffset)),
            Observation::RitOffset(hz)
        );
    }
    for (level, value) in [
        (Level::AfGain, 255),
        (Level::RfGain, 0),
        (Level::MicGain, 100),
        (Level::KeyerSpeed, 4),
        (Level::RfPower, 5),
        (Level::Attenuator, 18),
        (Level::Preamp, 1),
    ] {
        exchange(&mut engine, &mut rig, Op::SetLevel(level, value));
        assert_eq!(
            observed(exchange(&mut engine, &mut rig, Op::GetLevel(level))),
            Observation::Level(level, value),
            "{level:?}"
        );
    }
    assert_eq!(
        observed(exchange(&mut engine, &mut rig, Op::GetSignal)),
        Observation::Signal(30)
    );
    exchange(&mut engine, &mut rig, Op::SetPtt(true));
    assert!(rig.state().ptt);
    exchange(&mut engine, &mut rig, Op::SetPtt(false));
    assert!(!rig.state().ptt);
}

#[test]
fn refusals_never_reach_the_wire_and_the_rig_refuses_what_it_does_not_know() {
    let desc = description();
    let mut engine = Ts890Engine::new(desc.clone()).expect("engine");
    for op in [
        Op::GetPtt,
        Op::SetLevel(Level::KeyerSpeed, 3),
        Op::SetLevel(Level::AfGain, 256),
        Op::SetLevel(Level::MicGain, 101),
        Op::SetLevel(Level::RfPower, 4),
        Op::GetLevel(Level::MonitorLevel),
        Op::SetLevel(Level::Attenuator, 7),
        Op::SetLevel(Level::Preamp, 3),
        Op::SetRitOffset(10_000),
        Op::SetFrequency(Vfo::A, Hz(100_000_000_000)),
        Op::SetMode(Mode::Wfm),
        Op::SendCw("cq".into()),
    ] {
        let err = engine.encode(&op).expect_err("refused");
        assert_eq!(err.kind, ErrorKind::Unsupported, "{op:?}");
    }
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    for bad in [
        &b"IF;"[..],
        b"MD2;",
        b"AG0128;",
        b"KS003;",
        b"RA9;",
        b"TB2;",
        b"OM0Z;",
    ] {
        assert_eq!(
            rig.feed(bad, NOW),
            b"?;",
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
    // Junk and unknown answers are reported, not guessed at.
    assert_eq!(
        engine.feed(b"FA123;ZZ9;?;RF20001;SM12;", NOW),
        vec![
            Inbound::Unrecognised(b"FA123;".to_vec()),
            Inbound::Unrecognised(b"ZZ9;".to_vec()),
            Inbound::Nak,
            Inbound::Unrecognised(b"RF20001;".to_vec()),
            Inbound::Unrecognised(b"SM12;".to_vec()),
        ]
    );
}

#[test]
fn the_classic_engine_refuses_the_ts890_and_the_ts890_engine_refuses_the_rest() {
    assert!(KenwoodEngine::new(description()).is_err());
    let classic = rigfile::parse(include_str!("../../../rigs/kenwood-ts-590sg.toml"))
        .expect("loads")
        .description;
    assert!(Ts890Engine::new(classic).is_err());
    assert!(Ts890Engine::new(radioctl_core::description::RigDescription::ic7300()).is_err());
}

// Tests [@ANCHOR: radioctl_ts890_lan_connect_request]
// Tests [@ANCHOR: radioctl_ts890_lan_login]
// Tests [@ANCHOR: radioctl_ts890_lan_reply]
#[test]
fn the_lan_frames_follow_the_guides_example() {
    assert_eq!(lan_connect_request(), b"##CN;");
    // The guide's example: administrator ID "kenwood", password "admin".
    assert_eq!(
        lan_login(true, "kenwood", "admin").expect("frame"),
        b"##ID00705kenwoodadmin;"
    );
    assert_eq!(
        lan_login(false, "ab", "c").expect("frame"),
        b"##ID10201abc;"
    );
    assert!(lan_login(true, "", "x").is_err());
    assert!(lan_login(true, "a;b", "x").is_err());
    assert!(lan_login(true, "x", &"p".repeat(33)).is_err());
    assert!(lan_login(true, "caf\u{e9}", "x").is_err());
    assert_eq!(lan_reply("##CN1;"), Some(true));
    assert_eq!(lan_reply("##CN0;"), Some(false));
    assert_eq!(lan_reply("##ID1"), Some(true));
    assert_eq!(lan_reply("##ID0;"), Some(false));
    assert_eq!(lan_reply("FA;"), None);
}

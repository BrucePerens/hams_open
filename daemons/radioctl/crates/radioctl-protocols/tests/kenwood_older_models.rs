// SPDX-License-Identifier: AGPL-3.0-or-later
//! The R-5000 (`kenwood.dialect = "r5000"`) and TRC-80 (`"trc80"`) dialects of the Kenwood engine
//! against the virtual rig. Hamlib facts only; nothing is keyed and no radio is involved.
#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use radioctl_core::engine::{Inbound, ProtocolEngine};
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Mode, Vfo};
use radioctl_protocols::kenwood::KenwoodEngine;
use radioctl_protocols::virtual_rig::VirtualRig;

const NOW: Duration = Duration::from_secs(3);

fn parts(text: &str) -> (KenwoodEngine, VirtualRig) {
    let desc = rigfile::parse(text).expect("loads").description;
    (
        KenwoodEngine::new(desc.clone()).expect("engine"),
        VirtualRig::from_description(&desc).expect("virtual rig"),
    )
}

fn observed(engine: &mut KenwoodEngine, rig: &mut VirtualRig, op: Op) -> Vec<Observation> {
    let request = engine.encode(&op).expect("encode");
    let answer = rig.feed(&request.bytes, NOW);
    engine
        .feed(&answer, NOW)
        .into_iter()
        .map(|i| match i {
            Inbound::Observation(o) => o,
            other => panic!("expected an observation, got {other:?}"),
        })
        .collect()
}

// Tests [@ANCHOR: radioctl_kenwood_engine_decode_if]
// Tests [@ANCHOR: radioctl_kenwood_engine_new]
// Tests [@ANCHOR: radioctl_virtual_ts440_is]
// Tests [@ANCHOR: radioctl_virtual_kenwood_if]
#[test]
fn the_r5000_if_answer_is_thirty_characters_between_if_and_the_semicolon() {
    let (mut engine, mut rig) = parts(include_str!("../../../rigs/kenwood-r-5000.toml"));
    let request = engine.encode(&Op::GetMode).expect("encode");
    assert_eq!(request.bytes, b"IF;");
    let answer = rig.feed(&request.bytes, NOW);
    // IF, 30 characters, semicolon.
    assert_eq!(
        answer.len(),
        2 + 30 + 1,
        "{}",
        String::from_utf8_lossy(&answer)
    );
    let seen = observed(&mut engine, &mut rig, Op::GetMode);
    assert!(seen.contains(&Observation::Mode(Mode::Usb)), "{seen:?}");
    // Set frequency and mode, read them back.
    let set = engine
        .encode(&Op::SetFrequency(Vfo::A, Hz(7_074_000)))
        .expect("encode");
    assert_eq!(set.bytes, b"FA00007074000;");
    assert!(rig.feed(&set.bytes, NOW).is_empty());
    let set = engine.encode(&Op::SetMode(Mode::Cw)).expect("encode");
    assert_eq!(set.bytes, b"MD3;");
    assert!(rig.feed(&set.bytes, NOW).is_empty());
    let seen = observed(&mut engine, &mut rig, Op::GetMode);
    assert!(seen.contains(&Observation::Mode(Mode::Cw)), "{seen:?}");
    let seen = observed(&mut engine, &mut rig, Op::GetFrequency(Vfo::A));
    assert!(
        seen.contains(&Observation::Frequency(Vfo::A, Hz(7_074_000))),
        "{seen:?}"
    );
    // A 35-character (TS-440S) answer is not an R-5000 answer.
    let long = format!(
        "IF{:011}{}{};",
        7_074_000, "00000-0120", "10000000200000000"
    );
    assert!(matches!(
        engine.feed(long.as_bytes(), NOW).as_slice(),
        [Inbound::Unrecognised(_)]
    ));
}

// Tests [@ANCHOR: radioctl_kenwood_engine_encode]
// Tests [@ANCHOR: radioctl_kenwood_engine_decode_if]
#[test]
fn the_trc80_reads_its_frequency_from_if() {
    let (mut engine, mut rig) = parts(include_str!("../../../rigs/kenwood-trc-80.toml"));
    let request = engine.encode(&Op::GetFrequency(Vfo::A)).expect("encode");
    assert_eq!(request.bytes, b"IF;");
    let set = engine
        .encode(&Op::SetFrequency(Vfo::A, Hz(14_195_000)))
        .expect("encode");
    assert_eq!(set.bytes, b"FA00014195000;");
    assert!(rig.feed(&set.bytes, NOW).is_empty());
    let seen = observed(&mut engine, &mut rig, Op::GetFrequency(Vfo::A));
    assert!(
        seen.contains(&Observation::Frequency(Vfo::A, Hz(14_195_000))),
        "{seen:?}"
    );
}

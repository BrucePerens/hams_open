// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property tests standing in for fuzzing, over EVERY shipped rig description (all protocol
//! families through `conformance::engine_for`) and its virtual twin: random bytes, truncated and
//! corrupted real replies, runs of one byte, and huge garbage must never panic or hang an engine.
//! Bounded for CI (16 cases); for a long run: `PROPTEST_CASES=20000 cargo test -p
//! radioctl-protocols --test all_rigs_fuzz --release`. Nothing here touches a radio.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use proptest::prelude::*;
use radioctl_core::description::RigDescription;
use radioctl_core::engine::ProtocolEngine;
use radioctl_core::op::Op;
use radioctl_core::rigfile;
use radioctl_core::units::Vfo;
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::virtual_rig::VirtualRig;

const NOW: Duration = Duration::from_secs(5);

fn rigs() -> &'static Vec<RigDescription> {
    static CACHE: OnceLock<Vec<RigDescription>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).expect("rigs") {
            let path = entry.expect("entry").path();
            if path.extension().is_some_and(|e| e == "toml") {
                let text = std::fs::read_to_string(&path).expect("read");
                out.push(
                    rigfile::parse(&text)
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
                        .description,
                );
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    })
}

fn config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);
    ProptestConfig {
        cases,
        ..ProptestConfig::default()
    }
}

const READS: [Op; 7] = [
    Op::GetFrequency(Vfo::A),
    Op::GetFrequency(Vfo::B),
    Op::GetMode,
    Op::GetPtt,
    Op::GetSignal,
    Op::GetModelId,
    Op::GetFirmware,
];

/// What an engine is fed in one case, as seen by one rig.
#[derive(Debug, Clone)]
enum Mutation {
    Truncate(usize),
    Flip(usize, u8),
    Insert(usize, Vec<u8>),
    Duplicate(usize),
    DropTail(usize),
}

fn mutate(mut bytes: Vec<u8>, m: &Mutation) -> Vec<u8> {
    match m {
        Mutation::Truncate(n) => bytes.truncate(*n),
        Mutation::Flip(at, x) => {
            if !bytes.is_empty() {
                let i = at % bytes.len();
                bytes[i] ^= x;
            }
        }
        Mutation::Insert(at, extra) => {
            let i = at % (bytes.len() + 1);
            bytes.splice(i..i, extra.iter().copied());
        }
        Mutation::Duplicate(times) => {
            let once = bytes.clone();
            for _ in 0..*times {
                bytes.extend_from_slice(&once);
            }
        }
        Mutation::DropTail(n) => {
            let keep = bytes.len().saturating_sub(*n);
            bytes.truncate(keep);
        }
    }
    bytes
}

fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        (0usize..40).prop_map(Mutation::Truncate),
        (any::<usize>(), 1u8..=255).prop_map(|(a, x)| Mutation::Flip(a, x)),
        (any::<usize>(), proptest::collection::vec(any::<u8>(), 0..8))
            .prop_map(|(a, e)| Mutation::Insert(a, e)),
        (1usize..200).prop_map(Mutation::Duplicate),
        (1usize..10).prop_map(Mutation::DropTail),
    ]
}

proptest! {
    #![proptest_config(config())]

    // Tests [@ANCHOR: radioctl_conformance_engine_for]
    #[test]
    fn every_engine_survives_random_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..700),
        split in any::<usize>(),
    ) {
        for desc in rigs() {
            let mut engine = engine_for(desc).expect("engine");
            let cut = split % (bytes.len() + 1);
            let _ = engine.feed(&bytes[..cut], NOW);
            let _ = engine.feed(&bytes[cut..], NOW);
        }
    }

    #[test]
    fn every_engine_survives_runs_of_one_byte(byte in any::<u8>(), len in 0usize..70_000) {
        let bytes = vec![byte; len];
        for desc in rigs() {
            let mut engine = engine_for(desc).expect("engine");
            let _ = engine.feed(&bytes, NOW);
            let _ = engine.feed(&[byte], NOW);
        }
    }

    // Tests [@ANCHOR: radioctl_virtual_feed]
    #[test]
    fn every_engine_survives_corrupted_real_replies(
        ops in proptest::collection::vec(0usize..READS.len(), 1..5),
        muts in proptest::collection::vec(mutation(), 1..4),
    ) {
        for desc in rigs() {
            let Ok(mut rig) = VirtualRig::from_description(desc) else { continue };
            let mut engine = engine_for(desc).expect("engine");
            for &i in &ops {
                let Ok(request) = engine.encode(&READS[i]) else { continue };
                // The twin's request side too: mutated requests must not panic it.
                let reply = rig.feed(&request.bytes, NOW);
                let mut bad = reply;
                for m in &muts {
                    bad = mutate(bad, m);
                }
                let _ = engine.feed(&bad, NOW);
                let mut bad_request = request.bytes;
                for m in &muts {
                    bad_request = mutate(bad_request, m);
                }
                let _ = rig.feed(&bad_request, NOW);
            }
        }
    }
}

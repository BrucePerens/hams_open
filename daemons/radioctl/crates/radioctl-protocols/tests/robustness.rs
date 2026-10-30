// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property tests standing in for fuzzing: no byte sequence may panic a parser, and chunking
//! must not change what a parser finds.
#![allow(clippy::expect_used)]

use std::time::Duration;

use proptest::prelude::*;
use radioctl_core::description::RigDescription;
use radioctl_core::engine::ProtocolEngine;
use radioctl_core::units::Hz;
use radioctl_protocols::civ::{self, CivEngine, FrameParser};
use radioctl_protocols::kenwood::{KenwoodEngine, Tokenizer};
use radioctl_protocols::virtual_rig::VirtualRig;

const NOW: Duration = Duration::from_secs(5);

proptest! {
    // Tests [@ANCHOR: radioctl_civ_engine_feed]
    // Tests [@ANCHOR: radioctl_civ_parser_feed]
    #[test]
    fn civ_engine_survives_any_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        let mut engine = CivEngine::new(RigDescription::ic7300()).expect("engine");
        let _ = engine.feed(&bytes, NOW);
    }

    // Tests [@ANCHOR: radioctl_kenwood_engine_feed]
    // Tests [@ANCHOR: radioctl_kenwood_tokenizer_feed]
    #[test]
    fn kenwood_engine_survives_any_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        let mut engine = KenwoodEngine::new(RigDescription::ts2000()).expect("engine");
        let _ = engine.feed(&bytes, NOW);
    }

    // Tests [@ANCHOR: radioctl_virtual_feed]
    #[test]
    fn virtual_rigs_survive_any_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        for desc in [RigDescription::ic7300(), RigDescription::ts2000()] {
            let mut rig = VirtualRig::from_description(&desc).expect("rig");
            let _ = rig.feed(&bytes, NOW);
        }
    }

    // Tests [@ANCHOR: radioctl_civ_encode_bcd_freq]
    // Tests [@ANCHOR: radioctl_civ_decode_bcd_freq]
    #[test]
    fn bcd_frequency_round_trips(hz in 0u64..=9_999_999_999) {
        let encoded = civ::encode_bcd_freq(Hz(hz)).expect("in range");
        prop_assert_eq!(civ::decode_bcd_freq(&encoded), Some(Hz(hz)));
    }

    // Tests [@ANCHOR: radioctl_civ_encode_bcd_meter]
    // Tests [@ANCHOR: radioctl_civ_decode_bcd_meter]
    #[test]
    fn bcd_meter_round_trips(value in 0u16..=9999) {
        prop_assert_eq!(civ::decode_bcd_meter(&civ::encode_bcd_meter(value)), Some(value));
    }

    // Tests [@ANCHOR: radioctl_civ_parser_feed]
    #[test]
    fn civ_chunking_does_not_change_the_result(
        bytes in proptest::collection::vec(any::<u8>(), 0..200),
        cut in 0usize..200,
    ) {
        let cut = cut.min(bytes.len());
        let whole = flatten_civ(FrameParser::default().feed(&bytes, NOW));
        let mut parser = FrameParser::default();
        let mut parts = parser.feed(&bytes[..cut], NOW);
        parts.extend(parser.feed(&bytes[cut..], NOW));
        prop_assert_eq!(flatten_civ(parts), whole);
    }

    // Tests [@ANCHOR: radioctl_kenwood_tokenizer_feed]
    #[test]
    fn kenwood_chunking_does_not_change_the_commands(
        bytes in proptest::collection::vec(any::<u8>(), 0..200),
        cut in 0usize..200,
    ) {
        let cut = cut.min(bytes.len());
        let commands = |tokens: Vec<radioctl_protocols::kenwood::Token>| {
            tokens
                .into_iter()
                .filter_map(|t| match t {
                    radioctl_protocols::kenwood::Token::Command(c) => Some(c),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let whole = commands(Tokenizer::default().feed(&bytes, NOW));
        let mut tokenizer = Tokenizer::default();
        let mut parts = tokenizer.feed(&bytes[..cut], NOW);
        parts.extend(tokenizer.feed(&bytes[cut..], NOW));
        prop_assert_eq!(commands(parts), whole);
    }
}

/// Frames only; junk boundaries legitimately differ between chunkings.
fn flatten_civ(items: Vec<civ::Parsed>) -> Vec<civ::Frame> {
    items
        .into_iter()
        .filter_map(|item| match item {
            civ::Parsed::Frame(frame) => Some(frame),
            civ::Parsed::Junk(_) => None,
        })
        .collect()
}

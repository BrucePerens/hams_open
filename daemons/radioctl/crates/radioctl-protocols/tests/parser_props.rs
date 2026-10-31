// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property tests standing in for fuzzing, for the parsers `robustness.rs` does not cover: the
//! Yaesu newcat and legacy engines, the FlexRadio engine and its line parsers, the CI-V scope
//! parser and assembler, the HPSDR control-plane engine and the virtual Yaesu and Flex radios.
//! No byte sequence may panic any of them. The `hpsdr` crate's packet parsers have the same in
//! `daemons/hpsdr/tests/parser_props.rs` (its anchors live there).
#![allow(clippy::expect_used)]

use std::time::Duration;

use proptest::prelude::*;
use radioctl_core::description::RigDescription;
use radioctl_core::engine::ProtocolEngine;
use radioctl_core::rigfile;
use radioctl_protocols::civ_scope::{self, ScopeAssembler, ScopeSegment};
use radioctl_protocols::flex::{self, FlexEngine};
use radioctl_protocols::hpsdr_net::HpsdrEngine;
use radioctl_protocols::virtual_flex::VirtualFlex;
use radioctl_protocols::virtual_rig::VirtualRig;
use radioctl_protocols::yaesu::YaesuEngine;
use radioctl_protocols::yaesu_legacy::YaesuLegacyEngine;

const NOW: Duration = Duration::from_secs(5);

fn shipped(text: &str) -> RigDescription {
    rigfile::parse(text).expect("shipped rig file").description
}

fn ft991a() -> RigDescription {
    shipped(include_str!("../../../rigs/yaesu-ft-991a.toml"))
}

fn ft817() -> RigDescription {
    shipped(include_str!("../../../rigs/yaesu-ft-817.toml"))
}

fn flex6400() -> RigDescription {
    shipped(include_str!("../../../rigs/flex-6400.toml"))
}

fn anan100() -> RigDescription {
    shipped(include_str!("../../../rigs/anan-100.toml"))
}

fn ic7300() -> RigDescription {
    shipped(include_str!("../../../rigs/icom-ic-7300.toml"))
}

proptest! {
    // Tests [@ANCHOR: radioctl_yaesu_engine_feed]
    #[test]
    fn yaesu_engine_survives_any_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..600),
        text in "(IF|FA|FB|MD|TX|SM|RM|AG|RG|MG|PC|KS|RT|XT|ID|AI|FT)[ -~]{0,40};",
    ) {
        let mut engine = YaesuEngine::new(ft991a()).expect("engine");
        let _ = engine.feed(&bytes, NOW);
        let _ = engine.feed(text.as_bytes(), NOW);
        let mut rig = VirtualRig::from_description(&ft991a()).expect("rig");
        let _ = rig.feed(&bytes, NOW);
        let _ = rig.feed(text.as_bytes(), NOW);
    }

    // Tests [@ANCHOR: radioctl_ylegacy_engine_feed]
    // Tests [@ANCHOR: radioctl_virtual_legacy_block]
    #[test]
    fn yaesu_legacy_engine_survives_any_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..600),
        cut in 0usize..600,
    ) {
        let desc = ft817();
        let mut engine = YaesuLegacyEngine::new(desc.clone()).expect("engine");
        // A reply arrives in any chunking; the five-byte blocks are not self-delimiting.
        let cut = cut.min(bytes.len());
        let _ = engine.feed(&bytes[..cut], NOW);
        let _ = engine.feed(&bytes[cut..], NOW);
        let mut rig = VirtualRig::from_description(&desc).expect("rig");
        let _ = rig.feed(&bytes, NOW);
    }

    // Tests [@ANCHOR: radioctl_flex_engine_feed]
    // Tests [@ANCHOR: radioctl_virtual_flex_feed]
    #[test]
    fn flex_engine_and_virtual_radio_survive_any_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..600),
        line in "[VHSMRC][0-9A-F]{0,9}\\|[ -~]{0,80}\n",
    ) {
        let desc = flex6400();
        let mut engine = FlexEngine::new(desc.clone()).expect("engine");
        let _ = engine.feed(&bytes, NOW);
        let _ = engine.feed(line.as_bytes(), NOW);
        let mut radio = VirtualFlex::from_description(&desc).expect("radio");
        let _ = radio.feed(&bytes, NOW);
        let _ = radio.feed(line.as_bytes(), NOW);
    }

    // Tests [@ANCHOR: radioctl_flex_parse_reply]
    // Tests [@ANCHOR: radioctl_flex_parse_pairs]
    // Tests [@ANCHOR: radioctl_flex_parse_info]
    // Tests [@ANCHOR: radioctl_flex_parse_firmware]
    // Tests [@ANCHOR: radioctl_flex_parse_mhz]
    #[test]
    fn flex_line_parsers_survive_any_text(text in ".{0,200}") {
        let _ = flex::parse_reply(&text);
        let _ = flex::parse_pairs(&text);
        let _ = flex::parse_info(&text);
        let _ = flex::parse_firmware(&text);
        let _ = flex::parse_mhz(&text);
        let _ = flex::parse_mhz_signed(&text);
        let _ = flex::mode_from_name(&text);
        let _ = flex::line_may_transmit(&text);
        let _ = flex::interlock_keyed(&text);
    }

    // Tests [@ANCHOR: radioctl_flex_parse_discovery]
    #[test]
    fn flex_discovery_parser_survives_any_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..400),
    ) {
        let _ = flex::parse_discovery(&bytes);
    }

    // Tests [@ANCHOR: radioctl_scope_parse_segment]
    // Tests [@ANCHOR: radioctl_scope_decode_reference]
    // Tests [@ANCHOR: radioctl_scope_assembler_push]
    #[test]
    fn scope_parser_and_assembler_survive_any_frames(
        frames in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..80), 0..40),
    ) {
        let params = ic7300().scope.expect("the IC-7300 has a scope");
        let mut assembler = ScopeAssembler::new(params);
        for data in &frames {
            let _ = civ_scope::decode_reference(data);
            if let Some(segment) = civ_scope::parse_segment(data) {
                let _ = assembler.push(segment);
            }
        }
    }

    // Tests [@ANCHOR: radioctl_scope_assembler_push]
    #[test]
    fn scope_assembler_survives_any_segment(
        receiver in any::<u8>(),
        division in any::<u8>(),
        divisions in any::<u8>(),
        samples in proptest::collection::vec(any::<u8>(), 0..600),
    ) {
        let params = ic7300().scope.expect("the IC-7300 has a scope");
        let mut assembler = ScopeAssembler::new(params);
        let segment = ScopeSegment {
            receiver,
            division,
            divisions,
            header: None,
            samples,
        };
        let _ = assembler.push(segment.clone());
        let _ = assembler.push(segment);
    }

    // Tests [@ANCHOR: radioctl_civ_engine_feed]
    #[test]
    fn civ_scope_frames_through_the_engine_survive_any_payload(
        payload in proptest::collection::vec(any::<u8>(), 0..120),
    ) {
        let mut engine = radioctl_protocols::civ::CivEngine::new(ic7300()).expect("engine");
        let mut frame = vec![0xFE, 0xFE, 0xE0, 0x94, 0x27];
        frame.extend_from_slice(&payload);
        frame.push(0xFD);
        let _ = engine.feed(&frame, NOW);
    }

    // Tests [@ANCHOR: radioctl_hpsdr_engine_feed]
    #[test]
    fn hpsdr_control_engine_survives_any_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..600),
        line in "(OK|ERR|F|P|W|R|A)[ -~]{0,40}\n",
    ) {
        let mut engine = HpsdrEngine::new(anan100()).expect("engine");
        let _ = engine.feed(&bytes, NOW);
        let _ = engine.feed(line.as_bytes(), NOW);
    }
}

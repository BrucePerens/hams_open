// SPDX-License-Identifier: AGPL-3.0-or-later
//! The CI-V scope parser and assembler: arbitrary frames, arbitrary segments, and arbitrary
//! payloads through the engine as a scope frame (command 27).
#![no_main]

use std::sync::OnceLock;
use std::time::Duration;

use libfuzzer_sys::fuzz_target;
use radioctl_core::description::RigDescription;
use radioctl_core::engine::ProtocolEngine;
use radioctl_core::rigfile;
use radioctl_protocols::civ::CivEngine;
use radioctl_protocols::civ_scope::{self, ScopeAssembler, ScopeSegment};

static DESC: OnceLock<RigDescription> = OnceLock::new();

fuzz_target!(|data: &[u8]| {
    // The shipped IC-7300 file carries the scope parameters; the built-in description does not.
    let desc = DESC.get_or_init(|| {
        rigfile::parse(include_str!("../../rigs/icom-ic-7300.toml"))
            .expect("shipped rig file")
            .description
    });
    let Some(params) = desc.scope.clone() else {
        return;
    };
    let mut assembler = ScopeAssembler::new(params);
    // The input is a list of frames: a length byte, then that many bytes.
    let mut rest = data;
    while let Some((len, tail)) = rest.split_first() {
        let (frame, tail) = tail.split_at(usize::from(*len).min(tail.len()));
        rest = tail;
        let _ = civ_scope::decode_reference(frame);
        if let Some(segment) = civ_scope::parse_segment(frame) {
            let _ = assembler.push(segment);
        }
        if let [receiver, division, divisions, samples @ ..] = frame {
            let _ = assembler.push(ScopeSegment {
                receiver: *receiver,
                division: *division,
                divisions: *divisions,
                header: None,
                samples: samples.to_vec(),
            });
        }
        if let Ok(mut engine) = CivEngine::new(desc.clone()) {
            let mut wire = vec![0xFE, 0xFE, 0xE0, 0x94, 0x27];
            wire.extend_from_slice(frame);
            wire.push(0xFD);
            let _ = engine.feed(&wire, Duration::from_secs(5));
        }
    }
});

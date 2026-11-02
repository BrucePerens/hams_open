// SPDX-License-Identifier: AGPL-3.0-or-later
//! Every protocol engine and virtual rig fed arbitrary bytes, split at an arbitrary point (a reply
//! arrives in any chunking). The first input byte picks the engine, the second the split point.
//! No input may panic. Stands behind the proptests in `radioctl-protocols/tests/robustness.rs`
//! and `parser_props.rs`.
#![no_main]

use std::sync::OnceLock;
use std::time::Duration;

use libfuzzer_sys::fuzz_target;
use radioctl_core::description::RigDescription;
use radioctl_core::engine::ProtocolEngine;
use radioctl_core::rigfile;
use radioctl_protocols::civ::CivEngine;
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::flex::FlexEngine;
use radioctl_protocols::hpsdr_net::HpsdrEngine;
use radioctl_protocols::kenwood::KenwoodEngine;
use radioctl_protocols::virtual_flex::VirtualFlex;
use radioctl_protocols::virtual_rig::VirtualRig;
use radioctl_protocols::yaesu::YaesuEngine;
use radioctl_protocols::yaesu_legacy::YaesuLegacyEngine;

const NOW: Duration = Duration::from_secs(5);

/// The shipped descriptions the engines are built from, parsed once.
static FILES: OnceLock<Vec<RigDescription>> = OnceLock::new();

fuzz_target!(|data: &[u8]| {
    let files = FILES.get_or_init(|| {
        [
            include_str!("../../rigs/icom-ic-7300.toml"),
            include_str!("../../rigs/elecraft-kx3.toml"),
            include_str!("../../rigs/yaesu-ft-991a.toml"),
            include_str!("../../rigs/yaesu-ft-817.toml"),
            include_str!("../../rigs/yaesu-ft-840.toml"),
            include_str!("../../rigs/flex-6400.toml"),
            include_str!("../../rigs/anan-100.toml"),
        ]
        .iter()
        .map(|text| rigfile::parse(text).expect("shipped rig file").description)
        .collect()
    });
    let [select, split, rest @ ..] = data else {
        return;
    };
    let cut = usize::from(*split).min(rest.len());
    let (first, second) = rest.split_at(cut);
    // Engines built from the built-in descriptions (the ts2000 has no file of its own here).
    let mut engines: Vec<Box<dyn ProtocolEngine>> = Vec::new();
    match select % 12 {
        0 => engines.extend(CivEngine::new(files[0].clone()).map(|e| Box::new(e) as _)),
        1 => engines.extend(KenwoodEngine::new(RigDescription::ts2000()).map(|e| Box::new(e) as _)),
        2 => engines.extend(ElecraftEngine::new(files[1].clone()).map(|e| Box::new(e) as _)),
        3 => engines.extend(YaesuEngine::new(files[2].clone()).map(|e| Box::new(e) as _)),
        4 => engines.extend(YaesuLegacyEngine::new(files[3].clone()).map(|e| Box::new(e) as _)),
        5 => engines.extend(YaesuLegacyEngine::new(files[4].clone()).map(|e| Box::new(e) as _)),
        6 => engines.extend(FlexEngine::new(files[5].clone()).map(|e| Box::new(e) as _)),
        7 => engines.extend(HpsdrEngine::new(files[6].clone()).map(|e| Box::new(e) as _)),
        8 => {
            // The virtual radios answer whatever a controller (or an attacker on a pty) sends.
            for desc in &files[..5] {
                if let Ok(mut rig) = VirtualRig::from_description(desc) {
                    let _ = rig.feed(first, NOW);
                    let _ = rig.feed(second, NOW);
                }
            }
        }
        9 => {
            if let Ok(mut rig) = VirtualRig::from_description(&RigDescription::ts2000()) {
                let _ = rig.feed(first, NOW);
                let _ = rig.feed(second, NOW);
            }
        }
        _ => {
            if let Ok(mut radio) = VirtualFlex::from_description(&files[5]) {
                let _ = radio.feed(first, NOW);
                let _ = radio.feed(second, NOW);
            }
        }
    }
    for engine in &mut engines {
        let _ = engine.feed(first, NOW);
        let _ = engine.feed(second, NOW);
    }
});

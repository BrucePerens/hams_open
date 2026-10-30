// SPDX-License-Identifier: AGPL-3.0-or-later
//! The level ranges radioctld scales by must be the ranges the protocol engines accept: for every
//! shipped rig description and every level radioctld serves for it, the engine takes the lowest and
//! highest value and refuses one beyond either end.
#![allow(clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::Path;

use radioctl_core::op::Op;
use radioctl_core::rigfile;
use radioctl_protocols::conformance::engine_for;
use radioctld::names::{level_range, served_levels, LevelSource};

// Tests [@ANCHOR: radioctld_names_level_range]
// Tests [@ANCHOR: radioctld_names_served_levels]
#[test]
fn level_ranges_match_what_each_engine_accepts() {
    let rigs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
    let mut checked = 0;
    for entry in fs::read_dir(rigs).expect("rigs dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|x| x != "toml") {
            continue;
        }
        let description = rigfile::parse(&fs::read_to_string(&path).expect("read"))
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .description;
        for info in served_levels(&description) {
            let LevelSource::Radio(level) = info.source else {
                continue;
            };
            let (min, max) = level_range(&description, level).expect("a served level has a range");
            let mut engine = engine_for(&description).expect("engine");
            let name = format!("{} {}", description.id, level.name());
            assert!(
                engine.encode(&Op::SetLevel(level, min)).is_ok(),
                "{name} at its minimum"
            );
            assert!(
                engine.encode(&Op::SetLevel(level, max)).is_ok(),
                "{name} at its maximum"
            );
            assert!(
                engine.encode(&Op::SetLevel(level, max + 1)).is_err(),
                "{name} above its maximum"
            );
            if min > 0 {
                assert!(
                    engine.encode(&Op::SetLevel(level, min - 1)).is_err(),
                    "{name} below its minimum"
                );
            }
            checked += 1;
        }
    }
    assert!(checked >= 20, "only {checked} levels checked");
}

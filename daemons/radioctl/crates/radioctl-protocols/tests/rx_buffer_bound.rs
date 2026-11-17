// SPDX-License-Identifier: AGPL-3.0-or-later
//! A line that never sends a terminator, with each burst inside the partial-frame timeout, must
//! not grow an engine's receive buffer without limit: the engine hands the junk back instead.
//! Covers the engines that keep a raw receive buffer (receiver, Kenwood handheld, serial vendor).
#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::time::Duration;

use radioctl_core::description::Family;
use radioctl_core::engine::Inbound;
use radioctl_core::rigfile;
use radioctl_protocols::conformance::engine_for;

// Tests [@ANCHOR: radioctl_conformance_engine_for]
#[test]
fn unterminated_flood_is_handed_back_not_buffered_forever() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
    let mut unbounded = Vec::new();
    for entry in std::fs::read_dir(dir).expect("rigs") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let d = rigfile::parse(&std::fs::read_to_string(&path).expect("read"))
            .expect("parse")
            .description;
        if !matches!(
            d.family,
            Family::Receiver | Family::KenwoodHh | Family::SerialVendor
        ) {
            continue;
        }
        let Ok(mut engine) = engine_for(&d) else {
            continue;
        };
        let mut junk = 0usize;
        for i in 0..64u64 {
            for r in engine.feed(&[b'A'; 1024], Duration::from_millis(5 + i)) {
                if let Inbound::Unrecognised(v) = r {
                    junk += v.len();
                }
            }
        }
        // These two resynchronise on a frame header and drop leading garbage in place: bounded.
        if junk == 0 && d.id != "guohe-pmr-171" && d.id != "micom-2-3" {
            unbounded.push(d.id.clone());
        }
    }
    assert!(
        unbounded.is_empty(),
        "unbounded receive buffer: {unbounded:?}"
    );
}

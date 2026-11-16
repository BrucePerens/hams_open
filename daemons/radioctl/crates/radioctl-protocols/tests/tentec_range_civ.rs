// SPDX-License-Identifier: AGPL-3.0-or-later
//! Frequency range of the civ Ten-Tec rig files: SetFrequency outside it is refused.
#![allow(clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::Path;

use radioctl_core::rigfile;

// Tests [@ANCHOR: radioctl_core_description_frequency_limits]
#[test]
fn civ_rig_files_state_their_frequency_range() {
    let text = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/tentec-delta-ii.toml")).expect("read");
    let l = rigfile::parse(&text).expect("parse");
    let (lo, hi) = l.description.frequency_limits();
    assert_eq!((lo.0, hi.0), (100000, 29999000), "tentec-delta-ii");
    let text = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/tentec-omni-vi-plus.toml")).expect("read");
    let l = rigfile::parse(&text).expect("parse");
    let (lo, hi) = l.description.frequency_limits();
    assert_eq!((lo.0, hi.0), (1770000, 30030000), "tentec-omni-vi-plus");
}

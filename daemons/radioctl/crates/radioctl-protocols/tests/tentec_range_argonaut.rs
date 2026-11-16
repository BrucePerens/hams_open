// SPDX-License-Identifier: AGPL-3.0-or-later
//! Frequency range of the argonaut Ten-Tec rig files: SetFrequency outside it is refused.
#![allow(clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::Path;

use radioctl_core::rigfile;

// Tests [@ANCHOR: radioctl_core_description_frequency_limits]
#[test]
fn argonaut_rig_files_state_their_frequency_range() {
    let text = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/tentec-tt-516.toml"),
    )
    .expect("read");
    let l = rigfile::parse(&text).expect("parse");
    let (lo, hi) = l.description.frequency_limits();
    assert_eq!((lo.0, hi.0), (500000, 30000000), "tentec-tt-516");
    let text = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/tentec-rx-350.toml"),
    )
    .expect("read");
    let l = rigfile::parse(&text).expect("parse");
    let (lo, hi) = l.description.frequency_limits();
    assert_eq!((lo.0, hi.0), (100000, 30000000), "tentec-rx-350");
}

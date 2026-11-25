// SPDX-License-Identifier: AGPL-3.0-or-later
//! The IC-9700 (144, 430 and 1240 MHz bands, IC-9700_ENG_CI-V_4.pdf p. 16 band stacking
//! register) and the IC-7851 (receiver 0.030 to 60 MHz, IC-7850_7851_ENG_IM_3.pdf specifications)
//! state their frequency range (the range `SetFrequency` accepts) instead of the 0 Hz to 50 GHz default.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use radioctl_core::rigfile;
use radioctl_core::units::Hz;

fn shipped(id: &str) -> radioctl_core::description::RigDescription {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../rigs")
        .join(format!("{id}.toml"));
    let text = std::fs::read_to_string(path).expect("read");
    rigfile::parse(&text).expect("loads").description
}

#[test]
fn the_ic_9700_and_ic_7851_state_their_frequency_range() {
    assert_eq!(
        shipped("icom-ic-9700").frequency_range,
        Some((Hz(144_000_000), Hz(1_300_000_000)))
    );
    assert_eq!(
        shipped("icom-ic-7851").frequency_range,
        Some((Hz(30_000), Hz(60_000_000)))
    );
}

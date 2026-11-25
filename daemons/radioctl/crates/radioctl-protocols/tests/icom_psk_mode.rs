// SPDX-License-Identifier: AGPL-3.0-or-later
//! PSK is a mode of its own on the IC-7610 and IC-7760 (command 26 codes 12 PSK and 13 PSK-R,
//! IC-7610_ENG_CI-V_4.pdf p.13, IC-7760_ENG_CI-V_2.pdf p.23), so radioctl's mode vocabulary has
//! `psk` and `psk-r` and a description may list them with those codes. No radio is involved.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Duration;

use radioctl_core::engine::Inbound;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::Mode;
use radioctl_protocols::conformance::engine_for;

#[test]
fn psk_and_psk_r_are_modes_with_codes_12_and_13() {
    assert_eq!(Mode::from_name("psk"), Some(Mode::Psk));
    assert_eq!(Mode::from_name("psk-r"), Some(Mode::PskR));
    assert_eq!(Mode::Psk.name(), "psk");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../rigs/icom-ic-7760.toml");
    let mut text = std::fs::read_to_string(path).expect("read");
    text.push_str(
        "\n[[modes]]\nmode = \"psk\"\ncode = 0x12\n\n[[modes]]\nmode = \"psk-r\"\ncode = 0x13\n",
    );
    let desc = rigfile::parse(&text).expect("loads").description;
    let mut engine = engine_for(&desc).expect("engine");
    assert_eq!(
        engine.encode(&Op::SetMode(Mode::PskR)).expect("set").bytes[4..],
        [0x26, 0x00, 0x13, 0x00, 0x01, 0xFD]
    );
    let answer = [0xFE, 0xFE, 0xE0, 0xB2, 0x26, 0x00, 0x12, 0x00, 0x01, 0xFD];
    assert_eq!(
        engine.feed(&answer, Duration::from_secs(1)),
        vec![Inbound::Observation(Observation::Mode(Mode::Psk))]
    );
}

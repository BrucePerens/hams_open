// SPDX-License-Identifier: AGPL-3.0-or-later
//! The IC-705 and IC-9700 carry the DATA flag in command 26 (`26 00 <mode> <data> <filter>`, their
//! CI-V reference guides), so USB-D and LSB-D must be modes of their descriptions, as on the
//! IC-7300; the IC-705 also has WFM (code 06). No radio is involved.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Duration;

use radioctl_core::engine::Inbound;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::Mode;
use radioctl_protocols::conformance::engine_for;

const NOW: Duration = Duration::from_secs(10);

fn shipped(id: &str) -> radioctl_core::description::RigDescription {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../rigs")
        .join(format!("{id}.toml"));
    let text = std::fs::read_to_string(path).expect("read");
    rigfile::parse(&text).expect("loads").description
}

#[test]
fn the_ic_705_and_ic_9700_set_and_read_the_data_flag_through_command_26() {
    for (id, address) in [("icom-ic-705", 0xA4u8), ("icom-ic-9700", 0xA2)] {
        let desc = shipped(id);
        let mut engine = engine_for(&desc).expect("engine");
        let head = [0xFE, 0xFE, address, 0xE0];
        let frame = |body: &[u8]| [&head[..], body, &[0xFD]].concat();
        assert_eq!(
            engine.encode(&Op::GetMode).expect("get").bytes,
            frame(&[0x26, 0x00]),
            "{id}"
        );
        // USB with DATA on, the default filter number 1.
        assert_eq!(
            engine.encode(&Op::SetMode(Mode::Data)).expect("set").bytes,
            frame(&[0x26, 0x00, 0x01, 0x01, 0x01]),
            "{id}"
        );
        assert_eq!(
            engine.encode(&Op::SetMode(Mode::DataR)).expect("set").bytes,
            frame(&[0x26, 0x00, 0x00, 0x01, 0x01]),
            "{id}"
        );
        // The rig answers 26 00 <mode> <data> <filter> from its own address.
        let answer = [
            0xFE, 0xFE, 0xE0, address, 0x26, 0x00, 0x01, 0x01, 0x02, 0xFD,
        ];
        assert_eq!(
            engine.feed(&answer, NOW),
            vec![Inbound::Observation(Observation::Mode(Mode::Data))],
            "{id}"
        );
    }
}

#[test]
fn the_ic_705_has_wfm_at_code_06() {
    let desc = shipped("icom-ic-705");
    let mut engine = engine_for(&desc).expect("engine");
    assert_eq!(
        engine.encode(&Op::SetMode(Mode::Wfm)).expect("set").bytes[4..],
        [0x26, 0x00, 0x06, 0x00, 0x01, 0xFD]
    );
    assert!(shipped("icom-ic-9700").mode_code(Mode::Wfm).is_none());
}

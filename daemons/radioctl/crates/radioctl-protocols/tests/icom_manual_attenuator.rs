// SPDX-License-Identifier: AGPL-3.0-or-later
//! The attenuator steps of the IC-7610 and IC-7760 against their CI-V reference guides: command 11
//! lists 3 dB steps from 3 to 45 dB (IC-7610_ENG_CI-V_4.pdf and IC-7760_ENG_CI-V_2.pdf, command
//! table, command 11). No radio is involved.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use radioctl_core::engine::ReplyKind;
use radioctl_core::op::Op;
use radioctl_core::rigfile;
use radioctl_core::units::Level;
use radioctl_protocols::conformance::engine_for;

fn shipped(id: &str) -> radioctl_core::description::RigDescription {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../rigs")
        .join(format!("{id}.toml"));
    let text = std::fs::read_to_string(path).expect("read");
    rigfile::parse(&text).expect("loads").description
}

#[test]
fn the_ic_7610_and_ic_7760_offer_every_3_db_step_to_45_db() {
    let expected: Vec<u8> = (1..=15).map(|n| n * 3).collect();
    for id in ["icom-ic-7610", "icom-ic-7760"] {
        let desc = shipped(id);
        assert_eq!(desc.levels.attenuator_db, expected, "{id}");
        let mut engine = engine_for(&desc).expect("engine");
        for db in &expected {
            let request = engine
                .encode(&Op::SetLevel(Level::Attenuator, u16::from(*db)))
                .expect("encode");
            assert_eq!(request.reply, ReplyKind::Ack);
            // Command 11, the step as one BCD byte.
            let bcd = (db / 10) << 4 | (db % 10);
            assert_eq!(request.bytes[4..], [0x11, bcd, 0xFD], "{id} {db} dB");
        }
        assert!(engine
            .encode(&Op::SetLevel(Level::Attenuator, 48))
            .is_err());
    }
}

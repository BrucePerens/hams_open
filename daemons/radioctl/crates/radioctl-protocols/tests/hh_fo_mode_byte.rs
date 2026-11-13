// SPDX-License-Identifier: AGPL-3.0-or-later
//! An `FO` reply whose mode byte is below '0' must be refused, not overflow (found by the
//! all-rigs fuzz run: `mode - b'0'` ran before the digit check).
#![allow(clippy::expect_used)]

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine};
use radioctl_core::op::Op;
use radioctl_core::rigfile;
use radioctl_core::units::Vfo;
use radioctl_protocols::conformance::engine_for;

// Tests [@ANCHOR: radioctl_hh_parse_fo]
// Tests [@ANCHOR: radioctl_hh_decode_line]
#[test]
fn fo_reply_with_a_non_digit_mode_byte_is_unrecognised() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/kenwood-th-d72a.toml");
    let desc: RigDescription = rigfile::parse(&std::fs::read_to_string(path).expect("rig"))
        .expect("parse")
        .description;
    let mut engine = engine_for(&desc).expect("engine");
    engine.encode(&Op::GetFrequency(Vfo::A)).expect("encode");
    // 52 bytes: "FO 0," + ten digits + "," + padding, the mode byte (last) below '0'.
    let mut line = format!("FO 0,0145000000,{} ", "0".repeat(35)).into_bytes();
    assert_eq!(line.len(), 52);
    line.push(b'\r');
    let out = engine.feed(&line, Duration::from_secs(1));
    assert!(
        out.iter()
            .all(|i| matches!(i, Inbound::Unrecognised(_) | Inbound::Nak)),
        "{out:?}"
    );
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! The IC-7760's command 26 is `26 <00 Main|01 Sub> <mode> <data> <filter>` with modes 12 PSK and
//! 13 PSK-R (IC-7760_ENG_CI-V_2.pdf p.23). D2 and D3 read back as the data mode. No radio is involved.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Duration;

use radioctl_core::engine::Inbound;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::Mode;
use radioctl_protocols::conformance::engine_for;

#[test]
fn the_ic_7760_has_psk_and_reads_data_modes() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../rigs/icom-ic-7760.toml");
    let desc = rigfile::parse(&std::fs::read_to_string(path).expect("read"))
        .expect("loads")
        .description;
    let mut engine = engine_for(&desc).expect("engine");
    let head = [0xFE, 0xFE, 0xB2, 0xE0];
    let frame = |body: &[u8]| [&head[..], body, &[0xFD]].concat();
    assert_eq!(
        engine.encode(&Op::SetMode(Mode::Psk)).expect("set").bytes,
        frame(&[0x26, 0x00, 0x12, 0x00, 0x01])
    );
    assert_eq!(
        engine.encode(&Op::SetMode(Mode::PskR)).expect("set").bytes,
        frame(&[0x26, 0x00, 0x13, 0x00, 0x01])
    );
    for data in [0x01u8, 0x02, 0x03] {
        let answer = [0xFE, 0xFE, 0xE0, 0xB2, 0x26, 0x00, 0x01, data, 0x01, 0xFD];
        assert_eq!(
            engine.feed(&answer, Duration::from_secs(1)),
            vec![Inbound::Observation(Observation::Mode(Mode::Data))],
            "D{data}"
        );
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! On the IC-7610 and IC-7760 the selector byte of command 25 is `00` Main band, `01` Sub band
//! (IC-7610_ENG_CI-V_4.pdf, command 25 and the "Main or Sub band's frequency settings" format,
//! IC-7760_ENG_CI-V_2.pdf command 25), not the selected and unselected VFO of the IC-7300, IC-705
//! and IC-9700. Reading `25 01` and calling it VFO B would report the Sub receiver's frequency as
//! VFO B, so these models do not claim a second VFO. No radio is involved.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use radioctl_core::capability::Capability;
use radioctl_core::op::Op;
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Vfo};
use radioctl_protocols::conformance::engine_for;

#[test]
fn the_ic_7610_and_ic_7760_do_not_present_the_sub_band_as_vfo_b() {
    for id in ["icom-ic-7610", "icom-ic-7760"] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../rigs")
            .join(format!("{id}.toml"));
        let text = std::fs::read_to_string(path).expect("read");
        let desc = rigfile::parse(&text).expect("loads").description;
        assert!(!desc.capabilities.has(Capability::SecondVfo), "{id}");
        assert!(desc.capabilities.has(Capability::Receivers), "{id}");
        let mut engine = engine_for(&desc).expect("engine");
        assert!(engine.encode(&Op::GetFrequency(Vfo::B)).is_err(), "{id}");
        assert!(engine
            .encode(&Op::SetFrequency(Vfo::B, Hz(7_000_000)))
            .is_err());
    }
}

// Tests [@ANCHOR: radioctl_civ_engine_mode_selector]
// The mode commands of these models are `26 <00 Main|01 Sub>` (IC-7610_ENG_CI-V_4.pdf p.13,
// IC-7760_ENG_CI-V_2.pdf p.23): the band selected with `07 D0`/`D1` (or read with `07 D2`) is the
// one the engine addresses; other Icoms keep `26 00` whatever the selection.
#[test]
fn mode_commands_follow_the_selected_band_on_the_ic_7760() {
    use radioctl_core::engine::Inbound;
    use radioctl_core::op::Observation;
    use radioctl_core::units::{Mode, Receiver};
    use std::time::Duration;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../rigs/icom-ic-7760.toml");
    let desc = rigfile::parse(&std::fs::read_to_string(path).expect("read"))
        .expect("loads")
        .description;
    let mut engine = engine_for(&desc).expect("engine");
    let head = [0xFE, 0xFE, 0xB2, 0xE0];
    let frame = |body: &[u8]| [&head[..], body, &[0xFD]].concat();
    let get = |e: &mut Box<dyn radioctl_core::engine::ProtocolEngine>| e.encode(&Op::GetMode).expect("get").bytes;
    assert_eq!(get(&mut engine), frame(&[0x26, 0x00]));
    engine.encode(&Op::SelectReceiver(Receiver::Sub)).expect("select");
    assert_eq!(get(&mut engine), frame(&[0x26, 0x01]));
    assert_eq!(
        engine.encode(&Op::SetMode(Mode::Data)).expect("set").bytes,
        frame(&[0x26, 0x01, 0x01, 0x01, 0x01])
    );
    let answer = [0xFE, 0xFE, 0xE0, 0xB2, 0x26, 0x01, 0x00, 0x01, 0x02, 0xFD];
    assert_eq!(
        engine.feed(&answer, Duration::from_secs(1)),
        vec![Inbound::Observation(Observation::Mode(Mode::DataR))]
    );
    // A reply for the other band is not this band's mode.
    let other = [0xFE, 0xFE, 0xE0, 0xB2, 0x26, 0x00, 0x01, 0x00, 0x01, 0xFD];
    assert!(!engine
        .feed(&other, Duration::from_secs(1))
        .iter()
        .any(|i| matches!(i, Inbound::Observation(Observation::Mode(_)))));
    engine.encode(&Op::SelectReceiver(Receiver::Main)).expect("select");
    assert_eq!(get(&mut engine), frame(&[0x26, 0x00]));
}

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

// SPDX-License-Identifier: AGPL-3.0-or-later
//! PSK is a mode of its own on the IC-7610 and IC-7760 (command 26 codes 12 PSK and 13 PSK-R,
//! IC-7610_ENG_CI-V_4.pdf p.13, IC-7760_ENG_CI-V_2.pdf p.23), so radioctl's mode vocabulary has
//! `psk` and `psk-r` (the rig files are tested in their own files). No radio is involved.
#![allow(clippy::expect_used, clippy::panic)]

use radioctl_core::units::Mode;

#[test]
fn psk_and_psk_r_are_modes_with_codes_12_and_13() {
    assert_eq!(Mode::from_name("psk"), Some(Mode::Psk));
    assert_eq!(Mode::from_name("psk-r"), Some(Mode::PskR));
    assert_eq!(Mode::Psk.name(), "psk");
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! The FPGA question. An FDM-S1/S2 is a bus-powered front end whose USB bridge runs from an EEPROM and whose FPGA must be
//! programmed from a vendor image (one per sample rate) before samples flow. The images are ELAD's, not redistributable, and
//! come from ELAD's loader. This crate neither contains, downloads nor runs them. An FDM-DUO programs its own FPGA.

use crate::protocol::Model;

/// What the caller says about the FPGA.
// [@ANCHOR: fpga_fpga_state_enum]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpgaState {
    /// Someone else loaded the image for the sample rate in use (the caller's statement, not checked here).
    LoadedExternally,
    /// Nothing is loaded or nothing is known.
    Unknown,
}

impl FpgaState {
    /// Whether samples can be expected for `model` in this state.
    // [@ANCHOR: elad_fpga_ready]
    pub fn ready_for(self, model: Model) -> bool {
        match self {
            FpgaState::LoadedExternally => true,
            FpgaState::Unknown => !model.needs_fpga_image(),
        }
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! Open driver for the DG8SAQ VNWA 3E (USB 20a0:4118), written from observed USB traffic of the
//! vendor program running against the owner's own unit (`docs/PROTOCOL.md`). No vendor code and no
//! firmware is used or sent. This first cut only encodes the requests that were confirmed against
//! the wire; it makes no USB transfers yet.

pub mod protocol;

/// USB vendor ID and product ID of the VNWA control chip.
pub const VID: u16 = 0x20a0;
pub const PID: u16 = 0x4118;

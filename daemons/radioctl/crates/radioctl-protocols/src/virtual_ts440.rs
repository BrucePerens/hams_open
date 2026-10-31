// SPDX-License-Identifier: AGPL-3.0-or-later
//! The virtual TS-440S: the command side of [`VirtualRig`] for a Kenwood description whose dialect
//! is `ts440`, written from the Kenwood IC-10 interface manual (printed pages 12 to 31: command list,
//! `AI`, `FN`, `IF`, `MD`, `RU`/`RD`, `SP`). `MD` only sets (the mode is read from `IF`), `FN` and `SP`
//! take the place of `FR` and `FT`, `RU` and `RD` take no parameter, and nothing here knows `SM`, `AG`,
//! `RG`, `PC` or `KS`. A child module of `virtual_rig` so it can use the rig's private state; it
//! answers only what the engine asks. Never connected to a radio.

use radioctl_core::description::KenwoodDialect;

use super::VirtualRig;

/// A refusal.
const REFUSED: &str = "?;";
/// One `RU`/`RD` step in hertz. The manual does not state it; this is our own virtual value.
const RIT_STEP_HZ: i16 = 10;

impl VirtualRig {
    // [@ANCHOR: radioctl_virtual_ts440_is]
    /// True when this rig speaks the TS-440S or the TS-450S/TS-690S dialect.
    pub(super) fn is_ts440(&self) -> bool {
        self.desc
            .kenwood
            .as_ref()
            .is_some_and(|k| matches!(k.dialect, KenwoodDialect::Ts440 | KenwoodDialect::Ts450))
    }

    // [@ANCHOR: radioctl_virtual_ts440_is_fn_sp]
    /// True for the TS-440S (`FN` and `SP`), false for the TS-450S dialect.
    fn is_fn_sp(&self) -> bool {
        self.desc
            .kenwood
            .as_ref()
            .is_some_and(|k| k.dialect == KenwoodDialect::Ts440)
    }

    // [@ANCHOR: radioctl_virtual_ts440_command]
    /// One TS-440S command; the reply bytes (none for a setting, `?;` for a refusal).
    pub(super) fn ts440_command(&mut self, text: &str) -> Vec<u8> {
        let (name, arg) = (text.get(..2).unwrap_or(""), text.get(2..).unwrap_or(""));
        let reply: Option<String> = match (name, arg) {
            ("FA", "") => Some(format!("FA{:011};", self.state.freq_a.0)),
            ("FA", digits) => self.set_frequency(digits, true),
            ("FB", "") => Some(format!("FB{:011};", self.state.freq_b.0)),
            ("FB", digits) => self.set_frequency(digits, false),
            // Set only: a bare `MD` is refused.
            ("MD", code) if !code.is_empty() => self.set_kenwood_mode(code),
            ("TX", "") => {
                self.state.ptt = true;
                None
            }
            ("RX", "") => {
                self.state.ptt = false;
                None
            }
            ("ID", "") => Some(format!("ID{:03};", self.kenwood_id())),
            ("AI", "0" | "1") => {
                self.state.transceive = arg == "1";
                None
            }
            ("IF", "") => Some(self.kenwood_if()),
            // FN: 0 VFO A, 1 VFO B (2, memory, is not modelled). Set only.
            ("FN", "0" | "1") if self.is_fn_sp() => {
                self.kenwood_rx_vfo = u8::from(arg == "1");
                self.kenwood_tx_vfo = if self.state.split {
                    1 - self.kenwood_rx_vfo
                } else {
                    self.kenwood_rx_vfo
                };
                None
            }
            // SP: split on or off. Set only.
            ("SP", "0" | "1") if self.is_fn_sp() => {
                self.state.split = arg == "1";
                self.kenwood_tx_vfo = if self.state.split {
                    1 - self.kenwood_rx_vfo
                } else {
                    self.kenwood_rx_vfo
                };
                None
            }
            // TS-450S and TS-690S: FR and FT select the receive and transmit VFO (set only).
            ("FR" | "FT", "0" | "1") if !self.is_fn_sp() => {
                self.kenwood_select_vfo(arg, name == "FR")
            }
            ("RT", "0" | "1") => {
                self.state.rit = arg == "1";
                None
            }
            ("XT", "0" | "1") => {
                self.state.xit = arg == "1";
                None
            }
            ("RC", "") => {
                self.state.rit_offset = 0;
                None
            }
            ("RU" | "RD", "") => {
                let step = if name == "RU" {
                    RIT_STEP_HZ
                } else {
                    -RIT_STEP_HZ
                };
                self.state.rit_offset = self
                    .state
                    .rit_offset
                    .saturating_add(step)
                    .clamp(-9_990, 9_990);
                None
            }
            _ => Some(REFUSED.to_string()),
        };
        reply.map(String::into_bytes).unwrap_or_default()
    }
}

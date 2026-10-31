// SPDX-License-Identifier: AGPL-3.0-or-later
//! The virtual TS-990S: the command side of [`VirtualRig`] for a Kenwood description whose dialect
//! is `ts990`, written from Kenwood's "PC CONTROL COMMAND REFERENCE for the TS-990S" (March/29/2013).
//! The commands the classic virtual rig and the TS-990S share (`FA`, `ID`, `AI`, `SM0`, `IF`, `RT`,
//! `XT`, `RC`, `RU`/`RD`, `TX`, `RX`) are answered by the classic table; this adds `OM0` (the mode)
//! and refuses what the TS-990S has no such form of. A child module of `virtual_rig`. Never
//! connected to a radio.

use radioctl_core::description::KenwoodDialect;

use super::VirtualRig;

impl VirtualRig {
    // [@ANCHOR: radioctl_virtual_ts990_is]
    /// True when this rig speaks the TS-990S dialect.
    pub(super) fn is_ts990(&self) -> bool {
        self.desc
            .kenwood
            .as_ref()
            .is_some_and(|k| k.dialect == KenwoodDialect::Ts990)
    }

    // [@ANCHOR: radioctl_virtual_ts990_command]
    /// The TS-990S forms that differ from the classic table: `Some(reply bytes)` when this handled
    /// the command (the mode, or a refusal), `None` to let the classic table answer it.
    pub(super) fn ts990_command(&mut self, text: &str) -> Option<Vec<u8>> {
        let (name, arg) = (text.get(..2).unwrap_or(""), text.get(2..).unwrap_or(""));
        let reply: Option<String> = match (name, arg) {
            // `OM0;` reads the main band's mode; `OM0<m>;` sets it (P1 is not used for a set).
            ("OM", "0") => self
                .desc
                .mode_code(self.state.mode)
                .map(|code| format!("OM0{};", char::from(code))),
            ("OM", rest) if rest.len() == 2 && rest.starts_with(['0', '1']) => {
                self.set_kenwood_mode(&rest[1..])
            }
            // No `MD`, `FR`, `FT`, `SP` as the classic table has them, and the level and front end
            // forms are other ones (a band digit, other ranges).
            (
                "MD" | "FR" | "FT" | "SP" | "AG" | "RG" | "MG" | "ML" | "PC" | "KS" | "RA" | "PA",
                _,
            ) => Some("?;".to_string()),
            _ => return None,
        };
        Some(reply.map(String::into_bytes).unwrap_or_default())
    }
}

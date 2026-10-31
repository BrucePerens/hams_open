// SPDX-License-Identifier: AGPL-3.0-or-later
//! The virtual TS-890S: the command side of [`VirtualRig`] for a Kenwood description whose dialect
//! is `ts890`, written from the same guide as `kenwood_ts890.rs` (Kenwood "TS-890S PC CONTROL
//! COMMAND Reference Guide", January/30/2019). It is a child module of `virtual_rig` so it can use the
//! rig's private state; like the rest of the virtual rigs it answers only what the engine asks.

use radioctl_core::description::KenwoodDialect;
use radioctl_core::units::Level;

use super::VirtualRig;
use crate::attenuation;
use crate::kenwood;

/// A refusal.
const REFUSED: &str = "?;";

impl VirtualRig {
    // [@ANCHOR: radioctl_virtual_ts890_is]
    /// True when this rig speaks the TS-890S dialect.
    pub(super) fn is_ts890(&self) -> bool {
        self.desc
            .kenwood
            .as_ref()
            .is_some_and(|k| k.dialect == KenwoodDialect::Ts890)
    }

    // [@ANCHOR: radioctl_virtual_ts890_command]
    /// One TS-890S command; the reply bytes (none for a setting, `?;` for a refusal).
    pub(super) fn ts890_command(&mut self, text: &str) -> Vec<u8> {
        let (name, arg) = (text.get(..2).unwrap_or(""), text.get(2..).unwrap_or(""));
        let flag = |a: &str| match a {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        };
        let reply: Option<String> = match (name, arg) {
            ("FA", "") => Some(format!("FA{:011};", self.state.freq_a.0)),
            ("FA", digits) => self.set_frequency(digits, true),
            ("FB", "") => Some(format!("FB{:011};", self.state.freq_b.0)),
            ("FB", digits) => self.set_frequency(digits, false),
            ("OM", "0") => self
                .desc
                .mode_code(self.state.mode)
                .map(|code| format!("OM0{};", char::from(code))),
            ("OM", rest) if rest.len() == 2 && rest.starts_with(['0', '1']) => {
                self.set_kenwood_mode(&rest[1..])
            }
            ("TX", _) => {
                self.state.ptt = true;
                None
            }
            ("RX", _) => {
                self.state.ptt = false;
                None
            }
            ("ID", "") => Some(format!("ID{:03};", self.kenwood_id())),
            ("AI", "") => Some(format!("AI{};", u8::from(self.state.transceive))),
            ("AI", a) => {
                self.state.transceive = a != "0";
                None
            }
            ("SM", "") => Some(format!("SM{:04};", self.state.signal.min(70))),
            ("FR", "") => Some(format!("FR{};", self.kenwood_rx_vfo)),
            ("FR", "0" | "1") => self.kenwood_select_vfo(arg, true),
            ("FT", "") => Some(format!("FT{};", self.kenwood_tx_vfo)),
            ("FT", "0" | "1") => self.kenwood_select_vfo(arg, false),
            ("TB", "") => Some(format!("TB{};", u8::from(self.state.split))),
            ("TB", a) => flag(a).map_or_else(
                || Some(REFUSED.to_string()),
                |on| {
                    self.state.split = on;
                    self.kenwood_tx_vfo = if on {
                        self.kenwood_rx_vfo ^ 1
                    } else {
                        self.kenwood_rx_vfo
                    };
                    None
                },
            ),
            ("RT", "") => Some(format!("RT{};", u8::from(self.state.rit))),
            ("RT", a) => flag(a).map_or_else(
                || Some(REFUSED.to_string()),
                |on| {
                    self.state.rit = on;
                    None
                },
            ),
            ("XT", "") => Some(format!("XT{};", u8::from(self.state.xit))),
            ("XT", a) => flag(a).map_or_else(
                || Some(REFUSED.to_string()),
                |on| {
                    self.state.xit = on;
                    None
                },
            ),
            ("RC", "") => {
                self.state.rit_offset = 0;
                None
            }
            ("RU" | "RD", digits) => self.kenwood_move_offset(name == "RU", digits),
            ("RF", "") => {
                let offset = self.state.rit_offset;
                let sign = u8::from(offset < 0);
                Some(format!("RF{sign}{:04};", offset.unsigned_abs()))
            }
            ("AG" | "RG" | "MG" | "KS" | "PC", _) => self.ts890_level(name, arg),
            ("RA" | "PA", "") => {
                let level = if name == "RA" {
                    Level::Attenuator
                } else {
                    Level::Preamp
                };
                attenuation::virtual_get(&self.desc, &self.state.levels, level)
                    .and_then(|v| attenuation::step_of(&self.desc, level, v).ok())
                    .map(|step| format!("{name}{step};"))
                    .or_else(|| Some(REFUSED.to_string()))
            }
            ("RA" | "PA", digit) => {
                let level = if name == "RA" {
                    Level::Attenuator
                } else {
                    Level::Preamp
                };
                let value = (digit.len() == 1)
                    .then(|| digit.parse::<u8>().ok())
                    .flatten()
                    .and_then(|step| attenuation::value_of(&self.desc, level, step));
                match value {
                    Some(v)
                        if attenuation::virtual_set(
                            &self.desc,
                            &mut self.state.levels,
                            level,
                            v,
                        ) =>
                    {
                        None
                    }
                    _ => Some(REFUSED.to_string()),
                }
            }
            ("PS", "") => Some("PS1;".to_string()),
            ("FV", "") => Some("FV1.00;".to_string()),
            _ => Some(REFUSED.to_string()),
        };
        reply.map(String::into_bytes).unwrap_or_default()
    }

    // [@ANCHOR: radioctl_virtual_ts890_level]
    /// `AGnnn`, `RGnnn`, `MGnnn`, `KSnnn`, `PCnnn`: a read when `arg` is empty, else a set inside the
    /// guide's range.
    fn ts890_level(&mut self, name: &str, arg: &str) -> Option<String> {
        let (level, min, max) = match name {
            "AG" => (Level::AfGain, 0, 255),
            "RG" => (Level::RfGain, 0, 255),
            "MG" => (Level::MicGain, 0, 100),
            "KS" => (Level::KeyerSpeed, 4, 60),
            _ => (Level::RfPower, 5, 100),
        };
        if arg.is_empty() {
            let value = self.state.levels.get(&level).copied().unwrap_or(min);
            return Some(format!("{name}{value:03};"));
        }
        let value = kenwood::parse_digits(arg)
            .filter(|_| arg.len() == 3)
            .and_then(|v| u16::try_from(v).ok())
            .filter(|v| (min..=max).contains(v));
        match value {
            Some(value) => {
                self.state.levels.insert(level, value);
                None
            }
            None => Some(REFUSED.to_string()),
        }
    }
}

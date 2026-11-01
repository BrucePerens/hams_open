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

/// The weakest `##DD2` sample (-100 dB) and the number of samples in a frame.
const SWEEP_MAX: u8 = 0x8C;
const SWEEP_SAMPLES: usize = 640;

/// The bandscope settings of a virtual TS-890S (every `BS` command the guide has up to `BSC`, kept
/// as the digits the guide defines) and the sweep counter.
#[derive(Debug)]
pub(super) struct Ts890Scope {
    display: u8,
    kind: u8,
    mode: u8,
    span: u8,
    range: u8,
    pause: u8,
    marker: u8,
    attenuator: u8,
    max_hold: u8,
    averaging: u8,
    waterfall_speed: u8,
    reference: u16,
    output: u8,
    sweep: u32,
}

impl Default for Ts890Scope {
    // [@ANCHOR: radioctl_virtual_ts890_scope_default]
    /// The starting state (our choice, not a radio's): center mode, 50 kHz, 0 dB, output off.
    fn default() -> Ts890Scope {
        Ts890Scope {
            display: 1,
            kind: 0,
            mode: 0,
            span: 3,
            range: 0,
            pause: 0,
            marker: 0,
            attenuator: 0,
            max_hold: 0,
            averaging: 0,
            waterfall_speed: 1,
            reference: 40,
            output: 0,
            sweep: 0,
        }
    }
}

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
            ("BS", sub) => self.ts890_scope_command(sub),
            ("DD", "0") => Some(format!("DD0{};", self.ts890_scope.output)),
            ("DD", set) if set.len() == 2 && set.starts_with('0') => {
                match set[1..].parse::<u8>().ok().filter(|v| *v <= 5) {
                    Some(value) => {
                        self.ts890_scope.output = value;
                        None
                    }
                    None => Some(REFUSED.to_string()),
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

    // [@ANCHOR: radioctl_virtual_ts890_scope_command]
    /// One `BS` command (`sub` is what follows `BS`): a read when only the sub-command letter is
    /// there, else a set inside the guide's range. The settings are kept as the guide's digits.
    fn ts890_scope_command(&mut self, sub: &str) -> Option<String> {
        let (letter, arg) = (sub.get(..1).unwrap_or(""), sub.get(1..).unwrap_or(""));
        let scope = &mut self.ts890_scope;
        let (slot, max, digits): (&mut u8, u8, usize) = match letter {
            "0" => (&mut scope.display, 1, 1),
            "1" => (&mut scope.kind, 4, 1),
            "3" => (&mut scope.mode, 2, 1),
            "4" => (&mut scope.span, 6, 1),
            "5" => (&mut scope.range, 3, 1),
            "6" => (&mut scope.pause, 1, 1),
            "7" => (&mut scope.marker, 1, 1),
            "8" => (&mut scope.attenuator, 3, 1),
            "9" => (&mut scope.max_hold, 1, 1),
            "A" => (&mut scope.averaging, 3, 1),
            "B" => (&mut scope.waterfall_speed, 4, 1),
            "C" => {
                if arg.is_empty() {
                    return Some(format!("BSC{:03};", scope.reference));
                }
                let step = arg
                    .parse::<u16>()
                    .ok()
                    .filter(|v| arg.len() == 3 && *v <= 60);
                return match step {
                    Some(step) => {
                        scope.reference = step;
                        None
                    }
                    None => Some(REFUSED.to_string()),
                };
            }
            _ => return Some(REFUSED.to_string()),
        };
        if arg.is_empty() {
            return Some(format!("BS{letter}{slot};"));
        }
        // `BS4` also takes 9, the "initial value" of a span (a set only).
        let initial = letter == "4" && arg == "9";
        let value = arg
            .parse::<u8>()
            .ok()
            .filter(|v| arg.len() == digits && *v <= max);
        match (initial, value) {
            (true, _) => {
                *slot = 3;
                None
            }
            (false, Some(value)) if !(letter == "B" && value == 0) => {
                *slot = value;
                None
            }
            _ => Some(REFUSED.to_string()),
        }
    }

    // [@ANCHOR: radioctl_virtual_ts890_sweep]
    /// One `##DD2` frame (LAN output) when the waveform output is on a LAN cycle (`DD0` 1 to 3) and
    /// auto information is on, else nothing; the samples are the deterministic synthetic ones.
    pub(super) fn ts890_sweep(&mut self) -> Vec<u8> {
        let on = self.state.transceive && (1..=3).contains(&self.ts890_scope.output);
        if !on || self.desc.scope.is_none() {
            return Vec::new();
        }
        self.ts890_scope.sweep = self.ts890_scope.sweep.wrapping_add(1);
        let mut out = String::from("##DD2");
        for index in 0..SWEEP_SAMPLES {
            let value =
                crate::civ_scope::sample_value(index, self.ts890_scope.sweep, u16::from(SWEEP_MAX));
            out.push_str(&format!("{value:02X}"));
        }
        out.push(';');
        out.into_bytes()
    }
}

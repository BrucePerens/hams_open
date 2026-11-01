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
/// The weakest `##DD3` sample (-50 dB) and the number of samples in a filter-scope frame.
const FILTER_SWEEP_MAX: u8 = 0x32;
const FILTER_SWEEP_SAMPLES: usize = 213;

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
    audio_attenuator: u8,
    audio_span: u8,
    oscilloscope_level: u8,
    oscilloscope_sweep: u8,
    shift_position: u8,
    audio_pause: u8,
    expand: u8,
    /// Edges set by the marker-center command in fixed mode, until the mode or range changes.
    centred_edges: Option<(u32, u32)>,
    reference: u16,
    output: u8,
    /// `DD1`, the filter-scope output: 0 off, 1 LAN, 2 COM/USB.
    filter_output: u8,
    sweep: u32,
    /// The lower and upper edge of each fixed-mode range in hertz; entry 0 is the default range.
    edges: [(u32, u32); 4],
}

/// The span choices of `BS4` in hertz.
const SPANS_HZ: [u64; 7] = [5_000, 10_000, 25_000, 50_000, 100_000, 200_000, 500_000];

/// The edges of every fixed-mode range at the start (our choice, not a radio's): the 20 m band
/// for the default range, then 40 m, 80 m and 15 m.
const DEFAULT_EDGES: [(u32, u32); 4] = [
    (14_000_000, 14_350_000),
    (7_000_000, 7_300_000),
    (3_500_000, 4_000_000),
    (21_000_000, 21_450_000),
];

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
            audio_attenuator: 0,
            audio_span: 0,
            oscilloscope_level: 0,
            oscilloscope_sweep: 0,
            shift_position: 4,
            audio_pause: 0,
            expand: 0,
            centred_edges: None,
            reference: 40,
            output: 0,
            filter_output: 0,
            sweep: 0,
            edges: DEFAULT_EDGES,
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
            ("DD", "1") => Some(format!("DD1{};", self.ts890_scope.filter_output)),
            ("DD", set) if set.len() == 2 && set.starts_with('1') => {
                match set[1..].parse::<u8>().ok().filter(|v| *v <= 2) {
                    Some(value) => {
                        self.ts890_scope.filter_output = value;
                        None
                    }
                    None => Some(REFUSED.to_string()),
                }
            }
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
            "G" => (&mut scope.audio_attenuator, 3, 1),
            "H" => (&mut scope.audio_span, 1, 1),
            "I" => (&mut scope.oscilloscope_level, 3, 1),
            "J" => (&mut scope.oscilloscope_sweep, 5, 1),
            "K" => (&mut scope.shift_position, 8, 1),
            "N" => (&mut scope.audio_pause, 1, 1),
            "O" => (&mut scope.expand, 1, 1),
            // The overflow state has a read form only; this rig never overflows.
            "L" => {
                return Some(if arg.is_empty() {
                    "BSL0;".to_string()
                } else {
                    REFUSED.to_string()
                })
            }
            // Waterfall clear: the rig confirms it when auto information is on.
            "D" if arg.is_empty() => return self.state.transceive.then(|| "BSD;".to_string()),
            // Marker shift (auto scroll) or centre (fixed); invalid in center mode.
            "E" if arg.is_empty() => return self.ts890_marker_shift(),
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
            "M" => return self.ts890_edges_command(arg),
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
                if matches!(letter, "3" | "5") {
                    scope.centred_edges = None;
                }
                None
            }
            _ => Some(REFUSED.to_string()),
        }
    }

    // [@ANCHOR: radioctl_virtual_ts890_current_edges]
    /// The edges the scope now shows. Fixed mode: the range's own, or those the marker-center
    /// command moved. Auto scroll (our model; the guide gives none): the page of the span's width
    /// that holds the receive frequency, so the edges move when the dial leaves the page.
    fn ts890_current_edges(&self) -> (u32, u32) {
        let scope = &self.ts890_scope;
        if scope.mode != 2 {
            return scope
                .centred_edges
                .unwrap_or(scope.edges[usize::from(scope.range)]);
        }
        let freq = if self.kenwood_rx_vfo == 1 {
            self.state.freq_b.0
        } else {
            self.state.freq_a.0
        };
        let span = SPANS_HZ[usize::from(scope.span.min(6))];
        let low = freq - freq % span;
        let clamp = |hz: u64| u32::try_from(hz).unwrap_or(u32::MAX);
        (clamp(low), clamp(low + span))
    }

    // [@ANCHOR: radioctl_virtual_ts890_marker_shift]
    /// `BSE`: in fixed mode the range is re-centred on the receive frequency (until the mode or
    /// range changes); in auto scroll it has no effect here; in center mode the rig refuses it.
    fn ts890_marker_shift(&mut self) -> Option<String> {
        match self.ts890_scope.mode {
            0 => Some(REFUSED.to_string()),
            1 => {
                let (low, high) = self.ts890_current_edges();
                let freq = if self.kenwood_rx_vfo == 1 {
                    self.state.freq_b.0
                } else {
                    self.state.freq_a.0
                };
                let half = u64::from(high - low) / 2;
                let clamp = |hz: u64| u32::try_from(hz).unwrap_or(u32::MAX);
                self.ts890_scope.centred_edges =
                    Some((clamp(freq.saturating_sub(half)), clamp(freq + half)));
                None
            }
            _ => None,
        }
    }

    // [@ANCHOR: radioctl_virtual_ts890_edges_command]
    /// `BSM`: a read answers the edges of the range in use as `BSM0` and two eight-digit
    /// frequencies; a set stores the edges of range 1 to 3 (range 0 cannot be set), and two
    /// `99999999` put that range's initial edges back.
    fn ts890_edges_command(&mut self, arg: &str) -> Option<String> {
        let scope = &mut self.ts890_scope;
        if arg.is_empty() {
            let (low, high) = self.ts890_current_edges();
            return Some(format!("BSM0{low:08}{high:08};"));
        }
        let range = arg
            .get(..1)
            .and_then(|digit| digit.parse::<usize>().ok())
            .filter(|range| (1..=3).contains(range));
        let low = arg.get(1..9).and_then(kenwood::parse_digits);
        let high = arg.get(9..17).and_then(kenwood::parse_digits);
        match (range, low, high, arg.len()) {
            (Some(range), Some(99_999_999), Some(99_999_999), 17) => {
                scope.edges[range] = DEFAULT_EDGES[range];
                None
            }
            (Some(range), Some(low), Some(high), 17) if low < high => {
                match (u32::try_from(low), u32::try_from(high)) {
                    (Ok(low), Ok(high)) => {
                        scope.edges[range] = (low, high);
                        None
                    }
                    _ => Some(REFUSED.to_string()),
                }
            }
            _ => Some(REFUSED.to_string()),
        }
    }

    // [@ANCHOR: radioctl_virtual_ts890_filter_sweep]
    /// One `##DD3` frame (LAN output) when the filter-scope output is on LAN (`DD1` 1) and auto
    /// information is on, as the guide says (p. 70): 213 deterministic samples from 00 to 32.
    pub(super) fn ts890_filter_sweep(&mut self) -> Vec<u8> {
        let on = self.state.transceive && self.ts890_scope.filter_output == 1;
        if !on || self.desc.scope.is_none() {
            return Vec::new();
        }
        let mut out = String::from("##DD3");
        for index in 0..FILTER_SWEEP_SAMPLES {
            let value = crate::civ_scope::sample_value(
                index,
                self.ts890_scope.sweep,
                u16::from(FILTER_SWEEP_MAX),
            );
            out.push_str(&format!("{value:02X}"));
        }
        out.push(';');
        out.into_bytes()
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

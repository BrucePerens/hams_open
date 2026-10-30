// SPDX-License-Identifier: AGPL-3.0-or-later
//! Virtual rigs: sans-IO radios generated from a [`RigDescription`], replacing the semantics of
//! `tools/rig_simulator.py` (CI-V and Kenwood ASCII) for tests and CI.
//!
//! A [`VirtualRig`] owns all of its state; any number of them run at once and share nothing.
//! `feed` takes the bytes a controller wrote and returns the bytes the radio answers. Delays,
//! pseudo-terminals and sockets are the caller's business, so a virtual rig is also replayable
//! from a transcript.
//!
//! The Elecraft wire (KX3) is generated from a description of family `elecraft-ascii` and follows
//! the Programmer's Reference cited in `elecraft.rs`; the starting values for levels, firmware
//! (the manual's `02.37` example) and SWR are our own choices, not a KX3's.
//!
//! Differences from the Python simulator, on purpose: meter values are BCD as on real Icom
//! radios, not big-endian binary; an unknown CI-V mode code is refused with NG instead of being
//! accepted silently; the composite `IF;` answer is not implemented (`?;`), see `kenwood.rs`.

use std::time::Duration;

use std::collections::BTreeMap;

use radioctl_core::description::{
    CivParams, ElecraftParams, Family, KenwoodParams, RigDescription,
};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::units::{Hz, Level, Mode};

use crate::civ::{self, Frame, FrameParser, Parsed};
use crate::elecraft;
use crate::kenwood::{self, Token, Tokenizer};

/// Signal level a fresh virtual rig reports (the simulator's raw 30).
const DEFAULT_SIGNAL: u16 = 30;
/// Power meter reading while keyed (the simulator's raw 128).
const KEYED_POWER: u16 = 128;

/// The firmware revision a virtual Elecraft rig reports (the manual's example, not a real KX3's).
const ELECRAFT_FIRMWARE: (u8, u8) = (2, 37);
/// SWR (tenths) a virtual rig reports: 1.0:1.
const DEFAULT_SWR: u16 = 10;

/// The mutable state of a virtual rig.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigState {
    /// VFO A.
    pub freq_a: Hz,
    /// VFO B, modelled separately from A.
    pub freq_b: Hz,
    /// The mode.
    pub mode: Mode,
    /// True while keyed.
    pub ptt: bool,
    /// S-meter, raw units.
    pub signal: u16,
    /// Every Morse text received.
    pub cw_sent: Vec<String>,
    /// True when the rig reports front-panel changes on its own (CI-V transceive, Kenwood AI).
    pub transceive: bool,
    /// Split operation (Elecraft).
    pub split: bool,
    /// RIT on (Elecraft).
    pub rit: bool,
    /// XIT on (Elecraft).
    pub xit: bool,
    /// RIT/XIT offset in hertz (Elecraft).
    pub rit_offset: i16,
    /// Receive filter bandwidth (Elecraft).
    pub filter_width: Hz,
    /// Numeric levels (Elecraft).
    pub levels: BTreeMap<Level, u16>,
    /// SWR in tenths (Elecraft).
    pub swr: u16,
    /// Auto-info mode 0-3 (Elecraft `AI`); 0 sends nothing on its own.
    pub auto_info: u8,
    /// K2 command mode 0-3 as set by the controller (Elecraft `K2`); stored, the extended
    /// formats are not emulated.
    pub k2_mode: u8,
    /// K3 command mode 0-1 as set by the controller (Elecraft `K3`); stored, not emulated.
    pub k3_mode: u8,
}

impl RigState {
    // [@ANCHOR: radioctl_virtual_state_power]
    /// The power meter: nonzero only while keyed.
    pub fn power(&self) -> u16 {
        if self.ptt {
            KEYED_POWER
        } else {
            0
        }
    }
}

#[derive(Debug)]
enum Wire {
    Civ {
        params: CivParams,
        parser: FrameParser,
    },
    Kenwood {
        params: KenwoodParams,
        tokenizer: Tokenizer,
    },
    Elecraft {
        params: ElecraftParams,
        tokenizer: Tokenizer,
    },
}

/// One virtual radio.
#[derive(Debug)]
pub struct VirtualRig {
    desc: RigDescription,
    state: RigState,
    wire: Wire,
}

impl VirtualRig {
    // [@ANCHOR: radioctl_virtual_from_description]
    /// Builds a virtual rig of the described model, in its default state.
    pub fn from_description(desc: &RigDescription) -> Result<VirtualRig, RigError> {
        let wire = match (&desc.family, &desc.civ, &desc.kenwood) {
            (Family::IcomCiv, Some(params), _) => Wire::Civ {
                params: params.clone(),
                parser: FrameParser::default(),
            },
            (Family::KenwoodAscii, _, Some(params)) => Wire::Kenwood {
                params: params.clone(),
                tokenizer: Tokenizer::default(),
            },
            (Family::ElecraftAscii, _, _) if desc.elecraft.is_some() => Wire::Elecraft {
                params: desc
                    .elecraft
                    .clone()
                    .unwrap_or(ElecraftParams { model_id: 0 }),
                tokenizer: Tokenizer::default(),
            },
            _ => {
                return Err(RigError::new(
                    ErrorKind::Unsupported,
                    "description lacks its family block",
                ))
            }
        };
        let state = RigState {
            freq_a: desc.default_frequency,
            freq_b: desc.default_frequency,
            mode: desc.default_mode,
            ptt: false,
            signal: DEFAULT_SIGNAL,
            cw_sent: Vec::new(),
            transceive: desc.civ.is_some(),
            split: false,
            rit: false,
            xit: false,
            rit_offset: 0,
            filter_width: Hz(500),
            levels: BTreeMap::from([
                (Level::AfGain, 30),
                (Level::RfGain, 250),
                (Level::MicGain, 25),
                (Level::MonitorLevel, 20),
                (Level::KeyerSpeed, 18),
                (Level::RfPower, 5),
            ]),
            swr: DEFAULT_SWR,
            auto_info: 0,
            k2_mode: 0,
            k3_mode: 0,
        };
        Ok(VirtualRig {
            desc: desc.clone(),
            state,
            wire,
        })
    }

    // [@ANCHOR: radioctl_virtual_state]
    /// The rig's current state, for assertions.
    pub fn state(&self) -> &RigState {
        &self.state
    }

    // [@ANCHOR: radioctl_virtual_feed]
    /// Consumes bytes the controller wrote at `now`; returns the bytes the radio sends back.
    pub fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        match &mut self.wire {
            Wire::Civ { parser, .. } => {
                let items = parser.feed(bytes, now);
                let mut out = Vec::new();
                for item in items {
                    if let Parsed::Frame(frame) = item {
                        out.extend(self.civ_frame(&frame));
                    }
                }
                out
            }
            Wire::Kenwood { tokenizer, .. } => {
                let tokens = tokenizer.feed(bytes, now);
                let mut out = Vec::new();
                for token in tokens {
                    out.extend(match token {
                        Token::Command(text) => self.kenwood_command(&text),
                        Token::Junk(_) => b"?;".to_vec(),
                    });
                }
                out
            }
            Wire::Elecraft { tokenizer, .. } => {
                let tokens = tokenizer.feed(bytes, now);
                let mut out = Vec::new();
                for token in tokens {
                    out.extend(match token {
                        Token::Command(text) => self.elecraft_command(&text),
                        Token::Junk(_) => b"?;".to_vec(),
                    });
                }
                out
            }
        }
    }

    // [@ANCHOR: radioctl_virtual_front_panel]
    /// Changes state as the radio's own knobs would; returns the unsolicited bytes the rig sends
    /// when transceive is on, otherwise nothing.
    pub fn front_panel(&mut self, change: PanelChange) -> Vec<u8> {
        match change {
            PanelChange::Frequency(hz) => self.state.freq_a = hz,
            PanelChange::Mode(mode) => self.state.mode = mode,
            PanelChange::Ptt(on) => self.state.ptt = on,
            PanelChange::Signal(level) => self.state.signal = level,
            PanelChange::Split(on) => self.state.split = on,
            PanelChange::RitOffset(hz) => self.state.rit_offset = hz,
        }
        if matches!(self.wire, Wire::Elecraft { .. }) {
            return self.elecraft_panel(change);
        }
        if !self.state.transceive {
            return Vec::new();
        }
        match (&self.wire, change) {
            (Wire::Civ { params, .. }, PanelChange::Frequency(hz)) => civ::encode_bcd_freq(hz)
                .map(|bcd| unsolicited_civ(params, 0x00, &bcd))
                .unwrap_or_default(),
            (Wire::Civ { params, .. }, PanelChange::Mode(mode)) => self
                .desc
                .mode_code(mode)
                .map(|code| unsolicited_civ(params, 0x01, &[code, 0x01]))
                .unwrap_or_default(),
            (Wire::Kenwood { .. }, PanelChange::Frequency(hz)) => {
                format!("FA{:011};", hz.0).into_bytes()
            }
            (Wire::Kenwood { .. }, PanelChange::Mode(mode)) => self
                .desc
                .mode_code(mode)
                .map(|code| format!("MD{};", char::from(code)).into_bytes())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }
}

/// A change made at the radio itself rather than over the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelChange {
    /// The tuning knob moved VFO A.
    Frequency(Hz),
    /// A mode button was pressed.
    Mode(Mode),
    /// The microphone PTT was pressed or released.
    Ptt(bool),
    /// The received signal strength changed.
    Signal(u16),
    /// Split was switched on or off.
    Split(bool),
    /// The RIT/XIT offset was changed.
    RitOffset(i16),
}

fn unsolicited_civ(params: &CivParams, cmd: u8, data: &[u8]) -> Vec<u8> {
    Frame {
        to: civ::BROADCAST,
        from: params.address,
        cmd,
        data: data.to_vec(),
    }
    .to_bytes()
}

impl VirtualRig {
    // [@ANCHOR: radioctl_virtual_civ_frame]
    fn civ_frame(&mut self, frame: &Frame) -> Vec<u8> {
        let Wire::Civ { params, .. } = &self.wire else {
            return Vec::new();
        };
        let params = params.clone();
        if frame.from != params.controller && frame.from != 0x00 && frame.to != params.address {
            return Vec::new();
        }
        if frame.to != params.address && frame.to != civ::BROADCAST {
            return Vec::new();
        }
        let mut out = Vec::new();
        if params.echo {
            out.extend(frame.to_bytes());
        }
        let reply = |cmd: u8, data: &[u8]| {
            Frame {
                to: frame.from,
                from: params.address,
                cmd,
                data: data.to_vec(),
            }
            .to_bytes()
        };
        out.extend(match self.civ_dispatch(frame, &params) {
            CivReply::Ok => reply(civ::OK, &[]),
            CivReply::Ng => reply(civ::NG, &[]),
            CivReply::Data(cmd, data) => reply(cmd, &data),
        });
        out
    }

    // [@ANCHOR: radioctl_virtual_civ_dispatch]
    fn civ_dispatch(&mut self, frame: &Frame, params: &CivParams) -> CivReply {
        let data = frame.data.as_slice();
        match (frame.cmd, data) {
            (0x03, []) => data_reply(0x03, civ::encode_bcd_freq(self.state.freq_a), &[]),
            (0x05, bcd) => match civ::decode_bcd_freq(bcd) {
                Some(hz) if bcd.len() == 5 => {
                    self.state.freq_a = hz;
                    CivReply::Ok
                }
                _ => CivReply::Ng,
            },
            (0x25, [0x00]) => data_reply(0x25, civ::encode_bcd_freq(self.state.freq_a), &[0x00]),
            (0x25, [0x00, bcd @ ..]) => match civ::decode_bcd_freq(bcd) {
                Some(hz) if bcd.len() == 5 => {
                    self.state.freq_a = hz;
                    CivReply::Ok
                }
                _ => CivReply::Ng,
            },
            (0x04, []) => self.civ_mode_reply(0x04, &[]),
            (0x26, [0x00]) => self.civ_mode_reply(0x26, &[0x00]),
            (0x06, [code, ..]) | (0x26, [0x00, code, ..]) => match self.desc.code_mode(*code) {
                Some(mode) => {
                    self.state.mode = mode;
                    CivReply::Ok
                }
                None => CivReply::Ng,
            },
            (0x1C, [0x00]) => CivReply::Data(0x1C, vec![0x00, u8::from(self.state.ptt)]),
            (0x1C, [0x00, state]) => {
                self.state.ptt = *state != 0;
                CivReply::Ok
            }
            (0x17, text) => {
                self.state
                    .cw_sent
                    .push(String::from_utf8_lossy(text).into_owned());
                CivReply::Ok
            }
            (0x19, [0x00]) => CivReply::Data(0x19, vec![0x00, params.model_id]),
            (0x15, [0x02]) => meter_reply(0x02, self.state.signal),
            (0x15, [0x11]) => meter_reply(0x11, self.state.power()),
            _ => CivReply::Ng,
        }
    }

    // [@ANCHOR: radioctl_virtual_civ_mode_reply]
    fn civ_mode_reply(&self, cmd: u8, prefix: &[u8]) -> CivReply {
        let Some(code) = self.desc.mode_code(self.state.mode) else {
            return CivReply::Ng;
        };
        let mut data = prefix.to_vec();
        data.push(code);
        if cmd == 0x26 {
            // `26 00` also reports the DATA flag (off) between the mode and the filter; Hamlib
            // reads the filter byte as the DATA flag if it is missing (found by the rigctld
            // oracle test).
            data.push(0x00);
        }
        data.push(0x01);
        CivReply::Data(cmd, data)
    }

    // [@ANCHOR: radioctl_virtual_kenwood_command]
    fn kenwood_command(&mut self, text: &str) -> Vec<u8> {
        let (name, arg) = (text.get(..2).unwrap_or(""), text.get(2..).unwrap_or(""));
        let reply = match (name, arg) {
            ("FA", "") => Some(format!("FA{:011};", self.state.freq_a.0)),
            ("FA", digits) => self.set_frequency(digits, true),
            ("FB", "") => Some(format!("FB{:011};", self.state.freq_b.0)),
            ("FB", digits) => self.set_frequency(digits, false),
            ("MD", "") => self.kenwood_mode_reply(),
            ("MD", code) => self.set_kenwood_mode(code),
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
            ("AI", flag) => {
                self.state.transceive = flag != "0";
                None
            }
            ("SM", "" | "0") => Some(format!("SM0{:04};", self.state.signal)),
            ("KS", "") => Some("KS020;".to_string()),
            ("PS", "") => Some("PS1;".to_string()),
            ("KS" | "PS" | "VX", _) => None,
            _ => Some("?;".to_string()),
        };
        reply.map(String::into_bytes).unwrap_or_default()
    }

    // [@ANCHOR: radioctl_virtual_kenwood_id]
    fn kenwood_id(&self) -> u16 {
        match &self.wire {
            Wire::Kenwood { params, .. } => params.model_id,
            Wire::Civ { .. } | Wire::Elecraft { .. } => 0,
        }
    }

    // [@ANCHOR: radioctl_virtual_set_frequency]
    fn set_frequency(&mut self, digits: &str, vfo_a: bool) -> Option<String> {
        let Some(hz) = kenwood::parse_digits(digits).filter(|_| digits.len() == 11) else {
            return Some("?;".to_string());
        };
        if vfo_a {
            self.state.freq_a = Hz(hz);
        } else {
            self.state.freq_b = Hz(hz);
        }
        None
    }

    fn kenwood_mode_reply(&self) -> Option<String> {
        self.desc
            .mode_code(self.state.mode)
            .map(|code| format!("MD{};", char::from(code)))
    }

    // [@ANCHOR: radioctl_virtual_set_kenwood_mode]
    fn set_kenwood_mode(&mut self, code: &str) -> Option<String> {
        let mode = match code.as_bytes() {
            [byte] => self.desc.code_mode(*byte),
            _ => None,
        };
        match mode {
            Some(mode) => {
                self.state.mode = mode;
                None
            }
            None => Some("?;".to_string()),
        }
    }
}

impl VirtualRig {
    // [@ANCHOR: radioctl_virtual_elecraft_command]
    /// The virtual Elecraft command table (Programmer's Reference, see `elecraft.rs`).
    fn elecraft_command(&mut self, text: &str) -> Vec<u8> {
        let upper = text.to_ascii_uppercase();
        let (name, arg) = (upper.get(..2).unwrap_or(""), upper.get(2..).unwrap_or(""));
        let flag = |arg: &str| match arg {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        };
        let refuse = || Some("?;".to_string());
        let reply: Option<String> = match (name, arg) {
            ("FA", "") => Some(format!("FA{:011};", self.state.freq_a.0)),
            ("FA", digits) => self.set_frequency(digits, true),
            ("FB", "") => Some(format!("FB{:011};", self.state.freq_b.0)),
            ("FB", digits) => self.set_frequency(digits, false),
            ("MD", "") => self.kenwood_mode_reply(),
            ("MD", code) => self.set_kenwood_mode(code),
            ("TQ", "") => Some(format!("TQ{};", u8::from(self.state.ptt))),
            ("TX", "") => {
                self.state.ptt = true;
                None
            }
            ("RX", "") => {
                self.state.ptt = false;
                None
            }
            ("SM", "") => {
                let level = if self.state.ptt {
                    0
                } else {
                    self.state.signal.min(15)
                };
                Some(format!("SM{level:04};"))
            }
            ("PO", "") => Some(format!("PO{:03};", self.elecraft_power())),
            ("FT", "") => Some(format!("FT{};", u8::from(self.state.split))),
            ("FT", on) => flag(on).map_or_else(refuse, |on| {
                self.state.split = on;
                None
            }),
            ("FR", _) => {
                self.state.split = false;
                None
            }
            ("RT", "") => Some(format!("RT{};", u8::from(self.state.rit))),
            ("RT", on) => flag(on).map_or_else(refuse, |on| {
                self.state.rit = on;
                None
            }),
            ("XT", "") => Some(format!("XT{};", u8::from(self.state.xit))),
            ("XT", on) => flag(on).map_or_else(refuse, |on| {
                self.state.xit = on;
                None
            }),
            ("RO", "") => Some(format!(
                "RO{};",
                elecraft::signed_offset(self.state.rit_offset)
            )),
            ("RO", data) => match elecraft::parse_offset(data) {
                Some(hz) => {
                    self.state.rit_offset = hz;
                    None
                }
                None => refuse(),
            },
            ("RC", "") => {
                self.state.rit_offset = 0;
                None
            }
            ("BW", "") => Some(format!("BW{:04};", self.state.filter_width.0 / 10)),
            ("BW", digits) => match kenwood::parse_digits(digits).filter(|_| digits.len() == 4) {
                Some(tens) => {
                    self.state.filter_width = Hz(tens * 10);
                    None
                }
                None => refuse(),
            },
            ("SW", "") => Some(format!("SW{:03};", self.state.swr)),
            ("ID", "") => Some(format!("ID{:03};", self.elecraft_id())),
            ("IF", "") => Some(self.elecraft_if()),
            // Firmware revisions ("RV", p. 23): the MCU and the main DSP report ours; a module
            // that is not present or unknown reports 99.99.
            ("RV", module @ ("M" | "D")) => Some(format!(
                "RV{module}{:02}.{:02};",
                ELECRAFT_FIRMWARE.0, ELECRAFT_FIRMWARE.1
            )),
            ("RV", module)
                if module.len() == 1 && module.bytes().all(|b| b.is_ascii_uppercase()) =>
            {
                Some(format!("RV{module}99.99;"))
            }
            ("AI", "") => Some(format!("AI{};", self.state.auto_info)),
            ("AI", level) => {
                match kenwood::parse_digits(level).filter(|v| *v <= 3 && level.len() == 1) {
                    Some(value) => {
                        self.state.auto_info = u8::try_from(value).unwrap_or(0);
                        self.state.transceive = value > 0;
                        (value == 1).then(|| self.elecraft_if())
                    }
                    None => refuse(),
                }
            }
            ("PS", "") => Some("PS1;".to_string()),
            ("PS", "1") => None,
            // Option modules (Programmer's Reference, "OM", p. 21): none installed, product id 02.
            ("OM", "") => Some("OM ----------02;".to_string()),
            ("K2", "") => Some(format!("K2{};", self.state.k2_mode)),
            ("K2", mode) => self.set_meta(mode, 3, true),
            ("K3", "") => Some(format!("K3{};", self.state.k3_mode)),
            ("K3", mode) => self.set_meta(mode, 1, false),
            ("KY", "") => Some("KY0;".to_string()),
            ("KY", _) => {
                let sent = text.get(3..).unwrap_or("").to_string();
                self.state.cw_sent.push(sent);
                None
            }
            (prefix, data) => match [
                Level::AfGain,
                Level::RfGain,
                Level::MicGain,
                Level::MonitorLevel,
                Level::KeyerSpeed,
                Level::RfPower,
            ]
            .into_iter()
            .find(|l| elecraft::level_wire(*l).prefix == prefix)
            {
                Some(level) => self.elecraft_level(level, data),
                None => refuse(),
            },
        };
        reply.map(String::into_bytes).unwrap_or_default()
    }

    // [@ANCHOR: radioctl_virtual_elecraft_set_meta]
    /// Stores a `K2n` (0-3) or `K3n` (0-1) meta-command value; anything else is refused. The
    /// virtual rig keeps answering in the Basic formats whatever is stored.
    fn set_meta(&mut self, digit: &str, max: u8, k2: bool) -> Option<String> {
        match kenwood::parse_digits(digit)
            .filter(|_| digit.len() == 1)
            .and_then(|v| u8::try_from(v).ok())
            .filter(|v| *v <= max)
        {
            Some(value) => {
                if k2 {
                    self.state.k2_mode = value;
                } else {
                    self.state.k3_mode = value;
                }
                None
            }
            None => Some("?;".to_string()),
        }
    }

    // [@ANCHOR: radioctl_virtual_elecraft_level]
    /// Reads or sets a level of the virtual Elecraft rig; a set outside the manual's range is
    /// clamped to it, a malformed one is refused.
    fn elecraft_level(&mut self, level: Level, data: &str) -> Option<String> {
        let wire = elecraft::level_wire(level);
        if data.is_empty() {
            let value = self.state.levels.get(&level).copied().unwrap_or(wire.min);
            return Some(format!("{}{value:03};", wire.prefix));
        }
        let parsed = kenwood::parse_digits(data).filter(|_| data.len() == 3);
        match parsed.and_then(|v| u16::try_from(v).ok()) {
            Some(value) => {
                self.state
                    .levels
                    .insert(level, value.clamp(wire.min, wire.max));
                None
            }
            None => Some("?;".to_string()),
        }
    }

    // [@ANCHOR: radioctl_virtual_elecraft_if]
    /// The `IF` answer for the current state (Programmer's Reference, "IF", p. 14).
    fn elecraft_if(&self) -> String {
        let mode = self.desc.mode_code(self.state.mode).map_or('0', char::from);
        format!(
            "IF{:011}     {}{}{} 00{}{}00{}001 ;",
            self.state.freq_a.0,
            elecraft::signed_offset(self.state.rit_offset),
            u8::from(self.state.rit),
            u8::from(self.state.xit),
            u8::from(self.state.ptt),
            mode,
            u8::from(self.state.split),
        )
    }

    // [@ANCHOR: radioctl_virtual_elecraft_panel]
    /// What a virtual Elecraft rig sends on its own after a front-panel change: nothing in `AI0`,
    /// an `IF` answer for frequency, mode, split and offset events in `AI1`, and in `AI2` and
    /// `AI3` the specific `FA` or `MD` answer (or `IF` for split and offset changes).
    fn elecraft_panel(&self, change: PanelChange) -> Vec<u8> {
        let text = match (self.state.auto_info, change) {
            (0, _) | (_, PanelChange::Ptt(_) | PanelChange::Signal(_)) => return Vec::new(),
            (1, _) | (_, PanelChange::Split(_) | PanelChange::RitOffset(_)) => self.elecraft_if(),
            (_, PanelChange::Frequency(hz)) => format!("FA{:011};", hz.0),
            (_, PanelChange::Mode(mode)) => match self.desc.mode_code(mode) {
                Some(code) => format!("MD{};", char::from(code)),
                None => return Vec::new(),
            },
        };
        text.into_bytes()
    }

    // [@ANCHOR: radioctl_virtual_elecraft_power]
    /// The `PO` reading: the requested power in tenths of a watt while keyed, else zero.
    fn elecraft_power(&self) -> u16 {
        if !self.state.ptt {
            return 0;
        }
        let watts = self.state.levels.get(&Level::RfPower).copied().unwrap_or(0);
        watts.saturating_mul(10).min(999)
    }

    // [@ANCHOR: radioctl_virtual_elecraft_id]
    fn elecraft_id(&self) -> u16 {
        match &self.wire {
            Wire::Elecraft { params, .. } => params.model_id,
            _ => 0,
        }
    }
}

enum CivReply {
    Ok,
    Ng,
    Data(u8, Vec<u8>),
}

// [@ANCHOR: radioctl_virtual_data_reply]
fn data_reply(cmd: u8, bcd: Option<[u8; 5]>, prefix: &[u8]) -> CivReply {
    match bcd {
        Some(bytes) => {
            let mut data = prefix.to_vec();
            data.extend(bytes);
            CivReply::Data(cmd, data)
        }
        None => CivReply::Ng,
    }
}

// [@ANCHOR: radioctl_virtual_meter_reply]
fn meter_reply(sub: u8, value: u16) -> CivReply {
    let mut data = vec![sub];
    data.extend(civ::encode_bcd_meter(value));
    CivReply::Data(0x15, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn civ_rig() -> VirtualRig {
        VirtualRig::from_description(&RigDescription::ic7300()).expect("rig")
    }

    fn kenwood_rig() -> VirtualRig {
        VirtualRig::from_description(&RigDescription::ts2000()).expect("rig")
    }

    // Tests [@ANCHOR: radioctl_virtual_from_description]
    // Tests [@ANCHOR: radioctl_virtual_state]
    // Tests [@ANCHOR: radioctl_virtual_state_power]
    #[test]
    fn defaults_come_from_the_description() {
        let rig = civ_rig();
        assert_eq!(rig.state().freq_a, Hz(14_074_000));
        assert_eq!(rig.state().mode, Mode::Usb);
        assert!(!rig.state().ptt);
        assert_eq!(rig.state().power(), 0);
        let mut broken = RigDescription::ic7300();
        broken.civ = None;
        assert!(VirtualRig::from_description(&broken).is_err());
    }

    // Tests [@ANCHOR: radioctl_virtual_feed]
    // Tests [@ANCHOR: radioctl_virtual_civ_dispatch]
    // Tests [@ANCHOR: radioctl_virtual_civ_frame]
    // Tests [@ANCHOR: radioctl_virtual_civ_mode_reply]
    // Tests [@ANCHOR: radioctl_virtual_data_reply]
    // Tests [@ANCHOR: radioctl_virtual_meter_reply]
    #[test]
    fn civ_transcript_read_set_read() {
        let mut rig = civ_rig();
        let read = [0xFE, 0xFE, 0x94, 0xE0, 0x03, 0xFD];
        assert_eq!(
            rig.feed(&read, T0),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x03, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD]
        );
        let set = [
            0xFE, 0xFE, 0x94, 0xE0, 0x05, 0x00, 0x00, 0x03, 0x07, 0x00, 0xFD,
        ];
        assert_eq!(rig.feed(&set, T0), vec![0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD]);
        assert_eq!(rig.state().freq_a, Hz(7_030_000));
        let ptt_on = [0xFE, 0xFE, 0x94, 0xE0, 0x1C, 0x00, 0x01, 0xFD];
        assert_eq!(
            rig.feed(&ptt_on, T0),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD]
        );
        let power = [0xFE, 0xFE, 0x94, 0xE0, 0x15, 0x11, 0xFD];
        assert_eq!(
            rig.feed(&power, T0),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x15, 0x11, 0x01, 0x28, 0xFD]
        );
        let mode = [0xFE, 0xFE, 0x94, 0xE0, 0x04, 0xFD];
        assert_eq!(
            rig.feed(&mode, T0),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x04, 0x01, 0x01, 0xFD]
        );
        let mode_26 = [0xFE, 0xFE, 0x94, 0xE0, 0x26, 0x00, 0xFD];
        assert_eq!(
            rig.feed(&mode_26, T0),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x26, 0x00, 0x01, 0x00, 0x01, 0xFD]
        );
        let unknown = [0xFE, 0xFE, 0x94, 0xE0, 0x7F, 0xFD];
        assert_eq!(
            rig.feed(&unknown, T0),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD]
        );
        let other = [0xFE, 0xFE, 0x70, 0xE0, 0x03, 0xFD];
        assert!(rig.feed(&other, T0).is_empty());
        let id = [0xFE, 0xFE, 0x00, 0xE0, 0x19, 0x00, 0xFD];
        assert_eq!(
            rig.feed(&id, T0),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x19, 0x00, 0x94, 0xFD]
        );
    }

    // Tests [@ANCHOR: radioctl_virtual_feed]
    #[test]
    fn civ_bus_echo_precedes_the_reply() {
        let mut desc = RigDescription::ic7300();
        if let Some(civ) = desc.civ.as_mut() {
            civ.echo = true;
        }
        let mut rig = VirtualRig::from_description(&desc).expect("rig");
        let read = [0xFE, 0xFE, 0x94, 0xE0, 0x03, 0xFD];
        let out = rig.feed(&read, T0);
        assert_eq!(out.get(..6), Some(&read[..]));
        assert_eq!(out.len(), 6 + 11);
    }

    // Tests [@ANCHOR: radioctl_virtual_feed]
    // Tests [@ANCHOR: radioctl_virtual_kenwood_command]
    // Tests [@ANCHOR: radioctl_virtual_kenwood_id]
    // Tests [@ANCHOR: radioctl_virtual_set_frequency]
    // Tests [@ANCHOR: radioctl_virtual_set_kenwood_mode]
    #[test]
    fn kenwood_transcript_matches_the_simulator_semantics() {
        let mut rig = kenwood_rig();
        assert_eq!(rig.feed(b"FA;", T0), b"FA00014074000;");
        assert!(rig.feed(b"FA00007040000;", T0).is_empty());
        assert_eq!(rig.state().freq_a, Hz(7_040_000));
        assert!(rig.feed(b"FB00003573000;", T0).is_empty());
        assert_eq!(rig.feed(b"FB;", T0), b"FB00003573000;");
        assert_eq!(rig.feed(b"FA;", T0), b"FA00007040000;");
        assert_eq!(rig.feed(b"MD;", T0), b"MD2;");
        assert!(rig.feed(b"TX;", T0).is_empty());
        assert!(rig.state().ptt);
        assert!(rig.feed(b"RX;", T0).is_empty());
        assert!(!rig.state().ptt);
        assert_eq!(rig.feed(b"ID;", T0), b"ID019;");
        assert_eq!(rig.feed(b"SM0;", T0), b"SM00030;");
        assert_eq!(rig.feed(b"SM;", T0), b"SM00030;");
        assert_eq!(rig.feed(b"ZZ;", T0), b"?;");
        assert_eq!(rig.feed(b"FA123;", T0), b"?;");
        assert_eq!(rig.feed(b"MD9;MD8;", T0), b"?;");
        assert_eq!(rig.state().mode, Mode::RttyR);
    }

    // Tests [@ANCHOR: radioctl_virtual_front_panel]
    #[test]
    fn front_panel_changes_are_reported_only_with_transceive() {
        let mut civ = civ_rig();
        assert_eq!(
            civ.front_panel(PanelChange::Frequency(Hz(14_074_000))),
            vec![0xFE, 0xFE, 0x00, 0x94, 0x00, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD]
        );
        assert_eq!(
            civ.front_panel(PanelChange::Mode(Mode::Cw)),
            vec![0xFE, 0xFE, 0x00, 0x94, 0x01, 0x03, 0x01, 0xFD]
        );
        assert!(civ.front_panel(PanelChange::Ptt(true)).is_empty());
        assert!(civ.state().ptt);
        let mut ken = kenwood_rig();
        assert!(ken
            .front_panel(PanelChange::Frequency(Hz(7_100_000)))
            .is_empty());
        assert!(ken.feed(b"AI1;", T0).is_empty());
        assert_eq!(ken.feed(b"AI;", T0), b"AI1;");
        assert_eq!(
            ken.front_panel(PanelChange::Frequency(Hz(7_100_000))),
            b"FA00007100000;"
        );
        assert_eq!(ken.front_panel(PanelChange::Mode(Mode::Lsb)), b"MD1;");
    }

    // Tests [@ANCHOR: radioctl_virtual_feed]
    #[test]
    fn two_virtual_rigs_share_no_state() {
        let mut first = civ_rig();
        let second = civ_rig();
        let set = [
            0xFE, 0xFE, 0x94, 0xE0, 0x05, 0x00, 0x00, 0x03, 0x07, 0x00, 0xFD,
        ];
        first.feed(&set, T0);
        assert_eq!(first.state().freq_a, Hz(7_030_000));
        assert_eq!(second.state().freq_a, Hz(14_074_000));
    }

    fn kx3_rig() -> VirtualRig {
        let desc = radioctl_core::rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))
            .expect("kx3 file")
            .description;
        VirtualRig::from_description(&desc).expect("rig")
    }

    // Tests [@ANCHOR: radioctl_virtual_elecraft_command]
    // Tests [@ANCHOR: radioctl_virtual_elecraft_level]
    // Tests [@ANCHOR: radioctl_virtual_elecraft_set_meta]
    // Tests [@ANCHOR: radioctl_virtual_elecraft_id]
    // Tests [@ANCHOR: radioctl_virtual_elecraft_power]
    // Tests [@ANCHOR: radioctl_virtual_from_description]
    #[test]
    fn elecraft_commands_follow_the_programmers_reference() {
        let mut rig = kx3_rig();
        assert_eq!(rig.feed(b"fa;", T0), b"FA00014060000;");
        assert!(rig.feed(b"FB00007040000;", T0).is_empty());
        assert_eq!(rig.feed(b"FB;", T0), b"FB00007040000;");
        assert_eq!(rig.feed(b"MD;", T0), b"MD3;");
        assert_eq!(rig.feed(b"ID;", T0), b"ID017;");
        assert_eq!(rig.feed(b"RVM;", T0), b"RVM02.37;");
        assert_eq!(rig.feed(b"TQ;", T0), b"TQ0;");
        assert!(rig.feed(b"TX;", T0).is_empty());
        assert_eq!(rig.feed(b"TQ;", T0), b"TQ1;");
        assert_eq!(rig.feed(b"PO;", T0), b"PO050;");
        assert_eq!(rig.feed(b"SM;", T0), b"SM0000;");
        assert!(rig.feed(b"RX;", T0).is_empty());
        assert_eq!(rig.feed(b"PO;", T0), b"PO000;");
        assert_eq!(rig.feed(b"SM;", T0), b"SM0015;");
        assert_eq!(rig.feed(b"AG;", T0), b"AG030;");
        assert!(rig.feed(b"KS999;", T0).is_empty());
        assert_eq!(rig.feed(b"KS;", T0), b"KS050;");
        assert!(rig.feed(b"KS001;", T0).is_empty());
        assert_eq!(rig.feed(b"KS;", T0), b"KS008;");
        assert_eq!(rig.feed(b"KS5;", T0), b"?;");
        assert!(rig.feed(b"RO-0120;RT1;XT1;FT1;BW0025;", T0).is_empty());
        assert_eq!(rig.state().rit_offset, -120);
        assert!(rig.state().rit && rig.state().xit && rig.state().split);
        assert_eq!(rig.feed(b"BW;", T0), b"BW0025;");
        assert!(rig.feed(b"RC;", T0).is_empty());
        assert_eq!(rig.feed(b"RO;", T0), b"RO+0000;");
        assert!(rig.feed(b"FR0;", T0).is_empty());
        assert_eq!(rig.feed(b"FT;", T0), b"FT0;");
        assert!(rig.feed(b"KY CQ DE K6BP;", T0).is_empty());
        assert_eq!(rig.state().cw_sent, vec!["CQ DE K6BP".to_string()]);
        assert_eq!(rig.feed(b"KY;", T0), b"KY0;");
        assert_eq!(rig.feed(b"SW;", T0), b"SW010;");
        for refused in [
            &b"ZZ;"[..],
            b"RT7;",
            b"RO*0050;",
            b"BW12;",
            b"AI9;",
            b"RV1;",
            b"MD8;",
        ] {
            assert_eq!(rig.feed(refused, T0), b"?;", "{refused:?}");
        }
        assert_eq!(rig.feed(&[0x01, b';'], T0), b"?;");
        assert_eq!(rig.feed(b"RVD;RVA;", T0), b"RVD02.37;RVA99.99;");
        assert_eq!(rig.feed(b"IF;", T0).get(..2), Some(&b"IF"[..]));
        assert_eq!(rig.feed(b"PS;", T0), b"PS1;");
        assert_eq!(rig.feed(b"OM;", T0), b"OM ----------02;");
        assert!(rig.feed(b"PS1;", T0).is_empty());
        assert_eq!(rig.feed(b"K2;K3;", T0), b"K20;K30;");
        assert!(rig.feed(b"K22;K31;", T0).is_empty());
        assert_eq!(rig.feed(b"K2;K3;", T0), b"K22;K31;");
        assert_eq!(rig.feed(b"K24;K32;", T0), b"?;?;");
    }

    // Tests [@ANCHOR: radioctl_virtual_elecraft_if]
    // Tests [@ANCHOR: radioctl_virtual_elecraft_panel]
    // Tests [@ANCHOR: radioctl_virtual_front_panel]
    #[test]
    fn elecraft_auto_info_modes_decide_what_the_panel_sends() {
        let mut rig = kx3_rig();
        assert!(rig
            .front_panel(PanelChange::Frequency(Hz(7_100_000)))
            .is_empty());
        assert_eq!(rig.state().auto_info, 0);
        assert!(!rig.state().transceive);
        assert_eq!(
            rig.feed(b"AI1;", T0),
            b"IF00007100000     +000000 0003000001 ;"
        );
        assert_eq!(rig.state().auto_info, 1);
        let if_after_mode = rig.front_panel(PanelChange::Mode(Mode::Usb));
        assert_eq!(if_after_mode.get(..2), Some(&b"IF"[..]));
        assert_eq!(if_after_mode.get(27 + 2..28 + 2), Some(&b"2"[..]));
        assert!(rig.front_panel(PanelChange::Ptt(true)).is_empty());
        assert!(rig.front_panel(PanelChange::Signal(3)).is_empty());
        assert!(rig.feed(b"AI2;", T0).is_empty());
        assert_eq!(
            rig.front_panel(PanelChange::Frequency(Hz(7_101_000))),
            b"FA00007101000;"
        );
        assert_eq!(rig.front_panel(PanelChange::Mode(Mode::Cw)), b"MD3;");
        let split = rig.front_panel(PanelChange::Split(true));
        assert_eq!(split.get(..2), Some(&b"IF"[..]));
        assert_eq!(split.get(30 + 2..31 + 2), Some(&b"1"[..]));
        let offset = rig.front_panel(PanelChange::RitOffset(-5));
        assert_eq!(offset.get(16 + 2..21 + 2), Some(&b"-0005"[..]));
        assert_eq!(rig.feed(b"AI;", T0), b"AI2;");
    }
}

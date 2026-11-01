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
    CivCommandSet, CivParams, ElecraftParams, Family, KenwoodParams, RigDescription, YaesuParams,
};
use radioctl_core::engine::PARTIAL_FRAME_TIMEOUT;
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::units::{Hz, Level, Meter, Mode};

use crate::attenuation;

use crate::civ::{self, Frame, FrameParser, Parsed};
use crate::civ_scope::{ScopeAnswer, ScopeSim, SCOPE_CMD, SUB_WAVEFORM};
use crate::elecraft;
use crate::kenwood::{self, Token, Tokenizer};
use crate::receiver::VirtualReceiver;
use crate::yaesu_legacy as legacy;

/// Signal level a fresh virtual rig reports (the simulator's raw 30).
const DEFAULT_SIGNAL: u16 = 30;
/// Power meter reading while keyed (the simulator's raw 128).
const KEYED_POWER: u16 = 128;

/// The firmware revision a virtual Elecraft rig reports (the manual's example, not a real KX3's).
const ELECRAFT_FIRMWARE: (u8, u8) = (2, 37);
/// SWR (tenths) a virtual rig reports: 1.0:1.
const DEFAULT_SWR: u16 = 10;
/// Raw CI-V meter deflections (0 to 255) of a virtual Icom rig while keyed: SWR, ALC, COMP and
/// Id read nonzero only while transmitting, Vd at any time. Our own choices, not a radio's.
const KEYED_METERS: [(Meter, u16); 5] = [
    (Meter::Swr, 12),
    (Meter::Alc, 80),
    (Meter::Comp, 0),
    (Meter::Vd, 130),
    (Meter::Id, 90),
];
/// The filter number (1 to 3) a virtual Icom rig reports with its mode.
const CIV_FILTER_NUMBER: u8 = 0x01;

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
    /// How many times a stop-Morse command cleared the buffer (`cw_sent` is emptied too).
    pub cw_stops: u32,
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

    // [@ANCHOR: radioctl_virtual_state_meter]
    /// A raw CI-V meter: the supply voltage at any time, the transmitter meters only while keyed.
    pub fn meter(&self, meter: Meter) -> u16 {
        let keyed = KEYED_METERS
            .iter()
            .find(|(m, _)| *m == meter)
            .map_or(0, |(_, value)| *value);
        if self.ptt || meter == Meter::Vd {
            keyed
        } else {
            0
        }
    }
}

#[path = "virtual_ts440.rs"]
mod virtual_ts440;
#[path = "virtual_ts890.rs"]
mod virtual_ts890;
#[path = "virtual_ts990.rs"]
mod virtual_ts990;

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
    Yaesu {
        params: YaesuParams,
        tokenizer: Tokenizer,
    },
    /// Kenwood handheld/mobile CR-ended lines, kept by the simulator in `kenwood_hh`.
    KenwoodHh { sim: crate::kenwood_hh::HhSim },
    /// Five-byte blocks; `pending` holds the bytes of a block still arriving.
    YaesuLegacy {
        pending: Vec<u8>,
        last_byte_at: Duration,
    },
    /// A receiver or scanner with a line protocol; see `receiver`.
    Receiver(Box<VirtualReceiver>),
}

/// One virtual radio.
#[derive(Debug)]
pub struct VirtualRig {
    desc: RigDescription,
    state: RigState,
    wire: Wire,
    scope: Option<ScopeSim>,
    /// CI-V: true while VFO B is the selected VFO (command 07 01); `03`, `05` and `25 00` act on
    /// the selected VFO and `25 01` on the other.
    civ_b_selected: bool,
    /// Kenwood: the VFO `FR` selected for receive (0 A, 1 B) and `FT` for transmit; split is
    /// when they differ.
    kenwood_rx_vfo: u8,
    kenwood_tx_vfo: u8,
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
            (Family::KenwoodHh, _, _) if desc.kenwood_hh.is_some() => Wire::KenwoodHh {
                sim: crate::kenwood_hh::HhSim::new(
                    desc.kenwood_hh
                        .map_or(radioctl_core::description::HhDialect::Th, |k| k.dialect),
                    desc.default_frequency,
                    desc.mode_code(desc.default_mode).unwrap_or(0),
                ),
            },
            (Family::YaesuLegacy, _, _) if desc.yaesu_legacy.is_some() => Wire::YaesuLegacy {
                pending: Vec::new(),
                last_byte_at: Duration::ZERO,
            },
            (Family::Receiver, _, _) if desc.receiver.is_some() => {
                Wire::Receiver(Box::new(VirtualReceiver::new(desc)?))
            }
            (Family::YaesuNewcat, _, _) if desc.yaesu.is_some() => Wire::Yaesu {
                params: desc.yaesu.clone().unwrap_or(YaesuParams {
                    model_id: 0,
                    frequency_digits: 9,
                }),
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
            cw_stops: 0,
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
        let mut state = state;
        if desc.kenwood.is_some() {
            // Starting values inside the Kenwood ranges (our own choice, not a radio's).
            state.levels.insert(Level::MonitorLevel, 5);
            state.levels.insert(Level::KeyerSpeed, 18);
            state.levels.insert(Level::RfPower, 50);
        }
        Ok(VirtualRig {
            desc: desc.clone(),
            state,
            wire,
            scope: desc.scope.as_ref().map(ScopeSim::new),
            civ_b_selected: false,
            kenwood_rx_vfo: 0,
            kenwood_tx_vfo: 0,
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
                        // The documented stop, `KY <space><EOT>;`, carries a control byte the
                        // client-side tokenizer classes as junk; the radio takes it ("KY", p. 15).
                        Token::Junk(raw) if raw.eq_ignore_ascii_case(b"KY \x04;") => {
                            self.stop_cw();
                            Vec::new()
                        }
                        Token::Junk(_) => b"?;".to_vec(),
                    });
                }
                out
            }
            Wire::KenwoodHh { sim } => sim.feed(bytes, now, &self.desc, &mut self.state),
            Wire::YaesuLegacy {
                pending,
                last_byte_at,
            } => {
                if now.saturating_sub(*last_byte_at) > PARTIAL_FRAME_TIMEOUT {
                    pending.clear();
                }
                *last_byte_at = now;
                pending.extend_from_slice(bytes);
                let mut blocks = Vec::new();
                while pending.len() >= 5 {
                    blocks.push(pending.drain(..5).collect::<Vec<u8>>());
                }
                let mut out = Vec::new();
                for block in blocks {
                    out.extend(self.legacy_block(&block));
                }
                out
            }
            Wire::Receiver(twin) => twin.feed(bytes, now, &mut self.state),
            Wire::Yaesu { tokenizer, .. } => {
                let tokens = tokenizer.feed(bytes, now);
                let mut out = Vec::new();
                for token in tokens {
                    out.extend(match token {
                        Token::Command(text) => self.yaesu_command(&text),
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
            (Wire::Civ { params, .. }, PanelChange::Frequency(hz)) => {
                civ::encode_bcd_freq_len(hz, params.field_len(hz))
                    .map(|bcd| unsolicited_civ(params, 0x00, &bcd))
                    .unwrap_or_default()
            }
            (Wire::Civ { params, .. }, PanelChange::Mode(mode)) => self
                .desc
                .mode_code(mode)
                .map(|code| unsolicited_civ(params, 0x01, &[code, 0x01]))
                .unwrap_or_default(),
            (Wire::Kenwood { .. }, PanelChange::Frequency(hz)) => {
                format!("FA{:011};", hz.0).into_bytes()
            }
            (Wire::Yaesu { params, .. }, PanelChange::Frequency(hz)) => {
                format!("FA{:0w$};", hz.0, w = usize::from(params.frequency_digits)).into_bytes()
            }
            (Wire::Yaesu { .. }, PanelChange::Mode(mode)) => self
                .desc
                .mode_code(mode)
                .map(|code| format!("MD0{};", char::from(code)).into_bytes())
                .unwrap_or_default(),
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
            (0x03, []) => data_reply(0x03, self.civ_freq_field(params, false), &[]),
            (0x05, bcd) => self.civ_set_freq(params, false, bcd),
            (0x1A, [0x35, bcd @ ..]) if params.command_set == CivCommandSet::F8101 => {
                self.civ_set_freq(params, false, bcd)
            }
            (0x1A, [0x34]) if params.command_set == CivCommandSet::F8101 => {
                match self.civ_mode_reply(0x04, &[]) {
                    CivReply::Data(_, bytes) => {
                        CivReply::Data(0x1A, [&[0x34, 0x00][..], &bytes[..1]].concat())
                    }
                    other => other,
                }
            }
            (0x1A, [0x36, 0x00, code]) if params.command_set == CivCommandSet::F8101 => {
                self.civ_mode_set(*code, false)
            }
            (0x25, [0x00]) => data_reply(0x25, self.civ_freq_field(params, false), &[0x00]),
            (0x25, [0x00, bcd @ ..]) => self.civ_set_freq(params, false, bcd),
            (0x25, [0x01]) => data_reply(0x25, self.civ_freq_field(params, true), &[0x01]),
            (0x25, [0x01, bcd @ ..]) => self.civ_set_freq(params, true, bcd),
            (0x07, [0x00]) => {
                self.civ_b_selected = false;
                CivReply::Ok
            }
            (0x07, [0x01]) => {
                self.civ_b_selected = true;
                CivReply::Ok
            }
            (0x07, [0xA0]) => {
                *self.civ_slot(true) = *self.civ_slot(false);
                CivReply::Ok
            }
            (0x07, [0xB0]) => {
                std::mem::swap(&mut self.state.freq_a, &mut self.state.freq_b);
                CivReply::Ok
            }
            (0x0F, []) => CivReply::Data(0x0F, vec![u8::from(self.state.split)]),
            (0x0F, [state @ (0x00 | 0x01)]) => {
                self.state.split = *state == 1;
                CivReply::Ok
            }
            (0x21, [0x00]) => match civ::encode_rit_offset(self.state.rit_offset) {
                Some(bytes) => CivReply::Data(0x21, [&[0x00][..], &bytes[..]].concat()),
                None => CivReply::Ng,
            },
            (0x21, [0x00, bytes @ ..]) => match civ::decode_rit_offset(bytes) {
                Some(hz) => {
                    self.state.rit_offset = hz;
                    CivReply::Ok
                }
                None => CivReply::Ng,
            },
            (0x21, [0x01]) => CivReply::Data(0x21, vec![0x01, u8::from(self.state.rit)]),
            (0x21, [0x01, state @ (0x00 | 0x01)]) => {
                self.state.rit = *state == 1;
                CivReply::Ok
            }
            (0x21, [0x02]) => CivReply::Data(0x21, vec![0x02, u8::from(self.state.xit)]),
            (0x21, [0x02, state @ (0x00 | 0x01)]) => {
                self.state.xit = *state == 1;
                CivReply::Ok
            }
            (0x11, []) => self.civ_front_end_get(Level::Attenuator),
            (0x11, [byte]) => self.civ_front_end_set(Level::Attenuator, *byte),
            (0x16, [0x02]) => self.civ_front_end_get(Level::Preamp),
            (0x16, [0x02, step]) => self.civ_front_end_set(Level::Preamp, *step),
            (0x14, [sub]) => self.civ_level_get(*sub),
            (0x14, [sub, value @ ..]) => self.civ_level_set(*sub, value),
            (0x1A, [0x03]) => self.civ_filter_get(),
            (0x1A, [0x03, index]) => self.civ_filter_set(*index),
            (SCOPE_CMD, rest) => match self.scope.as_mut() {
                Some(scope) => match scope.handle(rest) {
                    ScopeAnswer::Ok => CivReply::Ok,
                    ScopeAnswer::Data(data) => CivReply::Data(SCOPE_CMD, data),
                    ScopeAnswer::Refused => CivReply::Ng,
                },
                None => CivReply::Ng,
            },
            (0x04, []) => self.civ_mode_reply(0x04, &[]),
            // `26 01` is the unselected VFO's mode; the virtual rig keeps one mode for both.
            (0x26, [sel @ (0x00 | 0x01)]) => self.civ_mode_reply(0x26, &[*sel]),
            (0x06, [code, ..]) => self.civ_mode_set(*code, false),
            (0x26, [0x00 | 0x01, code]) => self.civ_mode_set(*code, false),
            (0x26, [0x00 | 0x01, code, data, rest @ ..])
                if rest.len() <= 1 && rest.iter().all(|f| (1..=3).contains(f)) =>
            {
                self.civ_mode_set(*code, *data != 0)
            }
            (0x1A, [0x06]) => CivReply::Data(
                0x1A,
                vec![
                    0x06,
                    u8::from(civ::base_mode(self.state.mode).1),
                    CIV_FILTER_NUMBER,
                ],
            ),
            (0x1A, [0x06, data, rest @ ..]) if rest.len() <= 1 => {
                let (base, _) = civ::base_mode(self.state.mode);
                match self.desc.mode_code(base) {
                    Some(code) => self.civ_mode_set(code, *data != 0),
                    None => CivReply::Ng,
                }
            }
            (0x1C, [0x00]) => CivReply::Data(0x1C, vec![0x00, u8::from(self.state.ptt)]),
            (0x1C, [0x00, state]) => {
                self.state.ptt = *state != 0;
                CivReply::Ok
            }
            (0x17, [0xFF]) => {
                self.stop_cw();
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
            (0x15, [sub]) => match civ::sub_meter(*sub) {
                Some(meter) => meter_reply(*sub, self.state.meter(meter)),
                None => CivReply::Ng,
            },
            _ => CivReply::Ng,
        }
    }

    // [@ANCHOR: radioctl_virtual_civ_slot]
    /// The frequency of the selected VFO, or of the other one when `other` is true.
    fn civ_slot(&mut self, other: bool) -> &mut Hz {
        if self.civ_b_selected != other {
            &mut self.state.freq_b
        } else {
            &mut self.state.freq_a
        }
    }

    // [@ANCHOR: radioctl_virtual_civ_freq_field]
    /// The VFO's frequency as the model's BCD field.
    fn civ_freq_field(&mut self, params: &CivParams, other: bool) -> Option<Vec<u8>> {
        let hz = *self.civ_slot(other);
        civ::encode_bcd_freq_len(hz, params.field_len(hz))
    }

    // [@ANCHOR: radioctl_virtual_civ_set_freq]
    fn civ_set_freq(&mut self, params: &CivParams, other: bool, bcd: &[u8]) -> CivReply {
        match civ::decode_bcd_freq(bcd) {
            Some(hz) if bcd.len() == params.field_len(hz) => {
                *self.civ_slot(other) = hz;
                CivReply::Ok
            }
            _ => CivReply::Ng,
        }
    }

    // [@ANCHOR: radioctl_virtual_civ_front_end_get]
    /// `11` (attenuator, one BCD byte of decibels) and `16 02` (preamplifier step) reads.
    fn civ_front_end_get(&self, level: Level) -> CivReply {
        match attenuation::virtual_get(&self.desc, &self.state.levels, level) {
            Some(value) if level == Level::Attenuator => {
                CivReply::Data(0x11, vec![bcd_byte(u8::try_from(value).unwrap_or(0))])
            }
            Some(value) => CivReply::Data(0x16, vec![0x02, u8::try_from(value).unwrap_or(0)]),
            None => CivReply::Ng,
        }
    }

    // [@ANCHOR: radioctl_virtual_civ_front_end_set]
    fn civ_front_end_set(&mut self, level: Level, byte: u8) -> CivReply {
        let value = if level == Level::Attenuator {
            let (high, low) = (byte >> 4, byte & 15);
            if high > 9 || low > 9 {
                return CivReply::Ng;
            }
            u16::from(high) * 10 + u16::from(low)
        } else {
            u16::from(byte)
        };
        if attenuation::virtual_set(&self.desc, &mut self.state.levels, level, value) {
            CivReply::Ok
        } else {
            CivReply::Ng
        }
    }

    // [@ANCHOR: radioctl_virtual_civ_level]
    fn civ_level_get(&self, sub: u8) -> CivReply {
        let value = civ_level_of(sub)
            .filter(|level| !self.desc.levels.lacks(*level))
            .and_then(|level| self.state.levels.get(&level));
        match value {
            Some(value) => {
                let mut data = vec![sub];
                data.extend(civ::encode_bcd_meter(*value));
                CivReply::Data(0x14, data)
            }
            None => CivReply::Ng,
        }
    }

    // [@ANCHOR: radioctl_virtual_civ_level_set]
    fn civ_level_set(&mut self, sub: u8, value: &[u8]) -> CivReply {
        match (civ_level_of(sub), civ::decode_bcd_meter(value)) {
            (Some(level), Some(raw)) if raw <= 255 && !self.desc.levels.lacks(level) => {
                self.state.levels.insert(level, raw);
                CivReply::Ok
            }
            _ => CivReply::Ng,
        }
    }

    // [@ANCHOR: radioctl_virtual_civ_filter]
    fn civ_filter_get(&self) -> CivReply {
        let am = self.state.mode == Mode::Am;
        let index = u32::try_from(self.state.filter_width.0)
            .ok()
            .and_then(|hz| civ::filter_hz_to_index(am, hz));
        match index {
            Some(index) => CivReply::Data(0x1A, vec![0x03, bcd_byte(index)]),
            None => CivReply::Ng,
        }
    }

    // [@ANCHOR: radioctl_virtual_civ_filter_set]
    fn civ_filter_set(&mut self, byte: u8) -> CivReply {
        let (high, low) = (byte >> 4, byte & 15);
        let am = self.state.mode == Mode::Am;
        let width = (high <= 9 && low <= 9)
            .then(|| civ::filter_index_to_hz(am, high * 10 + low))
            .flatten();
        match width {
            Some(hz) => {
                self.state.filter_width = Hz(u64::from(hz));
                CivReply::Ok
            }
            None => CivReply::Ng,
        }
    }

    // [@ANCHOR: radioctl_virtual_scope_sweep]
    /// The bytes the rig sends on its own for one scope sweep of the Main receiver at the VFO A
    /// frequency, as broadcast frames; empty when the model has no scope, output is off or the
    /// scope is held.
    pub fn scope_sweep(&mut self) -> Vec<u8> {
        let (Wire::Civ { params, .. }, Some(scope)) = (&self.wire, self.scope.as_mut()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for segment in scope.sweep(0, self.state.freq_a) {
            let mut data = vec![SUB_WAVEFORM];
            data.extend(segment);
            out.extend(unsolicited_civ(params, SCOPE_CMD, &data));
        }
        out
    }

    // [@ANCHOR: radioctl_virtual_civ_mode_reply]
    /// `04` answers `code filter`; `26 <sel>` answers `code data filter`. A data mode is its
    /// sideband's code with the DATA byte set; `04` cannot show the flag, so it shows the
    /// sideband. Hamlib reads the filter byte as the DATA flag if it is missing (found by the
    /// rigctld oracle test).
    fn civ_mode_reply(&self, cmd: u8, prefix: &[u8]) -> CivReply {
        let (base, data) = civ::base_mode(self.state.mode);
        let Some(code) = self.desc.mode_code(base) else {
            return CivReply::Ng;
        };
        let mut reply = prefix.to_vec();
        reply.push(code);
        if cmd == 0x26 {
            reply.push(u8::from(data));
        }
        reply.push(CIV_FILTER_NUMBER);
        CivReply::Data(cmd, reply)
    }

    // [@ANCHOR: radioctl_virtual_civ_mode_set]
    /// Sets the mode from a code and the DATA flag. `06` carries no flag, so it lands on the
    /// plain mode (a choice: whether a real radio keeps DATA across `06` is a bench question).
    /// DATA on a mode with no data variant in radioctl (AM, FM) is refused.
    fn civ_mode_set(&mut self, code: u8, data: bool) -> CivReply {
        let plain = self.desc.code_mode(code).map(|mode| civ::base_mode(mode).0);
        match plain.and_then(|base| civ::mode_with_data(base, data)) {
            Some(mode) => {
                self.state.mode = mode;
                CivReply::Ok
            }
            None => CivReply::Ng,
        }
    }

    // [@ANCHOR: radioctl_virtual_stop_cw]
    /// A stop-Morse command arrived: the text buffer is emptied and the stop counted.
    fn stop_cw(&mut self) {
        self.state.cw_sent.clear();
        self.state.cw_stops = self.state.cw_stops.saturating_add(1);
    }

    // [@ANCHOR: radioctl_virtual_kenwood_command]
    fn kenwood_command(&mut self, text: &str) -> Vec<u8> {
        if self.is_ts890() {
            return self.ts890_command(text);
        }
        if self.is_ts440() {
            return self.ts440_command(text);
        }
        if self.is_ts990() {
            if let Some(reply) = self.ts990_command(text) {
                return reply;
            }
        }
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
            ("KY", "0") => {
                self.stop_cw();
                None
            }
            ("ID", "") => Some(format!("ID{:03};", self.kenwood_id())),
            ("AI", "") => Some(format!("AI{};", u8::from(self.state.transceive))),
            ("AI", flag) => {
                self.state.transceive = flag != "0";
                None
            }
            ("SM", "" | "0") => Some(format!("SM0{:04};", self.state.signal)),
            ("PS", "") => Some("PS1;".to_string()),
            ("PS" | "VX", _) => None,
            // TS-590 firmware string and DATA mode (guide pages 12 and 6); our own values.
            ("FV", "") => Some("FV1.08;".to_string()),
            ("DA", "") => Some("DA0;".to_string()),
            ("DA", "0") => None,
            ("IF", "") => Some(self.kenwood_if()),
            ("FR", "") => Some(format!("FR{};", self.kenwood_rx_vfo)),
            ("FR", "0" | "1") => self.kenwood_select_vfo(arg, true),
            ("FT", "") => Some(format!("FT{};", self.kenwood_tx_vfo)),
            ("FT", "0" | "1") => self.kenwood_select_vfo(arg, false),
            ("RT", "") => Some(format!("RT{};", u8::from(self.state.rit))),
            ("RT", flag @ ("0" | "1")) => {
                self.state.rit = flag == "1";
                None
            }
            ("XT", "") => Some(format!("XT{};", u8::from(self.state.xit))),
            ("XT", flag @ ("0" | "1")) => {
                self.state.xit = flag == "1";
                None
            }
            ("RC", "") => {
                self.state.rit_offset = 0;
                None
            }
            ("RU" | "RD", digits) => self.kenwood_move_offset(name == "RU", digits),
            ("AG" | "RG" | "MG" | "ML" | "PC" | "KS", _) => self.kenwood_level(name, arg),
            ("RA" | "PA", _) => {
                attenuation::virtual_kenwood(&self.desc, &mut self.state.levels, name, arg)
            }
            _ => Some("?;".to_string()),
        };
        reply.map(String::into_bytes).unwrap_or_default()
    }

    // [@ANCHOR: radioctl_virtual_legacy_block]
    /// One five-byte block: reads answer, settings change state, anything else is ignored (the
    /// radio is silent).
    fn legacy_block(&mut self, block: &[u8]) -> Vec<u8> {
        let high_band = self
            .desc
            .yaesu_legacy
            .as_ref()
            .is_some_and(|y| y.high_band_flag);
        let [a, b, c, d, opcode] = block else {
            return Vec::new();
        };
        match *opcode {
            legacy::OP_READ_FREQUENCY => {
                let Some(mode) = self.desc.mode_code(self.state.mode) else {
                    return Vec::new();
                };
                let mut out = legacy::encode_frequency(self.state.freq_a, high_band)
                    .map(|bcd| bcd.to_vec())
                    .unwrap_or_default();
                out.push(mode);
                out
            }
            legacy::OP_READ_RX => vec![(self.state.signal.min(15)) as u8],
            legacy::OP_READ_TX => {
                // Power meter nibble, split (bit 5 is 0 for ON), PTT (bit 7 clear transmits).
                let power = (self.state.power() / 16).min(15) as u8;
                let split_bit = if self.state.split { 0 } else { 0x20 };
                let ptt_bit = if self.state.ptt { 0 } else { 0x80 };
                vec![power | split_bit | ptt_bit]
            }
            legacy::OP_SET_FREQUENCY => {
                if let Some(hz) = legacy::decode_frequency(&[*a, *b, *c, *d], high_band) {
                    self.state.freq_a = hz;
                }
                Vec::new()
            }
            legacy::OP_SET_MODE => {
                if let Some(mode) = self.desc.code_mode(*a) {
                    self.state.mode = mode;
                }
                Vec::new()
            }
            legacy::OP_PTT_ON => {
                self.state.ptt = true;
                Vec::new()
            }
            legacy::OP_PTT_OFF => {
                self.state.ptt = false;
                Vec::new()
            }
            legacy::OP_SPLIT_ON => {
                self.state.split = true;
                Vec::new()
            }
            legacy::OP_SPLIT_OFF => {
                self.state.split = false;
                Vec::new()
            }
            legacy::OP_CLAR_ON => {
                self.state.rit = true;
                Vec::new()
            }
            legacy::OP_CLAR_OFF => {
                self.state.rit = false;
                Vec::new()
            }
            legacy::OP_CLAR_FREQUENCY => {
                // Two BCD bytes of 10 Hz, at most 9.99 kHz (the engine's MAX_CLAR_HZ). A nibble
                // above 9, or a value past the range, is not BCD the radio documents: ignored, as
                // a setting the radio does not take. Computed in i32: the naive i16 arithmetic
                // overflowed on non-BCD bytes (found by `parser_props.rs`).
                let bcd = |byte: u8| {
                    let (high, low) = (byte >> 4, byte & 15);
                    (high <= 9 && low <= 9).then(|| i32::from(high) * 10 + i32::from(low))
                };
                if let (Some(high), Some(low)) = (bcd(*c), bcd(*d)) {
                    let hz = (high * 100 + low) * 10;
                    if let Ok(hz) = i16::try_from(hz) {
                        if hz <= legacy::MAX_CLAR_HZ_I16 {
                            self.state.rit_offset = if *a == 0 { hz } else { -hz };
                        }
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    // [@ANCHOR: radioctl_virtual_yaesu_command]
    fn yaesu_command(&mut self, text: &str) -> Vec<u8> {
        let (name, arg) = (text.get(..2).unwrap_or(""), text.get(2..).unwrap_or(""));
        let id = match &self.wire {
            Wire::Yaesu { params, .. } => params.model_id,
            _ => 0,
        };
        let reply = match (name, arg) {
            ("FA", "") => Some(format!(
                "FA{:0w$};",
                self.state.freq_a.0,
                w = self.yaesu_digits()
            )),
            ("FB", "") => Some(format!(
                "FB{:0w$};",
                self.state.freq_b.0,
                w = self.yaesu_digits()
            )),
            ("FA" | "FB", digits) => self.yaesu_set_frequency(name == "FA", digits),
            ("MD", "0") => self
                .desc
                .mode_code(self.state.mode)
                .map(|code| format!("MD0{};", char::from(code))),
            ("MD", rest) if rest.len() == 2 && rest.starts_with('0') => {
                let mode = rest.as_bytes().get(1).and_then(|c| self.desc.code_mode(*c));
                match mode {
                    Some(mode) => {
                        self.state.mode = mode;
                        None
                    }
                    None => Some("?;".to_string()),
                }
            }
            ("TX", "") => Some(format!("TX{};", u8::from(self.state.ptt))),
            ("TX", flag @ ("0" | "1")) => {
                self.state.ptt = flag == "1";
                None
            }
            ("FT", "") => Some(format!("FT{};", u8::from(self.state.split))),
            ("FT", flag @ ("2" | "3")) => {
                self.state.split = flag == "3";
                None
            }
            ("RT", "") => Some(format!("RT{};", u8::from(self.state.rit))),
            ("RT", flag @ ("0" | "1")) => {
                self.state.rit = flag == "1";
                None
            }
            ("XT", "") => Some(format!("XT{};", u8::from(self.state.xit))),
            ("XT", flag @ ("0" | "1")) => {
                self.state.xit = flag == "1";
                None
            }
            ("RC", "") => {
                self.state.rit_offset = 0;
                None
            }
            ("RU" | "RD", digits) => self.yaesu_move_offset(name == "RU", digits),
            ("SM", "0") => Some(format!("SM0{:03};", self.state.signal.min(255))),
            ("RM", "5") => {
                let power = self.state.power().min(255);
                let tail = if id == 670 { "" } else { "000" };
                Some(format!("RM5{power:03}{tail};"))
            }
            ("IF", "") => Some(self.yaesu_if()),
            ("ID", "") => Some(format!("ID{id:04};")),
            ("AI", "") => Some(format!("AI{};", u8::from(self.state.transceive))),
            ("AI", flag @ ("0" | "1")) => {
                self.state.transceive = flag == "1";
                None
            }
            // Read by Hamlib with every mode read; fixed answers (guide: PS, SH width code 00 =
            // default, NA narrow off), settings accepted and not modelled.
            ("PS", "") => Some("PS1;".to_string()),
            ("SH", "0") if id == 670 => Some("SH000;".to_string()),
            ("SH", "0") => Some("SH0000;".to_string()),
            ("SH", rest)
                if rest.starts_with('0') && rest.len() == if id == 670 { 3 } else { 4 } =>
            {
                None
            }
            ("NA", "0") => Some("NA00;".to_string()),
            ("NA", "00" | "01") => None,
            // FTDX10 and FTDX101 (guide: VS, ST, RF): VFO select, split and roofing filter are
            // answered and the settings accepted; VFO selection is not modelled.
            ("VS", "") if id != 670 => Some("VS0;".to_string()),
            ("VS", "0" | "1") if id != 670 => None,
            ("ST", "") if id != 670 => Some(format!("ST{};", u8::from(self.state.split))),
            ("ST", flag @ ("0" | "1" | "2")) if id != 670 => {
                self.state.split = flag != "0";
                None
            }
            ("RF", "0") if id != 670 => Some("RF01;".to_string()),
            ("RF", rest) if id != 670 && rest.len() == 2 && rest.starts_with('0') => None,
            ("AG" | "RG" | "MG" | "PC" | "KS", _) => self.yaesu_level(name, arg),
            ("RA" | "PA", _) => {
                attenuation::virtual_yaesu(&self.desc, &mut self.state.levels, name, arg)
            }
            _ => Some("?;".to_string()),
        };
        reply.map(String::into_bytes).unwrap_or_default()
    }

    // [@ANCHOR: radioctl_virtual_yaesu_digits]
    /// Digits of the frequency fields of this Yaesu model (9, or 8 on the older ones).
    fn yaesu_digits(&self) -> usize {
        match &self.wire {
            Wire::Yaesu { params, .. } => usize::from(params.frequency_digits),
            _ => 9,
        }
    }

    // [@ANCHOR: radioctl_virtual_yaesu_set_frequency]
    fn yaesu_set_frequency(&mut self, vfo_a: bool, digits: &str) -> Option<String> {
        let width = self.yaesu_digits();
        let Some(hz) = kenwood::parse_digits(digits).filter(|_| digits.len() == width) else {
            return Some("?;".to_string());
        };
        if vfo_a {
            self.state.freq_a = Hz(hz);
        } else {
            self.state.freq_b = Hz(hz);
        }
        None
    }

    // [@ANCHOR: radioctl_virtual_yaesu_if]
    /// The `IF` answer: channel, VFO-A frequency, clarifier, flags, mode and fixed fields.
    fn yaesu_if(&self) -> String {
        let offset = self.state.rit_offset;
        let sign = if offset < 0 { '-' } else { '+' };
        let mode = self.desc.mode_code(self.state.mode).map_or('0', char::from);
        format!(
            "IF001{:0w$}{sign}{:04}{}{}{mode}00000;",
            self.state.freq_a.0,
            offset.unsigned_abs(),
            u8::from(self.state.rit),
            u8::from(self.state.xit),
            w = self.yaesu_digits(),
        )
    }

    // [@ANCHOR: radioctl_virtual_yaesu_move_offset]
    fn yaesu_move_offset(&mut self, up: bool, digits: &str) -> Option<String> {
        let Some(hz) = kenwood::parse_digits(digits).filter(|_| digits.len() == 4) else {
            return Some("?;".to_string());
        };
        let hz = i32::try_from(hz).unwrap_or(i32::MAX);
        let current = i32::from(self.state.rit_offset);
        let moved = if up { current + hz } else { current - hz };
        match i16::try_from(moved) {
            Ok(value) if value.unsigned_abs() <= 9_999 => {
                self.state.rit_offset = value;
                None
            }
            _ => Some("?;".to_string()),
        }
    }

    // [@ANCHOR: radioctl_virtual_yaesu_level]
    /// A newcat level command: `AG`/`RG` carry a leading `0`; the others are three digits.
    fn yaesu_level(&mut self, name: &str, arg: &str) -> Option<String> {
        let (level, leading_zero, min, max) = match name {
            "AG" => (Level::AfGain, true, 0, 255),
            "RG" => (Level::RfGain, true, 0, 255),
            "MG" => (Level::MicGain, false, 0, 100),
            "KS" => (Level::KeyerSpeed, false, 4, 60),
            _ => (Level::RfPower, false, 5, 100),
        };
        let reading = if leading_zero {
            arg == "0"
        } else {
            arg.is_empty()
        };
        let prefix = if leading_zero { "0" } else { "" };
        if reading {
            let value = self.state.levels.get(&level).copied().unwrap_or(0);
            return Some(format!("{name}{prefix}{value:03};"));
        }
        let number = if leading_zero {
            arg.strip_prefix('0')
        } else {
            Some(arg)
        };
        let value = number
            .filter(|n| n.len() == 3)
            .and_then(kenwood::parse_digits)
            .and_then(|v| u16::try_from(v).ok())
            .filter(|v| (min..=max).contains(v));
        match value {
            Some(value) => {
                self.state.levels.insert(level, value);
                None
            }
            None => Some("?;".to_string()),
        }
    }

    // [@ANCHOR: radioctl_virtual_kenwood_if]
    /// The `IF` answer. The TS-590S (ID 021) and TS-590SG (023) put five spaces and a signed
    /// 4-digit offset where the TS-2000 has a 4-digit step and a signed 5-digit offset.
    fn kenwood_if(&self) -> String {
        let id = self.kenwood_id();
        let offset = self.state.rit_offset;
        let sign = if offset < 0 { '-' } else { '+' };
        let region = if self.is_ts990() {
            // P2 is five spaces and P3 a sign and four digits (PC command reference, IF).
            format!("     {sign}{:04}", offset.unsigned_abs())
        } else if self.is_ts440() {
            // P2 is five characters and P3 a sign and four digits (IC-10 manual, page 18).
            format!("00000{sign}{:04}", offset.unsigned_abs())
        } else if id == 21 || id == 23 {
            format!("     {sign}{:04}", offset.unsigned_abs())
        } else {
            format!("0000{sign}{:05}", offset.unsigned_abs())
        };
        let frequency = if self.kenwood_rx_vfo == 0 {
            self.state.freq_a
        } else {
            self.state.freq_b
        };
        let mode = self.desc.mode_code(self.state.mode).map_or('0', char::from);
        format!(
            "IF{:011}{region}{}{}000{}{}{}0{}0000;",
            frequency.0,
            u8::from(self.state.rit),
            u8::from(self.state.xit),
            // P8 is always 0 on the TS-990S: the transmit state is not in IF.
            u8::from(self.state.ptt && !self.is_ts990()),
            mode,
            self.kenwood_rx_vfo,
            u8::from(self.state.split),
        )
    }

    // [@ANCHOR: radioctl_virtual_kenwood_select_vfo]
    fn kenwood_select_vfo(&mut self, arg: &str, receive: bool) -> Option<String> {
        let vfo = u8::from(arg == "1");
        if receive {
            self.kenwood_rx_vfo = vfo;
        } else {
            self.kenwood_tx_vfo = vfo;
        }
        self.state.split = self.kenwood_rx_vfo != self.kenwood_tx_vfo;
        None
    }

    // [@ANCHOR: radioctl_virtual_kenwood_move_offset]
    fn kenwood_move_offset(&mut self, up: bool, digits: &str) -> Option<String> {
        let Some(hz) = kenwood::parse_digits(digits).filter(|_| digits.len() == 5) else {
            return Some("?;".to_string());
        };
        let hz = i32::try_from(hz).unwrap_or(i32::MAX);
        let current = i32::from(self.state.rit_offset);
        let moved = if up { current + hz } else { current - hz };
        match i16::try_from(moved) {
            Ok(value) if value.unsigned_abs() <= 9_999 => {
                self.state.rit_offset = value;
                None
            }
            _ => Some("?;".to_string()),
        }
    }

    // [@ANCHOR: radioctl_virtual_kenwood_level]
    /// A Kenwood level command: read when `arg` is empty (the lone `0` for `AG`), else set.
    fn kenwood_level(&mut self, name: &str, arg: &str) -> Option<String> {
        let (level, main_digit, min, max) = match name {
            "AG" => (Level::AfGain, true, 0, 255),
            "RG" => (Level::RfGain, false, 0, 255),
            "MG" => (Level::MicGain, false, 0, 100),
            "ML" => (Level::MonitorLevel, false, 0, 9),
            "KS" => (Level::KeyerSpeed, false, 10, 60),
            _ => (Level::RfPower, false, 5, 100),
        };
        // `AG` needs its P1 (the lone `0` reads it, `0nnn` sets it); the others read when bare.
        let reading = if main_digit {
            arg == "0"
        } else {
            arg.is_empty()
        };
        let prefix = if main_digit { "0" } else { "" };
        if reading {
            let value = self.state.levels.get(&level).copied().unwrap_or(0);
            return Some(format!("{name}{prefix}{value:03};"));
        }
        let number = if main_digit {
            arg.strip_prefix('0')
        } else {
            Some(arg)
        };
        let Some(number) = number else {
            return Some("?;".to_string());
        };
        let value = kenwood::parse_digits(number)
            .filter(|_| number.len() == 3)
            .and_then(|v| u16::try_from(v).ok())
            .filter(|v| (min..=max).contains(v));
        match value {
            Some(value) => {
                self.state.levels.insert(level, value);
                None
            }
            None => Some("?;".to_string()),
        }
    }

    // [@ANCHOR: radioctl_virtual_kenwood_id]
    fn kenwood_id(&self) -> u16 {
        match &self.wire {
            Wire::Kenwood { params, .. } => params.model_id,
            Wire::Civ { .. }
            | Wire::Elecraft { .. }
            | Wire::Yaesu { .. }
            | Wire::KenwoodHh { .. }
            | Wire::YaesuLegacy { .. }
            | Wire::Receiver(_) => 0,
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
            ("KY", " \u{4}") => {
                self.stop_cw();
                None
            }
            ("KY", _) => {
                let sent = text.get(3..).unwrap_or("").to_string();
                self.state.cw_sent.push(sent);
                None
            }
            (prefix @ ("RA" | "PA"), data) => {
                attenuation::virtual_elecraft(&self.desc, &mut self.state.levels, prefix, data)
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

// [@ANCHOR: radioctl_virtual_civ_level_of]
/// The level behind a `14` sub-command (AF 01, RF 02, power 0A, mic 0B, monitor 15).
fn civ_level_of(sub: u8) -> Option<Level> {
    match sub {
        0x01 => Some(Level::AfGain),
        0x02 => Some(Level::RfGain),
        0x0A => Some(Level::RfPower),
        0x0B => Some(Level::MicGain),
        0x15 => Some(Level::MonitorLevel),
        _ => None,
    }
}

fn bcd_byte(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

// [@ANCHOR: radioctl_virtual_data_reply]
fn data_reply(cmd: u8, bcd: Option<Vec<u8>>, prefix: &[u8]) -> CivReply {
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
    // Tests [@ANCHOR: radioctl_virtual_stop_cw]
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
        assert!(rig.feed(b"KY \x04;", T0).is_empty());
        assert!(rig.state().cw_sent.is_empty());
        assert_eq!(rig.state().cw_stops, 1);
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

    fn shipped_rig(id: &str) -> VirtualRig {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        let loaded = radioctl_core::rigfile::parse(&text).expect("loads");
        VirtualRig::from_description(&loaded.description).expect("rig")
    }

    // Tests [@ANCHOR: radioctl_virtual_state_meter]
    // Tests [@ANCHOR: radioctl_virtual_civ_dispatch]
    #[test]
    fn civ_raw_meters_read_zero_unkeyed_and_the_supply_voltage_always() {
        let mut rig = shipped_rig("icom-ic-7300");
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x15, 0x12]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x15, 0x12, 0x00, 0x00, 0xFD]
        );
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x15, 0x15]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x15, 0x15, 0x01, 0x30, 0xFD]
        );
        rig.front_panel(PanelChange::Ptt(true));
        assert_eq!(rig.state().meter(Meter::Swr), 12);
        assert_eq!(rig.state().meter(Meter::Alc), 80);
        assert_eq!(rig.state().meter(Meter::Id), 90);
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x15, 0x13]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x15, 0x13, 0x00, 0x80, 0xFD]
        );
        assert!(civ_ask(&mut rig, 0x94, &[0x15, 0x17]).ends_with(&[0xFA, 0xFD]));
    }

    // Tests [@ANCHOR: radioctl_virtual_civ_mode_reply]
    // Tests [@ANCHOR: radioctl_virtual_civ_mode_set]
    // Tests [@ANCHOR: radioctl_virtual_civ_dispatch]
    #[test]
    fn civ_data_modes_are_the_sideband_plus_the_data_flag_of_26() {
        let mut rig = shipped_rig("icom-ic-7300");
        let ok = vec![0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD];
        // Hamlib's form: 26 00 <mode> <data> <filter>.
        assert_eq!(civ_ask(&mut rig, 0x94, &[0x26, 0x00, 0x01, 0x01, 0x01]), ok);
        assert_eq!(rig.state().mode, Mode::Data);
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x26, 0x00]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x26, 0x00, 0x01, 0x01, 0x01, 0xFD]
        );
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x04]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x04, 0x01, 0x01, 0xFD],
            "04 cannot show the flag"
        );
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x26, 0x01]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x26, 0x01, 0x01, 0x01, 0x01, 0xFD],
            "the unselected VFO shares the one mode"
        );
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x1A, 0x06]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x1A, 0x06, 0x01, 0x01, 0xFD]
        );
        // Without the filter byte, and the older 1A 06 form.
        assert_eq!(civ_ask(&mut rig, 0x94, &[0x26, 0x00, 0x00, 0x01]), ok);
        assert_eq!(rig.state().mode, Mode::DataR);
        assert_eq!(civ_ask(&mut rig, 0x94, &[0x1A, 0x06, 0x00]), ok);
        assert_eq!(rig.state().mode, Mode::Lsb);
        assert_eq!(civ_ask(&mut rig, 0x94, &[0x1A, 0x06, 0x01, 0x02]), ok);
        assert_eq!(rig.state().mode, Mode::DataR);
        // 06 carries no flag: the plain mode.
        assert_eq!(civ_ask(&mut rig, 0x94, &[0x06, 0x01]), ok);
        assert_eq!(rig.state().mode, Mode::Usb);
        // DATA on a mode with no data variant, or a filter outside 1 to 3, is refused.
        let ng = vec![0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD];
        assert_eq!(civ_ask(&mut rig, 0x94, &[0x26, 0x00, 0x02, 0x01, 0x01]), ng);
        assert_eq!(civ_ask(&mut rig, 0x94, &[0x26, 0x00, 0x01, 0x01, 0x04]), ng);
        assert_eq!(civ_ask(&mut rig, 0x94, &[0x26, 0x00, 0x03, 0x00, 0x02]), ok);
        assert_eq!(rig.state().mode, Mode::Cw);
    }

    fn civ_ask(rig: &mut VirtualRig, address: u8, body: &[u8]) -> Vec<u8> {
        let mut wire = vec![0xFE, 0xFE, address, 0xE0];
        wire.extend_from_slice(body);
        wire.push(0xFD);
        rig.feed(&wire, Duration::from_secs(1))
    }

    // Tests [@ANCHOR: radioctl_virtual_civ_level]
    // Tests [@ANCHOR: radioctl_virtual_civ_level_set]
    // Tests [@ANCHOR: radioctl_virtual_civ_level_of]
    // Tests [@ANCHOR: radioctl_virtual_civ_dispatch]
    #[test]
    fn civ_levels_split_rit_and_vfo_b_follow_the_description() {
        let mut rig = shipped_rig("icom-ic-705");
        let ok = vec![0xFE, 0xFE, 0xE0, 0xA4, 0xFB, 0xFD];
        let ng = vec![0xFE, 0xFE, 0xE0, 0xA4, 0xFA, 0xFD];
        assert_eq!(civ_ask(&mut rig, 0xA4, &[0x14, 0x0B, 0x01, 0x99]), ok);
        assert_eq!(rig.state().levels.get(&Level::MicGain), Some(&199));
        assert_eq!(
            civ_ask(&mut rig, 0xA4, &[0x14, 0x0B, 0x03, 0x00]),
            ng,
            "over 255"
        );
        assert_eq!(
            civ_ask(&mut rig, 0xA4, &[0x14, 0x0C]),
            ng,
            "keyer speed is a gap"
        );
        assert_eq!(
            civ_ask(&mut rig, 0xA4, &[0x14, 0x0B, 0x01, 0x9A]),
            ng,
            "not BCD"
        );
        assert_eq!(
            civ_ask(&mut rig, 0xA4, &[0x14, 0x0B]),
            vec![0xFE, 0xFE, 0xE0, 0xA4, 0x14, 0x0B, 0x01, 0x99, 0xFD]
        );
        assert_eq!(civ_ask(&mut rig, 0xA4, &[0x0F, 0x01]), ok);
        assert!(rig.state().split);
        assert_eq!(civ_ask(&mut rig, 0xA4, &[0x0F, 0x02]), ng);
        assert_eq!(civ_ask(&mut rig, 0xA4, &[0x21, 0x00, 0x50, 0x01, 0x01]), ok);
        assert_eq!(rig.state().rit_offset, -150);
        assert_eq!(civ_ask(&mut rig, 0xA4, &[0x21, 0x01, 0x01]), ok);
        assert_eq!(civ_ask(&mut rig, 0xA4, &[0x21, 0x02, 0x01]), ok);
        assert!(rig.state().rit && rig.state().xit);
        assert_eq!(
            civ_ask(&mut rig, 0xA4, &[0x25, 0x01, 0x00, 0x00, 0x03, 0x07, 0x00]),
            ok
        );
        assert_eq!(rig.state().freq_b, Hz(7_030_000));
        assert_eq!(civ_ask(&mut rig, 0xA4, &[0x25, 0x01, 0x00]), ng);
    }

    // Tests [@ANCHOR: radioctl_virtual_civ_slot]
    // Tests [@ANCHOR: radioctl_virtual_civ_set_freq]
    #[test]
    fn civ_vfo_selection_swaps_what_the_plain_commands_act_on() {
        let mut rig = shipped_rig("icom-ic-7300");
        let ok = vec![0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD];
        civ_ask(&mut rig, 0x94, &[0x25, 0x01, 0x00, 0x00, 0x03, 0x07, 0x00]);
        assert_eq!(civ_ask(&mut rig, 0x94, &[0x07, 0x01]), ok);
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x03]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x03, 0x00, 0x00, 0x03, 0x07, 0x00, 0xFD],
            "plain read now sees VFO B"
        );
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x05, 0x00, 0x00, 0x05, 0x07, 0x00]),
            ok
        );
        assert_eq!(
            (rig.state().freq_a, rig.state().freq_b),
            (Hz(14_074_000), Hz(7_050_000))
        );
        assert_eq!(civ_ask(&mut rig, 0x94, &[0x07, 0xB0]), ok);
        assert_eq!(
            (rig.state().freq_a, rig.state().freq_b),
            (Hz(7_050_000), Hz(14_074_000))
        );
        assert_eq!(civ_ask(&mut rig, 0x94, &[0x07, 0xA0]), ok);
        assert_eq!(rig.state().freq_a, rig.state().freq_b);
        assert_eq!(civ_ask(&mut rig, 0x94, &[0x07, 0x00]), ok);
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x07, 0x55]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD]
        );
    }

    // Tests [@ANCHOR: radioctl_virtual_civ_filter]
    // Tests [@ANCHOR: radioctl_virtual_civ_filter_set]
    #[test]
    fn civ_filter_index_follows_the_mode() {
        let mut rig = shipped_rig("icom-ic-7300");
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x1A, 0x03]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x1A, 0x03, 0x09, 0xFD]
        );
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x1A, 0x03, 0x41]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD]
        );
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x1A, 0x03, 0x4A]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD]
        );
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x1A, 0x03, 0x00]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD]
        );
        assert_eq!(rig.state().filter_width, Hz(50));
        civ_ask(&mut rig, 0x94, &[0x06, 0x02]);
        assert_eq!(
            civ_ask(&mut rig, 0x94, &[0x1A, 0x03]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD],
            "50 Hz is not an AM width"
        );
    }

    // Tests [@ANCHOR: radioctl_virtual_scope_sweep]
    #[test]
    fn only_a_model_with_a_scope_sends_sweeps_and_only_when_asked() {
        let mut rig = shipped_rig("icom-ic-9700");
        assert!(rig.scope_sweep().is_empty(), "output starts off");
        civ_ask(&mut rig, 0xA2, &[0x27, 0x11, 0x01]);
        let wire = rig.scope_sweep();
        assert!(wire.starts_with(&[0xFE, 0xFE, 0x00, 0xA2, 0x27, 0x00, 0x00, 0x01, 0x11]));
        assert_eq!(wire.iter().filter(|b| **b == 0xFD).count(), 11);
        let mut plain = VirtualRig::from_description(&RigDescription::ic7300()).expect("rig");
        assert_eq!(
            civ_ask(&mut plain, 0x94, &[0x27, 0x11, 0x01]),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD]
        );
        assert!(plain.scope_sweep().is_empty());
        let mut kenwood = VirtualRig::from_description(&RigDescription::ts2000()).expect("rig");
        assert!(kenwood.scope_sweep().is_empty());
    }

    // Tests [@ANCHOR: radioctl_virtual_kenwood_if]
    // Tests [@ANCHOR: radioctl_virtual_kenwood_select_vfo]
    // Tests [@ANCHOR: radioctl_virtual_kenwood_move_offset]
    // Tests [@ANCHOR: radioctl_virtual_kenwood_level]
    // Tests [@ANCHOR: radioctl_virtual_kenwood_command]
    #[test]
    fn kenwood_if_split_rit_and_levels_follow_the_guides() {
        let mut ts2000 = shipped_rig("kenwood-ts-2000");
        let answer = ts2000.feed(b"IF;", T0);
        assert_eq!(answer, b"IF000140740000000+0000000000020000000;".to_vec());
        assert_eq!(answer.len(), 38);
        let mut s590 = shipped_rig("kenwood-ts-590sg");
        let answer = s590.feed(b"IF;", T0);
        assert_eq!(answer, b"IF00014074000     +000000000020000000;".to_vec());
        assert_eq!(answer.len(), 38);
        assert!(s590.feed(b"RT1;FR0;FT1;RC;RD00120;", T0).is_empty());
        let answer = String::from_utf8(s590.feed(b"IF;", T0)).expect("ascii");
        assert_eq!(answer, "IF00014074000     -012010000020010000;");
        assert_eq!(s590.feed(b"FR;FT;", T0), b"FR0;FT1;");
        assert!(s590.state().split && s590.state().rit);
        assert_eq!(s590.feed(b"FR2;", T0), b"?;", "memory is not modelled");
        assert_eq!(s590.feed(b"RU20000;", T0), b"?;");
        assert_eq!(s590.feed(b"RU1;", T0), b"?;");
        assert!(s590.feed(b"RU00020;", T0).is_empty());
        assert_eq!(s590.state().rit_offset, -100);
        assert_eq!(
            s590.feed(b"AG0;MG;ML;PC;KS;RG;", T0),
            b"AG0030;MG025;ML005;PC050;KS018;RG250;"
        );
        assert!(s590
            .feed(b"AG0200;MG100;ML009;PC100;KS060;RG255;", T0)
            .is_empty());
        assert_eq!(s590.feed(b"AG0;", T0), b"AG0200;");
        for bad in [
            &b"AG200;"[..],
            b"AG0256;",
            b"MG101;",
            b"ML010;",
            b"PC004;",
            b"KS009;",
            b"RG25;",
            b"MG5;",
        ] {
            assert_eq!(s590.feed(bad, T0), b"?;", "{bad:?}");
        }
        assert_eq!(s590.feed(b"ID;", T0), b"ID023;");
    }

    // Tests [@ANCHOR: radioctl_virtual_yaesu_digits]
    // Tests [@ANCHOR: radioctl_virtual_yaesu_command]
    // Tests [@ANCHOR: radioctl_virtual_yaesu_set_frequency]
    // Tests [@ANCHOR: radioctl_virtual_yaesu_if]
    // Tests [@ANCHOR: radioctl_virtual_from_description]
    #[test]
    fn an_eight_digit_yaesu_twin_speaks_eight_digit_frequencies() {
        let path = format!(
            "{}/../../rigs/yaesu-ft-991a.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        let mut desc = radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description;
        desc.yaesu.as_mut().expect("yaesu block").frequency_digits = 8;
        let mut rig = VirtualRig::from_description(&desc).expect("rig");
        assert_eq!(rig.feed(b"FA;FB;", T0), b"FA14074000;FB14074000;");
        assert!(rig.feed(b"FA07074000;", T0).is_empty());
        assert_eq!(rig.feed(b"FA;", T0), b"FA07074000;");
        assert_eq!(rig.feed(b"FA007074000;", T0), b"?;", "nine digits refused");
        assert_eq!(rig.feed(b"FA7074000;", T0), b"?;", "seven digits refused");
        let answer = String::from_utf8(rig.feed(b"IF;", T0)).expect("ascii");
        assert_eq!(answer, "IF00107074000+000000200000;");
        assert_eq!(answer.len(), 28 - 1);
    }

    // Tests [@ANCHOR: radioctl_virtual_yaesu_command]
    // Tests [@ANCHOR: radioctl_virtual_yaesu_set_frequency]
    // Tests [@ANCHOR: radioctl_virtual_yaesu_if]
    // Tests [@ANCHOR: radioctl_virtual_yaesu_move_offset]
    // Tests [@ANCHOR: radioctl_virtual_yaesu_level]
    #[test]
    fn yaesu_newcat_commands_follow_the_cat_manuals() {
        let mut ft991 = shipped_rig("yaesu-ft-991a");
        assert_eq!(
            ft991.feed(b"ID;FA;FB;MD0;", T0),
            b"ID0670;FA014074000;FB014074000;MD02;"
        );
        assert!(ft991.feed(b"FA007074000;MD03;TX1;", T0).is_empty());
        assert_eq!(ft991.feed(b"TX;FA;MD0;", T0), b"TX1;FA007074000;MD03;");
        assert_eq!(
            ft991.feed(b"FA7074000;", T0),
            b"?;",
            "nine digits are required"
        );
        assert_eq!(ft991.feed(b"MD0Z;", T0), b"?;");
        assert_eq!(ft991.feed(b"FT2;FT;", T0), b"FT0;");
        assert!(ft991.feed(b"FT3;RT1;XT1;RC;RD0120;", T0).is_empty());
        assert_eq!(ft991.feed(b"FT;RT;XT;", T0), b"FT1;RT1;XT1;");
        let answer = String::from_utf8(ft991.feed(b"IF;", T0)).expect("ascii");
        assert_eq!(answer, "IF001007074000-012011300000;");
        assert_eq!(answer.len(), 28);
        assert_eq!(ft991.feed(b"RU10000;", T0), b"?;");
        assert_eq!(ft991.feed(b"RU120;", T0), b"?;");
        assert!(ft991.feed(b"RU0020;", T0).is_empty());
        assert_eq!(ft991.state().rit_offset, -100);
        assert_eq!(ft991.feed(b"SM0;RM5;", T0), b"SM0030;RM5128;");
        assert_eq!(
            ft991.feed(b"AG0;RG0;MG;PC;KS;", T0),
            b"AG0030;RG0250;MG025;PC005;KS018;"
        );
        assert!(ft991.feed(b"AG0200;MG100;PC100;KS060;", T0).is_empty());
        assert_eq!(ft991.feed(b"AG0;", T0), b"AG0200;");
        for bad in [
            &b"AG;"[..],
            b"AG200;",
            b"MG101;",
            b"PC004;",
            b"KS003;",
            b"RG0256;",
        ] {
            assert_eq!(ft991.feed(bad, T0), b"?;", "{bad:?}");
        }
        assert!(ft991.feed(b"AI1;", T0).is_empty());
        assert_eq!(ft991.feed(b"AI;", T0), b"AI1;");
        let mut dx10 = shipped_rig("yaesu-ftdx10");
        assert_eq!(dx10.feed(b"ID;RM5;", T0), b"ID0761;RM5000000;");
    }

    // Tests [@ANCHOR: radioctl_virtual_legacy_block]
    #[test]
    fn a_clarifier_block_that_is_not_bcd_is_ignored_not_a_panic() {
        let mut rig = shipped_rig("yaesu-ft-897d");
        // Found by parser_props.rs: nibbles above 9 overflowed the i16 arithmetic.
        assert!(rig
            .feed(&[0x00, 0x00, 0xFF, 0xFF, legacy::OP_CLAR_FREQUENCY], T0)
            .is_empty());
        assert_eq!(rig.state().rit_offset, 0);
        // Past the documented 9.99 kHz is ignored too; a documented value is taken.
        assert!(rig
            .feed(&[0x00, 0x00, 0x99, 0x99, legacy::OP_CLAR_FREQUENCY], T0)
            .is_empty());
        assert_eq!(rig.state().rit_offset, 0);
        assert!(rig
            .feed(&[0x01, 0x00, 0x09, 0x99, legacy::OP_CLAR_FREQUENCY], T0)
            .is_empty());
        assert_eq!(rig.state().rit_offset, -9_990);
    }

    // Tests [@ANCHOR: radioctl_virtual_legacy_block]
    #[test]
    fn legacy_blocks_set_and_read_as_the_opcode_chart_says() {
        let mut rig = shipped_rig("yaesu-ft-897d");
        assert_eq!(
            rig.feed(&[0, 0, 0, 0, 0x03], T0),
            vec![0x01, 0x40, 0x74, 0x00, 0x01]
        );
        assert!(rig
            .feed(&[0x43, 0x97, 0x00, 0x00, 0x01, 0x08, 0, 0, 0, 0x07], T0)
            .is_empty());
        assert_eq!(rig.state().freq_a, Hz(439_700_000));
        assert_eq!(rig.state().mode, Mode::Fm);
        assert_eq!(
            rig.feed(&[0, 0, 0, 0, 0x03], T0),
            vec![0x43, 0x97, 0x00, 0x00, 0x08]
        );
        assert_eq!(
            rig.feed(&[0, 0, 0, 0, 0xF7], T0),
            vec![0xA0],
            "RX, split off, no power"
        );
        assert!(rig
            .feed(&[0, 0, 0, 0, 0x08, 0, 0, 0, 0, 0x02], T0)
            .is_empty());
        assert_eq!(
            rig.feed(&[0, 0, 0, 0, 0xF7], T0),
            vec![0x08],
            "TX, split on, power 8"
        );
        assert!(rig.feed(&[0, 0, 0, 0, 0x88], T0).is_empty());
        assert!(rig.feed(&[0, 0, 0, 0, 0x05], T0).is_empty());
        assert!(rig.feed(&[1, 0, 0x01, 0x23, 0xF5], T0).is_empty());
        assert!(rig.state().rit && rig.state().split);
        assert_eq!(rig.state().rit_offset, -1230);
        assert_eq!(rig.feed(&[0, 0, 0, 0, 0xE7], T0), vec![15]);
        assert!(
            rig.feed(&[0, 0, 0, 0, 0x77], T0).is_empty(),
            "unknown opcodes are silent"
        );
        assert!(rig.feed(&[0x0A, 0, 0, 0, 0x07], T0).is_empty());
        assert!(
            rig.feed(&[0x7F, 0, 0, 0, 0x07], T0).is_empty(),
            "unknown mode ignored"
        );
        assert_eq!(rig.state().mode, Mode::Data);
        assert!(rig.feed(&[0, 0, 0], T0).is_empty());
        let later = T0 + Duration::from_secs(2);
        assert_eq!(
            rig.feed(&[0, 0, 0, 0, 0xE7], later),
            vec![15],
            "a stale partial block is dropped"
        );
    }
}

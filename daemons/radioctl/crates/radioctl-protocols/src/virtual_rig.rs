// SPDX-License-Identifier: AGPL-3.0-or-later
//! Virtual rigs: sans-IO radios generated from a [`RigDescription`], replacing the semantics of
//! `tools/rig_simulator.py` (CI-V and Kenwood ASCII) for tests and CI.
//!
//! A [`VirtualRig`] owns all of its state; any number of them run at once and share nothing.
//! `feed` takes the bytes a controller wrote and returns the bytes the radio answers. Delays,
//! pseudo-terminals and sockets are the caller's business, so a virtual rig is also replayable
//! from a transcript.
//!
//! Differences from the Python simulator, on purpose: meter values are BCD as on real Icom
//! radios, not big-endian binary; an unknown CI-V mode code is refused with NG instead of being
//! accepted silently; the composite `IF;` answer is not implemented (`?;`), see `kenwood.rs`.

use std::time::Duration;

use radioctl_core::description::{CivParams, Family, KenwoodParams, RigDescription};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::units::{Hz, Mode};

use crate::civ::{self, Frame, FrameParser, Parsed};
use crate::kenwood::{self, Token, Tokenizer};

/// Signal level a fresh virtual rig reports (the simulator's raw 30).
const DEFAULT_SIGNAL: u16 = 30;
/// Power meter reading while keyed (the simulator's raw 128).
const KEYED_POWER: u16 = 128;

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
        data.extend([code, 0x01]);
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
            Wire::Civ { .. } => 0,
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
}

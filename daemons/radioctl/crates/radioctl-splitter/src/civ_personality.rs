// SPDX-License-Identifier: AGPL-3.0-or-later
//! The IC-7300 CI-V personality: what outside software sees when it talks to the virtual port as
//! if it were an Icom IC-7300.
//!
//! Commands: `03` read frequency, `05` set frequency, `25` read or set the selected (`00`) or
//! unselected (`01`) VFO, `04`/`26 00` read mode, `06`/`26 00` set mode, `1C 00` read or set PTT,
//! `19 00` read the ID, `15 02` S-meter, `15 11` power meter, `17` send CW. Anything else is
//! answered NG. The byte layouts are those of this repository's own CI-V engine and virtual
//! rig (`radioctl-protocols::civ`); no IC-7300 has verified them.
//!
//! Echo: with `echo` on, each complete frame the client sent is first echoed back to it, as a
//! single-wire CI-V interface does. Transceive: every client is sent the radio's unsolicited frames
//! (`00` frequency, `01` mode, to the broadcast address) when VFO A or the mode changes. The mode
//! table is the IC-7300's regardless of the real rig; data modes map to the sideband they sit on
//! (DATA to USB, DATA-REV to LSB), a limit of this personality.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::event::Change;
use radioctl_core::op::Observation;
use radioctl_core::units::{Mode, Vfo};
use radioctl_protocols::civ::{
    decode_bcd_freq, encode_bcd_freq, encode_bcd_meter, Frame, FrameParser, Parsed, BROADCAST, NG,
    OK,
};

use crate::personality::{Action, Outcome, Pending, Personality, Request};

/// The IC-7300 emulated by a CI-V endpoint.
pub struct CivPersonality {
    address: u8,
    echo: bool,
    modes: Vec<(Mode, u8)>,
    parser: FrameParser,
}

impl CivPersonality {
    // [@ANCHOR: radioctl_splitter_civ_new]
    /// A personality answering at CI-V `address` (0x94 for the IC-7300), with the bus echo on or
    /// off.
    pub fn new(address: u8, echo: bool) -> CivPersonality {
        CivPersonality {
            address,
            echo,
            modes: RigDescription::ic7300().mode_codes,
            parser: FrameParser::default(),
        }
    }

    // [@ANCHOR: radioctl_splitter_civ_mode_code]
    /// The CI-V mode byte for a mode; data modes use the sideband they sit on.
    pub fn mode_code(&self, mode: Mode) -> Option<u8> {
        let mode = match mode {
            Mode::Data => Mode::Usb,
            Mode::DataR => Mode::Lsb,
            other => other,
        };
        self.modes.iter().find(|(m, _)| *m == mode).map(|(_, c)| *c)
    }

    fn code_mode(&self, code: u8) -> Option<Mode> {
        self.modes.iter().find(|(_, c)| *c == code).map(|(m, _)| *m)
    }

    // [@ANCHOR: radioctl_splitter_civ_status_frame]
    /// An OK or NG frame to `to`.
    fn status_frame(&self, to: u8, ok: bool) -> Vec<u8> {
        self.frame(to, if ok { OK } else { NG }, &[])
    }

    fn frame(&self, to: u8, cmd: u8, data: &[u8]) -> Vec<u8> {
        Frame {
            to,
            from: self.address,
            cmd,
            data: data.to_vec(),
        }
        .to_bytes()
    }

    // [@ANCHOR: radioctl_splitter_civ_classify]
    /// Turns one frame addressed to us into an action.
    fn classify(&self, frame: &Frame) -> Action {
        let rig = |request: Request, prefix: &[u8]| {
            let mut context = vec![frame.from, frame.cmd];
            context.extend_from_slice(prefix);
            Action::Rig { request, context }
        };
        let refuse = || Action::Local(self.status_frame(frame.from, false));
        let vfo_of = |selector: u8| if selector == 0 { Vfo::A } else { Vfo::B };
        match (frame.cmd, frame.data.as_slice()) {
            (0x03, []) => rig(Request::ReadFrequency(Vfo::A), &[]),
            (0x05, bcd) if bcd.len() == 5 => match decode_bcd_freq(bcd) {
                Some(hz) => rig(Request::SetFrequency(Vfo::A, hz), &[]),
                None => refuse(),
            },
            (0x25, [selector @ (0x00 | 0x01)]) => {
                rig(Request::ReadFrequency(vfo_of(*selector)), &[*selector])
            }
            (0x25, [selector @ (0x00 | 0x01), bcd @ ..]) if bcd.len() == 5 => {
                match decode_bcd_freq(bcd) {
                    Some(hz) => rig(Request::SetFrequency(vfo_of(*selector), hz), &[]),
                    None => refuse(),
                }
            }
            (0x04, []) => rig(Request::ReadMode, &[]),
            (0x26, [0x00]) => rig(Request::ReadMode, &[0x00]),
            (0x06, [code, ..]) | (0x26, [0x00, code, ..]) => match self.code_mode(*code) {
                Some(mode) => rig(Request::SetMode(mode), &[]),
                None => refuse(),
            },
            (0x1C, [0x00]) => rig(Request::ReadPtt, &[0x00]),
            (0x1C, [0x00, state]) => rig(Request::SetPtt(*state != 0), &[]),
            (0x19, [0x00]) => Action::Local(self.frame(frame.from, 0x19, &[0x00, self.address])),
            (0x15, [0x02]) => rig(Request::ReadSignal, &[0x02]),
            (0x15, [0x11]) => rig(Request::ReadPower, &[0x11]),
            (0x17, text) if !text.is_empty() && text.iter().all(|b| (0x20..0x7F).contains(b)) => {
                rig(
                    Request::SendCw(String::from_utf8_lossy(text).into_owned()),
                    &[],
                )
            }
            _ => refuse(),
        }
    }
}

impl Personality for CivPersonality {
    // [@ANCHOR: radioctl_splitter_civ_parse]
    fn parse(&mut self, bytes: &[u8], now: Duration) -> Vec<Pending> {
        let mut out = Vec::new();
        for item in self.parser.feed(bytes, now) {
            let Parsed::Frame(frame) = item else {
                continue;
            };
            let echo = if self.echo {
                frame.to_bytes()
            } else {
                Vec::new()
            };
            let for_us = frame.to == self.address || frame.to == BROADCAST;
            let action = if for_us && frame.from != self.address {
                self.classify(&frame)
            } else {
                Action::Ignore
            };
            out.push(Pending { echo, action });
        }
        out
    }

    // [@ANCHOR: radioctl_splitter_civ_reply]
    fn reply(&mut self, context: &[u8], _request: &Request, outcome: &Outcome) -> Vec<u8> {
        let (Some(&to), Some(&cmd)) = (context.first(), context.get(1)) else {
            return Vec::new();
        };
        let prefix = context.get(2..).unwrap_or(&[]);
        let with_prefix = |tail: &[u8]| {
            let mut data = prefix.to_vec();
            data.extend_from_slice(tail);
            self.frame(to, cmd, &data)
        };
        match outcome {
            Outcome::Refused => self.status_frame(to, false),
            Outcome::Done => self.status_frame(to, true),
            Outcome::Value(Observation::Frequency(_, hz)) => match encode_bcd_freq(*hz) {
                Some(bcd) => with_prefix(&bcd),
                None => self.status_frame(to, false),
            },
            Outcome::Value(Observation::Mode(mode)) => match self.mode_code(*mode) {
                // `26 00` carries the DATA flag between mode and filter; `04` does not.
                Some(code) if cmd == 0x26 => {
                    let data = u8::from(matches!(mode, Mode::Data | Mode::DataR));
                    with_prefix(&[code, data, 0x01])
                }
                Some(code) => with_prefix(&[code, 0x01]),
                None => self.status_frame(to, false),
            },
            Outcome::Value(Observation::Ptt(on)) => with_prefix(&[u8::from(*on)]),
            Outcome::Value(Observation::Signal(v) | Observation::Power(v)) => {
                with_prefix(&encode_bcd_meter(*v))
            }
            Outcome::Value(_) => self.status_frame(to, false),
        }
    }

    // [@ANCHOR: radioctl_splitter_civ_unsolicited]
    fn unsolicited(&mut self, change: &Change) -> Vec<u8> {
        match change.new {
            Observation::Frequency(Vfo::A, hz) => encode_bcd_freq(hz)
                .map(|bcd| self.frame(BROADCAST, 0x00, &bcd))
                .unwrap_or_default(),
            Observation::Mode(mode) => self
                .mode_code(mode)
                .map(|code| self.frame(BROADCAST, 0x01, &[code, 0x01]))
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    // [@ANCHOR: radioctl_splitter_civ_reset]
    fn reset(&mut self) {
        self.parser = FrameParser::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::op::Field;
    use radioctl_core::units::Hz;

    const T0: Duration = Duration::from_secs(1);
    const READ_FREQ: [u8; 6] = [0xFE, 0xFE, 0x94, 0xE0, 0x03, 0xFD];

    fn ask(p: &mut CivPersonality, frame: &[u8]) -> Pending {
        let mut items = p.parse(frame, T0);
        assert_eq!(items.len(), 1, "{items:?}");
        items.remove(0)
    }

    fn context_of(pending: &Pending) -> (Request, Vec<u8>) {
        match &pending.action {
            Action::Rig { request, context } => (request.clone(), context.clone()),
            other => panic!("expected a rig action, got {other:?}"),
        }
    }

    // Tests [@ANCHOR: radioctl_splitter_civ_new]
    // Tests [@ANCHOR: radioctl_splitter_civ_mode_code]
    // Tests [@ANCHOR: radioctl_splitter_civ_parse]
    // Tests [@ANCHOR: radioctl_splitter_civ_classify]
    #[test]
    fn requests_become_rig_actions_and_the_rest_is_refused_or_ignored() {
        let mut p = CivPersonality::new(0x94, false);
        let pending = ask(&mut p, &READ_FREQ);
        assert!(pending.echo.is_empty());
        assert_eq!(context_of(&pending).0, Request::ReadFrequency(Vfo::A));
        let set = [
            0xFE, 0xFE, 0x94, 0xE0, 0x05, 0x00, 0x00, 0x03, 0x07, 0x00, 0xFD,
        ];
        assert_eq!(
            context_of(&ask(&mut p, &set)).0,
            Request::SetFrequency(Vfo::A, Hz(7_030_000))
        );
        let vfo_b = [0xFE, 0xFE, 0x94, 0xE0, 0x25, 0x01, 0xFD];
        assert_eq!(
            context_of(&ask(&mut p, &vfo_b)).0,
            Request::ReadFrequency(Vfo::B)
        );
        let mode = [0xFE, 0xFE, 0x94, 0xE0, 0x06, 0x03, 0x01, 0xFD];
        assert_eq!(
            context_of(&ask(&mut p, &mode)).0,
            Request::SetMode(Mode::Cw)
        );
        let ptt = [0xFE, 0xFE, 0x94, 0xE0, 0x1C, 0x00, 0x01, 0xFD];
        assert_eq!(context_of(&ask(&mut p, &ptt)).0, Request::SetPtt(true));
        let cw = [0xFE, 0xFE, 0x94, 0xE0, 0x17, b'C', b'Q', 0xFD];
        assert_eq!(
            context_of(&ask(&mut p, &cw)).0,
            Request::SendCw("CQ".into())
        );
        let id = [0xFE, 0xFE, 0x00, 0xE0, 0x19, 0x00, 0xFD];
        assert_eq!(
            ask(&mut p, &id).action,
            Action::Local(vec![0xFE, 0xFE, 0xE0, 0x94, 0x19, 0x00, 0x94, 0xFD])
        );
        let unknown = [0xFE, 0xFE, 0x94, 0xE0, 0x7F, 0xFD];
        assert_eq!(
            ask(&mut p, &unknown).action,
            Action::Local(vec![0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD])
        );
        let other_radio = [0xFE, 0xFE, 0x70, 0xE0, 0x03, 0xFD];
        assert_eq!(ask(&mut p, &other_radio).action, Action::Ignore);
        let bad_bcd = [
            0xFE, 0xFE, 0x94, 0xE0, 0x05, 0x0A, 0x00, 0x00, 0x00, 0x00, 0xFD,
        ];
        assert!(matches!(ask(&mut p, &bad_bcd).action, Action::Local(_)));
        assert!(p.parse(&[0x01, 0x02], T0).is_empty());
        assert_eq!(p.mode_code(Mode::Data), p.mode_code(Mode::Usb));
        assert_eq!(p.mode_code(Mode::DataR), p.mode_code(Mode::Lsb));
        let mut echoing = CivPersonality::new(0x94, true);
        assert_eq!(ask(&mut echoing, &READ_FREQ).echo, READ_FREQ.to_vec());
        assert_eq!(ask(&mut echoing, &other_radio).echo, other_radio.to_vec());
    }

    // Tests [@ANCHOR: radioctl_splitter_civ_reply]
    // Tests [@ANCHOR: radioctl_splitter_civ_status_frame]
    #[test]
    fn replies_carry_the_addresses_prefix_and_value() {
        let mut p = CivPersonality::new(0x94, false);
        let (request, context) = context_of(&ask(&mut p, &READ_FREQ));
        let value = Outcome::Value(Observation::Frequency(Vfo::A, Hz(14_074_000)));
        assert_eq!(
            p.reply(&context, &request, &value),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x03, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD]
        );
        assert_eq!(
            p.reply(&context, &request, &Outcome::Done),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD]
        );
        assert_eq!(
            p.reply(&context, &request, &Outcome::Refused),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD]
        );
        let mode_read = [0xFE, 0xFE, 0x94, 0xE0, 0x26, 0x00, 0xFD];
        let (request, context) = context_of(&ask(&mut p, &mode_read));
        assert_eq!(
            p.reply(
                &context,
                &request,
                &Outcome::Value(Observation::Mode(Mode::Cw))
            ),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x26, 0x00, 0x03, 0x00, 0x01, 0xFD]
        );
        assert_eq!(
            p.reply(
                &context,
                &request,
                &Outcome::Value(Observation::Mode(Mode::Data))
            ),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x26, 0x00, 0x01, 0x01, 0x01, 0xFD]
        );
        let ptt_read = [0xFE, 0xFE, 0x94, 0xE0, 0x1C, 0x00, 0xFD];
        let (request, context) = context_of(&ask(&mut p, &ptt_read));
        assert_eq!(
            p.reply(&context, &request, &Outcome::Value(Observation::Ptt(true))),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x1C, 0x00, 0x01, 0xFD]
        );
        let meter = [0xFE, 0xFE, 0x94, 0xE0, 0x15, 0x02, 0xFD];
        let (request, context) = context_of(&ask(&mut p, &meter));
        assert_eq!(
            p.reply(
                &context,
                &request,
                &Outcome::Value(Observation::Signal(120))
            ),
            vec![0xFE, 0xFE, 0xE0, 0x94, 0x15, 0x02, 0x01, 0x20, 0xFD]
        );
        assert!(p.reply(&[], &request, &Outcome::Done).is_empty());
    }

    // Tests [@ANCHOR: radioctl_splitter_civ_unsolicited]
    // Tests [@ANCHOR: radioctl_splitter_civ_reset]
    #[test]
    fn transceive_frames_follow_vfo_a_and_the_mode_only() {
        let mut p = CivPersonality::new(0x94, false);
        let freq = Change {
            field: Field::Frequency(Vfo::A),
            old: None,
            new: Observation::Frequency(Vfo::A, Hz(7_074_000)),
        };
        assert_eq!(
            p.unsolicited(&freq),
            vec![0xFE, 0xFE, 0x00, 0x94, 0x00, 0x00, 0x40, 0x07, 0x07, 0x00, 0xFD]
        );
        let mode = Change {
            field: Field::Mode,
            old: None,
            new: Observation::Mode(Mode::Lsb),
        };
        assert_eq!(
            p.unsolicited(&mode),
            vec![0xFE, 0xFE, 0x00, 0x94, 0x01, 0x00, 0x01, 0xFD]
        );
        let ptt = Change {
            field: Field::Ptt,
            old: None,
            new: Observation::Ptt(true),
        };
        assert!(p.unsolicited(&ptt).is_empty());
        let b = Change {
            field: Field::Frequency(Vfo::B),
            old: None,
            new: Observation::Frequency(Vfo::B, Hz(1)),
        };
        assert!(p.unsolicited(&b).is_empty());
        p.parse(&[0xFE, 0xFE, 0x94], T0);
        p.reset();
        assert_eq!(p.parse(&READ_FREQ, T0).len(), 1);
    }
}

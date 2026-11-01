// SPDX-License-Identifier: AGPL-3.0-or-later
//! JRC receivers and transceivers with the `F` / `D` / `I` ASCII command set (NRD-525, NRD-535D,
//! NRD-545 DSP, JST-145, JST-245): sans-IO client engine and virtual twin.
//!
//! **Source.** Hamlib 4.6.2 (released source tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8), read for facts only:
//! `rigs/jrc/jrc.c` (`jrc_transaction`, `jrc_set_freq`, `jrc_get_freq`, `jrc_set_mode`,
//! `jrc_get_mode`, `jrc_get_level`, `get_current_istate`), `nrd525.c` (`nrd525_set_freq`,
//! `nrd525_set_mode`), `nrd535.c` and `nrd545.c` (the `jrc_priv_caps` blocks), `jst145.c`
//! (`jst145_set_freq`, `jst145_get_freq`, `jst145_set_mode`, `jst145_get_mode`, `jst145_set_ptt`,
//! `jst145_get_ptt`). No code or comment was copied and no JRC manual was read; every fact is
//! `hamlib-source` and unverified.
//!
//! * Commands end with CR. Hamlib does not wait for an answer to a setting (no acknowledgement
//!   exists in these sources), so settings are sent with no reply expected.
//! * NRD-535D and NRD-545 DSP: `F` and 8 (NRD-535D) or 10 (NRD-545) digits of hertz; `D` and a mode
//!   character; read the state with `I` (the NRD-535D: `I1` then `I0`), whose answer
//!   `I a b d <digits> ...` has the width in the third character, the mode in the fourth and the
//!   frequency from the fifth, and is 14 (NRD-535D) or 18 (NRD-545) characters with its CR. The
//!   signal is `M` and three digits.
//! * NRD-525: write only. `F` and eight digits in units of 10 Hz, and `D` and a mode character,
//!   both sent without a terminator as Hamlib sends them; nothing can be read back.
//! * JST-145 and JST-245: `F`, eight digits of hertz and `A` or `B` for the VFO, `D` and a mode
//!   character, `I` reads (the frequency is the eight digits after the first three characters,
//!   the mode the fourth), `X1` and `X0` key and unkey and `X` reads (`X0` or `X1`). They are
//!   transceivers; the transmit gate stays closed by default.
//! * Modes: `0` RTTY, `1` CW, `2` USB, `3` LSB, `4` AM, `5` FM (`6` is AMS or FAX, `7` and `8`
//!   ECSS, `9` WFM on the models that have them; those have no mapped radioctl mode except WFM).
//!
//! Not implemented: bandwidth (`B`), levels, functions, memories, the VFO selection of the JST.

use radioctl_core::description::{ReceiverProtocol, RigDescription};
use radioctl_core::engine::{Inbound, ReplyKind};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Vfo};

use crate::receiver::{Dialect, LineEngine, ReceiverTwin};
use crate::virtual_rig::RigState;

/// The JRC engine.
pub type JrcEngine = LineEngine<JrcDialect>;

// [@ANCHOR: radioctl_jrc_engine]
/// The engine for a JRC description.
pub fn engine(desc: RigDescription) -> Result<JrcEngine, RigError> {
    LineEngine::new(desc, ReceiverProtocol::Jrc)
}

/// Which JRC command set a model speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// NRD-535D and NRD-545 DSP: digits of hertz, state through `I`.
    Nrd {
        digits: usize,
        info_len: usize,
        i_pair: bool,
    },
    /// NRD-525: write only, units of 10 Hz, no terminator.
    Nrd525,
    /// JST-145 and JST-245.
    Jst,
}

/// The JRC command set of one model.
#[derive(Debug, Clone, Copy)]
pub struct JrcDialect {
    kind: Kind,
}

impl Dialect for JrcDialect {
    // [@ANCHOR: radioctl_jrc_dialect_from_description]
    fn from_description(desc: &RigDescription) -> Result<JrcDialect, RigError> {
        let kind = match desc.receiver.as_ref().map_or("", |r| r.dialect.as_str()) {
            "nrd535" => Kind::Nrd {
                digits: 8,
                info_len: 14,
                i_pair: true,
            },
            "nrd545" => Kind::Nrd {
                digits: 10,
                info_len: 18,
                i_pair: false,
            },
            "nrd525" => Kind::Nrd525,
            "jst145" => Kind::Jst,
            other => {
                return Err(RigError::new(
                    ErrorKind::Unsupported,
                    format!("unknown JRC dialect {other:?}"),
                ))
            }
        };
        Ok(JrcDialect { kind })
    }

    // [@ANCHOR: radioctl_jrc_dialect_encode]
    fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError> {
        let unsupported = |why: &str| RigError::new(ErrorKind::Unsupported, why.to_string());
        let mode_char = |mode| {
            desc.mode_code(mode)
                .map(char::from)
                .ok_or_else(|| unsupported("mode not in the rig description"))
        };
        let none = ReplyKind::None;
        let ask = |field| ReplyKind::Observation(field);
        Ok(match (op, self.kind) {
            (Op::SetFrequency(Vfo::A, hz), Kind::Nrd { digits, .. }) => {
                if hz.0 >= 10u64.pow(u32::try_from(digits).unwrap_or(8)) {
                    return Err(unsupported("frequency out of range"));
                }
                (format!("F{:0w$}\r", hz.0, w = digits).into_bytes(), none)
            }
            (Op::SetFrequency(Vfo::A, hz), Kind::Nrd525) => {
                if hz.0 / 10 > 99_999_999 {
                    return Err(unsupported("frequency out of range"));
                }
                (format!("F{:08}", hz.0 / 10).into_bytes(), none)
            }
            (Op::SetFrequency(vfo @ (Vfo::A | Vfo::B), hz), Kind::Jst) => {
                if hz.0 > 99_999_999 {
                    return Err(unsupported("frequency out of range"));
                }
                let letter = if *vfo == Vfo::B { 'B' } else { 'A' };
                (format!("F{:08}{letter}\r", hz.0).into_bytes(), none)
            }
            (Op::GetFrequency(Vfo::A), Kind::Nrd { i_pair, .. }) => (
                if i_pair {
                    b"I1\rI0\r".to_vec()
                } else {
                    b"I\r".to_vec()
                },
                ask(Field::Frequency(Vfo::A)),
            ),
            (Op::GetFrequency(Vfo::A), Kind::Jst) => {
                (b"I\r".to_vec(), ask(Field::Frequency(Vfo::A)))
            }
            (Op::SetMode(mode), Kind::Nrd525) => {
                (format!("D{}", mode_char(*mode)?).into_bytes(), none)
            }
            (Op::SetMode(mode), _) => (format!("D{}\r", mode_char(*mode)?).into_bytes(), none),
            (Op::GetMode, Kind::Nrd { i_pair, .. }) => (
                if i_pair {
                    b"I1\rI0\r".to_vec()
                } else {
                    b"I\r".to_vec()
                },
                ask(Field::Mode),
            ),
            (Op::GetMode, Kind::Jst) => (b"I\r".to_vec(), ask(Field::Mode)),
            (Op::GetSignal, Kind::Nrd { .. }) => (b"M\r".to_vec(), ask(Field::Signal)),
            (Op::SetPtt(on), Kind::Jst) => (format!("X{}\r", u8::from(*on)).into_bytes(), none),
            (Op::GetPtt, Kind::Jst) => (b"X\r".to_vec(), ask(Field::Ptt)),
            _ => return Err(unsupported("not implemented for this JRC model")),
        })
    }

    // [@ANCHOR: radioctl_jrc_dialect_status]
    fn status(&mut self, _expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>> {
        line.is_empty().then(Vec::new)
    }

    // [@ANCHOR: radioctl_jrc_dialect_decode]
    fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound> {
        let unknown = || vec![Inbound::Unrecognised(line.as_bytes().to_vec())];
        let ReplyKind::Observation(field) = expecting else {
            return unknown();
        };
        let found = match field {
            Field::Frequency(_) => self
                .info_frequency(line)
                .map(|hz| Observation::Frequency(Vfo::A, hz)),
            Field::Mode => self.info_mode(desc, line).map(Observation::Mode),
            Field::Signal => line
                .strip_prefix('M')
                .filter(|d| d.len() == 3)
                .and_then(|d| d.parse::<u16>().ok())
                .map(Observation::Signal),
            Field::Ptt => match line {
                "X0" => Some(Observation::Ptt(false)),
                "X1" => Some(Observation::Ptt(true)),
                _ => None,
            },
            _ => None,
        };
        found.map_or_else(unknown, |o| vec![Inbound::Observation(o)])
    }
}

impl JrcDialect {
    // [@ANCHOR: radioctl_jrc_info_checked]
    /// The `I` answer if it has the length the model's answer has (the CR counts), else `None`.
    fn info_checked<'a>(&self, line: &'a str) -> Option<&'a str> {
        let right_start = line.starts_with('I');
        match self.kind {
            Kind::Nrd { info_len, .. } => {
                (right_start && line.len() + 1 == info_len).then_some(line)
            }
            Kind::Jst => (right_start && line.len() >= 12).then_some(line),
            Kind::Nrd525 => None,
        }
    }

    // [@ANCHOR: radioctl_jrc_info_frequency]
    /// The frequency in an `I` answer: from the fifth character on the NRD, after three
    /// characters on the JST.
    fn info_frequency(&self, line: &str) -> Option<Hz> {
        let info = self.info_checked(line)?;
        let (start, len) = match self.kind {
            Kind::Nrd { digits, .. } => (4, digits),
            _ => (4, 8),
        };
        let digits = info.get(start..start + len)?;
        digits
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| digits.parse().ok().map(Hz))
            .flatten()
    }

    // [@ANCHOR: radioctl_jrc_info_mode]
    /// The mode in an `I` answer: its fourth character.
    fn info_mode(&self, desc: &RigDescription, line: &str) -> Option<radioctl_core::units::Mode> {
        let info = self.info_checked(line)?;
        desc.code_mode(*info.as_bytes().get(3)?)
    }
}

/// The virtual JRC radio.
#[derive(Debug)]
pub struct JrcTwin {
    desc: RigDescription,
    dialect: JrcDialect,
}

impl JrcTwin {
    // [@ANCHOR: radioctl_jrc_twin_new]
    /// The twin of a JRC description.
    pub fn new(desc: &RigDescription) -> Result<JrcTwin, RigError> {
        Ok(JrcTwin {
            dialect: JrcDialect::from_description(desc)?,
            desc: desc.clone(),
        })
    }
}

impl ReceiverTwin for JrcTwin {
    // [@ANCHOR: radioctl_jrc_twin_line]
    fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8> {
        let mode_char = self.desc.mode_code(state.mode).map_or('4', char::from);
        let reply = |text: String| format!("{text}\r").into_bytes();
        match (self.dialect.kind, line) {
            (
                Kind::Nrd {
                    digits, info_len, ..
                },
                "I1" | "I",
            ) => {
                let head = format!("I00{mode_char}{:0w$}", state.freq_a.0, w = digits);
                reply(format!("{head:0<w$}", w = info_len - 1))
            }
            (Kind::Jst, "I") => reply(format!("I00{mode_char}{:08}0", state.freq_a.0)),
            (Kind::Nrd { .. }, "M") => reply(format!("M{:03}", state.signal.min(999))),
            (Kind::Jst, "X") => reply(format!("X{}", u8::from(state.ptt))),
            (Kind::Jst, "X0" | "X1") => {
                state.ptt = line == "X1";
                Vec::new()
            }
            (kind, _) => {
                self.set(kind, state, line);
                Vec::new()
            }
        }
    }
}

impl JrcTwin {
    // [@ANCHOR: radioctl_jrc_twin_set]
    /// Applies a setting (`F...` or `D.`); anything else is ignored, as these radios do not refuse.
    fn set(&mut self, kind: Kind, state: &mut RigState, line: &str) {
        if let Some(digits) = line.strip_prefix('F') {
            let digits = digits.trim_end_matches(['A', 'B']);
            if let Ok(n) = digits.parse::<u64>() {
                state.freq_a = Hz(if kind == Kind::Nrd525 { n * 10 } else { n });
            }
        } else if let Some(code) = line.strip_prefix('D') {
            if let Some(mode) = code
                .as_bytes()
                .first()
                .and_then(|c| self.desc.code_mode(*c))
            {
                state.mode = mode;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use radioctl_core::engine::ProtocolEngine;
    use radioctl_core::units::Mode;

    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    fn bytes(e: &mut JrcEngine, op: Op) -> Vec<u8> {
        e.encode(&op).expect("encode").bytes
    }

    // Tests [@ANCHOR: radioctl_jrc_engine]
    // Tests [@ANCHOR: radioctl_jrc_dialect_from_description]
    // Tests [@ANCHOR: radioctl_jrc_dialect_encode]
    #[test]
    fn each_model_spells_its_commands_as_hamlib_does() {
        let mut n545 = engine(shipped("jrc-nrd-545")).expect("engine");
        assert_eq!(
            bytes(&mut n545, Op::SetFrequency(Vfo::A, Hz(7_074_000))),
            b"F0007074000\r"
        );
        assert_eq!(bytes(&mut n545, Op::SetMode(Mode::Usb)), b"D2\r");
        assert_eq!(bytes(&mut n545, Op::GetFrequency(Vfo::A)), b"I\r");
        assert_eq!(bytes(&mut n545, Op::GetSignal), b"M\r");
        let mut n535 = engine(shipped("jrc-nrd-535d")).expect("engine");
        assert_eq!(
            bytes(&mut n535, Op::SetFrequency(Vfo::A, Hz(7_074_000))),
            b"F07074000\r"
        );
        assert_eq!(bytes(&mut n535, Op::GetMode), b"I1\rI0\r");
        assert!(n535
            .encode(&Op::SetFrequency(Vfo::A, Hz(100_000_000)))
            .is_err());
        let mut n525 = engine(shipped("jrc-nrd-525")).expect("engine");
        assert_eq!(
            bytes(&mut n525, Op::SetFrequency(Vfo::A, Hz(7_074_000))),
            b"F00707400"
        );
        assert_eq!(bytes(&mut n525, Op::SetMode(Mode::Cw)), b"D1");
        assert!(
            n525.encode(&Op::GetFrequency(Vfo::A)).is_err(),
            "write only"
        );
        let mut j = engine(shipped("jrc-jst-145")).expect("engine");
        assert_eq!(
            bytes(&mut j, Op::SetFrequency(Vfo::A, Hz(14_200_000))),
            b"F14200000A\r"
        );
        assert_eq!(bytes(&mut j, Op::SetPtt(true)), b"X1\r");
        assert_eq!(bytes(&mut j, Op::GetPtt), b"X\r");
        assert!(j.encode(&Op::GetSignal).is_err());
        assert!(engine(shipped("aor-ar8000")).is_err());
    }

    // Tests [@ANCHOR: radioctl_jrc_dialect_status]
    // Tests [@ANCHOR: radioctl_jrc_dialect_decode]
    // Tests [@ANCHOR: radioctl_jrc_info_checked]
    // Tests [@ANCHOR: radioctl_jrc_info_frequency]
    // Tests [@ANCHOR: radioctl_jrc_info_mode]
    #[test]
    fn the_info_answer_is_read_by_position_and_length() {
        let mut n545 = engine(shipped("jrc-nrd-545")).expect("engine");
        n545.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(
            n545.feed(b"I0020007074000000\r", T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(7_074_000)
            ))]
        );
        n545.encode(&Op::GetMode).expect("get");
        assert_eq!(
            n545.feed(b"I0020007074000000\r", T0),
            vec![Inbound::Observation(Observation::Mode(Mode::Usb))]
        );
        n545.encode(&Op::GetMode).expect("get");
        assert!(
            matches!(
                n545.feed(b"I002\r", T0).as_slice(),
                [Inbound::Unrecognised(_)]
            ),
            "wrong length"
        );
        n545.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            n545.feed(b"M123\r", T0),
            vec![Inbound::Observation(Observation::Signal(123))]
        );
        n545.encode(&Op::GetSignal).expect("get");
        assert!(matches!(
            n545.feed(b"M12\r", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        let mut j = engine(shipped("jrc-jst-145")).expect("engine");
        j.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(
            j.feed(b"I00414200000X\r", T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(14_200_000)
            ))]
        );
        j.encode(&Op::GetPtt).expect("get");
        assert_eq!(
            j.feed(b"X1\r", T0),
            vec![Inbound::Observation(Observation::Ptt(true))]
        );
        assert!(j.feed(b"\r", T0).is_empty());
    }

    // Tests [@ANCHOR: radioctl_jrc_twin_new]
    // Tests [@ANCHOR: radioctl_jrc_twin_line]
    // Tests [@ANCHOR: radioctl_jrc_twin_set]
    #[test]
    fn the_twin_keeps_state_and_never_refuses() {
        let desc = shipped("jrc-nrd-525");
        let mut twin = JrcTwin::new(&desc).expect("twin");
        let mut state = crate::virtual_rig::VirtualRig::from_description(&desc)
            .expect("rig")
            .state()
            .clone();
        assert!(twin.line(&mut state, "F00707400").is_empty());
        assert_eq!(state.freq_a, Hz(7_074_000));
        assert!(twin.line(&mut state, "Q99").is_empty());
        assert!(
            twin.line(&mut state, "D9").is_empty(),
            "unknown mode ignored"
        );
        assert!(JrcTwin::new(&shipped("aor-ar8000")).is_err());
    }
}

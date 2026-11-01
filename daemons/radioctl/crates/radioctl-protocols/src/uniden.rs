// SPDX-License-Identifier: AGPL-3.0-or-later
//! Uniden and Bearcat scanners with the `RF` / `RM` / `SG` ASCII command set (BC245XLT, BC250D,
//! BC780XLT, BC895XLT, BC898T): sans-IO client engine and virtual twin.
//!
//! **Source.** Hamlib 4.6.2 (released source tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8), read for facts only:
//! `rigs/uniden/uniden.c` (`uniden_transaction`, `uniden_set_freq`, `uniden_get_freq`,
//! `uniden_get_freq_2`, `uniden_set_mode`, `uniden_get_mode`, `uniden_get_level`) and the
//! `bc*.c` model files (which of the two frequency reads each model uses). No code or comment
//! was copied and no Uniden manual was read; every fact is `hamlib-source` and unverified.
//!
//! * Commands end with CR and so do answers. A setting is answered `OK`; `NG`, `ORER` and `ERR`
//!   refuse it.
//! * Frequency: `RF` and eight digits in units of 100 Hz. Read with `RF` (answer `RF` and the
//!   digits), except on the BC780XLT, which Hamlib reads through `SG`.
//! * Mode: `RM AM`, `RM FM`, `RM NFM` or `RM WFM`; read with `RM`, answer `RM` and the name.
//!   The description's mode codes are radioctl's own numbering (1 AM, 2 FM, 3 WFM); narrow FM is
//!   read as FM and never sent (Hamlib chooses it by passband width, which radioctl has no
//!   operation for).
//! * Signal and frequency: `SG` answers `S` and a number, a space, `F` and eight digits
//!   (`S182 F08594375`); the number is the raw signal.
//!
//! The BCD-396T and BCD-996T are in the `uniden_digital.c` backend, whose frequency and mode
//! functions are stubs in Hamlib; they are not covered here. Memories, banks, scans, squelch and
//! the attenuator (`AT`) are not implemented.

use radioctl_core::description::{ReceiverProtocol, RigDescription};
use radioctl_core::engine::{Inbound, ReplyKind};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Mode, Vfo};

use crate::receiver::{Dialect, LineEngine, ReceiverTwin};
use crate::virtual_rig::RigState;

/// The Uniden engine.
pub type UnidenEngine = LineEngine<UnidenDialect>;

// [@ANCHOR: radioctl_uniden_engine]
/// The engine for a Uniden description.
pub fn engine(desc: RigDescription) -> Result<UnidenEngine, RigError> {
    LineEngine::new(desc, ReceiverProtocol::Uniden)
}

/// The mode spellings by radioctl's mode code (1, 2, 3).
const MODE_NAMES: [(u8, &str, Mode); 3] = [
    (1, "AM", Mode::Am),
    (2, "FM", Mode::Fm),
    (3, "WFM", Mode::Wfm),
];

/// The Uniden command set of one model.
#[derive(Debug, Clone, Copy)]
pub struct UnidenDialect {
    /// The BC780XLT reads its frequency through `SG`.
    frequency_via_sg: bool,
}

impl Dialect for UnidenDialect {
    // [@ANCHOR: radioctl_uniden_dialect_from_description]
    fn from_description(desc: &RigDescription) -> Result<UnidenDialect, RigError> {
        match desc.receiver.as_ref().map_or("", |r| r.dialect.as_str()) {
            "uniden" => Ok(UnidenDialect {
                frequency_via_sg: false,
            }),
            "bc780" => Ok(UnidenDialect {
                frequency_via_sg: true,
            }),
            other => Err(RigError::new(
                ErrorKind::Unsupported,
                format!("unknown Uniden dialect {other:?}"),
            )),
        }
    }

    // [@ANCHOR: radioctl_uniden_dialect_encode]
    fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError> {
        let unsupported = |why: &str| RigError::new(ErrorKind::Unsupported, why.to_string());
        Ok(match op {
            Op::GetFrequency(Vfo::A) => {
                let command = if self.frequency_via_sg {
                    "SG\r"
                } else {
                    "RF\r"
                };
                (
                    command.into(),
                    ReplyKind::Observation(Field::Frequency(Vfo::A)),
                )
            }
            Op::SetFrequency(Vfo::A, hz) => {
                let units = hz.0 / 100;
                if units > 99_999_999 {
                    return Err(unsupported("frequency out of range"));
                }
                (format!("RF{units:08}\r").into_bytes(), ReplyKind::Ack)
            }
            Op::GetMode => (b"RM\r".to_vec(), ReplyKind::Observation(Field::Mode)),
            Op::SetMode(mode) => {
                let code = desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                let name = MODE_NAMES
                    .iter()
                    .find(|(c, _, _)| *c == code)
                    .map(|(_, n, _)| *n)
                    .ok_or_else(|| unsupported("mode code not a Uniden mode"))?;
                (format!("RM {name}\r").into_bytes(), ReplyKind::Ack)
            }
            Op::GetSignal => (b"SG\r".to_vec(), ReplyKind::Observation(Field::Signal)),
            _ => return Err(unsupported("not implemented for the Uniden engine")),
        })
    }

    // [@ANCHOR: radioctl_uniden_dialect_status]
    fn status(&mut self, expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>> {
        match line {
            "OK" => Some(if expecting == ReplyKind::Ack {
                vec![Inbound::Ack]
            } else {
                Vec::new()
            }),
            "NG" | "ORER" | "ERR" => Some(vec![Inbound::Nak]),
            "" => Some(Vec::new()),
            _ => None,
        }
    }

    // [@ANCHOR: radioctl_uniden_dialect_decode]
    fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound> {
        let unknown = || vec![Inbound::Unrecognised(line.as_bytes().to_vec())];
        let ReplyKind::Observation(field) = expecting else {
            return unknown();
        };
        let found = match field {
            Field::Frequency(_) => {
                let digits = if self.frequency_via_sg {
                    line.split_once(" F").map(|(_, d)| d)
                } else {
                    line.strip_prefix("RF")
                };
                digits
                    .and_then(parse_units)
                    .map(|hz| Observation::Frequency(Vfo::A, hz))
            }
            Field::Mode => line
                .strip_prefix("RM ")
                .and_then(|name| mode_named(desc, name))
                .map(Observation::Mode),
            Field::Signal => line
                .strip_prefix('S')
                .and_then(|rest| rest.split(' ').next())
                .and_then(|n| n.parse::<u16>().ok())
                .map(Observation::Signal),
            _ => None,
        };
        found.map_or_else(unknown, |o| vec![Inbound::Observation(o)])
    }
}

// [@ANCHOR: radioctl_uniden_parse_units]
/// A frequency from eight digits in units of 100 Hz.
fn parse_units(digits: &str) -> Option<Hz> {
    (digits.len() == 8 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse::<u64>().ok().map(|u| Hz(u * 100)))
        .flatten()
}

// [@ANCHOR: radioctl_uniden_mode_named]
/// The mode a radio's name stands for (`NFM` reads as FM), if the description has it.
fn mode_named(desc: &RigDescription, name: &str) -> Option<Mode> {
    let name = if name == "NFM" { "FM" } else { name };
    MODE_NAMES
        .iter()
        .find(|(_, n, _)| *n == name)
        .map(|(_, _, mode)| *mode)
        .filter(|mode| desc.mode_code(*mode).is_some())
}

/// The virtual Uniden scanner.
#[derive(Debug)]
pub struct UnidenTwin {
    desc: RigDescription,
}

impl UnidenTwin {
    // [@ANCHOR: radioctl_uniden_twin_new]
    /// The twin of a Uniden description.
    pub fn new(desc: &RigDescription) -> Result<UnidenTwin, RigError> {
        UnidenDialect::from_description(desc)?;
        Ok(UnidenTwin { desc: desc.clone() })
    }
}

impl ReceiverTwin for UnidenTwin {
    // [@ANCHOR: radioctl_uniden_twin_line]
    fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8> {
        let reply = |text: String| format!("{text}\r").into_bytes();
        let name_of = |mode: Mode| {
            self.desc
                .mode_code(mode)
                .and_then(|c| MODE_NAMES.iter().find(|(code, _, _)| *code == c))
                .map_or("FM", |(_, n, _)| *n)
        };
        match line {
            "RF" => reply(format!("RF{:08}", state.freq_a.0 / 100)),
            "RM" => reply(format!("RM {}", name_of(state.mode))),
            "SG" => reply(format!("S{:03} F{:08}", state.signal, state.freq_a.0 / 100)),
            _ => {
                if let Some(digits) = line.strip_prefix("RF") {
                    return match parse_units(digits) {
                        Some(hz) => {
                            state.freq_a = hz;
                            reply("OK".into())
                        }
                        None => reply("ERR".into()),
                    };
                }
                if let Some(name) = line.strip_prefix("RM ") {
                    return match mode_named(&self.desc, name) {
                        Some(mode) if name != "NFM" => {
                            state.mode = mode;
                            reply("OK".into())
                        }
                        Some(_) => reply("NG".into()),
                        None => reply("ERR".into()),
                    };
                }
                reply("ERR".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use radioctl_core::engine::ProtocolEngine;

    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_uniden_engine]
    // Tests [@ANCHOR: radioctl_uniden_dialect_from_description]
    // Tests [@ANCHOR: radioctl_uniden_dialect_encode]
    #[test]
    fn commands_are_spelled_as_the_rf_rm_sg_set() {
        let mut e = engine(shipped("uniden-bc245xlt")).expect("engine");
        let bytes = |e: &mut UnidenEngine, op: Op| e.encode(&op).expect("encode").bytes;
        assert_eq!(
            bytes(&mut e, Op::SetFrequency(Vfo::A, Hz(154_445_000))),
            b"RF01544450\r"
        );
        assert_eq!(bytes(&mut e, Op::GetFrequency(Vfo::A)), b"RF\r");
        assert_eq!(bytes(&mut e, Op::SetMode(Mode::Wfm)), b"RM WFM\r");
        assert_eq!(bytes(&mut e, Op::GetMode), b"RM\r");
        assert_eq!(bytes(&mut e, Op::GetSignal), b"SG\r");
        assert!(e.encode(&Op::SetMode(Mode::Usb)).is_err());
        assert!(e
            .encode(&Op::SetFrequency(Vfo::A, Hz(10_000_000_000)))
            .is_err());
        let mut b780 = engine(shipped("uniden-bc780xlt")).expect("engine");
        assert_eq!(bytes(&mut b780, Op::GetFrequency(Vfo::A)), b"SG\r");
        assert!(engine(shipped("aor-ar8000")).is_err(), "wrong protocol");
    }

    // Tests [@ANCHOR: radioctl_uniden_dialect_status]
    // Tests [@ANCHOR: radioctl_uniden_dialect_decode]
    // Tests [@ANCHOR: radioctl_uniden_parse_units]
    // Tests [@ANCHOR: radioctl_uniden_mode_named]
    #[test]
    fn answers_decode_and_refusals_are_naks() {
        let mut e = engine(shipped("uniden-bc245xlt")).expect("engine");
        e.encode(&Op::SetMode(Mode::Fm)).expect("set");
        assert_eq!(e.feed(b"OK\r", T0), vec![Inbound::Ack]);
        for refusal in [&b"NG\r"[..], b"ERR\r", b"ORER\r"] {
            e.encode(&Op::SetMode(Mode::Fm)).expect("set");
            assert_eq!(e.feed(refusal, T0), vec![Inbound::Nak]);
        }
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(
            e.feed(b"RF01544450\r", T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(154_445_000)
            ))]
        );
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(matches!(
            e.feed(b"RF0154\r", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::GetMode).expect("get");
        assert_eq!(
            e.feed(b"RM NFM\r", T0),
            vec![Inbound::Observation(Observation::Mode(Mode::Fm))]
        );
        e.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            e.feed(b"S182 F08594375\r", T0),
            vec![Inbound::Observation(Observation::Signal(182))]
        );
        let mut b780 = engine(shipped("uniden-bc780xlt")).expect("engine");
        b780.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(
            b780.feed(b"S182 F08594375\r", T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(859_437_500)
            ))]
        );
    }

    // Tests [@ANCHOR: radioctl_uniden_twin_new]
    // Tests [@ANCHOR: radioctl_uniden_twin_line]
    #[test]
    fn the_twin_answers_as_the_scanner_does() {
        let desc = shipped("uniden-bc245xlt");
        let mut twin = UnidenTwin::new(&desc).expect("twin");
        let mut state = crate::virtual_rig::VirtualRig::from_description(&desc)
            .expect("rig")
            .state()
            .clone();
        assert_eq!(twin.line(&mut state, "RF1544"), b"ERR\r");
        assert_eq!(twin.line(&mut state, "RM XFM"), b"ERR\r");
        assert_eq!(twin.line(&mut state, "RM NFM"), b"NG\r");
        assert_eq!(twin.line(&mut state, "RF01544450"), b"OK\r");
        assert_eq!(state.freq_a, Hz(154_445_000));
        assert!(UnidenTwin::new(&shipped("aor-ar8000")).is_err());
    }
}

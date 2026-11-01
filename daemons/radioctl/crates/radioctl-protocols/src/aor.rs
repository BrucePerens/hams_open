// SPDX-License-Identifier: AGPL-3.0-or-later
//! AOR receivers and scanners with the `RF` / `MD` / `LM` ASCII command set (AR8000, AR8200,
//! AR8600, AR5000, AR5000A, AR2700, SR2200): sans-IO client engine and virtual twin.
//!
//! **Source.** Hamlib 4.6.2 (released source tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8), read for facts only:
//! `rigs/aor/aor.c` (`aor_transaction`, `format_freq`, `aor_set_freq`, `aor_get_freq`,
//! `aor_set_mode`, `aor_get_mode`, `aor_get_level`, `format8k_mode`), `ar2700.c` (`format2700_mode`),
//! `ar5000.c` (`format5k_mode`), `sr2200.c` (`sr2200_get_level`, the SR2200 mode codes). No code or
//! comment was copied and no AOR manual was read; every fact is `hamlib-source` and unverified.
//!
//! * Commands end with CR. Hamlib reads every answer up to a CR after dropping a leading LF, and
//!   waits for one after a setting too, so a setting is acknowledged by an empty line and the
//!   engine treats an empty line as `Ack`. An answer that starts with `?` is a refusal.
//! * Frequency: `RF` and ten digits of hertz, rounded to a multiple of 50 Hz (below 25 Hz of the
//!   hundred rounds down, below 75 up to 50, else up to 100); read with `RX`, whose answer
//!   contains `RF` and the digits (the AR8000 may use `VA` or `VB` instead).
//!   The SR2200 sends the frequency as given and answers with `RF` and the digits.
//! * Mode: `MD` and one code character, read with `MD`. The codes differ by family and are in the
//!   description's mode table. The AR5000 also takes a bandwidth with `BW`; not used here.
//! * Signal: `LM` and a reading (the AR8000: two hex digits, bit 7 is squelch status; AR8200 and
//!   AR8600: a status character then decimal; the others: a status character then hex), and on
//!   the SR2200 `LB`, a space and a signed decimal in dB that Hamlib offsets by 73.
//!
//! Not implemented: the AR3000A, AR3030 and AR7030 families (different command sets, see
//! `docs/RIG_COVERAGE_PLAN.md`), memories, banks, scans, bandwidth, attenuator, AGC, squelch.

use radioctl_core::description::{ReceiverProtocol, RigDescription};
use radioctl_core::engine::{Inbound, ReplyKind};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Vfo};

use crate::receiver::{Dialect, LineEngine, ReceiverTwin};
use crate::virtual_rig::RigState;

/// The AOR engine.
pub type AorEngine = LineEngine<AorDialect>;

// [@ANCHOR: radioctl_aor_engine]
/// The engine for an AOR description.
pub fn engine(desc: RigDescription) -> Result<AorEngine, RigError> {
    LineEngine::new(desc, ReceiverProtocol::Aor)
}

/// How a model's `LM` / `LB` signal reading is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignalStyle {
    /// `LMxx`: two hex digits, bit 7 squelch status (AR8000).
    HexMasked,
    /// `LM`, a status character, decimal digits (AR8200, AR8600).
    StatusDecimal,
    /// `LM`, a status character, hex digits (AR5000, AR5000A, AR2700).
    StatusHex,
    /// `LB`, a space, a signed decimal in dB offset by 73 (SR2200).
    Lb,
}

/// Offset Hamlib adds to the SR2200's dB reading to refer it to S9.
const SR2200_OFFSET: i32 = 73;

/// The AOR command set of one model.
#[derive(Debug, Clone, Copy)]
pub struct AorDialect {
    signal: SignalStyle,
    /// The AR8000 may answer `RX` with `VA` or `VB` in place of `RF`.
    vfo_tags: bool,
    /// The SR2200 sends the frequency exactly (no rounding to 50 Hz) and answers a frequency
    /// setting with `RF` and the frequency, which Hamlib's `sr2200_set_freq` requires.
    echo_frequency: bool,
}

impl Dialect for AorDialect {
    // [@ANCHOR: radioctl_aor_dialect_from_description]
    fn from_description(desc: &RigDescription) -> Result<AorDialect, RigError> {
        let name = desc.receiver.as_ref().map_or("", |r| r.dialect.as_str());
        let (signal, vfo_tags) = match name {
            "ar8000" => (SignalStyle::HexMasked, true),
            "ar8200" | "ar8600" => (SignalStyle::StatusDecimal, false),
            "ar5000" | "ar2700" => (SignalStyle::StatusHex, false),
            "sr2200" => (SignalStyle::Lb, false),
            other => {
                return Err(RigError::new(
                    ErrorKind::Unsupported,
                    format!("unknown AOR dialect {other:?}"),
                ))
            }
        };
        Ok(AorDialect {
            signal,
            vfo_tags,
            echo_frequency: name == "sr2200",
        })
    }

    // [@ANCHOR: radioctl_aor_dialect_encode]
    fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError> {
        let text = match op {
            Op::GetFrequency(Vfo::A) => {
                return Ok((
                    b"RX\r".to_vec(),
                    ReplyKind::Observation(Field::Frequency(Vfo::A)),
                ))
            }
            Op::SetFrequency(Vfo::A, hz) if self.echo_frequency => {
                if hz.0 > 9_999_999_999 {
                    return Err(RigError::new(
                        ErrorKind::Unsupported,
                        "frequency out of range",
                    ));
                }
                return Ok((format!("RF{:010}\r", hz.0).into_bytes(), ReplyKind::Ack));
            }
            Op::SetFrequency(Vfo::A, hz) => {
                let rounded = round_to_50_hz(*hz).ok_or_else(|| {
                    RigError::new(ErrorKind::Unsupported, "frequency out of range")
                })?;
                format!("RF{rounded:010}\r")
            }
            Op::GetMode => return Ok((b"MD\r".to_vec(), ReplyKind::Observation(Field::Mode))),
            Op::SetMode(mode) => {
                let code = desc.mode_code(*mode).ok_or_else(|| {
                    RigError::new(ErrorKind::Unsupported, "mode not in the rig description")
                })?;
                format!("MD{}\r", char::from(code))
            }
            Op::GetSignal => {
                let command = if self.signal == SignalStyle::Lb {
                    "LB\r"
                } else {
                    "LM\r"
                };
                return Ok((
                    command.as_bytes().to_vec(),
                    ReplyKind::Observation(Field::Signal),
                ));
            }
            _ => {
                return Err(RigError::new(
                    ErrorKind::Unsupported,
                    "not implemented for the AOR engine",
                ))
            }
        };
        Ok((text.into_bytes(), ReplyKind::Ack))
    }

    // [@ANCHOR: radioctl_aor_dialect_decode]
    fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound> {
        let unknown = || vec![Inbound::Unrecognised(line.as_bytes().to_vec())];
        let ReplyKind::Observation(field) = expecting else {
            // The SR2200 answers a frequency setting with the frequency; that is its acknowledgement.
            if expecting == ReplyKind::Ack && self.echo_frequency && line.starts_with("RF") {
                return vec![Inbound::Ack];
            }
            return unknown();
        };
        let parsed = match field {
            Field::Frequency(_) => self
                .tagged_frequency(line)
                .map(|hz| Observation::Frequency(Vfo::A, hz)),
            Field::Mode => line
                .find("MD")
                .and_then(|at| line.as_bytes().get(at + 2))
                .and_then(|code| desc.code_mode(*code))
                .map(Observation::Mode),
            Field::Signal => self.signal_reading(line).map(Observation::Signal),
            _ => None,
        };
        parsed.map_or_else(unknown, |o| vec![Inbound::Observation(o)])
    }
}

impl AorDialect {
    // [@ANCHOR: radioctl_aor_tagged_frequency]
    /// The frequency after the first `RF` (or, on the AR8000, `VA` or `VB`) in a line.
    fn tagged_frequency(&self, line: &str) -> Option<Hz> {
        let tags: &[&str] = if self.vfo_tags {
            &["RF", "VA", "VB"]
        } else {
            &["RF"]
        };
        tags.iter().find_map(|tag| {
            let rest = line.get(line.find(tag)? + 2..)?;
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u64>().ok().map(Hz)
        })
    }

    // [@ANCHOR: radioctl_aor_signal_reading]
    /// The raw signal value of an `LM` or `LB` line, `None` if the line is not one.
    fn signal_reading(&self, line: &str) -> Option<u16> {
        let tag = if self.signal == SignalStyle::Lb {
            "LB"
        } else {
            "LM"
        };
        if line.len() < 4 || !line.starts_with(tag) {
            return None;
        }
        let digits = |from: usize| line.get(from..).map(str::trim);
        match self.signal {
            SignalStyle::HexMasked => u16::from_str_radix(digits(2)?, 16).ok().map(|v| v & !0x80),
            SignalStyle::StatusDecimal => digits(3)?.parse::<u16>().ok(),
            SignalStyle::StatusHex => u16::from_str_radix(digits(3)?, 16).ok(),
            SignalStyle::Lb => {
                let db: i32 = digits(3)?.parse().ok()?;
                u16::try_from((db + SR2200_OFFSET).max(0)).ok()
            }
        }
    }
}

// [@ANCHOR: radioctl_aor_round_to_50_hz]
/// A frequency rounded as the AOR command needs it (a multiple of 50 Hz), `None` above the ten
/// digits the command holds.
pub fn round_to_50_hz(hz: Hz) -> Option<u64> {
    let low = hz.0 % 100;
    let rounded = hz.0 / 100 * 100
        + if low < 25 {
            0
        } else if low < 75 {
            50
        } else {
            100
        };
    (rounded <= 9_999_999_999).then_some(rounded)
}

/// The virtual AOR receiver.
#[derive(Debug)]
pub struct AorTwin {
    dialect: AorDialect,
    modes: Vec<u8>,
    desc: RigDescription,
}

impl AorTwin {
    // [@ANCHOR: radioctl_aor_twin_new]
    /// The twin of an AOR description.
    pub fn new(desc: &RigDescription) -> Result<AorTwin, RigError> {
        Ok(AorTwin {
            dialect: AorDialect::from_description(desc)?,
            modes: desc.mode_codes.iter().map(|(_, c)| *c).collect(),
            desc: desc.clone(),
        })
    }

    // [@ANCHOR: radioctl_aor_twin_reply]
    /// A reply line: a leading line feed and the closing CR, as Hamlib reads them.
    fn reply(text: &str) -> Vec<u8> {
        format!("\n{text}\r").into_bytes()
    }
}

impl ReceiverTwin for AorTwin {
    // [@ANCHOR: radioctl_aor_twin_line]
    fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8> {
        let refuse = || Self::reply("?");
        match (line.get(..2).unwrap_or(""), line.get(2..).unwrap_or("")) {
            ("RX", "") => {
                let mode = self.desc.mode_code(state.mode).map_or('0', char::from);
                Self::reply(&format!("RF{:010} MD{mode}", state.freq_a.0))
            }
            ("RF", digits) if digits.len() == 10 && digits.bytes().all(|b| b.is_ascii_digit()) => {
                match digits.parse::<u64>() {
                    Ok(hz) if hz % 50 == 0 || self.dialect.echo_frequency => {
                        state.freq_a = Hz(hz);
                        if self.dialect.echo_frequency {
                            Self::reply(&format!("RF{hz:010}"))
                        } else {
                            Self::reply("")
                        }
                    }
                    _ => refuse(),
                }
            }
            ("MD", "") => {
                let mode = self.desc.mode_code(state.mode).map_or('0', char::from);
                Self::reply(&format!("MD{mode}"))
            }
            ("MD", code) if code.len() == 1 => {
                let byte = code.as_bytes().first().copied().unwrap_or(0);
                match self
                    .desc
                    .code_mode(byte)
                    .filter(|_| self.modes.contains(&byte))
                {
                    Some(mode) => {
                        state.mode = mode;
                        Self::reply("")
                    }
                    None => refuse(),
                }
            }
            ("LM", "") if self.dialect.signal != SignalStyle::Lb => {
                let s = state.signal;
                let status = if s == 0 { '%' } else { '$' };
                Self::reply(&match self.dialect.signal {
                    SignalStyle::HexMasked => format!("LM{:02X}", s & 0x7F),
                    SignalStyle::StatusDecimal => format!("LM{status}{s:03}"),
                    _ => format!("LM{status}{s:02X}"),
                })
            }
            ("LB", "") if self.dialect.signal == SignalStyle::Lb => Self::reply(&format!(
                "LB {:+04}",
                i32::from(state.signal) - SR2200_OFFSET
            )),
            _ => refuse(),
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

    // Tests [@ANCHOR: radioctl_aor_round_to_50_hz]
    #[test]
    fn frequencies_round_to_fifty_hertz_as_hamlib_does() {
        assert_eq!(round_to_50_hz(Hz(145_500_000)), Some(145_500_000));
        assert_eq!(round_to_50_hz(Hz(145_500_024)), Some(145_500_000));
        assert_eq!(round_to_50_hz(Hz(145_500_025)), Some(145_500_050));
        assert_eq!(round_to_50_hz(Hz(145_500_074)), Some(145_500_050));
        assert_eq!(round_to_50_hz(Hz(145_500_075)), Some(145_500_100));
        assert_eq!(round_to_50_hz(Hz(10_000_000_000)), None);
    }

    // Tests [@ANCHOR: radioctl_aor_engine]
    // Tests [@ANCHOR: radioctl_aor_dialect_from_description]
    // Tests [@ANCHOR: radioctl_aor_dialect_encode]
    #[test]
    fn commands_are_spelled_as_the_rf_md_lm_set() {
        let mut e = engine(shipped("aor-ar8000")).expect("engine");
        let bytes = |e: &mut AorEngine, op: Op| e.encode(&op).expect("encode").bytes;
        assert_eq!(
            bytes(&mut e, Op::SetFrequency(Vfo::A, Hz(145_500_030))),
            b"RF0145500050\r"
        );
        assert_eq!(bytes(&mut e, Op::GetFrequency(Vfo::A)), b"RX\r");
        assert_eq!(bytes(&mut e, Op::SetMode(Mode::Usb)), b"MD3\r");
        assert_eq!(bytes(&mut e, Op::GetMode), b"MD\r");
        assert_eq!(bytes(&mut e, Op::GetSignal), b"LM\r");
        assert!(e.encode(&Op::SetMode(Mode::Rtty)).is_err());
        assert!(e.encode(&Op::GetFrequency(Vfo::B)).is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        let mut sr = engine(shipped("aor-sr2200")).expect("engine");
        assert_eq!(bytes(&mut sr, Op::GetSignal), b"LB\r");
        assert!(engine(shipped("yaesu-ft-817")).is_err(), "wrong family");
    }

    // Tests [@ANCHOR: radioctl_aor_dialect_decode]
    // Tests [@ANCHOR: radioctl_aor_tagged_frequency]
    // Tests [@ANCHOR: radioctl_aor_signal_reading]
    // Tests [@ANCHOR: radioctl_receiver_engine_description]
    #[test]
    fn answers_are_read_by_tag_and_refusals_are_naks() {
        let mut e = engine(shipped("aor-ar8000")).expect("engine");
        assert_eq!(e.description().id, "aor-ar8000");
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(
            e.feed(b"\nVA RF0145500000 MD1\r", T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(145_500_000)
            ))]
        );
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(matches!(
            e.feed(b"\nXX\r", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::GetMode).expect("get");
        assert_eq!(
            e.feed(b"\nMD4\r", T0),
            vec![Inbound::Observation(Observation::Mode(Mode::Lsb))]
        );
        e.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            e.feed(b"\nLMA5\r", T0),
            vec![Inbound::Observation(Observation::Signal(0x25))],
            "bit 7 is squelch status"
        );
        e.encode(&Op::SetMode(Mode::Fm)).expect("set");
        assert_eq!(e.feed(b"\n?\r", T0), vec![Inbound::Nak]);
        e.encode(&Op::SetMode(Mode::Fm)).expect("set");
        assert_eq!(e.feed(b"\n\r", T0), vec![Inbound::Ack]);
        let mut sr = engine(shipped("aor-sr2200")).expect("engine");
        sr.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            sr.feed(b"\nLB -063\r", T0),
            vec![Inbound::Observation(Observation::Signal(10))]
        );
        let mut a8 = engine(shipped("aor-ar8200")).expect("engine");
        a8.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            a8.feed(b"\nLM$045\r", T0),
            vec![Inbound::Observation(Observation::Signal(45))]
        );
    }

    // Tests [@ANCHOR: radioctl_aor_twin_new]
    // Tests [@ANCHOR: radioctl_aor_twin_line]
    // Tests [@ANCHOR: radioctl_aor_twin_reply]
    #[test]
    fn the_twin_refuses_what_the_radio_would() {
        let desc = shipped("aor-ar5000");
        let mut twin = AorTwin::new(&desc).expect("twin");
        let mut state = crate::virtual_rig::VirtualRig::from_description(&desc)
            .expect("rig")
            .state()
            .clone();
        assert_eq!(twin.line(&mut state, "MD9"), b"\n?\r");
        assert_eq!(twin.line(&mut state, "RF0145500025"), b"\n?\r");
        assert_eq!(twin.line(&mut state, "LB"), b"\n?\r");
        assert_eq!(twin.line(&mut state, "RF0145500050"), b"\n\r");
        assert_eq!(state.freq_a, Hz(145_500_050));
        assert!(AorTwin::new(&shipped("yaesu-ft-817")).is_err());
    }
}

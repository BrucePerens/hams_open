// SPDX-License-Identifier: AGPL-3.0-or-later
//! AOR AR3030 and AR3000A, whose ASCII command sets differ from the `RF` / `MD` / `LM` family of
//! `aor.rs`: sans-IO client engine and virtual twin.
//!
//! **Sources.** The AR3030 facts were read in the "AR3030 operating manual", section 7 "Computer
//! control (RS232C)" (printed pages 40 to 44, PDF pages 41 to 46; sha256 a73307a5d107479753a0a1620838725d98270f4127b5fd7eb81c0ad74fde53ce,
//! kept in `~/manuals/aor/`). The AR3000A facts were read in the "AR3000A" operating manual (a
//! scanned copy, read by OCR and checked against page images), section 6-6 "Computer control",
//! printed pages 48 to 51 (PDF pages 50 to 53; sha256 in `~/manuals/aor/SHA256SUMS`). Hamlib 4.6.2's
//! `rigs/aor/ar3030.c` and `ar3000.c` (released source tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8) were read for facts only, for the
//! details the manuals leave open. No code or comment was copied.
//!
//! **AR3030.** Commands end with CR; every command is answered with CR LF (an empty line is the
//! acknowledgement). `D` answers the dial state, `D R<att> G<agc> B<filter> T<tone> F<eight digits>
//! <mode>` with the frequency in units of 10 Hz; `Y` answers the signal level as two hex digits
//! (00 to 3F). A frequency is entered in MHz (`7.074000`), a mode as one letter (`C` CW, `U` USB,
//! `L` LSB, `A` AM, `S` synchronous AM, `N` FM, `X` fax). Start-up on the radio's keypad (type the
//! baud rate, 4800 or 9600, and ENT); `Q` ends remote control.
//!
//! **AR3000A.** Commands end with CR (Hamlib sends CR LF). The radio prints a `>` prompt and does
//! not acknowledge settings, as far as Hamlib reads it. `D` answers the dial state: memory channel,
//! `J0`, attenuator (`W` on, `X` off), step (`Z` and kHz), `Y` and the frequency in MHz, then the mode
//! letter (`Q` FM, `R` wide FM, `S` AM, `T` LSB, `U` USB, `V` CW); `Y` answers the signal as one
//! letter, `A` (lowest) to `P` (highest) in 16 steps, or `%` while the squelch is closed. A frequency
//! is entered in MHz (any multiple of 50 Hz) and a mode as `N`, `W`, `A`, `L`, `U` or `C`.
//!
//! Not implemented: memories, banks, passes, the attenuator and AGC, tone and filter selection,
//! step size, the radio's mute commands.

use radioctl_core::description::{ReceiverProtocol, RigDescription};
use radioctl_core::engine::{Inbound, ReplyKind};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Mode, Vfo};

use crate::aor::round_to_50_hz;
use crate::receiver::{Dialect, LineEngine, ReceiverTwin};
use crate::virtual_rig::RigState;

/// The engine of the AR3030 and AR3000A.
pub type AorClassicEngine = LineEngine<AorClassicDialect>;

// [@ANCHOR: radioctl_aorc_engine]
/// The engine for an AR3030 or AR3000A description.
pub fn engine(desc: RigDescription) -> Result<AorClassicEngine, RigError> {
    LineEngine::new(desc, ReceiverProtocol::Aor)
}

/// Which of the two command sets a model speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    Ar3030,
    Ar3000a,
}

/// The AR3030 frequency range in hertz (30 kHz to 30 MHz).
const AR3030_RANGE: (u64, u64) = (30_000, 30_000_000);

/// The AR3000A frequency range in hertz (100 kHz to 2036 MHz).
const AR3000A_RANGE: (u64, u64) = (100_000, 2_036_000_000);

/// The mode letters the AR3000A reports in its dial state, with their radioctl modes.
const AR3000A_READ: [(char, Mode); 6] = [
    ('Q', Mode::Fm),
    ('R', Mode::Wfm),
    ('S', Mode::Am),
    ('T', Mode::Lsb),
    ('U', Mode::Usb),
    ('V', Mode::Cw),
];

/// The AR3030 mode letters, the same for setting and reading (`S` and `X` have no radioctl mode).
const AR3030_LETTERS: [(char, Mode); 5] = [
    ('C', Mode::Cw),
    ('U', Mode::Usb),
    ('L', Mode::Lsb),
    ('A', Mode::Am),
    ('N', Mode::Fm),
];

/// The command set of one of the two models.
#[derive(Debug, Clone, Copy)]
pub struct AorClassicDialect {
    style: Style,
}

// [@ANCHOR: radioctl_aorc_style_of]
/// The style of a description's dialect, `None` for any other.
fn style_of(desc: &RigDescription) -> Option<Style> {
    match desc.receiver.as_ref().map_or("", |r| r.dialect.as_str()) {
        "ar3030" => Some(Style::Ar3030),
        "ar3000a" => Some(Style::Ar3000a),
        _ => None,
    }
}

/// True when the description's dialect is `ar3030` or `ar3000a`.
// [@ANCHOR: radioctl_aorc_handles]
pub fn handles(desc: &RigDescription) -> bool {
    style_of(desc).is_some()
}

// [@ANCHOR: radioctl_aorc_megahertz]
/// A frequency as megahertz text with the given number of decimals, from integers only.
fn megahertz(hz: u64, decimals: u32) -> String {
    let unit = 10u64.pow(6 - decimals);
    let scaled = hz / unit;
    let per = 10u64.pow(decimals);
    format!(
        "{}.{:0width$}",
        scaled / per,
        scaled % per,
        width = decimals as usize
    )
}

// [@ANCHOR: radioctl_aorc_parse_megahertz]
/// A decimal megahertz text (`7.074`, `0145.50000`) as hertz, rounded to 10 Hz; `None` for anything
/// else.
fn parse_megahertz(text: &str) -> Option<u64> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    if !whole.bytes().all(|b| b.is_ascii_digit()) || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let digits = format!("{fraction:0<6}");
    let micro: u64 = digits.get(..6)?.parse().ok()?;
    let units = whole.checked_mul(1_000_000)?.checked_add(micro)?;
    // Round to 10 Hz.
    Some((units + 5) / 10 * 10)
}

impl Dialect for AorClassicDialect {
    // [@ANCHOR: radioctl_aorc_dialect_from_description]
    fn from_description(desc: &RigDescription) -> Result<AorClassicDialect, RigError> {
        style_of(desc)
            .map(|style| AorClassicDialect { style })
            .ok_or_else(|| {
                RigError::new(ErrorKind::Unsupported, "not an AR3030 or AR3000A dialect")
            })
    }

    // [@ANCHOR: radioctl_aorc_dialect_encode]
    fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError> {
        let unsupported = |why: &str| RigError::new(ErrorKind::Unsupported, why.to_string());
        let ar3030 = self.style == Style::Ar3030;
        let eol = if ar3030 { "\r" } else { "\r\n" };
        let ask = |text: &str, field| {
            (
                format!("{text}{eol}").into_bytes(),
                ReplyKind::Observation(field),
            )
        };
        Ok(match op {
            Op::GetFrequency(Vfo::A) => ask("D", Field::Frequency(Vfo::A)),
            Op::GetMode => ask("D", Field::Mode),
            Op::GetSignal => ask("Y", Field::Signal),
            Op::SetFrequency(Vfo::A, hz) if ar3030 => {
                if !(AR3030_RANGE.0..=AR3030_RANGE.1).contains(&hz.0) {
                    return Err(unsupported("frequency out of range"));
                }
                (
                    format!("{}{eol}", megahertz(hz.0, 6)).into_bytes(),
                    ReplyKind::Ack,
                )
            }
            Op::SetFrequency(Vfo::A, hz) => {
                let rounded = round_to_50_hz(*hz)
                    .filter(|r| (AR3000A_RANGE.0..=AR3000A_RANGE.1).contains(r))
                    .ok_or_else(|| unsupported("frequency out of range"))?;
                (
                    format!("{}{eol}", megahertz(rounded, 5)).into_bytes(),
                    ReplyKind::None,
                )
            }
            Op::SetMode(mode) => {
                let code = desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the rig description"))?;
                let reply = if ar3030 {
                    ReplyKind::Ack
                } else {
                    ReplyKind::None
                };
                (format!("{}{eol}", char::from(code)).into_bytes(), reply)
            }
            _ => {
                return Err(unsupported(
                    "not implemented for the AR3030 / AR3000A engine",
                ))
            }
        })
    }

    // [@ANCHOR: radioctl_aorc_dialect_status]
    fn status(&mut self, expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>> {
        if line.is_empty() {
            return Some(if expecting == ReplyKind::Ack {
                vec![Inbound::Ack]
            } else {
                Vec::new()
            });
        }
        // The AR3000A's ready prompt is not a reply.
        (line == ">").then(Vec::new)
    }

    // [@ANCHOR: radioctl_aorc_dialect_decode]
    fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound> {
        let unknown = || vec![Inbound::Unrecognised(line.as_bytes().to_vec())];
        let ReplyKind::Observation(field) = expecting else {
            return unknown();
        };
        let found = match (self.style, field) {
            (Style::Ar3030, Field::Frequency(_)) => {
                ar3030_state(line).map(|(hz, _)| Observation::Frequency(Vfo::A, Hz(hz)))
            }
            (Style::Ar3030, Field::Mode) => ar3030_state(line)
                .and_then(|(_, letter)| {
                    AR3030_LETTERS
                        .iter()
                        .find(|(l, _)| *l == letter)
                        .map(|(_, m)| *m)
                })
                .filter(|m| desc.mode_code(*m).is_some())
                .map(Observation::Mode),
            (Style::Ar3030, Field::Signal) => u16::from_str_radix(line.trim(), 16)
                .ok()
                .filter(|v| *v <= 0x3F)
                .map(Observation::Signal),
            (Style::Ar3000a, Field::Frequency(_)) => {
                ar3000a_state(line).map(|(hz, _)| Observation::Frequency(Vfo::A, Hz(hz)))
            }
            (Style::Ar3000a, Field::Mode) => ar3000a_state(line)
                .and_then(|(_, letter)| {
                    AR3000A_READ
                        .iter()
                        .find(|(l, _)| *l == letter)
                        .map(|(_, m)| *m)
                })
                .filter(|m| desc.mode_code(*m).is_some())
                .map(Observation::Mode),
            (Style::Ar3000a, Field::Signal) => match line.trim().chars().next() {
                Some('%') => Some(Observation::Signal(0)),
                Some(c @ 'A'..='P') => Some(Observation::Signal(u16::from(c as u8 - b'A') + 1)),
                _ => None,
            },
            _ => None,
        };
        found.map_or_else(unknown, |o| vec![Inbound::Observation(o)])
    }
}

// [@ANCHOR: radioctl_aorc_ar3030_state]
/// The frequency (hertz) and mode letter of an AR3030 dial-state line.
fn ar3030_state(line: &str) -> Option<(u64, char)> {
    let rest = line.strip_prefix('D')?;
    let after = rest.get(rest.find('F')? + 1..)?;
    let digits = after.get(..8)?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let units: u64 = digits.parse().ok()?;
    let letter = after.get(8..)?.trim_start().chars().next()?;
    Some((units * 10, letter))
}

// [@ANCHOR: radioctl_aorc_ar3000a_state]
/// The frequency (hertz) and mode letter of an AR3000A dial-state line: after the `Y`, nine digits
/// (megahertz with five decimals, a point or space between the fourth and fifth tolerated), then
/// the letter.
fn ar3000a_state(line: &str) -> Option<(u64, char)> {
    let after = line.get(line.find('Y')? + 1..)?;
    let mut digits = String::new();
    let mut letter = None;
    for c in after.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if c == '.' || c == ' ' {
            continue;
        } else {
            letter = Some(c);
            break;
        }
    }
    if digits.len() != 9 {
        return None;
    }
    let units: u64 = digits.parse().ok()?;
    Some((units * 10, letter?))
}

/// The virtual AR3030 or AR3000A.
#[derive(Debug)]
pub struct AorClassicTwin {
    style: Style,
    desc: RigDescription,
}

impl AorClassicTwin {
    // [@ANCHOR: radioctl_aorc_twin_new]
    /// The twin of an AR3030 or AR3000A description.
    pub fn new(desc: &RigDescription) -> Result<AorClassicTwin, RigError> {
        let dialect = AorClassicDialect::from_description(desc)?;
        Ok(AorClassicTwin {
            style: dialect.style,
            desc: desc.clone(),
        })
    }

    // [@ANCHOR: radioctl_aorc_twin_reply]
    /// One answer line, ended CR LF.
    fn reply(text: &str) -> Vec<u8> {
        format!("{text}\r\n").into_bytes()
    }

    // [@ANCHOR: radioctl_aorc_twin_state_line]
    /// The dial-state line.
    fn state_line(&self, state: &RigState) -> String {
        if self.style == Style::Ar3030 {
            let letter = AR3030_LETTERS
                .iter()
                .find(|(_, m)| *m == state.mode)
                .map_or('A', |(l, _)| *l);
            format!("D R0 G0 B0 T0 F{:08} {letter}", state.freq_a.0 / 10)
        } else {
            let letter = AR3000A_READ
                .iter()
                .find(|(_, m)| *m == state.mode)
                .map_or('S', |(l, _)| *l);
            format!(
                "#--J0X Z000.05 Y{}{letter}",
                format!("{:09}", state.freq_a.0 / 10).char_indices().fold(
                    String::new(),
                    |mut s, (i, c)| {
                        if i == 4 {
                            s.push('.');
                        }
                        s.push(c);
                        s
                    }
                )
            )
        }
    }

    // [@ANCHOR: radioctl_aorc_twin_apply]
    /// Applies a frequency and/or mode entry such as `7.074000`, `C` or `128.8A`.
    fn apply(&mut self, state: &mut RigState, line: &str) {
        let split = line
            .find(|c: char| c.is_ascii_alphabetic())
            .unwrap_or(line.len());
        let (number, letters) = line.split_at(split);
        if !number.is_empty() {
            let range = if self.style == Style::Ar3030 {
                AR3030_RANGE
            } else {
                AR3000A_RANGE
            };
            if let Some(hz) = parse_megahertz(number).filter(|h| (range.0..=range.1).contains(h)) {
                state.freq_a = Hz(if self.style == Style::Ar3030 {
                    hz
                } else {
                    round_to_50_hz(Hz(hz)).unwrap_or(hz)
                });
            }
        }
        if let Some(letter) = letters.chars().next() {
            if let Some(mode) = self
                .desc
                .mode_codes
                .iter()
                .find(|(_, c)| char::from(*c) == letter)
                .map(|(m, _)| *m)
            {
                state.mode = mode;
            }
        }
    }
}

impl ReceiverTwin for AorClassicTwin {
    // [@ANCHOR: radioctl_aorc_twin_line]
    fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8> {
        let ar3030 = self.style == Style::Ar3030;
        match line {
            "D" => Self::reply(&self.state_line(state)),
            "Y" if ar3030 => Self::reply(&format!("{:02X}", state.signal.min(0x3F))),
            "Y" => {
                let letter = if state.signal == 0 {
                    '%'
                } else {
                    char::from(b'A' + u8::try_from(state.signal.min(16) - 1).unwrap_or(0))
                };
                Self::reply(&letter.to_string())
            }
            "" => Self::reply(""),
            _ => {
                self.apply(state, line);
                if ar3030 {
                    Self::reply("")
                } else {
                    Vec::new()
                }
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

    fn fresh_state(desc: &RigDescription) -> RigState {
        crate::virtual_rig::VirtualRig::from_description(desc)
            .expect("rig")
            .state()
            .clone()
    }

    // Tests [@ANCHOR: radioctl_aorc_engine]
    // Tests [@ANCHOR: radioctl_aorc_style_of]
    // Tests [@ANCHOR: radioctl_aorc_handles]
    // Tests [@ANCHOR: radioctl_aorc_megahertz]
    // Tests [@ANCHOR: radioctl_aorc_dialect_from_description]
    // Tests [@ANCHOR: radioctl_aorc_dialect_encode]
    #[test]
    fn commands_follow_the_manuals() {
        let bytes = |e: &mut AorClassicEngine, op: Op| e.encode(&op).expect("encode").bytes;
        let mut a = engine(shipped("aor-ar3030")).expect("engine");
        assert_eq!(
            bytes(&mut a, Op::SetFrequency(Vfo::A, Hz(7_074_000))),
            b"7.074000\r"
        );
        assert_eq!(bytes(&mut a, Op::SetMode(Mode::Usb)), b"U\r");
        assert_eq!(bytes(&mut a, Op::GetFrequency(Vfo::A)), b"D\r");
        assert_eq!(bytes(&mut a, Op::GetSignal), b"Y\r");
        assert!(a.encode(&Op::SetFrequency(Vfo::A, Hz(10_000))).is_err());
        assert!(a.encode(&Op::SetMode(Mode::Wfm)).is_err());
        assert!(a.encode(&Op::SetPtt(true)).is_err());
        let mut b = engine(shipped("aor-ar3000a")).expect("engine");
        assert_eq!(
            bytes(&mut b, Op::SetFrequency(Vfo::A, Hz(128_800_030))),
            b"128.80005\r\n"
        );
        assert_eq!(bytes(&mut b, Op::SetMode(Mode::Wfm)), b"W\r\n");
        assert_eq!(bytes(&mut b, Op::GetMode), b"D\r\n");
        assert!(b.encode(&Op::SetFrequency(Vfo::A, Hz(50_000))).is_err());
        assert_eq!(megahertz(1_807_500, 5), "1.80750");
        assert!(
            engine(shipped("aor-ar5000")).is_err(),
            "the other AOR dialect"
        );
        assert!(handles(&shipped("aor-ar3030")) && !handles(&shipped("aor-ar5000")));
    }

    // Tests [@ANCHOR: radioctl_aorc_dialect_status]
    // Tests [@ANCHOR: radioctl_aorc_dialect_decode]
    // Tests [@ANCHOR: radioctl_aorc_ar3030_state]
    // Tests [@ANCHOR: radioctl_aorc_ar3000a_state]
    // Tests [@ANCHOR: radioctl_aorc_parse_megahertz]
    #[test]
    fn dial_state_lines_are_read() {
        let mut a = engine(shipped("aor-ar3030")).expect("engine");
        a.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(
            a.feed(b"D R0 G1 B0 T0 F00707400 U\r\n", T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(7_074_000)
            ))]
        );
        a.encode(&Op::GetMode).expect("get");
        assert_eq!(
            a.feed(b"D R0 G1 B0 T0 F00707400 U\r\n", T0),
            vec![Inbound::Observation(Observation::Mode(Mode::Usb))]
        );
        a.encode(&Op::GetMode).expect("get");
        assert!(matches!(
            a.feed(b"D R0 G1 B0 T0 F00707400 X\r\n", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        a.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            a.feed(b"2A\r\n", T0),
            vec![Inbound::Observation(Observation::Signal(0x2A))]
        );
        a.encode(&Op::SetMode(Mode::Am)).expect("set");
        assert_eq!(a.feed(b"\r\n", T0), vec![Inbound::Ack]);
        let mut b = engine(shipped("aor-ar3000a")).expect("engine");
        b.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(
            b.feed(b"#--J0X Z000.05 Y0145.50000Q\r\n", T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(145_500_000)
            ))]
        );
        b.encode(&Op::GetMode).expect("get");
        assert_eq!(
            b.feed(b"#--J0X Z000.05 Y014550000T\r\n", T0),
            vec![Inbound::Observation(Observation::Mode(Mode::Lsb))]
        );
        b.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            b.feed(b"F\r\n", T0),
            vec![Inbound::Observation(Observation::Signal(6))]
        );
        b.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            b.feed(b"%\r\n", T0),
            vec![Inbound::Observation(Observation::Signal(0))]
        );
        assert!(b.feed(b">\r\n", T0).is_empty(), "the ready prompt");
        assert_eq!(parse_megahertz("7.0740004"), Some(7_074_000));
        assert_eq!(parse_megahertz("7.074006"), Some(7_074_010));
        assert_eq!(parse_megahertz("x"), None);
        assert_eq!(parse_megahertz("."), None);
    }

    // Tests [@ANCHOR: radioctl_aorc_twin_new]
    // Tests [@ANCHOR: radioctl_aorc_twin_reply]
    // Tests [@ANCHOR: radioctl_aorc_twin_state_line]
    // Tests [@ANCHOR: radioctl_aorc_twin_apply]
    // Tests [@ANCHOR: radioctl_aorc_twin_line]
    #[test]
    fn the_twins_keep_the_documented_formats() {
        let desc = shipped("aor-ar3030");
        let mut twin = AorClassicTwin::new(&desc).expect("twin");
        let mut state = fresh_state(&desc);
        assert_eq!(twin.line(&mut state, "7.074000"), b"\r\n");
        assert_eq!(state.freq_a, Hz(7_074_000));
        assert_eq!(twin.line(&mut state, "40.0"), b"\r\n");
        assert_eq!(state.freq_a, Hz(7_074_000), "out of range is ignored");
        assert_eq!(twin.line(&mut state, "L"), b"\r\n");
        assert_eq!(state.mode, Mode::Lsb);
        assert_eq!(twin.line(&mut state, "D"), b"D R0 G0 B0 T0 F00707400 L\r\n");
        let desc = shipped("aor-ar3000a");
        let mut twin = AorClassicTwin::new(&desc).expect("twin");
        let mut state = fresh_state(&desc);
        assert!(twin.line(&mut state, "128.8A").is_empty());
        assert_eq!(state.freq_a, Hz(128_800_000));
        assert_eq!(state.mode, Mode::Am);
        let line = String::from_utf8(twin.line(&mut state, "D")).expect("text");
        assert!(line.contains("Y0128.80000S"), "{line}");
        state.signal = 0;
        assert_eq!(twin.line(&mut state, "Y"), b"%\r\n");
        assert!(AorClassicTwin::new(&shipped("aor-ar5000")).is_err());
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rohde and Schwarz HF receivers and transceivers with the line-feed framed `F` / `I` command sets
//! (EK 895, EK 896, XK 2100) and the `*F` / `*I` / `*O1` set (XK 852): sans-IO client engine and
//! virtual twin.
//!
//! **Sources.** The EK 895 / EK 896 command facts were read in the "VLF-HF Receivers EK 895 / EK 896
//! Operator Manual, General", chapter 2 "Remote Control" (document number 6075.6991.82.01 on its
//! pages; PDF pages 66 to 77; sha256 in `~/manuals/rohde_schwarz/SHA256SUMS`): block framing 2.1,
//! frequency 2.3.1, modulation mode 2.3.2, list of commands 2.6. The XK 2100 command set is that of
//! Hamlib's `gp2000.c` and the XK 852 set that of `xk852.c` (Hamlib 4.6.2, released source tarball,
//! sha256 b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8), read for facts only;
//! no manual for these two could be found. No code or comment was copied.
//!
//! * A command block starts with LF and ends with CR. An answer starts with LF and ends with CR; the
//!   engine drops the LF. A setting is answered only when the command asks for it.
//! * EK 895 / 896: `F<hertz>` sets, `F?` reads the frequency (0 to 30 MHz); `I<n>` sets and `I?`
//!   reads the modulation (15 = USB with the lower sideband monitored, 16 = LSB; 17 and 18 are the
//!   independent-sideband modes, which radioctl has no mode for); `L?` reads the level in dBuV.
//! * XK 2100 (Hamlib `gp2000.c`): `F<hertz>,<hertz>` sets, `F?` reads; `I<n>` sets and `I?` reads
//!   (1 AM, 2 USB, 3 LSB, 5 CW, 9 FM, 13 and 14 the two packet modes, which are not mapped).
//! * XK 852 (Hamlib `xk852.c`): `*F<7 digits>` sets the frequency in units of 10 Hz, `*I<n>` the mode
//!   (1 AM, 2 USB, 3 LSB, 5 CW); every setting is answered with a line; `*O1` answers with a
//!   fixed-column status line from which Hamlib reads the frequency and mode.
//!
//! Not implemented: the PTT of the XK 2100 and XK 852 (Hamlib sends the XK 2100's `X<n>` without
//! LF or CR, so its framing is doubtful), addressed operation (`A01` to `A99`), channels, the
//! preamplifier, bandwidth, audio and squelch.

use radioctl_core::description::{ReceiverProtocol, RigDescription};
use radioctl_core::engine::{Inbound, ReplyKind};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Vfo};

use crate::receiver::{Dialect, LineEngine, ReceiverTwin};
use crate::virtual_rig::RigState;

/// The engine of the Rohde and Schwarz HF dialects.
pub type RsHfEngine = LineEngine<RsHfDialect>;

// [@ANCHOR: radioctl_rshf_engine]
/// The engine for an EK 895, EK 896, XK 2100 or XK 852 description.
pub fn engine(desc: RigDescription) -> Result<RsHfEngine, RigError> {
    LineEngine::new(desc, ReceiverProtocol::RohdeSchwarz)
}

/// Which command set a model speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    /// EK 895 and EK 896: `F<hz>`, `I<n>`, `L?`.
    Ek,
    /// XK 2100: `F<hz>,<hz>`, `I<n>`.
    Gp2000,
    /// XK 852: `*F`, `*I`, `*O1`.
    Xk852,
}

/// The highest frequency the EK 895 and EK 896 accept.
const EK_MAX_HZ: u64 = 30_000_000;

/// The highest value of the XK 852's seven-digit frequency field (units of 10 Hz).
const XK852_MAX_UNITS: u64 = 9_999_999;

/// The command set of one Rohde and Schwarz HF model.
#[derive(Debug, Clone, Copy)]
pub struct RsHfDialect {
    style: Style,
}

/// True when the description names one of the HF dialects.
// [@ANCHOR: radioctl_rshf_handles]
pub fn handles(desc: &RigDescription) -> bool {
    style_of(desc).is_some()
}

// [@ANCHOR: radioctl_rshf_style_of]
/// The style of a description's dialect, `None` for any other.
fn style_of(desc: &RigDescription) -> Option<Style> {
    match desc.receiver.as_ref().map_or("", |r| r.dialect.as_str()) {
        "ek895" | "ek896" => Some(Style::Ek),
        "xk2100" => Some(Style::Gp2000),
        "xk852" => Some(Style::Xk852),
        _ => None,
    }
}

impl Dialect for RsHfDialect {
    // [@ANCHOR: radioctl_rshf_dialect_from_description]
    fn from_description(desc: &RigDescription) -> Result<RsHfDialect, RigError> {
        style_of(desc)
            .map(|style| RsHfDialect { style })
            .ok_or_else(|| {
                RigError::new(
                    ErrorKind::Unsupported,
                    "not a Rohde and Schwarz HF dialect (ek895, ek896, xk2100, xk852)",
                )
            })
    }

    // [@ANCHOR: radioctl_rshf_dialect_encode]
    fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError> {
        let unsupported = |why: &str| RigError::new(ErrorKind::Unsupported, why.to_string());
        let ask = |text: &str, field| {
            (
                format!("\n{text}\r").into_bytes(),
                ReplyKind::Observation(field),
            )
        };
        let mode_code = |mode| {
            desc.mode_code(mode)
                .ok_or_else(|| unsupported("mode not in the rig description"))
        };
        Ok(match (self.style, op) {
            (Style::Ek, Op::SetFrequency(Vfo::A, hz)) if hz.0 <= EK_MAX_HZ => {
                (format!("\nF{}\r", hz.0).into_bytes(), ReplyKind::None)
            }
            (Style::Gp2000, Op::SetFrequency(Vfo::A, hz)) if hz.0 <= EK_MAX_HZ => (
                format!("\nF{},{}\r", hz.0, hz.0).into_bytes(),
                ReplyKind::None,
            ),
            (Style::Xk852, Op::SetFrequency(Vfo::A, hz)) if (hz.0 + 5) / 10 <= XK852_MAX_UNITS => (
                format!("\n*F{:07}\r", (hz.0 + 5) / 10).into_bytes(),
                ReplyKind::Ack,
            ),
            (_, Op::SetFrequency(Vfo::A, _)) => return Err(unsupported("frequency out of range")),
            (Style::Ek | Style::Gp2000, Op::GetFrequency(Vfo::A)) => {
                ask("F?", Field::Frequency(Vfo::A))
            }
            (Style::Xk852, Op::GetFrequency(Vfo::A)) => ask("*O1", Field::Frequency(Vfo::A)),
            (Style::Ek | Style::Gp2000, Op::SetMode(mode)) => (
                format!("\nI{}\r", mode_code(*mode)?).into_bytes(),
                ReplyKind::None,
            ),
            (Style::Xk852, Op::SetMode(mode)) => (
                format!("\n*I{}\r", mode_code(*mode)?).into_bytes(),
                ReplyKind::Ack,
            ),
            (Style::Ek | Style::Gp2000, Op::GetMode) => ask("I?", Field::Mode),
            (Style::Xk852, Op::GetMode) => ask("*O1", Field::Mode),
            (Style::Ek, Op::GetSignal) => ask("L?", Field::Signal),
            _ => {
                return Err(unsupported(
                    "not implemented for the Rohde and Schwarz HF engine",
                ))
            }
        })
    }

    // [@ANCHOR: radioctl_rshf_dialect_status]
    fn status(&mut self, expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>> {
        match self.style {
            // The XK 852 answers every setting with some line; Hamlib discards its content.
            Style::Xk852 => (expecting == ReplyKind::Ack).then(|| vec![Inbound::Ack]),
            // The others do not answer settings; a stray empty line is dropped.
            Style::Ek | Style::Gp2000 => line.is_empty().then(Vec::new),
        }
    }

    // [@ANCHOR: radioctl_rshf_dialect_decode]
    fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound> {
        let unknown = || vec![Inbound::Unrecognised(line.as_bytes().to_vec())];
        let ReplyKind::Observation(field) = expecting else {
            return unknown();
        };
        let found = match (self.style, field) {
            (Style::Xk852, Field::Frequency(_)) => tagged_digits(line, "*F", 7)
                .map(|units| Observation::Frequency(Vfo::A, Hz(units * 10))),
            (Style::Xk852, Field::Mode) => tagged_digits(line, "*I", 1)
                .and_then(|code| u8::try_from(code).ok())
                .and_then(|code| desc.code_mode(code))
                .map(Observation::Mode),
            (_, Field::Frequency(_)) => {
                number_after(line, "F").map(|hz| Observation::Frequency(Vfo::A, Hz(hz)))
            }
            (_, Field::Mode) => number_after(line, "I")
                .and_then(|code| u8::try_from(code).ok())
                .and_then(|code| desc.code_mode(code))
                .map(Observation::Mode),
            (_, Field::Signal) => number_after(line, "L")
                .map(|level| Observation::Signal(u16::try_from(level).unwrap_or(u16::MAX))),
            _ => None,
        };
        found.map_or_else(unknown, |o| vec![Inbound::Observation(o)])
    }
}

// [@ANCHOR: radioctl_rshf_number_after]
/// The unsigned number that follows `tag` at the start of a reply line.
fn number_after(line: &str, tag: &str) -> Option<u64> {
    let digits: String = line
        .strip_prefix(tag)?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

// [@ANCHOR: radioctl_rshf_tagged_digits]
/// The `count` digits that follow the first `tag` anywhere in a status line.
fn tagged_digits(line: &str, tag: &str, count: usize) -> Option<u64> {
    let rest = line.get(line.find(tag)? + tag.len()..)?;
    let digits: String = rest.chars().take(count).collect();
    (digits.len() == count && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

/// The virtual Rohde and Schwarz HF receiver.
#[derive(Debug)]
pub struct RsHfTwin {
    style: Style,
    desc: RigDescription,
}

impl RsHfTwin {
    // [@ANCHOR: radioctl_rshf_twin_new]
    /// The twin of an EK 895, EK 896, XK 2100 or XK 852 description.
    pub fn new(desc: &RigDescription) -> Result<RsHfTwin, RigError> {
        let dialect = RsHfDialect::from_description(desc)?;
        Ok(RsHfTwin {
            style: dialect.style,
            desc: desc.clone(),
        })
    }

    // [@ANCHOR: radioctl_rshf_twin_reply]
    /// One answer line: LF, the text, CR.
    fn reply(text: &str) -> Vec<u8> {
        format!("\n{text}\r").into_bytes()
    }

    // [@ANCHOR: radioctl_rshf_twin_status_line]
    /// The XK 852's status line, with its fields at the columns Hamlib reads them from
    /// (`*F` at 1, `*I` at 12, `*N` at 22, `*S` at 30; column 0 is the LF).
    fn status_line(&self, state: &RigState) -> Vec<u8> {
        let mode = self.desc.mode_code(state.mode).unwrap_or(1);
        let mut line = format!("*F{:07}", state.freq_a.0 / 10);
        line.push_str("  ");
        line.push_str(&format!("*I{mode}"));
        while line.len() < 21 {
            line.push(' ');
        }
        line.push_str("*N0");
        while line.len() < 29 {
            line.push(' ');
        }
        line.push_str("*S1");
        Self::reply(&line)
    }
}

impl ReceiverTwin for RsHfTwin {
    // [@ANCHOR: radioctl_rshf_twin_line]
    fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8> {
        if self.style == Style::Xk852 {
            return self.xk852_line(state, line);
        }
        if line == "F?" {
            return Self::reply(&format!("F{}", state.freq_a.0));
        }
        if line == "I?" {
            let code = self.desc.mode_code(state.mode).unwrap_or(0);
            return Self::reply(&format!("I{code}"));
        }
        if line == "L?" && self.style == Style::Ek {
            return Self::reply(&format!("L{}", state.signal));
        }
        if let Some(rest) = line.strip_prefix('F') {
            let first = rest.split(',').next().unwrap_or("");
            if let Ok(hz) = first.parse::<u64>() {
                if hz <= EK_MAX_HZ {
                    state.freq_a = Hz(hz);
                }
            }
            return Vec::new();
        }
        if let Some(code) = line.strip_prefix('I') {
            if let Some(mode) = code
                .parse::<u8>()
                .ok()
                .and_then(|code| self.desc.code_mode(code))
            {
                state.mode = mode;
            }
        }
        Vec::new()
    }
}

impl RsHfTwin {
    // [@ANCHOR: radioctl_rshf_twin_xk852_line]
    /// One command line of the XK 852: a setting is answered with the status line.
    fn xk852_line(&mut self, state: &mut RigState, line: &str) -> Vec<u8> {
        if line == "*O1" {
            return self.status_line(state);
        }
        if let Some(digits) = line.strip_prefix("*F") {
            if let Ok(units) = digits.parse::<u64>() {
                state.freq_a = Hz(units * 10);
            }
            return self.status_line(state);
        }
        if let Some(code) = line.strip_prefix("*I") {
            if let Some(mode) = code
                .parse::<u8>()
                .ok()
                .and_then(|code| self.desc.code_mode(code))
            {
                state.mode = mode;
            }
            return self.status_line(state);
        }
        Vec::new()
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

    fn fresh_state(desc: &RigDescription) -> RigState {
        crate::virtual_rig::VirtualRig::from_description(desc)
            .expect("rig")
            .state()
            .clone()
    }

    // Tests [@ANCHOR: radioctl_rshf_engine]
    // Tests [@ANCHOR: radioctl_rshf_handles]
    // Tests [@ANCHOR: radioctl_rshf_style_of]
    // Tests [@ANCHOR: radioctl_rshf_dialect_from_description]
    // Tests [@ANCHOR: radioctl_rshf_dialect_encode]
    #[test]
    fn commands_follow_the_manual_and_hamlib() {
        let bytes = |e: &mut RsHfEngine, op: Op| e.encode(&op).expect("encode").bytes;
        // EK 895: manual chapter 2.3 examples (frequency in Hz, modulation 15 and 16).
        let mut ek = engine(shipped("rs-ek895")).expect("engine");
        assert_eq!(
            bytes(&mut ek, Op::SetFrequency(Vfo::A, Hz(21_500_000))),
            b"\nF21500000\r"
        );
        assert_eq!(bytes(&mut ek, Op::GetFrequency(Vfo::A)), b"\nF?\r");
        assert_eq!(bytes(&mut ek, Op::SetMode(Mode::Usb)), b"\nI15\r");
        assert_eq!(bytes(&mut ek, Op::SetMode(Mode::Lsb)), b"\nI16\r");
        assert_eq!(bytes(&mut ek, Op::GetMode), b"\nI?\r");
        assert_eq!(bytes(&mut ek, Op::GetSignal), b"\nL?\r");
        assert!(ek
            .encode(&Op::SetFrequency(Vfo::A, Hz(30_000_001)))
            .is_err());
        assert!(ek.encode(&Op::SetMode(Mode::Am)).is_err());
        assert!(ek.encode(&Op::SetPtt(true)).is_err());
        // XK 2100: Hamlib sends the frequency twice.
        let mut xk = engine(shipped("rs-xk2100")).expect("engine");
        assert_eq!(
            bytes(&mut xk, Op::SetFrequency(Vfo::A, Hz(14_100_000))),
            b"\nF14100000,14100000\r"
        );
        assert_eq!(bytes(&mut xk, Op::SetMode(Mode::Fm)), b"\nI9\r");
        assert!(xk.encode(&Op::GetSignal).is_err());
        // XK 852: ten-hertz units, seven digits, an answer to every setting.
        let mut x8 = engine(shipped("rs-xk852")).expect("engine");
        let set = x8
            .encode(&Op::SetFrequency(Vfo::A, Hz(7_074_004)))
            .expect("encode");
        assert_eq!(set.bytes, b"\n*F0707400\r");
        assert_eq!(set.reply, ReplyKind::Ack);
        assert_eq!(bytes(&mut x8, Op::SetMode(Mode::Cw)), b"\n*I5\r");
        assert_eq!(bytes(&mut x8, Op::GetMode), b"\n*O1\r");
        assert!(x8
            .encode(&Op::SetFrequency(Vfo::A, Hz(100_000_000)))
            .is_err());
        assert!(
            engine(shipped("rs-esmc")).is_err(),
            "the ESMC has its own dialect"
        );
    }

    // Tests [@ANCHOR: radioctl_rshf_dialect_status]
    // Tests [@ANCHOR: radioctl_rshf_dialect_decode]
    // Tests [@ANCHOR: radioctl_rshf_number_after]
    // Tests [@ANCHOR: radioctl_rshf_tagged_digits]
    #[test]
    fn answers_are_read_by_tag() {
        let mut ek = engine(shipped("rs-ek896")).expect("engine");
        ek.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(
            ek.feed(b"\nF21500000\r", T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(21_500_000)
            ))]
        );
        ek.encode(&Op::GetMode).expect("get");
        assert_eq!(
            ek.feed(b"\nI16\r", T0),
            vec![Inbound::Observation(Observation::Mode(Mode::Lsb))]
        );
        ek.encode(&Op::GetMode).expect("get");
        assert!(matches!(
            ek.feed(b"\nI18\r", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        ek.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            ek.feed(b"\nL57\r", T0),
            vec![Inbound::Observation(Observation::Signal(57))]
        );
        let mut x8 = engine(shipped("rs-xk852")).expect("engine");
        x8.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        let seen = x8.feed(b"\n*F0707400  *I2        *N0       *S1\r", T0);
        assert_eq!(
            seen,
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(7_074_000)
            ))]
        );
        x8.encode(&Op::SetMode(Mode::Usb)).expect("set");
        assert_eq!(x8.feed(b"\nanything\r", T0), vec![Inbound::Ack]);
    }

    // Tests [@ANCHOR: radioctl_rshf_twin_new]
    // Tests [@ANCHOR: radioctl_rshf_twin_reply]
    // Tests [@ANCHOR: radioctl_rshf_twin_status_line]
    // Tests [@ANCHOR: radioctl_rshf_twin_line]
    // Tests [@ANCHOR: radioctl_rshf_twin_xk852_line]
    #[test]
    fn the_twins_answer_as_the_radios_are_documented_to() {
        let desc = shipped("rs-ek895");
        let mut twin = RsHfTwin::new(&desc).expect("twin");
        let mut state = fresh_state(&desc);
        assert!(twin.line(&mut state, "F7074000").is_empty());
        assert_eq!(twin.line(&mut state, "F?"), b"\nF7074000\r");
        assert!(twin.line(&mut state, "F31000000").is_empty());
        assert_eq!(state.freq_a, Hz(7_074_000), "out of range is ignored");
        assert!(twin.line(&mut state, "I16").is_empty());
        assert_eq!(twin.line(&mut state, "I?"), b"\nI16\r");
        assert!(twin.line(&mut state, "I99").is_empty());
        assert_eq!(state.mode, Mode::Lsb);
        let desc = shipped("rs-xk852");
        let mut twin = RsHfTwin::new(&desc).expect("twin");
        let mut state = fresh_state(&desc);
        let answer = twin.line(&mut state, "*F0707400");
        assert_eq!(state.freq_a, Hz(7_074_000));
        let text = String::from_utf8(answer).expect("text");
        assert_eq!(text.find("*F"), Some(1), "Hamlib reads *F at column 1");
        assert_eq!(text.find("*I"), Some(12), "and *I at column 12");
        assert!(twin.line(&mut state, "nonsense").is_empty());
        assert!(RsHfTwin::new(&shipped("rs-esmc")).is_err());
    }
}

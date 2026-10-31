// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kenwood-style ASCII: command tokenizer, sans-IO client engine.
//!
//! Commands are two letters, optional digits, and `;` (`FA00014074000;`). Setting commands get
//! no reply. A refusal is the single token `?;`.
//!
//! **Sources.** The basic commands (FA, FB, MD, TX, RX, SM0, AI) come from our own
//! `tools/rig_simulator.py` and the relay's `ts2000_bridge.rs`. The rest is from Kenwood's
//! published PC command references, which carry no reuse restriction (only a "does not support
//! or warrant" disclaimer):
//!
//! * TS-590S/TS-590SG: "TS-590S TS-590SG PC CONTROL COMMAND Reference Guide", B5A-0316-00,
//!   October 23 2014 (file `ts_590_g_pc_command_e.pdf`, sha256
//!   b99787985aa4f0dd758a6301a829519a3e46f9f5354ca4199773b0557b4ad999), page 13 `ID`
//!   (021 TS-590S, 023 TS-590SG), page 14 `IF`, and the entries for `AG`, `KS`, `MG`, `ML`, `PC`,
//!   `RC`, `RD`/`RU`, `RG`, `RT`, `XT`, `SM` (alphabetical sections of the same guide).
//! * TS-2000: the TS-2000 Instruction Manual (B62-1221-10), appendix 21 "PC Control Command
//!   Tables", printed pages 115 to 140 (`IF` on page 125, `ID` 019 on page 125, `FR`/`FT` on
//!   page 124, `AG` on page 115, `KS` on page 125, `MG`/`ML` on page 127, `PC` on page 131,
//!   `RG` and `RT` on pages 134 and 135, `SM` on page 137). Kenwood's site does not host a
//!   TS-2000 command guide as a file; the text was read from the manual as transcribed (and
//!   mirrored in reverse character order) by manualsdir.com, pages 123 to 148, so the printed
//!   page numbers above come from that transcription and the transcription itself is unverified.
//!
//! The composite `IF;` answer is the same length (38 characters with `;`) on both models: P1 is
//! the 11-digit frequency of the receive VFO (the one `FR` selects), then 10 characters that are
//! P2 and P3 (TS-2000: 4-digit step and a signed 5-digit RIT/XIT offset; TS-590: 5 spaces and a
//! signed 4-digit offset), then P4 RIT, P5 XIT, P6 and P7 memory bank and channel, P8 TX (1) or RX
//! (0), P9 mode, P10 function (0 VFO A, 1 VFO B, 2 memory, 3 call), P11 scan, P12 split, P13
//! tone, P14 tone number, P15. The offset is found as the sign closest to the end of the 10
//! characters, which fits both layouts.
//!
//! Not implemented: the TS-890S dialect (its guide, Kenwood rev1 of 30 January 2019, has no `IF`
//! and sets the mode with `OM`), the 590's `ML` maximum of 20 on the SG (the engine takes 9), data
//! mode (the 590's `IF` cannot be read in data mode), the offset step for `RU`/`RD` (taken as
//! hertz from a cleared offset, which the guides do not state outright), and the bandscope.

use std::time::Duration;

use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request, PARTIAL_FRAME_TIMEOUT};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Level, Vfo};

/// Command terminator.
pub const TERMINATOR: u8 = b';';
/// Buffered bytes beyond this without a terminator are dropped.
const MAX_PENDING: usize = 256;
/// Largest RIT/XIT offset the engine sets, in hertz (the 590 shows 9990 at most).
const MAX_OFFSET_HZ: u16 = 9_999;
/// Largest frequency the 11-digit field holds.
const MAX_HZ: u64 = 99_999_999_999;

/// What the tokenizer found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A command or reply without its terminator (`FA00014074000`).
    Command(String),
    /// Bytes that were not a printable-ASCII command.
    Junk(Vec<u8>),
}

/// Splits a byte stream on `;`. Holds a partial command between calls.
#[derive(Debug, Default)]
pub struct Tokenizer {
    pending: Vec<u8>,
    last_byte_at: Duration,
}

impl Tokenizer {
    // [@ANCHOR: radioctl_kenwood_tokenizer_feed]
    /// Adds bytes received at `now` and returns every complete token.
    ///
    /// A partial command older than [`PARTIAL_FRAME_TIMEOUT`] is discarded as junk first.
    pub fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Token> {
        let mut out = Vec::new();
        let stale = now.saturating_sub(self.last_byte_at) > PARTIAL_FRAME_TIMEOUT;
        if stale && !self.pending.is_empty() {
            out.push(Token::Junk(std::mem::take(&mut self.pending)));
        }
        self.last_byte_at = now;
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|b| *b == TERMINATOR) {
            let raw: Vec<u8> = self.pending.drain(..=end).collect();
            let body = raw.get(..end).unwrap_or(&[]);
            let printable = body
                .iter()
                .all(|b| (0x20..0x7F).contains(b) && *b != TERMINATOR);
            let text = String::from_utf8_lossy(body).trim().to_string();
            if printable && !text.is_empty() {
                out.push(Token::Command(text));
            } else {
                out.push(Token::Junk(raw));
            }
        }
        if self.pending.len() > MAX_PENDING {
            out.push(Token::Junk(std::mem::take(&mut self.pending)));
        }
        out
    }
}

// [@ANCHOR: radioctl_kenwood_parse_digits]
/// Parses a string of ASCII digits only (no sign, no spaces).
pub fn parse_digits(text: &str) -> Option<u64> {
    if text.is_empty() || text.len() > 18 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The Kenwood-style client engine for one rig.
#[derive(Debug)]
pub struct KenwoodEngine {
    desc: RigDescription,
    tokenizer: Tokenizer,
}

impl KenwoodEngine {
    // [@ANCHOR: radioctl_kenwood_engine_new]
    /// Builds an engine from a Kenwood-style description.
    pub fn new(desc: RigDescription) -> Result<KenwoodEngine, RigError> {
        if desc.family != Family::KenwoodAscii || desc.kenwood.is_none() {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "not a Kenwood description",
            ));
        }
        Ok(KenwoodEngine {
            desc,
            tokenizer: Tokenizer::default(),
        })
    }

    fn unsupported(what: &str) -> RigError {
        RigError::new(ErrorKind::Unsupported, what.to_string())
    }

    fn command(text: String, reply: ReplyKind) -> Request {
        Request {
            bytes: text.into_bytes(),
            reply,
        }
    }

    // [@ANCHOR: radioctl_kenwood_engine_decode]
    fn decode(&self, text: &str) -> Vec<Inbound> {
        if text == "?" {
            return vec![Inbound::Nak];
        }
        if let Some(rest) = text.strip_prefix("IF") {
            return self
                .decode_if(rest)
                .unwrap_or_else(|| vec![Inbound::Unrecognised(format!("{text};").into_bytes())]);
        }
        let observation = match (text.get(..2), text.get(2..)) {
            (Some("FA"), Some(digits)) if digits.len() == 11 => {
                parse_digits(digits).map(|hz| Observation::Frequency(Vfo::A, Hz(hz)))
            }
            (Some("FB"), Some(digits)) if digits.len() == 11 => {
                parse_digits(digits).map(|hz| Observation::Frequency(Vfo::B, Hz(hz)))
            }
            (Some("MD"), Some(code)) if code.len() == 1 => code
                .bytes()
                .next()
                .and_then(|c| self.desc.code_mode(c))
                .map(Observation::Mode),
            (Some("SM"), Some(rest)) if rest.len() == 5 && rest.starts_with('0') => rest
                .get(1..)
                .and_then(parse_digits)
                .and_then(|v| u16::try_from(v).ok())
                .map(Observation::Signal),
            (Some("ID"), Some(digits)) if digits.len() == 3 => parse_digits(digits)
                .and_then(|v| u16::try_from(v).ok())
                .map(Observation::ModelId),
            (Some("FR"), Some(vfo @ ("0" | "1"))) => Some(Observation::RxVfo(vfo_of(vfo))),
            (Some("FT"), Some(vfo @ ("0" | "1"))) => Some(Observation::TxVfo(vfo_of(vfo))),
            (Some("RT"), Some(flag @ ("0" | "1"))) => Some(Observation::Rit(*flag == *"1")),
            (Some("XT"), Some(flag @ ("0" | "1"))) => Some(Observation::Xit(*flag == *"1")),
            (Some(name), Some(digits)) => decode_level(name, digits),
            _ => None,
        };
        vec![match observation {
            Some(value) => Inbound::Observation(value),
            None => Inbound::Unrecognised(format!("{text};").into_bytes()),
        }]
    }

    // [@ANCHOR: radioctl_kenwood_engine_decode_if]
    /// The observations in an `IF` answer (without `IF` and `;`): 35 characters, laid out as the
    /// module note says. `None` when the length or a field is wrong.
    fn decode_if(&self, body: &str) -> Option<Vec<Inbound>> {
        let field = |from: usize, to: usize| body.get(from..to);
        if body.len() != 35 || !body.is_ascii() {
            return None;
        }
        let frequency = parse_digits(field(0, 11)?)?;
        let offset = parse_if_offset(field(11, 21)?)?;
        let flag = |at: usize| match field(at, at + 1) {
            Some("0") => Some(false),
            Some("1") => Some(true),
            _ => None,
        };
        let (rit, xit, tx, split) = (flag(21)?, flag(22)?, flag(26)?, flag(30)?);
        let mode = field(27, 28)?
            .bytes()
            .next()
            .and_then(|c| self.desc.code_mode(c))?;
        let mut out = Vec::new();
        // P10 is the receive VFO (`FR`): the frequency is that VFO's. The transmit VFO is not in
        // `IF`; it is the other VFO when the split flag is set and the same one otherwise (`FT;`
        // reads it directly).
        let receive = match field(28, 29)? {
            "0" => Some(Vfo::A),
            "1" => Some(Vfo::B),
            _ => None,
        };
        if let Some(vfo) = receive {
            out.push(Observation::Frequency(vfo, Hz(frequency)));
            out.push(Observation::RxVfo(vfo));
            out.push(Observation::TxVfo(if split { vfo.other() } else { vfo }));
        }
        out.extend([
            Observation::Mode(mode),
            Observation::Ptt(tx),
            Observation::Rit(rit),
            Observation::Xit(xit),
            Observation::RitOffset(offset),
            Observation::Split(split),
        ]);
        Some(out.into_iter().map(Inbound::Observation).collect())
    }
}

/// The VFO named by an `FR`/`FT` digit (`0` is A).
fn vfo_of(digit: &str) -> Vfo {
    if digit == "1" {
        Vfo::B
    } else {
        Vfo::A
    }
}

/// How a level is written on the wire: the command, whether a leading `0` (main receiver)
/// follows it, and the largest value.
struct LevelWire {
    name: &'static str,
    main_digit: bool,
    min: u16,
    max: u16,
}

// [@ANCHOR: radioctl_kenwood_level_wire]
/// The wire form of a level. Ranges are the intersection of the TS-2000 and TS-590 guides.
fn level_wire(level: Level) -> LevelWire {
    let (name, main_digit, min, max) = match level {
        Level::AfGain => ("AG", true, 0, 255),
        Level::RfGain => ("RG", false, 0, 255),
        Level::MicGain => ("MG", false, 0, 100),
        Level::MonitorLevel => ("ML", false, 0, 9),
        Level::KeyerSpeed => ("KS", false, 10, 60),
        Level::RfPower => ("PC", false, 5, 100),
    };
    LevelWire {
        name,
        main_digit,
        min,
        max,
    }
}

// [@ANCHOR: radioctl_kenwood_decode_level]
/// A level answer: the command name and its digits (`AG0128` is name `AG`, digits `0128`).
fn decode_level(name: &str, digits: &str) -> Option<Observation> {
    let level = [
        Level::AfGain,
        Level::RfGain,
        Level::MicGain,
        Level::MonitorLevel,
        Level::KeyerSpeed,
        Level::RfPower,
    ]
    .into_iter()
    .find(|l| level_wire(*l).name == name)?;
    let wire = level_wire(level);
    let number = if wire.main_digit {
        digits.strip_prefix('0')?
    } else {
        digits
    };
    if number.len() != 3 {
        return None;
    }
    let value = u16::try_from(parse_digits(number)?).ok()?;
    Some(Observation::Level(level, value))
}

// [@ANCHOR: radioctl_kenwood_parse_if_offset]
/// The RIT/XIT offset in the 10 characters that hold P2 and P3 of an `IF` answer: the sign nearest
/// the end, then 4 or 5 digits to the end.
pub fn parse_if_offset(region: &str) -> Option<i16> {
    let at = region.rfind(['+', '-'])?;
    let digits = region.get(at + 1..)?;
    if !(4..=5).contains(&digits.len()) || at < 4 {
        return None;
    }
    let magnitude = i16::try_from(parse_digits(digits)?).ok()?;
    Some(if region.get(at..=at) == Some("-") {
        -magnitude
    } else {
        magnitude
    })
}

impl ProtocolEngine for KenwoodEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_kenwood_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(Self::unsupported("capability not in the rig description"));
        }
        let none = ReplyKind::None;
        let request = match op {
            Op::GetFrequency(vfo) => {
                let name = if *vfo == Vfo::A { "FA" } else { "FB" };
                Self::command(
                    format!("{name};"),
                    ReplyKind::Observation(Field::Frequency(*vfo)),
                )
            }
            Op::SetFrequency(vfo, hz) => {
                if hz.0 > MAX_HZ {
                    return Err(Self::unsupported("frequency above 11 digits"));
                }
                let name = if *vfo == Vfo::A { "FA" } else { "FB" };
                Self::command(format!("{name}{:011};", hz.0), none)
            }
            Op::GetMode => Self::command("MD;".into(), ReplyKind::Observation(Field::Mode)),
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| Self::unsupported("mode not in the description"))?;
                Self::command(format!("MD{};", char::from(code)), none)
            }
            Op::SetPtt(true) => Self::command("TX;".into(), none),
            Op::SetPtt(false) => Self::command("RX;".into(), none),
            Op::GetSignal => Self::command("SM0;".into(), ReplyKind::Observation(Field::Signal)),
            // AI2 (extended format) is the "on" of both guides; AI1 and AI3, the old format, are
            // refused by the TS-2000 and are not offered by the TS-590.
            Op::SetTransceive(on) => Self::command(format!("AI{};", if *on { 2 } else { 0 }), none),
            Op::GetPtt => Self::command("IF;".into(), ReplyKind::Observation(Field::Ptt)),
            Op::GetSplit => Self::command("IF;".into(), ReplyKind::Observation(Field::Split)),
            Op::GetTxVfo => Self::command("FT;".into(), ReplyKind::Observation(Field::TxVfo)),
            Op::GetRit => Self::command("IF;".into(), ReplyKind::Observation(Field::Rit)),
            Op::GetXit => Self::command("IF;".into(), ReplyKind::Observation(Field::Xit)),
            Op::GetRitOffset => {
                Self::command("IF;".into(), ReplyKind::Observation(Field::RitOffset))
            }
            Op::SetSplit(on) => Self::command(format!("FR0;FT{};", u8::from(*on)), none),
            Op::SetRit(on) => Self::command(format!("RT{};", u8::from(*on)), none),
            Op::SetXit(on) => Self::command(format!("XT{};", u8::from(*on)), none),
            Op::SetRitOffset(hz) => {
                if hz.unsigned_abs() > MAX_OFFSET_HZ {
                    return Err(Self::unsupported("RIT offset beyond 9999 Hz"));
                }
                // Clear, then move by the whole offset: the guides do not say whether RU/RD's
                // five digits are a delta or a target, and from zero the two are the same.
                let step = match hz {
                    0 => String::new(),
                    h if *h > 0 => format!("RU{:05};", h.unsigned_abs()),
                    h => format!("RD{:05};", h.unsigned_abs()),
                };
                Self::command(format!("RC;{step}"), none)
            }
            Op::GetLevel(level) => {
                let wire = level_wire(*level);
                let digit = if wire.main_digit { "0" } else { "" };
                Self::command(
                    format!("{}{digit};", wire.name),
                    ReplyKind::Observation(Field::Level(*level)),
                )
            }
            Op::SetLevel(level, value) => {
                let wire = level_wire(*level);
                if *value < wire.min || *value > wire.max {
                    return Err(Self::unsupported("level outside the range of the guides"));
                }
                let digit = if wire.main_digit { "0" } else { "" };
                Self::command(format!("{}{digit}{value:03};", wire.name), none)
            }
            Op::GetModelId => Self::command("ID;".into(), ReplyKind::Observation(Field::ModelId)),
            _ => {
                return Err(Self::unsupported("not implemented for the Kenwood engine"));
            }
        };
        Ok(request)
    }

    // [@ANCHOR: radioctl_kenwood_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        self.tokenizer
            .feed(bytes, now)
            .into_iter()
            .flat_map(|token| match token {
                Token::Command(text) => self.decode(&text),
                Token::Junk(junk) => vec![Inbound::Unrecognised(junk)],
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::units::{Level, Mode};

    const T0: Duration = Duration::from_secs(1);

    fn engine() -> KenwoodEngine {
        KenwoodEngine::new(RigDescription::ts2000()).expect("engine")
    }

    // Tests [@ANCHOR: radioctl_kenwood_parse_digits]
    #[test]
    fn digits_only() {
        assert_eq!(parse_digits("00014074000"), Some(14_074_000));
        assert_eq!(parse_digits("+1"), None);
        assert_eq!(parse_digits(""), None);
        assert_eq!(parse_digits("1 2"), None);
        assert_eq!(parse_digits("1234567890123456789"), None);
    }

    // Tests [@ANCHOR: radioctl_kenwood_tokenizer_feed]
    #[test]
    fn tokenizer_splits_joins_and_flags_junk() {
        let mut tok = Tokenizer::default();
        assert!(tok.feed(b"FA0001", T0).is_empty());
        assert_eq!(
            tok.feed(b"4074000;MD2;", T0),
            vec![
                Token::Command("FA00014074000".into()),
                Token::Command("MD2".into())
            ]
        );
        assert_eq!(
            tok.feed(&[0x01, b';'], T0),
            vec![Token::Junk(vec![0x01, b';'])]
        );
        assert_eq!(tok.feed(b";", T0), vec![Token::Junk(vec![b';'])]);
        assert!(tok.feed(b"FA", T0).is_empty());
        let later = T0 + PARTIAL_FRAME_TIMEOUT + Duration::from_millis(1);
        assert_eq!(
            tok.feed(b"MD2;", later),
            vec![Token::Junk(b"FA".to_vec()), Token::Command("MD2".into())]
        );
        let flood = vec![b'A'; MAX_PENDING + 1];
        assert!(matches!(
            tok.feed(&flood, later).as_slice(),
            [Token::Junk(_)]
        ));
    }

    // Tests [@ANCHOR: radioctl_kenwood_engine_new]
    #[test]
    fn engine_refuses_a_civ_description() {
        let err = KenwoodEngine::new(RigDescription::ic7300()).expect_err("must refuse");
        assert_eq!(err.kind, ErrorKind::Unsupported);
    }

    // Tests [@ANCHOR: radioctl_kenwood_engine_encode]
    #[test]
    fn encode_matches_the_ts2000_commands() {
        let mut engine = engine();
        let get = engine.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(get.bytes, b"FA;");
        let set = engine
            .encode(&Op::SetFrequency(Vfo::B, Hz(7_040_000)))
            .expect("set");
        assert_eq!(set.bytes, b"FB00007040000;");
        assert_eq!(set.reply, ReplyKind::None);
        assert_eq!(
            engine.encode(&Op::SetMode(Mode::Usb)).expect("mode").bytes,
            b"MD2;"
        );
        assert_eq!(engine.encode(&Op::SetPtt(true)).expect("tx").bytes, b"TX;");
        assert_eq!(engine.encode(&Op::SetPtt(false)).expect("rx").bytes, b"RX;");
        assert_eq!(engine.encode(&Op::GetSignal).expect("sm").bytes, b"SM0;");
        assert_eq!(
            engine.encode(&Op::SetTransceive(true)).expect("ai").bytes,
            b"AI2;"
        );
        assert!(
            engine.encode(&Op::GetPtt).is_err(),
            "the built-in lacks ReadPtt"
        );
        assert!(engine.encode(&Op::SendCw("cq".into())).is_err());
        assert!(engine
            .encode(&Op::SetFrequency(Vfo::A, Hz(100_000_000_000)))
            .is_err());
    }

    // Tests [@ANCHOR: radioctl_kenwood_engine_feed]
    // Tests [@ANCHOR: radioctl_kenwood_engine_decode]
    #[test]
    fn feed_decodes_replies_and_refusals() {
        let mut engine = engine();
        assert_eq!(
            engine.feed(b"FA00014074000;MD2;SM00012;?;", T0),
            vec![
                Inbound::Observation(Observation::Frequency(Vfo::A, Hz(14_074_000))),
                Inbound::Observation(Observation::Mode(Mode::Usb)),
                Inbound::Observation(Observation::Signal(12)),
                Inbound::Nak,
            ]
        );
        assert!(matches!(
            engine.feed(b"FA123;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(matches!(
            engine.feed(b"MD8;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert_eq!(
            engine.feed(b"ID019;", T0),
            vec![Inbound::Observation(Observation::ModelId(19))]
        );
    }

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_kenwood_parse_if_offset]
    #[test]
    fn the_if_offset_is_found_in_both_layouts() {
        assert_eq!(parse_if_offset("0000+00150"), Some(150));
        assert_eq!(parse_if_offset("0000-00150"), Some(-150));
        assert_eq!(parse_if_offset("     -0120"), Some(-120));
        assert_eq!(parse_if_offset("     +0000"), Some(0));
        assert_eq!(parse_if_offset("0000 00150"), None);
        assert_eq!(parse_if_offset("+000000000"), None);
        assert_eq!(parse_if_offset("0000+001"), None);
    }

    // Tests [@ANCHOR: radioctl_kenwood_engine_decode_if]
    // Tests [@ANCHOR: radioctl_kenwood_engine_decode]
    #[test]
    fn an_if_answer_gives_frequency_mode_ptt_rit_xit_offset_and_split() {
        let mut e = KenwoodEngine::new(shipped("kenwood-ts-2000")).expect("engine");
        // P1 freq, P2 step, P3 offset, RIT, XIT, bank, channel, TX, mode, function, scan, split,
        // tone, tone number, P15.
        let answer = [
            "IF",
            "00014074000",
            "0000",
            "+00150",
            "1",
            "0",
            "0",
            "00",
            "1",
            "2",
            "0",
            "0",
            "1",
            "0",
            "00",
            "0",
            ";",
        ]
        .concat();
        assert_eq!(answer.len(), 38);
        assert_eq!(
            e.feed(answer.as_bytes(), T0),
            vec![
                Inbound::Observation(Observation::Frequency(Vfo::A, Hz(14_074_000))),
                Inbound::Observation(Observation::RxVfo(Vfo::A)),
                Inbound::Observation(Observation::TxVfo(Vfo::B)),
                Inbound::Observation(Observation::Mode(Mode::Usb)),
                Inbound::Observation(Observation::Ptt(true)),
                Inbound::Observation(Observation::Rit(true)),
                Inbound::Observation(Observation::Xit(false)),
                Inbound::Observation(Observation::RitOffset(150)),
                Inbound::Observation(Observation::Split(true)),
            ]
        );
        let mut e590 = KenwoodEngine::new(shipped("kenwood-ts-590sg")).expect("engine");
        let answer = [
            "IF",
            "00007074000",
            "     ",
            "-0120",
            "0",
            "1",
            "0",
            "00",
            "0",
            "2",
            "1",
            "0",
            "0",
            "0",
            "00",
            "0",
            ";",
        ]
        .concat();
        assert_eq!(answer.len(), 38);
        let seen = e590.feed(answer.as_bytes(), T0);
        assert!(seen.contains(&Inbound::Observation(Observation::RitOffset(-120))));
        assert!(seen.contains(&Inbound::Observation(Observation::Xit(true))));
        assert!(seen.contains(&Inbound::Observation(Observation::Frequency(
            Vfo::B,
            Hz(7_074_000)
        ))));
        assert!(matches!(
            e.feed(b"IF1234;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        let long = answer.replace(';', "0;");
        assert!(matches!(
            e590.feed(long.as_bytes(), T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }

    // Tests [@ANCHOR: radioctl_kenwood_engine_encode]
    // Tests [@ANCHOR: radioctl_kenwood_level_wire]
    // Tests [@ANCHOR: radioctl_kenwood_decode_level]
    #[test]
    fn the_extended_commands_follow_the_command_guides() {
        let mut e = KenwoodEngine::new(shipped("kenwood-ts-590s")).expect("engine");
        let b = |e: &mut KenwoodEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        assert_eq!(b(&mut e, Op::GetPtt), b"IF;");
        assert_eq!(b(&mut e, Op::GetModelId), b"ID;");
        assert_eq!(b(&mut e, Op::SetSplit(true)), b"FR0;FT1;");
        assert_eq!(b(&mut e, Op::SetSplit(false)), b"FR0;FT0;");
        assert_eq!(b(&mut e, Op::SetRit(true)), b"RT1;");
        assert_eq!(b(&mut e, Op::SetXit(false)), b"XT0;");
        assert_eq!(b(&mut e, Op::SetRitOffset(150)), b"RC;RU00150;");
        assert_eq!(b(&mut e, Op::SetRitOffset(-1200)), b"RC;RD01200;");
        assert_eq!(b(&mut e, Op::SetRitOffset(0)), b"RC;");
        assert_eq!(b(&mut e, Op::GetLevel(Level::AfGain)), b"AG0;");
        assert_eq!(b(&mut e, Op::SetLevel(Level::AfGain, 128)), b"AG0128;");
        assert_eq!(b(&mut e, Op::SetLevel(Level::MicGain, 100)), b"MG100;");
        assert_eq!(b(&mut e, Op::SetLevel(Level::KeyerSpeed, 18)), b"KS018;");
        assert_eq!(b(&mut e, Op::SetLevel(Level::RfPower, 5)), b"PC005;");
        assert!(e.encode(&Op::SetLevel(Level::MicGain, 101)).is_err());
        assert!(e.encode(&Op::SetLevel(Level::KeyerSpeed, 9)).is_err());
        assert!(e.encode(&Op::SetRitOffset(10_000)).is_err());
        for (answer, level, value) in [
            (&b"AG0128;"[..], Level::AfGain, 128),
            (b"RG250;", Level::RfGain, 250),
            (b"MG025;", Level::MicGain, 25),
            (b"ML005;", Level::MonitorLevel, 5),
            (b"KS018;", Level::KeyerSpeed, 18),
            (b"PC050;", Level::RfPower, 50),
        ] {
            assert_eq!(
                e.feed(answer, T0),
                vec![Inbound::Observation(Observation::Level(level, value))]
            );
        }
        for bad in [&b"AG128;"[..], b"RG0250;", b"MG;", b"ZZ012;"] {
            assert!(
                matches!(e.feed(bad, T0).as_slice(), [Inbound::Unrecognised(_)]),
                "{bad:?}"
            );
        }
        assert_eq!(
            e.feed(b"RT1;XT0;ID021;", T0),
            vec![
                Inbound::Observation(Observation::Rit(true)),
                Inbound::Observation(Observation::Xit(false)),
                Inbound::Observation(Observation::ModelId(21)),
            ]
        );
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! Yaesu "newcat" ASCII (FT-991A, FTDX10, FTDX101): sans-IO client engine.
//!
//! Commands are two letters, digits and `;` like Kenwood's, but the formats differ: the frequency
//! is 9 digits (`FA014074000;`), the mode is `MD0<code>;`, the transmit state is read with `TX;`,
//! the levels carry a leading `0` (`AG0128;`, `RG0250;`) except `MG`, `PC` and `KS`.
//!
//! **Sources.** Yaesu's published "CAT Operation Reference Manual" for each model, read from the
//! PDFs (no reuse restriction beyond the usual copyright notice):
//!
//! * FT-991A: file `ft991a_cat_manual.pdf` (the pages are headed "FT-991 CAT Operation Reference
//!   Book"), sha256 52164f737e37a3ffc8276eccedf8ecb524607198c38d386ae4ff7b2fc1d60c95: `ID`
//!   (0670 FT-991A), `IF`, `FA`, `FB`, `FT`, `MD`, `TX`, `SM`, `RM`, `AG`, `RG`, `MG`, `PC`,
//!   `KS`, `RC`/`RD`/`RU`, `RT`, `XT`, `AI`.
//! * FTDX10: `ftdx10_cat_manual.pdf` (copyright 2022), sha256
//!   c63d7899e4308ca621271087f20da425fc9d68e2bb1d89d38c545adeb72a6db3: `ID` 0761, `IF`, `MD`
//!   (adds E PSK and F DATA-FM-N), `RM` (answer carries a second field), `FT` (MAIN/SUB band
//!   transmitter).
//! * FTDX101D: `FTDX101D_CAT_OM_ENG_1904-A.pdf`, sha256
//!   c1f65e35fc406fedea6cd75aaa710831f1be2b5058b6e23befa8d7e22039d88c: `ID` 0681,
//!   `FT`, `AG`/`RG`/`MD` with a MAIN/SUB first parameter.
//!
//! The page numbers are in each description fact. The composite `IF;` answer is 28 characters:
//! P1 3 characters (memory channel, or letters on the FTDX10), P2 the 9-digit VFO-A frequency,
//! P3 the clarifier direction and 4-digit offset, P4 RX clarifier, P5 TX clarifier, P6 mode,
//! P7 VFO or memory, P8 tone, P9 two fixed characters, P10 shift.
//!
//! Not implemented: SWR and calibrated meters (`RM` scales are model tables), VFO-B reads on the
//! MAIN/SUB models beyond `FB` (the FTDX10 and FTDX101 split is by band, not VFO, so their
//! descriptions have no `split`), monitor level, the data and narrow modes (A, B, D, E, F),
//! memory channels, and the FTDX101MP (ID 0682 is not in the guide read).

use std::time::Duration;

use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Level, Vfo};

use crate::attenuation;
use crate::kenwood::{parse_digits, Token, Tokenizer};

/// Largest frequency the 9-digit field holds.
const MAX_HZ: u64 = 999_999_999;
/// Largest clarifier offset, in hertz.
const MAX_OFFSET_HZ: u16 = 9_999;

/// How a level is written on the wire.
struct LevelWire {
    name: &'static str,
    /// True when the command carries a leading `0` (main receiver / fixed) before the digits.
    leading_zero: bool,
    min: u16,
    max: u16,
}

// [@ANCHOR: radioctl_yaesu_level_wire]
/// The wire form of a level, or `None` when the family has no such level command here.
fn level_wire(level: Level) -> Option<LevelWire> {
    let (name, leading_zero, min, max) = match level {
        Level::AfGain => ("AG", true, 0, 255),
        Level::RfGain => ("RG", true, 0, 255),
        Level::MicGain => ("MG", false, 0, 100),
        Level::KeyerSpeed => ("KS", false, 4, 60),
        Level::RfPower => ("PC", false, 5, 100),
        Level::MonitorLevel | Level::Attenuator | Level::Preamp => return None,
    };
    Some(LevelWire {
        name,
        leading_zero,
        min,
        max,
    })
}

// [@ANCHOR: radioctl_yaesu_decode_level]
/// A level answer: command name and digits (`AG0128` is name `AG`, digits `0128`).
fn decode_level(name: &str, digits: &str) -> Option<Observation> {
    let level = [
        Level::AfGain,
        Level::RfGain,
        Level::MicGain,
        Level::KeyerSpeed,
        Level::RfPower,
    ]
    .into_iter()
    .find(|l| level_wire(*l).is_some_and(|w| w.name == name))?;
    let wire = level_wire(level)?;
    let number = if wire.leading_zero {
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

// [@ANCHOR: radioctl_yaesu_parse_clarifier]
/// The clarifier offset in the 5 characters of `IF` P3: a sign and four digits.
pub fn parse_clarifier(text: &str) -> Option<i16> {
    let digits = text.get(1..)?;
    if digits.len() != 4 {
        return None;
    }
    let magnitude = i16::try_from(parse_digits(digits)?).ok()?;
    match text.get(..1)? {
        "+" => Some(magnitude),
        "-" => Some(-magnitude),
        _ => None,
    }
}

/// The Yaesu newcat client engine for one rig.
#[derive(Debug)]
pub struct YaesuEngine {
    desc: RigDescription,
    tokenizer: Tokenizer,
}

impl YaesuEngine {
    // [@ANCHOR: radioctl_yaesu_engine_new]
    /// Builds an engine from a newcat description.
    pub fn new(desc: RigDescription) -> Result<YaesuEngine, RigError> {
        if desc.family != Family::YaesuNewcat || desc.yaesu.is_none() {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "not a Yaesu newcat description",
            ));
        }
        Ok(YaesuEngine {
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

    // [@ANCHOR: radioctl_yaesu_engine_decode_front_end]
    /// An `RA` or `PA` answer: `0` (main receiver; the FT-991A has this digit as a fixed zero)
    /// and the step, mapped by the description.
    fn decode_front_end(&self, name: &str, digits: &str) -> Option<Observation> {
        let level = if name == "RA" {
            Level::Attenuator
        } else {
            Level::Preamp
        };
        let step = u8::try_from(parse_digits(
            digits.strip_prefix('0').filter(|r| r.len() == 1)?,
        )?)
        .ok()?;
        attenuation::value_of(&self.desc, level, step).map(|v| Observation::Level(level, v))
    }

    // [@ANCHOR: radioctl_yaesu_engine_decode]
    fn decode(&self, text: &str) -> Vec<Inbound> {
        if text == "?" {
            return vec![Inbound::Nak];
        }
        if let Some(body) = text.strip_prefix("IF") {
            return self
                .decode_if(body)
                .unwrap_or_else(|| vec![Inbound::Unrecognised(format!("{text};").into_bytes())]);
        }
        let observation = match (text.get(..2), text.get(2..)) {
            (Some("FA"), Some(d)) if d.len() == 9 => {
                parse_digits(d).map(|hz| Observation::Frequency(Vfo::A, Hz(hz)))
            }
            (Some("FB"), Some(d)) if d.len() == 9 => {
                parse_digits(d).map(|hz| Observation::Frequency(Vfo::B, Hz(hz)))
            }
            (Some("MD"), Some(rest)) if rest.len() == 2 && rest.starts_with('0') => rest
                .as_bytes()
                .get(1)
                .and_then(|c| self.desc.code_mode(*c))
                .map(Observation::Mode),
            (Some("TX"), Some(flag @ ("0" | "1" | "2"))) => Some(Observation::Ptt(*flag != *"0")),
            (Some("FT"), Some(flag @ ("0" | "1"))) => Some(Observation::Split(*flag == *"1")),
            (Some("RT"), Some(flag @ ("0" | "1"))) => Some(Observation::Rit(*flag == *"1")),
            (Some("XT"), Some(flag @ ("0" | "1"))) => Some(Observation::Xit(*flag == *"1")),
            (Some("SM"), Some(rest)) if rest.len() == 4 && rest.starts_with('0') => rest
                .get(1..)
                .and_then(parse_digits)
                .and_then(|v| u16::try_from(v).ok())
                .map(Observation::Signal),
            (Some("RM"), Some(rest)) if rest.len() >= 4 && rest.starts_with('5') => rest
                .get(1..4)
                .and_then(parse_digits)
                .and_then(|v| u16::try_from(v).ok())
                .map(Observation::Power),
            (Some("ID"), Some(d)) if d.len() == 4 => parse_digits(d)
                .and_then(|v| u16::try_from(v).ok())
                .map(Observation::ModelId),
            (Some(name @ ("RA" | "PA")), Some(digits)) => self.decode_front_end(name, digits),
            (Some(name), Some(digits)) => decode_level(name, digits),
            _ => None,
        };
        match observation {
            // `FT` here is the split flag; the transmitter is on B in split and on A otherwise.
            Some(value @ Observation::Split(on)) => vec![
                Inbound::Observation(value),
                Inbound::Observation(Observation::TxVfo(if on { Vfo::B } else { Vfo::A })),
            ],
            Some(value) => vec![Inbound::Observation(value)],
            None => vec![Inbound::Unrecognised(format!("{text};").into_bytes())],
        }
    }

    // [@ANCHOR: radioctl_yaesu_engine_decode_if]
    /// The observations in an `IF` answer without `IF` and `;`: 25 characters.
    fn decode_if(&self, body: &str) -> Option<Vec<Inbound>> {
        if body.len() != 25 || !body.is_ascii() {
            return None;
        }
        let frequency = parse_digits(body.get(3..12)?)?;
        let offset = parse_clarifier(body.get(12..17)?)?;
        let flag = |at: usize| match body.get(at..at + 1) {
            Some("0") => Some(false),
            Some("1") => Some(true),
            _ => None,
        };
        let (rit, xit) = (flag(17)?, flag(18)?);
        let mode = body
            .get(19..20)?
            .bytes()
            .next()
            .and_then(|c| self.desc.code_mode(c))?;
        Some(
            [
                Observation::Frequency(Vfo::A, Hz(frequency)),
                Observation::Mode(mode),
                Observation::Rit(rit),
                Observation::Xit(xit),
                Observation::RitOffset(offset),
            ]
            .into_iter()
            .map(Inbound::Observation)
            .collect(),
        )
    }
}

impl ProtocolEngine for YaesuEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_yaesu_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(Self::unsupported("capability not in the rig description"));
        }
        let none = ReplyKind::None;
        let observe = |field: Field| ReplyKind::Observation(field);
        let request = match op {
            Op::GetFrequency(vfo) => {
                let name = if *vfo == Vfo::A { "FA" } else { "FB" };
                Self::command(format!("{name};"), observe(Field::Frequency(*vfo)))
            }
            Op::SetFrequency(vfo, hz) => {
                if hz.0 > MAX_HZ {
                    return Err(Self::unsupported("frequency above 9 digits"));
                }
                let name = if *vfo == Vfo::A { "FA" } else { "FB" };
                Self::command(format!("{name}{:09};", hz.0), none)
            }
            Op::GetMode => Self::command("MD0;".into(), observe(Field::Mode)),
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| Self::unsupported("mode not in the description"))?;
                Self::command(format!("MD0{};", char::from(code)), none)
            }
            Op::GetPtt => Self::command("TX;".into(), observe(Field::Ptt)),
            Op::SetPtt(on) => Self::command(format!("TX{};", u8::from(*on)), none),
            Op::GetSignal => Self::command("SM0;".into(), observe(Field::Signal)),
            Op::GetPower => Self::command("RM5;".into(), observe(Field::Power)),
            Op::SetTransceive(on) => Self::command(format!("AI{};", u8::from(*on)), none),
            Op::GetSplit => Self::command("FT;".into(), observe(Field::Split)),
            Op::GetTxVfo => Self::command("FT;".into(), observe(Field::TxVfo)),
            Op::SetSplit(on) => Self::command(format!("FT{};", if *on { 3 } else { 2 }), none),
            Op::GetRit => Self::command("RT;".into(), observe(Field::Rit)),
            Op::GetXit => Self::command("XT;".into(), observe(Field::Xit)),
            Op::GetRitOffset => Self::command("IF;".into(), observe(Field::RitOffset)),
            Op::SetRit(on) => Self::command(format!("RT{};", u8::from(*on)), none),
            Op::SetXit(on) => Self::command(format!("XT{};", u8::from(*on)), none),
            Op::SetRitOffset(hz) => {
                if hz.unsigned_abs() > MAX_OFFSET_HZ {
                    return Err(Self::unsupported("clarifier offset beyond 9999 Hz"));
                }
                let step = match hz {
                    0 => String::new(),
                    h if *h > 0 => format!("RU{:04};", h.unsigned_abs()),
                    h => format!("RD{:04};", h.unsigned_abs()),
                };
                Self::command(format!("RC;{step}"), none)
            }
            Op::GetLevel(level @ (Level::Attenuator | Level::Preamp)) => {
                attenuation::step_of(&self.desc, *level, 0)?;
                let name = if *level == Level::Attenuator {
                    "RA"
                } else {
                    "PA"
                };
                Self::command(format!("{name}0;"), observe(Field::Level(*level)))
            }
            Op::SetLevel(level @ (Level::Attenuator | Level::Preamp), value) => {
                let step = attenuation::step_of(&self.desc, *level, *value)?;
                let name = if *level == Level::Attenuator {
                    "RA"
                } else {
                    "PA"
                };
                Self::command(format!("{name}0{step};"), none)
            }
            Op::GetLevel(level) => {
                let wire = level_wire(*level)
                    .ok_or_else(|| Self::unsupported("level not implemented for newcat"))?;
                let digit = if wire.leading_zero { "0" } else { "" };
                Self::command(
                    format!("{}{digit};", wire.name),
                    observe(Field::Level(*level)),
                )
            }
            Op::SetLevel(level, value) => {
                let wire = level_wire(*level)
                    .ok_or_else(|| Self::unsupported("level not implemented for newcat"))?;
                if *value < wire.min || *value > wire.max {
                    return Err(Self::unsupported("level outside the range of the guides"));
                }
                let digit = if wire.leading_zero { "0" } else { "" };
                Self::command(format!("{}{digit}{value:03};", wire.name), none)
            }
            Op::GetModelId => Self::command("ID;".into(), observe(Field::ModelId)),
            // No Morse over CAT: the three guides' `KY` plays back keyer memories 1 to 5 and
            // message keyer 1 to 5 only, and none documents a stop, so `StopCw` is refused with
            // the rest. Unkeying (`TX0;`) remains the stop path.
            Op::SendCw(_) | Op::StopCw => {
                return Err(Self::unsupported(
                    "the CAT guides have no Morse text command and no stop for one",
                ))
            }
            _ => return Err(Self::unsupported("not implemented for the Yaesu engine")),
        };
        Ok(request)
    }

    // [@ANCHOR: radioctl_yaesu_engine_feed]
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
    use radioctl_core::units::Mode;

    const T0: Duration = Duration::from_secs(1);

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_yaesu_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        let err = YaesuEngine::new(RigDescription::ts2000()).expect_err("must refuse");
        assert_eq!(err.kind, ErrorKind::Unsupported);
    }

    // Tests [@ANCHOR: radioctl_yaesu_parse_clarifier]
    #[test]
    fn clarifier_is_a_sign_and_four_digits() {
        assert_eq!(parse_clarifier("+0150"), Some(150));
        assert_eq!(parse_clarifier("-1200"), Some(-1200));
        assert_eq!(parse_clarifier("00150"), None);
        assert_eq!(parse_clarifier("+015"), None);
        assert_eq!(parse_clarifier("+01a0"), None);
    }

    // Tests [@ANCHOR: radioctl_yaesu_engine_encode]
    // Tests [@ANCHOR: radioctl_yaesu_level_wire]
    #[test]
    fn commands_follow_the_cat_reference_manual() {
        let mut e = YaesuEngine::new(shipped("yaesu-ft-991a")).expect("engine");
        let b = |e: &mut YaesuEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        assert_eq!(b(&mut e, Op::GetFrequency(Vfo::A)), b"FA;");
        assert_eq!(
            b(&mut e, Op::SetFrequency(Vfo::B, Hz(7_074_000))),
            b"FB007074000;"
        );
        assert_eq!(b(&mut e, Op::GetMode), b"MD0;");
        assert_eq!(b(&mut e, Op::SetMode(Mode::Cw)), b"MD03;");
        assert_eq!(b(&mut e, Op::SetMode(Mode::Data)), b"MD0C;");
        assert_eq!(b(&mut e, Op::GetPtt), b"TX;");
        assert_eq!(b(&mut e, Op::SetPtt(true)), b"TX1;");
        assert_eq!(b(&mut e, Op::SetPtt(false)), b"TX0;");
        assert_eq!(b(&mut e, Op::GetSignal), b"SM0;");
        assert_eq!(b(&mut e, Op::GetPower), b"RM5;");
        assert_eq!(b(&mut e, Op::SetSplit(true)), b"FT3;");
        assert_eq!(b(&mut e, Op::SetSplit(false)), b"FT2;");
        assert_eq!(b(&mut e, Op::SetRitOffset(150)), b"RC;RU0150;");
        assert_eq!(b(&mut e, Op::SetRitOffset(-1200)), b"RC;RD1200;");
        assert_eq!(b(&mut e, Op::SetRitOffset(0)), b"RC;");
        assert_eq!(b(&mut e, Op::GetLevel(Level::AfGain)), b"AG0;");
        assert_eq!(b(&mut e, Op::SetLevel(Level::AfGain, 128)), b"AG0128;");
        assert_eq!(b(&mut e, Op::SetLevel(Level::MicGain, 100)), b"MG100;");
        assert_eq!(b(&mut e, Op::SetLevel(Level::KeyerSpeed, 4)), b"KS004;");
        assert_eq!(b(&mut e, Op::GetModelId), b"ID;");
        assert!(e.encode(&Op::SetLevel(Level::MicGain, 101)).is_err());
        assert!(e.encode(&Op::SetLevel(Level::KeyerSpeed, 3)).is_err());
        assert!(e.encode(&Op::GetLevel(Level::MonitorLevel)).is_err());
        assert!(e.encode(&Op::SetRitOffset(10_000)).is_err());
        assert!(e
            .encode(&Op::SetFrequency(Vfo::A, Hz(1_000_000_000)))
            .is_err());
        assert!(e.encode(&Op::SendCw("cq".into())).is_err());
        assert_eq!(
            e.encode(&Op::StopCw).expect_err("no stop").kind,
            ErrorKind::Unsupported
        );
        let mut x = YaesuEngine::new(shipped("yaesu-ftdx10")).expect("engine");
        assert!(
            x.encode(&Op::SetSplit(true)).is_err(),
            "FTDX10 splits by band"
        );
    }

    // Tests [@ANCHOR: radioctl_yaesu_engine_feed]
    // Tests [@ANCHOR: radioctl_yaesu_engine_decode]
    // Tests [@ANCHOR: radioctl_yaesu_decode_level]
    #[test]
    fn replies_decode_and_garbage_is_unrecognised() {
        let mut e = YaesuEngine::new(shipped("yaesu-ft-991a")).expect("engine");
        let seen = e.feed(
            b"FA014074000;FB007074000;MD02;TX2;FT1;RT1;XT0;SM0123;RM5100000;ID0670;?;",
            T0,
        );
        assert_eq!(
            seen,
            vec![
                Inbound::Observation(Observation::Frequency(Vfo::A, Hz(14_074_000))),
                Inbound::Observation(Observation::Frequency(Vfo::B, Hz(7_074_000))),
                Inbound::Observation(Observation::Mode(Mode::Usb)),
                Inbound::Observation(Observation::Ptt(true)),
                Inbound::Observation(Observation::Split(true)),
                Inbound::Observation(Observation::TxVfo(Vfo::B)),
                Inbound::Observation(Observation::Rit(true)),
                Inbound::Observation(Observation::Xit(false)),
                Inbound::Observation(Observation::Signal(123)),
                Inbound::Observation(Observation::Power(100)),
                Inbound::Observation(Observation::ModelId(670)),
                Inbound::Nak,
            ]
        );
        for (answer, level, value) in [
            (&b"AG0128;"[..], Level::AfGain, 128),
            (b"RG0250;", Level::RfGain, 250),
            (b"MG025;", Level::MicGain, 25),
            (b"KS018;", Level::KeyerSpeed, 18),
            (b"PC050;", Level::RfPower, 50),
        ] {
            assert_eq!(
                e.feed(answer, T0),
                vec![Inbound::Observation(Observation::Level(level, value))]
            );
        }
        for bad in [
            &b"FA14074000;"[..],
            b"MD0Z;",
            b"MD2;",
            b"TX3;",
            b"AG128;",
            b"ZZ1;",
            b"SM123;",
        ] {
            assert!(
                matches!(e.feed(bad, T0).as_slice(), [Inbound::Unrecognised(_)]),
                "{bad:?}"
            );
        }
    }

    // Tests [@ANCHOR: radioctl_yaesu_engine_decode_if]
    #[test]
    fn an_if_answer_gives_frequency_mode_clarifier_and_offset() {
        let mut e = YaesuEngine::new(shipped("yaesu-ft-991a")).expect("engine");
        // P1 channel, P2 frequency, P3 offset, RX CLAR, TX CLAR, mode, VFO, tone, P9, P10.
        let answer = [
            "IF",
            "001",
            "014074000",
            "-0120",
            "1",
            "0",
            "2",
            "0",
            "0",
            "00",
            "0",
            ";",
        ]
        .concat();
        assert_eq!(answer.len(), 28);
        assert_eq!(
            e.feed(answer.as_bytes(), T0),
            vec![
                Inbound::Observation(Observation::Frequency(Vfo::A, Hz(14_074_000))),
                Inbound::Observation(Observation::Mode(Mode::Usb)),
                Inbound::Observation(Observation::Rit(true)),
                Inbound::Observation(Observation::Xit(false)),
                Inbound::Observation(Observation::RitOffset(-120)),
            ]
        );
        for bad in ["IF1234;", "IF001014074000-0120102001000X;"] {
            assert!(matches!(
                e.feed(bad.as_bytes(), T0).as_slice(),
                [Inbound::Unrecognised(_)]
            ));
        }
        let long = answer.replace(';', "0;");
        assert!(matches!(
            e.feed(long.as_bytes(), T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }
}

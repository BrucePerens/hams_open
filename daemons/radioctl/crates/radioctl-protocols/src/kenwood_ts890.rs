// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Kenwood TS-890S dialect: ASCII commands ended by `;`, like the TS-2000 and TS-590 family, but
//! a different command set. Sans-IO engine.
//!
//! **Source.** Kenwood "TS-890S PC CONTROL COMMAND Reference Guide", January/30/2019 (file
//! `ts890_pc_command_en_rev1.pdf`, sha256 e106050c990747234bc9bf6d5b5ca7619ebebdb0ce6f36fe3009a1304e37ea77),
//! whose only terms are a disclaimer that Kenwood "does not support nor warrant" it. Nothing here comes
//! from Hamlib source.
//!
//! Differences from the classic family (`kenwood.rs`) that this engine implements:
//!
//! * There is **no `IF`**. Split is `TB` (guide p. 64), the receive and transmit VFOs `FR` and `FT`
//!   (p. 34 and 35), RIT and XIT `RT` and `XT` (p. 59 and 70), and the RIT/XIT offset is read with
//!   `RF` (p. 58: a direction digit and four digits of hertz) and set the classic way with `RC`,
//!   `RU` and `RD` (p. 57). The transmit state cannot be read at all (`TX` has no read form, p. 67),
//!   so a TS-890S description has no `read-ptt`.
//! * The mode is `OM` (p. 52): `OM0;` reads the mode of the left frequency display and `OM0<m>;`
//!   sets it, `<m>` being 1 LSB, 2 USB, 3 CW, 4 FM, 5 AM, 6 FSK, 7 CW-R, 9 FSK-R, A PSK, B PSK-R, C LSB-D,
//!   D USB-D, E FM-D, F AM-D. There is no `MD`.
//! * Levels have no receiver digit: `AGnnn;` (p. 4), `RGnnn;`, `MGnnn;`, `KSnnn;` (4 to 60), `PCnnn;`;
//!   the meter is `SM;` answering `SMnnnn;` (p. 62). `RA` and `PA` take one digit (p. 57, 52).
//! * `ID;` answers `ID024;` (p. 36).
//!
//! Not implemented: the bandscope (`BS` commands and the LAN-only `##DD2`/`##DD3` waveform data), CW
//! text, memories, the LAN session beyond the two login frames in [`lan_connect_request`] and
//! [`lan_login`] (the session that sends them, with the owner's account, and keeps the line alive is
//! [`crate::kenwood_ts890_lan::Ts890LanFraming`]), and any of the many menu commands. Never run against a radio.

use std::time::Duration;

use radioctl_core::description::{Family, KenwoodDialect, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Level, Vfo};

use crate::attenuation;
use crate::kenwood::{parse_digits, Token, Tokenizer};

/// Largest frequency the 11-digit field holds.
const MAX_HZ: u64 = 99_999_999_999;
/// Largest RIT/XIT offset, in hertz (`RF` carries four digits).
const MAX_OFFSET_HZ: u16 = 9_999;

/// How a level is written: the command and the guide's range.
struct LevelWire {
    name: &'static str,
    min: u16,
    max: u16,
}

// [@ANCHOR: radioctl_ts890_level_wire]
/// The wire form of a level, or `None` where the guide has no such level command here.
fn level_wire(level: Level) -> Option<LevelWire> {
    let (name, min, max) = match level {
        Level::AfGain => ("AG", 0, 255),
        Level::RfGain => ("RG", 0, 255),
        Level::MicGain => ("MG", 0, 100),
        Level::KeyerSpeed => ("KS", 4, 60),
        Level::RfPower => ("PC", 5, 100),
        Level::MonitorLevel | Level::Attenuator | Level::Preamp => return None,
    };
    Some(LevelWire { name, min, max })
}

// [@ANCHOR: radioctl_ts890_lan_connect_request]
/// The first LAN frame: `##CN;` asks the radio for a connection (guide p. 71); it answers `##CN1;`
/// (authorised) or `##CN0;` (denied).
pub fn lan_connect_request() -> Vec<u8> {
    b"##CN;".to_vec()
}

// [@ANCHOR: radioctl_ts890_lan_login]
/// The second LAN frame, `##ID<type><len><len><account><password>;` (guide p. 72): `administrator`
/// picks account type 0 (administrator) or 1 (user); each length is two digits, 1 to 32.
///
/// # Errors
///
/// `Unsupported` for an empty or over-long account or password, or one that holds a `;` or a
/// non-ASCII byte (the frame could not be told apart from its terminator).
pub fn lan_login(administrator: bool, account: &str, password: &str) -> Result<Vec<u8>, RigError> {
    let fits = |s: &str| {
        (1..=32).contains(&s.len()) && s.bytes().all(|b| (0x20..0x7F).contains(&b) && b != b';')
    };
    if !fits(account) || !fits(password) {
        return Err(RigError::new(
            ErrorKind::Unsupported,
            "LAN account and password are 1 to 32 printable ASCII characters without ';'",
        ));
    }
    Ok(format!(
        "##ID{}{:02}{:02}{account}{password};",
        u8::from(!administrator),
        account.len(),
        password.len()
    )
    .into_bytes())
}

// [@ANCHOR: radioctl_ts890_lan_reply]
/// What a LAN answer says: `Some(true)` for `##CN1;` and `##ID1;` (authorised), `Some(false)` for
/// `##CN0;` and `##ID0;`, `None` for anything else.
pub fn lan_reply(text: &str) -> Option<bool> {
    match text.trim_end_matches(';') {
        "##CN1" | "##ID1" => Some(true),
        "##CN0" | "##ID0" => Some(false),
        _ => None,
    }
}

/// The TS-890S client engine for one rig.
#[derive(Debug)]
pub struct Ts890Engine {
    desc: RigDescription,
    tokenizer: Tokenizer,
    /// `AI2;` was sent and the rig loses it with the LAN or serial session; sent again by
    /// [`ProtocolEngine::link_restored`]. (The LAN login itself is the framing's, repeated by
    /// `net::open_link` on every reconnect, not the engine's.)
    transceive: bool,
}

impl Ts890Engine {
    // [@ANCHOR: radioctl_ts890_engine_new]
    /// Builds an engine from a Kenwood-family description whose dialect is `ts890`.
    ///
    /// # Errors
    ///
    /// `Unsupported` for any other description.
    pub fn new(desc: RigDescription) -> Result<Ts890Engine, RigError> {
        let is_ts890 = desc.family == Family::KenwoodAscii
            && desc
                .kenwood
                .as_ref()
                .is_some_and(|k| k.dialect == KenwoodDialect::Ts890);
        if !is_ts890 {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "not a TS-890S description",
            ));
        }
        Ok(Ts890Engine {
            desc,
            tokenizer: Tokenizer::default(),
            transceive: false,
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

    // [@ANCHOR: radioctl_ts890_engine_decode]
    fn decode(&self, text: &str) -> Inbound {
        if text == "?" {
            return Inbound::Nak;
        }
        let observation = match (text.get(..2), text.get(2..)) {
            (Some("FA"), Some(d)) if d.len() == 11 => {
                parse_digits(d).map(|hz| Observation::Frequency(Vfo::A, Hz(hz)))
            }
            (Some("FB"), Some(d)) if d.len() == 11 => {
                parse_digits(d).map(|hz| Observation::Frequency(Vfo::B, Hz(hz)))
            }
            (Some("OM"), Some(rest)) if rest.len() == 2 && rest.starts_with('0') => rest
                .as_bytes()
                .get(1)
                .copied()
                .and_then(|c| self.desc.code_mode(c))
                .map(Observation::Mode),
            (Some("SM"), Some(d)) if d.len() == 4 => parse_digits(d)
                .and_then(|v| u16::try_from(v).ok())
                .map(Observation::Signal),
            (Some("ID"), Some(d)) if d.len() == 3 => parse_digits(d)
                .and_then(|v| u16::try_from(v).ok())
                .map(Observation::ModelId),
            (Some("FR"), Some(v @ ("0" | "1"))) => Some(Observation::RxVfo(vfo_of(v))),
            (Some("FT"), Some(v @ ("0" | "1"))) => Some(Observation::TxVfo(vfo_of(v))),
            (Some("TB"), Some(v @ ("0" | "1"))) => Some(Observation::Split(v == "1")),
            (Some("RT"), Some(v @ ("0" | "1"))) => Some(Observation::Rit(v == "1")),
            (Some("XT"), Some(v @ ("0" | "1"))) => Some(Observation::Xit(v == "1")),
            (Some("RF"), Some(d)) if d.len() == 5 => decode_offset(d),
            (Some("RA"), Some(d)) => self.decode_step(Level::Attenuator, d),
            (Some("PA"), Some(d)) => self.decode_step(Level::Preamp, d),
            (Some(name), Some(d)) => decode_level(name, d),
            _ => None,
        };
        match observation {
            Some(value) => Inbound::Observation(value),
            None => Inbound::Unrecognised(format!("{text};").into_bytes()),
        }
    }

    // [@ANCHOR: radioctl_ts890_engine_decode_step]
    /// An `RA` or `PA` answer: one digit, the step, mapped by the description.
    fn decode_step(&self, level: Level, digits: &str) -> Option<Observation> {
        if digits.len() != 1 {
            return None;
        }
        let step = u8::try_from(parse_digits(digits)?).ok()?;
        attenuation::value_of(&self.desc, level, step).map(|v| Observation::Level(level, v))
    }
}

// [@ANCHOR: radioctl_ts890_vfo_of]
/// The VFO named by an `FR`/`FT` digit (`0` is A).
fn vfo_of(digit: &str) -> Vfo {
    if digit == "1" {
        Vfo::B
    } else {
        Vfo::A
    }
}

// [@ANCHOR: radioctl_ts890_decode_offset]
/// The `RF` answer after its name: a direction digit (0 plus, 1 minus) and four digits of hertz.
fn decode_offset(digits: &str) -> Option<Observation> {
    let magnitude = i16::try_from(parse_digits(digits.get(1..)?)?).ok()?;
    match digits.get(..1)? {
        "0" => Some(Observation::RitOffset(magnitude)),
        "1" => Some(Observation::RitOffset(-magnitude)),
        _ => None,
    }
}

// [@ANCHOR: radioctl_ts890_decode_level]
/// A numeric level answer: the command name and its three digits (`AG128`).
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
    if digits.len() != 3 {
        return None;
    }
    let value = u16::try_from(parse_digits(digits)?).ok()?;
    Some(Observation::Level(level, value))
}

impl ProtocolEngine for Ts890Engine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_ts890_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(Self::unsupported("capability not in the rig description"));
        }
        let none = ReplyKind::None;
        let observe = ReplyKind::Observation;
        let request = match op {
            Op::GetFrequency(vfo) => {
                let name = if *vfo == Vfo::A { "FA" } else { "FB" };
                Self::command(format!("{name};"), observe(Field::Frequency(*vfo)))
            }
            Op::SetFrequency(vfo, hz) => {
                if hz.0 > MAX_HZ {
                    return Err(Self::unsupported("frequency above 11 digits"));
                }
                let name = if *vfo == Vfo::A { "FA" } else { "FB" };
                Self::command(format!("{name}{:011};", hz.0), none)
            }
            Op::GetMode => Self::command("OM0;".into(), observe(Field::Mode)),
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| Self::unsupported("mode not in the description"))?;
                Self::command(format!("OM0{};", char::from(code)), none)
            }
            Op::SetPtt(true) => Self::command("TX0;".into(), none),
            Op::SetPtt(false) => Self::command("RX;".into(), none),
            Op::GetSignal => Self::command("SM;".into(), observe(Field::Signal)),
            Op::SetTransceive(on) => {
                self.transceive = *on;
                Self::command(format!("AI{};", if *on { 2 } else { 0 }), none)
            }
            Op::GetSplit => Self::command("TB;".into(), observe(Field::Split)),
            Op::SetSplit(on) => Self::command(format!("TB{};", u8::from(*on)), none),
            Op::GetTxVfo => Self::command("FT;".into(), observe(Field::TxVfo)),
            Op::GetRit => Self::command("RT;".into(), observe(Field::Rit)),
            Op::GetXit => Self::command("XT;".into(), observe(Field::Xit)),
            Op::SetRit(on) => Self::command(format!("RT{};", u8::from(*on)), none),
            Op::SetXit(on) => Self::command(format!("XT{};", u8::from(*on)), none),
            Op::GetRitOffset => Self::command("RF;".into(), observe(Field::RitOffset)),
            Op::SetRitOffset(hz) => {
                if hz.unsigned_abs() > MAX_OFFSET_HZ {
                    return Err(Self::unsupported("RIT offset beyond 9999 Hz"));
                }
                // Clear, then move by the whole offset (as the classic engine does).
                let step = match hz {
                    0 => String::new(),
                    h if *h > 0 => format!("RU{:05};", h.unsigned_abs()),
                    h => format!("RD{:05};", h.unsigned_abs()),
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
                Self::command(format!("{name};"), observe(Field::Level(*level)))
            }
            Op::SetLevel(level @ (Level::Attenuator | Level::Preamp), value) => {
                let step = attenuation::step_of(&self.desc, *level, *value)?;
                let name = if *level == Level::Attenuator {
                    "RA"
                } else {
                    "PA"
                };
                Self::command(format!("{name}{step};"), none)
            }
            Op::GetLevel(level) => {
                let wire = level_wire(*level)
                    .ok_or_else(|| Self::unsupported("level not implemented for the TS-890S"))?;
                Self::command(format!("{};", wire.name), observe(Field::Level(*level)))
            }
            Op::SetLevel(level, value) => {
                let wire = level_wire(*level)
                    .ok_or_else(|| Self::unsupported("level not implemented for the TS-890S"))?;
                if *value < wire.min || *value > wire.max {
                    return Err(Self::unsupported("level outside the range of the guide"));
                }
                Self::command(format!("{}{value:03};", wire.name), none)
            }
            Op::GetModelId => Self::command("ID;".into(), observe(Field::ModelId)),
            _ => return Err(Self::unsupported("not implemented for the TS-890S engine")),
        };
        Ok(request)
    }

    // [@ANCHOR: radioctl_ts890_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        self.tokenizer
            .feed(bytes, now)
            .into_iter()
            .map(|token| match token {
                Token::Command(text) => self.decode(&text),
                Token::Junk(junk) => Inbound::Unrecognised(junk),
            })
            .collect()
    }

    // [@ANCHOR: radioctl_ts890_engine_link_restored]
    fn link_restored(&mut self) -> Vec<Request> {
        // A half frame from the old link must not be joined to the new link's first bytes.
        self.tokenizer = Tokenizer::default();
        if !self.transceive {
            return Vec::new();
        }
        // The rig forgot its auto-information with the link: turn it on again.
        match self.encode(&Op::SetTransceive(true)) {
            Ok(request) => vec![request],
            Err(_) => Vec::new(),
        }
    }
}

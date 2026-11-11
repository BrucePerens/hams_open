// SPDX-License-Identifier: AGPL-3.0-or-later
//! Elecraft K3/KX3/KX2 ASCII protocol: sans-IO client engine.
//!
//! Written from Elecraft's published manual only: "Elecraft K3S/K3/KX3/KX2 Programmer's
//! Reference", Rev. G5, February 20, 2019,
//! <https://ftp.elecraft.com/KX3/Manuals%20Downloads/K3S&K3&KX3&KX2%20Pgmrs%20Ref,%20G5.pdf>,
//! sha256 `015b60c1b5f44346247a00375a23174012ce4da66a6a27eaa00c00c20877f95d`. No Hamlib source
//! was read for this module. The manual has no numbered sections, so citations below name the
//! manual's own headings and the printed page number ("p."):
//!
//! - Command Format and Response Time (p. 3): commands are 2 or 3 letters, optional data, `;`;
//!   replies are upper case; worst-case latency about 100 ms, band changes up to 500 ms.
//! - "Busy/Limited Access Indication (?;)" (p. 3): the refusal is the token `?;`.
//! - "Meta-commands: AI, K2 and K3" (p. 4): `AI0` none, `AI1` an `IF` answer on frequency or
//!   mode events, `AI2` the specific response (`FA`, `MD`, ...) for each front-panel event.
//! - "AG" and "AI" (p. 7); "BW" (p. 8); "FA and FB", "FR" and "FT" (p. 11); "ID" and "IF"
//!   (p. 14); "KS" and "KY" (p. 15); "MD", "MG" and "ML" (p. 16); "PC", "PO" and "RC" (p. 22);
//!   "RG", "RO", "RT", "RV", "RX" and "SM" (p. 23); "SW" (p. 24); "TQ" (p. 27); "TX" and "XT"
//!   (p. 28).
//!
//! Choices, all from the manual: only the Basic response formats are used and the K2/K3 command
//! extension modes (`K2n;`, `K3n;`) are never sent, so `SM` is 0000-0015 and the `IF` fields
//! `b` and `d` are always 0. The non-modal `BW` is used instead of the legacy `FW` ("BW" note 1,
//! p. 8). Transmit state is read with `TQ` ("TQ", p. 28: the preferred way). `FR` answers are not
//! decoded: the manual says its value is ignored and any `FR` SET only cancels split.
//!
//! `SET` commands get no reply ("Command Format", p. 3), so the engine reports them as complete
//! when written; a refusal (`?;`) that arrives later is an [`Inbound::Nak`] the actor logs. Not
//! implemented: the `KY` buffer-full GET (`KY;`), `TB` text reads, menus (`MN`/`MP`), switch
//! emulation (`SWT`/`SWH`), the `$` sub-receiver/VFO B forms, memories and DSP commands.
//!
//! Transmit-capable commands: `TX;` and `KY` text. Both are gated by the rig actor and require
//! the capability in the description; `KY` text containing `<` or `>` (which the manual says
//! switch the radio between TX TEST and TX NORM, p. 15) is refused here, as are `;` and control
//! characters, so a text cannot smuggle a second command.

use std::time::Duration;

use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Level, Mode, Vfo};

use crate::kenwood::{parse_digits, Token, Tokenizer};

/// Largest frequency the 11-digit field holds.
const MAX_HZ: u64 = 99_999_999_999;
/// Longest `KY` text the radio takes in one command ("KY", p. 15: 0 to 24 characters).
pub const MAX_KY_CHARS: usize = 24;
/// Largest RIT/XIT offset under computer control ("RD", p. 23: -9.999 to +9.999 kHz).
pub const MAX_RIT_HZ: i16 = 9_999;

/// The command prefix and the allowed range of a level, from the manual.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelWire {
    /// The two-letter command.
    pub prefix: &'static str,
    /// Lowest value.
    pub min: u16,
    /// Highest value.
    pub max: u16,
}

// [@ANCHOR: radioctl_elecraft_level_wire]
/// The wire command and range of a level: `AG` 0-255, `RG` 0-250, `MG` 0-60, `ML` 0-60,
/// `KS` 8-50 words per minute, `PC` 0-110 (the largest the manual lists, with a KXPA100).
pub fn level_wire(level: Level) -> LevelWire {
    let (prefix, min, max) = match level {
        Level::AfGain => ("AG", 0, 255),
        Level::RfGain => ("RG", 0, 250),
        Level::MicGain => ("MG", 0, 60),
        Level::MonitorLevel => ("ML", 0, 60),
        Level::KeyerSpeed => ("KS", 8, 50),
        Level::RfPower => ("PC", 0, 110),
        // Not numeric levels: `RAnn;` and `PAn;` carry steps (see `front_end_set`). The ranges
        // here are never read for them; the prefixes make the command names available.
        Level::Attenuator => ("RA", 0, 0),
        Level::Preamp => ("PA", 0, 0),
        // Receiver-board levels with no Elecraft command; the empty prefix matches no reply and the
        // encoder refuses both before it asks for a wire form.
        Level::Dither | Level::Preselector | Level::Antenna => ("", 0, 0),
    };
    LevelWire { prefix, min, max }
}

// [@ANCHOR: radioctl_elecraft_level_wire_for]
/// The wire command and range of a level on the described model: [`level_wire`] with the
/// description's per-model range (`[elecraft] levels`) where it gives one. A real KX3 (MCU 3.02)
/// clamps `AG` to 0-60, `RG` to 190-250, `MG` to 0-80 and `PC` to its maximum power, so the K3
/// ranges made a Hamlib fraction mean something else on it (half power was full power).
pub fn level_wire_for(desc: &RigDescription, level: Level) -> LevelWire {
    let wire = level_wire(level);
    match desc.elecraft.as_ref().and_then(|e| e.level_range(level)) {
        Some((min, max)) => LevelWire { min, max, ..wire },
        None => wire,
    }
}

/// Decibels relative to S9 for each `SM` reading 0 to 15, as Hamlib 4.6.2's `K3_SM_CAL` has them
/// (rigs/kenwood/elecraft.h). The manual's anchors agree: S9 = 6, S9+20 = 9, S9+40 = 12,
/// S9+60 = 15 ("SM", p. 23).
const SM_DB: [i32; 16] = [
    -54, -42, -36, -24, -12, -6, 0, 10, 15, 20, 30, 35, 40, 50, 55, 60,
];

// [@ANCHOR: radioctl_elecraft_strength_db]
/// The signal strength in decibels relative to S9 for an `SM` reading (0 to 15, the basic
/// format): -54 at 0, 0 at 6 (S9), +60 at 15. `None` for a reading outside 0 to 15.
pub fn strength_db(sm: u16) -> Option<i32> {
    SM_DB.get(usize::from(sm)).copied()
}

// [@ANCHOR: radioctl_elecraft_parse_power_amplifier]
/// Parses `OM` data ("OM", p. 21) for whether the power amplifier module is present: a blank
/// and 12 option positions, in the second of which `P` means installed and `-` absent. The
/// K3 and K3S list `APXSDFfLVR--` (P is the KPA3); the KX3 and KX2 list `APF---TBXI0n` (P is the
/// KXPA100, the external 100 W amplifier). `None` when the answer is not that shape.
pub fn parse_power_amplifier(data: &str) -> Option<bool> {
    let positions = data.strip_prefix(' ').unwrap_or(data);
    if positions.len() != 12 || !positions.is_ascii() {
        return None;
    }
    match positions.as_bytes().get(1)? {
        b'P' => Some(true),
        b'-' => Some(false),
        _ => None,
    }
}

// [@ANCHOR: radioctl_elecraft_signed_offset]
/// Formats a RIT/XIT offset as the `ROsnnnn` data (`+0050`, `-0050`).
pub fn signed_offset(hz: i16) -> String {
    let sign = if hz < 0 { '-' } else { '+' };
    format!("{sign}{:04}", hz.unsigned_abs())
}

// [@ANCHOR: radioctl_elecraft_parse_offset]
/// Parses `ROsnnnn` data: a sign (`+`, `-` or a space for plus) and four digits.
pub fn parse_offset(data: &str) -> Option<i16> {
    let (sign, digits) = (data.get(..1)?, data.get(1..)?);
    if digits.len() != 4 {
        return None;
    }
    let magnitude = i16::try_from(parse_digits(digits)?).ok()?;
    match sign {
        "+" | " " => Some(magnitude),
        "-" => Some(-magnitude),
        _ => None,
    }
}

// [@ANCHOR: radioctl_elecraft_parse_firmware]
/// Parses the `NN.NN` revision of an `RVx` answer.
pub fn parse_firmware(text: &str) -> Option<(u8, u8)> {
    let (major, minor) = text.split_once('.')?;
    if major.len() != 2 || minor.len() != 2 {
        return None;
    }
    let major = u8::try_from(parse_digits(major)?).ok()?;
    let minor = u8::try_from(parse_digits(minor)?).ok()?;
    Some((major, minor))
}

// [@ANCHOR: radioctl_elecraft_decode_if]
/// Decodes the payload of an `IF` answer ("IF", p. 14): `[f]*****+yyyyrx*00tmvspbd1*`, as text
/// after the `IF`. The tokenizer has already dropped the trailing blank. Returns the frequency of
/// VFO A, the RIT/XIT offset, RIT, XIT, transmit state, mode and split, in that order. `None`
/// when a fixed field is wrong or the mode has no code in the description.
pub fn decode_if(payload: &str, desc: &RigDescription) -> Option<Vec<Observation>> {
    if !(34..=35).contains(&payload.len()) || !payload.is_ascii() {
        return None;
    }
    let field = |from: usize, to: usize| payload.get(from..to);
    let flag = |at: usize| match field(at, at + 1) {
        Some("0") => Some(false),
        Some("1") => Some(true),
        _ => None,
    };
    if field(11, 16)? != "     " || field(23, 26)? != " 00" {
        return None;
    }
    let hz = parse_digits(field(0, 11)?)?;
    let offset = parse_offset(field(16, 21)?)?;
    let mode_code = *field(27, 28)?.as_bytes().first()?;
    let split = flag(30)?;
    Some(vec![
        Observation::Frequency(Vfo::A, Hz(hz)),
        Observation::RitOffset(offset),
        Observation::Rit(flag(21)?),
        Observation::Xit(flag(22)?),
        Observation::Ptt(flag(26)?),
        Observation::Mode(desc.code_mode(mode_code)?),
        Observation::Split(split),
        // The KX3 transmits on VFO B in split (`FT1`) and on A otherwise.
        Observation::TxVfo(if split { Vfo::B } else { Vfo::A }),
    ])
}

/// The Elecraft ASCII client engine for one rig.
#[derive(Debug)]
pub struct ElecraftEngine {
    desc: RigDescription,
    tokenizer: Tokenizer,
    /// The auto-information the controller turned on (`AI2;`), which the rig loses with the
    /// link; sent again by [`ProtocolEngine::link_restored`].
    transceive: bool,
}

impl ElecraftEngine {
    // [@ANCHOR: radioctl_elecraft_engine_new]
    /// Builds an engine from an Elecraft description.
    pub fn new(desc: RigDescription) -> Result<ElecraftEngine, RigError> {
        if desc.family != Family::ElecraftAscii || desc.elecraft.is_none() {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "not an Elecraft description",
            ));
        }
        Ok(ElecraftEngine {
            desc,
            tokenizer: Tokenizer::default(),
            transceive: false,
        })
    }

    fn unsupported(what: &str) -> RigError {
        RigError::new(ErrorKind::Unsupported, what.to_string())
    }

    fn get(text: &str, field: Field) -> Request {
        Request {
            bytes: text.as_bytes().to_vec(),
            reply: ReplyKind::Observation(field),
        }
    }

    fn set(text: String) -> Request {
        Request {
            bytes: text.into_bytes(),
            reply: ReplyKind::None,
        }
    }

    // [@ANCHOR: radioctl_elecraft_check_ky_text]
    /// Accepts `KY` text of 1 to 24 printable ASCII characters without `;`, `<` or `>`.
    fn check_ky_text(text: &str) -> Result<(), RigError> {
        let ok = !text.is_empty()
            && text.len() <= MAX_KY_CHARS
            && text
                .bytes()
                .all(|b| (0x20..0x7F).contains(&b) && !matches!(b, b';' | b'<' | b'>'));
        if ok {
            Ok(())
        } else {
            Err(Self::unsupported(
                "CW text must be 1 to 24 printable ASCII characters without ; < >",
            ))
        }
    }

    // [@ANCHOR: radioctl_elecraft_encode_set]
    fn encode_set(&mut self, op: &Op) -> Result<Request, RigError> {
        Ok(match op {
            Op::SetFrequency(vfo, hz) => {
                if hz.0 > MAX_HZ {
                    return Err(Self::unsupported("frequency above 11 digits"));
                }
                let name = if *vfo == Vfo::A { "FA" } else { "FB" };
                Self::set(format!("{name}{:011};", hz.0))
            }
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| Self::unsupported("mode not in the description"))?;
                Self::set(format!("MD{};", char::from(code)))
            }
            Op::SetPtt(true) => Self::set("TX;".into()),
            Op::SetPtt(false) => Self::set("RX;".into()),
            Op::SetTransceive(on) => {
                self.transceive = *on;
                Self::set(format!("AI{};", if *on { 2 } else { 0 }))
            }
            Op::SetSplit(true) => Self::set("FT1;".into()),
            Op::SetSplit(false) => Self::set("FR0;".into()),
            Op::SetRit(on) => Self::set(format!("RT{};", u8::from(*on))),
            Op::SetXit(on) => Self::set(format!("XT{};", u8::from(*on))),
            Op::SetRitOffset(hz) => {
                if hz.unsigned_abs() > MAX_RIT_HZ.unsigned_abs() {
                    return Err(Self::unsupported("RIT/XIT offset beyond +-9999 Hz"));
                }
                Self::set(format!("RO{};", signed_offset(*hz)))
            }
            Op::SetFilterWidth(hz) => {
                if hz.0 % 10 != 0 || hz.0 > 99_990 {
                    return Err(Self::unsupported(
                        "filter width must be a multiple of 10 Hz up to 99990",
                    ));
                }
                Self::set(format!("BW{:04};", hz.0 / 10))
            }
            Op::GetLevel(Level::Dither | Level::Preselector | Level::Antenna)
            | Op::SetLevel(Level::Dither | Level::Preselector | Level::Antenna, _) => {
                return Err(Self::unsupported("no such level on this rig"));
            }
            Op::SetLevel(level @ (Level::Attenuator | Level::Preamp), value) => {
                let step = crate::attenuation::step_of(&self.desc, *level, *value)?;
                if *level == Level::Attenuator {
                    Self::set(format!("RA{step:02};"))
                } else {
                    Self::set(format!("PA{step};"))
                }
            }
            Op::SetLevel(level, value) => {
                let wire = level_wire_for(&self.desc, *level);
                if *value < wire.min || *value > wire.max {
                    return Err(Self::unsupported("level outside the rig's range"));
                }
                Self::set(format!("{}{value:03};", wire.prefix))
            }
            Op::SendCw(text) => {
                Self::check_ky_text(text)?;
                Self::set(format!("KY {text};"))
            }
            // "KY" (p. 15): `^D (EOT, ASCII 04) Quickly terminates transmission`. The other
            // terminator the page lists, `@`, is a prosign instead when CONFIG:CW WGHT has been
            // changed, so EOT is the one that cannot be remapped. Sent as `KY <space><EOT>;`
            // (the blank is the KY mode character, `KY*[text]`).
            //
            // A real KX3 (MCU 3.02, 2026-11-05) does NOT stop on it: with `KY TEST TEST TEST;`
            // buffered at 8 WPM it went on keying the carrier for more than 2 s after the EOT (and
            // after `KY @;`). `RX;` ("RX", p. 23) stopped it at once and the rest of the text was
            // not sent. So the stop is the EOT, which empties the buffer on radios that honour it,
            // followed by `RX;`, which ends the transmission on all of them.
            Op::StopCw => Self::set("KY \u{4};RX;".to_string()),
            _ => return Err(Self::unsupported("not a setting command")),
        })
    }

    // [@ANCHOR: radioctl_elecraft_decode]
    fn decode(&self, text: &str) -> Vec<Inbound> {
        if text == "?" {
            return vec![Inbound::Nak];
        }
        if let Some(payload) = text.strip_prefix("IF") {
            return match decode_if(payload, &self.desc) {
                Some(list) => list.into_iter().map(Inbound::Observation).collect(),
                None => vec![Self::unrecognised(text)],
            };
        }
        match self.decode_one(text) {
            // `FT` is the transmit-VFO command: FT1 transmits on B, FT0 on A.
            Some(value @ Observation::Split(on)) if text.starts_with("FT") => vec![
                Inbound::Observation(value),
                Inbound::Observation(Observation::TxVfo(if on { Vfo::B } else { Vfo::A })),
            ],
            Some(value) => vec![Inbound::Observation(value)],
            None => vec![Self::unrecognised(text)],
        }
    }

    fn unrecognised(text: &str) -> Inbound {
        Inbound::Unrecognised(format!("{text};").into_bytes())
    }

    // [@ANCHOR: radioctl_elecraft_decode_one]
    fn decode_one(&self, text: &str) -> Option<Observation> {
        let (name, data) = (text.get(..2)?, text.get(2..)?);
        let number = |digits: usize| {
            (data.len() == digits)
                .then(|| parse_digits(data))
                .flatten()
                .and_then(|v| u16::try_from(v).ok())
        };
        let flag = || match data {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        };
        if matches!(name, "RA" | "PA") {
            let (level, width) = if name == "RA" {
                (Level::Attenuator, 2)
            } else {
                (Level::Preamp, 1)
            };
            return number(width)
                .and_then(|step| u8::try_from(step).ok())
                .and_then(|step| crate::attenuation::value_of(&self.desc, level, step))
                .map(|v| Observation::Level(level, v));
        }
        if let Some(level) = [
            Level::AfGain,
            Level::RfGain,
            Level::MicGain,
            Level::MonitorLevel,
            Level::KeyerSpeed,
            Level::RfPower,
        ]
        .into_iter()
        .find(|l| level_wire(*l).prefix == name)
        {
            let wire = level_wire_for(&self.desc, level);
            return number(3)
                .filter(|v| *v >= wire.min && *v <= wire.max)
                .map(|v| Observation::Level(level, v));
        }
        match name {
            "FA" | "FB" if data.len() == 11 => {
                let vfo = if name == "FA" { Vfo::A } else { Vfo::B };
                parse_digits(data).map(|hz| Observation::Frequency(vfo, Hz(hz)))
            }
            "MD" if data.len() == 1 => {
                let code = *data.as_bytes().first()?;
                self.desc.code_mode(code).map(Observation::Mode)
            }
            "TQ" => flag().map(Observation::Ptt),
            "FT" => flag().map(Observation::Split),
            "RT" => flag().map(Observation::Rit),
            "XT" => flag().map(Observation::Xit),
            "RO" => parse_offset(data).map(Observation::RitOffset),
            "SM" => number(4).filter(|v| *v <= 15).map(Observation::Signal),
            "PO" => number(3).map(Observation::Power),
            // The manual gives 010-999, but a KX3 (MCU 3.02) answers `SW000;` in receive: no
            // reading. It is passed on as 0, which is what Hamlib's KX3 backend reports too.
            "SW" => number(3)
                .filter(|v| *v == 0 || (10..=999).contains(v))
                .map(Observation::Swr),
            "BW" => number(4).map(|v| Observation::FilterWidth(Hz(u64::from(v) * 10))),
            "ID" => number(3).map(Observation::ModelId),
            "OM" => parse_power_amplifier(data).map(Observation::PowerAmplifier),
            "RV" => {
                let revision = data.strip_prefix('M')?;
                let (major, minor) = parse_firmware(revision)?;
                Some(Observation::Firmware(major, minor))
            }
            _ => None,
        }
    }
}

impl ProtocolEngine for ElecraftEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_elecraft_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(Self::unsupported("capability not in the rig description"));
        }
        let level_get = |level: Level| {
            if matches!(level, Level::Dither | Level::Preselector) {
                return Err(Self::unsupported("no such level on this rig"));
            }
            if crate::attenuation::is_front_end_level(level) {
                crate::attenuation::step_of(&self.desc, level, 0)?;
            }
            let command = format!("{};", level_wire(level).prefix);
            Ok::<Request, RigError>(Self::get(&command, Field::Level(level)))
        };
        Ok(match op {
            Op::GetFrequency(Vfo::A) => Self::get("FA;", Field::Frequency(Vfo::A)),
            Op::GetFrequency(Vfo::B) => Self::get("FB;", Field::Frequency(Vfo::B)),
            Op::GetMode => Self::get("MD;", Field::Mode),
            Op::GetPtt => Self::get("TQ;", Field::Ptt),
            Op::GetSignal => Self::get("SM;", Field::Signal),
            Op::GetPower => Self::get("PO;", Field::Power),
            Op::GetSplit => Self::get("FT;", Field::Split),
            Op::GetTxVfo => Self::get("FT;", Field::TxVfo),
            Op::GetRit => Self::get("RT;", Field::Rit),
            Op::GetXit => Self::get("XT;", Field::Xit),
            Op::GetRitOffset => Self::get("RO;", Field::RitOffset),
            Op::GetFilterWidth => Self::get("BW;", Field::FilterWidth),
            Op::GetLevel(level) => level_get(*level)?,
            Op::GetSwr => Self::get("SW;", Field::Swr),
            Op::GetModelId => Self::get("ID;", Field::ModelId),
            Op::GetPowerAmplifier => Self::get("OM;", Field::PowerAmplifier),
            Op::GetFirmware => Self::get("RVM;", Field::Firmware),
            other => return self.encode_set(other),
        })
    }

    // [@ANCHOR: radioctl_elecraft_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let tokens = self.tokenizer.feed(bytes, now);
        let mut out = Vec::new();
        for token in tokens {
            match token {
                Token::Command(text) => out.extend(self.decode(&text)),
                Token::Junk(junk) => out.push(Inbound::Unrecognised(junk)),
            }
        }
        out
    }

    // [@ANCHOR: radioctl_elecraft_engine_link_restored]
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

/// The mode the manual lists for each `MD` digit (for documentation and tests):
/// 1 LSB, 2 USB, 3 CW, 4 FM, 5 AM, 6 DATA, 7 CW-REV, 9 DATA-REV ("MD", p. 16).
pub const MODE_DIGITS: [(Mode, u8); 8] = [
    (Mode::Lsb, b'1'),
    (Mode::Usb, b'2'),
    (Mode::Cw, b'3'),
    (Mode::Fm, b'4'),
    (Mode::Am, b'5'),
    (Mode::Data, b'6'),
    (Mode::CwR, b'7'),
    (Mode::DataR, b'9'),
];

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::rigfile;

    const T0: Duration = Duration::from_secs(1);

    fn kx3() -> RigDescription {
        rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))
            .expect("kx3 file")
            .description
    }

    fn engine() -> ElecraftEngine {
        ElecraftEngine::new(kx3()).expect("engine")
    }

    fn bytes(engine: &mut ElecraftEngine, op: Op) -> Vec<u8> {
        engine.encode(&op).expect("encodes").bytes
    }

    // Tests [@ANCHOR: radioctl_elecraft_level_wire]
    // Tests [@ANCHOR: radioctl_elecraft_signed_offset]
    // Tests [@ANCHOR: radioctl_elecraft_parse_offset]
    // Tests [@ANCHOR: radioctl_elecraft_parse_firmware]
    #[test]
    fn small_codecs_follow_the_manual() {
        assert_eq!(level_wire(Level::KeyerSpeed).min, 8);
        assert_eq!(level_wire(Level::RfGain).max, 250);
        assert_eq!(level_wire(Level::AfGain).prefix, "AG");
        assert_eq!(signed_offset(50), "+0050");
        assert_eq!(signed_offset(-9999), "-9999");
        assert_eq!(signed_offset(0), "+0000");
        for hz in [-9999, -1, 0, 7, 9999] {
            assert_eq!(parse_offset(&signed_offset(hz)), Some(hz));
        }
        assert_eq!(parse_offset(" 0050"), Some(50));
        assert_eq!(parse_offset("*0050"), None);
        assert_eq!(parse_offset("+050"), None);
        assert_eq!(parse_firmware("02.37"), Some((2, 37)));
        assert_eq!(parse_firmware("2.37"), None);
        assert_eq!(parse_firmware("99.99"), Some((99, 99)));
    }

    // Tests [@ANCHOR: radioctl_elecraft_parse_power_amplifier]
    // Tests [@ANCHOR: radioctl_elecraft_engine_encode]
    // Tests [@ANCHOR: radioctl_elecraft_decode_one]
    #[test]
    fn the_om_answer_tells_whether_a_power_amplifier_is_installed() {
        // The answer a real KX3 (KXAT3 and KXFL3, no KXPA100) gave on 2026-11-05.
        assert_eq!(parse_power_amplifier(" A-F----B--02"), Some(false));
        // The manual's examples: K3 with a PA and sub receiver, KX3 with a KXPA100.
        assert_eq!(parse_power_amplifier(" -P-S--------"), Some(true));
        assert_eq!(parse_power_amplifier(" APF---TBXI01"), Some(true));
        assert_eq!(parse_power_amplifier(" AXF---TBXI01"), None);
        assert_eq!(parse_power_amplifier(" A-F"), None);
        let mut e = engine();
        assert_eq!(bytes(&mut e, Op::GetPowerAmplifier), b"OM;");
        assert_eq!(
            e.feed(b"OM A-F----B--02;", T0),
            vec![Inbound::Observation(Observation::PowerAmplifier(false))]
        );
        assert_eq!(
            e.feed(b"OM APF----B--02;", T0),
            vec![Inbound::Observation(Observation::PowerAmplifier(true))]
        );
        assert!(matches!(
            e.feed(b"OM garbage;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }

    // Tests [@ANCHOR: radioctl_elecraft_strength_db]
    #[test]
    fn strength_follows_the_sm_anchors_of_the_manual_and_hamlibs_table() {
        // "SM", p. 23: S9 = 6, S9+20 = 9, S9+40 = 12, S9+60 = 15.
        assert_eq!(strength_db(6), Some(0));
        assert_eq!(strength_db(9), Some(20));
        assert_eq!(strength_db(12), Some(40));
        assert_eq!(strength_db(15), Some(60));
        // Hamlib's KX3 serves -54 at SM0000.
        assert_eq!(strength_db(0), Some(-54));
        assert_eq!(strength_db(16), None);
        assert!(
            SM_DB.windows(2).all(|w| w[0] < w[1]),
            "rises with the reading"
        );
    }

    // Tests [@ANCHOR: radioctl_elecraft_level_wire_for]
    #[test]
    fn the_kx3_levels_use_its_own_ranges() {
        let desc = kx3();
        assert_eq!(level_wire_for(&desc, Level::AfGain).max, 60);
        assert_eq!(level_wire_for(&desc, Level::RfGain).min, 190);
        assert_eq!(level_wire_for(&desc, Level::MicGain).max, 80);
        assert_eq!(level_wire_for(&desc, Level::RfPower).max, 15);
        assert_eq!(
            level_wire_for(&desc, Level::KeyerSpeed),
            level_wire(Level::KeyerSpeed)
        );
        let mut e = engine();
        assert_eq!(bytes(&mut e, Op::SetLevel(Level::RfPower, 15)), b"PC015;");
        for op in [
            Op::SetLevel(Level::RfPower, 16),
            Op::SetLevel(Level::AfGain, 61),
            Op::SetLevel(Level::RfGain, 189),
        ] {
            assert!(e.encode(&op).is_err(), "{op:?}");
        }
        assert!(matches!(
            e.feed(b"AG061;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert_eq!(
            e.feed(b"RG190;", T0),
            vec![Inbound::Observation(Observation::Level(Level::RfGain, 190))]
        );
    }

    // Tests [@ANCHOR: radioctl_elecraft_level_wire_for]
    #[test]
    fn the_kx2_k3_and_k3s_state_their_own_ranges_and_never_the_110_watt_default() {
        let load = |text: &str| rigfile::parse(text).expect("parses").description;
        let kx2 = load(include_str!("../../../rigs/elecraft-kx2.toml"));
        let k3 = load(include_str!("../../../rigs/elecraft-k3.toml"));
        let k3s = load(include_str!("../../../rigs/elecraft-k3s.toml"));
        // The KX2 shares the KX3's AG, RG and MG ranges (Hamlib's kx3_set_level serves both) and
        // tops out at 12 W, the smallest per-band maximum in the Programmer's Reference and the
        // Owner's Manual.
        let kx2_ranges = [
            (Level::AfGain, 0, 60),
            (Level::RfGain, 190, 250),
            (Level::MicGain, 0, 80),
            (Level::RfPower, 0, 12),
        ];
        // The K3 and K3S: the manual's ranges, and the K3 without a KPA3 (12 W) for power.
        let k3_ranges = [
            (Level::AfGain, 0, 255),
            (Level::RfGain, 0, 250),
            (Level::MicGain, 0, 60),
            (Level::RfPower, 0, 12),
        ];
        for (desc, ranges) in [(&kx2, kx2_ranges), (&k3, k3_ranges), (&k3s, k3_ranges)] {
            for (level, min, max) in ranges {
                let wire = level_wire_for(desc, level);
                assert_eq!((wire.min, wire.max), (min, max), "{} {level:?}", desc.id);
            }
        }
        // Half of the 12 W scale is PC006, and one watt past it is refused, not sent.
        for desc in [kx2, k3, k3s] {
            let mut e = ElecraftEngine::new(desc).expect("engine");
            assert_eq!(bytes(&mut e, Op::SetLevel(Level::RfPower, 6)), b"PC006;");
            assert!(e.encode(&Op::SetLevel(Level::RfPower, 13)).is_err());
            assert!(matches!(
                e.feed(b"PC055;", T0).as_slice(),
                [Inbound::Unrecognised(_)]
            ));
        }
    }

    // Tests [@ANCHOR: radioctl_elecraft_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        let err = ElecraftEngine::new(RigDescription::ts2000()).expect_err("must refuse");
        assert_eq!(err.kind, ErrorKind::Unsupported);
    }

    // Tests [@ANCHOR: radioctl_elecraft_engine_encode]
    // Tests [@ANCHOR: radioctl_elecraft_encode_set]
    #[test]
    fn encode_matches_the_programmers_reference() {
        let mut e = engine();
        assert_eq!(bytes(&mut e, Op::GetFrequency(Vfo::A)), b"FA;");
        assert_eq!(bytes(&mut e, Op::GetFrequency(Vfo::B)), b"FB;");
        assert_eq!(
            bytes(&mut e, Op::SetFrequency(Vfo::A, Hz(14_060_000))),
            b"FA00014060000;"
        );
        assert_eq!(bytes(&mut e, Op::SetMode(Mode::Cw)), b"MD3;");
        assert_eq!(bytes(&mut e, Op::SetMode(Mode::DataR)), b"MD9;");
        assert_eq!(bytes(&mut e, Op::GetPtt), b"TQ;");
        assert_eq!(bytes(&mut e, Op::SetPtt(true)), b"TX;");
        assert_eq!(bytes(&mut e, Op::SetPtt(false)), b"RX;");
        assert_eq!(bytes(&mut e, Op::SetTransceive(true)), b"AI2;");
        assert_eq!(bytes(&mut e, Op::SetTransceive(false)), b"AI0;");
        assert_eq!(bytes(&mut e, Op::SetSplit(true)), b"FT1;");
        assert_eq!(bytes(&mut e, Op::SetSplit(false)), b"FR0;");
        assert_eq!(bytes(&mut e, Op::GetSplit), b"FT;");
        assert_eq!(bytes(&mut e, Op::SetRit(true)), b"RT1;");
        assert_eq!(bytes(&mut e, Op::SetXit(false)), b"XT0;");
        assert_eq!(bytes(&mut e, Op::SetRitOffset(-50)), b"RO-0050;");
        assert_eq!(bytes(&mut e, Op::GetRitOffset), b"RO;");
        assert_eq!(bytes(&mut e, Op::SetFilterWidth(Hz(500))), b"BW0050;");
        assert_eq!(bytes(&mut e, Op::GetFilterWidth), b"BW;");
        assert_eq!(
            bytes(&mut e, Op::SetLevel(Level::KeyerSpeed, 20)),
            b"KS020;"
        );
        assert_eq!(bytes(&mut e, Op::GetLevel(Level::MicGain)), b"MG;");
        assert_eq!(bytes(&mut e, Op::SetLevel(Level::RfPower, 5)), b"PC005;");
        assert_eq!(bytes(&mut e, Op::GetSignal), b"SM;");
        assert_eq!(bytes(&mut e, Op::GetPower), b"PO;");
        assert_eq!(bytes(&mut e, Op::GetSwr), b"SW;");
        assert_eq!(bytes(&mut e, Op::GetModelId), b"ID;");
        assert_eq!(bytes(&mut e, Op::GetFirmware), b"RVM;");
        let set = e.encode(&Op::SetMode(Mode::Usb)).expect("set");
        assert_eq!(set.reply, ReplyKind::None);
        let get = e.encode(&Op::GetMode).expect("get");
        assert_eq!(get.reply, ReplyKind::Observation(Field::Mode));
        assert_eq!(bytes(&mut e, Op::SendCw("CQ TEST".into())), b"KY CQ TEST;");
        assert_eq!(bytes(&mut e, Op::StopCw), b"KY \x04;RX;");
    }

    // Tests [@ANCHOR: radioctl_elecraft_engine_encode]
    // Tests [@ANCHOR: radioctl_elecraft_encode_set]
    // Tests [@ANCHOR: radioctl_elecraft_check_ky_text]
    #[test]
    fn out_of_range_values_and_unsafe_text_are_refused() {
        let mut e = engine();
        let refused = |e: &mut ElecraftEngine, op: Op| {
            assert_eq!(
                e.encode(&op).expect_err("refused").kind,
                ErrorKind::Unsupported,
                "{op:?}"
            )
        };
        refused(&mut e, Op::SetFrequency(Vfo::A, Hz(100_000_000_000)));
        refused(&mut e, Op::SetMode(Mode::Rtty));
        refused(&mut e, Op::SetRitOffset(10_000));
        refused(&mut e, Op::SetFilterWidth(Hz(505)));
        refused(&mut e, Op::SetFilterWidth(Hz(100_000)));
        refused(&mut e, Op::SetLevel(Level::KeyerSpeed, 7));
        refused(&mut e, Op::SetLevel(Level::AfGain, 256));
        refused(&mut e, Op::SetLevel(Level::RfPower, 111));
        refused(&mut e, Op::SendCw(String::new()));
        refused(&mut e, Op::SendCw("A".repeat(25)));
        refused(&mut e, Op::SendCw("CQ;FA00007000000".into()));
        refused(&mut e, Op::SendCw("<".into()));
        refused(&mut e, Op::SendCw(">".into()));
        refused(&mut e, Op::SendCw("A\u{4}B".into()));
        refused(&mut e, Op::SendCw("caf\u{e9}".into()));
        assert!(e.encode(&Op::SendCw("A".repeat(24))).is_ok());
        let mut ts = ElecraftEngine::new(kx3()).expect("engine");
        ts.desc.capabilities = radioctl_core::capability::CapabilitySet::of(&[]);
        refused(&mut ts, Op::GetMode);
    }

    // Tests [@ANCHOR: radioctl_elecraft_engine_feed]
    // Tests [@ANCHOR: radioctl_elecraft_decode]
    // Tests [@ANCHOR: radioctl_elecraft_decode_one]
    #[test]
    fn feed_decodes_every_reply_the_engine_asks_for() {
        let mut e = engine();
        let one = |e: &mut ElecraftEngine, text: &str| e.feed(text.as_bytes(), T0);
        use Observation as O;
        let expect = |text: &str, value: Observation| {
            let mut e = engine();
            let mut want = vec![Inbound::Observation(value)];
            if let Observation::Split(on) = value {
                // `FT` also says which VFO transmits.
                let tx = if on { Vfo::B } else { Vfo::A };
                want.push(Inbound::Observation(Observation::TxVfo(tx)));
            }
            assert_eq!(one(&mut e, text), want, "{text}");
        };
        expect("FA00014060000;", O::Frequency(Vfo::A, Hz(14_060_000)));
        expect("FB00007040000;", O::Frequency(Vfo::B, Hz(7_040_000)));
        expect("MD3;", O::Mode(Mode::Cw));
        expect("MD9;", O::Mode(Mode::DataR));
        expect("TQ1;", O::Ptt(true));
        expect("FT0;", O::Split(false));
        expect("RT1;", O::Rit(true));
        expect("XT0;", O::Xit(false));
        expect("RO-0050;", O::RitOffset(-50));
        expect("SM0006;", O::Signal(6));
        expect("PO050;", O::Power(50));
        expect("SW015;", O::Swr(15));
        expect("SW000;", O::Swr(0));
        expect("BW0050;", O::FilterWidth(Hz(500)));
        expect("AG040;", O::Level(Level::AfGain, 40));
        expect("RG250;", O::Level(Level::RfGain, 250));
        expect("KS020;", O::Level(Level::KeyerSpeed, 20));
        expect("PC005;", O::Level(Level::RfPower, 5));
        expect("ID017;", O::ModelId(17));
        expect("RVM02.37;", O::Firmware(2, 37));
        assert_eq!(
            one(&mut e, "FA00014060000;MD3;?;"),
            vec![
                Inbound::Observation(O::Frequency(Vfo::A, Hz(14_060_000))),
                Inbound::Observation(O::Mode(Mode::Cw)),
                Inbound::Nak,
            ]
        );
        for bad in [
            "FA123;",
            "MD8;",
            "SM0016;",
            "SW005;",
            "KS007;",
            "RVD02.37;",
            "ZZ;",
            "TQ2;",
            "FR0;",
        ] {
            assert!(
                matches!(one(&mut e, bad).as_slice(), [Inbound::Unrecognised(_)]),
                "{bad}"
            );
        }
        assert!(e.feed(b"FA000140", T0).is_empty());
        assert_eq!(
            e.feed(b"60000;", T0),
            vec![Inbound::Observation(O::Frequency(Vfo::A, Hz(14_060_000)))]
        );
    }

    // Tests [@ANCHOR: radioctl_elecraft_decode_if]
    #[test]
    fn an_if_answer_decodes_into_every_field_it_carries() {
        let mut e = engine();
        // 11 digits, 5 blanks, sign and 4 digits, RIT=1 XIT=0, blank, "00", then TX=0 MODE
        // VFO=0 SCAN=0 SPLIT=1 b=0 d=0, then "1" and a blank ("IF", p. 14).
        let make = |mode: &str| format!("IF00014060000     +005010 000{mode}001001 ;");
        let good = make("3");
        assert_eq!(good.len(), 2 + 35 + 1, "{good}");
        let inbound = e.feed(good.as_bytes(), T0);
        assert_eq!(
            inbound,
            vec![
                Inbound::Observation(Observation::Frequency(Vfo::A, Hz(14_060_000))),
                Inbound::Observation(Observation::RitOffset(50)),
                Inbound::Observation(Observation::Rit(true)),
                Inbound::Observation(Observation::Xit(false)),
                Inbound::Observation(Observation::Ptt(false)),
                Inbound::Observation(Observation::Mode(Mode::Cw)),
                Inbound::Observation(Observation::Split(true)),
                Inbound::Observation(Observation::TxVfo(Vfo::B)),
            ]
        );
        let broken = good.replace("     +", "    X+");
        assert!(matches!(
            e.feed(broken.as_bytes(), T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        let short = "IF00014060000;";
        assert!(matches!(
            e.feed(short.as_bytes(), T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        let no_mode = make("8");
        assert!(matches!(
            e.feed(no_mode.as_bytes(), T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }
}

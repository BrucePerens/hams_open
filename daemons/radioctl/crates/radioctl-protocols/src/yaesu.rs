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
//! The 8-digit models (FT-450, FT-950, FT-2000, FTDX1200, FTDX3000, FTDX5000, FTDX9000) are
//! selected by `yaesu.frequency_digits = 8` in the description: `FA`, `FB` and P2 of `IF` have 8
//! digits and the `IF` answer is 27 characters with `;` (their guides, cited in each file).
//!
//! The page numbers are in each description fact. The composite `IF;` answer is 28 characters:
//! P1 3 characters (memory channel, or letters on the FTDX10), P2 the 9-digit VFO-A frequency,
//! P3 the clarifier direction and 4-digit offset, P4 RX clarifier, P5 TX clarifier, P6 mode,
//! P7 VFO or memory, P8 tone, P9 two fixed characters, P10 shift.
//!
//! Not implemented: SWR and calibrated meters (`RM` scales are model tables), VFO-B reads on the
//! MAIN/SUB models beyond `FB`, monitor level, the data and narrow modes (A, B, D, E, F),
//! memory channels, and the FTDX101MP (ID 0682 is not in the guide read).

use std::time::Duration;

use radioctl_core::description::{Family, RigDescription, YaesuClarifier};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Level, Receiver, RepeaterShift, ToneMode, Vfo};

use crate::attenuation;
use crate::kenwood::{parse_digits, Token, Tokenizer};

/// Largest clarifier offset, in hertz.
const MAX_OFFSET_HZ: u16 = 9_999;

/// Largest `SM` and `RM` reading (P2 is 000-255 in the CAT manuals).
const MAX_METER: u16 = 255;

/// How a level is written on the wire.
/// The CTCSS tone chart of the FT-991A CAT book (Table 1) in tenths of a hertz, by the tone number
/// `CN00` takes (000 is 67.0 Hz, 049 is 254.1 Hz).
const CTCSS_TENTHS: [u16; 50] = [
    670, 693, 719, 744, 770, 797, 825, 854, 885, 915, 948, 974, 1000, 1035, 1072, 1109, 1148, 1188,
    1230, 1273, 1318, 1365, 1413, 1462, 1514, 1567, 1598, 1622, 1655, 1679, 1713, 1738, 1773, 1799,
    1835, 1862, 1899, 1928, 1966, 1995, 2035, 2065, 2107, 2181, 2257, 2291, 2336, 2418, 2503, 2541,
];

/// The DCS code chart of the FT-991A CAT book (Table 2) as the decimal number of the three octal
/// digits (`023` is 23), by the code number `CN01` takes (000 is 023, 103 is 754).
const DCS_CODES: [u16; 104] = [
    23, 25, 26, 31, 32, 36, 43, 47, 51, 53, 54, 65, 71, 72, 73, 74, 114, 115, 116, 122, 125, 131,
    132, 134, 143, 145, 152, 155, 156, 162, 165, 172, 174, 205, 212, 223, 225, 226, 243, 244, 245,
    246, 251, 252, 255, 261, 263, 265, 266, 271, 274, 306, 311, 315, 325, 331, 332, 343, 346, 351,
    356, 364, 365, 371, 411, 412, 413, 423, 431, 432, 445, 446, 452, 454, 455, 462, 464, 465, 466,
    503, 506, 516, 523, 526, 532, 546, 565, 606, 612, 624, 627, 631, 632, 654, 662, 664, 703, 712,
    723, 731, 732, 734, 743, 754,
];

// [@ANCHOR: radioctl_yaesu_tone_number]
/// The number `CN` takes for a CTCSS tone in tenths of a hertz (`dcs` false) or for a DCS code
/// (`dcs` true), or `None` when the rig's chart has no such entry.
pub(crate) fn tone_number(dcs: bool, value: u16) -> Option<usize> {
    let chart: &[u16] = if dcs { &DCS_CODES } else { &CTCSS_TENTHS };
    chart.iter().position(|v| *v == value)
}

// [@ANCHOR: radioctl_yaesu_tone_value]
/// The CTCSS tone in tenths of a hertz (`dcs` false) or the DCS code (`dcs` true) behind the
/// number `CN` carries, or `None` past the end of the rig's chart.
pub(crate) fn tone_value(dcs: bool, number: usize) -> Option<u16> {
    let chart: &[u16] = if dcs { &DCS_CODES } else { &CTCSS_TENTHS };
    chart.get(number).copied()
}

// [@ANCHOR: radioctl_yaesu_level_wire_struct]
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
        Level::MonitorLevel
        | Level::Attenuator
        | Level::Preamp
        | Level::Dither
        | Level::Preselector
        | Level::Antenna => return None,
    };
    Some(LevelWire {
        name,
        leading_zero,
        min,
        max,
    })
}

// [@ANCHOR: radioctl_yaesu_level_range]
/// The `(min, max)` of a level on the described model, or `None` when the family has no such
/// level command: the description's `[yaesu] levels` range where it gives one, else the default
/// range of `level_wire` (the guides differ per model: the FT-891's `RG` is 000-030, the
/// FTDX5000's `MG` and `PC` are 000-255).
pub fn level_range(desc: &RigDescription, level: Level) -> Option<(u16, u16)> {
    let wire = level_wire(level)?;
    Some(
        desc.yaesu
            .as_ref()
            .and_then(|y| y.level_range(level))
            .unwrap_or((wire.min, wire.max)),
    )
}

// [@ANCHOR: radioctl_yaesu_decode_level]
/// A level answer: command name and digits (`AG0128` is name `AG`, digits `0128`).
fn decode_level(desc: &RigDescription, name: &str, digits: &str) -> Option<Observation> {
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
    // A value above the model's range is a garbled answer, not a level (as the Kenwood and
    // Elecraft engines treat it).
    let (_, max) = level_range(desc, level)?;
    (value <= max).then_some(Observation::Level(level, value))
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
// [@ANCHOR: radioctl_yaesu_yaesu_engine]
#[derive(Debug)]
pub struct YaesuEngine {
    desc: RigDescription,
    tokenizer: Tokenizer,
    /// The last known receive and transmit clarifier switches, for the `CF` form that sets both
    /// in one command (`yaesu.clarifier = "cf-rx-tx"`): switching one keeps the other.
    clar: (bool, bool),
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
            clar: (false, false),
        })
    }

    // [@ANCHOR: radioctl_yaesu_engine_digits]
    /// Digits of the frequency fields (`FA`, `FB`, and P2 of `IF`): 9, or 8 on the older models.
    fn digits(&self) -> usize {
        self.desc
            .yaesu
            .as_ref()
            .map_or(9, |y| usize::from(y.frequency_digits))
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

    // [@ANCHOR: radioctl_yaesu_engine_cf_receive]
    /// True when this model's clarifier is the `CF` command (`yaesu.clarifier = "cf-receive"`), not `RT` and `XT`.
    fn cf_receive(&self) -> bool {
        self.desc
            .yaesu
            .as_ref()
            .is_some_and(|y| y.clarifier == YaesuClarifier::CfReceive)
    }

    /// True when this model's clarifier is the two-switch `CF` command (`yaesu.clarifier = "cf-rx-tx"`, the FT-710).
    fn cf_rx_tx(&self) -> bool {
        self.desc
            .yaesu
            .as_ref()
            .is_some_and(|y| y.clarifier == YaesuClarifier::CfRxTx)
    }

    /// True when this model's clarifier is `RT` with no transmit one (`yaesu.clarifier = "rt-receive"`: FT-450, FT-450D).
    fn rt_receive(&self) -> bool {
        self.desc
            .yaesu
            .as_ref()
            .is_some_and(|y| y.clarifier == YaesuClarifier::RtReceive)
    }

    // [@ANCHOR: radioctl_yaesu_engine_split_st]
    /// True when this model's split is the `ST` command (`yaesu.split = "st"`), not `FT2`/`FT3`.
    fn split_st(&self) -> bool {
        self.desc.yaesu.as_ref().is_some_and(|y| y.split_st)
    }

    // [@ANCHOR: radioctl_yaesu_engine_tx_band]
    /// True when this model's `FT` picks the transmitting band, Main or Sub (`yaesu.tx_band`).
    fn tx_band(&self) -> bool {
        self.desc.yaesu.as_ref().is_some_and(|y| y.tx_band)
    }

    // [@ANCHOR: radioctl_yaesu_engine_split_read]
    /// The read command for the split state and the transmit VFO.
    fn split_read(&self) -> String {
        if self.split_st() { "ST;" } else { "FT;" }.to_string()
    }

    // [@ANCHOR: radioctl_yaesu_engine_has_antenna_ports]
    /// True for the models whose `AN` command has been read: the FTDX101D and FTDX101MP (IDs 681
    /// and 682), three antenna connectors (CAT Operation Reference Manual, `AN`, PDF p. 7).
    fn has_antenna_ports(&self) -> bool {
        self.desc.levels.antenna_ports == 3
            && self
                .desc
                .yaesu
                .as_ref()
                .is_some_and(|y| matches!(y.model_id, 681 | 682))
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
        // `CF` on the FT-710: P1 0, P2 0, P3 0, P4 the receive and P5 the transmit clarifier (0 off,
        // 1 on), P6-P8 000; or P3 1 with a sign and four digits of offset.
        if self.cf_rx_tx() {
            if let Some(body) = text
                .strip_prefix("CF")
                .filter(|b| b.len() == 8 && b.is_ascii())
            {
                let on = |at: usize| match body.get(at..=at) {
                    Some("0") => Some(false),
                    Some("1") => Some(true),
                    _ => None,
                };
                let decoded = match body.get(..3) {
                    Some("000") if body.get(5..) == Some("000") => on(3)
                        .zip(on(4))
                        .map(|(rx, tx)| vec![Observation::Rit(rx), Observation::Xit(tx)]),
                    Some("001") => body
                        .get(3..)
                        .and_then(parse_clarifier)
                        .map(|hz| vec![Observation::RitOffset(hz)]),
                    _ => None,
                };
                return decoded.map_or_else(
                    || vec![Inbound::Unrecognised(format!("{text};").into_bytes())],
                    |all| all.into_iter().map(Inbound::Observation).collect(),
                );
            }
        }
        let observation = match (text.get(..2), text.get(2..)) {
            (Some("FA"), Some(d)) if d.len() == self.digits() => {
                parse_digits(d).map(|hz| Observation::Frequency(Vfo::A, Hz(hz)))
            }
            (Some("FB"), Some(d)) if d.len() == self.digits() => {
                parse_digits(d).map(|hz| Observation::Frequency(Vfo::B, Hz(hz)))
            }
            (Some("MD"), Some(rest)) if rest.len() == 2 && rest.starts_with('0') => rest
                .as_bytes()
                .get(1)
                .and_then(|c| self.desc.code_mode(*c))
                .map(Observation::Mode),
            (Some("TX"), Some(flag @ ("0" | "1" | "2"))) => Some(Observation::Ptt(*flag != *"0")),
            // `FT` answers the band the transmitter follows (P2 0 Main, 1 Sub) on the dual-band models.
            (Some("FT"), Some(flag @ ("0" | "1"))) if self.tx_band() => {
                Some(Observation::TxReceiver(if *flag == *"1" {
                    Receiver::Sub
                } else {
                    Receiver::Main
                }))
            }
            (Some("FT"), Some(flag @ ("0" | "1"))) => Some(Observation::Split(*flag == *"1")),
            // `ST` (P1 0 off, 1 on, 2 on and 5 kHz up): the split flag on the models that have it.
            (Some("ST"), Some("0" | "1" | "2")) if self.split_st() => {
                Some(Observation::Split(text.get(2..) != Some("0")))
            }
            (Some("RT"), Some(flag @ ("0" | "1"))) => Some(Observation::Rit(*flag == *"1")),
            // `CF` P1 0, P2 the receive clarifier (0 off, 1 on), P3 0: the FT-891's form.
            (Some("CF"), Some(rest @ ("000" | "010"))) if self.cf_receive() => {
                Some(Observation::Rit(rest == "010"))
            }
            (Some("XT"), Some(flag @ ("0" | "1"))) => Some(Observation::Xit(*flag == *"1")),
            (Some("SM"), Some(rest)) if rest.len() == 4 && rest.starts_with('0') => rest
                .get(1..)
                .and_then(parse_digits)
                .and_then(|v| u16::try_from(v).ok())
                .filter(|v| *v <= MAX_METER)
                .map(Observation::Signal),
            (Some("RM"), Some(rest)) if rest.len() >= 4 && rest.starts_with('5') => rest
                .get(1..4)
                .and_then(parse_digits)
                .and_then(|v| u16::try_from(v).ok())
                .filter(|v| *v <= MAX_METER)
                .map(Observation::Power),
            (Some("ID"), Some(d)) if d.len() == 4 => parse_digits(d)
                .and_then(|v| u16::try_from(v).ok())
                .map(Observation::ModelId),
            (Some(name @ ("RA" | "PA")), Some(digits)) => self.decode_front_end(name, digits),
            // The repeater shift and tones, `P1` a fixed 0 (the FT-991A and FT-891 books): `OS0` then
            // 0 simplex, 1 plus, 2 minus; `CT0` then 0 off, 1 CTCSS encode and decode, 2 CTCSS
            // encode, 3 or 4 DCS; `CN00` and a tone number, `CN01` and a DCS code number.
            (Some("OS"), Some("00")) => Some(Observation::RepeaterShift(RepeaterShift::Simplex)),
            (Some("OS"), Some("01")) => Some(Observation::RepeaterShift(RepeaterShift::Plus)),
            (Some("OS"), Some("02")) => Some(Observation::RepeaterShift(RepeaterShift::Minus)),
            (Some("CT"), Some("00")) => Some(Observation::ToneMode(ToneMode::Off)),
            (Some("CT"), Some("01")) => Some(Observation::ToneMode(ToneMode::Tsql)),
            (Some("CT"), Some("02")) => Some(Observation::ToneMode(ToneMode::Tone)),
            (Some("CT"), Some("03" | "04")) => Some(Observation::ToneMode(ToneMode::Dcs)),
            (Some("CN"), Some(rest)) if rest.len() == 5 && rest.starts_with("00") => rest
                .get(2..)
                .and_then(parse_digits)
                .and_then(|n| tone_value(false, usize::try_from(n).ok()?))
                .map(Observation::CtcssTone),
            (Some("CN"), Some(rest)) if rest.len() == 5 && rest.starts_with("01") => rest
                .get(2..)
                .and_then(parse_digits)
                .and_then(|n| tone_value(true, usize::try_from(n).ok()?))
                .map(Observation::DcsCode),
            // `ANp1p2p4;`: main band (p1 0), connector 1 to 3, p4 a fixed 0.
            (Some("AN"), Some(rest)) if self.has_antenna_ports() => match rest.as_bytes() {
                [b'0', port @ b'1'..=b'3', b'0'] => {
                    Some(Observation::Level(Level::Antenna, u16::from(port - b'0')))
                }
                _ => None,
            },
            (Some(name), Some(digits)) => decode_level(&self.desc, name, digits),
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
    /// The observations in an `IF` answer without `IF` and `;`: 25 characters with a 9-digit
    /// frequency, 24 with an 8-digit one.
    fn decode_if(&self, body: &str) -> Option<Vec<Inbound>> {
        let digits = self.digits();
        if body.len() != digits + 16 || !body.is_ascii() {
            return None;
        }
        let frequency = parse_digits(body.get(3..3 + digits)?)?;
        let offset = parse_clarifier(body.get(3 + digits..8 + digits)?)?;
        let flag = |at: usize| match body.get(at..at + 1) {
            Some("0") => Some(false),
            Some("1") => Some(true),
            _ => None,
        };
        let (rit, xit) = (flag(8 + digits)?, flag(9 + digits)?);
        let mode = body
            .get(10 + digits..11 + digits)?
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
                let width = self.digits();
                if hz.0 >= 10u64.pow(u32::try_from(width).unwrap_or(9)) {
                    return Err(Self::unsupported(
                        "frequency does not fit the model's frequency field",
                    ));
                }
                let name = if *vfo == Vfo::A { "FA" } else { "FB" };
                Self::command(format!("{name}{:0width$};", hz.0), none)
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
            Op::GetSplit => Self::command(self.split_read(), observe(Field::Split)),
            Op::GetTxVfo => Self::command(self.split_read(), observe(Field::TxVfo)),
            Op::GetTxReceiver if self.tx_band() => {
                Self::command("FT;".into(), observe(Field::TxReceiver))
            }
            Op::SetTxReceiver(rx) if self.tx_band() => Self::command(
                format!("FT{};", if *rx == Receiver::Sub { 3 } else { 2 }),
                none,
            ),
            Op::SelectReceiver(_)
            | Op::GetReceiver
            | Op::SetTxReceiver(_)
            | Op::GetTxReceiver
            | Op::SetDualWatch(_)
            | Op::GetDualWatch => {
                return Err(Self::unsupported(
                    "not a Main/Sub command on this Yaesu model",
                ))
            }
            Op::SetSplit(on) if self.split_st() => {
                Self::command(format!("ST{};", u8::from(*on)), none)
            }
            Op::SetSplit(on) => Self::command(format!("FT{};", if *on { 3 } else { 2 }), none),
            // FT-710: `CF000;` reads both switches, `CF000` + RX + TX + `000;` sets them, and the
            // offset is `CF001` + sign + four digits; the other switch is re-sent as last known.
            Op::GetRit | Op::GetXit if self.cf_rx_tx() => {
                let field = if matches!(op, Op::GetRit) {
                    Field::Rit
                } else {
                    Field::Xit
                };
                Self::command("CF000;".into(), observe(field))
            }
            Op::SetRit(on) if self.cf_rx_tx() => {
                self.clar.0 = *on;
                Self::command(
                    format!("CF000{}{}000;", u8::from(*on), u8::from(self.clar.1)),
                    none,
                )
            }
            Op::SetXit(on) if self.cf_rx_tx() => {
                self.clar.1 = *on;
                Self::command(
                    format!("CF000{}{}000;", u8::from(self.clar.0), u8::from(*on)),
                    none,
                )
            }
            Op::SetRitOffset(hz) if self.cf_rx_tx() => {
                if hz.unsigned_abs() > MAX_OFFSET_HZ {
                    return Err(Self::unsupported("clarifier offset beyond 9999 Hz"));
                }
                let sign = if *hz < 0 { '-' } else { '+' };
                Self::command(format!("CF001{sign}{:04};", hz.unsigned_abs()), none)
            }
            Op::GetRit if self.cf_receive() => Self::command("CF0;".into(), observe(Field::Rit)),
            Op::SetRit(on) if self.cf_receive() => {
                Self::command(format!("CF0{}0;", u8::from(*on)), none)
            }
            Op::GetXit | Op::SetXit(_) if self.cf_receive() || self.rt_receive() => {
                return Err(Self::unsupported(
                    "this model has no transmit clarifier command (the clarifier is receive only)",
                ))
            }
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
            Op::GetLevel(Level::Antenna) if self.has_antenna_ports() => {
                Self::command("AN0;".to_string(), observe(Field::Level(Level::Antenna)))
            }
            Op::SetLevel(Level::Antenna, port @ 1..=3) if self.has_antenna_ports() => {
                Self::command(format!("AN0{port};"), none)
            }
            Op::GetLevel(Level::Antenna) | Op::SetLevel(Level::Antenna, _) => {
                return Err(Self::unsupported("no antenna port list on this model"));
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
                let (min, max) = level_range(&self.desc, *level)
                    .ok_or_else(|| Self::unsupported("level not implemented for newcat"))?;
                if *value < min || *value > max {
                    return Err(Self::unsupported("level outside the range of the guides"));
                }
                let digit = if wire.leading_zero { "0" } else { "" };
                Self::command(format!("{}{digit}{value:03};", wire.name), none)
            }
            Op::GetModelId => Self::command("ID;".into(), observe(Field::ModelId)),
            Op::GetRepeaterShift => Self::command("OS0;".into(), observe(Field::RepeaterShift)),
            Op::SetRepeaterShift(shift) => Self::command(
                format!(
                    "OS0{};",
                    match shift {
                        RepeaterShift::Simplex => 0,
                        RepeaterShift::Plus => 1,
                        RepeaterShift::Minus => 2,
                    }
                ),
                none,
            ),
            Op::GetToneMode => Self::command("CT0;".into(), observe(Field::ToneMode)),
            Op::SetToneMode(mode) => Self::command(
                format!(
                    "CT0{};",
                    match mode {
                        ToneMode::Off => 0,
                        ToneMode::Tsql => 1,
                        ToneMode::Tone => 2,
                        ToneMode::Dcs => 3,
                    }
                ),
                none,
            ),
            Op::GetCtcssTone => Self::command("CN00;".into(), observe(Field::CtcssTone)),
            Op::SetCtcssTone(tenths) => match tone_number(false, *tenths) {
                Some(n) => Self::command(format!("CN00{n:03};"), none),
                None => return Err(Self::unsupported("the rig's CTCSS chart has no such tone")),
            },
            Op::GetDcsCode => Self::command("CN01;".into(), observe(Field::DcsCode)),
            Op::SetDcsCode(code) => match tone_number(true, *code) {
                Some(n) => Self::command(format!("CN01{n:03};"), none),
                None => return Err(Self::unsupported("the rig's DCS chart has no such code")),
            },
            // The repeater offset is a menu item on these models, not a CAT command: refused
            // by the catch-all below with the other ops the guides do not have.
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
        let tokens = self.tokenizer.feed(bytes, now);
        let mut inbound = Vec::new();
        for token in tokens {
            match token {
                Token::Command(text) => inbound.extend(self.decode(&text)),
                Token::Junk(junk) => inbound.push(Inbound::Unrecognised(junk)),
            }
        }
        for item in &inbound {
            match item {
                Inbound::Observation(Observation::Rit(on)) => self.clar.0 = *on,
                Inbound::Observation(Observation::Xit(on)) => self.clar.1 = *on,
                _ => {}
            }
        }
        inbound
    }
}

#[cfg(test)]
mod tests {
    // Tests [@ANCHOR: radioctl_yaesu_level_wire_struct]
    // Tests [@ANCHOR: radioctl_yaesu_yaesu_engine]
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

    /// The FT-991A description with the 8-digit frequency field of the FT-950 family.
    fn eight_digit() -> YaesuEngine {
        let mut desc = shipped("yaesu-ft-991a");
        desc.yaesu.as_mut().expect("yaesu block").frequency_digits = 8;
        YaesuEngine::new(desc).expect("engine")
    }

    // Tests [@ANCHOR: radioctl_yaesu_engine_digits]
    // Tests [@ANCHOR: radioctl_yaesu_engine_encode]
    // Tests [@ANCHOR: radioctl_yaesu_level_range]
    // Tests [@ANCHOR: radioctl_yaesu_level_wire]
    #[test]
    fn a_description_may_replace_the_default_level_ranges() {
        let b = |e: &mut YaesuEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        // FT-891: RG is 000-030, the other levels keep the guide's ranges.
        let desc = shipped("yaesu-ft-891");
        assert_eq!(level_range(&desc, Level::RfGain), Some((0, 30)));
        assert_eq!(level_range(&desc, Level::AfGain), Some((0, 255)));
        assert_eq!(level_range(&desc, Level::MonitorLevel), None);
        let mut e = YaesuEngine::new(desc).expect("engine");
        assert_eq!(b(&mut e, Op::SetLevel(Level::RfGain, 30)), b"RG0030;");
        assert!(e.encode(&Op::SetLevel(Level::RfGain, 31)).is_err());
        // FTDX5000: MG and PC are 000-255 (the default is 0-100 and 5-100).
        let mut x = YaesuEngine::new(shipped("yaesu-ftdx-5000")).expect("engine");
        assert_eq!(b(&mut x, Op::SetLevel(Level::MicGain, 255)), b"MG255;");
        assert_eq!(b(&mut x, Op::SetLevel(Level::RfPower, 0)), b"PC000;");
        assert!(x.encode(&Op::SetLevel(Level::MicGain, 256)).is_err());
        // FT-450D: MG is 000-255, PC keeps 005-100.
        let mut d = YaesuEngine::new(shipped("yaesu-ft-450d")).expect("engine");
        assert_eq!(b(&mut d, Op::SetLevel(Level::MicGain, 171)), b"MG171;");
        assert!(d.encode(&Op::SetLevel(Level::RfPower, 4)).is_err());
        // A model without a stated range keeps the default.
        assert_eq!(
            level_range(&shipped("yaesu-ft-991a"), Level::MicGain),
            Some((0, 100))
        );
    }

    // Tests [@ANCHOR: radioctl_yaesu_engine_has_antenna_ports]
    // Tests [@ANCHOR: radioctl_yaesu_engine_decode]
    #[test]
    fn the_ftdx101_antenna_command_follows_the_cat_manual() {
        for id in ["yaesu-ftdx101d", "yaesu-ftdx-101mp"] {
            let mut e = YaesuEngine::new(shipped(id)).expect("engine");
            assert_eq!(
                e.encode(&Op::GetLevel(Level::Antenna)).unwrap().bytes,
                b"AN0;"
            );
            assert_eq!(
                e.encode(&Op::SetLevel(Level::Antenna, 3)).unwrap().bytes,
                b"AN03;"
            );
            assert!(e.encode(&Op::SetLevel(Level::Antenna, 0)).is_err());
            assert!(e.encode(&Op::SetLevel(Level::Antenna, 4)).is_err());
            assert_eq!(
                e.feed(b"AN020;", Duration::ZERO),
                vec![Inbound::Observation(Observation::Level(Level::Antenna, 2))]
            );
            // The sub band's answer, and a connector outside 1 to 3, are not this reading.
            assert!(matches!(
                e.feed(b"AN110;", Duration::ZERO)[0],
                Inbound::Unrecognised(_)
            ));
            assert!(matches!(
                e.feed(b"AN040;", Duration::ZERO)[0],
                Inbound::Unrecognised(_)
            ));
        }
        // No other Yaesu has had its AN command read: refused.
        let mut other = YaesuEngine::new(shipped("yaesu-ft-950")).expect("engine");
        assert!(other.encode(&Op::GetLevel(Level::Antenna)).is_err());
    }

    // Tests [@ANCHOR: radioctl_yaesu_level_range]
    // Tests [@ANCHOR: radioctl_yaesu_level_wire]
    #[test]
    fn the_ft_950_claims_levels_with_mg_000_255_and_pc_005_100() {
        // FT-950 book PDF p. 12 (MG 000-255), p. 14 (PC 005-100).
        let desc = shipped("yaesu-ft-950");
        assert!(desc
            .capabilities
            .has(radioctl_core::capability::Capability::Levels));
        assert_eq!(level_range(&desc, Level::MicGain), Some((0, 255)));
        assert_eq!(level_range(&desc, Level::RfPower), Some((5, 100)));
        let mut e = YaesuEngine::new(desc).expect("engine");
        let b = |e: &mut YaesuEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        assert_eq!(b(&mut e, Op::SetLevel(Level::MicGain, 255)), b"MG255;");
        assert!(e.encode(&Op::SetLevel(Level::MicGain, 256)).is_err());
        assert!(e.encode(&Op::SetLevel(Level::RfPower, 4)).is_err());
    }

    // Tests [@ANCHOR: radioctl_yaesu_level_range]
    // Tests [@ANCHOR: radioctl_yaesu_level_wire]
    #[test]
    fn the_ft_2000_claims_levels_with_mg_and_pc_000_255() {
        // FT-2000 book PDF p. 12 (MG 000-255), p. 14 (PC 000-255).
        let desc = shipped("yaesu-ft-2000");
        assert!(desc
            .capabilities
            .has(radioctl_core::capability::Capability::Levels));
        assert_eq!(level_range(&desc, Level::MicGain), Some((0, 255)));
        assert_eq!(level_range(&desc, Level::RfPower), Some((0, 255)));
        let mut e = YaesuEngine::new(desc).expect("engine");
        let b = |e: &mut YaesuEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        assert_eq!(b(&mut e, Op::SetLevel(Level::MicGain, 255)), b"MG255;");
        assert_eq!(b(&mut e, Op::SetLevel(Level::RfPower, 0)), b"PC000;");
        assert!(e.encode(&Op::SetLevel(Level::RfPower, 256)).is_err());
    }

    // Tests [@ANCHOR: radioctl_yaesu_engine_feed]
    // Tests [@ANCHOR: radioctl_yaesu_engine_decode]
    // Tests [@ANCHOR: radioctl_yaesu_engine_decode_if]
    #[test]
    fn an_eight_digit_model_writes_reads_and_parses_eight_digit_frequencies() {
        let mut nine = YaesuEngine::new(shipped("yaesu-ft-991a")).expect("engine");
        let mut e = eight_digit();
        assert_eq!(nine.digits(), 9);
        assert_eq!(e.digits(), 8);
        let set = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(14_074_000)))
            .expect("encode");
        assert_eq!(set.bytes, b"FA14074000;");
        let set_b = e
            .encode(&Op::SetFrequency(Vfo::B, Hz(7_030_000)))
            .expect("encode");
        assert_eq!(set_b.bytes, b"FB07030000;");
        assert_eq!(
            e.encode(&Op::SetFrequency(Vfo::A, Hz(100_000_000)))
                .expect_err("nine digits do not fit")
                .kind,
            ErrorKind::Unsupported
        );
        assert!(nine
            .encode(&Op::SetFrequency(Vfo::A, Hz(100_000_000)))
            .is_ok());
        assert_eq!(
            e.feed(b"FA14074000;", T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(14_074_000)
            ))]
        );
        assert!(matches!(
            e.feed(b"FA014074000;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        // P1 channel, P2 8-digit frequency, P3 offset, RX CLAR, TX CLAR, mode, VFO, tone, P9, P10.
        let answer = "IF00114074000+005001200000;";
        assert_eq!(answer.len(), 28 - 1);
        assert_eq!(
            e.feed(answer.as_bytes(), T0),
            vec![
                Inbound::Observation(Observation::Frequency(Vfo::A, Hz(14_074_000))),
                Inbound::Observation(Observation::Mode(Mode::Usb)),
                Inbound::Observation(Observation::Rit(false)),
                Inbound::Observation(Observation::Xit(true)),
                Inbound::Observation(Observation::RitOffset(50)),
            ]
        );
        // The 9-digit answer is the wrong length for an 8-digit model, and the reverse.
        assert!(matches!(
            e.feed(b"IF001014074000+005001200000;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(matches!(
            nine.feed(answer.as_bytes(), T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
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
        let mut x = YaesuEngine::new(shipped("yaesu-ft-450")).expect("engine");
        assert!(
            x.encode(&Op::SetSplit(true)).is_err(),
            "the FT-450 file does not claim split (its ST is the tuning step)"
        );
    }

    // Tests [@ANCHOR: radioctl_yaesu_engine_cf_receive]
    #[test]
    fn the_ft_891_switches_its_clarifier_with_cf_and_has_no_transmit_one() {
        let mut e = YaesuEngine::new(shipped("yaesu-ft-891")).expect("engine");
        let b = |e: &mut YaesuEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        assert_eq!(b(&mut e, Op::SetRit(true)), b"CF010;");
        assert_eq!(b(&mut e, Op::SetRit(false)), b"CF000;");
        assert_eq!(b(&mut e, Op::GetRit), b"CF0;");
        assert_eq!(b(&mut e, Op::GetRitOffset), b"IF;");
        assert_eq!(b(&mut e, Op::SetRitOffset(150)), b"RC;RU0150;");
        assert!(e.encode(&Op::SetXit(true)).is_err());
        assert!(e.encode(&Op::GetXit).is_err());
        assert_eq!(
            e.feed(b"CF010;CF000;", T0),
            vec![
                Inbound::Observation(Observation::Rit(true)),
                Inbound::Observation(Observation::Rit(false)),
            ]
        );
        assert!(matches!(
            e.feed(b"CF020;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        // The FT-991A keeps RT and XT and does not read CF.
        let mut e = YaesuEngine::new(shipped("yaesu-ft-991a")).expect("engine");
        assert_eq!(b(&mut e, Op::SetRit(true)), b"RT1;");
        assert!(matches!(
            e.feed(b"CF010;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }

    // Tests [@ANCHOR: radioctl_yaesu_engine_cf_receive]
    #[test]
    fn the_ft_710_sets_both_clarifier_switches_and_the_offset_with_cf() {
        let mut e = YaesuEngine::new(shipped("yaesu-ft-710")).expect("engine");
        let b = |e: &mut YaesuEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        assert_eq!(b(&mut e, Op::GetRit), b"CF000;");
        assert_eq!(b(&mut e, Op::GetXit), b"CF000;");
        assert_eq!(b(&mut e, Op::GetRitOffset), b"IF;");
        // Each switch keeps the other as last set or read.
        assert_eq!(b(&mut e, Op::SetRit(true)), b"CF00010000;");
        assert_eq!(b(&mut e, Op::SetXit(true)), b"CF00011000;");
        assert_eq!(b(&mut e, Op::SetRit(false)), b"CF00001000;");
        assert_eq!(b(&mut e, Op::SetRitOffset(150)), b"CF001+0150;");
        assert_eq!(b(&mut e, Op::SetRitOffset(-1200)), b"CF001-1200;");
        assert_eq!(b(&mut e, Op::SetRitOffset(0)), b"CF001+0000;");
        assert!(e.encode(&Op::SetRitOffset(10_000)).is_err());
        // An answer carries both switches and refreshes the remembered state.
        assert_eq!(
            e.feed(b"CF00011000;", T0),
            vec![
                Inbound::Observation(Observation::Rit(true)),
                Inbound::Observation(Observation::Xit(true)),
            ]
        );
        assert_eq!(b(&mut e, Op::SetRit(false)), b"CF00001000;");
        assert_eq!(
            e.feed(b"CF001-0150;", T0),
            vec![Inbound::Observation(Observation::RitOffset(-150))]
        );
        for bad in [
            &b"CF00012000;"[..],
            b"CF00010100;",
            b"CF001*0150;",
            b"CF0001;",
        ] {
            assert!(
                matches!(e.feed(bad, T0).as_slice(), [Inbound::Unrecognised(_)]),
                "{bad:?}"
            );
        }
    }

    // Tests [@ANCHOR: radioctl_yaesu_engine_cf_receive]
    #[test]
    fn the_ft_450_and_450d_clarifier_is_rt_with_no_transmit_one() {
        for id in ["yaesu-ft-450", "yaesu-ft-450d"] {
            let mut e = YaesuEngine::new(shipped(id)).expect("engine");
            let b = |e: &mut YaesuEngine, op: Op| e.encode(&op).expect("encodes").bytes;
            assert_eq!(b(&mut e, Op::SetRit(true)), b"RT1;", "{id}");
            assert_eq!(b(&mut e, Op::GetRit), b"RT;");
            assert_eq!(b(&mut e, Op::SetRitOffset(-150)), b"RC;RD0150;");
            assert!(e.encode(&Op::SetXit(true)).is_err());
            assert!(e.encode(&Op::GetXit).is_err());
        }
    }

    // Tests [@ANCHOR: radioctl_yaesu_engine_split_read]
    #[test]
    fn split_is_read_with_ft_on_the_models_without_st_and_with_st_on_the_others() {
        let b = |e: &mut YaesuEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        let mut old = YaesuEngine::new(shipped("yaesu-ft-991a")).expect("engine");
        assert!(!old.split_st());
        assert_eq!(b(&mut old, Op::GetSplit), b"FT;");
        assert_eq!(b(&mut old, Op::GetTxVfo), b"FT;");
        let mut st = YaesuEngine::new(shipped("yaesu-ft-891")).expect("engine");
        assert_eq!(b(&mut st, Op::GetSplit), b"ST;");
    }

    // Tests [@ANCHOR: radioctl_yaesu_engine_split_st]
    #[test]
    fn split_is_the_st_command_on_the_models_whose_book_has_it() {
        for id in [
            "yaesu-ft-891",
            "yaesu-ft-710",
            "yaesu-ftdx10",
            "yaesu-ftdx101d",
            "yaesu-ftdx-101mp",
        ] {
            let mut e = YaesuEngine::new(shipped(id)).expect("engine");
            let b = |e: &mut YaesuEngine, op: Op| e.encode(&op).expect("encodes").bytes;
            assert_eq!(b(&mut e, Op::SetSplit(true)), b"ST1;", "{id}");
            assert_eq!(b(&mut e, Op::SetSplit(false)), b"ST0;", "{id}");
            assert_eq!(b(&mut e, Op::GetSplit), b"ST;", "{id}");
            assert_eq!(b(&mut e, Op::GetTxVfo), b"ST;", "{id}");
            // P1 2 is split on with the transmitter 5 kHz up.
            for (answer, on) in [(&b"ST0;"[..], false), (b"ST1;", true), (b"ST2;", true)] {
                assert_eq!(
                    e.feed(answer, T0),
                    vec![
                        Inbound::Observation(Observation::Split(on)),
                        Inbound::Observation(Observation::TxVfo(if on { Vfo::B } else { Vfo::A })),
                    ],
                    "{id}"
                );
            }
            assert!(matches!(
                e.feed(b"ST3;", T0).as_slice(),
                [Inbound::Unrecognised(_)]
            ));
        }
        // The FT-991A keeps FT2/FT3 and does not read ST.
        let mut e = YaesuEngine::new(shipped("yaesu-ft-991a")).expect("engine");
        assert!(matches!(
            e.feed(b"ST1;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
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

    // Tests [@ANCHOR: radioctl_yaesu_decode_level]
    #[test]
    fn a_level_answer_above_the_models_range_is_unrecognised() {
        let mut e = YaesuEngine::new(shipped("yaesu-ft-991a")).expect("engine");
        for bad in [&b"AG0999;"[..], b"RG0256;", b"MG101;", b"KS061;", b"PC999;"] {
            assert!(
                matches!(e.feed(bad, T0).as_slice(), [Inbound::Unrecognised(_)]),
                "{bad:?}"
            );
        }
    }

    // Tests [@ANCHOR: radioctl_yaesu_engine_decode]
    #[test]
    fn a_meter_answer_above_255_is_unrecognised() {
        let mut e = YaesuEngine::new(shipped("yaesu-ft-991a")).expect("engine");
        for bad in [&b"SM0256;"[..], b"SM0999;", b"RM5256000;"] {
            assert!(
                matches!(e.feed(bad, T0).as_slice(), [Inbound::Unrecognised(_)]),
                "{bad:?}"
            );
        }
        assert_eq!(
            e.feed(b"SM0255;", T0),
            vec![Inbound::Observation(Observation::Signal(255))]
        );
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

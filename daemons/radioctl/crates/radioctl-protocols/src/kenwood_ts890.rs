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
//! **Bandscope** (guide "BS0".."BSO", "DD0", "DD2"/"DD3", "##DD2"/"##DD3"; read from the August 2018
//! edition, `ts890_pc_command_e.pdf`, sha256 47aaa103...): with the `scope` capability the core scope
//! operations map to `BS3` (axis: 0 center, 1 fixed, 2 auto scroll, read as [`ScopeAxis::ScrollFixed`]),
//! `BS4` (span, full width: 5, 10, 25, 50, 100, 200, 500 kHz), `BSC` (reference level, 000 is -20 dB,
//! 0.5 dB a step) and `DD0` (waveform output: 0 off, and our choice `2`, LAN medium cycle, on). The LAN
//! sends the bandscope as `##DD2` + 640 two-digit hexadecimal samples (00 strongest to 8C weakest) +
//! `;`; the engine flips them to the core's "0 is the bottom" and publishes a [`ScopeFrame`] for the
//! main receiver. The frame carries no frequency: the engine keeps the last `FA`/`FB`, `FR`, `BS3`
//! and `BS4` it saw (turning output on reads them first) and gives the axis only in center mode; in
//! fixed or auto-scroll mode, or before those are known, the sweep is counted
//! ([`Ts890Engine::scope_frames_dropped`]) and not published. The guide sends nothing unless the
//! rig's auto information is on (`transceive`). `##DD3` (the filter scope, 213 samples 00 to 32) is
//! checked and dropped: the core has no filter-scope frame. `BSO` (expanded analysis range, whose
//! output range exceeds the displayed span) is assumed off. Not mapped to an operation yet: `BS0`
//! (display on/off), `BS1`, `BS5` and `BSM` (fixed-mode ranges and edges), `BS6` to `BS9`, `BSA`, `BSB`
//! and `BSD` to `BSO`.
//!
//! Not implemented: CW text, memories, the LAN session beyond the two login frames in [`lan_connect_request`] and
//! [`lan_login`] (the session that sends them, with the owner's account, and keeps the line alive is
//! [`crate::kenwood_ts890_lan::Ts890LanFraming`]), and any of the many menu commands. Never run against a radio.

use std::time::Duration;

use radioctl_core::description::{Family, KenwoodDialect, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::scope::{ScopeAxis, ScopeFrame};
use radioctl_core::units::{Hz, Level, Vfo};

use crate::attenuation;
use crate::kenwood::{parse_digits, Token, Tokenizer};

/// Largest frequency the 11-digit field holds.
const MAX_HZ: u64 = 99_999_999_999;
/// Largest RIT/XIT offset, in hertz (`RF` carries four digits).
const MAX_OFFSET_HZ: u16 = 9_999;

/// The bandscope span choices of `BS4` in hertz, indexed by the guide's code 0 to 6.
const SCOPE_SPANS_HZ: [u64; 7] = [5_000, 10_000, 25_000, 50_000, 100_000, 200_000, 500_000];
/// Spectrum samples in one `##DD2` frame.
const SCOPE_SAMPLES: usize = 640;
/// Samples in one `##DD3` frame.
const FILTER_SCOPE_SAMPLES: usize = 213;
/// The weakest bandscope sample (-100 dB); 00 is the strongest.
pub const SCOPE_SAMPLE_MAX: u8 = 0x8C;
/// The weakest filter-scope sample (-50 dB).
const FILTER_SCOPE_SAMPLE_MAX: u8 = 0x32;
/// The longest command or frame the engine buffers (`##DD2` is 1286 bytes).
const MAX_FRAME: usize = 2048;
/// The `DD0` value the engine sets to turn the waveform output on: LAN, medium cycle (our choice).
const SCOPE_OUTPUT_ON: u8 = 2;

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
    /// The scope output was asked for; sent again after the link is restored.
    scope_output: bool,
    /// What the frame edges are computed from.
    scope: ScopeContext,
}

/// What a `##DD2` frame (which has no frequency) is placed with.
#[derive(Debug, Default)]
struct ScopeContext {
    freq_a: Option<Hz>,
    freq_b: Option<Hz>,
    rx_vfo: Option<Vfo>,
    axis: Option<ScopeAxis>,
    span: Option<Hz>,
    dropped: u64,
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
            tokenizer: Tokenizer::with_max_pending(MAX_FRAME),
            transceive: false,
            scope_output: false,
            scope: ScopeContext::default(),
        })
    }

    // [@ANCHOR: radioctl_ts890_engine_scope_frames_dropped]
    /// Bandscope sweeps received and not published (not center mode, or the frequency, span or
    /// axis not yet known, or a malformed frame).
    pub fn scope_frames_dropped(&self) -> u64 {
        self.scope.dropped
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
            (Some("BS"), Some(d)) => decode_scope(d),
            (Some("DD"), Some(d)) if d.len() == 2 && d.starts_with('0') => match d.get(1..) {
                Some("0") => Some(Observation::ScopeOutput(false)),
                Some("1" | "2" | "3" | "4" | "5") => Some(Observation::ScopeOutput(true)),
                _ => None,
            },
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

// [@ANCHOR: radioctl_ts890_scope_axis_code]
/// The `BS3` digit of an axis mode; auto scroll is the only scrolling mode the rig has, so the core's
/// scrolling-centred mode has no digit.
fn scope_axis_code(axis: ScopeAxis) -> Option<u8> {
    match axis {
        ScopeAxis::Center => Some(0),
        ScopeAxis::Fixed => Some(1),
        ScopeAxis::ScrollFixed => Some(2),
        ScopeAxis::ScrollCenter => None,
    }
}

// [@ANCHOR: radioctl_ts890_scope_axis_of]
/// The axis mode a `BS3` digit names.
fn scope_axis_of(digit: &str) -> Option<ScopeAxis> {
    match digit {
        "0" => Some(ScopeAxis::Center),
        "1" => Some(ScopeAxis::Fixed),
        "2" => Some(ScopeAxis::ScrollFixed),
        _ => None,
    }
}

// [@ANCHOR: radioctl_ts890_scope_span_code]
/// The `BS4` digit of a span in hertz, if the rig offers it.
fn scope_span_code(span: Hz) -> Option<usize> {
    SCOPE_SPANS_HZ.iter().position(|hz| *hz == span.0)
}

// [@ANCHOR: radioctl_ts890_scope_reference_code]
/// The three `BSC` digits of a reference level in hundredths of a dB: 000 is -20 dB, 040 is 0 dB and
/// 060 is +10 dB, half a decibel a step. `None` for a level that is not a step or is out of range.
fn scope_reference_code(hundredths: i16) -> Option<u16> {
    let offset = i32::from(hundredths) + 2000;
    if offset < 0 || offset % 50 != 0 || offset / 50 > 60 {
        return None;
    }
    u16::try_from(offset / 50).ok()
}

// [@ANCHOR: radioctl_ts890_decode_scope]
/// A `BS3`, `BS4` or `BSC` answer after the command name `BS`; other `BS` commands are not mapped.
fn decode_scope(rest: &str) -> Option<Observation> {
    let (sub, value) = (rest.get(..1)?, rest.get(1..)?);
    match sub {
        "3" => scope_axis_of(value).map(|axis| Observation::ScopeAxis(0, axis)),
        "4" => {
            let code = usize::try_from(parse_digits(value).filter(|_| value.len() == 1)?).ok()?;
            SCOPE_SPANS_HZ
                .get(code)
                .map(|hz| Observation::ScopeSpan(0, Hz(*hz)))
        }
        "C" => {
            let step = i16::try_from(parse_digits(value).filter(|_| value.len() == 3)?).ok()?;
            (step <= 60).then_some(Observation::ScopeReference(0, step * 50 - 2000))
        }
        _ => None,
    }
}

// [@ANCHOR: radioctl_ts890_scope_samples]
/// The samples of a `##DD2` or `##DD3` body: `count` two-digit hexadecimal values, none above `max`.
/// `None` for a wrong length, a non-hexadecimal digit or a value past the guide's range.
fn scope_samples(body: &str, count: usize, max: u8) -> Option<Vec<u8>> {
    if body.len() != count * 2 || !body.is_ascii() {
        return None;
    }
    body.as_bytes()
        .chunks(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(text, 16).ok().filter(|v| *v <= max)
        })
        .collect()
}

impl Ts890Engine {
    // [@ANCHOR: radioctl_ts890_engine_scope_frame]
    /// A `##DD2` body as a frame placed from what the engine last saw, or `None` (and a count) when
    /// it is malformed, not in center mode, or the frequency, span or axis is not known.
    fn scope_frame(&mut self, body: &str) -> Option<ScopeFrame> {
        let samples = scope_samples(body, SCOPE_SAMPLES, SCOPE_SAMPLE_MAX);
        let centre = match self.scope.rx_vfo {
            Some(Vfo::B) => self.scope.freq_b,
            _ => self.scope.freq_a,
        };
        let placed = match (samples, self.scope.axis, self.scope.span, centre) {
            (Some(samples), Some(ScopeAxis::Center), Some(span), Some(centre)) => {
                Some((samples, span, centre))
            }
            _ => None,
        };
        let Some((samples, span, centre)) = placed else {
            self.scope.dropped += 1;
            return None;
        };
        let half = span.0 / 2;
        Some(ScopeFrame {
            seq: 0,
            receiver: 0,
            axis: ScopeAxis::Center,
            low: Hz(centre.0.saturating_sub(half)),
            high: Hz(centre.0.saturating_add(half)),
            out_of_range: false,
            amplitude_max: u16::from(SCOPE_SAMPLE_MAX),
            samples: samples.into_iter().map(|v| SCOPE_SAMPLE_MAX - v).collect(),
        })
    }

    // [@ANCHOR: radioctl_ts890_engine_note]
    /// Keeps what the frame edges are computed from.
    fn note(&mut self, inbound: &Inbound) {
        let Inbound::Observation(observation) = inbound else {
            return;
        };
        match *observation {
            Observation::Frequency(Vfo::A, hz) => self.scope.freq_a = Some(hz),
            Observation::Frequency(Vfo::B, hz) => self.scope.freq_b = Some(hz),
            Observation::RxVfo(vfo) => self.scope.rx_vfo = Some(vfo),
            Observation::ScopeAxis(0, axis) => self.scope.axis = Some(axis),
            Observation::ScopeSpan(0, span) => self.scope.span = Some(span),
            _ => {}
        }
    }

    // [@ANCHOR: radioctl_ts890_engine_scope_start]
    /// The commands that turn the waveform output on: first read what the frames are placed from
    /// (`FA`, `FB`, `FR`, `BS3`, `BS4`; the answers update the engine's copies as they arrive), then
    /// `DD0`. Nothing here transmits.
    fn scope_start() -> String {
        format!("FA;FB;FR;BS3;BS4;DD0{SCOPE_OUTPUT_ON};")
    }
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
                // What the next bandscope frame is placed from.
                self.note(&Inbound::Observation(Observation::Frequency(*vfo, *hz)));
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
            Op::SetScopeOutput(on) => {
                self.scope_output = *on;
                let text = if *on {
                    Self::scope_start()
                } else {
                    "DD00;".to_string()
                };
                Self::command(text, none)
            }
            Op::GetScopeOutput => Self::command("DD0;".into(), observe(Field::ScopeOutput)),
            Op::GetScopeAxis(0) => Self::command("BS3;".into(), observe(Field::ScopeAxis(0))),
            Op::SetScopeAxis(0, axis) => {
                let code = scope_axis_code(*axis)
                    .ok_or_else(|| Self::unsupported("the TS-890S scrolls in one mode only"))?;
                self.scope.axis = Some(*axis);
                Self::command(format!("BS3{code};"), none)
            }
            Op::GetScopeSpan(0) => Self::command("BS4;".into(), observe(Field::ScopeSpan(0))),
            Op::SetScopeSpan(0, span) => {
                let code = scope_span_code(*span)
                    .ok_or_else(|| Self::unsupported("span not offered by the TS-890S"))?;
                self.scope.span = Some(*span);
                Self::command(format!("BS4{code};"), none)
            }
            Op::GetScopeReference(0) => {
                Self::command("BSC;".into(), observe(Field::ScopeReference(0)))
            }
            Op::SetScopeReference(0, hundredths) => {
                let step = scope_reference_code(*hundredths).ok_or_else(|| {
                    Self::unsupported(
                        "reference level is not a half-decibel step from -20 to +10 dB",
                    )
                })?;
                Self::command(format!("BSC{step:03};"), none)
            }
            _ => return Err(Self::unsupported("not implemented for the TS-890S engine")),
        };
        Ok(request)
    }

    // [@ANCHOR: radioctl_ts890_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        for token in self.tokenizer.feed(bytes, now) {
            match token {
                Token::Command(text) => {
                    if let Some(body) = text.strip_prefix("##DD2") {
                        out.extend(self.scope_frame(body).map(Inbound::ScopeFrame));
                    } else if let Some(body) = text.strip_prefix("##DD3") {
                        // The filter scope has no core frame; checked, counted if malformed.
                        if scope_samples(body, FILTER_SCOPE_SAMPLES, FILTER_SCOPE_SAMPLE_MAX)
                            .is_none()
                        {
                            self.scope.dropped += 1;
                        }
                    } else {
                        let inbound = self.decode(&text);
                        self.note(&inbound);
                        out.push(inbound);
                    }
                }
                Token::Junk(junk) => out.push(Inbound::Unrecognised(junk)),
            }
        }
        out
    }

    // [@ANCHOR: radioctl_ts890_engine_link_restored]
    fn link_restored(&mut self) -> Vec<Request> {
        // A half frame from the old link must not be joined to the new link's first bytes.
        self.tokenizer = Tokenizer::with_max_pending(MAX_FRAME);
        let mut requests = Vec::new();
        // The rig forgot its auto-information with the link: turn it on again.
        if self.transceive {
            requests.extend(self.encode(&Op::SetTransceive(true)).ok());
        }
        if self.scope_output {
            requests.extend(self.encode(&Op::SetScopeOutput(true)).ok());
        }
        requests
    }
}

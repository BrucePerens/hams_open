// SPDX-License-Identifier: AGPL-3.0-or-later
//! Icom CI-V: frame codec, sans-IO client engine.
//!
//! Frame: `FE FE <to> <from> <cmd> [data] FD`. A reply either echoes the command byte with data
//! or is `FB` (OK) / `FA` (NG). Frequencies are 5 bytes of BCD, least-significant pair first
//! (14,074,000 Hz is `00 40 07 14 00`, the capture recorded in `tools/rig_simulator.py`).
//! Meter values are 2 bytes of BCD (0000 to 9999; radios use 0000 to 0255).
//!
//! **Sources.** The basic commands (03, 04, 05, 06, 1C 00, 15 02, 15 11, 17, 19 00) come from our
//! own `tools/rig_simulator.py` and the relay's `civ_bridge.rs`. The rest was read from Hamlib
//! 4.6.2 for facts only (LGPL, no code or table copied): command and sub-command numbers from
//! `rigs/icom/icom_defs.h` (`C_CTL_SPLT` 0F, `C_CTL_LVL` 14 with `S_LVL_*`, `C_CTL_RIT` 21,
//! `C_SEND_SEL_FREQ` 25, `S_MEM_FILT_WDTH` 1A 03); payload layouts from `rigs/icom/icom.c`
//! (`icom_get_rit_new` and `icom_set_it_new` for the 21 00 offset: two BCD bytes least
//! significant pair first, then a sign byte; `icom_get_dsp_flt` for the 1A 03 filter index
//! tables). Icom's own CI-V reference guides (IC-7300, IC-705, IC-7610, IC-9700) were not used:
//! their download terms forbid reuse of their data and radioctl skips such sources. No radio has
//! verified this engine. Scope data (command 27) is in [`crate::civ_scope`].
//!
//! Known gaps, deliberately not guessed: the keyer-speed level (Hamlib maps 0 to 255 to words per
//! minute through a table), SWR and the calibrated power and S-meter (calibration tables are
//! per-model measurements), transceive on and off over CAT (a menu item reached through
//! `1A 05` with model-specific numbers), data-mode and filter-number selection (`26`), and the
//! XIT of the IC-9700 (Hamlib lists RIT only). Transceive: Hamlib's per-model menu items were
//! found (`1A 05`: IC-7300 `00 71`, IC-9700 `01 27`, IC-705 `01 31`) but the description format has
//! no field for them yet. `Vfo::A` is the *selected* VFO (`03`, `05`, `25 00`) and `Vfo::B` the
//! *unselected* one (`25 01`); the engine does not track `07 00`/`07 01`, so after a selection
//! change the labels swap. On the Main/Sub rigs (IC-7610, IC-9700) the Main/Sub family of `07`
//! (`C2`, `D0`, `D1`, `D2`) is not modelled and Hamlib's split flow differs (see the oracle test).

use std::time::Duration;

use radioctl_core::description::{CivParams, Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request, PARTIAL_FRAME_TIMEOUT};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::scope::ScopeFrame;
use radioctl_core::units::{Hz, Level, Mode, Vfo};

use crate::attenuation;

use crate::civ_scope::{self, ScopeAssembler, ScopeCommand, ScopeMode};

/// Start-of-frame byte (sent twice).
pub const PREAMBLE: u8 = 0xFE;
/// End-of-frame byte.
pub const END: u8 = 0xFD;
/// Reply meaning OK.
pub const OK: u8 = 0xFB;
/// Reply meaning NG.
pub const NG: u8 = 0xFA;
/// The broadcast address every Icom rig also answers to.
pub const BROADCAST: u8 = 0x00;
/// Buffered bytes beyond this without a frame end are dropped.
const MAX_PENDING: usize = 256;
/// Largest frequency a 5-byte BCD field holds.
const MAX_BCD_HZ: u64 = 9_999_999_999;
/// Most characters one CW text command carries.
const MAX_CW_CHARS: usize = 30;

/// One decoded frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Destination address.
    pub to: u8,
    /// Source address.
    pub from: u8,
    /// Command byte.
    pub cmd: u8,
    /// Sub-command and data bytes.
    pub data: Vec<u8>,
}

impl Frame {
    // [@ANCHOR: radioctl_civ_frame_to_bytes]
    /// The wire form of the frame.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = vec![PREAMBLE, PREAMBLE, self.to, self.from, self.cmd];
        out.extend_from_slice(&self.data);
        out.push(END);
        out
    }
}

/// What the frame parser found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// A well-formed frame.
    Frame(Frame),
    /// Bytes that were not part of any frame.
    Junk(Vec<u8>),
}

// [@ANCHOR: radioctl_civ_encode_bcd_freq]
/// Encodes a frequency as 5 BCD bytes, or `None` above 9,999,999,999 Hz.
pub fn encode_bcd_freq(hz: Hz) -> Option<[u8; 5]> {
    if hz.0 > MAX_BCD_HZ {
        return None;
    }
    let mut rest = hz.0;
    let mut out = [0u8; 5];
    for byte in out.iter_mut() {
        let pair = (rest % 100) as u8;
        *byte = ((pair / 10) << 4) | (pair % 10);
        rest /= 100;
    }
    Some(out)
}

// [@ANCHOR: radioctl_civ_decode_bcd_freq]
/// Decodes up to 5 BCD bytes (least-significant pair first); `None` on a non-BCD nibble.
pub fn decode_bcd_freq(bytes: &[u8]) -> Option<Hz> {
    if bytes.is_empty() || bytes.len() > 5 {
        return None;
    }
    let mut total: u64 = 0;
    let mut scale: u64 = 1;
    for byte in bytes {
        let (high, low) = (byte >> 4, byte & 0x0F);
        if high > 9 || low > 9 {
            return None;
        }
        total += u64::from(high * 10 + low) * scale;
        scale *= 100;
    }
    Some(Hz(total))
}

// [@ANCHOR: radioctl_civ_decode_bcd_meter]
/// Decodes a 2-byte BCD meter value, most-significant pair first (`01 20` is 120).
pub fn decode_bcd_meter(bytes: &[u8]) -> Option<u16> {
    let (high, low) = (*bytes.first()?, *bytes.get(1)?);
    if bytes.len() != 2
        || [high >> 4, high & 15, low >> 4, low & 15]
            .iter()
            .any(|d| *d > 9)
    {
        return None;
    }
    let digits = |b: u8| u16::from(b >> 4) * 10 + u16::from(b & 15);
    Some(digits(high) * 100 + digits(low))
}

// [@ANCHOR: radioctl_civ_encode_bcd_meter]
/// Encodes a meter value 0 to 9999 as 2 BCD bytes, most-significant pair first.
pub fn encode_bcd_meter(value: u16) -> [u8; 2] {
    let value = value.min(9999);
    let pair = |n: u16| (((n / 10) as u8) << 4) | ((n % 10) as u8);
    [pair(value / 100), pair(value % 100)]
}

/// Splits a byte stream into CI-V frames. Holds partial frames between calls.
#[derive(Debug, Default)]
pub struct FrameParser {
    pending: Vec<u8>,
    last_byte_at: Duration,
}

impl FrameParser {
    // [@ANCHOR: radioctl_civ_parser_feed]
    /// Adds bytes received at `now` and returns every complete frame and every run of junk.
    ///
    /// A partial frame older than [`PARTIAL_FRAME_TIMEOUT`] is discarded as junk first. A second
    /// preamble inside a frame restarts it (a collision on the bus).
    pub fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Parsed> {
        let mut out = Vec::new();
        let stale = now.saturating_sub(self.last_byte_at) > PARTIAL_FRAME_TIMEOUT;
        if stale && !self.pending.is_empty() {
            out.push(Parsed::Junk(std::mem::take(&mut self.pending)));
        }
        self.last_byte_at = now;
        self.pending.extend_from_slice(bytes);
        loop {
            let Some(start) = find_preamble(&self.pending) else {
                self.drop_leading_junk(&mut out);
                break;
            };
            if start > 0 {
                out.push(Parsed::Junk(self.pending.drain(..start).collect()));
            }
            let Some(end) = self.pending.iter().position(|b| *b == END) else {
                if self.pending.len() > MAX_PENDING {
                    out.push(Parsed::Junk(std::mem::take(&mut self.pending)));
                }
                break;
            };
            let raw: Vec<u8> = self.pending.drain(..=end).collect();
            out.push(frame_or_junk(raw));
        }
        out
    }

    // [@ANCHOR: radioctl_civ_parser_drop_leading_junk]
    /// With no preamble pending, everything but a trailing lone `FE` is junk.
    fn drop_leading_junk(&mut self, out: &mut Vec<Parsed>) {
        let keep = usize::from(self.pending.last() == Some(&PREAMBLE));
        let cut = self.pending.len().saturating_sub(keep);
        if cut > 0 {
            out.push(Parsed::Junk(self.pending.drain(..cut).collect()));
        }
    }
}

fn find_preamble(bytes: &[u8]) -> Option<usize> {
    bytes.windows(2).position(|w| w == [PREAMBLE, PREAMBLE])
}

// [@ANCHOR: radioctl_civ_frame_or_junk]
/// Turns the bytes `FE FE ... FD` into a frame, or junk. A later preamble restarts the frame.
fn frame_or_junk(raw: Vec<u8>) -> Parsed {
    let restart = raw
        .windows(2)
        .enumerate()
        .skip(1)
        .filter(|(_, w)| *w == [PREAMBLE, PREAMBLE])
        .map(|(i, _)| i)
        .next_back();
    if let Some(at) = restart {
        return frame_or_junk(raw.get(at..).map(<[u8]>::to_vec).unwrap_or_default());
    }
    let body = raw.get(2..raw.len().saturating_sub(1)).unwrap_or(&[]);
    match body {
        [to, from, cmd, data @ ..] => Parsed::Frame(Frame {
            to: *to,
            from: *from,
            cmd: *cmd,
            data: data.to_vec(),
        }),
        _ => Parsed::Junk(raw),
    }
}

/// The CI-V client engine for one rig.
#[derive(Debug)]
pub struct CivEngine {
    desc: RigDescription,
    params: CivParams,
    parser: FrameParser,
    /// The last mode the rig reported; the filter index tables depend on it.
    mode: Option<Mode>,
    scope: Option<ScopeAssembler>,
    /// True between a PTT set and the next reply: some Xiegu radios answer it with `1C 00 <state>`
    /// instead of `FB` (Hamlib's `x108g_set_ptt` comment), which counts as the acknowledgement.
    awaiting_ptt_ack: bool,
}

impl CivEngine {
    // [@ANCHOR: radioctl_civ_engine_new]
    /// Builds an engine from a CI-V description.
    pub fn new(desc: RigDescription) -> Result<CivEngine, RigError> {
        let params = match (&desc.family, &desc.civ) {
            (Family::IcomCiv, Some(params)) => params.clone(),
            _ => {
                return Err(RigError::new(
                    ErrorKind::Unsupported,
                    "not a CI-V description",
                ))
            }
        };
        let scope = desc.scope.clone().map(ScopeAssembler::new);
        Ok(CivEngine {
            desc,
            params,
            parser: FrameParser::default(),
            mode: None,
            scope,
            awaiting_ptt_ack: false,
        })
    }

    // [@ANCHOR: radioctl_civ_engine_scope_request]
    /// The request for a scope command (command 27). Fails when the model has no scope or a value
    /// does not fit its field. A query's answer is not waited for (`ReplyKind::None`); the
    /// operations of [`Op`] wait for it.
    pub fn scope_request(&self, command: ScopeCommand) -> Result<Request, RigError> {
        let reply = if command.is_query() {
            ReplyKind::None
        } else {
            ReplyKind::Ack
        };
        self.scope_request_for(command, reply)
    }

    // [@ANCHOR: radioctl_civ_engine_scope_request_for]
    fn scope_request_for(
        &self,
        command: ScopeCommand,
        reply: ReplyKind,
    ) -> Result<Request, RigError> {
        if self.desc.scope.is_none() {
            return Err(Self::unsupported("this model has no scope description"));
        }
        let data = civ_scope::command_data(command)
            .ok_or_else(|| Self::unsupported("scope value out of range"))?;
        let (sub, rest) = data
            .split_first()
            .ok_or_else(|| Self::unsupported("empty scope command"))?;
        let mut payload = vec![*sub];
        payload.extend_from_slice(rest);
        Ok(self.command(civ_scope::SCOPE_CMD, &payload, reply))
    }

    // [@ANCHOR: radioctl_civ_engine_encode_scope]
    /// The scope operations of the core API as command 27. A span the model's menu does not
    /// offer is refused here rather than sent for the rig to refuse.
    fn encode_scope(&self, op: &Op) -> Option<Result<Request, RigError>> {
        let ack = ReplyKind::Ack;
        let (command, reply) = match op {
            Op::SetScopeOutput(on) => (ScopeCommand::SetOutput(*on), ack),
            Op::GetScopeOutput => (
                ScopeCommand::GetOutput,
                ReplyKind::Observation(Field::ScopeOutput),
            ),
            Op::SetScopeAxis(rx, axis) => (ScopeCommand::SetMode(*rx, (*axis).into()), ack),
            Op::GetScopeAxis(rx) => (
                ScopeCommand::GetMode(*rx),
                ReplyKind::Observation(Field::ScopeAxis(*rx)),
            ),
            Op::SetScopeSpan(rx, span) => {
                let offered = self
                    .desc
                    .scope
                    .as_ref()
                    .is_none_or(|s| s.spans_hz.is_empty() || s.spans_hz.contains(&span.0));
                if !offered {
                    return Some(Err(Self::unsupported("span not offered by this scope")));
                }
                (ScopeCommand::SetSpan(*rx, *span), ack)
            }
            Op::GetScopeSpan(rx) => (
                ScopeCommand::GetSpan(*rx),
                ReplyKind::Observation(Field::ScopeSpan(*rx)),
            ),
            Op::SetScopeEdges(range, edge, low, high) => {
                (ScopeCommand::SetEdges(*range, *edge, *low, *high), ack)
            }
            Op::SetScopeReference(rx, centi_db) => {
                (ScopeCommand::SetReference(*rx, *centi_db), ack)
            }
            Op::GetScopeReference(rx) => (
                ScopeCommand::GetReference(*rx),
                ReplyKind::Observation(Field::ScopeReference(*rx)),
            ),
            _ => return None,
        };
        Some(self.scope_request_for(command, reply))
    }

    // [@ANCHOR: radioctl_civ_engine_scope_dropped]
    /// Scope frames or sweeps discarded as out of order or malformed.
    pub fn scope_dropped(&self) -> u64 {
        self.scope.as_ref().map_or(0, ScopeAssembler::dropped)
    }
}

// [@ANCHOR: radioctl_civ_level_sub]
/// Wire sub-command of a level under `14`, for the levels this engine supports.
fn level_sub(level: Level) -> Option<u8> {
    match level {
        Level::AfGain => Some(0x01),
        Level::RfGain => Some(0x02),
        Level::RfPower => Some(0x0A),
        Level::MicGain => Some(0x0B),
        Level::MonitorLevel => Some(0x15),
        Level::KeyerSpeed | Level::Attenuator | Level::Preamp => None,
    }
}

/// The level behind a `14` sub-command.
fn sub_level(sub: u8) -> Option<Level> {
    [
        Level::AfGain,
        Level::RfGain,
        Level::RfPower,
        Level::MicGain,
        Level::MonitorLevel,
    ]
    .into_iter()
    .find(|level| level_sub(*level) == Some(sub))
}

/// Largest level value a rig takes (levels are 0000 to 0255 in BCD).
const MAX_LEVEL: u16 = 255;
/// Largest RIT/XIT offset, in hertz (4 BCD digits).
const MAX_OFFSET_HZ: u16 = 9_999;

// [@ANCHOR: radioctl_civ_encode_rit_offset]
/// The 3 data bytes of the RIT/XIT offset after `21 00`: tens and ones, thousands and hundreds,
/// then a sign byte (1 is negative).
pub fn encode_rit_offset(hz: i16) -> Option<[u8; 3]> {
    let magnitude = hz.unsigned_abs();
    if magnitude > MAX_OFFSET_HZ {
        return None;
    }
    let pair = |n: u16| (((n / 10) as u8) << 4) | ((n % 10) as u8);
    Some([
        pair(magnitude % 100),
        pair(magnitude / 100),
        u8::from(hz < 0),
    ])
}

// [@ANCHOR: radioctl_civ_decode_rit_offset]
/// The offset in hertz from the 3 data bytes after `21 00`.
pub fn decode_rit_offset(bytes: &[u8]) -> Option<i16> {
    let [low, high, sign] = bytes else {
        return None;
    };
    let digits = |b: u8| -> Option<i16> {
        let (h, l) = (b >> 4, b & 15);
        (h <= 9 && l <= 9).then(|| i16::from(h) * 10 + i16::from(l))
    };
    let magnitude = digits(*high)? * 100 + digits(*low)?;
    Some(if *sign == 0 { magnitude } else { -magnitude })
}

// [@ANCHOR: radioctl_civ_filter_index_to_hz]
/// The width in hertz of filter index `index` (command `1A 03`): 0 to 9 are 50 to 500 Hz in
/// steps of 50, 10 to 40 are 600 to 3600 Hz in steps of 100 (SSB, CW, RTTY); for AM the index
/// 0 to 49 is 200 to 10000 Hz in steps of 200. `None` outside the table.
pub fn filter_index_to_hz(am: bool, index: u8) -> Option<u32> {
    let index = u32::from(index);
    if am {
        return (index <= 49).then(|| (index + 1) * 200);
    }
    match index {
        0..=9 => Some((index + 1) * 50),
        10..=40 => Some(600 + (index - 10) * 100),
        _ => None,
    }
}

// [@ANCHOR: radioctl_civ_filter_hz_to_index]
/// The filter index whose width is exactly `hz`, or `None` when the table has no such width.
pub fn filter_hz_to_index(am: bool, hz: u32) -> Option<u8> {
    (0..=49u8).find(|index| filter_index_to_hz(am, *index) == Some(hz))
}

fn bcd_pair(value: u16) -> u8 {
    (((value / 10) as u8) << 4) | ((value % 10) as u8)
}

// [@ANCHOR: radioctl_civ_decode_bcd_byte]
/// One BCD byte (`0x20` is 20); `None` if a nibble is not a digit.
fn decode_bcd_byte(byte: u8) -> Option<u16> {
    let (high, low) = (byte >> 4, byte & 15);
    (high <= 9 && low <= 9).then(|| u16::from(high) * 10 + u16::from(low))
}

impl CivEngine {
    fn unsupported(what: &str) -> RigError {
        RigError::new(ErrorKind::Unsupported, what.to_string())
    }

    fn command(&self, cmd: u8, data: &[u8], reply: ReplyKind) -> Request {
        let frame = Frame {
            to: self.params.address,
            from: self.params.controller,
            cmd,
            data: data.to_vec(),
        };
        Request {
            bytes: frame.to_bytes(),
            reply,
        }
    }

    fn get(&self, cmd: u8, data: &[u8], field: Field) -> Request {
        self.command(cmd, data, ReplyKind::Observation(field))
    }

    fn set(&self, cmd: u8, data: &[u8]) -> Request {
        self.command(cmd, data, ReplyKind::Ack)
    }

    // [@ANCHOR: radioctl_civ_engine_filter_table_is_am]
    fn filter_table_is_am(&self) -> Result<bool, RigError> {
        match self.mode {
            Some(Mode::Am) => Ok(true),
            Some(Mode::Fm | Mode::Wfm) => Err(Self::unsupported("no filter width in FM")),
            Some(_) => Ok(false),
            None => Err(Self::unsupported(
                "read the mode before the filter width: the index tables differ",
            )),
        }
    }

    // [@ANCHOR: radioctl_civ_engine_encode_vfo_b_split]
    fn encode_vfo_b_or_split(&self, op: &Op) -> Option<Result<Request, RigError>> {
        Some(match op {
            Op::GetFrequency(Vfo::B) => Ok(self.get(0x25, &[0x01], Field::Frequency(Vfo::B))),
            Op::SetFrequency(Vfo::B, hz) => match encode_bcd_freq(*hz) {
                Some(bcd) => {
                    let mut data = vec![0x01];
                    data.extend(bcd);
                    Ok(self.set(0x25, &data))
                }
                None => Err(Self::unsupported("frequency above 10 BCD digits")),
            },
            Op::GetSplit => Ok(self.get(0x0F, &[], Field::Split)),
            Op::SetSplit(on) => Ok(self.set(0x0F, &[u8::from(*on)])),
            _ => return None,
        })
    }

    // [@ANCHOR: radioctl_civ_engine_encode_rit_xit]
    fn encode_rit_xit(&self, op: &Op) -> Option<Result<Request, RigError>> {
        Some(match op {
            Op::GetRit => Ok(self.get(0x21, &[0x01], Field::Rit)),
            Op::SetRit(on) => Ok(self.set(0x21, &[0x01, u8::from(*on)])),
            Op::GetXit => Ok(self.get(0x21, &[0x02], Field::Xit)),
            Op::SetXit(on) => Ok(self.set(0x21, &[0x02, u8::from(*on)])),
            Op::GetRitOffset => Ok(self.get(0x21, &[0x00], Field::RitOffset)),
            Op::SetRitOffset(hz) => match encode_rit_offset(*hz) {
                Some(bytes) => {
                    let mut data = vec![0x00];
                    data.extend(bytes);
                    Ok(self.set(0x21, &data))
                }
                None => Err(Self::unsupported("RIT offset beyond 9999 Hz")),
            },
            _ => return None,
        })
    }

    // [@ANCHOR: radioctl_civ_engine_encode_front_end]
    /// Sets the attenuator (`11`, one BCD byte of decibels) or the preamplifier (`16 02`, a step
    /// number), when the description lists the value.
    fn encode_front_end(&self, level: Level, value: u16) -> Result<Request, RigError> {
        let step = attenuation::step_of(&self.desc, level, value)?;
        if level == Level::Attenuator {
            if value > 99 {
                return Err(Self::unsupported("attenuation does not fit one BCD byte"));
            }
            Ok(self.set(0x11, &[bcd_pair(value)]))
        } else {
            Ok(self.set(0x16, &[0x02, step]))
        }
    }

    // [@ANCHOR: radioctl_civ_engine_encode_levels_and_filter]
    fn encode_levels_and_filter(&self, op: &Op) -> Option<Result<Request, RigError>> {
        Some(match op {
            Op::GetLevel(level @ Level::Attenuator) => {
                Ok(self.get(0x11, &[], Field::Level(*level)))
            }
            Op::GetLevel(level @ Level::Preamp) => {
                Ok(self.get(0x16, &[0x02], Field::Level(*level)))
            }
            Op::SetLevel(level @ (Level::Attenuator | Level::Preamp), value) => {
                self.encode_front_end(*level, *value)
            }
            Op::GetLevel(level) => match level_sub(*level) {
                Some(sub) => Ok(self.get(0x14, &[sub], Field::Level(*level))),
                None => Err(Self::unsupported("level not implemented for CI-V")),
            },
            Op::SetLevel(level, value) => match (level_sub(*level), *value <= MAX_LEVEL) {
                (Some(sub), true) => {
                    let mut data = vec![sub];
                    data.extend(encode_bcd_meter(*value));
                    Ok(self.set(0x14, &data))
                }
                (None, _) => Err(Self::unsupported("level not implemented for CI-V")),
                (_, false) => Err(Self::unsupported("level above 255")),
            },
            Op::GetFilterWidth => match self.filter_table_is_am() {
                Ok(_) => Ok(self.get(0x1A, &[0x03], Field::FilterWidth)),
                Err(e) => Err(e),
            },
            Op::SetFilterWidth(hz) => self.filter_table_is_am().and_then(|am| {
                let index = u32::try_from(hz.0)
                    .ok()
                    .and_then(|width| filter_hz_to_index(am, width))
                    .ok_or_else(|| Self::unsupported("no filter of exactly that width"))?;
                Ok(self.set(0x1A, &[0x03, bcd_pair(u16::from(index))]))
            }),
            Op::GetModelId => Ok(self.get(0x19, &[0x00], Field::ModelId)),
            _ => return None,
        })
    }
}

impl ProtocolEngine for CivEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_civ_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(Self::unsupported("capability not in the rig description"));
        }
        self.awaiting_ptt_ack = matches!(op, Op::SetPtt(_));
        let ack = ReplyKind::Ack;
        if let Some(done) = self
            .encode_vfo_b_or_split(op)
            .or_else(|| self.encode_rit_xit(op))
            .or_else(|| self.encode_levels_and_filter(op))
            .or_else(|| self.encode_scope(op))
        {
            return done;
        }
        let request = match op {
            Op::GetFrequency(_) => {
                self.command(0x03, &[], ReplyKind::Observation(Field::Frequency(Vfo::A)))
            }
            Op::SetFrequency(_, hz) => {
                let bcd = encode_bcd_freq(*hz)
                    .ok_or_else(|| Self::unsupported("frequency above 10 BCD digits"))?;
                self.command(0x05, &bcd, ack)
            }
            Op::GetMode => self.command(0x04, &[], ReplyKind::Observation(Field::Mode)),
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| Self::unsupported("mode not in the description"))?;
                self.command(0x06, &[code], ack)
            }
            Op::GetPtt => self.command(0x1C, &[0x00], ReplyKind::Observation(Field::Ptt)),
            Op::SetPtt(on) => self.command(0x1C, &[0x00, u8::from(*on)], ack),
            Op::GetSignal => self.command(0x15, &[0x02], ReplyKind::Observation(Field::Signal)),
            Op::GetPower => self.command(0x15, &[0x11], ReplyKind::Observation(Field::Power)),
            Op::SendCw(text) => {
                let ok = !text.is_empty()
                    && text.len() <= MAX_CW_CHARS
                    && text.bytes().all(|b| (0x20..0x7F).contains(&b));
                if !ok {
                    return Err(Self::unsupported("CW text must be 1 to 30 printable ASCII"));
                }
                self.command(0x17, text.as_bytes(), ack)
            }
            // Command 17 with the single byte FF stops the CW text being sent and clears the
            // buffer. Fact from Hamlib 4.6.2 `rigs/icom/icom.c` (`icom_stop_morse`: `C_SND_CW`
            // with one data byte 0xff); Icom's CI-V reference guides were not read (see the
            // module note). Unverified on a radio, like the rest of this engine.
            Op::StopCw => self.command(0x17, &[0xFF], ack),
            Op::SetTransceive(_) => {
                return Err(Self::unsupported(
                    "CI-V transceive is set in the rig's menu",
                ));
            }
            _ => {
                return Err(Self::unsupported("not implemented for the CI-V engine"));
            }
        };
        Ok(request)
    }

    // [@ANCHOR: radioctl_civ_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        for item in self.parser.feed(bytes, now) {
            match item {
                Parsed::Junk(junk) => out.push(Inbound::Unrecognised(junk)),
                Parsed::Frame(frame) => {
                    if let Some(event) = self.decode(&frame) {
                        if let Inbound::Observation(Observation::Mode(mode)) = &event {
                            self.mode = Some(*mode);
                        }
                        out.push(event);
                    }
                }
            }
        }
        out
    }
}

impl CivEngine {
    // [@ANCHOR: radioctl_civ_engine_decode]
    /// Decodes one frame; `None` for our own echoed frames, frames for other stations and scope
    /// frames (those go to the scope assembler).
    fn decode(&mut self, frame: &Frame) -> Option<Inbound> {
        if frame.from == self.params.controller {
            return None;
        }
        let for_us = frame.to == self.params.controller || frame.to == BROADCAST;
        if !for_us || frame.from != self.params.address {
            return None;
        }
        if frame.cmd == civ_scope::SCOPE_CMD {
            return self.decode_scope(&frame.data);
        }
        let data = frame.data.as_slice();
        let ptt_echo = frame.cmd == 0x1C && matches!(data, [0x00, _]);
        if std::mem::take(&mut self.awaiting_ptt_ack) && ptt_echo {
            return Some(Inbound::Ack);
        }
        let observation = match (frame.cmd, data) {
            (OK, []) => return Some(Inbound::Ack),
            (NG, []) => return Some(Inbound::Nak),
            (0x00 | 0x03, bcd) => decode_bcd_freq(bcd).map(|hz| Observation::Frequency(Vfo::A, hz)),
            (0x25, [selector, bcd @ ..]) => decode_bcd_freq(bcd).and_then(|hz| match selector {
                0x00 => Some(Observation::Frequency(Vfo::A, hz)),
                0x01 => Some(Observation::Frequency(Vfo::B, hz)),
                _ => None,
            }),
            (0x01 | 0x04, [code, ..]) => self.desc.code_mode(*code).map(Observation::Mode),
            (0x0F, [state]) => Some(Observation::Split(*state != 0)),
            (0x21, [0x00, rest @ ..]) => decode_rit_offset(rest).map(Observation::RitOffset),
            (0x21, [0x01, state]) => Some(Observation::Rit(*state != 0)),
            (0x21, [0x02, state]) => Some(Observation::Xit(*state != 0)),
            (0x14, [sub, rest @ ..]) => sub_level(*sub).and_then(|level| {
                decode_bcd_meter(rest)
                    .filter(|value| *value <= MAX_LEVEL)
                    .map(|value| Observation::Level(level, value))
            }),
            (0x11, [byte]) => decode_bcd_byte(*byte)
                .and_then(|db| {
                    attenuation::step_of(&self.desc, Level::Attenuator, db)
                        .ok()
                        .map(|_| db)
                })
                .map(|db| Observation::Level(Level::Attenuator, db)),
            (0x16, [0x02, step]) => attenuation::value_of(&self.desc, Level::Preamp, *step)
                .map(|v| Observation::Level(Level::Preamp, v)),
            (0x1A, [0x03, index]) => self.decode_filter(*index),
            (0x19, [0x00, id]) => Some(Observation::ModelId(u16::from(*id))),
            (0x1C, [0x00, state]) => Some(Observation::Ptt(*state != 0)),
            (0x15, [0x02, rest @ ..]) => decode_bcd_meter(rest).map(Observation::Signal),
            (0x15, [0x11, rest @ ..]) => decode_bcd_meter(rest).map(Observation::Power),
            _ => None,
        };
        Some(match observation {
            Some(value) => Inbound::Observation(value),
            None => Inbound::Unrecognised(frame.to_bytes()),
        })
    }

    // [@ANCHOR: radioctl_civ_engine_decode_filter]
    /// The filter width for index byte `index`, by the mode last reported.
    fn decode_filter(&self, index: u8) -> Option<Observation> {
        let (high, low) = (index >> 4, index & 15);
        if high > 9 || low > 9 {
            return None;
        }
        let am = self.filter_table_is_am().ok()?;
        filter_index_to_hz(am, high * 10 + low)
            .map(|hz| Observation::FilterWidth(Hz(u64::from(hz))))
    }

    // [@ANCHOR: radioctl_civ_engine_decode_scope]
    /// A command 27 frame from the rig: a waveform frame goes to the assembler and comes out as
    /// a [`ScopeFrame`] when it completes a sweep; a setting's answer becomes its observation.
    fn decode_scope(&mut self, data: &[u8]) -> Option<Inbound> {
        let observation = match data {
            [civ_scope::SUB_WAVEFORM, rest @ ..] => {
                let assembler = self.scope.as_mut()?;
                return match civ_scope::parse_segment(rest) {
                    Some(segment) => {
                        let line = assembler.push(segment)?;
                        let amplitude_max = self.desc.scope.as_ref().map_or(0, |s| s.amplitude_max);
                        Some(Inbound::ScopeFrame(ScopeFrame {
                            seq: 0,
                            receiver: line.receiver,
                            axis: line.mode.into(),
                            low: line.low,
                            high: line.high,
                            out_of_range: line.out_of_range,
                            amplitude_max,
                            samples: line.samples,
                        }))
                    }
                    None => {
                        assembler.note_malformed();
                        None
                    }
                };
            }
            [civ_scope::SUB_OUTPUT, on] => Observation::ScopeOutput(*on != 0),
            [civ_scope::SUB_MODE, rx, code] => {
                Observation::ScopeAxis(*rx, ScopeMode::from_code(*code)?.into())
            }
            [civ_scope::SUB_SPAN, rx, bcd @ ..] if bcd.len() == 5 => {
                let half = decode_bcd_freq(bcd)?;
                Observation::ScopeSpan(*rx, Hz(half.0.saturating_mul(2)))
            }
            [civ_scope::SUB_REFERENCE, rx, bcd @ ..] => {
                Observation::ScopeReference(*rx, civ_scope::decode_reference(bcd)?)
            }
            _ => return None,
        };
        Some(Inbound::Observation(observation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn engine() -> CivEngine {
        CivEngine::new(RigDescription::ic7300()).expect("engine")
    }

    // Tests [@ANCHOR: radioctl_civ_encode_bcd_freq]
    // Tests [@ANCHOR: radioctl_civ_decode_bcd_freq]
    #[test]
    fn bcd_frequency_matches_the_recorded_capture() {
        assert_eq!(
            encode_bcd_freq(Hz(14_074_000)),
            Some([0x00, 0x40, 0x07, 0x14, 0x00])
        );
        assert_eq!(
            decode_bcd_freq(&[0x00, 0x40, 0x07, 0x14, 0x00]),
            Some(Hz(14_074_000))
        );
        assert_eq!(encode_bcd_freq(Hz(10_000_000_000)), None);
        assert_eq!(decode_bcd_freq(&[0x0A]), None);
        assert_eq!(decode_bcd_freq(&[]), None);
        assert_eq!(decode_bcd_freq(&[0; 6]), None);
    }

    // Tests [@ANCHOR: radioctl_civ_decode_bcd_meter]
    // Tests [@ANCHOR: radioctl_civ_encode_bcd_meter]
    #[test]
    fn bcd_meter_round_trips() {
        assert_eq!(encode_bcd_meter(120), [0x01, 0x20]);
        assert_eq!(decode_bcd_meter(&[0x01, 0x20]), Some(120));
        assert_eq!(decode_bcd_meter(&[0x01]), None);
        assert_eq!(decode_bcd_meter(&[0x0F, 0x00]), None);
        assert_eq!(decode_bcd_meter(&[0, 0, 0]), None);
    }

    // Tests [@ANCHOR: radioctl_civ_frame_to_bytes]
    // Tests [@ANCHOR: radioctl_civ_parser_feed]
    // Tests [@ANCHOR: radioctl_civ_parser_drop_leading_junk]
    #[test]
    fn parser_splits_frames_across_feeds_and_reports_junk() {
        let mut parser = FrameParser::default();
        assert!(parser.feed(&[0xFE, 0xFE, 0xE0], T0).is_empty());
        let out = parser.feed(&[0x94, 0xFB, 0xFD], T0);
        let frame = Frame {
            to: 0xE0,
            from: 0x94,
            cmd: 0xFB,
            data: vec![],
        };
        assert_eq!(out, vec![Parsed::Frame(frame.clone())]);
        assert_eq!(frame.to_bytes(), vec![0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD]);
        let out = parser.feed(&[0x01, 0x02, 0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD], T0);
        assert_eq!(out.len(), 2);
        assert_eq!(out.first(), Some(&Parsed::Junk(vec![0x01, 0x02])));
    }

    // Tests [@ANCHOR: radioctl_civ_parser_feed]
    // Tests [@ANCHOR: radioctl_civ_frame_or_junk]
    #[test]
    fn parser_restarts_on_a_second_preamble_and_drops_stale_partials() {
        let mut parser = FrameParser::default();
        let out = parser.feed(&[0xFE, 0xFE, 0x01, 0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD], T0);
        assert_eq!(out.len(), 1);
        assert!(matches!(out.first(), Some(Parsed::Frame(f)) if f.to == 0xE0));
        assert!(parser.feed(&[0xFE, 0xFE, 0xE0], T0).is_empty());
        let later = T0 + PARTIAL_FRAME_TIMEOUT + Duration::from_millis(1);
        let out = parser.feed(&[0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD], later);
        assert_eq!(out.len(), 2);
        assert_eq!(out.first(), Some(&Parsed::Junk(vec![0xFE, 0xFE, 0xE0])));
    }

    // Tests [@ANCHOR: radioctl_civ_engine_new]
    #[test]
    fn engine_refuses_a_kenwood_description() {
        let err = CivEngine::new(RigDescription::ts2000()).expect_err("must refuse");
        assert_eq!(err.kind, ErrorKind::Unsupported);
    }

    // Tests [@ANCHOR: radioctl_civ_engine_encode]
    #[test]
    fn encode_produces_the_capture_bytes() {
        let mut engine = engine();
        let get = engine.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(get.bytes, vec![0xFE, 0xFE, 0x94, 0xE0, 0x03, 0xFD]);
        let set = engine
            .encode(&Op::SetFrequency(Vfo::A, Hz(14_074_000)))
            .expect("set");
        assert_eq!(
            set.bytes,
            vec![0xFE, 0xFE, 0x94, 0xE0, 0x05, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD]
        );
        assert_eq!(set.reply, ReplyKind::Ack);
        let ptt = engine.encode(&Op::SetPtt(false)).expect("ptt");
        assert_eq!(
            ptt.bytes,
            vec![0xFE, 0xFE, 0x94, 0xE0, 0x1C, 0x00, 0x00, 0xFD]
        );
        assert!(engine.encode(&Op::GetFrequency(Vfo::B)).is_err());
        assert!(engine.encode(&Op::SendCw(String::new())).is_err());
        assert_eq!(
            engine.encode(&Op::StopCw).expect("stop").bytes,
            vec![0xFE, 0xFE, 0x94, 0xE0, 0x17, 0xFF, 0xFD]
        );
        assert!(engine.encode(&Op::SetTransceive(true)).is_err());
    }

    // Tests [@ANCHOR: radioctl_civ_engine_feed]
    // Tests [@ANCHOR: radioctl_civ_engine_decode]
    #[test]
    fn feed_decodes_replies_skips_echo_and_ignores_other_stations() {
        let mut engine = engine();
        let echo = [0xFE, 0xFE, 0x94, 0xE0, 0x03, 0xFD];
        let reply = [
            0xFE, 0xFE, 0xE0, 0x94, 0x03, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD,
        ];
        let other = [
            0xFE, 0xFE, 0xE0, 0x70, 0x03, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD,
        ];
        let mut wire = Vec::new();
        wire.extend_from_slice(&echo);
        wire.extend_from_slice(&other);
        wire.extend_from_slice(&reply);
        assert_eq!(
            engine.feed(&wire, T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(14_074_000)
            ))]
        );
        let ng = [0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD];
        assert_eq!(engine.feed(&ng, T0), vec![Inbound::Nak]);
        let unsolicited = [0xFE, 0xFE, 0x00, 0x94, 0x01, 0x01, 0x01, 0xFD];
        assert_eq!(
            engine.feed(&unsolicited, T0),
            vec![Inbound::Observation(Observation::Mode(
                radioctl_core::units::Mode::Usb
            ))]
        );
        let strange = [0xFE, 0xFE, 0xE0, 0x94, 0x42, 0x01, 0xFD];
        assert!(matches!(
            engine.feed(&strange, T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    fn reply(address: u8, cmd: u8, data: &[u8]) -> Vec<u8> {
        Frame {
            to: 0xE0,
            from: address,
            cmd,
            data: data.to_vec(),
        }
        .to_bytes()
    }

    // Tests [@ANCHOR: radioctl_civ_encode_rit_offset]
    // Tests [@ANCHOR: radioctl_civ_decode_rit_offset]
    #[test]
    fn rit_offset_is_two_bcd_bytes_and_a_sign() {
        assert_eq!(encode_rit_offset(150), Some([0x50, 0x01, 0x00]));
        assert_eq!(encode_rit_offset(-1200), Some([0x00, 0x12, 0x01]));
        assert_eq!(encode_rit_offset(9999), Some([0x99, 0x99, 0x00]));
        assert_eq!(encode_rit_offset(10_000), None);
        assert_eq!(decode_rit_offset(&[0x50, 0x01, 0x00]), Some(150));
        assert_eq!(decode_rit_offset(&[0x00, 0x12, 0x01]), Some(-1200));
        assert_eq!(decode_rit_offset(&[0x5A, 0x01, 0x00]), None);
        assert_eq!(decode_rit_offset(&[0x50, 0x01]), None);
    }

    // Tests [@ANCHOR: radioctl_civ_filter_index_to_hz]
    // Tests [@ANCHOR: radioctl_civ_filter_hz_to_index]
    #[test]
    fn filter_tables_match_the_two_ranges() {
        assert_eq!(filter_index_to_hz(false, 0), Some(50));
        assert_eq!(filter_index_to_hz(false, 9), Some(500));
        assert_eq!(filter_index_to_hz(false, 10), Some(600));
        assert_eq!(filter_index_to_hz(false, 40), Some(3600));
        assert_eq!(filter_index_to_hz(false, 41), None);
        assert_eq!(filter_index_to_hz(true, 0), Some(200));
        assert_eq!(filter_index_to_hz(true, 49), Some(10_000));
        assert_eq!(filter_index_to_hz(true, 50), None);
        assert_eq!(filter_hz_to_index(false, 2400), Some(28));
        assert_eq!(filter_hz_to_index(false, 575), None);
        assert_eq!(filter_hz_to_index(true, 6000), Some(29));
        for index in 0..=40u8 {
            let hz = filter_index_to_hz(false, index).expect("in table");
            assert_eq!(filter_hz_to_index(false, hz), Some(index));
        }
    }

    // Tests [@ANCHOR: radioctl_civ_engine_encode]
    // Tests [@ANCHOR: radioctl_civ_engine_encode_levels_and_filter]
    // Tests [@ANCHOR: radioctl_civ_engine_encode_rit_xit]
    // Tests [@ANCHOR: radioctl_civ_engine_encode_vfo_b_split]
    // Tests [@ANCHOR: radioctl_civ_engine_filter_table_is_am]
    // Tests [@ANCHOR: radioctl_civ_level_sub]
    #[test]
    fn the_extended_commands_encode_as_read_from_the_facts() {
        let mut e = CivEngine::new(shipped("icom-ic-7610")).expect("engine");
        let b = |e: &mut CivEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        let head = [0xFE, 0xFE, 0x98, 0xE0];
        let with = |body: &[u8]| [&head[..], body, &[0xFD]].concat();
        assert_eq!(b(&mut e, Op::GetFrequency(Vfo::B)), with(&[0x25, 0x01]));
        assert_eq!(
            b(&mut e, Op::SetFrequency(Vfo::B, Hz(7_000_000))),
            with(&[0x25, 0x01, 0x00, 0x00, 0x00, 0x07, 0x00])
        );
        assert_eq!(b(&mut e, Op::GetSplit), with(&[0x0F]));
        assert_eq!(b(&mut e, Op::SetSplit(true)), with(&[0x0F, 0x01]));
        assert_eq!(b(&mut e, Op::SetXit(true)), with(&[0x21, 0x02, 0x01]));
        assert_eq!(b(&mut e, Op::GetRitOffset), with(&[0x21, 0x00]));
        assert_eq!(
            b(&mut e, Op::SetLevel(Level::MicGain, 255)),
            with(&[0x14, 0x0B, 0x02, 0x55])
        );
        assert_eq!(
            b(&mut e, Op::GetLevel(Level::MonitorLevel)),
            with(&[0x14, 0x15])
        );
        assert_eq!(b(&mut e, Op::GetModelId), with(&[0x19, 0x00]));
        assert!(e.encode(&Op::SetLevel(Level::AfGain, 256)).is_err());
        assert!(e.encode(&Op::GetLevel(Level::KeyerSpeed)).is_err());
        assert!(e.encode(&Op::SetRitOffset(10_000)).is_err());
        assert!(e.encode(&Op::GetFilterWidth).is_err(), "mode unknown yet");
        assert!(e.encode(&Op::GetSwr).is_err(), "SWR is a gap");
    }

    // Tests [@ANCHOR: radioctl_civ_engine_decode]
    // Tests [@ANCHOR: radioctl_civ_engine_decode_filter]
    // Tests [@ANCHOR: radioctl_civ_engine_feed]
    #[test]
    fn the_filter_index_is_read_by_the_mode_last_reported() {
        let mut e = CivEngine::new(shipped("icom-ic-7300")).expect("engine");
        let usb = reply(0x94, 0x04, &[0x01, 0x01]);
        e.feed(&usb, T0);
        let ssb = reply(0x94, 0x1A, &[0x03, 0x28]);
        assert_eq!(
            e.feed(&ssb, T0),
            vec![Inbound::Observation(Observation::FilterWidth(Hz(2400)))]
        );
        let set = e.encode(&Op::SetFilterWidth(Hz(2400))).expect("set");
        assert_eq!(
            set.bytes,
            vec![0xFE, 0xFE, 0x94, 0xE0, 0x1A, 0x03, 0x28, 0xFD]
        );
        assert!(e.encode(&Op::SetFilterWidth(Hz(2450))).is_err());
        e.feed(&reply(0x94, 0x04, &[0x02, 0x01]), T0);
        assert_eq!(
            e.feed(&reply(0x94, 0x1A, &[0x03, 0x29]), T0),
            vec![Inbound::Observation(Observation::FilterWidth(Hz(6000)))]
        );
        e.feed(&reply(0x94, 0x04, &[0x05, 0x01]), T0);
        assert!(e.encode(&Op::GetFilterWidth).is_err(), "FM has none");
        assert!(matches!(
            e.feed(&reply(0x94, 0x1A, &[0x03, 0x29]), T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(matches!(
            e.feed(&reply(0x94, 0x1A, &[0x03, 0x7F]), T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }

    // Tests [@ANCHOR: radioctl_civ_engine_decode]
    #[test]
    fn the_extended_replies_decode() {
        let mut e = CivEngine::new(shipped("icom-ic-9700")).expect("engine");
        let seen = |e: &mut CivEngine, cmd: u8, data: &[u8]| e.feed(&reply(0xA2, cmd, data), T0);
        assert_eq!(
            seen(&mut e, 0x25, &[0x01, 0x00, 0x40, 0x17, 0x44, 0x01]),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::B,
                Hz(144_174_000)
            ))]
        );
        assert_eq!(
            seen(&mut e, 0x25, &[0x00, 0x00, 0x40, 0x17, 0x44, 0x01]),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(144_174_000)
            ))]
        );
        assert_eq!(
            seen(&mut e, 0x0F, &[0x01]),
            vec![Inbound::Observation(Observation::Split(true))]
        );
        assert_eq!(
            seen(&mut e, 0x21, &[0x00, 0x00, 0x12, 0x01]),
            vec![Inbound::Observation(Observation::RitOffset(-1200))]
        );
        assert_eq!(
            seen(&mut e, 0x21, &[0x01, 0x01]),
            vec![Inbound::Observation(Observation::Rit(true))]
        );
        assert_eq!(
            seen(&mut e, 0x21, &[0x02, 0x00]),
            vec![Inbound::Observation(Observation::Xit(false))]
        );
        assert_eq!(
            seen(&mut e, 0x14, &[0x0A, 0x01, 0x28]),
            vec![Inbound::Observation(Observation::Level(
                Level::RfPower,
                128
            ))]
        );
        assert_eq!(
            seen(&mut e, 0x19, &[0x00, 0xA2]),
            vec![Inbound::Observation(Observation::ModelId(0xA2))]
        );
        for bad in [
            (0x14u8, vec![0x01, 0x03, 0x00]),
            (0x14, vec![0x77, 0x00, 0x10]),
            (0x25, vec![0x07, 0x00]),
        ] {
            assert!(matches!(
                seen(&mut e, bad.0, &bad.1).as_slice(),
                [Inbound::Unrecognised(_)]
            ));
        }
    }

    // Tests [@ANCHOR: radioctl_civ_engine_scope_request]
    // Tests [@ANCHOR: radioctl_civ_engine_scope_request_for]
    // Tests [@ANCHOR: radioctl_civ_engine_scope_dropped]
    // Tests [@ANCHOR: radioctl_civ_engine_decode_scope]
    #[test]
    fn scope_frames_become_lines_and_never_surface_as_unrecognised() {
        use crate::civ_scope::{ScopeCommand, ScopeMode};
        use crate::virtual_rig::VirtualRig;
        let desc = shipped("icom-ic-7610");
        let mut e = CivEngine::new(desc.clone()).expect("engine");
        let mut rig = VirtualRig::from_description(&desc).expect("rig");
        let on = e
            .scope_request(ScopeCommand::SetOutput(true))
            .expect("request");
        assert_eq!(on.reply, ReplyKind::Ack);
        assert_eq!(
            on.bytes,
            vec![0xFE, 0xFE, 0x98, 0xE0, 0x27, 0x11, 0x01, 0xFD]
        );
        assert_eq!(rig.feed(&on.bytes, T0), reply(0x98, OK, &[]));
        let query = e.scope_request(ScopeCommand::GetMode(1)).expect("query");
        assert_eq!(query.reply, ReplyKind::None);
        let sweep = rig.scope_sweep();
        let seen = e.feed(&sweep, T0);
        let [Inbound::ScopeFrame(frame)] = seen.as_slice() else {
            panic!("one scope frame and nothing unrecognised, got {seen:?}");
        };
        assert_eq!(frame.samples.len(), 689);
        assert_eq!(ScopeMode::from(frame.axis), ScopeMode::Center);
        assert!(
            e.feed(&sweep, T0).len() == 1,
            "the next sweep is another frame"
        );
        let mut broken = sweep.clone();
        let cut = broken.len() / 2;
        broken.drain(..cut);
        assert!(
            !e.feed(&broken, T0)
                .iter()
                .any(|i| matches!(i, Inbound::ScopeFrame(_))),
            "half a sweep is never delivered"
        );
        e.feed(&reply(0x98, 0x27, &[0x00, 0x05]), T0);
        assert!(e.scope_dropped() >= 1);
        assert!(e.scope_request(ScopeCommand::GetSpan(2)).is_err());
        let plain = CivEngine::new(RigDescription::ic7300()).expect("engine");
        assert!(plain.scope_request(ScopeCommand::GetDisplay).is_err());
    }

    // Tests [@ANCHOR: radioctl_civ_engine_decode]
    // Tests [@ANCHOR: radioctl_civ_engine_encode]
    #[test]
    fn a_ptt_echo_acknowledges_a_ptt_set_but_not_a_ptt_read() {
        let mut e = CivEngine::new(shipped("xiegu-g90")).expect("engine");
        e.encode(&Op::SetPtt(false)).expect("set");
        assert_eq!(
            e.feed(&reply(0xA4, 0x1C, &[0x00, 0x00]), T0),
            vec![Inbound::Ack],
            "the Xiegu echo of the set"
        );
        assert_eq!(
            e.feed(&reply(0xA4, 0x1C, &[0x00, 0x00]), T0),
            vec![Inbound::Observation(Observation::Ptt(false))],
            "the quirk is for one reply only"
        );
        e.encode(&Op::SetPtt(true)).expect("set");
        assert_eq!(e.feed(&reply(0xA4, OK, &[]), T0), vec![Inbound::Ack]);
        e.encode(&Op::GetPtt).expect("get");
        assert_eq!(
            e.feed(&reply(0xA4, 0x1C, &[0x00, 0x01]), T0),
            vec![Inbound::Observation(Observation::Ptt(true))]
        );
    }
}

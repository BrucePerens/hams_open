// SPDX-License-Identifier: AGPL-3.0-or-later
//! Icom CI-V: frame codec, sans-IO client engine.
//!
//! Frame: `FE FE <to> <from> <cmd> [data] FD`. A reply either echoes the command byte with data
//! or is `FB` (OK) / `FA` (NG). Frequencies are 5 bytes of BCD, least-significant pair first
//! (14,074,000 Hz is `00 40 07 14 00`, the capture recorded in `tools/rig_simulator.py`).
//! Meter values are 2 bytes of BCD (0000 to 9999; radios use 0000 to 0255). Sources: our own
//! `tools/rig_simulator.py` and the relay's `civ_bridge.rs`; no radio has verified this engine.

use std::time::Duration;

use radioctl_core::description::{CivParams, Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request, PARTIAL_FRAME_TIMEOUT};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Vfo};

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
        Ok(CivEngine {
            desc,
            params,
            parser: FrameParser::default(),
        })
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

    fn unsupported(what: &str) -> RigError {
        RigError::new(ErrorKind::Unsupported, what.to_string())
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
        let ack = ReplyKind::Ack;
        let request = match op {
            Op::GetFrequency(Vfo::A) => {
                self.command(0x03, &[], ReplyKind::Observation(Field::Frequency(Vfo::A)))
            }
            Op::GetFrequency(Vfo::B) | Op::SetFrequency(Vfo::B, _) => {
                return Err(Self::unsupported("CI-V engine: VFO B is not implemented"));
            }
            Op::SetFrequency(Vfo::A, hz) => {
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
            Op::SetTransceive(_) => {
                return Err(Self::unsupported(
                    "CI-V transceive is set in the rig's menu",
                ));
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
                        out.push(event);
                    }
                }
            }
        }
        out
    }
}

impl CivEngine {
    /// Decodes one frame; `None` for our own echoed frames and frames for other stations.
    fn decode(&self, frame: &Frame) -> Option<Inbound> {
        if frame.from == self.params.controller {
            return None;
        }
        let for_us = frame.to == self.params.controller || frame.to == BROADCAST;
        if !for_us || frame.from != self.params.address {
            return None;
        }
        let data = frame.data.as_slice();
        let observation = match (frame.cmd, data) {
            (OK, []) => return Some(Inbound::Ack),
            (NG, []) => return Some(Inbound::Nak),
            (0x00 | 0x03, bcd) => decode_bcd_freq(bcd).map(|hz| Observation::Frequency(Vfo::A, hz)),
            (0x01 | 0x04, [code, ..]) => self.desc.code_mode(*code).map(Observation::Mode),
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
        assert!(engine.encode(&Op::SetTransceive(true)).is_err());
    }

    // Tests [@ANCHOR: radioctl_civ_engine_feed]
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
}

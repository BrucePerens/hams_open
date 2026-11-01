// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ten-Tec serial protocols: sans-IO client engine and virtual twin, one file, one dialect per
//! model family (`vendor.dialect` in the description).
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/tentec/orion.c`,
//! `orion.h` (Orion TT-565/566 and Eagle TT-599), `tentec2.c` (Argonaut V TT-516), `jupiter.c`
//! (TT-538) and `omnivii.c` (TT-588). No vendor protocol document was read; nothing here is
//! verified against a radio.
//!
//! * `orion`: ASCII lines ended by CR. `*AF<hz>` sets VFO A (`B` for VFO B) with no reply;
//!   `?AF` is answered `@AF<digits>`. Mode is on the main receiver: `*RMM<c>` and `?RMM` answered
//!   `@RMM<c>`, with `c` an ASCII digit (0 USB, 1 LSB, 2 CW, 3 CW-R, 4 AM, 5 FM, 6 RTTY, from the
//!   description). Hamlib also sends `*RMF<width>` after a mode change; this engine does not.
//!   PTT is `*TK` (key) and `*TU` (unkey); there is no read. `Z!` refuses a command.
//! * `argonaut`: binary frequency, CR-ended. `*A` plus four bytes (big-endian hertz) and CR is
//!   answered `G` CR (or `Z` CR for a refusal); `?A` CR is answered `A`, four bytes, CR, `G`, CR.
//!   Mode: `?M` CR answered `M<a><b>` CR `G` CR (a for VFO A, b for VFO B, ASCII digits), set by
//!   `*M<a><b>` CR. PTT: `#1` / `#0` CR answered `G` CR; `?C` CR answered `C<2 bytes>` CR `G` CR,
//!   transmit when bit 0 of the second status byte is set.
//! * `jupiter` (TT-538) and `omnivii` (TT-588): the same frames without the `G` replies on
//!   writes; reads are `A`, four bytes, CR (six bytes) and `M<a><b>` CR. PTT is `Q1` / `Q0` CR on
//!   the Jupiter and `*T` 04 01 CR / `*T` 00 01 CR on the Omni VII.
//!
//! The mode command writes both VFOs' modes; the engine keeps VFO B's from the last read and
//! until then repeats the new mode for B (a documented compromise).

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Mode, Vfo};

use crate::serial_vendor::{parse_decimal, vendor_of, RxBuffer, VendorTwin};

/// Carriage return, the end of every Ten-Tec command and reply line.
const CR: u8 = b'\r';

/// The Ten-Tec model families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// Orion TT-565/566 and Eagle TT-599: ASCII.
    Orion,
    /// Argonaut V TT-516: binary with `G` acknowledgements.
    Argonaut,
    /// Jupiter TT-538: binary, no acknowledgements.
    Jupiter,
    /// Omni VII TT-588: binary, no acknowledgements, its own PTT frame.
    OmniVii,
}

impl Dialect {
    // [@ANCHOR: radioctl_tentec_dialect_from_name]
    /// The dialect with a file-format name.
    pub fn from_name(name: &str) -> Option<Dialect> {
        match name {
            "orion" => Some(Dialect::Orion),
            "argonaut" => Some(Dialect::Argonaut),
            "jupiter" => Some(Dialect::Jupiter),
            "omnivii" => Some(Dialect::OmniVii),
            _ => None,
        }
    }

    fn acknowledges(self) -> bool {
        self == Dialect::Argonaut
    }
}

// [@ANCHOR: radioctl_tentec_vfo_letter]
/// The letter of a VFO in Ten-Tec commands.
pub fn vfo_letter(vfo: Vfo) -> u8 {
    match vfo {
        Vfo::A => b'A',
        Vfo::B => b'B',
    }
}

// [@ANCHOR: radioctl_tentec_encode_frequency]
/// The four big-endian bytes of the binary dialects, or `None` above `u32::MAX` hertz.
pub fn encode_frequency(hz: Hz) -> Option<[u8; 4]> {
    u32::try_from(hz.0).ok().map(u32::to_be_bytes)
}

// [@ANCHOR: radioctl_tentec_decode_frequency]
/// The frequency of four big-endian bytes, or `None` unless exactly four.
pub fn decode_frequency(bytes: &[u8]) -> Option<Hz> {
    let four: [u8; 4] = bytes.try_into().ok()?;
    Some(Hz(u64::from(u32::from_be_bytes(four))))
}

/// What the engine waits for after a read or an acknowledged write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Frequency(Vfo),
    Mode,
    Ptt,
    Ack,
}

/// The Ten-Tec client engine for one rig.
#[derive(Debug)]
pub struct TentecEngine {
    desc: RigDescription,
    dialect: Dialect,
    pending: Pending,
    rx: RxBuffer,
    /// The two mode characters (VFO A, VFO B) of the last `M` reply.
    modes: Option<(u8, u8)>,
}

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

impl TentecEngine {
    // [@ANCHOR: radioctl_tentec_engine_new]
    /// Builds an engine from a Ten-Tec vendor description.
    pub fn new(desc: RigDescription) -> Result<TentecEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        let dialect = Dialect::from_name(name)
            .ok_or_else(|| unsupported("unknown Ten-Tec dialect in the description"))?;
        Ok(TentecEngine {
            desc,
            dialect,
            pending: Pending::Nothing,
            rx: RxBuffer::default(),
            modes: None,
        })
    }

    fn request(bytes: Vec<u8>, reply: ReplyKind) -> Request {
        Request { bytes, reply }
    }

    fn write_reply(&self) -> ReplyKind {
        if self.dialect.acknowledges() {
            ReplyKind::Ack
        } else {
            ReplyKind::None
        }
    }

    // [@ANCHOR: radioctl_tentec_encode_mode_write]
    fn encode_mode_write(&self, mode: Mode) -> Result<Vec<u8>, RigError> {
        let code = self
            .desc
            .mode_code(mode)
            .ok_or_else(|| unsupported("mode not in the description"))?;
        Ok(match self.dialect {
            Dialect::Orion => format!("*RMM{}\r", char::from(code)).into_bytes(),
            _ => {
                let other = self.modes.map_or(code, |(_, b)| b);
                vec![b'*', b'M', code, other, CR]
            }
        })
    }

    // [@ANCHOR: radioctl_tentec_encode_ptt]
    fn encode_ptt(&self, on: bool) -> Result<Vec<u8>, RigError> {
        Ok(match self.dialect {
            Dialect::Orion => {
                if on {
                    b"*TK\r".to_vec()
                } else {
                    b"*TU\r".to_vec()
                }
            }
            Dialect::Argonaut => {
                if on {
                    b"#1\r".to_vec()
                } else {
                    b"#0\r".to_vec()
                }
            }
            Dialect::Jupiter => {
                if on {
                    b"Q1\r".to_vec()
                } else {
                    b"Q0\r".to_vec()
                }
            }
            Dialect::OmniVii => vec![b'*', b'T', if on { 4 } else { 0 }, 1, CR],
        })
    }

    // [@ANCHOR: radioctl_tentec_frame_len]
    /// Bytes in a binary-dialect answer.
    fn frame_len(&self, pending: Pending) -> usize {
        match (pending, self.dialect) {
            (Pending::Frequency(_), Dialect::Argonaut) => 8,
            (Pending::Frequency(_), _) => 6,
            (Pending::Mode, Dialect::Argonaut) => 6,
            (Pending::Mode, _) => 4,
            (Pending::Ptt, _) => 6,
            (Pending::Ack, _) => 2,
            (Pending::Nothing, _) => 0,
        }
    }

    // [@ANCHOR: radioctl_tentec_decode_binary]
    fn decode_binary(&mut self, pending: Pending, frame: &[u8]) -> Vec<Inbound> {
        let bad = || vec![Inbound::Unrecognised(frame.to_vec())];
        let tail_ok =
            !self.dialect.acknowledges() || frame.ends_with(b"\rG\r") || pending == Pending::Ack;
        match (pending, frame) {
            (Pending::Ack, b"G\r") => vec![Inbound::Ack],
            (Pending::Frequency(vfo), [letter, f @ .., CR]) if tail_ok => {
                let body = f.get(..4).unwrap_or(&[]);
                match (*letter == vfo_letter(vfo), decode_frequency(body)) {
                    (true, Some(hz)) => vec![Inbound::Observation(Observation::Frequency(vfo, hz))],
                    _ => bad(),
                }
            }
            (Pending::Mode, [b'M', a, b, CR, ..]) if tail_ok => match self.desc.code_mode(*a) {
                Some(mode) => {
                    self.modes = Some((*a, *b));
                    vec![Inbound::Observation(Observation::Mode(mode))]
                }
                None => bad(),
            },
            (Pending::Ptt, [b'C', _, status, CR, b'G', CR]) => {
                vec![Inbound::Observation(Observation::Ptt(status & 1 == 1))]
            }
            _ => bad(),
        }
    }

    // [@ANCHOR: radioctl_tentec_decode_orion_line]
    fn decode_orion_line(&mut self, pending: Pending, line: &[u8]) -> Vec<Inbound> {
        if line.starts_with(b"Z!") {
            return vec![Inbound::Nak];
        }
        match (pending, line) {
            (Pending::Frequency(vfo), [b'@', letter, b'F', digits @ ..])
                if *letter == vfo_letter(vfo) =>
            {
                match parse_decimal(digits) {
                    Some(hz) => vec![Inbound::Observation(Observation::Frequency(vfo, Hz(hz)))],
                    None => vec![Inbound::Unrecognised(line.to_vec())],
                }
            }
            (Pending::Mode, [b'@', b'R', b'M', b'M', code]) => match self.desc.code_mode(*code) {
                Some(mode) => vec![Inbound::Observation(Observation::Mode(mode))],
                None => vec![Inbound::Unrecognised(line.to_vec())],
            },
            _ => vec![Inbound::Unrecognised(line.to_vec())],
        }
    }
}

impl ProtocolEngine for TentecEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_tentec_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let orion = self.dialect == Dialect::Orion;
        let (request, pending) = match op {
            Op::GetFrequency(vfo) => {
                let bytes = if orion {
                    vec![b'?', vfo_letter(*vfo), b'F', CR]
                } else {
                    vec![b'?', vfo_letter(*vfo), CR]
                };
                (
                    Self::request(bytes, ReplyKind::Observation(Field::Frequency(*vfo))),
                    Pending::Frequency(*vfo),
                )
            }
            Op::SetFrequency(vfo, hz) => {
                if orion {
                    let text = format!("*{}F{}\r", char::from(vfo_letter(*vfo)), hz.0);
                    (
                        Self::request(text.into_bytes(), ReplyKind::None),
                        Pending::Nothing,
                    )
                } else {
                    let be = encode_frequency(*hz)
                        .ok_or_else(|| unsupported("frequency does not fit four bytes"))?;
                    let mut bytes = vec![b'*', vfo_letter(*vfo)];
                    bytes.extend_from_slice(&be);
                    bytes.push(CR);
                    let reply = self.write_reply();
                    let pending = if reply == ReplyKind::Ack {
                        Pending::Ack
                    } else {
                        Pending::Nothing
                    };
                    (Self::request(bytes, reply), pending)
                }
            }
            Op::GetMode => {
                let bytes = if orion {
                    b"?RMM\r".to_vec()
                } else {
                    b"?M\r".to_vec()
                };
                (
                    Self::request(bytes, ReplyKind::Observation(Field::Mode)),
                    Pending::Mode,
                )
            }
            Op::SetMode(mode) => {
                let bytes = self.encode_mode_write(*mode)?;
                let reply = if orion {
                    ReplyKind::None
                } else {
                    self.write_reply()
                };
                let pending = if reply == ReplyKind::Ack {
                    Pending::Ack
                } else {
                    Pending::Nothing
                };
                (Self::request(bytes, reply), pending)
            }
            Op::SetPtt(on) => {
                let bytes = self.encode_ptt(*on)?;
                let reply = if orion {
                    ReplyKind::None
                } else {
                    self.write_reply()
                };
                let pending = if reply == ReplyKind::Ack {
                    Pending::Ack
                } else {
                    Pending::Nothing
                };
                (Self::request(bytes, reply), pending)
            }
            Op::GetPtt if self.dialect == Dialect::Argonaut => (
                Self::request(b"?C\r".to_vec(), ReplyKind::Observation(Field::Ptt)),
                Pending::Ptt,
            ),
            _ => return Err(unsupported("not implemented for the Ten-Tec engine")),
        };
        self.pending = pending;
        self.rx.take_all();
        Ok(request)
    }

    // [@ANCHOR: radioctl_tentec_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        if self.dialect == Dialect::Orion {
            while let Some(line) = self.rx.take_line(CR) {
                let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
                out.extend(self.decode_orion_line(pending, &line));
            }
            return out;
        }
        if self.pending == Pending::Nothing {
            let junk = self.rx.take_all();
            if !junk.is_empty() {
                out.push(Inbound::Unrecognised(junk));
            }
            return out;
        }
        if self.dialect.acknowledges() && self.rx.peek() == b"Z\r" {
            self.rx.take_all();
            self.pending = Pending::Nothing;
            out.push(Inbound::Nak);
            return out;
        }
        let need = self.frame_len(self.pending);
        if let Some(frame) = self.rx.take(need) {
            let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
            out.extend(self.decode_binary(pending, &frame));
            let rest = self.rx.take_all();
            if !rest.is_empty() {
                out.push(Inbound::Unrecognised(rest));
            }
        }
        out
    }
}

/// A virtual Ten-Tec radio of one dialect.
#[derive(Debug)]
pub struct TentecTwin {
    dialect: Dialect,
    codes: Vec<u8>,
    rx: RxBuffer,
    freq: [u64; 2],
    mode: [u8; 2],
    ptt: bool,
}

impl TentecTwin {
    // [@ANCHOR: radioctl_tentec_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<TentecTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        let dialect = Dialect::from_name(name)
            .ok_or_else(|| unsupported("unknown Ten-Tec dialect in the description"))?;
        let codes: Vec<u8> = desc.mode_codes.iter().map(|(_, c)| *c).collect();
        let default = desc
            .mode_code(desc.default_mode)
            .or_else(|| codes.first().copied());
        Ok(TentecTwin {
            dialect,
            codes,
            rx: RxBuffer::default(),
            freq: [desc.default_frequency.0; 2],
            mode: [default.unwrap_or(b'0'); 2],
            ptt: false,
        })
    }

    fn slot(letter: u8) -> Option<usize> {
        match letter {
            b'A' => Some(0),
            b'B' => Some(1),
            _ => None,
        }
    }

    // [@ANCHOR: radioctl_tentec_twin_orion_line]
    fn orion_line(&mut self, line: &[u8]) -> Vec<u8> {
        let refuse = b"Z!\r".to_vec();
        match line {
            [b'?', l, b'F'] => match Self::slot(*l) {
                Some(i) => format!("@{}F{:08}\r", char::from(*l), self.freq[i]).into_bytes(),
                None => refuse,
            },
            [b'*', l, b'F', digits @ ..] => match (Self::slot(*l), parse_decimal(digits)) {
                (Some(i), Some(hz)) => {
                    self.freq[i] = hz;
                    Vec::new()
                }
                _ => refuse,
            },
            b"?RMM" => format!("@RMM{}\r", char::from(self.mode[0])).into_bytes(),
            [b'*', b'R', b'M', b'M', c] if self.codes.contains(c) => {
                self.mode = [*c; 2];
                Vec::new()
            }
            b"*TK" => {
                self.ptt = true;
                Vec::new()
            }
            b"*TU" => {
                self.ptt = false;
                Vec::new()
            }
            _ => refuse,
        }
    }

    // [@ANCHOR: radioctl_tentec_twin_binary_frame]
    /// Handles one binary command at the front of the buffer; `None` when more bytes are needed.
    fn binary_frame(&mut self) -> Option<Vec<u8>> {
        let g: &[u8] = if self.dialect.acknowledges() {
            b"G\r"
        } else {
            b""
        };
        let first = *self.rx.peek().first()?;
        let need = match (first, self.rx.peek().get(1)) {
            (b'*', Some(b'A' | b'B')) => 7,
            (b'*', Some(b'M' | b'T')) => 5,
            (b'*', None) => return None,
            _ => 3,
        };
        let frame = self.rx.take(need)?;
        let mut reply = Vec::new();
        match frame.as_slice() {
            [b'?', l @ (b'A' | b'B'), CR] => {
                let i = Self::slot(*l).unwrap_or(0);
                reply.push(*l);
                reply.extend_from_slice(&(self.freq[i] as u32).to_be_bytes());
                reply.push(CR);
                reply.extend_from_slice(g);
            }
            [b'*', l @ (b'A' | b'B'), f @ .., CR] => {
                if let (Some(i), Some(hz)) = (Self::slot(*l), decode_frequency(f)) {
                    self.freq[i] = hz.0;
                    reply.extend_from_slice(g);
                } else {
                    reply.extend_from_slice(b"Z\r");
                }
            }
            [b'?', b'M', CR] => {
                reply.extend_from_slice(&[b'M', self.mode[0], self.mode[1], CR]);
                reply.extend_from_slice(g);
            }
            [b'*', b'M', a, b, CR] if self.codes.contains(a) && self.codes.contains(b) => {
                self.mode = [*a, *b];
                reply.extend_from_slice(g);
            }
            [b'?', b'C', CR] if self.dialect == Dialect::Argonaut => {
                reply.extend_from_slice(&[b'C', 0, u8::from(self.ptt), CR]);
                reply.extend_from_slice(g);
            }
            [b'#', c @ (b'0' | b'1'), CR] if self.dialect == Dialect::Argonaut => {
                self.ptt = *c == b'1';
                reply.extend_from_slice(g);
            }
            [b'Q', c @ (b'0' | b'1'), CR] if self.dialect == Dialect::Jupiter => {
                self.ptt = *c == b'1';
            }
            [b'*', b'T', k, 1, CR] if self.dialect == Dialect::OmniVii && matches!(k, 0 | 4) => {
                self.ptt = *k == 4;
            }
            _ if self.dialect.acknowledges() => reply.extend_from_slice(b"Z\r"),
            _ => {}
        }
        Some(reply)
    }
}

impl VendorTwin for TentecTwin {
    // [@ANCHOR: radioctl_tentec_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        if self.dialect == Dialect::Orion {
            while let Some(line) = self.rx.take_line(CR) {
                out.extend(self.orion_line(&line));
            }
        } else {
            while let Some(reply) = self.binary_frame() {
                out.extend(reply);
            }
        }
        out
    }

    // [@ANCHOR: radioctl_tentec_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.freq[0] = hz.0,
            Panel::Ptt(on) => self.ptt = on,
            Panel::Mode(_) => {
                return Err("the Ten-Tec twin takes mode changes over the wire".into())
            }
            other => return Err(format!("a Ten-Tec twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    fn exchange(e: &mut TentecEngine, t: &mut TentecTwin, op: &Op) -> Vec<Inbound> {
        let request = e.encode(op).expect("encodes");
        let answer = t.feed(&request.bytes, T0);
        e.feed(&answer, T0)
    }

    // Tests [@ANCHOR: radioctl_tentec_dialect_from_name]
    // Tests [@ANCHOR: radioctl_tentec_vfo_letter]
    // Tests [@ANCHOR: radioctl_tentec_encode_frequency]
    // Tests [@ANCHOR: radioctl_tentec_decode_frequency]
    #[test]
    fn dialects_and_binary_frequency() {
        assert_eq!(Dialect::from_name("orion"), Some(Dialect::Orion));
        assert_eq!(Dialect::from_name("omnivii"), Some(Dialect::OmniVii));
        assert_eq!(Dialect::from_name("nope"), None);
        assert_eq!(vfo_letter(Vfo::B), b'B');
        assert_eq!(
            encode_frequency(Hz(14_200_000)),
            Some([0x00, 0xD8, 0xAC, 0xC0])
        );
        assert_eq!(encode_frequency(Hz(5_000_000_000)), None);
        assert_eq!(
            decode_frequency(&[0x00, 0xD8, 0xAC, 0xC0]),
            Some(Hz(14_200_000))
        );
        assert_eq!(decode_frequency(&[0, 1, 2]), None);
    }

    // Tests [@ANCHOR: radioctl_tentec_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        assert!(TentecEngine::new(RigDescription::ts2000()).is_err());
        assert!(TentecTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_tentec_engine_encode]
    // Tests [@ANCHOR: radioctl_tentec_encode_mode_write]
    // Tests [@ANCHOR: radioctl_tentec_encode_ptt]
    #[test]
    fn orion_commands_are_the_cr_ended_ascii_lines() {
        let mut e = TentecEngine::new(shipped("tentec-tt-565")).expect("engine");
        let b = |e: &mut TentecEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        assert_eq!(b(&mut e, Op::GetFrequency(Vfo::A)), b"?AF\r");
        assert_eq!(
            b(&mut e, Op::SetFrequency(Vfo::B, Hz(7_074_000))),
            b"*BF7074000\r"
        );
        assert_eq!(b(&mut e, Op::GetMode), b"?RMM\r");
        assert_eq!(b(&mut e, Op::SetMode(Mode::Cw)), b"*RMM2\r");
        assert_eq!(b(&mut e, Op::SetPtt(true)), b"*TK\r");
        assert_eq!(b(&mut e, Op::SetPtt(false)), b"*TU\r");
        assert!(e.encode(&Op::GetPtt).is_err());
        assert!(e.encode(&Op::SetMode(Mode::Wfm)).is_err());
        assert!(e.encode(&Op::SendCw("x".into())).is_err());
    }

    // Tests [@ANCHOR: radioctl_tentec_engine_encode]
    // Tests [@ANCHOR: radioctl_tentec_encode_mode_write]
    // Tests [@ANCHOR: radioctl_tentec_encode_ptt]
    #[test]
    fn binary_commands_are_star_letter_four_bytes_cr() {
        let mut e = TentecEngine::new(shipped("tentec-tt-516")).expect("engine");
        let b = |e: &mut TentecEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        assert_eq!(b(&mut e, Op::GetFrequency(Vfo::A)), b"?A\r");
        assert_eq!(
            b(&mut e, Op::SetFrequency(Vfo::A, Hz(14_200_000))),
            [b'*', b'A', 0x00, 0xD8, 0xAC, 0xC0, b'\r']
        );
        assert_eq!(b(&mut e, Op::SetMode(Mode::Usb)), b"*M11\r");
        assert_eq!(b(&mut e, Op::SetPtt(true)), b"#1\r");
        assert_eq!(b(&mut e, Op::GetPtt), b"?C\r");
        let mut j = TentecEngine::new(shipped("tentec-tt-538")).expect("engine");
        assert_eq!(b(&mut j, Op::SetPtt(true)), b"Q1\r");
        assert_eq!(
            j.encode(&Op::SetPtt(false)).expect("e").reply,
            ReplyKind::None
        );
        let mut o = TentecEngine::new(shipped("tentec-tt-588")).expect("engine");
        assert_eq!(b(&mut o, Op::SetPtt(true)), [b'*', b'T', 4, 1, b'\r']);
        assert_eq!(b(&mut o, Op::SetMode(Mode::CwR)), b"*M55\r");
        assert!(e
            .encode(&Op::SetFrequency(Vfo::A, Hz(5_000_000_000)))
            .is_err());
    }

    // Tests [@ANCHOR: radioctl_tentec_engine_feed]
    // Tests [@ANCHOR: radioctl_tentec_decode_orion_line]
    // Tests [@ANCHOR: radioctl_tentec_decode_binary]
    // Tests [@ANCHOR: radioctl_tentec_frame_len]
    // Tests [@ANCHOR: radioctl_tentec_twin_new]
    // Tests [@ANCHOR: radioctl_tentec_twin_feed]
    // Tests [@ANCHOR: radioctl_tentec_twin_orion_line]
    // Tests [@ANCHOR: radioctl_tentec_twin_binary_frame]
    #[test]
    fn every_dialect_round_trips_against_its_twin() {
        for id in [
            "tentec-tt-565",
            "tentec-tt-599",
            "tentec-tt-516",
            "tentec-tt-538",
            "tentec-tt-588",
        ] {
            let desc = shipped(id);
            let mut e = TentecEngine::new(desc.clone()).expect("engine");
            let mut t = TentecTwin::new(&desc).expect("twin");
            let set = exchange(&mut e, &mut t, &Op::SetFrequency(Vfo::A, Hz(14_195_000)));
            assert!(
                set.iter().all(|i| matches!(i, Inbound::Ack)),
                "{id}: {set:?}"
            );
            assert_eq!(
                exchange(&mut e, &mut t, &Op::GetFrequency(Vfo::A)),
                vec![Inbound::Observation(Observation::Frequency(
                    Vfo::A,
                    Hz(14_195_000)
                ))],
                "{id}"
            );
            exchange(&mut e, &mut t, &Op::SetFrequency(Vfo::B, Hz(3_573_000)));
            assert_eq!(
                exchange(&mut e, &mut t, &Op::GetFrequency(Vfo::B)),
                vec![Inbound::Observation(Observation::Frequency(
                    Vfo::B,
                    Hz(3_573_000)
                ))],
                "{id}"
            );
            exchange(&mut e, &mut t, &Op::SetMode(Mode::Lsb));
            assert_eq!(
                exchange(&mut e, &mut t, &Op::GetMode),
                vec![Inbound::Observation(Observation::Mode(Mode::Lsb))],
                "{id}"
            );
            exchange(&mut e, &mut t, &Op::SetPtt(true));
            assert!(t.ptt, "{id}");
            if id == "tentec-tt-516" {
                assert_eq!(
                    exchange(&mut e, &mut t, &Op::GetPtt),
                    vec![Inbound::Observation(Observation::Ptt(true))]
                );
            }
            exchange(&mut e, &mut t, &Op::SetPtt(false));
            assert!(!t.ptt, "{id}");
        }
    }

    // Tests [@ANCHOR: radioctl_tentec_engine_feed]
    // Tests [@ANCHOR: radioctl_tentec_twin_feed]
    #[test]
    fn refusals_partial_frames_and_junk_are_reported() {
        let mut a = TentecEngine::new(shipped("tentec-tt-516")).expect("engine");
        a.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(a.feed(b"A\x00\xD8", T0).is_empty(), "partial");
        let later = T0 + radioctl_core::engine::PARTIAL_FRAME_TIMEOUT + Duration::from_millis(1);
        let out = a.feed(b"Z\r", later);
        assert!(matches!(out.first(), Some(Inbound::Unrecognised(stale)) if stale == b"A\x00\xD8"));
        assert_eq!(out.get(1), Some(&Inbound::Nak));
        assert!(
            matches!(a.feed(b"x", T0).as_slice(), [Inbound::Unrecognised(_)]),
            "unasked"
        );
        let mut o = TentecEngine::new(shipped("tentec-tt-565")).expect("engine");
        o.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(o.feed(b"Z!\r", T0), vec![Inbound::Nak]);
        o.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(matches!(
            o.feed(b"@BF00001000\r", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        o.encode(&Op::GetMode).expect("get");
        assert!(matches!(
            o.feed(b"@RMM9\r", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        let mut t = TentecTwin::new(&shipped("tentec-tt-565")).expect("twin");
        assert_eq!(t.feed(b"?XX\r", T0), b"Z!\r");
        let mut t5 = TentecTwin::new(&shipped("tentec-tt-516")).expect("twin");
        assert_eq!(t5.feed(b"!!\r", T0), b"Z\r");
    }

    // Tests [@ANCHOR: radioctl_tentec_twin_front_panel]
    #[test]
    fn the_front_panel_moves_the_twin() {
        let desc = shipped("tentec-tt-565");
        let mut t = TentecTwin::new(&desc).expect("twin");
        t.front_panel(Panel::Frequency(Hz(10_136_000)))
            .expect("tunes");
        assert_eq!(t.feed(b"?AF\r", T0), b"@AF10136000\r");
        t.front_panel(Panel::Ptt(true)).expect("keys");
        assert!(t.ptt);
        assert!(t.front_panel(Panel::Split(true)).is_err());
        assert!(t.front_panel(Panel::Mode(Mode::Cw)).is_err());
    }
}

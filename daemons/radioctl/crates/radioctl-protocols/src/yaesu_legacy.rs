// SPDX-License-Identifier: AGPL-3.0-or-later
//! Yaesu legacy CAT (FT-817, FT-857, FT-897): five-byte binary blocks, sans-IO client engine.
//!
//! Every command is five bytes, four parameters then the opcode; one start bit, eight data bits,
//! no parity, two stop bits, at most 200 ms between bytes. The radio sends nothing back for a
//! setting; a read answers with a fixed number of bytes and no framing at all, so the engine
//! remembers which answer it is waiting for and counts bytes.
//!
//! **Sources.** Yaesu's published operating manuals, "CAT System Programming" and the opcode
//! chart: FT-817ND (`ft817nd_manual.pdf`, sha256 1d87424a7b09ee6aae96fc3043525119bc133916144e9cbb808624da236fdf27, PDF pages 74 and 75,
//! printed pages 72 and 73), FT-857D (`ft857d_manual.pdf`, sha256 64b49ba7cd49f3d72eea9cece3288a5eeb833c2c5f46efafda361c20d3bf4e18, PDF
//! pages 119 and 120, printed 117) and FT-897D (`ft897d_manual.pdf`, sha256
//! b7a3ce382fd39bb31e05fc17ecbe0770083d5b6387cec8e2deeb3412560c6176, PDF page 66, printed page 64). The notes under the chart are
//! figures; they were read from rendered pages of the FT-817ND and FT-897D (the same layout):
//!
//! * `03` (read frequency and mode): five bytes, the first four the frequency in BCD, two digits
//!   per byte, most significant first, in units of 10 Hz (`43 97 00 00` is 439.700 MHz), the fifth
//!   the mode.
//! * `E7` (read RX status): one byte; bits 0 to 3 the S-meter, bit 7 squelch.
//! * `F7` (read TX status): one byte; bits 0 to 3 the power meter, bit 5 split (0 is ON, 1 is
//!   OFF), bit 6 high SWR, bit 7 PTT. The figure does not say which PTT level is transmit.
//! * `01` set frequency (four BCD bytes), `07` set mode, `08` and `88` PTT on and off, `02` and
//!   `82` split on and off, `05` and `85` clarifier on and off, `F5` clarifier frequency (a sign
//!   byte, a dummy, then two BCD bytes in 10 Hz units: `12 34` is 12.34 kHz).
//! * Modes: 00 LSB, 01 USB, 02 CW, 03 CW-R, 04 AM, 08 FM, 0A DIG, 0C PKT (the FT-857D and FT-897D
//!   add 88, narrow FM).
//!
//! The one fact the manuals leave out, the level of the PTT bit, was read from Hamlib 4.6.2
//! (`rigs/yaesu/ft817.c`, the comment on `tx_status`: "Highest bit 0 = PTT"): a clear bit 7 means
//! transmitting. That is a `hamlib-source` fact in the description and is unverified.
//!
//! Not implemented: the VFO A/B toggle (`81`, so no second VFO), lock, repeater offset, CTCSS and
//! DCS, power on and off, the clarifier frequency read (there is none), FM-N and PKT modes, and
//! the FT-818 (no manual was found; Hamlib treats it as an FT-817 derivative).

use std::time::Duration;

use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request, PARTIAL_FRAME_TIMEOUT};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Vfo};

/// Opcode: set frequency.
pub const OP_SET_FREQUENCY: u8 = 0x01;
/// Opcode: split on.
pub const OP_SPLIT_ON: u8 = 0x02;
/// Opcode: read frequency and mode.
pub const OP_READ_FREQUENCY: u8 = 0x03;
/// Opcode: clarifier on.
pub const OP_CLAR_ON: u8 = 0x05;
/// Opcode: set mode.
pub const OP_SET_MODE: u8 = 0x07;
/// Opcode: PTT on.
pub const OP_PTT_ON: u8 = 0x08;
/// Opcode: split off.
pub const OP_SPLIT_OFF: u8 = 0x82;
/// Opcode: clarifier off.
pub const OP_CLAR_OFF: u8 = 0x85;
/// Opcode: PTT off.
pub const OP_PTT_OFF: u8 = 0x88;
/// Opcode: read RX status.
pub const OP_READ_RX: u8 = 0xE7;
/// Opcode: read TX status.
pub const OP_READ_TX: u8 = 0xF7;
/// Opcode: clarifier frequency.
pub const OP_CLAR_FREQUENCY: u8 = 0xF5;
/// Largest frequency eight BCD digits of 10 Hz hold.
const MAX_HZ: u64 = 999_999_990;
/// Largest clarifier offset, in hertz (two BCD bytes of 10 Hz).
const MAX_CLAR_HZ: u16 = 9_990;

/// What the engine is waiting for after a read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    FrequencyMode,
    RxStatus,
    TxStatus,
}

// [@ANCHOR: radioctl_ylegacy_encode_bcd_frequency]
/// The four BCD bytes of a frequency in 10 Hz units, or `None` when it is not a multiple of 10 Hz
/// or above 999,999,990 Hz.
pub fn encode_bcd_frequency(hz: Hz) -> Option<[u8; 4]> {
    if hz.0 > MAX_HZ || !hz.0.is_multiple_of(10) {
        return None;
    }
    let mut rest = hz.0 / 10;
    let mut out = [0u8; 4];
    for byte in out.iter_mut().rev() {
        let pair = (rest % 100) as u8;
        *byte = ((pair / 10) << 4) | (pair % 10);
        rest /= 100;
    }
    Some(out)
}

// [@ANCHOR: radioctl_ylegacy_decode_bcd_frequency]
/// The frequency in hertz from four BCD bytes (10 Hz units), or `None` on a non-digit nibble.
pub fn decode_bcd_frequency(bytes: &[u8]) -> Option<Hz> {
    if bytes.len() != 4 {
        return None;
    }
    let mut total: u64 = 0;
    for byte in bytes {
        let (high, low) = (byte >> 4, byte & 15);
        if high > 9 || low > 9 {
            return None;
        }
        total = total * 100 + u64::from(high * 10 + low);
    }
    Some(Hz(total * 10))
}

/// The Yaesu legacy client engine for one rig.
#[derive(Debug)]
pub struct YaesuLegacyEngine {
    desc: RigDescription,
    pending: Pending,
    buffer: Vec<u8>,
    last_byte_at: Duration,
}

impl YaesuLegacyEngine {
    // [@ANCHOR: radioctl_ylegacy_engine_new]
    /// Builds an engine from a yaesu-legacy description.
    pub fn new(desc: RigDescription) -> Result<YaesuLegacyEngine, RigError> {
        if desc.family != Family::YaesuLegacy || desc.yaesu_legacy.is_none() {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "not a Yaesu legacy description",
            ));
        }
        Ok(YaesuLegacyEngine {
            desc,
            pending: Pending::Nothing,
            buffer: Vec::new(),
            last_byte_at: Duration::ZERO,
        })
    }

    fn unsupported(what: &str) -> RigError {
        RigError::new(ErrorKind::Unsupported, what.to_string())
    }

    fn block(params: [u8; 4], opcode: u8, reply: ReplyKind) -> Request {
        Request {
            bytes: vec![params[0], params[1], params[2], params[3], opcode],
            reply,
        }
    }

    // [@ANCHOR: radioctl_ylegacy_decode_frequency_mode]
    fn decode_frequency_mode(&self, bytes: &[u8]) -> Vec<Inbound> {
        let (freq, mode) = match bytes {
            [a, b, c, d, mode] => (decode_bcd_frequency(&[*a, *b, *c, *d]), *mode),
            _ => (None, 0),
        };
        match (freq, self.desc.code_mode(mode)) {
            (Some(hz), Some(mode)) => vec![
                Inbound::Observation(Observation::Frequency(Vfo::A, hz)),
                Inbound::Observation(Observation::Mode(mode)),
            ],
            _ => vec![Inbound::Unrecognised(bytes.to_vec())],
        }
    }
}

impl ProtocolEngine for YaesuLegacyEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_ylegacy_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(Self::unsupported("capability not in the rig description"));
        }
        let none = ReplyKind::None;
        let zero = [0u8; 4];
        let (request, pending) = match op {
            Op::GetFrequency(Vfo::A) => (
                Self::block(
                    zero,
                    OP_READ_FREQUENCY,
                    ReplyKind::Observation(Field::Frequency(Vfo::A)),
                ),
                Pending::FrequencyMode,
            ),
            Op::GetMode => (
                Self::block(zero, OP_READ_FREQUENCY, ReplyKind::Observation(Field::Mode)),
                Pending::FrequencyMode,
            ),
            Op::SetFrequency(Vfo::A, hz) => {
                let bcd = encode_bcd_frequency(*hz).ok_or_else(|| {
                    Self::unsupported("frequency must be a multiple of 10 Hz below 1 GHz")
                })?;
                (Self::block(bcd, OP_SET_FREQUENCY, none), Pending::Nothing)
            }
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| Self::unsupported("mode not in the description"))?;
                (
                    Self::block([code, 0, 0, 0], OP_SET_MODE, none),
                    Pending::Nothing,
                )
            }
            Op::SetPtt(on) => (
                Self::block(zero, if *on { OP_PTT_ON } else { OP_PTT_OFF }, none),
                Pending::Nothing,
            ),
            Op::GetPtt => (
                Self::block(zero, OP_READ_TX, ReplyKind::Observation(Field::Ptt)),
                Pending::TxStatus,
            ),
            Op::GetSplit => (
                Self::block(zero, OP_READ_TX, ReplyKind::Observation(Field::Split)),
                Pending::TxStatus,
            ),
            Op::GetPower => (
                Self::block(zero, OP_READ_TX, ReplyKind::Observation(Field::Power)),
                Pending::TxStatus,
            ),
            Op::GetSignal => (
                Self::block(zero, OP_READ_RX, ReplyKind::Observation(Field::Signal)),
                Pending::RxStatus,
            ),
            Op::SetSplit(on) => (
                Self::block(zero, if *on { OP_SPLIT_ON } else { OP_SPLIT_OFF }, none),
                Pending::Nothing,
            ),
            Op::SetRit(on) => (
                Self::block(zero, if *on { OP_CLAR_ON } else { OP_CLAR_OFF }, none),
                Pending::Nothing,
            ),
            Op::SetRitOffset(hz) => {
                let magnitude = hz.unsigned_abs();
                if magnitude > MAX_CLAR_HZ || !magnitude.is_multiple_of(10) {
                    return Err(Self::unsupported(
                        "clarifier offset must be a multiple of 10 Hz up to 9990 Hz",
                    ));
                }
                let tens = magnitude / 10;
                let pair = |n: u16| (((n / 10) as u8) << 4) | ((n % 10) as u8);
                let params = [u8::from(*hz < 0), 0, pair(tens / 100), pair(tens % 100)];
                (
                    Self::block(params, OP_CLAR_FREQUENCY, none),
                    Pending::Nothing,
                )
            }
            _ => {
                return Err(Self::unsupported(
                    "not implemented for the Yaesu legacy engine",
                ))
            }
        };
        self.pending = pending;
        self.buffer.clear();
        Ok(request)
    }

    // [@ANCHOR: radioctl_ylegacy_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        if now.saturating_sub(self.last_byte_at) > PARTIAL_FRAME_TIMEOUT && !self.buffer.is_empty()
        {
            let stale = std::mem::take(&mut self.buffer);
            self.last_byte_at = now;
            let mut out = vec![Inbound::Unrecognised(stale)];
            out.extend(self.feed(bytes, now));
            return out;
        }
        self.last_byte_at = now;
        self.buffer.extend_from_slice(bytes);
        let need = match self.pending {
            Pending::Nothing => {
                return if self.buffer.is_empty() {
                    Vec::new()
                } else {
                    vec![Inbound::Unrecognised(std::mem::take(&mut self.buffer))]
                };
            }
            Pending::FrequencyMode => 5,
            Pending::RxStatus | Pending::TxStatus => 1,
        };
        if self.buffer.len() < need {
            return Vec::new();
        }
        let answer: Vec<u8> = self.buffer.drain(..need).collect();
        let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
        let mut out = match (pending, answer.as_slice()) {
            (Pending::FrequencyMode, bytes) => self.decode_frequency_mode(bytes),
            (Pending::RxStatus, [status]) => {
                vec![Inbound::Observation(Observation::Signal(u16::from(
                    status & 0x0F,
                )))]
            }
            (Pending::TxStatus, [status]) => vec![
                // Bit 7 clear is transmitting (Hamlib's reading; the manual's figure is silent).
                Inbound::Observation(Observation::Ptt(status & 0x80 == 0)),
                // Bit 5 is 0 for split ON, 1 for OFF (the manual's figure).
                Inbound::Observation(Observation::Split(status & 0x20 == 0)),
                Inbound::Observation(Observation::Power(u16::from(status & 0x0F))),
            ],
            _ => vec![Inbound::Unrecognised(answer)],
        };
        if !self.buffer.is_empty() {
            out.push(Inbound::Unrecognised(std::mem::take(&mut self.buffer)));
        }
        out
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

    // Tests [@ANCHOR: radioctl_ylegacy_encode_bcd_frequency]
    // Tests [@ANCHOR: radioctl_ylegacy_decode_bcd_frequency]
    #[test]
    fn frequency_is_four_bcd_bytes_of_ten_hertz() {
        assert_eq!(
            encode_bcd_frequency(Hz(14_234_560)),
            Some([0x01, 0x42, 0x34, 0x56])
        );
        assert_eq!(
            encode_bcd_frequency(Hz(439_700_000)),
            Some([0x43, 0x97, 0x00, 0x00])
        );
        assert_eq!(encode_bcd_frequency(Hz(14_074_005)), None);
        assert_eq!(encode_bcd_frequency(Hz(1_000_000_000)), None);
        assert_eq!(
            decode_bcd_frequency(&[0x43, 0x97, 0x00, 0x00]),
            Some(Hz(439_700_000))
        );
        assert_eq!(decode_bcd_frequency(&[0x4A, 0x97, 0x00, 0x00]), None);
        assert_eq!(decode_bcd_frequency(&[0x43, 0x97, 0x00]), None);
    }

    // Tests [@ANCHOR: radioctl_ylegacy_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        let err = YaesuLegacyEngine::new(RigDescription::ts2000()).expect_err("must refuse");
        assert_eq!(err.kind, ErrorKind::Unsupported);
    }

    // Tests [@ANCHOR: radioctl_ylegacy_engine_encode]
    #[test]
    fn commands_are_the_five_byte_blocks_of_the_opcode_chart() {
        let mut e = YaesuLegacyEngine::new(shipped("yaesu-ft-817")).expect("engine");
        let b = |e: &mut YaesuLegacyEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        assert_eq!(b(&mut e, Op::GetFrequency(Vfo::A)), [0, 0, 0, 0, 0x03]);
        assert_eq!(
            b(&mut e, Op::SetFrequency(Vfo::A, Hz(14_234_560))),
            [0x01, 0x42, 0x34, 0x56, 0x01]
        );
        assert_eq!(b(&mut e, Op::SetMode(Mode::Cw)), [0x02, 0, 0, 0, 0x07]);
        assert_eq!(b(&mut e, Op::SetMode(Mode::Data)), [0x0A, 0, 0, 0, 0x07]);
        assert_eq!(b(&mut e, Op::SetPtt(true)), [0, 0, 0, 0, 0x08]);
        assert_eq!(b(&mut e, Op::SetPtt(false)), [0, 0, 0, 0, 0x88]);
        assert_eq!(b(&mut e, Op::GetPtt), [0, 0, 0, 0, 0xF7]);
        assert_eq!(b(&mut e, Op::GetSignal), [0, 0, 0, 0, 0xE7]);
        assert_eq!(b(&mut e, Op::SetSplit(true)), [0, 0, 0, 0, 0x02]);
        assert_eq!(b(&mut e, Op::SetSplit(false)), [0, 0, 0, 0, 0x82]);
        assert_eq!(b(&mut e, Op::SetRit(true)), [0, 0, 0, 0, 0x05]);
        assert_eq!(b(&mut e, Op::SetRit(false)), [0, 0, 0, 0, 0x85]);
        assert_eq!(
            b(&mut e, Op::SetRitOffset(1_230)),
            [0x00, 0x00, 0x01, 0x23, 0xF5]
        );
        assert_eq!(
            b(&mut e, Op::SetRitOffset(-9_990)),
            [0x01, 0x00, 0x09, 0x99, 0xF5]
        );
        assert!(e.encode(&Op::SetRitOffset(1_235)).is_err());
        assert!(e.encode(&Op::SetRitOffset(10_000)).is_err());
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(7_000_005))).is_err());
        assert!(
            e.encode(&Op::GetFrequency(Vfo::B)).is_err(),
            "no second VFO"
        );
        assert!(e.encode(&Op::SendCw("cq".into())).is_err());
        assert!(e.encode(&Op::SetMode(Mode::Wfm)).is_err());
    }

    // Tests [@ANCHOR: radioctl_ylegacy_engine_feed]
    // Tests [@ANCHOR: radioctl_ylegacy_decode_frequency_mode]
    #[test]
    fn answers_are_counted_by_what_was_asked() {
        let mut e = YaesuLegacyEngine::new(shipped("yaesu-ft-817")).expect("engine");
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(e.feed(&[0x01, 0x40, 0x74], T0).is_empty(), "partial");
        assert_eq!(
            e.feed(&[0x00, 0x01], T0),
            vec![
                Inbound::Observation(Observation::Frequency(Vfo::A, Hz(14_074_000))),
                Inbound::Observation(Observation::Mode(Mode::Usb)),
            ]
        );
        e.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            e.feed(&[0xA7], T0),
            vec![Inbound::Observation(Observation::Signal(7))]
        );
        e.encode(&Op::GetPtt).expect("get");
        assert_eq!(
            e.feed(&[0x0B], T0),
            vec![
                Inbound::Observation(Observation::Ptt(true)),
                Inbound::Observation(Observation::Split(true)),
                Inbound::Observation(Observation::Power(11)),
            ]
        );
        e.encode(&Op::GetPtt).expect("get");
        assert_eq!(
            e.feed(&[0xA0], T0),
            vec![
                Inbound::Observation(Observation::Ptt(false)),
                Inbound::Observation(Observation::Split(false)),
                Inbound::Observation(Observation::Power(0)),
            ]
        );
        assert!(
            matches!(e.feed(&[0x55], T0).as_slice(), [Inbound::Unrecognised(_)]),
            "unasked"
        );
        e.encode(&Op::GetMode).expect("get");
        assert!(matches!(
            e.feed(&[0x01, 0x40, 0x74, 0x00, 0x7F], T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::GetMode).expect("get");
        assert!(e.feed(&[0x01, 0x40], T0).is_empty());
        let later = T0 + PARTIAL_FRAME_TIMEOUT + Duration::from_millis(1);
        let out = e.feed(&[0xA7], later);
        assert!(
            matches!(out.first(), Some(Inbound::Unrecognised(stale)) if stale == &[0x01, 0x40])
        );
    }
}

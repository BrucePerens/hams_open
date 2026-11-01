// SPDX-License-Identifier: AGPL-3.0-or-later
//! Yaesu legacy CAT (FT-817, FT-818, FT-857, FT-897): five-byte binary blocks, sans-IO client engine.
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
//! **FT-818.** Yaesu's FT-818ND operating manual (`FT-818ND_OM_ENG_E13772004_2003u-ES-1.pdf`, sha256
//! e723eab91774d62bfcc525681a60f66a4a801d014d368d64c88a534a8d9df802, PDF pages 67 to 69, printed 65 to
//! 67) has the same 17 opcodes with the same parameters as the FT-817ND's. Compared chapter against
//! chapter, the documented differences are: Note 4 states the PTT level (bit 7: 0 is PTT ON, TX),
//! which the FT-817ND figure leaves out, so the FT-818 description takes `read-ptt` from the manual
//! and not from Hamlib; Note 5 lists `06` (WFM) among the modes a read can return, which the mode
//! command's table does not list, so it is not mapped; the power-on note names the FT-818's
//! SBR-32MH battery pack where the FT-817ND's names the FNB-85. The FT-817ND's Note 5
//! is a figure that was not read, so whether it also lists `06` is not known. Nothing else in the
//! text of the two CAT chapters differs.
//!
//! Not implemented: the VFO A/B toggle (`81`, so no second VFO), lock, repeater offset, CTCSS and
//! DCS, power on and off, the clarifier frequency read (there is none), FM-N and PKT modes.

// TODO(radioctl tx frequency): split and the transmit VFO (`Observation::TxVfo`, `Op::GetTxVfo`) are
// not mapped here; another agent is rebasing this engine. When it lands, report `TxVfo` from the
// split flag so `active_tx_frequency` follows (see radioctl-core `txstate`).

use std::time::Duration;

use radioctl_core::description::{
    Family, LegacyBlock, LegacyCommand, LegacyFlag, LegacyRead, RigDescription,
};
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
/// The same bound as a signed value, for the virtual rig's decode of the block.
pub const MAX_CLAR_HZ_I16: i16 = 9_990;

/// What the engine is waiting for after a read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    FrequencyMode,
    RxStatus,
    TxStatus,
    /// A block-protocol status read; the index is into `LegacyBlock::reads`.
    Block(usize),
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

// [@ANCHOR: radioctl_ylegacy_encode_frequency]
/// The four bytes of a frequency block. With `high_band_flag` (FT-736R), a frequency from 1.2 GHz
/// up to 1.3 GHz is sent as the seven low digits of its 10 Hz count with the first nibble set to C,
/// which stands for the digits 12 (Hamlib's `ft736_set_freq` ORs C into the first byte of the eight
/// low digits, replacing the 2; unverified: no manual read).
pub fn encode_frequency(hz: Hz, high_band_flag: bool) -> Option<[u8; 4]> {
    if high_band_flag && hz.0 >= 1_200_000_000 && hz.0 < 1_300_000_000 && hz.0.is_multiple_of(10) {
        let mut bytes = encode_bcd_frequency(Hz(hz.0 - 1_200_000_000))?;
        bytes[0] |= 0xC0;
        return Some(bytes);
    }
    encode_bcd_frequency(hz)
}

// [@ANCHOR: radioctl_ylegacy_decode_frequency]
/// The inverse of [`encode_frequency`]: a first nibble of C stands for the digits 12.
pub fn decode_frequency(bytes: &[u8], high_band_flag: bool) -> Option<Hz> {
    if high_band_flag && bytes.first().is_some_and(|b| b >> 4 == 0xC) {
        let mut plain = bytes.to_vec();
        plain[0] &= 0x0F;
        let hz = decode_bcd_frequency(&plain)?;
        return Some(Hz(hz.0 + 1_200_000_000));
    }
    decode_bcd_frequency(bytes)
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
    /// The command bytes a radio that echoes (FT-767GX) is expected to send back.
    echo_of: Option<Vec<u8>>,
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
            echo_of: None,
        })
    }

    fn spec(&self) -> Option<&LegacyBlock> {
        self.desc
            .yaesu_legacy
            .as_ref()
            .and_then(|y| y.block.as_ref())
    }

    // [@ANCHOR: radioctl_ylegacy_fill_command]
    /// A command's block with the parameter bytes in its slot.
    fn fill(command: &LegacyCommand, params: &[u8]) -> [u8; 5] {
        let mut out = command.template;
        for (i, byte) in params.iter().enumerate() {
            if let Some(slot) = out.get_mut(usize::from(command.at) + i) {
                *slot = *byte;
            }
        }
        out
    }

    // [@ANCHOR: radioctl_ylegacy_encode_block]
    /// The request for an operation on a block-protocol model, and what to expect back.
    fn encode_block(&self, op: &Op) -> Result<(Request, Pending), RigError> {
        let block = self
            .spec()
            .ok_or_else(|| Self::unsupported("no block commands"))?;
        let ack = if block.echo {
            ReplyKind::Ack
        } else {
            ReplyKind::None
        };
        let send = |bytes: [u8; 5]| Request {
            bytes: bytes.to_vec(),
            reply: ack,
        };
        let read_for = |want: &dyn Fn(&LegacyRead) -> bool, field: Field| {
            block
                .reads
                .iter()
                .position(want)
                .map(|index| {
                    (
                        Request {
                            bytes: block.reads[index].request.to_vec(),
                            reply: ReplyKind::Observation(field),
                        },
                        Pending::Block(index),
                    )
                })
                .ok_or_else(|| Self::unsupported("this model has no such status read"))
        };
        match op {
            Op::SetFrequency(Vfo::A, hz) => {
                let command = block
                    .set_frequency
                    .as_ref()
                    .ok_or_else(|| Self::unsupported("no set-frequency command"))?;
                let form = command
                    .form
                    .ok_or_else(|| Self::unsupported("no frequency form"))?;
                let bytes = form.encode(*hz).ok_or_else(|| {
                    Self::unsupported("frequency not representable in this model's command")
                })?;
                Ok((send(Self::fill(command, &bytes)), Pending::Nothing))
            }
            Op::SetMode(mode) => {
                let command = block
                    .set_mode
                    .as_ref()
                    .ok_or_else(|| Self::unsupported("no set-mode command"))?;
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| Self::unsupported("mode not in the description"))?;
                Ok((send(Self::fill(command, &[code])), Pending::Nothing))
            }
            Op::SetPtt(on) => {
                let (a, b) = block
                    .ptt
                    .ok_or_else(|| Self::unsupported("no PTT command"))?;
                Ok((send(if *on { a } else { b }), Pending::Nothing))
            }
            Op::SetSplit(on) => {
                let (a, b) = block
                    .split
                    .ok_or_else(|| Self::unsupported("no split command"))?;
                Ok((send(if *on { a } else { b }), Pending::Nothing))
            }
            Op::GetFrequency(Vfo::A) => {
                read_for(&|r| r.frequency.is_some(), Field::Frequency(Vfo::A))
            }
            Op::GetMode => read_for(&|r| r.mode.is_some(), Field::Mode),
            Op::GetPtt => read_for(&|r| r.ptt.is_some(), Field::Ptt),
            Op::GetSplit => read_for(&|r| r.split.is_some(), Field::Split),
            _ => Err(Self::unsupported(
                "not implemented for the Yaesu legacy engine",
            )),
        }
    }

    // [@ANCHOR: radioctl_ylegacy_decode_block]
    fn decode_block(&self, read: &LegacyRead, reply: &[u8]) -> Vec<Inbound> {
        let flag = |f: &LegacyFlag| {
            reply
                .get(usize::from(f.offset))
                .map(|b| (b & f.mask != 0) == f.on_when_set)
        };
        let mut out = Vec::new();
        if let Some((offset, form)) = read.frequency {
            let start = usize::from(offset);
            match reply
                .get(start..start + form.width())
                .and_then(|b| form.decode(b))
            {
                Some(hz) => out.push(Inbound::Observation(Observation::Frequency(Vfo::A, hz))),
                None => return vec![Inbound::Unrecognised(reply.to_vec())],
            }
        }
        if let Some((offset, mask)) = read.mode {
            let mode = reply.get(usize::from(offset)).and_then(|b| {
                let code = b & mask;
                self.spec()?
                    .status_modes
                    .iter()
                    .find(|(c, _)| *c == code)
                    .map(|(_, m)| *m)
            });
            match mode {
                Some(mode) => out.push(Inbound::Observation(Observation::Mode(mode))),
                None => return vec![Inbound::Unrecognised(reply.to_vec())],
            }
        }
        if let Some(on) = read.ptt.as_ref().and_then(flag) {
            out.push(Inbound::Observation(Observation::Ptt(on)));
        }
        if let Some(on) = read.split.as_ref().and_then(flag) {
            out.push(Inbound::Observation(Observation::Split(on)));
        }
        out
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
        if self.spec().is_some() {
            let (request, pending) = self.encode_block(op)?;
            self.pending = pending;
            self.echo_of = self
                .spec()
                .is_some_and(|b| b.echo)
                .then(|| request.bytes.clone());
            self.buffer.clear();
            return Ok(request);
        }
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
                let high = self
                    .desc
                    .yaesu_legacy
                    .as_ref()
                    .is_some_and(|y| y.high_band_flag);
                let bcd = encode_frequency(*hz, high).ok_or_else(|| {
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
        let mut out = Vec::new();
        if let Some(echo) = self.echo_of.clone() {
            if self.buffer.len() < echo.len() {
                return out;
            }
            let got: Vec<u8> = self.buffer.drain(..echo.len()).collect();
            self.echo_of = None;
            out.push(if got == echo {
                Inbound::Ack
            } else {
                Inbound::Nak
            });
            if self.buffer.is_empty() {
                return out;
            }
        }
        let need = match self.pending {
            Pending::Block(index) => self
                .spec()
                .and_then(|b| b.reads.get(index))
                .map_or(1, |r| usize::from(r.length)),
            Pending::Nothing => {
                if !self.buffer.is_empty() {
                    out.push(Inbound::Unrecognised(std::mem::take(&mut self.buffer)));
                }
                return out;
            }
            Pending::FrequencyMode => 5,
            Pending::RxStatus | Pending::TxStatus => 1,
        };
        if self.buffer.len() < need {
            return out;
        }
        let answer: Vec<u8> = self.buffer.drain(..need).collect();
        let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
        out.extend(match (pending, answer.as_slice()) {
            (Pending::Block(index), reply) => match self.spec().and_then(|b| b.reads.get(index)) {
                Some(read) => self.decode_block(read, reply),
                None => vec![Inbound::Unrecognised(answer.clone())],
            },
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
        });
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
        // The five-byte protocol has no Morse command and so nothing to stop.
        assert!(e.encode(&Op::StopCw).is_err());
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

    // Tests [@ANCHOR: radioctl_ylegacy_encode_frequency]
    // Tests [@ANCHOR: radioctl_ylegacy_decode_frequency]
    #[test]
    fn the_high_band_flag_replaces_the_leading_digit_with_c() {
        assert_eq!(
            encode_frequency(Hz(1_240_000_000), true),
            Some([0xC4, 0x00, 0x00, 0x00])
        );
        assert_eq!(
            decode_frequency(&[0xC4, 0x00, 0x00, 0x00], true),
            Some(Hz(1_240_000_000))
        );
        assert_eq!(
            encode_frequency(Hz(144_200_000), true),
            Some([0x14, 0x42, 0x00, 0x00])
        );
        assert_eq!(encode_frequency(Hz(1_240_000_000), false), None);
        assert_eq!(decode_frequency(&[0xC4, 0, 0, 0], false), None);
    }

    // Tests [@ANCHOR: radioctl_ylegacy_engine_encode]
    // Tests [@ANCHOR: radioctl_virtual_legacy_block]
    #[test]
    fn the_hamlib_described_family_members_set_and_some_read_back() {
        use crate::virtual_rig::VirtualRig;
        for (id, reads, hz) in [
            ("yaesu-ft-897", true, 7_100_000),
            ("yaesu-ft-847", true, 144_200_000),
            ("yaesu-ft-847uni", false, 14_200_000),
            ("yaesu-ft-650", false, 50_100_000),
            ("yaesu-ft-736r", false, 1_296_100_000),
        ] {
            let d = shipped(id);
            let mut e = YaesuLegacyEngine::new(d.clone()).expect("engine");
            let mut r = VirtualRig::from_description(&d).expect("rig");
            fn send(e: &mut YaesuLegacyEngine, r: &mut VirtualRig, op: Op) -> Vec<Inbound> {
                let request = e.encode(&op).expect("encodes");
                let answer = r.feed(&request.bytes, T0);
                e.feed(&answer, T0)
            }
            assert!(
                send(&mut e, &mut r, Op::SetFrequency(Vfo::A, Hz(hz))).is_empty(),
                "{id}"
            );
            assert!(
                send(&mut e, &mut r, Op::SetMode(Mode::Usb)).is_empty(),
                "{id}"
            );
            assert_eq!(r.state().freq_a, Hz(hz), "{id}");
            assert_eq!(r.state().mode, Mode::Usb, "{id}");
            if reads {
                assert_eq!(
                    send(&mut e, &mut r, Op::GetFrequency(Vfo::A)),
                    vec![
                        Inbound::Observation(Observation::Frequency(Vfo::A, Hz(hz))),
                        Inbound::Observation(Observation::Mode(Mode::Usb)),
                    ],
                    "{id}"
                );
            } else {
                assert!(
                    e.encode(&Op::GetFrequency(Vfo::A)).is_err(),
                    "{id}: one way"
                );
                assert!(e.encode(&Op::GetMode).is_err(), "{id}: one way");
            }
            assert!(e.encode(&Op::SetPtt(true)).is_ok(), "{id}");
        }
        let mut e = YaesuLegacyEngine::new(shipped("yaesu-ft-736r")).expect("engine");
        assert_eq!(
            e.encode(&Op::SetFrequency(Vfo::A, Hz(1_296_100_000)))
                .expect("encodes")
                .bytes,
            [0xC9, 0x61, 0x00, 0x00, 0x01]
        );
        assert_eq!(
            e.encode(&Op::SetMode(Mode::Fm)).expect("encodes").bytes,
            [0x08, 0, 0, 0, 0x07]
        );
    }

    /// The block-protocol descriptions added after the FT-817 family.
    const BLOCK_RIGS: [&str; 23] = [
        "yaesu-ft-747gx",
        "yaesu-ft-757gx",
        "yaesu-ft-757gxii",
        "yaesu-ft-767gx",
        "yaesu-ft-840",
        "yaesu-ft-890",
        "yaesu-ft-900",
        "yaesu-ft-920",
        "yaesu-ft-990",
        "yaesu-ft-990-oldrom",
        "yaesu-ft-1000",
        "yaesu-ft-1000d",
        "yaesu-ft-1000mp",
        "yaesu-ft-1000mp-mkv",
        "yaesu-ft-1000mp-mkv-field",
        "yaesu-ft-100",
        "yaesu-ft-600",
        "yaesu-ft-980",
        "yaesu-frg-100",
        "yaesu-frg-8800",
        "yaesu-frg-9600",
        "yaesu-vr-5000",
        "vertex-vx-1700",
    ];

    fn send(
        e: &mut YaesuLegacyEngine,
        r: &mut crate::virtual_rig::VirtualRig,
        op: Op,
    ) -> Vec<Inbound> {
        let request = e.encode(&op).expect("encodes");
        let answer = r.feed(&request.bytes, T0);
        e.feed(&answer, T0)
    }

    // Tests [@ANCHOR: radioctl_ylegacy_encode_block]
    // Tests [@ANCHOR: radioctl_ylegacy_decode_block]
    // Tests [@ANCHOR: radioctl_ylegacy_fill_command]
    // Tests [@ANCHOR: radioctl_virtual_legacy_model_block]
    // Tests [@ANCHOR: radioctl_core_rigfile_build_legacy_block]
    // Tests [@ANCHOR: radioctl_core_rigfile_legacy_form]
    #[test]
    fn every_block_protocol_twin_sets_and_reads_back_what_its_description_offers() {
        use crate::virtual_rig::VirtualRig;
        use radioctl_core::capability::Capability;
        for id in BLOCK_RIGS {
            let d = shipped(id);
            let hz = Hz(d.default_frequency.0 - d.default_frequency.0 % 1_000);
            let mut e = YaesuLegacyEngine::new(d.clone()).expect("engine");
            let mut r = VirtualRig::from_description(&d).expect("rig");
            let spec = d
                .yaesu_legacy
                .as_ref()
                .and_then(|y| y.block.clone())
                .expect("block");
            assert!(
                send(&mut e, &mut r, Op::SetFrequency(Vfo::A, hz))
                    .iter()
                    .all(|i| *i == Inbound::Ack),
                "{id}"
            );
            assert_eq!(r.state().freq_a, hz, "{id}: frequency set");
            let reads_frequency = d.capabilities.has(Capability::ReadFrequency);
            if reads_frequency {
                let got = send(&mut e, &mut r, Op::GetFrequency(Vfo::A));
                assert!(
                    got.contains(&Inbound::Observation(Observation::Frequency(Vfo::A, hz))),
                    "{id}: {got:?}"
                );
            } else {
                assert!(
                    e.encode(&Op::GetFrequency(Vfo::A)).is_err(),
                    "{id}: set-only"
                );
            }
            for (mode, _) in d.mode_codes.clone() {
                send(&mut e, &mut r, Op::SetMode(mode));
                assert_eq!(r.state().mode, mode, "{id}: {mode:?} set");
                let readable = spec.status_modes.iter().any(|(_, m)| *m == mode);
                if d.capabilities.has(Capability::ReadMode) && readable {
                    let got = send(&mut e, &mut r, Op::GetMode);
                    assert!(
                        got.contains(&Inbound::Observation(Observation::Mode(mode))),
                        "{id}: {mode:?} read: {got:?}"
                    );
                }
            }
            if d.capabilities.has(Capability::SetPtt) {
                send(&mut e, &mut r, Op::SetPtt(true));
                assert!(r.state().ptt, "{id}: PTT on");
                if d.capabilities.has(Capability::ReadPtt) {
                    let got = send(&mut e, &mut r, Op::GetPtt);
                    assert!(
                        got.contains(&Inbound::Observation(Observation::Ptt(true))),
                        "{id}: {got:?}"
                    );
                }
                send(&mut e, &mut r, Op::SetPtt(false));
                assert!(!r.state().ptt, "{id}: PTT off");
            } else {
                assert!(e.encode(&Op::SetPtt(true)).is_err(), "{id}: no CAT PTT");
            }
            if d.capabilities.has(Capability::ReadPtt) {
                // Receiving reads as not transmitting.
                let got = send(&mut e, &mut r, Op::GetPtt);
                assert!(
                    got.contains(&Inbound::Observation(Observation::Ptt(false))),
                    "{id}: {got:?}"
                );
            }
            if d.capabilities.has(Capability::Split) {
                send(&mut e, &mut r, Op::SetSplit(true));
                assert!(r.state().split, "{id}: split on");
                let got = send(&mut e, &mut r, Op::GetSplit);
                assert!(
                    got.contains(&Inbound::Observation(Observation::Split(true))),
                    "{id}: {got:?}"
                );
                send(&mut e, &mut r, Op::SetSplit(false));
            }
        }
    }

    // Tests [@ANCHOR: radioctl_core_legacy_form_encode]
    // Tests [@ANCHOR: radioctl_core_legacy_form_decode]
    // Tests [@ANCHOR: radioctl_core_legacy_form_width]
    // Tests [@ANCHOR: radioctl_core_legacy_form_unit]
    #[test]
    fn the_frequency_forms_encode_the_manuals_worked_examples() {
        use radioctl_core::description::LegacyFreqForm as F;
        // 14.25000 MHz is 00 50 42 01 on the wire for the least-significant-first BCD forms
        // (FT-747, FT-757GXII, FT-840 and the others' manuals).
        assert_eq!(
            F::Bcd10HzLsb.encode(Hz(14_250_000)),
            Some(vec![0x00, 0x50, 0x42, 0x01])
        );
        assert_eq!(
            F::Bcd10HzLsb.decode(&[0x00, 0x50, 0x42, 0x01]),
            Some(Hz(14_250_000))
        );
        // FT-100 manual's example: 439.70 MHz is 00 00 97 43.
        assert_eq!(
            F::Bcd10HzLsb.encode(Hz(439_700_000)),
            Some(vec![0x00, 0x00, 0x97, 0x43])
        );
        // VR-5000 manual's example: 439.70 MHz is 02 9E ED D0.
        assert_eq!(
            F::Bin10HzMsb4.encode(Hz(439_700_000)),
            Some(vec![0x02, 0x9E, 0xED, 0xD0])
        );
        // FRG-8800 manual's example: 14.254 MHz is 01 54 42 01; +75 Hz sets the step nibble to 8.
        assert_eq!(
            F::Frg8800.encode(Hz(14_254_000)),
            Some(vec![0x01, 0x54, 0x42, 0x01])
        );
        assert_eq!(
            F::Frg8800.encode(Hz(14_254_075)),
            Some(vec![0x08, 0x54, 0x42, 0x01])
        );
        assert_eq!(
            F::Frg8800.decode(&[0x08, 0x54, 0x42, 0x01]),
            Some(Hz(14_254_075))
        );
        assert_eq!(F::Frg8800.encode(Hz(14_254_010)), None, "not a 25 Hz step");
        // FT-890 family status: three binary bytes in 10 Hz units, most significant first.
        assert_eq!(
            F::Bin10HzMsb3.encode(Hz(14_250_000)),
            Some(vec![0x15, 0xBE, 0x68])
        );
        assert_eq!(
            F::Bin10HzLsb3.encode(Hz(14_250_000)),
            Some(vec![0x68, 0xBE, 0x15])
        );
        // FT-100 status: 1.25 Hz units; 5 Hz is the finest whole-hertz step.
        assert_eq!(
            F::Bin125Msb4.encode(Hz(14_250_000)),
            Some(vec![0x00, 0xAD, 0xF3, 0x40])
        );
        assert_eq!(F::Bin125Msb4.encode(Hz(14_250_001)), None);
        assert_eq!(
            F::Bin125Msb4.decode(&[0x00, 0xAD, 0xF3, 0x40]),
            Some(Hz(14_250_000))
        );
        // FT-747 status: eight BCD digits of hertz, most significant first.
        assert_eq!(
            F::Bcd1HzMsb.encode(Hz(14_250_025)),
            Some(vec![0x14, 0x25, 0x00, 0x25])
        );
        // FT-1000MP status as Hamlib reads it: 0.625 Hz units.
        assert_eq!(
            F::Bin0625Msb4.decode(&[0x01, 0x58, 0x5B, 0x00]),
            Some(Hz(14_104_800))
        );
        assert_eq!(F::Bcd10HzMsb.decode(&[0x4A, 0, 0, 0]), None);
        assert_eq!(F::Bin10HzMsb3.decode(&[1, 2]), None);
        assert_eq!(F::Bin10HzMsb3.width(), 3);
        assert_eq!(F::Bin125Msb4.width(), 4);
        assert_eq!(
            F::Bin10HzMsb3.encode(Hz(2_000_000_000)),
            None,
            "too big for 24 bits"
        );
    }

    // Tests [@ANCHOR: radioctl_ylegacy_encode_block]
    // Tests [@ANCHOR: radioctl_ylegacy_fill_command]
    #[test]
    fn block_commands_are_the_manuals_wire_bytes() {
        let wire = |id: &str, op: Op| {
            YaesuLegacyEngine::new(shipped(id))
                .expect("engine")
                .encode(&op)
                .expect("encodes")
                .bytes
        };
        assert_eq!(
            wire("yaesu-ft-747gx", Op::SetFrequency(Vfo::A, Hz(14_250_000))),
            [0x00, 0x50, 0x42, 0x01, 0x0A]
        );
        assert_eq!(
            wire("yaesu-ft-747gx", Op::SetMode(Mode::Usb)),
            [0, 0, 0, 1, 0x0C]
        );
        assert_eq!(wire("yaesu-ft-747gx", Op::SetPtt(true)), [0, 0, 0, 1, 0x0F]);
        assert_eq!(wire("yaesu-ft-747gx", Op::GetPtt), [0, 0, 0, 0, 0x10]);
        assert_eq!(
            wire("yaesu-ft-890", Op::GetFrequency(Vfo::A)),
            [0, 0, 0, 3, 0x10]
        );
        assert_eq!(wire("yaesu-ft-890", Op::GetPtt), [0, 0, 0, 0, 0xFA]);
        assert_eq!(
            wire("yaesu-ft-767gx", Op::SetFrequency(Vfo::A, Hz(14_250_000))),
            [0x00, 0x50, 0x42, 0x01, 0x08]
        );
        assert_eq!(
            wire("yaesu-ft-767gx", Op::SetMode(Mode::Fm)),
            [0, 0, 0, 0x14, 0x0A]
        );
        assert_eq!(
            wire("yaesu-frg-8800", Op::SetFrequency(Vfo::A, Hz(14_254_000))),
            [0x01, 0x54, 0x42, 0x01, 0x01]
        );
        assert_eq!(
            wire("yaesu-frg-8800", Op::SetMode(Mode::Usb)),
            [0, 0, 0, 2, 0x80]
        );
        assert_eq!(
            wire("yaesu-frg-9600", Op::SetFrequency(Vfo::A, Hz(145_000_000))),
            [0x0A, 0x14, 0x50, 0x00, 0x00]
        );
        assert_eq!(
            wire("yaesu-frg-9600", Op::SetMode(Mode::Fm)),
            [0x16, 0, 0, 0, 0]
        );
        assert_eq!(
            wire("yaesu-vr-5000", Op::SetFrequency(Vfo::A, Hz(439_700_000))),
            [0x02, 0x9E, 0xED, 0xD0, 0x01]
        );
        assert_eq!(
            wire("yaesu-vr-5000", Op::SetMode(Mode::Fm)),
            [0x88, 0x02, 0, 0, 0x07]
        );
        assert_eq!(
            wire("yaesu-ft-100", Op::SetFrequency(Vfo::A, Hz(439_700_000))),
            [0x00, 0x00, 0x97, 0x43, 0x0A]
        );
        let mut e = YaesuLegacyEngine::new(shipped("yaesu-ft-890")).expect("engine");
        assert!(e.encode(&Op::GetSignal).is_err(), "meters are not mapped");
        assert!(
            e.encode(&Op::SetFrequency(Vfo::A, Hz(14_250_005))).is_err(),
            "10 Hz units"
        );
    }

    // Tests [@ANCHOR: radioctl_ylegacy_engine_feed]
    // Tests [@ANCHOR: radioctl_ylegacy_decode_block]
    #[test]
    fn a_status_block_is_counted_by_length_and_a_bad_field_is_unrecognised() {
        let mut e = YaesuLegacyEngine::new(shipped("yaesu-ft-890")).expect("engine");
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        // The 18-byte VFO A and B block: VFO A's frequency at offset 1, mode at offset 6.
        let mut block = vec![0u8; 18];
        block[1..4].copy_from_slice(&[0x15, 0xBE, 0x68]);
        block[6] = 0x01;
        assert!(e.feed(&block[..10], T0).is_empty(), "partial");
        assert_eq!(
            e.feed(&block[10..], T0),
            vec![
                Inbound::Observation(Observation::Frequency(Vfo::A, Hz(14_250_000))),
                Inbound::Observation(Observation::Mode(Mode::Usb)),
            ]
        );
        e.encode(&Op::GetMode).expect("get");
        block[6] = 0x07;
        assert!(matches!(
            e.feed(&block, T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        // The flags block: transmitting is bit 7 of the third byte.
        e.encode(&Op::GetPtt).expect("get");
        assert_eq!(
            e.feed(&[0x00, 0x00, 0x80, 0x08, 0x41], T0),
            vec![
                Inbound::Observation(Observation::Ptt(true)),
                Inbound::Observation(Observation::Split(false)),
            ]
        );
        // The FT-767GX echoes: a matching echo is an Ack, a different one a Nak.
        let mut e = YaesuLegacyEngine::new(shipped("yaesu-ft-767gx")).expect("engine");
        let request = e.encode(&Op::SetMode(Mode::Usb)).expect("set");
        assert_eq!(request.reply, ReplyKind::Ack);
        assert!(e.feed(&request.bytes[..3], T0).is_empty());
        assert_eq!(e.feed(&request.bytes[3..], T0), vec![Inbound::Ack]);
        e.encode(&Op::SetMode(Mode::Usb)).expect("set");
        assert_eq!(e.feed(&[1, 2, 3, 4, 5], T0), vec![Inbound::Nak]);
    }

    /// Where the manuals or Hamlib disagree with each other, the description follows the manual and
    /// this test pins the choice so a change is visible. None of these is settled without a radio.
    // Tests [@ANCHOR: radioctl_core_rigfile_build_legacy_block]
    #[test]
    fn the_open_questions_are_pinned_to_the_choice_made() {
        let spec = |id: &str| {
            shipped(id)
                .yaesu_legacy
                .and_then(|y| y.block)
                .expect("block")
        };
        // FT-747GX: the manual's update block is 345 bytes, Hamlib reads 344; the displayed
        // frequency is at offset 2 by the manual, offset 1 in Hamlib.
        let ft747 = spec("yaesu-ft-747gx");
        assert_eq!(ft747.reads[0].length, 345);
        assert_eq!(ft747.reads[0].frequency.map(|f| f.0), Some(2));
        // FT-890 split-on parameter: Hamlib and the FT-840/900 manuals send 1; the FT-890 manual's
        // table prints 0.
        assert_eq!(spec("yaesu-ft-890").split.map(|s| s.0[3]), Some(1));
        // FT-1000MP status frequency unit: Hamlib's 0.625 Hz, unconfirmed by the manual.
        assert_eq!(
            spec("yaesu-ft-1000mp").reads[0].frequency.map(|f| f.1),
            Some(radioctl_core::description::LegacyFreqForm::Bin0625Msb4)
        );
        // FT-920: Hamlib's plain-hertz reading of the record's frequency.
        assert_eq!(
            spec("yaesu-ft-920").reads[0].frequency.map(|f| f.1),
            Some(radioctl_core::description::LegacyFreqForm::Bin1HzMsb4)
        );
        // FT-990: the manual's operating-data record is 16 bytes, Hamlib reads 32.
        assert_eq!(spec("yaesu-ft-990").reads[1].length, 32);
        // FT-100: the manual's status block is 16 bytes, Hamlib reads 32.
        assert_eq!(spec("yaesu-ft-100").reads[0].length, 16);
        // VR-5000 set-mode step byte is the one both sources agree on (100 Hz, 02), not the 25 kHz
        // code on which the manual (34) and Hamlib (35) differ.
        assert_eq!(
            spec("yaesu-vr-5000").set_mode.map(|c| c.template[1]),
            Some(0x02)
        );
    }
}

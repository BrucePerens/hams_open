// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Si570 USB kits: SoftRock AVR-USB, KTH-SDR PIC-USB, AE9RB Peaberry V1 and V2, Funkamateur
//! FA-SDR, and FiFi-SDR. Sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/kit/si570avrusb.c`,
//! `si570avrusb.h`, `fifisdr.c`. No vendor document was read and nothing is verified against a
//! board.
//!
//! The boards are a Si570 oscillator behind an Atmel or PIC USB controller (V-USB, shared id
//! 16C0:05DC). Every operation is a vendor control transfer on endpoint 0; the engine's request
//! bytes are [`UsbTransfer`] frames carried by `radioctl_core::usb_link`:
//!
//! * set frequency: request 0x32, direction out, four little-endian bytes holding the
//!   *oscillator* frequency in MHz as 11.21 fixed point, that is the receive frequency times the
//!   board's multiplier (4 for a quadrature detector board, 2 for the KTH). wValue is 0x700 plus
//!   the Si570's I2C address 0x55 on the Si570 kits and 0 on the FiFi-SDR; wIndex is 0.
//! * read frequency: request 0x3A, direction in, four bytes, the same word.
//! * PTT: request 0x50, direction *in* (as Hamlib sends it), wValue 1 or 0, a three-byte answer
//!   that is ignored. Encoded for the descriptions that list `set-ptt`; unverified, and the
//!   transmit gate keeps it closed unless the operator opens it.
//!
//! Firmware older than 0x0F00 (DG8SAQ, `UsbProtocol::Si570Legacy`) has no by-value request. It
//! takes the Si570's six divider registers (7 to 12) instead: set is request 0x30 with the same
//! wValue (0x700 + 0x55) and the six bytes, read is request 0x3F with wValue 0x55 and a six-byte
//! answer. The host solves the dividers: HS_DIV from {4, 5, 6, 7, 9, 11} and N1 (1 or even, up to
//! 128) such that the DCO (output times HS_DIV times N1) lies in 4850 to 5670 MHz, the lowest
//! such DCO winning, and RFREQ = DCO / crystal as 10.28 fixed point with the board's nominal
//! 114.285 MHz crystal. The six bytes are the 48-bit word HS_DIV code (3 bits, 0 4, 1 5, 2 6, 3 7,
//! 5 9, 7 11), N1 minus one (7 bits), RFREQ (38 bits), big-endian. Facts: Hamlib
//! `si570avrusb.h` (constants, request numbers), `si570avrusb.c` `calcDividers`,
//! `si570xxxusb_set_freq`, `calculateFrequency`, `si570xxxusb_get_freq`.
//!
//! The multiplier comes from the description, not from the board.

use std::time::Duration;

use radioctl_core::description::{RigDescription, UsbProtocol};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Vfo};
use radioctl_core::usb_device::{UsbDevice, UsbFailure};
use radioctl_core::usb_wire::{FrameError, UsbReply, UsbStatus, UsbTransfer};

/// Request type of a vendor request to the device (direction out).
pub const VENDOR_OUT: u8 = 0x40;
/// Request type of a vendor request from the device (direction in).
pub const VENDOR_IN: u8 = 0xC0;
/// Set the frequency by value.
pub const REQUEST_SET_FREQ_BY_VALUE: u8 = 0x32;
/// Read the frequency.
pub const REQUEST_READ_FREQUENCY: u8 = 0x3A;
/// Set the frequency as Si570 registers (firmware before 0x0F00).
pub const REQUEST_SET_FREQ_REGISTERS: u8 = 0x30;
/// Read the Si570 registers (firmware before 0x0F00).
pub const REQUEST_READ_REGISTERS: u8 = 0x3F;
/// Set PTT.
pub const REQUEST_SET_PTT: u8 = 0x50;
/// The Si570's I2C address as the kits use it.
const SI570_I2C_ADDRESS: u16 = 0x55;

/// The nominal crystal of the DG8SAQ boards, in hertz.
const NOMINAL_CRYSTAL_HZ: u128 = 114_285_000;
/// DCO limits in hertz.
const DCO_LOW_HZ: u128 = 4_850_000_000;
const DCO_HIGH_HZ: u128 = 5_670_000_000;
/// The HS_DIV divider and its 3-bit register code.
const HS_DIVIDERS: [(u128, u8); 6] = [(4, 0), (5, 1), (6, 2), (7, 3), (9, 5), (11, 7)];

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_si570_usb_word_from_hz]
/// The 11.21 fixed-point oscillator word for a receive frequency, or `None` when it does not fit
/// in 32 bits (oscillator above 2047 MHz). Rounded to the nearest unit (about 0.5 Hz at 7 MHz).
pub fn word_from_hz(hz: Hz, multiplier: u8) -> Option<u32> {
    let numerator = u128::from(hz.0) * u128::from(multiplier) * (1u128 << 21);
    let word = (numerator + 500_000) / 1_000_000;
    u32::try_from(word).ok()
}

// [@ANCHOR: radioctl_si570_usb_hz_from_word]
/// The receive frequency for an oscillator word, rounded to the nearest hertz; `None` for a
/// multiplier of zero.
pub fn hz_from_word(word: u32, multiplier: u8) -> Option<Hz> {
    if multiplier == 0 {
        return None;
    }
    let divisor = u128::from(multiplier) * (1u128 << 21);
    let hz = (u128::from(word) * 1_000_000 + divisor / 2) / divisor;
    u64::try_from(hz).ok().map(Hz)
}

// [@ANCHOR: radioctl_si570_usb_registers_from_hz]
/// The six Si570 register bytes (registers 7 to 12) for a receive frequency, or `None` when no
/// divider setting puts the DCO in range.
pub fn registers_from_hz(hz: Hz, multiplier: u8) -> Option<[u8; 6]> {
    let output = u128::from(hz.0) * u128::from(multiplier);
    if output == 0 {
        return None;
    }
    let mut best: Option<(u128, u8, u128)> = None;
    for (hs, code) in HS_DIVIDERS {
        for n1 in std::iter::once(1u128).chain((2..=128).step_by(2)) {
            let dco = output * hs * n1;
            if (DCO_LOW_HZ..=DCO_HIGH_HZ).contains(&dco) && best.is_none_or(|(low, _, _)| dco < low)
            {
                best = Some((dco, code, n1));
            }
        }
    }
    let (dco, code, n1) = best?;
    let rfreq = (dco * (1u128 << 28) + NOMINAL_CRYSTAL_HZ / 2) / NOMINAL_CRYSTAL_HZ;
    let word = (u64::from(code) << 45)
        | (u64::try_from(n1 - 1).ok()? << 38)
        | u64::try_from(rfreq).ok()?;
    let bytes = word.to_be_bytes();
    <[u8; 6]>::try_from(bytes.get(2..)?).ok()
}

// [@ANCHOR: radioctl_si570_usb_hz_from_registers]
/// The receive frequency of six Si570 register bytes, rounded to the nearest hertz; `None` for a
/// multiplier of zero or an unused HS_DIV code.
pub fn hz_from_registers(registers: &[u8; 6], multiplier: u8) -> Option<Hz> {
    if multiplier == 0 {
        return None;
    }
    let word = registers
        .iter()
        .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte));
    let code = u8::try_from(word >> 45).ok()?;
    let hs = HS_DIVIDERS.iter().find(|(_, c)| *c == code)?.0;
    let n1 = u128::from((word >> 38) & 0x7F) + 1;
    let rfreq = u128::from(word & ((1u64 << 38) - 1));
    let divisor = (1u128 << 28) * hs * n1 * u128::from(multiplier);
    let hz = (NOMINAL_CRYSTAL_HZ * rfreq + divisor / 2) / divisor;
    u64::try_from(hz).ok().map(Hz)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Set,
    ReadFrequency,
    ReadRegisters,
}

/// The Si570 USB client engine.
#[derive(Debug)]
pub struct Si570UsbEngine {
    desc: RigDescription,
    protocol: UsbProtocol,
    multiplier: u8,
    pending: Pending,
    rx: Vec<u8>,
}

impl Si570UsbEngine {
    // [@ANCHOR: radioctl_si570_usb_engine_new]
    /// Builds an engine from a `si570` or `fifisdr` USB description.
    pub fn new(desc: RigDescription) -> Result<Si570UsbEngine, RigError> {
        let usb = desc
            .usb
            .as_ref()
            .ok_or_else(|| unsupported("not a USB description"))?;
        if !matches!(
            usb.protocol,
            UsbProtocol::Si570 | UsbProtocol::Si570Legacy | UsbProtocol::Fifisdr
        ) {
            return Err(unsupported("not an Si570 USB protocol"));
        }
        if usb.multiplier == 0 {
            return Err(unsupported("the description has no Si570 multiplier"));
        }
        Ok(Si570UsbEngine {
            protocol: usb.protocol,
            multiplier: usb.multiplier,
            desc,
            pending: Pending::Nothing,
            rx: Vec::new(),
        })
    }

    // [@ANCHOR: radioctl_si570_usb_set_value]
    /// wValue of the set-frequency request on this board.
    fn set_value(&self) -> u16 {
        match self.protocol {
            UsbProtocol::Fifisdr => 0,
            _ => 0x700 + SI570_I2C_ADDRESS,
        }
    }
}

impl ProtocolEngine for Si570UsbEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_si570_usb_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let legacy = self.protocol == UsbProtocol::Si570Legacy;
        let (transfer, reply, pending) = match op {
            Op::GetFrequency(Vfo::A) if legacy => (
                UsbTransfer::ControlIn {
                    request_type: VENDOR_IN,
                    request: REQUEST_READ_REGISTERS,
                    value: SI570_I2C_ADDRESS,
                    index: 0,
                    length: 6,
                },
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
                Pending::ReadRegisters,
            ),
            Op::SetFrequency(Vfo::A, hz) if legacy => {
                let registers = registers_from_hz(*hz, self.multiplier)
                    .ok_or_else(|| unsupported("frequency is beyond the Si570's range"))?;
                (
                    UsbTransfer::ControlOut {
                        request_type: VENDOR_OUT,
                        request: REQUEST_SET_FREQ_REGISTERS,
                        value: self.set_value(),
                        index: 0,
                        data: registers.to_vec(),
                    },
                    ReplyKind::Ack,
                    Pending::Set,
                )
            }
            Op::GetFrequency(Vfo::A) => (
                UsbTransfer::ControlIn {
                    request_type: VENDOR_IN,
                    request: REQUEST_READ_FREQUENCY,
                    value: 0,
                    index: 0,
                    length: 4,
                },
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
                Pending::ReadFrequency,
            ),
            Op::SetFrequency(Vfo::A, hz) => {
                let word = word_from_hz(*hz, self.multiplier)
                    .ok_or_else(|| unsupported("frequency is beyond the Si570's range"))?;
                (
                    UsbTransfer::ControlOut {
                        request_type: VENDOR_OUT,
                        request: REQUEST_SET_FREQ_BY_VALUE,
                        value: self.set_value(),
                        index: 0,
                        data: word.to_le_bytes().to_vec(),
                    },
                    ReplyKind::Ack,
                    Pending::Set,
                )
            }
            Op::SetPtt(on)
                if matches!(self.protocol, UsbProtocol::Si570 | UsbProtocol::Si570Legacy) =>
            {
                (
                    UsbTransfer::ControlIn {
                        request_type: VENDOR_IN,
                        request: REQUEST_SET_PTT,
                        value: u16::from(*on),
                        index: 0,
                        length: 3,
                    },
                    ReplyKind::Ack,
                    Pending::Set,
                )
            }
            _ => return Err(unsupported("not implemented for the Si570 USB engine")),
        };
        self.pending = pending;
        self.rx.clear();
        Ok(Request {
            bytes: transfer.encode(),
            reply,
        })
    }

    // [@ANCHOR: radioctl_si570_usb_engine_feed]
    fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound> {
        self.rx.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            let (reply, used) = match UsbReply::decode(&self.rx) {
                Ok(found) => found,
                Err(FrameError::Incomplete) => break,
                Err(FrameError::Malformed) => {
                    out.push(Inbound::Unrecognised(std::mem::take(&mut self.rx)));
                    break;
                }
            };
            self.rx.drain(..used);
            let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
            if matches!(reply.status, UsbStatus::Timeout | UsbStatus::NoDevice) {
                // Silence, as on a serial line: the actor ends the command in its own typed timeout.
                continue;
            }
            out.push(match (pending, reply.status) {
                (Pending::Nothing, _) => Inbound::Unrecognised(reply.encode()),
                (Pending::Set, UsbStatus::Ok) => Inbound::Ack,
                (Pending::ReadFrequency, UsbStatus::Ok) => {
                    match <[u8; 4]>::try_from(reply.data.as_slice())
                        .ok()
                        .and_then(|word| hz_from_word(u32::from_le_bytes(word), self.multiplier))
                    {
                        Some(hz) => Inbound::Observation(Observation::Frequency(Vfo::A, hz)),
                        None => Inbound::Unrecognised(reply.encode()),
                    }
                }
                (Pending::ReadRegisters, UsbStatus::Ok) => {
                    match <[u8; 6]>::try_from(reply.data.as_slice())
                        .ok()
                        .and_then(|regs| hz_from_registers(&regs, self.multiplier))
                    {
                        Some(hz) => Inbound::Observation(Observation::Frequency(Vfo::A, hz)),
                        None => Inbound::Unrecognised(reply.encode()),
                    }
                }
                (_, _) => Inbound::Nak,
            });
        }
        out
    }
}

/// A virtual Si570 USB board: a [`UsbDevice`] that answers the requests above.
#[derive(Debug)]
pub struct Si570Twin {
    protocol: UsbProtocol,
    word: u32,
    registers: [u8; 6],
    ptt: bool,
}

impl Si570Twin {
    // [@ANCHOR: radioctl_si570_usb_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<Si570Twin, RigError> {
        let usb = desc
            .usb
            .as_ref()
            .ok_or_else(|| unsupported("not a USB description"))?;
        let word = word_from_hz(desc.default_frequency, usb.multiplier)
            .ok_or_else(|| unsupported("default frequency is beyond the Si570's range"))?;
        let registers = registers_from_hz(desc.default_frequency, usb.multiplier).unwrap_or([0; 6]);
        Ok(Si570Twin {
            protocol: usb.protocol,
            word,
            registers,
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_si570_usb_twin_ptt]
    /// Whether the board is keyed.
    pub fn ptt(&self) -> bool {
        self.ptt
    }
}

impl UsbDevice for Si570Twin {
    // [@ANCHOR: radioctl_si570_usb_twin_control]
    fn control_in(
        &mut self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
        _timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        match (request_type, request, index, length) {
            (VENDOR_IN, REQUEST_READ_FREQUENCY, 0, 4)
                if self.protocol != UsbProtocol::Si570Legacy =>
            {
                Ok(self.word.to_le_bytes().to_vec())
            }
            (VENDOR_IN, REQUEST_READ_REGISTERS, 0, 6)
                if self.protocol == UsbProtocol::Si570Legacy && value == SI570_I2C_ADDRESS =>
            {
                Ok(self.registers.to_vec())
            }
            (VENDOR_IN, REQUEST_SET_PTT, 0, 3)
                if matches!(self.protocol, UsbProtocol::Si570 | UsbProtocol::Si570Legacy)
                    && value <= 1 =>
            {
                self.ptt = value == 1;
                Ok(vec![0, 0, 0])
            }
            _ => Err(UsbFailure::Stall),
        }
    }

    // [@ANCHOR: radioctl_si570_usb_twin_control_out]
    fn control_out(
        &mut self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
        _timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        let expected = match self.protocol {
            UsbProtocol::Fifisdr => 0,
            _ => 0x700 + SI570_I2C_ADDRESS,
        };
        if self.protocol == UsbProtocol::Si570Legacy {
            return match (
                request_type,
                request,
                value,
                index,
                <[u8; 6]>::try_from(data),
            ) {
                (VENDOR_OUT, REQUEST_SET_FREQ_REGISTERS, v, 0, Ok(registers)) if v == expected => {
                    self.registers = registers;
                    Ok(data.len())
                }
                _ => Err(UsbFailure::Stall),
            };
        }
        match (
            request_type,
            request,
            value,
            index,
            <[u8; 4]>::try_from(data),
        ) {
            (VENDOR_OUT, REQUEST_SET_FREQ_BY_VALUE, v, 0, Ok(word)) if v == expected => {
                self.word = u32::from_le_bytes(word);
                Ok(data.len())
            }
            _ => Err(UsbFailure::Stall),
        }
    }

    fn bulk_write(
        &mut self,
        _endpoint: u8,
        _data: &[u8],
        _timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        Err(UsbFailure::Stall)
    }

    fn bulk_read(
        &mut self,
        _endpoint: u8,
        _length: usize,
        _timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        Err(UsbFailure::Stall)
    }

    fn interrupt_write(
        &mut self,
        _endpoint: u8,
        _data: &[u8],
        _timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        Err(UsbFailure::Stall)
    }

    fn interrupt_read(
        &mut self,
        _endpoint: u8,
        _length: usize,
        _timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        Err(UsbFailure::Stall)
    }
}

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
//! Firmware older than 0x0F00 (DG8SAQ) takes request 0x30 with the Si570 registers solved on the
//! host; that is not implemented, and neither is the FiFi-SDR's own read and write requests
//! (0xAB and 0xAC), which carry levels and the VCO factor. The multiplier comes from the
//! description, not from the board.

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
/// Set PTT.
pub const REQUEST_SET_PTT: u8 = 0x50;
/// The Si570's I2C address as the kits use it.
const SI570_I2C_ADDRESS: u16 = 0x55;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Set,
    ReadFrequency,
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
        if !matches!(usb.protocol, UsbProtocol::Si570 | UsbProtocol::Fifisdr) {
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
        let (transfer, reply, pending) = match op {
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
            Op::SetPtt(on) if self.protocol == UsbProtocol::Si570 => (
                UsbTransfer::ControlIn {
                    request_type: VENDOR_IN,
                    request: REQUEST_SET_PTT,
                    value: u16::from(*on),
                    index: 0,
                    length: 3,
                },
                ReplyKind::Ack,
                Pending::Set,
            ),
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
        Ok(Si570Twin {
            protocol: usb.protocol,
            word,
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
            (VENDOR_IN, REQUEST_READ_FREQUENCY, 0, 4) => Ok(self.word.to_le_bytes().to_vec()),
            (VENDOR_IN, REQUEST_SET_PTT, 0, 3)
                if self.protocol == UsbProtocol::Si570 && value <= 1 =>
            {
                self.ptt = value == 1;
                Ok(vec![0, 0, 0])
            }
            _ => Err(UsbFailure::Stall),
        }
    }

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

// SPDX-License-Identifier: AGPL-3.0-or-later
//! Digital World Traveller DRM receiver (Coding Technologies, USB 1539:1730): set-frequency
//! only. Sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/kit/dwt.c`, the
//! libusb build. Hamlib marks the request "FIXME" and implements no read; its Windows build goes
//! through the vendor's DLL instead, which radioctl does not use. Nothing is verified against a
//! receiver.
//!
//! Setting the frequency is one control transfer to the device: request type 0, request 0,
//! wValue 0, wIndex 0, nine data bytes `4A 00 03 00 FF FF 32` followed by the frequency in
//! whole kilohertz, high byte first. Hamlib truncates the kilohertz to 16 bits, which would wrap
//! the broadcast FM band (87.5 to 108 MHz) that the description lists; this engine refuses a
//! frequency above 65535 kHz instead (an open question for a volunteer with the receiver).

use std::time::Duration;

use radioctl_core::description::{RigDescription, UsbProtocol};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::units::Vfo;
use radioctl_core::usb_device::{UsbDevice, UsbFailure};
use radioctl_core::usb_wire::{FrameError, UsbReply, UsbStatus, UsbTransfer};

const PREFIX: [u8; 7] = [0x4A, 0x00, 0x03, 0x00, 0xFF, 0xFF, 0x32];

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

/// The Digital World Traveller client engine.
#[derive(Debug)]
pub struct DwtEngine {
    desc: RigDescription,
    waiting: bool,
    rx: Vec<u8>,
}

impl DwtEngine {
    // [@ANCHOR: radioctl_dwt_engine_new]
    /// Builds an engine from a `dwt` USB description.
    pub fn new(desc: RigDescription) -> Result<DwtEngine, RigError> {
        match desc.usb.as_ref().map(|u| u.protocol) {
            Some(UsbProtocol::Dwt) => Ok(DwtEngine {
                desc,
                waiting: false,
                rx: Vec::new(),
            }),
            _ => Err(unsupported("not a Digital World Traveller description")),
        }
    }
}

impl ProtocolEngine for DwtEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_dwt_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let Op::SetFrequency(Vfo::A, hz) = op else {
            return Err(unsupported("the Traveller can only be tuned"));
        };
        let khz = u16::try_from(hz.0 / 1000)
            .map_err(|_| unsupported("the request carries 16 bits of kilohertz"))?;
        let mut data = PREFIX.to_vec();
        data.extend_from_slice(&khz.to_be_bytes());
        self.waiting = true;
        self.rx.clear();
        Ok(Request {
            bytes: UsbTransfer::ControlOut {
                request_type: 0,
                request: 0,
                value: 0,
                index: 0,
                data,
            }
            .encode(),
            reply: ReplyKind::Ack,
        })
    }

    // [@ANCHOR: radioctl_dwt_engine_feed]
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
            if !std::mem::replace(&mut self.waiting, false) {
                out.push(Inbound::Unrecognised(reply.encode()));
                continue;
            }
            match reply.status {
                UsbStatus::Timeout | UsbStatus::NoDevice => {}
                UsbStatus::Ok => out.push(Inbound::Ack),
                _ => out.push(Inbound::Nak),
            }
        }
        out
    }
}

/// A virtual Traveller: remembers the last kilohertz it was tuned to.
#[derive(Debug, Default)]
pub struct DwtTwin {
    khz: Option<u16>,
}

impl DwtTwin {
    // [@ANCHOR: radioctl_dwt_twin_khz]
    /// The kilohertz the receiver was last tuned to, if it has been tuned.
    pub fn khz(&self) -> Option<u16> {
        self.khz
    }
}

impl UsbDevice for DwtTwin {
    fn control_in(
        &mut self,
        _t: u8,
        _r: u8,
        _v: u16,
        _i: u16,
        _l: u16,
        _timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        Err(UsbFailure::Stall)
    }

    // [@ANCHOR: radioctl_dwt_twin_control_out]
    fn control_out(
        &mut self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
        _timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        match (
            request_type,
            request,
            value,
            index,
            data.get(..7),
            data.get(7..),
        ) {
            (0, 0, 0, 0, Some(prefix), Some([hi, lo])) if prefix == PREFIX => {
                self.khz = Some(u16::from_be_bytes([*hi, *lo]));
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

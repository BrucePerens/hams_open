// SPDX-License-Identifier: AGPL-3.0-or-later
//! AMSAT-UK FUNcube Dongle and Dongle Pro+: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/kit/funcube.c` and
//! `funcube.h`. No vendor document was read and nothing is verified against a dongle.
//!
//! Hamlib reaches the dongle with libusb interrupt transfers on the HID interface (interface 2),
//! not with a HID library, so radioctl needs only libusb here: a 64-byte report is written to
//! endpoint 0x02 and a 64-byte report is read from endpoint 0x82. The engine's request is one
//! [`UsbTransfer::InterruptExchange`] frame. Byte 0 of a command is the command number; byte 0 of
//! the answer repeats it and byte 1 is 1 when the dongle accepted it.
//!
//! * 0x65 set frequency in hertz, bytes 1-4 little-endian (firmware with the hertz commands);
//! * 0x66 read frequency, answer bytes 2-5 little-endian hertz;
//! * 0x64 set frequency in kilohertz, bytes 1-3 little-endian, for older firmware that refuses 0x65.
//!   Older firmware cannot read the frequency back at all; Hamlib reports the last value set.
//!
//! Sans-IO has one request per operation, so Hamlib's "try 0x65, then 0x64" fallback is spread over
//! two calls: when the dongle answers 0x65 with a refusal the engine answers `Nak`, remembers the
//! firmware is old, and encodes the next set as 0x64. A read on old firmware is refused as
//! unsupported (the daemon's cache holds the last value set). LNA, mixer and IF gain commands
//! (0x6E, 0x72, 0x75 and their reads) and RSSI (0x68) are not implemented.

use std::time::Duration;

use radioctl_core::description::{RigDescription, UsbProtocol};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Vfo};
use radioctl_core::usb_device::{UsbDevice, UsbFailure};
use radioctl_core::usb_wire::{FrameError, UsbReply, UsbStatus, UsbTransfer};

/// Interrupt OUT endpoint.
pub const OUTPUT_ENDPOINT: u8 = 0x02;
/// Interrupt IN endpoint.
pub const INPUT_ENDPOINT: u8 = 0x82;
/// The interface the HID endpoints belong to.
pub const INTERFACE: u8 = 2;
/// Report size.
pub const REPORT: usize = 64;
/// Set frequency, kilohertz (older firmware).
pub const REQUEST_SET_FREQ: u8 = 0x64;
/// Set frequency, hertz.
pub const REQUEST_SET_FREQ_HZ: u8 = 0x65;
/// Read frequency, hertz.
pub const REQUEST_GET_FREQ_HZ: u8 = 0x66;
/// The success byte of an answer.
pub const SUCCESS: u8 = 0x01;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_funcube_command_report]
/// The 64-byte report for command `command` with argument bytes `args`.
pub fn command_report(command: u8, args: &[u8]) -> Vec<u8> {
    let mut report = vec![0u8; REPORT];
    if let Some(slot) = report.first_mut() {
        *slot = command;
    }
    for (slot, byte) in report.iter_mut().skip(1).zip(args) {
        *slot = *byte;
    }
    report
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    SetHz,
    SetKhz,
    ReadFrequency,
}

/// The FUNcube client engine.
#[derive(Debug)]
pub struct FuncubeEngine {
    desc: RigDescription,
    legacy: bool,
    pending: Pending,
    rx: Vec<u8>,
}

impl FuncubeEngine {
    // [@ANCHOR: radioctl_funcube_engine_new]
    /// Builds an engine from a `funcube` USB description.
    pub fn new(desc: RigDescription) -> Result<FuncubeEngine, RigError> {
        match desc.usb.as_ref().map(|u| u.protocol) {
            Some(UsbProtocol::Funcube) => Ok(FuncubeEngine {
                desc,
                legacy: false,
                pending: Pending::Nothing,
                rx: Vec::new(),
            }),
            _ => Err(unsupported("not a FUNcube description")),
        }
    }

    // [@ANCHOR: radioctl_funcube_exchange]
    fn exchange(command: u8, args: &[u8]) -> Vec<u8> {
        UsbTransfer::InterruptExchange {
            out_endpoint: OUTPUT_ENDPOINT,
            in_endpoint: INPUT_ENDPOINT,
            data: command_report(command, args),
            read_length: REPORT as u16,
        }
        .encode()
    }
}

impl ProtocolEngine for FuncubeEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_funcube_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (bytes, reply, pending) = match op {
            Op::GetFrequency(Vfo::A) if !self.legacy => (
                Self::exchange(REQUEST_GET_FREQ_HZ, &[]),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
                Pending::ReadFrequency,
            ),
            Op::GetFrequency(Vfo::A) => {
                return Err(unsupported(
                    "this firmware cannot read the frequency back; the last value set is cached",
                ))
            }
            Op::SetFrequency(Vfo::A, hz) if !self.legacy => {
                let hz = u32::try_from(hz.0)
                    .map_err(|_| unsupported("frequency does not fit in 32 bits"))?;
                (
                    Self::exchange(REQUEST_SET_FREQ_HZ, &hz.to_le_bytes()),
                    ReplyKind::Ack,
                    Pending::SetHz,
                )
            }
            Op::SetFrequency(Vfo::A, hz) => {
                let khz = u32::try_from(hz.0 / 1000)
                    .ok()
                    .filter(|k| *k < (1 << 24))
                    .ok_or_else(|| unsupported("frequency does not fit in 24 bits of kilohertz"))?;
                let le = khz.to_le_bytes();
                (
                    Self::exchange(REQUEST_SET_FREQ, le.get(..3).unwrap_or(&[])),
                    ReplyKind::Ack,
                    Pending::SetKhz,
                )
            }
            _ => return Err(unsupported("not implemented for the FUNcube engine")),
        };
        self.pending = pending;
        self.rx.clear();
        Ok(Request { bytes, reply })
    }

    // [@ANCHOR: radioctl_funcube_engine_feed]
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
            let command = match pending {
                Pending::SetHz => REQUEST_SET_FREQ_HZ,
                Pending::SetKhz => REQUEST_SET_FREQ,
                Pending::ReadFrequency => REQUEST_GET_FREQ_HZ,
                Pending::Nothing => {
                    out.push(Inbound::Unrecognised(reply.encode()));
                    continue;
                }
            };
            let accepted = reply.status == UsbStatus::Ok
                && reply.data.first() == Some(&command)
                && reply.data.get(1) == Some(&SUCCESS);
            if reply.status == UsbStatus::Ok && !accepted && pending == Pending::SetHz {
                // The dongle answered but refused the hertz command: old firmware.
                self.legacy = true;
            }
            out.push(match (accepted, pending) {
                (false, _) => Inbound::Nak,
                (true, Pending::ReadFrequency) => {
                    match reply
                        .data
                        .get(2..6)
                        .and_then(|w| <[u8; 4]>::try_from(w).ok())
                    {
                        Some(w) => Inbound::Observation(Observation::Frequency(
                            Vfo::A,
                            Hz(u64::from(u32::from_le_bytes(w))),
                        )),
                        None => Inbound::Unrecognised(reply.encode()),
                    }
                }
                (true, _) => Inbound::Ack,
            });
        }
        out
    }
}

/// A virtual FUNcube dongle: a [`UsbDevice`] answering the HID reports above.
#[derive(Debug)]
pub struct FuncubeTwin {
    hz: u32,
    legacy: bool,
    answer: Vec<u8>,
}

impl FuncubeTwin {
    // [@ANCHOR: radioctl_funcube_twin_new]
    /// A twin in the description's default state; `legacy` makes it an older dongle that refuses
    /// the hertz commands.
    pub fn new(desc: &RigDescription, legacy: bool) -> Result<FuncubeTwin, RigError> {
        let hz = u32::try_from(desc.default_frequency.0)
            .map_err(|_| unsupported("default frequency does not fit in 32 bits"))?;
        Ok(FuncubeTwin {
            hz,
            legacy,
            answer: Vec::new(),
        })
    }

    // [@ANCHOR: radioctl_funcube_twin_frequency]
    /// The frequency the dongle is tuned to.
    pub fn frequency(&self) -> Hz {
        Hz(u64::from(self.hz))
    }
}

impl UsbDevice for FuncubeTwin {
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

    fn control_out(
        &mut self,
        _t: u8,
        _r: u8,
        _v: u16,
        _i: u16,
        _d: &[u8],
        _timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        Err(UsbFailure::Stall)
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

    // [@ANCHOR: radioctl_funcube_twin_interrupt]
    fn interrupt_write(
        &mut self,
        endpoint: u8,
        data: &[u8],
        _timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        if endpoint != OUTPUT_ENDPOINT || data.len() != REPORT {
            return Err(UsbFailure::Stall);
        }
        let command = data.first().copied().unwrap_or(0);
        let word = |from: usize, len: usize| -> u32 {
            data.get(from..from + len)
                .map(|b| {
                    b.iter()
                        .rev()
                        .fold(0u32, |acc, byte| (acc << 8) | u32::from(*byte))
                })
                .unwrap_or(0)
        };
        let mut success = true;
        let mut payload = Vec::new();
        match command {
            REQUEST_SET_FREQ_HZ if !self.legacy => self.hz = word(1, 4),
            REQUEST_GET_FREQ_HZ if !self.legacy => payload = self.hz.to_le_bytes().to_vec(),
            REQUEST_SET_FREQ => self.hz = word(1, 3).saturating_mul(1000),
            _ => success = false,
        }
        self.answer = command_report(command, &[]);
        if let Some(flag) = self.answer.get_mut(1) {
            *flag = u8::from(success);
        }
        for (slot, byte) in self.answer.iter_mut().skip(2).zip(&payload) {
            *slot = *byte;
        }
        Ok(data.len())
    }

    // [@ANCHOR: radioctl_funcube_twin_interrupt_read]
    fn interrupt_read(
        &mut self,
        endpoint: u8,
        length: usize,
        _timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        if endpoint != INPUT_ENDPOINT || self.answer.is_empty() {
            return Err(UsbFailure::Timeout);
        }
        let mut answer = std::mem::take(&mut self.answer);
        answer.truncate(length);
        Ok(answer)
    }
}

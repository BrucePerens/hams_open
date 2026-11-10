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
//! unsupported (the daemon's cache holds the last value set).
//!
//! Gain and signal commands, same framing (Hamlib `funcube.c` `funcube_set_level`,
//! `funcube_get_level`, `funcubepro_set_level`, `funcubepro_get_level`; `funcube.h` for the
//! numbers). The original Dongle (product id 0xFB56) has one LNA setting, written with 0x6E and
//! read with 0x96 as a code in answer byte 2: radioctl `preamp` steps 0 to 6 are the codes 4 (off),
//! 6, 8, 10, 12, 13, 14 (Hamlib's 5, 10, 15, 20, 25 and 30 dB), and `attenuator` values 0, 2 and 5 dB (steps 0 to 2) are
//! the codes 4 (off), 1 and 0 (Hamlib's 2 and 5 dB); a code outside the table reads as step 0. Its
//! RSSI is command 0x68, answer byte 2, reported raw as the signal. The Dongle Pro+ (0xFB31)
//! has three switches: the LNA (0x6E, read 0x96, `preamp` step 0 or 1, +10 dB), the mixer gain
//! (0x72, read 0x9A, +20 dB when on) and the IF gain (0x75, read 0x9D, 0 to 59 dB, `rf-gain`).
//! radioctl maps the mixer to `attenuator` step 1 (20 dB) meaning the mixer gain is *off*, so
//! step 0 is the full-gain state; the Pro+ has no RSSI command in Hamlib. Each set is acknowledged
//! as the frequency commands are. Nothing here is verified on a dongle.

use std::time::Duration;

use radioctl_core::description::{RigDescription, UsbProtocol};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Level, Vfo};
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
/// Set the LNA (gain code on the original Dongle, 0 or 1 on the Pro+).
pub const REQUEST_SET_LNA: u8 = 0x6E;
/// Set the mixer gain (Pro+).
pub const REQUEST_SET_MIXER: u8 = 0x72;
/// Set the IF gain (Pro+).
pub const REQUEST_SET_IF: u8 = 0x75;
/// Read the LNA.
pub const REQUEST_GET_LNA: u8 = 0x96;
/// Read the mixer gain (Pro+).
pub const REQUEST_GET_MIXER: u8 = 0x9A;
/// Read the IF gain (Pro+).
pub const REQUEST_GET_IF: u8 = 0x9D;
/// Read the RSSI (original Dongle).
pub const REQUEST_GET_RSSI: u8 = 0x68;
/// USB product id of the Pro+.
pub const PRO_PLUS_PRODUCT_ID: u16 = 0xFB31;
/// LNA codes of the original Dongle by `preamp` step.
const PREAMP_CODES: [u8; 7] = [4, 6, 8, 10, 12, 13, 14];
/// LNA codes of the original Dongle by `attenuator` step.
const ATTENUATOR_CODES: [u8; 3] = [4, 1, 0];
/// The largest IF gain, decibels.
const IF_GAIN_MAX: u16 = 59;
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
    /// A gain set; the command whose answer is expected.
    SetGain(u8),
    /// A read of the given command, decoded as given.
    Read(u8, Reading),
}

/// How the answer byte of a read is turned into an observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    ProPreamp,
    ProAttenuator,
    IfGain,
    Preamp,
    Attenuator,
    Signal,
}

/// The step whose code is `code` in `table`, 0 when it is not there.
fn code_step(table: &[u8], code: u8) -> u16 {
    table
        .iter()
        .position(|c| *c == code)
        .and_then(|i| u16::try_from(i).ok())
        .unwrap_or(0)
}

/// The FUNcube client engine.
#[derive(Debug)]
pub struct FuncubeEngine {
    desc: RigDescription,
    pro: bool,
    legacy: bool,
    pending: Pending,
    rx: Vec<u8>,
}

impl FuncubeEngine {
    // [@ANCHOR: radioctl_funcube_engine_new]
    /// Builds an engine from a `funcube` USB description.
    pub fn new(desc: RigDescription) -> Result<FuncubeEngine, RigError> {
        match desc.usb.as_ref().map(|u| (u.protocol, u.product_id)) {
            Some((UsbProtocol::Funcube, product_id)) => Ok(FuncubeEngine {
                pro: product_id == PRO_PLUS_PRODUCT_ID,
                desc,
                legacy: false,
                pending: Pending::Nothing,
                rx: Vec::new(),
            }),
            _ => Err(unsupported("not a FUNcube description")),
        }
    }

    // [@ANCHOR: radioctl_funcube_gain_value]
    /// The command and its argument byte for setting `level` to `step` on this dongle.
    fn gain_value(&self, level: Level, value: u16) -> Result<(u8, u8), RigError> {
        let bad = || unsupported("no such level or step on this dongle");
        // The attenuator value is decibels and the preamp value a step; the description maps both.
        let step = match level {
            Level::Preamp | Level::Attenuator => {
                usize::from(crate::attenuation::step_of(&self.desc, level, value)?)
            }
            _ => usize::from(u8::try_from(value).map_err(|_| bad())?),
        };
        match (self.pro, level) {
            (true, Level::Preamp) if step <= 1 => Ok((REQUEST_SET_LNA, u8::from(step == 1))),
            // Step 1 is 20 dB of attenuation: the mixer's +20 dB gain switched off.
            (true, Level::Attenuator) if step <= 1 => Ok((REQUEST_SET_MIXER, u8::from(step == 0))),
            (true, Level::RfGain) => Ok((
                REQUEST_SET_IF,
                u8::try_from(step.min(usize::from(IF_GAIN_MAX))).map_err(|_| bad())?,
            )),
            (false, Level::Preamp) => PREAMP_CODES
                .get(step)
                .map(|c| (REQUEST_SET_LNA, *c))
                .ok_or_else(bad),
            (false, Level::Attenuator) => ATTENUATOR_CODES
                .get(step)
                .map(|c| (REQUEST_SET_LNA, *c))
                .ok_or_else(bad),
            _ => Err(bad()),
        }
    }

    // [@ANCHOR: radioctl_funcube_gain_read]
    /// The command and the decoding for reading `level` on this dongle.
    fn gain_read(&self, level: Level) -> Result<(u8, Reading), RigError> {
        match (self.pro, level) {
            (true, Level::Preamp) => Ok((REQUEST_GET_LNA, Reading::ProPreamp)),
            (true, Level::Attenuator) => Ok((REQUEST_GET_MIXER, Reading::ProAttenuator)),
            (true, Level::RfGain) => Ok((REQUEST_GET_IF, Reading::IfGain)),
            (false, Level::Preamp) => Ok((REQUEST_GET_LNA, Reading::Preamp)),
            (false, Level::Attenuator) => Ok((REQUEST_GET_LNA, Reading::Attenuator)),
            _ => Err(unsupported("no such level on this dongle")),
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
            Op::SetLevel(level, step) => {
                let (command, value) = self.gain_value(*level, *step)?;
                (
                    Self::exchange(command, &[value]),
                    ReplyKind::Ack,
                    Pending::SetGain(command),
                )
            }
            Op::GetLevel(level) => {
                let (command, reading) = self.gain_read(*level)?;
                (
                    Self::exchange(command, &[]),
                    ReplyKind::Observation(Field::Level(*level)),
                    Pending::Read(command, reading),
                )
            }
            Op::GetSignal if !self.pro => (
                Self::exchange(REQUEST_GET_RSSI, &[]),
                ReplyKind::Observation(Field::Signal),
                Pending::Read(REQUEST_GET_RSSI, Reading::Signal),
            ),
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
                Pending::SetGain(command) | Pending::Read(command, _) => command,
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
                (true, Pending::Read(_, reading)) => {
                    let Some(byte) = reply.data.get(2).copied() else {
                        out.push(Inbound::Unrecognised(reply.encode()));
                        continue;
                    };
                    // The wire step of a front-end level, then the level value the description
                    // gives it (decibels for the attenuator).
                    let front_end = match reading {
                        Reading::ProPreamp => Some((Level::Preamp, u16::from(byte & 1))),
                        // Mixer gain on is the full-gain state, so attenuator step 0.
                        Reading::ProAttenuator => {
                            Some((Level::Attenuator, u16::from(1 - (byte & 1))))
                        }
                        Reading::Preamp => Some((Level::Preamp, code_step(&PREAMP_CODES, byte))),
                        Reading::Attenuator => {
                            Some((Level::Attenuator, code_step(&ATTENUATOR_CODES, byte)))
                        }
                        Reading::IfGain | Reading::Signal => None,
                    };
                    match (front_end, reading) {
                        (Some((level, step)), _) => u8::try_from(step)
                            .ok()
                            .and_then(|s| crate::attenuation::value_of(&self.desc, level, s))
                            .map(|v| Inbound::Observation(Observation::Level(level, v)))
                            .unwrap_or_else(|| Inbound::Unrecognised(reply.encode())),
                        (None, Reading::IfGain) => {
                            Inbound::Observation(Observation::Level(Level::RfGain, u16::from(byte)))
                        }
                        (None, _) => Inbound::Observation(Observation::Signal(u16::from(byte))),
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
    pro: bool,
    lna: u8,
    mixer: u8,
    if_gain: u8,
    rssi: u8,
    answer: Vec<u8>,
}

impl FuncubeTwin {
    // [@ANCHOR: radioctl_funcube_twin_new]
    /// A twin in the description's default state; `legacy` makes it an older dongle that refuses
    /// the hertz commands.
    pub fn new(desc: &RigDescription, legacy: bool) -> Result<FuncubeTwin, RigError> {
        let hz = u32::try_from(desc.default_frequency.0)
            .map_err(|_| unsupported("default frequency does not fit in 32 bits"))?;
        let pro = desc.usb.as_ref().map(|u| u.product_id) == Some(PRO_PLUS_PRODUCT_ID);
        Ok(FuncubeTwin {
            hz,
            legacy,
            pro,
            // The original Dongle's off code is 4; the Pro+ switches are off at 0.
            lna: if pro { 0 } else { 4 },
            mixer: 0,
            if_gain: 0,
            rssi: 0,
            answer: Vec::new(),
        })
    }

    /// Sets the RSSI byte the original Dongle's twin reports.
    pub fn set_rssi(&mut self, rssi: u8) {
        self.rssi = rssi;
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
            REQUEST_SET_LNA => self.lna = data.get(1).copied().unwrap_or(0),
            REQUEST_GET_LNA => payload = vec![self.lna],
            REQUEST_SET_MIXER if self.pro => self.mixer = data.get(1).copied().unwrap_or(0),
            REQUEST_GET_MIXER if self.pro => payload = vec![self.mixer],
            REQUEST_SET_IF if self.pro => self.if_gain = data.get(1).copied().unwrap_or(0),
            REQUEST_GET_IF if self.pro => payload = vec![self.if_gain],
            REQUEST_GET_RSSI if !self.pro => payload = vec![self.rssi],
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

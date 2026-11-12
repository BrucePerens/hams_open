// SPDX-License-Identifier: AGPL-3.0-or-later
//! Elektor SDR-USB (Elektor 5/07): sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/kit/elektor507.c`
//! (the libusb build: `elektor507_libusb_setup`, `elektor507_open`, `elektor507_set_freq`,
//! `find_P_Q_DIV1N`, `cy_update_pll`, `elektor507_set_level`, `i2c_write_regs` and the `ftdi_*`
//! bit-bang helpers). No code or comment was copied and nothing is verified against a board.
//!
//! The board is a QSD receiver whose local oscillator is a Cypress CY27EE16ZE clock chip. An FTDI
//! FT232R in asynchronous bit-bang mode (all eight pins outputs) is the only link: each byte
//! written to the bulk OUT endpoint 0x02 sets the eight pins for one sample. Pin 0 is the I2C data
//! line, pin 1 the clock, pins 2 to 4 the input multiplexer (the band filter or antenna), pins 5
//! and 6 the attenuator. The chip is therefore programmed by writing I2C transactions as pin
//! samples; the engine builds the samples and the twin decodes them again.
//!
//! * Opening sets the FT232R up with three vendor control transfers (reset; bit-bang mode with the
//!   all-outputs mask; baud rate value 49230, which Hamlib calls 38400), then writes the clock chip:
//!   output enable (register 0x09), divider 1 (0x0C), the clock source registers (0x44 to 0x46) and
//!   divider 2 source (0x47).
//! * Tuning solves the chip's pump, counter and divider settings on the host from the oscillator
//!   (fixed here at 10 MHz; Hamlib has a setting for it) so that the pump times the reference over
//!   the divider is four times the frequency, then writes registers 0x40 to 0x42, 0x0C and 0x46.
//!   The input multiplexer is chosen from the frequency too (Hamlib's automatic antenna mode):
//!   the 1.6 MHz low-pass path up to 1.6 MHz, the high-pass path above.
//! * The attenuator is 0, 10 or 20 dB, two pins.
//!
//! Nothing can be read back from the board (Hamlib answers reads from its own state), so the engine
//! refuses reads and the daemon's cache holds the last value set. Hamlib's antenna selection (an
//! external antenna and a 5 MHz test clock) has no radioctl operation and is not implemented.
//! Hamlib writes each register group as its own bulk transfer; the engine joins the groups of one
//! operation into a single transfer, which puts the same samples on the pins in the same order.

use std::time::Duration;

use radioctl_core::description::{RigDescription, UsbProtocol};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::units::{Hz, Level, Vfo};
use radioctl_core::usb_device::{UsbDevice, UsbFailure};
use radioctl_core::usb_wire::{FrameError, UsbReply, UsbStatus, UsbTransfer};

/// The bulk OUT endpoint of the FT232R.
pub const BULK_OUT: u8 = 0x02;
/// Vendor control transfer to the device (request type).
pub const VENDOR_OUT: u8 = 0x40;
/// FTDI request: reset.
pub const FTDI_RESET: u8 = 0;
/// FTDI request: baud rate.
pub const FTDI_BAUD: u8 = 3;
/// FTDI request: bit-bang mode.
pub const FTDI_BITMODE: u8 = 0x0B;
/// wValue of the bit-bang request: mode 1 in the high byte, all eight pins outputs in the low.
pub const BITMODE_VALUE: u16 = 0x01FF;
/// wValue of the baud request that Hamlib comments as 38400 baud.
pub const BAUD_VALUE: u16 = 49230;
/// The clock chip's I2C write address.
pub const CHIP_ADDRESS: u8 = 210;
/// Clock output enable register.
pub const REG_CLKOE: u8 = 0x09;
/// Output divider 1 register.
pub const REG_DIV1: u8 = 0x0C;
/// First of the three pump and counter registers.
pub const REG_PUMP: u8 = 0x40;
/// First of the three clock source registers.
pub const REG_CLKSRC: u8 = 0x44;
/// Divider 2 source register.
pub const REG_DIV2: u8 = 0x47;
/// The oscillator on the board, in hertz (Hamlib's default).
pub const OSCILLATOR_HZ: u32 = 10_000_000;
/// Lowest frequency the receiver covers (Hamlib's range).
pub const MIN_HZ: u64 = 30_000;
/// Highest frequency the receiver covers (Hamlib's range: 30 MHz less 1 kHz).
pub const MAX_HZ: u64 = 29_999_000;

const PIN_SDA: u8 = 0x01;
const PIN_SCL: u8 = 0x02;
const MUX_MASK: u8 = 0x1C;
const ATTENUATOR_SHIFT: u8 = 5;
const PORT_IDLE: u8 = PIN_SDA | PIN_SCL;
const VCO_MIN: f64 = 100_000_000.0;
const VCO_MAX: f64 = 400_000_000.0;
const Q_MAX: u32 = 40;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

/// The PLL settings: total pump `p`, total counter `q` and divider 1 `div`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pll {
    /// Total pump (P).
    pub p: u32,
    /// Total reference counter (Q).
    pub q: u32,
    /// Divider 1 as the frequency formula uses it (2 to 127).
    pub div: u32,
}

impl Pll {
    /// The settings the chip is given when it is opened.
    pub const SAFE: Pll = Pll { p: 8, q: 2, div: 8 };

    fn delta(self, quad: f64) -> f64 {
        (f64::from(self.p) * (f64::from(OSCILLATOR_HZ) / f64::from(self.q)) / f64::from(self.div)
            - quad)
            .abs()
    }

    // [@ANCHOR: radioctl_elektor507_pll_frequency]
    /// The frequency the settings give: the pump times the reference over the divider, over four
    /// (the quadrature detector divides by four).
    pub fn frequency(self) -> f64 {
        f64::from(self.p) * (f64::from(OSCILLATOR_HZ) / f64::from(self.q))
            / f64::from(self.div)
            / 4.0
    }

    // [@ANCHOR: radioctl_elektor507_pll_solve]
    /// The settings nearest to `hz`, searched over every counter 2 to 40 and every pump that keeps
    /// the voltage-controlled oscillator between 100 and 400 MHz. `self` stays if nothing beats it.
    pub fn solve(self, hz: u64) -> Pll {
        let quad = hz as f64 * 4.0;
        let reference = f64::from(OSCILLATOR_HZ);
        let mut best = self;
        let mut best_delta = self.delta(quad);
        for q in 2..=Q_MAX {
            let per_q = reference / f64::from(q);
            let p_min = (VCO_MIN / per_q) as u32;
            let p_max = (VCO_MAX / per_q) as u32;
            for p in p_min..=p_max {
                let vco = f64::from(p) * per_q;
                let div = (((vco + quad / 2.0) / quad) as u32).clamp(2, 127);
                let candidate = Pll { p, q, div };
                let delta = candidate.delta(quad);
                if delta < best_delta {
                    best_delta = delta;
                    best = candidate;
                }
            }
        }
        best
    }

    // [@ANCHOR: radioctl_elektor507_pll_registers]
    /// The register groups that program the settings: (first register, values). Registers 0x40 to
    /// 0x42 take the pump range, the pump total and the counter; divider 1 follows, with the fixed
    /// /2 and /3 options of the clock source register for a divider of 2 or 3; the last group sets
    /// the clock source's two low bits.
    pub fn registers(self) -> [(u8, Vec<u8>); 3] {
        let pump_range: u32 = match self.p {
            0..=44 => 0,
            45..=479 => 1,
            480..=639 => 2,
            640..=799 => 3,
            _ => 4,
        };
        let half = (self.p >> 1).saturating_sub(4);
        let (div1, source) = match self.div {
            2 => (8u8, 0x80u8),
            3 => (6, 0xC0),
            d => (d as u8, 0x40),
        };
        [
            (
                REG_PUMP,
                vec![
                    ((half >> 8) as u8) | ((pump_range as u8) << 2) | 0xC0,
                    (half & 0xFF) as u8,
                    ((self.q.saturating_sub(2)) as u8) | (((self.p & 1) as u8) << 7),
                ],
            ),
            (REG_DIV1, vec![div1]),
            (REG_CLKSRC + 2, vec![source | 0x07]),
        ]
    }
}

/// Builds the pin samples of I2C write transactions.
#[derive(Debug)]
struct Samples<'a> {
    port: &'a mut u8,
    out: Vec<u8>,
}

impl Samples<'_> {
    // [@ANCHOR: radioctl_elektor507_samples_sda]
    fn sda(&mut self, high: bool) {
        *self.port = if high {
            *self.port | PIN_SDA
        } else {
            *self.port & !PIN_SDA
        };
        self.out.push(*self.port);
    }

    // [@ANCHOR: radioctl_elektor507_samples_scl]
    fn scl(&mut self, high: bool) {
        *self.port = if high {
            *self.port | PIN_SCL
        } else {
            *self.port & !PIN_SCL
        };
        self.out.push(*self.port);
    }

    // [@ANCHOR: radioctl_elektor507_samples_byte]
    fn byte(&mut self, value: u8) {
        for bit in (0..8).rev() {
            self.sda(value & (1 << bit) != 0);
            self.scl(true);
            self.scl(false);
        }
        // The acknowledge slot: the line is released and clocked once.
        self.sda(true);
        self.scl(true);
        self.scl(false);
    }

    // [@ANCHOR: radioctl_elektor507_samples_write]
    fn write(&mut self, register: u8, values: &[u8]) {
        self.scl(true);
        self.sda(true);
        self.sda(false);
        self.scl(false);
        self.byte(CHIP_ADDRESS);
        self.byte(register);
        for value in values {
            self.byte(*value);
        }
        self.scl(false);
        self.sda(false);
        self.scl(true);
        self.sda(true);
    }
}

fn bulk(data: Vec<u8>) -> Vec<u8> {
    UsbTransfer::BulkWrite {
        endpoint: BULK_OUT,
        data,
    }
    .encode()
}

fn vendor(request: u8, value: u16) -> Request {
    Request {
        bytes: UsbTransfer::ControlOut {
            request_type: VENDOR_OUT,
            request,
            value,
            index: 0,
            data: Vec::new(),
        }
        .encode(),
        reply: ReplyKind::Ack,
    }
}

/// The Elektor SDR-USB client engine.
#[derive(Debug)]
pub struct Elektor507Engine {
    desc: RigDescription,
    port: u8,
    pll: Pll,
    rx: Vec<u8>,
}

impl Elektor507Engine {
    // [@ANCHOR: radioctl_elektor507_engine_new]
    /// Builds an engine from an `elektor507` USB description.
    pub fn new(desc: RigDescription) -> Result<Elektor507Engine, RigError> {
        match desc.usb.as_ref().map(|u| u.protocol) {
            Some(UsbProtocol::Elektor507) => Ok(Elektor507Engine {
                desc,
                port: PORT_START,
                pll: Pll::SAFE,
                rx: Vec::new(),
            }),
            _ => Err(unsupported("not an Elektor SDR-USB description")),
        }
    }

    // [@ANCHOR: radioctl_elektor507_engine_samples]
    fn samples(&mut self, writes: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut samples = Samples {
            port: &mut self.port,
            out: Vec::new(),
        };
        for (register, values) in writes {
            samples.write(*register, values);
        }
        samples.out
    }

    // [@ANCHOR: radioctl_elektor507_engine_setup]
    fn setup(&mut self) -> Vec<Request> {
        self.port = PORT_START;
        self.pll = Pll::SAFE;
        let chip = self.samples(&[
            (REG_CLKOE, vec![0x20]),
            (REG_DIV1, vec![8]),
            (REG_CLKSRC, vec![0x02, 0x8E, 0x47]),
            (REG_DIV2, vec![0x88]),
        ]);
        vec![
            vendor(FTDI_RESET, 0),
            vendor(FTDI_BITMODE, BITMODE_VALUE),
            vendor(FTDI_BAUD, BAUD_VALUE),
            Request {
                bytes: bulk(chip),
                reply: ReplyKind::Ack,
            },
        ]
    }
}

/// The pin byte of a freshly opened board: both I2C lines high, multiplexer and attenuator at 0.
const PORT_START: u8 = PORT_IDLE;

impl ProtocolEngine for Elektor507Engine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_elektor507_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let data = match op {
            Op::SetFrequency(Vfo::A, Hz(hz)) => {
                if !(MIN_HZ..=MAX_HZ).contains(hz) {
                    return Err(unsupported("frequency outside 30 kHz to 29.999 MHz"));
                }
                let mux: u8 = if *hz <= 1_600_000 { 1 } else { 2 };
                self.port = (self.port & !MUX_MASK) | (mux << 2);
                self.pll = self.pll.solve(*hz);
                let writes = self.pll.registers();
                self.samples(&writes)
            }
            Op::SetLevel(Level::Attenuator, db) => {
                let code: u8 = match db {
                    0 => 0,
                    10 => 1,
                    20 => 2,
                    _ => return Err(unsupported("the attenuator is 0, 10 or 20 dB")),
                };
                self.port = (self.port & 0x1F) | (code << ATTENUATOR_SHIFT);
                vec![self.port]
            }
            Op::GetFrequency(_) | Op::GetLevel(_) => {
                return Err(unsupported(
                    "the board cannot be read back; the last value set is cached",
                ))
            }
            _ => return Err(unsupported("not implemented for the Elektor SDR-USB")),
        };
        self.rx.clear();
        Ok(Request {
            bytes: bulk(data),
            reply: ReplyKind::Ack,
        })
    }

    // [@ANCHOR: radioctl_elektor507_engine_feed]
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
            match reply.status {
                // Silence, as on a serial line: the actor ends the command in its own timeout.
                UsbStatus::Timeout | UsbStatus::NoDevice => {}
                UsbStatus::Ok => out.push(Inbound::Ack),
                UsbStatus::Stall | UsbStatus::Other => out.push(Inbound::Nak),
            }
        }
        out
    }

    // [@ANCHOR: radioctl_elektor507_engine_open_requests]
    fn open_requests(&mut self) -> Vec<Request> {
        self.setup()
    }

    // [@ANCHOR: radioctl_elektor507_engine_link_restored]
    fn link_restored(&mut self) -> Vec<Request> {
        self.setup()
    }
}

/// A virtual Elektor SDR-USB: a [`UsbDevice`] that decodes the pin samples written to the bulk OUT
/// endpoint into I2C transactions on a clock chip's register file.
#[derive(Debug)]
pub struct Elektor507Twin {
    regs: [u8; 256],
    bitbang: bool,
    port: u8,
    scl: bool,
    sda: bool,
    in_transaction: bool,
    bits: u8,
    current: u8,
    bytes: Vec<u8>,
    pll_written: bool,
}

impl Default for Elektor507Twin {
    fn default() -> Self {
        Elektor507Twin {
            regs: [0; 256],
            bitbang: false,
            port: PORT_START,
            scl: true,
            sda: true,
            in_transaction: false,
            bits: 0,
            current: 0,
            bytes: Vec::new(),
            pll_written: false,
        }
    }
}

impl Elektor507Twin {
    // [@ANCHOR: radioctl_elektor507_twin_new]
    /// A twin that has not been set up yet.
    pub fn new(desc: &RigDescription) -> Result<Elektor507Twin, RigError> {
        match desc.usb.as_ref().map(|u| u.protocol) {
            Some(UsbProtocol::Elektor507) => Ok(Elektor507Twin::default()),
            _ => Err(unsupported("not an Elektor SDR-USB description")),
        }
    }

    // [@ANCHOR: radioctl_elektor507_twin_register]
    /// The clock chip's register `index`.
    pub fn register(&self, index: u8) -> u8 {
        self.regs.get(usize::from(index)).copied().unwrap_or(0)
    }

    // [@ANCHOR: radioctl_elektor507_twin_frequency]
    /// The frequency the clock chip's registers give, once the pump has been written.
    pub fn frequency(&self) -> Option<Hz> {
        if !self.pll_written {
            return None;
        }
        let high = u32::from(self.register(REG_PUMP) & 0x03);
        let half = (high << 8) | u32::from(self.register(REG_PUMP + 1));
        let p = (half + 4) * 2 + u32::from(self.register(REG_PUMP + 2) >> 7);
        let q = u32::from(self.register(REG_PUMP + 2) & 0x7F) + 2;
        let div = match self.register(REG_CLKSRC + 2) & 0xC0 {
            0x80 => 2,
            0xC0 => 3,
            _ => u32::from(self.register(REG_DIV1) & 0x7F),
        };
        let pll = Pll {
            p,
            q,
            div: div.max(1),
        };
        Some(Hz(pll.frequency().round() as u64))
    }

    // [@ANCHOR: radioctl_elektor507_twin_attenuation]
    /// The attenuation the two pins select, in decibels.
    pub fn attenuation_db(&self) -> u16 {
        match (self.port >> ATTENUATOR_SHIFT) & 3 {
            1 => 10,
            2 => 20,
            _ => 0,
        }
    }

    // [@ANCHOR: radioctl_elektor507_twin_multiplexer]
    /// The input multiplexer pins (0 to 7).
    pub fn multiplexer(&self) -> u8 {
        (self.port & MUX_MASK) >> 2
    }

    // [@ANCHOR: radioctl_elektor507_twin_sample]
    fn sample(&mut self, byte: u8) {
        let scl = byte & PIN_SCL != 0;
        let sda = byte & PIN_SDA != 0;
        self.port = byte;
        if scl && self.scl && sda != self.sda {
            if self.sda {
                // Data line falls while the clock is high: a start.
                self.in_transaction = true;
                self.bits = 0;
                self.current = 0;
                self.bytes.clear();
            } else {
                // It rises: a stop.
                self.finish();
            }
        } else if scl && !self.scl && self.in_transaction {
            if self.bits < 8 {
                self.current = (self.current << 1) | u8::from(sda);
                self.bits += 1;
            } else {
                // The acknowledge slot ends the byte.
                self.bytes.push(self.current);
                self.bits = 0;
                self.current = 0;
            }
        }
        self.scl = scl;
        self.sda = sda;
    }

    // [@ANCHOR: radioctl_elektor507_twin_finish]
    fn finish(&mut self) {
        if self.in_transaction && self.bytes.first() == Some(&CHIP_ADDRESS) {
            if let Some((&register, values)) = self.bytes.get(1..).and_then(<[u8]>::split_first) {
                for (offset, value) in values.iter().enumerate() {
                    let at = usize::from(register) + offset;
                    if let Some(slot) = self.regs.get_mut(at) {
                        *slot = *value;
                    }
                    if (usize::from(REG_PUMP)..usize::from(REG_PUMP) + 3).contains(&at) {
                        self.pll_written = true;
                    }
                }
            }
        }
        self.in_transaction = false;
    }
}

impl UsbDevice for Elektor507Twin {
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

    // [@ANCHOR: radioctl_elektor507_twin_control_out]
    fn control_out(
        &mut self,
        request_type: u8,
        request: u8,
        value: u16,
        _index: u16,
        data: &[u8],
        _timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        if request_type != VENDOR_OUT || !data.is_empty() {
            return Err(UsbFailure::Stall);
        }
        match (request, value) {
            (FTDI_RESET, 0) => self.bitbang = false,
            (FTDI_BITMODE, BITMODE_VALUE) => self.bitbang = true,
            (FTDI_BAUD, BAUD_VALUE) => {}
            _ => return Err(UsbFailure::Stall),
        }
        Ok(0)
    }

    // [@ANCHOR: radioctl_elektor507_twin_bulk_write]
    fn bulk_write(
        &mut self,
        endpoint: u8,
        data: &[u8],
        _timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        if endpoint != BULK_OUT {
            return Err(UsbFailure::Stall);
        }
        for byte in data {
            self.sample(*byte);
        }
        Ok(data.len())
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

// SPDX-License-Identifier: AGPL-3.0-or-later
//! The wire protocol as numbers and pure functions. Sources are listed in README.md (provenance);
//! every value here is `verified=unverified` until a unit has been run.

use crate::Error;

/// USB vendor id of ELAD S.r.l. receivers' sample interface.
pub const VENDOR_ID: u16 = 0x1721;
/// Bulk IN endpoint carrying I/Q.
pub const EP_SAMPLES: u8 = 0x86;
/// One bulk transfer: 512 x 24 bytes, a whole number of samples at every rate.
pub const BULK_BYTES: usize = 512 * 24;

/// Request: firmware version (2 bytes).
pub const REQ_FW_VERSION: u8 = 0xFF;
/// Request: EEPROM read (value = address, index = [`EEPROM_INDEX`]).
pub const REQ_EEPROM: u8 = 0xA2;
/// Index of an EEPROM read.
pub const EEPROM_INDEX: u16 = 0x0151;
/// Request: sampler (S1/S2) tune word.
pub const REQ_TUNE: u8 = 0xF2;
/// Request: sampler front end (filter index 2, attenuator index 3).
pub const REQ_FRONT_END: u8 = 0xF7;
/// Request: sampler FIFO start/stop (value 1 / 0).
pub const REQ_FIFO: u8 = 0xE9;
/// Request: the DUO gateway; the real sub-command is the high byte of the index.
pub const REQ_DUO_GATEWAY: u8 = 0xE1;
/// DUO sub-command: FIFO init.
pub const DUO_FIFO_INIT: u8 = 0xE8;
/// DUO sub-command: FIFO run (value 1) or stop (value 0).
pub const DUO_FIFO_RUN: u8 = 0xE9;
/// DUO sub-command: tune.
pub const DUO_TUNE: u8 = 0xF2;
/// DUO sub-command: tune read-back (11 bytes).
pub const DUO_TUNE_READ: u8 = 0xF5;
/// DUO sub-command: status (3 bytes).
pub const DUO_STATUS: u8 = 0xFC;

/// The supported units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    /// FDM-DUO (also -DUOr): transceiver; only its receive interface is used here.
    Duo,
    /// FDM-S2.
    S2,
    /// FDM-S1.
    S1,
}

impl Model {
    /// The model for a USB product id.
    // [@ANCHOR: elad_model_from_pid]
    pub fn from_pid(pid: u16) -> Result<Model, Error> {
        match pid {
            0x061a => Ok(Model::Duo),
            0x061c => Ok(Model::S2),
            0x0610 => Ok(Model::S1),
            other => Err(Error::UnknownDevice { pid: other }),
        }
    }

    /// The model's USB product id.
    // [@ANCHOR: elad_model_pid]
    pub fn pid(self) -> u16 {
        match self {
            Model::Duo => 0x061a,
            Model::S2 => 0x061c,
            Model::S1 => 0x0610,
        }
    }

    /// Display name.
    // [@ANCHOR: elad_model_name]
    pub fn name(self) -> &'static str {
        match self {
            Model::Duo => "FDM-DUO",
            Model::S2 => "FDM-S2",
            Model::S1 => "FDM-S1",
        }
    }

    /// ADC clock in Hz.
    // [@ANCHOR: elad_model_adc_clock_hz]
    pub fn adc_clock_hz(self) -> f64 {
        match self {
            Model::S1 => 61_440_000.0,
            _ => 122_880_000.0,
        }
    }

    /// Whether a vendor FPGA image must be loaded before samples flow.
    pub fn needs_fpga_image(self) -> bool {
        !matches!(self, Model::Duo)
    }

    /// Receive range in Hz, lowest to highest.
    // [@ANCHOR: elad_model_rx_range_hz]
    pub fn rx_range_hz(self) -> (u64, u64) {
        match self {
            Model::S1 => (10_000, 30_000_000),
            _ => (10_000, 54_000_000),
        }
    }

    /// Whether the front end has a switchable filter bank and attenuator (index 2 and 3 of the front-end request).
    pub fn has_front_end_switches(self) -> bool {
        matches!(self, Model::S2)
    }

    /// I/Q sample rates in Hz the model streams. The DUO's rate is set by the unit, not by software.
    // [@ANCHOR: elad_model_sample_rates_hz]
    pub fn sample_rates_hz(self) -> &'static [u32] {
        match self {
            Model::S2 => &[192_000, 384_000, 768_000, 1_536_000, 3_072_000, 6_144_000],
            Model::S1 => &[192_000, 384_000, 768_000, 1_536_000, 3_072_000],
            Model::Duo => &[192_000, 384_000, 768_000, 1_536_000, 3_072_000],
        }
    }
}

/// The 32-bit DDC tuning word for `hz` at ADC clock `clock_hz`: the fraction of the clock, times 2^32.
/// Negative frequencies clamp to 0; a frequency above the clock aliases (the modulo).
// [@ANCHOR: elad_tuning_word]
pub fn tuning_word(hz: f64, clock_hz: f64) -> u32 {
    if !clock_hz.is_finite() || clock_hz <= 0.0 || !hz.is_finite() || hz < 0.0 {
        return 0;
    }
    let frac = (hz % clock_hz) / clock_hz;
    ((frac * 4_294_967_296.0) as u64 & 0xFFFF_FFFF) as u32
}

/// A control request ready for the transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// `bRequest`.
    pub request: u8,
    /// `wValue`.
    pub value: u16,
    /// `wIndex`.
    pub index: u16,
    /// Data stage bytes (OUT), empty when none.
    pub data: Vec<u8>,
}

/// The sampler (S1/S2) tune request for a tuning word.
// [@ANCHOR: elad_tune_request_sampler]
pub fn tune_request_sampler(word: u32) -> Request {
    Request {
        request: REQ_TUNE,
        value: (word & 0xFFFF) as u16,
        index: (word >> 16) as u16,
        data: vec![(word >> 24) as u8, 0],
    }
}

/// The DUO tune request for a tuning word (through the gateway).
// [@ANCHOR: elad_tune_request_duo]
pub fn tune_request_duo(word: u32) -> Request {
    Request {
        request: REQ_DUO_GATEWAY,
        value: (word & 0xFFFF) as u16,
        index: ((DUO_TUNE as u16) << 8) | ((word >> 16) & 0xFF) as u16,
        data: vec![(word >> 24) as u8, 0],
    }
}

/// A DUO gateway request with sub-command `sub` and `value`.
pub fn duo_request(sub: u8, value: u16) -> Request {
    Request {
        request: REQ_DUO_GATEWAY,
        value,
        index: (sub as u16) << 8,
        data: Vec::new(),
    }
}

/// The S2 front-end code: filters bypassed `0x01`; in, below 61.44 MHz `0x03`; 61.44 to 122.88 MHz `0x02`; above `0x04`;
/// the attenuator ORs `0x08`.
// [@ANCHOR: elad_front_end_code]
pub fn front_end_code(filters: bool, hz: f64, attenuator: bool) -> u8 {
    let base = if !filters {
        0x01
    } else if hz < 61_440_000.0 {
        0x03
    } else if hz < 122_880_000.0 {
        0x02
    } else {
        0x04
    };
    if attenuator {
        base | 0x08
    } else {
        base
    }
}

/// True for the requests this crate may send: nothing that keys, writes CAT or writes the EEPROM.
// [@ANCHOR: elad_is_receive_request]
pub fn is_receive_request(r: &Request) -> bool {
    match r.request {
        REQ_FW_VERSION | REQ_EEPROM | REQ_TUNE | REQ_FRONT_END | REQ_FIFO => true,
        REQ_DUO_GATEWAY => matches!(
            (r.index >> 8) as u8,
            DUO_FIFO_INIT | DUO_FIFO_RUN | DUO_TUNE | DUO_TUNE_READ | DUO_STATUS
        ),
        _ => false,
    }
}

/// True for the requests that are control IN reads on this unit: firmware version and EEPROM. Request 0xA2 is the Cypress EEPROM
/// request whose direction picks read or write, so it must only ever go out as an IN.
// [@ANCHOR: elad_is_read_request]
pub fn is_read_request(request: u8) -> bool {
    matches!(request, REQ_FW_VERSION | REQ_EEPROM)
}

/// True for the requests this crate may send as a control OUT: the receive-only list minus the IN-only reads (firmware version,
/// EEPROM, and the DUO tune-read and status sub-commands), so an EEPROM write cannot pass.
// [@ANCHOR: elad_is_send_request]
pub fn is_send_request(r: &Request) -> bool {
    if !is_receive_request(r) || is_read_request(r.request) {
        return false;
    }
    !(r.request == REQ_DUO_GATEWAY && matches!((r.index >> 8) as u8, DUO_TUNE_READ | DUO_STATUS))
}

/// Calibration read from the unit's EEPROM (dB offsets and a rate correction); unprogrammed (all-ones) values are absent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Calibration {
    /// Serial number text.
    pub serial: Option<String>,
    /// Hardware version (major, minor).
    pub hardware: Option<(u8, u8)>,
    /// Sample-rate correction in Hz.
    pub rate_correction_hz: Option<i32>,
    /// Global level offset in dB.
    pub global_db: Option<f32>,
    /// Low-pass filter level offset in dB.
    pub lp_db: Option<f32>,
    /// Attenuator level offset in dB.
    pub att_db: Option<f32>,
}

/// EEPROM addresses and lengths: serial, hardware version, rate correction, global, LP and ATT offsets.
pub const EEPROM_MAP: [(u16, usize); 6] = [
    (0x4000, 32),
    (0x404C, 2),
    (0x4024, 4),
    (0x4028, 4),
    (0x402C, 4),
    (0x4030, 4),
];

fn finite_f32(b: &[u8]) -> Option<f32> {
    let v = f32::from_le_bytes([*b.first()?, *b.get(1)?, *b.get(2)?, *b.get(3)?]);
    v.is_finite().then_some(v)
}

impl Calibration {
    /// Builds the calibration from the six EEPROM reads in [`EEPROM_MAP`] order. Short or all-ones fields are absent.
    // [@ANCHOR: elad_calibration_from_eeprom]
    pub fn from_eeprom(fields: &[Vec<u8>; 6]) -> Calibration {
        let serial = {
            let raw = &fields[0];
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            let s: String = raw[..end]
                .iter()
                .filter(|b| b.is_ascii_graphic() || **b == b' ')
                .map(|&b| b as char)
                .collect();
            let t = s.trim().to_string();
            (!t.is_empty()).then_some(t)
        };
        let hardware = (fields[1].len() >= 2 && fields[1][..2] != [0xFF, 0xFF])
            .then(|| (fields[1][0], fields[1][1]));
        let rate_correction_hz = (fields[2].len() >= 4 && fields[2][..4] != [0xFF; 4])
            .then(|| i32::from_le_bytes([fields[2][0], fields[2][1], fields[2][2], fields[2][3]]));
        Calibration {
            serial,
            hardware,
            rate_correction_hz,
            global_db: finite_f32(&fields[3]),
            lp_db: finite_f32(&fields[4]),
            att_db: finite_f32(&fields[5]),
        }
    }

    /// Level offset in dB to apply to normalised full-scale samples for the model, rate and front-end state.
    // [@ANCHOR: elad_level_offset_db]
    pub fn level_offset_db(
        &self,
        model: Model,
        rate_hz: u32,
        low_pass: bool,
        attenuator: bool,
    ) -> f64 {
        let rescale = match rate_hz {
            384_000 | 768_000 => 6.0,
            1_536_000 | 3_072_000 => 5.4,
            6_144_000 => -0.7,
            _ => 0.0,
        };
        (if model == Model::Duo { 21.4 } else { 0.0 })
            + rescale
            + f64::from(self.global_db.unwrap_or(0.0))
            + if low_pass {
                f64::from(self.lp_db.unwrap_or(0.0))
            } else {
                0.0
            }
            + if attenuator {
                f64::from(self.att_db.unwrap_or(0.0))
            } else {
                0.0
            }
    }
}

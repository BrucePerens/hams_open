// SPDX-License-Identifier: AGPL-3.0-or-later
//! The receiver as the relay and the bench tools use it: open, tune, front end, start, read I/Q, stop.

use crate::convert::{Decoder, Format};
use crate::fpga::FpgaState;
use crate::protocol::{self, Calibration, Model, Request};
use crate::transport::Transport;
use crate::Error;
use std::time::Duration;

/// Options for [`Radio::open`].
#[derive(Clone, Copy, Debug)]
pub struct OpenOptions {
    /// What the caller says about the FPGA image.
    pub fpga: FpgaState,
    /// The sample rate the unit streams at (for an FDM-DUO: the rate it is set to, which software cannot change).
    pub sample_rate_hz: u32,
}

/// Read counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Complex samples decoded.
    pub samples: u64,
    /// Bytes read from the bulk endpoint.
    pub bytes: u64,
    /// Bulk reads that returned data.
    pub reads: u64,
    /// Bulk reads that timed out.
    pub timeouts: u64,
    /// Bulk reads that returned no bytes.
    pub empty_reads: u64,
}

/// What an open unit reports about itself.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceInfo {
    /// The model.
    pub model: Model,
    /// Firmware version, two bytes as the unit sends them.
    pub firmware: Option<[u8; 2]>,
    /// EEPROM calibration.
    pub calibration: Calibration,
}

/// An open receiver.
pub struct Radio<T: Transport> {
    t: T,
    info: DeviceInfo,
    rate_hz: u32,
    decoder: Decoder,
    stats: Stats,
    tuned_hz: Option<f64>,
    filters: bool,
    attenuator: bool,
    streaming: bool,
    fpga: FpgaState,
    bulk: Vec<u8>,
}

impl<T: Transport> Radio<T> {
    /// Opens a unit: checks the rate, reads the firmware version and the EEPROM calibration (read-only, so it works on an FDM-S1/S2
    /// with no FPGA image). Nothing here starts the stream; [`Radio::start`] refuses an FDM-S1/S2 whose FPGA the caller did not declare loaded.
    // [@ANCHOR: elad_radio_open]
    pub fn open(mut t: T, model: Model, opts: OpenOptions) -> Result<Radio<T>, Error> {
        if !model.sample_rates_hz().contains(&opts.sample_rate_hz) {
            return Err(Error::OutOfRange(format!(
                "{} does not stream {} Hz",
                model.name(),
                opts.sample_rate_hz
            )));
        }
        let mut fw = [0u8; 2];
        let n = read_request(&mut t, protocol::REQ_FW_VERSION, 0, 0, &mut fw)?;
        let firmware = (n == 2).then_some(fw);
        let mut fields: [Vec<u8>; 6] = Default::default();
        for (i, (addr, len)) in protocol::EEPROM_MAP.iter().enumerate() {
            let mut buf = vec![0u8; *len];
            let n = read_request(
                &mut t,
                protocol::REQ_EEPROM,
                *addr,
                protocol::EEPROM_INDEX,
                &mut buf,
            )?;
            buf.truncate(n);
            fields[i] = buf;
        }
        let calibration = Calibration::from_eeprom(&fields);
        Ok(Radio {
            t,
            info: DeviceInfo {
                model,
                firmware,
                calibration,
            },
            rate_hz: opts.sample_rate_hz,
            decoder: Decoder::new(Format::for_rate(opts.sample_rate_hz)),
            stats: Stats::default(),
            tuned_hz: None,
            filters: true,
            attenuator: false,
            streaming: false,
            fpga: opts.fpga,
            bulk: vec![0u8; protocol::BULK_BYTES * 4],
        })
    }

    // [@ANCHOR: elad_radio_send]
    pub(crate) fn send(&mut self, r: &Request) -> Result<(), Error> {
        if !protocol::is_send_request(r) {
            return Err(Error::NotReceiveOnly(r.request));
        }
        self.t.control_out(r.request, r.value, r.index, &r.data)?;
        Ok(())
    }

    /// What the unit reported at open.
    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    /// The sample rate.
    pub fn sample_rate_hz(&self) -> u32 {
        self.rate_hz
    }

    /// The centre frequency last set.
    pub fn tuned_hz(&self) -> Option<f64> {
        self.tuned_hz
    }

    /// The read counters.
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Level offset in dB for the current state; add it to 20 log10 of a normalised sample level to estimate dBm at the input.
    pub fn level_offset_db(&self) -> f64 {
        self.info.calibration.level_offset_db(
            self.info.model,
            self.rate_hz,
            self.filters,
            self.attenuator,
        )
    }

    /// Sets the centre frequency (and, on an FDM-S2, the filter code for that frequency).
    // [@ANCHOR: elad_radio_tune]
    pub fn tune(&mut self, hz: f64) -> Result<(), Error> {
        let (lo, hi) = self.info.model.rx_range_hz();
        if !hz.is_finite() || hz < lo as f64 || hz > hi as f64 {
            return Err(Error::OutOfRange(format!(
                "{hz} Hz is outside {lo} to {hi} Hz for the {}",
                self.info.model.name()
            )));
        }
        let word = protocol::tuning_word(hz, self.info.model.adc_clock_hz());
        let req = if self.info.model == Model::Duo {
            protocol::tune_request_duo(word)
        } else {
            protocol::tune_request_sampler(word)
        };
        self.send(&req)?;
        self.tuned_hz = Some(hz);
        self.write_front_end(self.filters, self.attenuator)
    }

    // [@ANCHOR: elad_radio_write_front_end]
    fn write_front_end(&mut self, filters: bool, attenuator: bool) -> Result<(), Error> {
        if !self.info.model.has_front_end_switches() {
            return Ok(());
        }
        let hz = self.tuned_hz.unwrap_or(0.0);
        let code = protocol::front_end_code(filters, hz, attenuator);
        self.send(&Request {
            request: protocol::REQ_FRONT_END,
            value: u16::from(code),
            index: 2,
            data: Vec::new(),
        })
    }

    // [@ANCHOR: elad_radio_require_switches]
    fn require_switches(&self, what: &str) -> Result<(), Error> {
        if self.info.model.has_front_end_switches() {
            Ok(())
        } else {
            Err(Error::OutOfRange(format!(
                "the {} has no switchable {what} here",
                self.info.model.name()
            )))
        }
    }

    /// Sets the S2's band filters and attenuator together. Before the first [`Radio::tune`] nothing is sent (the code depends on the
    /// band): the state is recorded and [`Radio::tune`] writes it, so the receiver is never tuned at a gain the caller did not ask for.
    /// The state is recorded only if the write succeeded.
    // [@ANCHOR: elad_radio_set_front_end]
    pub fn set_front_end(&mut self, filters: bool, attenuator: bool) -> Result<(), Error> {
        self.require_switches("filters or attenuator")?;
        if self.tuned_hz.is_some() {
            self.write_front_end(filters, attenuator)?;
        }
        self.filters = filters;
        self.attenuator = attenuator;
        Ok(())
    }

    /// Switches the S2's band filters in or out.
    // [@ANCHOR: elad_radio_set_filters]
    pub fn set_filters(&mut self, on: bool) -> Result<(), Error> {
        self.require_switches("filters")?;
        self.set_front_end(on, self.attenuator)
    }

    /// Switches the S2's attenuator.
    // [@ANCHOR: elad_radio_set_attenuator]
    pub fn set_attenuator(&mut self, on: bool) -> Result<(), Error> {
        self.require_switches("attenuator")?;
        self.set_front_end(self.filters, on)
    }

    /// Starts the sample FIFO.
    // [@ANCHOR: elad_radio_start]
    pub fn start(&mut self) -> Result<(), Error> {
        if !self.fpga.ready_for(self.info.model) {
            return Err(Error::FpgaImageRequired);
        }
        // Marked streaming first: a start request the unit accepted but reported as failed must still be followed by a stop.
        self.streaming = true;
        if self.info.model == Model::Duo {
            self.send(&protocol::duo_request(protocol::DUO_FIFO_INIT, 0))?;
            self.send(&protocol::duo_request(protocol::DUO_FIFO_RUN, 1))?;
        } else {
            self.send(&Request {
                request: protocol::REQ_FIFO,
                value: 1,
                index: 0,
                data: Vec::new(),
            })?;
        }
        self.decoder.reset();
        Ok(())
    }

    /// Stops the sample FIFO.
    // [@ANCHOR: elad_radio_stop]
    pub fn stop(&mut self) -> Result<(), Error> {
        if !self.streaming {
            return Ok(());
        }
        self.streaming = false;
        if self.info.model == Model::Duo {
            self.send(&protocol::duo_request(protocol::DUO_FIFO_RUN, 0))
        } else {
            self.send(&Request {
                request: protocol::REQ_FIFO,
                value: 0,
                index: 0,
                data: Vec::new(),
            })
        }
    }

    /// Reads one bulk transfer and appends its I/Q pairs to `out` (interleaved I, Q floats). Returns the pairs appended.
    /// Three timeouts or empty reads in a row before any sample arrived give [`Error::NoSamples`].
    // [@ANCHOR: elad_radio_read]
    pub fn read(&mut self, out: &mut Vec<f32>, timeout: Duration) -> Result<usize, Error> {
        match self.t.bulk_in(&mut self.bulk, timeout) {
            Ok(0) => {
                self.stats.empty_reads += 1;
                self.silence()
            }
            Ok(n) => {
                self.stats.reads += 1;
                self.stats.bytes += n as u64;
                let pairs = self.decoder.push(&self.bulk[..n], out);
                self.stats.samples += pairs as u64;
                Ok(pairs)
            }
            Err(Error::Timeout) => {
                self.stats.timeouts += 1;
                self.silence()
            }
            Err(e) => Err(e),
        }
    }

    // [@ANCHOR: elad_radio_silence]
    fn silence(&mut self) -> Result<usize, Error> {
        if self.stats.samples == 0 && self.stats.timeouts + self.stats.empty_reads >= 3 {
            Err(Error::NoSamples)
        } else {
            Err(Error::Timeout)
        }
    }
}

/// A control IN, refused unless the request is one of the read-only ones.
// [@ANCHOR: elad_radio_read_request]
fn read_request<T: Transport>(
    t: &mut T,
    request: u8,
    value: u16,
    index: u16,
    buf: &mut [u8],
) -> Result<usize, Error> {
    if !protocol::is_read_request(request) {
        return Err(Error::NotReceiveOnly(request));
    }
    t.control_in(request, value, index, buf)
}

impl<T: Transport> Drop for Radio<T> {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

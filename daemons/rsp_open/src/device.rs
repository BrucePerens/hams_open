// SPDX-License-Identifier: AGPL-3.0-or-later
//! The RSP2pro driver: control sequences and streaming on top of a [`Transport`].

use crate::encode::{self, *};
use crate::stream::{BlockParser, IqI16, StreamStats};
use crate::transport::{PacketSink, Transport};
use crate::usb::{self, UsbTransport};
use crate::{check_state, Error, PID_RSP2};
use std::sync::mpsc::{sync_channel, Receiver, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Vendor requests used (protocol document 1.4). Nothing else is ever sent.
const REQ_WREG: u8 = 0x41;
const REQ_START: u8 = 0x43;
const REQ_STOP: u8 = 0x45;
const REQ_READ48: u8 = 0x48;
const REQ_BOARD: u8 = 0x49;

// MSi2500 register values from the open sequence (1.5, 1.7).
const REG08_OPEN: u32 = 0x00ee80;
const REG05_OPEN: u32 = 0x00000c;
const REG05_CLOSE: u32 = 0x000004;
const REG00_OPEN: u32 = 0x000200;
const REG02_OPEN: u32 = 0x004801;

/// Current receiver settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub rf_hz: u64,
    pub antenna: Antenna,
    pub if_mode: IfMode,
    pub bandwidth: Bandwidth,
    pub adc_hz: u32,
    pub ppb: i64,
    pub if_gr_db: u8,
    pub lna_state: u8,
    pub notch: bool,
    pub bias_t: bool,
    pub ext_ref_out: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            rf_hz: 100_000_000,
            antenna: Antenna::A,
            if_mode: IfMode::Zero,
            bandwidth: Bandwidth::K1536,
            adc_hz: 2_000_000,
            ppb: 0,
            if_gr_db: 40,
            lna_state: 0,
            notch: false,
            bias_t: false,
            ext_ref_out: false,
        }
    }
}

type UserCb = Box<dyn FnMut(&[IqI16], &StreamStats) + Send + 'static>;

struct Core {
    parser: BlockParser,
    cb: UserCb,
    buf: Vec<IqI16>,
}

/// Blocking iterator over sample chunks (one chunk per isochronous packet).
pub struct SampleIter {
    rx: Receiver<Vec<IqI16>>,
    dropped: Arc<Mutex<u64>>,
    timeout: Duration,
}

impl SampleIter {
    /// Chunks dropped because the consumer was too slow.
    pub fn dropped(&self) -> u64 {
        *self.dropped.lock().unwrap()
    }
}

impl Iterator for SampleIter {
    type Item = Vec<IqI16>;
    fn next(&mut self) -> Option<Vec<IqI16>> {
        self.rx.recv_timeout(self.timeout).ok()
    }
}

/// An open RSP2pro.
pub struct Rsp2<T: Transport = UsbTransport> {
    t: T,
    s: Settings,
    band: Band,
    cfg: StreamConfig,
    core: Option<Arc<Mutex<Core>>>,
    streaming: bool,
    closed: bool,
    settle: Duration,
    /// Value returned by request 0x48 at open (0x04 on every observed open).
    pub read48: u8,
    /// Serial number string, if read.
    pub serial: Option<String>,
}

impl Rsp2<UsbTransport> {
    /// Open the first RSP2/RSP2pro with default settings.
    pub fn open() -> Result<Self, Error> {
        Self::open_with(Settings::default())
    }

    /// Open the first RSP2/RSP2pro. The device state is checked from its descriptor before
    /// the device is opened; a boot-ROM device gives [`Error::NotRunning`].
    pub fn open_with(settings: Settings) -> Result<Self, Error> {
        // [@ANCHOR: rsp_device_rsp2_open_with]
        let devs = usb::list_devices()?;
        let Some(info) = devs.iter().find(|d| d.pid == PID_RSP2) else {
            if let Some(d) = devs.first() {
                return Err(Error::UnsupportedModel {
                    pid: d.pid,
                    name: d.model.name(),
                });
            }
            return Err(Error::NotFound);
        };
        check_state(info.pid, info.bcd_device)?;
        let t = UsbTransport::open(PID_RSP2, Some((info.bus, info.address)))?;
        // Re-check on the opened handle's descriptor snapshot (the device may have re-enumerated).
        check_state(t.info.pid, t.info.bcd_device)?;
        let serial = t.serial();
        let mut rx = Rsp2::with_transport(t, settings, Duration::from_millis(300))?;
        rx.serial = serial;
        Ok(rx)
    }
}

impl<T: Transport> Rsp2<T> {
    /// Drive an already-opened transport whose device state was checked by the caller.
    /// Sends the open sequence (1.5) with `settings`.
    pub fn with_transport(t: T, settings: Settings, settle: Duration) -> Result<Self, Error> {
        // [@ANCHOR: rsp_device_rsp2_with_transport]
        if settings.bias_t {
            return Err(Error::InvalidArgument(
                "open with bias-T off; enable it explicitly later".into(),
            ));
        }
        let band = band_for(settings.rf_hz)?;
        check_antenna(settings.antenna, settings.rf_hz)?;
        let cfg = stream_config(settings.adc_hz, settings.if_mode)?;
        let mut s = settings;
        let n = lna_table(gain_table(&band, s.antenna)).len() as u8;
        if s.lna_state >= n {
            return Err(Error::InvalidArgument(format!(
                "LNA state {} >= {n} for this band",
                s.lna_state
            )));
        }
        if s.if_gr_db > IF_GR_MAX {
            return Err(Error::InvalidArgument(format!(
                "IF gain reduction {} > {IF_GR_MAX}",
                s.if_gr_db
            )));
        }
        s.ppb = s.ppb.clamp(-1_000_000, 1_000_000);
        let mut rx = Rsp2 {
            t,
            s,
            band,
            cfg,
            core: None,
            streaming: false,
            closed: false,
            settle,
            read48: 0,
            serial: None,
        };
        rx.init()?;
        Ok(rx)
    }

    pub fn settings(&self) -> &Settings {
        &self.s
    }
    pub fn stream_config(&self) -> StreamConfig {
        self.cfg
    }
    /// Output sample rate in samples/s (ADC clock, halved in the low-IF modes).
    pub fn output_rate(&self) -> f64 {
        // [@ANCHOR: rsp_device_rsp2_output_rate]
        let f = self.s.adc_hz as f64;
        if self.cfg.decim2 {
            f / 2.0
        } else {
            f
        }
    }
    pub fn transport(&self) -> &T {
        &self.t
    }

    fn reg(&mut self, reg: u8, value: u32) -> Result<(), Error> {
        let (v, i) = reg_write_args(reg, value);
        self.t.vendor_out(REQ_WREG, v, i)
    }
    fn tuner(&mut self, word: u32) -> Result<(), Error> {
        self.reg(REG_TUNER, word)
    }
    fn board(&mut self, addr: u8, byte: u8) -> Result<(), Error> {
        let (v, i) = board_write_args(addr, byte);
        self.t.vendor_out(REQ_BOARD, v, i)
    }

    fn table(&self) -> &'static [FrontEndStep] {
        lna_table(gain_table(&self.band, self.s.antenna))
    }
    fn step(&self) -> FrontEndStep {
        self.table()[self.s.lna_state as usize]
    }
    fn front_byte(&self) -> u8 {
        board_front_byte(self.step().board_step, self.s.notch, self.s.bias_t)
    }
    fn band_byte(&self) -> u8 {
        board_band_byte(self.band.filter, self.s.antenna, self.s.ext_ref_out)
    }
    fn r0_word(&self) -> u32 {
        r0(
            mode_byte(self.band.mode, self.s.antenna),
            self.s.if_mode,
            self.s.bandwidth,
        )
    }
    fn pll(&self) -> Result<Pll, Error> {
        encode::pll(
            lo_hz(&self.band, self.s.rf_hz),
            self.band.mode.lo_div(),
            self.s.ppb,
        )
    }
    fn gain_word(&self) -> u32 {
        r1(self.s.if_gr_db, &self.step())
    }

    fn init(&mut self) -> Result<(), Error> {
        // [@ANCHOR: rsp_device_rsp2_init]
        self.reg(0x08, REG08_OPEN)?;
        self.t.set_alt(self.cfg.alt)?;
        let mut b = [0u8; 1];
        self.t.vendor_in(REQ_READ48, 0, 0, &mut b)?;
        self.t.vendor_in(REQ_READ48, 0, 0, &mut b)?;
        self.read48 = b[0];
        self.reg(0x05, REG05_OPEN)?;
        self.tuner(R14_TUNE)?;
        self.reg(0x00, REG00_OPEN)?;
        self.reg(0x02, REG02_OPEN)?;
        self.tuner(R5_THRESH)?;
        self.reg(0x07, self.cfg.reg07)?;
        self.tuner(R6)?;
        self.full_tune()?;
        let clk = adc_clock(self.s.adc_hz, self.s.ppb, true)?;
        self.reg(0x04, clk.reg04)?;
        self.reg(0x03, clk.reg03)?;
        Ok(())
    }

    /// Full tune sequence used at open and on every band, antenna-port or gain-table change.
    fn full_tune(&mut self) -> Result<(), Error> {
        // [@ANCHOR: rsp_device_rsp2_full_tune]
        let p = self.pll()?;
        self.tuner(self.r0_word())?;
        self.board(MCP_GPIOA, self.band_byte())?;
        self.board(MCP_GPIOB, board_front_transient(self.s.bias_t))?;
        self.board(MCP_IODIRA, 0)?;
        self.board(MCP_IODIRB, 0)?;
        self.tuner(p.r3)?;
        self.tuner(p.r2)?;
        self.tuner(R13_TUNE)?;
        self.tuner(R14_TUNE)?;
        self.tuner(self.gain_word())?;
        self.board(MCP_GPIOB, self.front_byte())?;
        Ok(())
    }

    /// Tune to `rf_hz` (1 kHz to 2 GHz, except 375-420 MHz). Returns the LO actually set, Hz.
    /// If the new band has fewer LNA states than the current one, the LNA state is clamped to
    /// the band's highest state (see [`Settings::lna_state`] afterwards).
    pub fn set_frequency(&mut self, rf_hz: u64) -> Result<f64, Error> {
        // [@ANCHOR: rsp_device_rsp2_set_frequency]
        let band = band_for(rf_hz)?;
        check_antenna(self.s.antenna, rf_hz)?;
        let old_band = self.band;
        let old_rf = self.s.rf_hz;
        let old_lna = self.s.lna_state;
        self.band = band;
        self.s.rf_hz = rf_hz;
        let n = self.table().len() as u8;
        if self.s.lna_state >= n {
            self.s.lna_state = n - 1;
        }
        let res = (|| {
            let p = self.pll()?;
            if band != old_band || self.s.lna_state != old_lna {
                self.full_tune()?;
            } else {
                self.tuner(p.r3)?;
                self.tuner(p.r2)?;
            }
            Ok(p.lo_hz)
        })();
        if res.is_err() {
            self.band = old_band;
            self.s.rf_hz = old_rf;
            self.s.lna_state = old_lna;
        }
        let lo = res?;
        // Express the result as the RF that the LO corresponds to.
        Ok(match band.lo_plan {
            LoPlan::Direct => lo,
            LoPlan::UpBy120M => lo - 120e6,
            LoPlan::DownBy120M => lo + 120e6,
        })
    }

    /// Set IF gain reduction (0..=59 dB) and LNA state (0..number of states in the band).
    pub fn set_gain(&mut self, if_gr_db: u8, lna_state: u8) -> Result<(), Error> {
        // [@ANCHOR: rsp_device_rsp2_set_gain]
        if if_gr_db > IF_GR_MAX {
            return Err(Error::InvalidArgument(format!(
                "IF gain reduction {if_gr_db} > {IF_GR_MAX}"
            )));
        }
        let n = self.table().len() as u8;
        if lna_state >= n {
            return Err(Error::InvalidArgument(format!(
                "LNA state {lna_state} >= {n} for this band"
            )));
        }
        self.s.if_gr_db = if_gr_db;
        self.s.lna_state = lna_state;
        self.tuner(self.gain_word())?;
        self.board(MCP_GPIOB, self.front_byte())
    }

    /// Number of LNA states in the current band and port.
    pub fn lna_states(&self) -> u8 {
        self.table().len() as u8
    }

    /// Select Antenna A, B or the Hi-Z port (Hi-Z only below 60 MHz).
    pub fn select_antenna(&mut self, ant: Antenna) -> Result<(), Error> {
        // [@ANCHOR: rsp_device_rsp2_select_antenna]
        check_antenna(ant, self.s.rf_hz)?;
        let old = self.s.antenna;
        if old == ant {
            return Ok(());
        }
        self.s.antenna = ant;
        let hiz_change = (old == Antenna::HiZ) != (ant == Antenna::HiZ);
        if hiz_change {
            let n = self.table().len() as u8;
            if self.s.lna_state >= n {
                self.s.lna_state = n - 1;
            }
            self.full_tune()
        } else {
            self.board(MCP_GPIOA, self.band_byte())
        }
    }

    /// Broadcast (MW/FM) notch.
    pub fn set_notch(&mut self, on: bool) -> Result<(), Error> {
        self.s.notch = on;
        self.board(MCP_GPIOB, self.front_byte())
    }

    /// Bias-T (DC power on the antenna port). Off unless explicitly enabled.
    pub fn set_bias_t(&mut self, on: bool) -> Result<(), Error> {
        self.s.bias_t = on;
        self.board(MCP_GPIOB, self.front_byte())
    }

    /// 24 MHz reference clock output.
    pub fn set_ext_ref_out(&mut self, on: bool) -> Result<(), Error> {
        self.s.ext_ref_out = on;
        self.board(MCP_GPIOA, self.band_byte())
    }

    /// Tuner IF bandwidth.
    pub fn set_bandwidth(&mut self, bw: Bandwidth) -> Result<(), Error> {
        self.s.bandwidth = bw;
        self.tuner(self.r0_word())
    }

    /// Reference correction in ppm (positive when the reference runs fast); applied to the
    /// ADC clock and to the tuner PLL.
    pub fn set_ppm(&mut self, ppm: f64) -> Result<(), Error> {
        // [@ANCHOR: rsp_device_rsp2_set_ppm]
        let ppb = ppm_to_ppb(ppm)?;
        let old = self.s.ppb;
        self.s.ppb = ppb;
        let r = (|| {
            let p = self.pll()?;
            let clk = adc_clock(self.s.adc_hz, ppb, true)?;
            self.tuner(p.r3)?;
            self.tuner(p.r2)?;
            self.reg(0x04, clk.reg04)?;
            self.reg(0x03, clk.reg03)
        })();
        if r.is_err() {
            self.s.ppb = old;
        }
        r
    }

    /// Zero-IF sampling at `adc_hz` (2.0, 2.048 or 3..=10 MHz).
    pub fn set_sample_rate(&mut self, adc_hz: u32) -> Result<(), Error> {
        self.set_sampling(adc_hz, IfMode::Zero)
    }

    /// Low-IF mode with its ADC clock (450 kHz/2 MHz, 1.62 MHz/6 MHz, 2.048 MHz/8.192 MHz);
    /// the device then decimates by 2 and the host must do the IF down-conversion (the output
    /// frequency mapping is not yet characterised; see README). `IfMode::Zero` returns to zero
    /// IF at the current ADC clock.
    pub fn set_if_mode(&mut self, mode: IfMode) -> Result<(), Error> {
        let adc = mode.required_adc_hz().unwrap_or(self.s.adc_hz);
        self.set_sampling(adc, mode)
    }

    fn set_sampling(&mut self, adc_hz: u32, if_mode: IfMode) -> Result<(), Error> {
        // [@ANCHOR: rsp_device_rsp2_set_sampling]
        let cfg = stream_config(adc_hz, if_mode)?;
        let clk = adc_clock(adc_hz, self.s.ppb, false)?;
        if cfg == self.cfg && if_mode == self.s.if_mode {
            self.s.adc_hz = adc_hz;
            self.reg(0x04, clk.reg04)?;
            return self.reg(0x03, clk.reg03);
        }
        let was = self.streaming;
        if was {
            self.t.vendor_out(REQ_STOP, 0, 0)?;
            self.t.stop_iso()?;
            self.streaming = false;
            std::thread::sleep(self.settle);
        }
        self.reg(0x03, REG03_RATE_CHANGE)?;
        self.t.set_alt(cfg.alt)?;
        self.reg(0x07, cfg.reg07)?;
        self.reg(0x04, clk.reg04)?;
        self.reg(0x03, clk.reg03)?;
        let if_changed = if_mode != self.s.if_mode;
        self.cfg = cfg;
        self.s.adc_hz = adc_hz;
        self.s.if_mode = if_mode;
        if if_changed {
            self.tuner(self.r0_word())?;
        }
        if was {
            self.restart_stream()?;
        }
        self.tuner(self.gain_word())?;
        self.board(MCP_GPIOB, self.front_byte())
    }

    fn sink_for(core: Arc<Mutex<Core>>) -> PacketSink {
        // [@ANCHOR: rsp_device_rsp2_sink_for]
        Box::new(move |pkt: &[u8]| {
            if let Ok(mut c) = core.lock() {
                let Core { parser, cb, buf } = &mut *c;
                buf.clear();
                parser.push_packet(pkt, buf);
                if !buf.is_empty() {
                    let stats = parser.stats;
                    cb(buf, &stats);
                }
            }
        })
    }

    fn restart_stream(&mut self) -> Result<(), Error> {
        // [@ANCHOR: rsp_device_rsp2_restart_stream]
        let core = self
            .core
            .clone()
            .ok_or_else(|| Error::Stream("no stream callback".into()))?;
        {
            let mut c = core.lock().unwrap();
            let stats = c.parser.stats;
            c.parser = BlockParser::new(&self.cfg);
            c.parser.stats = stats;
        }
        self.t.start_iso(Self::sink_for(core))?;
        self.t.vendor_out(REQ_START, 0, 0)?;
        self.streaming = true;
        Ok(())
    }

    /// Start streaming; `cb` runs on the USB thread with each packet's samples and the running
    /// statistics.
    pub fn start_stream<F>(&mut self, cb: F) -> Result<(), Error>
    where
        F: FnMut(&[IqI16], &StreamStats) + Send + 'static,
    {
        // [@ANCHOR: rsp_device_rsp2_start_stream]
        if self.streaming {
            return Err(Error::Stream("already streaming".into()));
        }
        let core = Arc::new(Mutex::new(Core {
            parser: BlockParser::new(&self.cfg),
            cb: Box::new(cb),
            buf: Vec::with_capacity(1512),
        }));
        self.core = Some(core);
        self.restart_stream()
    }

    /// Start streaming into a bounded channel; returns a blocking iterator of sample chunks.
    pub fn start_stream_iter(&mut self, capacity: usize) -> Result<SampleIter, Error> {
        // [@ANCHOR: rsp_device_rsp2_start_stream_iter]
        let (tx, rx) = sync_channel(capacity.max(1));
        let dropped = Arc::new(Mutex::new(0u64));
        let d2 = dropped.clone();
        self.start_stream(move |s, _| match tx.try_send(s.to_vec()) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => *d2.lock().unwrap() += 1,
            Err(TrySendError::Disconnected(_)) => {}
        })?;
        Ok(SampleIter {
            rx,
            dropped,
            timeout: Duration::from_secs(2),
        })
    }

    /// Statistics of the current or last stream.
    pub fn stream_stats(&self) -> StreamStats {
        self.core
            .as_ref()
            .map(|c| c.lock().unwrap().parser.stats)
            .unwrap_or_default()
    }

    /// Stop streaming (STOP request first, then the transfers are cancelled).
    pub fn stop_stream(&mut self) -> Result<(), Error> {
        // [@ANCHOR: rsp_device_rsp2_stop_stream]
        if !self.streaming {
            return Ok(());
        }
        self.streaming = false;
        let a = self.t.vendor_out(REQ_STOP, 0, 0);
        let b = self.t.stop_iso();
        a.and(b)
    }

    pub fn is_streaming(&self) -> bool {
        self.streaming
    }

    /// Stop the stream, switch bias-T off, put the tuner and ADC in standby, select alt 0 and
    /// release the interface. Also run on drop.
    pub fn close(mut self) -> Result<(), Error> {
        self.shutdown()
    }

    fn shutdown(&mut self) -> Result<(), Error> {
        // [@ANCHOR: rsp_device_rsp2_shutdown]
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut first_err: Option<Error> = None;
        let mut keep = |r: Result<(), Error>| {
            if let Err(e) = r {
                first_err.get_or_insert(e);
            }
        };
        keep(self.stop_stream());
        self.s.bias_t = false;
        keep(self.board(MCP_GPIOB, self.front_byte()));
        keep(self.tuner(R12_CLOSE));
        keep(self.tuner(R0_STANDBY));
        keep(self.reg(0x03, REG03_CLOSE));
        keep(self.reg(0x05, REG05_CLOSE));
        keep(self.t.set_alt(0));
        keep(self.t.release());
        first_err.map_or(Ok(()), Err)
    }
}

impl<T: Transport> Drop for Rsp2<T> {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn check_antenna(ant: Antenna, rf_hz: u64) -> Result<(), Error> {
    // [@ANCHOR: rsp_device_check_antenna]
    if ant == Antenna::HiZ && rf_hz >= HIZ_MAX_HZ {
        return Err(Error::InvalidArgument(
            "the Hi-Z port works only below 60 MHz".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::tests::s14_block;
    use crate::transport::{MockTransport, Op};

    fn open_mock(s: Settings) -> (Rsp2<MockTransport>, MockTransport) {
        let m = MockTransport::new();
        let rx = Rsp2::with_transport(m.clone(), s, Duration::ZERO).unwrap();
        (rx, m)
    }

    fn w(reg: u8, v: u32) -> Op {
        let (value, index) = reg_write_args(reg, v);
        Op::Out {
            req: 0x41,
            value,
            index,
        }
    }
    fn t(v: u32) -> Op {
        w(0x09, v)
    }
    fn b(a: u8, v: u8) -> Op {
        let (value, index) = board_write_args(a, v);
        Op::Out {
            req: 0x49,
            value,
            index,
        }
    }
    fn r48() -> Op {
        Op::In {
            req: 0x48,
            value: 0,
            index: 0,
            len: 1,
        }
    }

    /// The API's own open sequence for zero IF, 2 MHz, 100 MHz, BW 200 kHz, GR 40, LNA 0
    /// (basic-open and frequency-sweep captures, protocol document 1.5) must be reproduced exactly.
    #[test]
    // Tests [@ANCHOR: rsp_device_rsp2_with_transport]
    // Tests [@ANCHOR: rsp_device_rsp2_init]
    // Tests [@ANCHOR: rsp_device_rsp2_full_tune]
    // Tests [@ANCHOR: rsp_transport_mocktransport_vendor_in]
    fn open_sequence_matches_api_capture() {
        let s = Settings {
            bandwidth: Bandwidth::K200,
            ..Settings::default()
        };
        let (rx, m) = open_mock(s);
        let want = vec![
            w(0x08, 0x00ee80),
            Op::Alt(2),
            r48(),
            r48(),
            w(0x05, 0x0c),
            t(0x00000e),
            w(0x00, 0x000200),
            w(0x02, 0x004801),
            t(0x28bb85),
            w(0x07, 0x14),
            t(0x200016),
            t(0x043420),
            b(0x12, 0x5d),
            b(0x13, 0xa5),
            b(0x00, 0),
            b(0x01, 0),
            t(0x00fff3),
            t(0x213e72),
            t(0x00000d),
            t(0x00000e),
            t(0x00c281),
            b(0x13, 0xb9),
            w(0x04, 0),
            w(0x03, 0x01181f),
        ];
        assert_eq!(m.ops(), want);
        assert_eq!(rx.read48, 0x04);
    }

    /// an earlier stand-alone verification program's sequence (BW 1.536 MHz, GR 30) except for r2, where this driver follows
    /// the API's rounding instead of rsp2verify's.
    #[test]
    // Tests [@ANCHOR: rsp_device_rsp2_with_transport]
    // Tests [@ANCHOR: rsp_device_rsp2_init]
    fn open_sequence_matches_rsp2verify() {
        let s = Settings {
            if_gr_db: 30,
            rf_hz: 95_800_000,
            ..Settings::default()
        };
        let (_rx, m) = open_mock(s);
        let ops = m.ops();
        assert_eq!(ops[11], t(0x04f420));
        assert_eq!(ops[20], t(0x00c1e1));
        // 95.8 MHz: 95.8e6 * 32 / 32 kHz = 95800 units exactly -> API style 95799.
        assert_eq!(ops[17], t((31 << 16) | (2799 << 4) | 2));
    }

    #[test]
    // Tests [@ANCHOR: rsp_device_rsp2_set_frequency]
    // Tests [@ANCHOR: rsp_device_rsp2_full_tune]
    fn retune_sequences() {
        let (mut rx, m) = open_mock(Settings::default());
        m.clear();
        // In-band: r3, r2 only.
        rx.set_frequency(101_000_000).unwrap();
        assert_eq!(m.ops(), vec![t(0x00fff3), t(0x217cf2)]);
        m.clear();
        // Band change to 10 MHz: full sequence with AM mode, filter 000.
        rx.set_frequency(10_000_000).unwrap();
        let want = vec![
            t(0x04fe10),
            b(0x12, 0x45),
            b(0x13, 0xa5),
            b(0x00, 0),
            b(0x01, 0),
            t(0x00fff3),
            t(0x157cf2),
            t(0x0d),
            t(0x0e),
            t(0x00c281),
            b(0x13, 0xb9),
        ];
        assert_eq!(m.ops(), want);
    }

    #[test]
    // Tests [@ANCHOR: rsp_device_rsp2_select_antenna]
    // Tests [@ANCHOR: rsp_device_rsp2_set_ppm]
    // Tests [@ANCHOR: rsp_device_rsp2_set_gain]
    // Tests [@ANCHOR: rsp_device_check_antenna]
    // Tests [@ANCHOR: rsp_device_rsp2_set_frequency]
    fn toggles_match_capture() {
        let (mut rx, m) = open_mock(Settings::default());
        m.clear();
        rx.select_antenna(Antenna::B).unwrap();
        rx.select_antenna(Antenna::A).unwrap();
        rx.set_notch(true).unwrap();
        rx.set_notch(false).unwrap();
        rx.set_ext_ref_out(true).unwrap();
        rx.set_ext_ref_out(false).unwrap();
        assert_eq!(
            m.ops(),
            vec![
                b(0x12, 0x5e),
                b(0x12, 0x5d),
                b(0x13, 0x79),
                b(0x13, 0xb9),
                b(0x12, 0x1d),
                b(0x12, 0x5d)
            ]
        );
        m.clear();
        rx.set_ppm(10.0).unwrap();
        assert_eq!(
            m.ops(),
            vec![
                t(0x000003),
                t(0x213e72),
                w(0x04, 0x0fff58),
                w(0x03, 0x01179f)
            ]
        );
        m.clear();
        rx.set_ppm(0.0).unwrap();
        assert_eq!(
            m.ops(),
            vec![t(0x00fff3), t(0x213e72), w(0x04, 0), w(0x03, 0x01181f)]
        );
        m.clear();
        rx.set_gain(20, 0).unwrap();
        assert_eq!(m.ops(), vec![t(0x00c141), b(0x13, 0xb9)]);
        // Hi-Z above 60 MHz is refused; below 60 MHz it re-tunes with mode 0x61.
        assert!(rx.select_antenna(Antenna::HiZ).is_err());
        rx.set_frequency(10_000_000).unwrap();
        m.clear();
        rx.select_antenna(Antenna::HiZ).unwrap();
        assert_eq!(
            m.ops()[0],
            t((2 << 17) | (3 << 14) | (3 << 12) | (0x61 << 4))
        );
        rx.set_gain(40, 4).unwrap();
        assert_eq!(m.ops().last().cloned(), Some(b(0x13, 0xb9)));
        assert!(rx.set_gain(40, 5).is_err());
        assert!(rx.set_frequency(70_000_000).is_err());
    }

    #[test]
    // Tests [@ANCHOR: rsp_device_rsp2_set_frequency]
    // Tests [@ANCHOR: rsp_device_rsp2_set_gain]
    fn lna_state_clamped_on_band_change() {
        let (mut rx, _m) = open_mock(Settings::default());
        rx.set_gain(40, 8).unwrap();
        rx.set_frequency(446_000_000).unwrap();
        assert_eq!(rx.settings().lna_state, 5);
    }

    #[test]
    // Tests [@ANCHOR: rsp_device_rsp2_set_sampling]
    // Tests [@ANCHOR: rsp_device_rsp2_output_rate]
    fn sample_rate_change_sequence() {
        let (mut rx, m) = open_mock(Settings::default());
        m.clear();
        rx.set_sample_rate(2_048_000).unwrap();
        assert_eq!(
            m.ops(),
            vec![
                w(0x03, 0x011300),
                Op::Alt(2),
                w(0x07, 0x05),
                w(0x04, 0x0624dd),
                w(0x03, 0x01081f),
                t(0x00c281),
                b(0x13, 0xb9)
            ]
        );
        rx.set_sample_rate(8_192_000).unwrap();
        m.clear();
        rx.set_sample_rate(9_000_000).unwrap();
        assert_eq!(m.ops(), vec![w(0x04, 0), w(0x03, 0x01c907)]);
        assert!(rx.set_sample_rate(1_536_000).is_err());
        assert_eq!(rx.output_rate(), 9_000_000.0);
        rx.set_if_mode(IfMode::Low1620k).unwrap();
        assert_eq!(rx.output_rate(), 3_000_000.0);
    }

    #[test]
    // Tests [@ANCHOR: rsp_device_rsp2_start_stream_iter]
    // Tests [@ANCHOR: rsp_device_rsp2_start_stream]
    fn stream_iterator_delivers_packets() {
        let (mut rx, m) = open_mock(Settings::default());
        m.queue_packet(s14_block(0));
        m.queue_packet(s14_block(252));
        let it = rx.start_stream_iter(8).unwrap();
        let chunks: Vec<Vec<IqI16>> = it.take(2).collect();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 252);
        assert!(rx.start_stream(|_, _| {}).is_err());
        rx.stop_stream().unwrap();
    }

    #[test]
    // Tests [@ANCHOR: rsp_device_rsp2_start_stream]
    // Tests [@ANCHOR: rsp_device_rsp2_sink_for]
    // Tests [@ANCHOR: rsp_device_rsp2_restart_stream]
    // Tests [@ANCHOR: rsp_device_rsp2_stop_stream]
    // Tests [@ANCHOR: rsp_device_rsp2_shutdown]
    // Tests [@ANCHOR: rsp_transport_mocktransport_start_iso]
    fn streaming_and_close() {
        let (mut rx, m) = open_mock(Settings::default());
        m.queue_packet(s14_block(0));
        m.queue_packet(s14_block(252));
        m.queue_packet(s14_block(1008));
        let got = Arc::new(Mutex::new(0usize));
        let g2 = got.clone();
        m.clear();
        rx.start_stream(move |s, _| *g2.lock().unwrap() += s.len())
            .unwrap();
        assert_eq!(*got.lock().unwrap(), 756);
        let st = rx.stream_stats();
        assert_eq!((st.blocks, st.gaps, st.lost_samples), (3, 1, 504));
        rx.set_bias_t(true).unwrap();
        rx.close().unwrap();
        let ops = m.ops();
        assert_eq!(ops[0], Op::IsoStart);
        assert_eq!(
            ops[1],
            Op::Out {
                req: 0x43,
                value: 0,
                index: 0
            }
        );
        let tail: Vec<Op> = ops[ops.len() - 9..].to_vec();
        assert_eq!(
            tail,
            vec![
                Op::Out {
                    req: 0x45,
                    value: 0,
                    index: 0
                },
                Op::IsoStop,
                b(0x13, 0xb9),
                t(0x1c),
                t(0),
                w(0x03, 0x091300),
                w(0x05, 0x04),
                Op::Alt(0),
                Op::Release,
            ]
        );
    }

    /// Every request the driver can emit must be one the protocol document lists as observed.
    fn assert_allowed(ops: &[Op]) {
        for op in ops {
            match op {
                Op::Out {
                    req: 0x41,
                    value,
                    index,
                } => {
                    let reg = (*value & 0xff) as u8;
                    assert!(
                        [0x00, 0x02, 0x03, 0x04, 0x05, 0x07, 0x08, 0x09].contains(&reg),
                        "register {reg:02x}"
                    );
                    if reg == 0x09 {
                        let word = ((*index as u32) << 8) | (*value as u32 >> 8);
                        assert!(
                            [0, 1, 2, 3, 5, 6, 12, 13, 14].contains(&(word & 0xf)),
                            "tuner reg {}",
                            word & 0xf
                        );
                    }
                }
                Op::Out {
                    req: 0x43 | 0x45,
                    value: 0,
                    index: 0,
                } => {}
                Op::Out {
                    req: 0x49,
                    value,
                    index: 0x40,
                } => {
                    assert!(
                        [0x00, 0x01, 0x12, 0x13].contains(&((*value >> 8) as u8)),
                        "board addr"
                    );
                }
                Op::In {
                    req: 0x48,
                    value: 0,
                    index: 0,
                    len: 1,
                } => {}
                Op::Alt(a) => assert!(*a <= 2, "alt {a}"),
                Op::IsoStart | Op::IsoStop | Op::Release => {}
                other => panic!("request not in the observed list: {other:?}"),
            }
        }
    }

    #[test]
    // Tests [@ANCHOR: rsp_device_rsp2_set_sampling]
    // Tests [@ANCHOR: rsp_device_rsp2_restart_stream]
    // Tests [@ANCHOR: rsp_device_rsp2_shutdown]
    // Tests [@ANCHOR: rsp_device_rsp2_select_antenna]
    fn only_observed_requests_are_emitted() {
        let (mut rx, m) = open_mock(Settings::default());
        for f in [
            1_000u64,
            7_074_000,
            14_000_000,
            50_000_000,
            95_800_000,
            144_000_000,
            300_000_000,
            446_000_000,
            1_296_000_000,
            2_000_000_000,
        ] {
            rx.set_frequency(f).unwrap();
            for l in 0..rx.lna_states() {
                rx.set_gain(0, l).unwrap();
                rx.set_gain(59, l).unwrap();
            }
            rx.select_antenna(Antenna::B).unwrap();
            rx.select_antenna(Antenna::A).unwrap();
            if f < 60_000_000 {
                rx.select_antenna(Antenna::HiZ).unwrap();
                rx.select_antenna(Antenna::A).unwrap();
            }
            rx.set_notch(true).unwrap();
            rx.set_notch(false).unwrap();
        }
        rx.start_stream(|_, _| {}).unwrap();
        for fs in [
            2_000_000, 2_048_000, 3_000_000, 6_000_000, 7_000_000, 8_192_000, 9_000_000, 10_000_000,
        ] {
            rx.set_sample_rate(fs).unwrap();
        }
        for mode in [
            IfMode::Low450k,
            IfMode::Low1620k,
            IfMode::Low2048k,
            IfMode::Zero,
        ] {
            rx.set_if_mode(mode).unwrap();
        }
        for bw in [Bandwidth::K200, Bandwidth::M8] {
            rx.set_bandwidth(bw).unwrap();
        }
        rx.set_ppm(-3.5).unwrap();
        rx.set_ext_ref_out(true).unwrap();
        rx.stop_stream().unwrap();
        rx.close().unwrap();
        let ops = m.ops();
        assert!(ops.len() > 300);
        assert_allowed(&ops);
        // Bias-T was never switched on in this test.
        for op in &ops {
            if let Op::Out {
                req: 0x49, value, ..
            } = op
            {
                if value >> 8 == 0x13 {
                    assert_eq!(value & 1, 1, "bias-T on");
                }
            }
        }
    }

    #[test]
    // Tests [@ANCHOR: rsp_device_rsp2_with_transport]
    // Tests [@ANCHOR: rsp_device_rsp2_set_frequency]
    fn refuses_bias_t_at_open_and_unsupported_band() {
        let m = MockTransport::new();
        assert!(Rsp2::with_transport(
            m.clone(),
            Settings {
                bias_t: true,
                ..Settings::default()
            },
            Duration::ZERO
        )
        .is_err());
        assert!(m.ops().is_empty());
        let (mut rx, _) = open_mock(Settings::default());
        assert!(matches!(
            rx.set_frequency(400_000_000),
            Err(Error::Unsupported(_))
        ));
        assert_eq!(rx.settings().rf_hz, 100_000_000);
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! A fake bus for the tests: one FDM-S2 whose control replies are the recording in `tests/elad_fixtures/fdm_s2_fw3_0_open.txt` (a real
//! unit's, scrubbed) and whose bulk endpoint serves a synthetic complex tone, so the whole tool path runs without hardware.

use super::Bench;
use elad_usb::radio::{OpenOptions, Radio};
use elad_usb::transport::{FakeTransport, Transport};
use elad_usb::usb::Found;
use elad_usb::Error;
use std::sync::Mutex;

pub const FIXTURE: &str = include_str!("../../tests/elad_fixtures/fdm_s2_fw3_0_open.txt");

pub struct FakeBench {
    pub devices: Vec<Found>,
    /// The tone: offset from the centre in Hz and amplitude (full scale 1.0).
    pub tone_hz: f64,
    pub amplitude: f64,
    /// Tones at absolute RF frequencies (Hz, amplitude): when not empty the bulk endpoint is synthesised on demand from the tuning the
    /// bench tool last sent, so a sweep sees each tone where it belongs.
    pub tones_rf: Vec<(f64, f64)>,
    /// Gaussian noise, rms per component (full scale 1.0), with `tones_rf`.
    pub noise_rms: f64,
    /// When true the bulk endpoint never answers (an FDM-S2 with no FPGA image).
    pub silent: bool,
    /// The transports handed out, to inspect what was sent.
    pub opened: Mutex<Vec<FakeTransport>>,
}

pub fn s2() -> Found {
    Found {
        bus: 1,
        address: 8,
        pid: 0x061c,
        model: elad_usb::protocol::Model::S2,
    }
}

impl FakeBench {
    pub fn new(devices: Vec<Found>) -> FakeBench {
        FakeBench {
            devices,
            tone_hz: 10_000.0,
            amplitude: 0.1,
            tones_rf: Vec::new(),
            noise_rms: 0.0,
            silent: false,
            opened: Mutex::new(Vec::new()),
        }
    }

    pub fn sent_requests(&self) -> Vec<u8> {
        use elad_usb::transport::Op;
        self.opened
            .lock()
            .map(|o| {
                o.iter()
                    .flat_map(|t| t.ops())
                    .filter_map(|op| {
                        if let Op::Out { request, .. } = op {
                            Some(request)
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn transport(rate: u32, tone_hz: f64, amp: f64, silent: bool) -> FakeTransport {
    let t = FakeTransport::new();
    for line in FIXTURE
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let mut p = line.splitn(3, ' ');
        let req = u8::from_str_radix(p.next().unwrap_or("0"), 16).unwrap_or(0);
        let val = u16::from_str_radix(p.next().unwrap_or("0"), 16).unwrap_or(0);
        let bytes: Vec<u8> = p
            .next()
            .unwrap_or("")
            .split(' ')
            .filter_map(|b| u8::from_str_radix(b, 16).ok())
            .collect();
        t.reply(req, val, bytes);
    }
    if silent {
        return t;
    }
    let wide = rate < 6_144_000;
    let per_chunk = if wide { 6144 } else { 12288 };
    let mut n = 0usize;
    for _ in 0..200 {
        let mut bytes = Vec::new();
        for _ in 0..per_chunk {
            let ph = 2.0 * std::f64::consts::PI * tone_hz * n as f64 / f64::from(rate);
            let (i, q) = (amp * ph.cos(), amp * ph.sin());
            if wide {
                bytes.extend_from_slice(&((i * 2_147_483_648.0) as i32).to_le_bytes());
                bytes.extend_from_slice(&((q * 2_147_483_648.0) as i32).to_le_bytes());
            } else {
                bytes.extend_from_slice(&((i * 32_768.0) as i16).to_le_bytes());
                bytes.extend_from_slice(&((q * 32_768.0) as i16).to_le_bytes());
            }
            n += 1;
        }
        t.queue_bulk(Ok(bytes));
    }
    t
}

impl Bench for FakeBench {
    fn find(&self) -> Result<Vec<Found>, String> {
        Ok(self.devices.clone())
    }

    fn open(&self, found: &Found, opts: OpenOptions) -> Result<Radio<Box<dyn Transport>>, String> {
        if !self.tones_rf.is_empty() && !self.silent {
            let s = Synth::new(opts.sample_rate_hz, self.tones_rf.clone(), self.noise_rms);
            if let Ok(mut o) = self.opened.lock() {
                o.push(s.ctl.clone());
            }
            return Radio::open(Box::new(s) as Box<dyn Transport>, found.model, opts)
                .map_err(|e: Error| e.to_string());
        }
        let t = transport(
            opts.sample_rate_hz,
            self.tone_hz,
            self.amplitude,
            self.silent,
        );
        if let Ok(mut o) = self.opened.lock() {
            o.push(t.clone());
        }
        Radio::open(Box::new(t) as Box<dyn Transport>, found.model, opts)
            .map_err(|e: Error| e.to_string())
    }
}

/// A receiver that synthesises its samples on demand: tones at absolute RF frequencies seen through the tuning word the driver last
/// sent, plus deterministic Gaussian noise. Control requests go to a recording `FakeTransport`.
pub struct Synth {
    pub ctl: FakeTransport,
    rate: u32,
    tones: Vec<(f64, f64)>,
    noise: f64,
    centre_hz: f64,
    n: u64,
    rng: u64,
}

impl Synth {
    pub fn new(rate: u32, tones: Vec<(f64, f64)>, noise: f64) -> Synth {
        Synth {
            ctl: transport(rate, 0.0, 0.0, true),
            rate,
            tones,
            noise,
            centre_hz: 0.0,
            n: 0,
            rng: 0x2545_F491_4F6C_DD1D,
        }
    }

    fn uniform(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    fn gauss(&mut self) -> f64 {
        (0..12).map(|_| self.uniform()).sum::<f64>() - 6.0
    }
}

impl Transport for Synth {
    fn control_in(&mut self, r: u8, v: u16, i: u16, b: &mut [u8]) -> Result<usize, Error> {
        self.ctl.control_in(r, v, i, b)
    }

    fn control_out(&mut self, r: u8, v: u16, i: u16, d: &[u8]) -> Result<usize, Error> {
        if r == elad_usb::protocol::REQ_TUNE {
            let word = u32::from(v) | (u32::from(i) << 16);
            self.centre_hz =
                f64::from(word) / 4_294_967_296.0 * elad_usb::protocol::Model::S2.adc_clock_hz();
        }
        self.ctl.control_out(r, v, i, d)
    }

    fn bulk_in(&mut self, buf: &mut [u8], _t: std::time::Duration) -> Result<usize, Error> {
        let wide = self.rate < 6_144_000;
        let width = if wide { 4 } else { 2 };
        let count = (buf.len() / (2 * width)).min(6144);
        let scale = if wide { 2_147_483_647.0 } else { 32_767.0 };
        let mut at = 0;
        for _ in 0..count {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (rf, amp) in self.tones.clone() {
                // The DDC's decimation filter removes what lies outside the passband; without this a tone far from the centre would alias in.
                if (rf - self.centre_hz).abs() > 0.45 * f64::from(self.rate) {
                    continue;
                }
                let ph = 2.0
                    * std::f64::consts::PI
                    * ((rf - self.centre_hz) * self.n as f64 / f64::from(self.rate)).fract();
                re += amp * ph.cos();
                im += amp * ph.sin();
            }
            if self.noise > 0.0 {
                re += self.noise * self.gauss();
                im += self.noise * self.gauss();
            }
            for x in [re, im] {
                let v = (x.clamp(-1.0, 1.0) * scale) as i64;
                if wide {
                    buf[at..at + 4].copy_from_slice(&(v as i32).to_le_bytes());
                } else {
                    buf[at..at + 2].copy_from_slice(&(v as i16).to_le_bytes());
                }
                at += width;
            }
            self.n += 1;
        }
        Ok(at)
    }
}

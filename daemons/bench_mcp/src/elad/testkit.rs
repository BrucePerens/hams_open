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

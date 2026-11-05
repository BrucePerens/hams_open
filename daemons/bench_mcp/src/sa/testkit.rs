// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test helpers for the sa instrument: a fake device on a local TCP socket, the pattern the scope tests use
//! (`src/scope.rs`, `fake_scope`). The code under test opens it with the production `Link::tcp`, so the whole path
//! (connect, timeouts, line handling, the dialect) is the production one; only the far end is fake.
//!
//! `FakeDevice::from_transcript` replays a recorded, scrubbed session (`tests/sa_fixtures/*.txt`);
//! `FakeDevice::sim` is a small simulator with switches for the faults the tools must survive.

use crate::config::Instrument;
use crate::link::Link;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// What the far end does with one command.
pub enum Reply {
    Text(String),
    /// Say nothing (a lost reply, a hung instrument).
    Silent,
    /// Close the connection.
    Hangup,
}

pub struct FakeDevice {
    port: u16,
    log: Arc<Mutex<Vec<String>>>,
}

impl FakeDevice {
    pub fn serve(handler: impl FnMut(&str) -> Reply + Send + 'static) -> FakeDevice {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(Mutex::new(handler));
        let seen = log.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(sock) = conn else { break };
                let (handler, seen) = (handler.clone(), seen.clone());
                std::thread::spawn(move || {
                    let mut out = sock.try_clone().unwrap();
                    for line in BufReader::new(sock).lines() {
                        let Ok(line) = line else { break };
                        seen.lock().unwrap().push(line.clone());
                        let reply = (handler.lock().unwrap())(&line);
                        match reply {
                            Reply::Text(t) => {
                                if writeln!(out, "{t}").is_err() {
                                    break;
                                }
                            }
                            Reply::Silent => {}
                            Reply::Hangup => break,
                        }
                    }
                });
            }
        });
        FakeDevice { port, log }
    }

    /// A device that answers with `f`; `None` is silence.
    pub fn start(mut f: impl FnMut(&str) -> Option<String> + Send + 'static) -> FakeDevice {
        FakeDevice::serve(move |c| f(c).map_or(Reply::Silent, Reply::Text))
    }

    pub fn sim(mut s: Sim) -> FakeDevice {
        FakeDevice::serve(move |c| s.handle(c))
    }

    /// Replays a recorded session: for each command the next recorded reply to that same command. A command the
    /// recording does not contain gets silence (and shows in `received`), so a test notices a changed conversation.
    pub fn from_transcript(path: &str) -> FakeDevice {
        let full = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
        let text =
            std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{}: {e}", full.display()));
        let mut replies: HashMap<String, Vec<String>> = HashMap::new();
        let mut last = String::new();
        for l in text.lines() {
            if let Some(c) = l.strip_prefix("> ") {
                last = c.to_string();
                replies.entry(last.clone()).or_default();
            } else if let Some(r) = l.strip_prefix("< ") {
                replies
                    .get_mut(&last)
                    .expect("a reply before any command")
                    .push(r.to_string());
            }
        }
        for v in replies.values_mut() {
            v.reverse();
        }
        FakeDevice::serve(move |c| match replies.get_mut(c).and_then(|v| v.pop()) {
            Some(r) => Reply::Text(r),
            None => Reply::Silent,
        })
    }

    /// The local port the device listens on, for a bench config that names it.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The commands received so far, in order, across all connections.
    pub fn received(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    /// A new link to this device, for code that opens its own (the supply's timed switch-off).
    pub fn link(&self) -> impl Fn() -> Result<Link, String> + Send + 'static {
        let port = self.port;
        move || Link::tcp("127.0.0.1", port)
    }

    /// The way for `call_with` to reach this device.
    pub fn connector(&self) -> impl Fn(&Instrument) -> Result<Link, String> {
        let port = self.port;
        move |_| Link::tcp("127.0.0.1", port)
    }
}

/// A simulator of the SSA3000X dialect, with fault switches. It keeps the settings it is given, rounds the RBW up like the
/// unit, clamps the stop frequency to 2.1 GHz, and makes a 751-point trace: a floor near -90 dBm, a tone of -30 dBm at point
/// 100 (only while the generator is off), and, while the generator is on, the generator level minus the shared `dut_db` loss at every point (an extra 10 dB dip
/// at point 300 when the loss is over 5 dB).
pub struct Sim {
    pub on: bool,
    pub level: f64,
    pub start: f64,
    pub stop: f64,
    /// The highest stop frequency the simulated unit accepts (it clamps to it).
    pub clamp_stop: f64,
    pub rbw: f64,
    pub vbw: f64,
    pub ref_level: f64,
    pub atten: f64,
    pub detector: String,
    pub continuous: bool,
    pub sweep_time: f64,
    pub trace: Vec<f64>,
    pub marker: (f64, f64),
    /// The loss of the "device under test" between the TG output and the RF input, dB; shared so a test can change it.
    pub dut_db: std::sync::Arc<Mutex<f64>>,
    /// `OUTPut:STATe OFF` is accepted but the output stays on.
    pub stuck_on: bool,
    /// The connection is closed when this command arrives.
    pub hangup_on: Option<String>,
    /// The first this many queries get no answer.
    pub silent_queries: u32,
    /// This query gets this reply instead of the true one.
    pub garbage: Option<(String, String)>,
    /// The options reply.
    pub options: String,
}

impl Default for Sim {
    fn default() -> Sim {
        Sim {
            on: false,
            level: -20.0,
            start: 0.0,
            stop: 2.1e9,
            clamp_stop: 2.1e9,
            rbw: 1e6,
            vbw: 1e6,
            ref_level: 0.0,
            atten: 20.0,
            detector: "POSitive".into(),
            continuous: true,
            sweep_time: 0.216,
            trace: Vec::new(),
            marker: (0.0, -999.0),
            dut_db: Arc::new(Mutex::new(0.5)),
            stuck_on: false,
            hangup_on: None,
            silent_queries: 0,
            garbage: None,
            options: "Meas EMI TG CAT ".into(),
        }
    }
}

fn hz(arg: &str) -> f64 {
    arg.trim().trim_end_matches("Hz").trim().parse().unwrap_or(f64::NAN)
}

impl Sim {
    fn make_trace(&mut self) {
        let loss = *self.dut_db.lock().unwrap();
        self.trace = (0..751)
            .map(|i| {
                let mut v = -90.0 + (i % 7) as f64 * 0.1;
                if i == 100 && !self.on {
                    v = -30.0;
                }
                if self.on {
                    let dip = if i == 300 && loss > 5.0 { 10.0 } else { 0.0 };
                    v = v.max(self.level - loss - dip);
                }
                v
            })
            .collect();
    }

    fn freq_of(&self, i: usize) -> f64 {
        self.start + (self.stop - self.start) * i as f64 / 750.0
    }

    fn handle(&mut self, c: &str) -> Reply {
        if self.hangup_on.as_deref() == Some(c) {
            return Reply::Hangup;
        }
        if c.contains('?') && self.silent_queries > 0 {
            self.silent_queries -= 1;
            return Reply::Silent;
        }
        if let Some((q, r)) = &self.garbage {
            if q == c {
                return Reply::Text(r.clone());
            }
        }
        let text = |s: String| Reply::Text(s);
        let e = |v: f64| text(format!("{v:+.10E}"));
        let (cmd, arg) = c.split_once(' ').unwrap_or((c, ""));
        match cmd {
            "*IDN?" => text("Siglent Technologies,SSA3021X,SN-0000,1.3.9.10".into()),
            "*OPC?" => text("1".into()),
            ":SYSTem:OPTions?" => text(self.options.clone()),
            ":OUTPut:STATe" => {
                self.on = arg == "ON" || (self.stuck_on && self.on);
                Reply::Silent
            }
            ":OUTPut:STATe?" => text(if self.on { "1" } else { "0" }.into()),
            ":SOURce:POWer" => {
                self.level = arg.parse().unwrap_or(self.level);
                Reply::Silent
            }
            ":SOURce:POWer?" => text(format!("{:.6}", self.level)),
            ":SOURce:CORRection:OFFSet?" => text("0.000000".into()),
            ":INITiate:CONTinuous" => {
                self.continuous = arg == "ON";
                Reply::Silent
            }
            ":INITiate:CONTinuous?" => text(if self.continuous { "1" } else { "0" }.into()),
            ":INITiate:IMMediate" => {
                self.make_trace();
                Reply::Silent
            }
            ":SENSe:FREQuency:STARt" => {
                self.start = hz(arg);
                Reply::Silent
            }
            ":SENSe:FREQuency:STOP" => {
                self.stop = hz(arg).min(self.clamp_stop);
                Reply::Silent
            }
            ":SENSe:FREQuency:STARt?" => e(self.start),
            ":SENSe:FREQuency:STOP?" => e(self.stop),
            ":SENSe:BWIDth:RESolution" => {
                let want = hz(arg);
                self.rbw = [10.0, 30.0, 100.0, 300.0, 1e3, 3e3, 1e4, 3e4, 1e5, 3e5, 1e6]
                    .into_iter()
                    .find(|&b| b >= want)
                    .unwrap_or(1e6);
                Reply::Silent
            }
            ":SENSe:BWIDth?" => e(self.rbw),
            ":SENSe:BWIDth:VIDeo" => {
                self.vbw = hz(arg);
                Reply::Silent
            }
            ":SENSe:BWIDth:VIDeo?" => e(self.vbw),
            ":DISPlay:WINDow:TRACe:Y:RLEVel" => {
                self.ref_level = arg.parse().unwrap_or(self.ref_level);
                Reply::Silent
            }
            ":DISPlay:WINDow:TRACe:Y:RLEVel?" => e(self.ref_level),
            ":SENSe:POWer:RF:ATTenuation" => {
                self.atten = arg.parse().unwrap_or(self.atten);
                Reply::Silent
            }
            ":SENSe:POWer:RF:ATTenuation?" => text(format!("{:.6}", self.atten)),
            ":SENSe:DETector:TRACe1:FUNCtion" => {
                self.detector = arg.into();
                Reply::Silent
            }
            ":SENSe:DETector:TRACe1:FUNCtion?" => text(self.detector.clone()),
            ":SENSe:SWEep:TIME?" => e(self.sweep_time),
            ":TRACe:DATA?" => {
                if self.trace.is_empty() {
                    self.make_trace();
                }
                text(
                    self.trace
                        .iter()
                        .map(|v| format!("{v:+.9E}"))
                        .collect::<Vec<_>>()
                        .join(","),
                )
            }
            _ if c.contains(":MAXimum") => {
                let i = (0..self.trace.len())
                    .max_by(|&a, &b| self.trace[a].total_cmp(&self.trace[b]))
                    .unwrap_or(0);
                self.marker = (self.freq_of(i), self.trace.get(i).copied().unwrap_or(-999.0));
                Reply::Silent
            }
            _ if c.contains(":X?") => e(self.marker.0),
            _ if c.contains(":Y?") => e(self.marker.1),
            _ if c.contains(":X ") => {
                let f = hz(arg);
                let i = (((f - self.start) / (self.stop - self.start) * 750.0).round() as usize).min(750);
                self.marker = (f, self.trace.get(i).copied().unwrap_or(-999.0));
                Reply::Silent
            }
            _ => Reply::Silent,
        }
    }
}

/// A device that answers `*IDN?` as an SSA3021X and sends `bytes` raw (no line end) for the screenshot query.
pub fn bmp_device(bytes: Vec<u8>) -> u16 {
    use std::io::Read;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut sock) = conn else { break };
            let bytes = bytes.clone();
            std::thread::spawn(move || {
                let mut line = Vec::new();
                let mut b = [0u8; 1];
                while sock.read_exact(&mut b).is_ok() {
                    if b[0] != b'\n' {
                        line.push(b[0]);
                        continue;
                    }
                    let cmd = String::from_utf8_lossy(&line).to_string();
                    line.clear();
                    let ok = if cmd == "*IDN?" {
                        writeln!(sock, "Siglent Technologies,SSA3021X,SN-0000,1.3.9.10").is_ok()
                    } else if cmd.starts_with(":HCOPy:SDUMp:DATA?") {
                        sock.write_all(&bytes).is_ok()
                    } else {
                        true
                    };
                    if !ok {
                        break;
                    }
                }
            });
        }
    });
    port
}

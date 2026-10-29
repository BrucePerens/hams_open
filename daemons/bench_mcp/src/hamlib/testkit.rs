// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test helpers for the hamlib module: a fake rigctld on a local TCP socket, for the faults a real one rarely produces (silence, a
//! closed connection, error codes, a stuck PTT, scripted S-meter readings). The happy paths use the real `rigctld -m 1` (Hamlib's
//! dummy rig), started by the module's own managed-daemon code on a free localhost port.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// What the far end does with one command (given without the `+\` prefix).
pub enum Reply {
    /// Lines to send after the echo line, and the return code.
    Rprt(Vec<String>, i32),
    /// Text sent exactly as given (a reply with no echo line and no RPRT, as `chk_vfo` has).
    Raw(String),
    Silent,
    Hangup,
}

#[cfg(test)]
pub fn ok(lines: &[&str]) -> Reply {
    Reply::Rprt(lines.iter().map(|s| s.to_string()).collect(), 0)
}

#[cfg(test)]
pub fn err(code: i32) -> Reply {
    Reply::Rprt(vec![], code)
}

pub struct FakeRigctld {
    pub port: u16,
    log: Arc<Mutex<Vec<String>>>,
}

#[cfg(test)]
impl FakeRigctld {
    #[cfg(test)]
    pub fn serve(handler: impl FnMut(&str) -> Reply + Send + 'static) -> FakeRigctld {
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
                        let cmd = line.trim_start_matches("+\\").to_string();
                        seen.lock().unwrap().push(cmd.clone());
                        let reply = (handler.lock().unwrap())(&cmd);
                        match reply {
                            Reply::Rprt(lines, code) => {
                                let name = cmd.split_whitespace().next().unwrap_or("");
                                let args = cmd.strip_prefix(name).unwrap_or("").trim();
                                let mut text = format!(
                                    "{name}:{}{args}\n",
                                    if args.is_empty() { "" } else { " " }
                                );
                                for l in lines {
                                    text.push_str(&l);
                                    text.push('\n');
                                }
                                text.push_str(&format!("RPRT {code}\n"));
                                if out.write_all(text.as_bytes()).is_err() {
                                    break;
                                }
                            }
                            Reply::Raw(t) => {
                                if out.write_all(t.as_bytes()).is_err() {
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
        FakeRigctld { port, log }
    }

    /// The commands received so far, across connections, in order.
    pub fn received(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

/// A scripted receiver: answers the commands of the read tools, with a sequence of S-meter readings that repeats and a PTT it remembers.
pub struct Radio {
    pub freq: f64,
    pub mode: String,
    pub ptt: bool,
    pub strength: Vec<f64>,
    pub next: usize,
    pub rfpower: f64,
    pub swr: f64,
}

#[cfg(test)]
impl Radio {
    #[cfg(test)]
    pub fn new(strength: &[f64]) -> Radio {
        Radio {
            freq: 7_100_000.0,
            mode: "USB".into(),
            ptt: false,
            strength: strength.to_vec(),
            next: 0,
            rfpower: 0.5,
            swr: 1.1,
        }
    }

    #[cfg(test)]
    pub fn handle(&mut self, cmd: &str) -> Reply {
        let mut w = cmd.split_whitespace();
        match (w.next(), w.next()) {
            (Some("get_freq"), _) => ok(&[&format!("Frequency: {}", self.freq)]),
            (Some("set_freq"), Some(f)) => {
                self.freq = f.parse().unwrap_or(self.freq);
                ok(&[])
            }
            (Some("get_mode"), _) => ok(&[&format!("Mode: {}", self.mode), "Passband: 2400"]),
            (Some("set_mode"), Some(m)) => {
                self.mode = m.to_string();
                ok(&[])
            }
            (Some("get_ptt"), _) => ok(&[&format!("PTT: {}", u8::from(self.ptt))]),
            (Some("set_ptt"), Some(v)) => {
                self.ptt = v == "1";
                ok(&[])
            }
            (Some("get_level"), Some("?")) => ok(&["STRENGTH AF RFPOWER SWR ALC ATT PREAMP"]),
            (Some("get_level"), Some("STRENGTH")) => {
                let v = self.strength[self.next % self.strength.len()];
                self.next += 1;
                ok(&[&format!("{v}")])
            }
            (Some("get_level"), Some("RFPOWER")) => ok(&[&format!("{}", self.rfpower)]),
            (Some("get_level"), Some("SWR")) => ok(&[&format!("{}", self.swr)]),
            (Some("set_level"), Some("RFPOWER")) => {
                self.rfpower = w
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(self.rfpower);
                ok(&[])
            }
            (Some("get_level"), Some(_)) => ok(&["0"]),
            (Some("get_func"), Some("?")) => ok(&["NB NR"]),
            (Some("get_func"), Some(_)) => ok(&["0"]),
            _ => err(-11),
        }
    }
}

/// A signal the simulated receiver hears: at `hz` (within 2 kHz), `db` over the floor in the S-meter, and what its audio holds.
#[cfg(test)]
#[derive(Clone, Debug)]
pub struct Signal {
    pub hz: f64,
    pub db: f64,
    /// `wwv` (second ticks and a 500 Hz tone), `carrier` (a steady 1500 Hz note) or `voice` (loud noise).
    pub audio: &'static str,
}

/// A simulated rig and its audio: the S-meter and the audio follow the frequency the rig is tuned to.
#[cfg(test)]
pub struct Sim {
    pub freq: f64,
    pub mode: String,
    pub width: f64,
    pub ptt: bool,
    pub af: f64,
    pub preamp: f64,
    pub att: f64,
    pub signals: Vec<Signal>,
    /// The S-meter reading with no signal (dB relative to S9), and the repeating wobble added to it.
    pub floor_db: f64,
    pub wobble: Vec<f64>,
    pub n: usize,
    /// The audio input returns only zeros.
    pub audio_dead: bool,
    /// The S-meter command fails.
    pub meter_broken: bool,
    pub log: Vec<String>,
    /// Transmit side: the power setting, the SWR reading while keyed, split and XIT, and the faults.
    pub rfpower: f64,
    pub swr: f64,
    pub split: bool,
    pub xit: f64,
    /// Reading a level while keyed fails with this code.
    pub error_when_keyed: Option<i32>,
    /// The connection is closed by the next command after the PTT goes on (a client that vanished).
    pub hangup_when_keyed: bool,
    /// `set_ptt 0` is accepted but the PTT stays on (a rig that will not un-key).
    pub ptt_sticks: bool,
    /// Every `set_ptt` value received, in order.
    pub ptt_commands: Vec<u8>,
    /// The next `set_ptt 0` is not executed: its connection is closed (the connection in use was lost at the moment of release).
    pub hangup_on_unkey: bool,
    /// Like rigctld: the second half of `dump_state` (level ranges) is printed only after a `chk_vfo`.
    pub vfo_checked: bool,
}

#[cfg(test)]
impl Sim {
    pub fn new() -> Sim {
        Sim {
            freq: 7_100_000.0,
            mode: "USB".into(),
            width: 2400.0,
            ptt: false,
            af: 0.3,
            preamp: 0.0,
            att: 10.0,
            signals: vec![],
            floor_db: -54.0,
            wobble: vec![0.0, 0.6, -0.5, 0.3, -0.4],
            n: 0,
            audio_dead: false,
            meter_broken: false,
            log: vec![],
            rfpower: 0.5,
            swr: 1.2,
            split: false,
            xit: 0.0,
            error_when_keyed: None,
            hangup_when_keyed: false,
            ptt_sticks: false,
            ptt_commands: vec![],
            hangup_on_unkey: false,
            vfo_checked: false,
        }
    }

    fn signal_here(&self) -> Option<&Signal> {
        self.signals
            .iter()
            .find(|s| (s.hz - self.freq).abs() < 2000.0)
    }

    #[cfg(test)]
    pub fn handle(&mut self, cmd: &str) -> Reply {
        self.log.push(cmd.to_string());
        let mut w = cmd.split_whitespace();
        let (name, a, b) = (w.next().unwrap_or(""), w.next(), w.next());
        if self.ptt && self.hangup_when_keyed && name != "set_ptt" {
            // The client vanishes: this connection is closed. Later connections are served again.
            self.hangup_when_keyed = false;
            return Reply::Hangup;
        }
        if self.ptt && name == "get_level" && a != Some("?") {
            if let Some(code) = self.error_when_keyed {
                return err(code);
            }
        }
        match (name, a) {
            ("set_ptt", Some("0")) if self.hangup_on_unkey => {
                self.hangup_on_unkey = false;
                Reply::Hangup
            }
            ("set_ptt", Some(v)) => {
                self.ptt_commands.push(u8::from(v == "1"));
                if v == "1" {
                    self.ptt = true;
                } else if !self.ptt_sticks {
                    self.ptt = false;
                }
                ok(&[])
            }
            ("get_level", Some("RFPOWER")) => ok(&[&format!("{}", self.rfpower)]),
            ("set_level", Some("RFPOWER")) => {
                self.rfpower = b.and_then(|x| x.parse().ok()).unwrap_or(self.rfpower);
                ok(&[])
            }
            ("get_level", Some("SWR")) => {
                ok(&[&format!("{}", if self.ptt { self.swr } else { 0.0 })])
            }
            ("get_level", Some("ALC")) => ok(&[if self.ptt { "0.3" } else { "0" }]),
            ("get_level", Some("RFPOWER_METER_WATTS")) => ok(&[&format!(
                "{}",
                if self.ptt { self.rfpower * 100.0 } else { 0.0 }
            )]),
            ("get_level", Some("TEMP_METER")) => err(-11),
            ("get_level", Some("RFPOWER_METER")) => err(-11),
            ("mW2power", Some(mw)) => ok(&[&format!(
                "Power [0.0..1.0]: {:.6}",
                (mw.parse::<f64>().unwrap_or(0.0) / 100_000.0).min(1.0)
            )]),
            ("power2mW", Some(f)) => ok(&[&format!(
                "Power mW: {:.0}",
                f.parse::<f64>().unwrap_or(0.0) * 100_000.0
            )]),
            ("get_split_vfo", _) => {
                ok(&[&format!("Split: {}", u8::from(self.split)), "TX VFO: VFOB"])
            }
            ("get_xit", _) => ok(&[&format!("XIT: {}", self.xit)]),
            ("get_freq", _) => ok(&[&format!("Frequency: {}", self.freq)]),
            ("set_freq", Some(f)) => {
                self.freq = f.parse().unwrap_or(self.freq);
                ok(&[])
            }
            ("get_mode", _) => ok(&[
                &format!("Mode: {}", self.mode),
                &format!("Passband: {}", self.width),
            ]),
            ("set_mode", Some(m)) => {
                self.mode = m.to_string();
                self.width = b.and_then(|x| x.parse().ok()).unwrap_or(self.width);
                ok(&[])
            }
            ("get_ptt", _) => ok(&[&format!("PTT: {}", u8::from(self.ptt))]),
            ("get_dcd", _) => ok(&[&format!(
                "DCD: {}",
                u8::from(self.signal_here().is_some_and(|s| s.db > 20.0))
            )]),
            ("get_level", Some("STRENGTH")) => {
                if self.meter_broken {
                    return err(-6);
                }
                self.n += 1;
                let wob = self.wobble[self.n % self.wobble.len()];
                let sig = self.signal_here().map_or(0.0, |s| s.db);
                ok(&[&format!("{}", self.floor_db + sig + wob)])
            }
            ("get_level", Some("AF")) => ok(&[&format!("{}", self.af)]),
            ("get_level", Some("PREAMP")) => ok(&[&format!("{}", self.preamp)]),
            ("get_level", Some("ATT")) => ok(&[&format!("{}", self.att)]),
            ("set_level", Some(n)) => {
                let v = b.and_then(|x| x.parse().ok()).unwrap_or(0.0);
                match n {
                    "AF" => self.af = v,
                    "PREAMP" => self.preamp = v,
                    "ATT" => self.att = v,
                    _ => {}
                }
                ok(&[])
            }
            ("chk_vfo", _) => {
                self.vfo_checked = true;
                Reply::Raw("ChkVFO: 0\n".into())
            }
            ("dump_state", _) => {
                let mut v = vec![
                    "1",
                    "1",
                    "0",
                    "100000.000000 30000000.000000 0x1ff -1 -1 0x17e00007 0xf",
                    "0 0 0 0 0 0 0",
                    "7000000.000000 7300000.000000 0x1ff 1000 100000 0x17e00007 0xf",
                    "0 0 0 0 0 0 0",
                    "0 0",
                ];
                if self.vfo_checked {
                    v.extend([
                        "vfo_ops=0x7ffffff",
                        "level_gran=0=0,0,0;12=0.05,1,0.00195695;13=0,0,0;",
                        "done",
                    ]);
                }
                ok(&v)
            }
            _ => err(-11),
        }
    }

    /// Audio for what the rig is tuned to now.
    #[cfg(test)]
    pub fn audio(&self, seconds: f64, rate: u32) -> Vec<f32> {
        use crate::audio::analysis::synth::{add, noise, sine};
        let n = (seconds * f64::from(rate)) as usize;
        if self.audio_dead {
            return vec![0.0; n];
        }
        let base = noise(rate, seconds, 0.01, 17);
        match self.signal_here().map(|s| s.audio) {
            Some("wwv") => {
                let mut ticks = vec![0.0f32; n];
                let burst = sine(rate, 0.005, 1000.0, 0.3);
                let mut t = 0.2;
                while t + 0.005 < seconds {
                    let at = (t * f64::from(rate)) as usize;
                    for (i, x) in burst.iter().enumerate() {
                        ticks[at + i] += *x;
                    }
                    t += 1.0;
                }
                add(&add(&base, &ticks), &sine(rate, seconds, 500.0, 0.04))
            }
            Some("carrier") => add(&base, &sine(rate, seconds, 1500.0, 0.05)),
            Some("voice") => noise(rate, seconds, 0.08, 19),
            _ => base,
        }
    }
}

/// The audio of a `Sim`, as an `AudioSource`.
#[cfg(test)]
pub struct SimAudio(pub Arc<Mutex<Sim>>);

#[cfg(test)]
impl super::audio_in::AudioSource for SimAudio {
    fn capture(&self, seconds: f64) -> Result<Vec<f32>, String> {
        Ok(self.0.lock().unwrap().audio(seconds, super::audio_in::RATE))
    }

    fn rate(&self) -> u32 {
        super::audio_in::RATE
    }

    fn describe(&self) -> String {
        "simulated audio input".into()
    }
}

/// Serves a `Sim` as a fake rigctld and returns it with the audio source that follows it.
#[cfg(test)]
pub fn serve_sim(sim: Sim) -> (FakeRigctld, Arc<Mutex<Sim>>) {
    let shared = Arc::new(Mutex::new(sim));
    let s = shared.clone();
    (
        FakeRigctld::serve(move |c| s.lock().unwrap().handle(c)),
        shared,
    )
}

/// Starts the watchdog as a thread running the production `guard::run`; a test binary is not `bench_mcp`, so the process itself is
/// tried in `tests/hamlib_guard.rs`. The events say what the watchdog did.
#[cfg(test)]
pub struct ThreadSpawner {
    pub events: Arc<Mutex<Vec<String>>>,
}

#[cfg(test)]
struct Armed(std::sync::mpsc::Sender<()>);

#[cfg(test)]
impl std::io::Write for Armed {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let _ = self.0.send(());
        Ok(b.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
struct Report(Arc<Mutex<Vec<String>>>);

#[cfg(test)]
impl std::io::Write for Report {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap()
            .push(String::from_utf8_lossy(b).trim().to_string());
        Ok(b.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
pub struct ThreadWatch {
    words: Option<std::sync::mpsc::Sender<super::guard::Word>>,
    handle: Option<std::thread::JoinHandle<i32>>,
    events: Arc<Mutex<Vec<String>>>,
}

#[cfg(test)]
impl super::guard::Spawner for ThreadSpawner {
    fn spawn(
        &self,
        host: &str,
        port: u16,
        deadline: std::time::Duration,
        min_power: Option<f64>,
        _log: &std::path::Path,
    ) -> Result<Box<dyn super::guard::Watch>, String> {
        let (words, rx) = std::sync::mpsc::channel();
        let (armed_tx, armed_rx) = std::sync::mpsc::channel();
        let events = self.events.clone();
        let (host, ev) = (host.to_string(), events.clone());
        let handle = std::thread::spawn(move || {
            super::guard::run(
                &host,
                port,
                deadline,
                min_power,
                &rx,
                &mut Armed(armed_tx),
                &mut Report(ev),
            )
        });
        armed_rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| "the test watchdog did not arm".to_string())?;
        events.lock().unwrap().push("armed".into());
        Ok(Box::new(ThreadWatch {
            words: Some(words),
            handle: Some(handle),
            events,
        }))
    }
}

#[cfg(test)]
impl super::guard::Watch for ThreadWatch {
    fn stand_down(mut self: Box<Self>) -> Result<(), String> {
        if let Some(w) = self.words.take() {
            let _ = w.send(super::guard::Word::Line("done".into()));
        }
        let code = self
            .handle
            .take()
            .map(|h| h.join().unwrap_or(99))
            .unwrap_or(99);
        self.events
            .lock()
            .unwrap()
            .push(format!("stood down ({code})"));
        Ok(())
    }

    fn leave_to_guard(mut self: Box<Self>) {
        self.events.lock().unwrap().push("left to guard".into());
        drop(self.words.take());
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Stands in for the sound tools' alert: no test makes sound. It records the message it was asked to give.
#[cfg(test)]
pub struct RecordingAlert {
    pub sent: Mutex<Vec<String>>,
    pub audible: bool,
}

#[cfg(test)]
impl super::txtools::Alerter for RecordingAlert {
    fn alert(&self, message: &str) -> Result<Value, String> {
        self.sent.lock().unwrap().push(message.to_string());
        Ok(
            json!({"audible": self.audible, "text_to_send": message, "error": if self.audible { Value::Null } else { json!("no sound in a test") }}),
        )
    }
}

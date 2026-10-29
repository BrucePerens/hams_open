// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test helpers for the hamlib module: a fake rigctld on a local TCP socket, for the faults a real one rarely produces (silence, a
//! closed connection, error codes, a stuck PTT, scripted S-meter readings). The happy paths use the real `rigctld -m 1` (Hamlib's
//! dummy rig), started by the module's own managed-daemon code on a free localhost port.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// What the far end does with one command (given without the `+\` prefix).
pub enum Reply {
    /// Lines to send after the echo line, and the return code.
    Rprt(Vec<String>, i32),
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

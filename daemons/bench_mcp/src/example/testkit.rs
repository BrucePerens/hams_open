// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test helpers for the example instrument: a fake device on a local TCP socket, the pattern the scope tests use
//! (`src/scope.rs`, `fake_scope`). The code under test opens it with the production `Link::tcp`, so the whole path
//! (connect, timeouts, line handling, the dialect) is the production one; only the far end is fake.
//!
//! `FakeDevice::from_transcript` replays a recorded, scrubbed session (`tests/example_fixtures/*.txt`);
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

/// A simulator of the example dialect, with fault switches.
#[derive(Default)]
pub struct Sim {
    pub on: bool,
    pub level: f64,
    /// The meter answers this text instead of a number.
    pub meter_reply: Option<String>,
    /// `OUTP OFF` is accepted but the output stays on.
    pub stuck_on: bool,
    /// The connection is closed when this command arrives.
    pub hangup_on: Option<String>,
    /// The first this many queries get no answer.
    pub silent_queries: u32,
}

impl Sim {
    pub fn dropping_first_replies(n: u32) -> Sim {
        Sim {
            silent_queries: n,
            ..Sim::default()
        }
    }

    fn handle(&mut self, c: &str) -> Reply {
        if self.hangup_on.as_deref() == Some(c) {
            return Reply::Hangup;
        }
        if c.ends_with('?') && self.silent_queries > 0 {
            self.silent_queries -= 1;
            return Reply::Silent;
        }
        let text = |s: String| Reply::Text(s);
        match c {
            "*IDN?" => text("ACME,EXAMPLE-1000,SN-0000,1.0.0".into()),
            "OUTP?" => text(if self.on { "ON" } else { "OFF" }.into()),
            "OUTP ON" => {
                self.on = true;
                Reply::Silent
            }
            "OUTP OFF" => {
                self.on = self.stuck_on;
                Reply::Silent
            }
            "LEV?" => text(format!("{:.3}", self.level)),
            "MEAS:VOLT?" => text(
                self.meter_reply
                    .clone()
                    .unwrap_or_else(|| format!("{:.3}", if self.on { self.level } else { 0.0 })),
            ),
            _ => {
                if let Some(v) = c.strip_prefix("LEV ") {
                    self.level = v.parse().unwrap_or(0.0);
                }
                Reply::Silent
            }
        }
    }
}

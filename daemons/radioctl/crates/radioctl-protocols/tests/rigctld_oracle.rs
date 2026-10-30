// SPDX-License-Identifier: AGPL-3.0-or-later
//! Differential tests: Hamlib's `rigctld` as an oracle, run against radioctl's virtual rigs.
//!
//! For each rig the same scenario is carried out twice on two fresh virtual rigs: once by Hamlib
//! (the `rigctld` binary is started on the slave side of a pseudo-terminal whose master is a
//! virtual rig, and driven over its TCP text protocol), once by radioctl's engine for that
//! family. The values each side reads back, and the final state of the two virtual rigs, must
//! agree. The `rigctld` binary is only invoked; nothing of Hamlib is copied. The bytes Hamlib sent
//! are recorded, so the harness also shows which of Hamlib's commands our virtual rig does not
//! understand.
//!
//! Needs the `rigctld` binary (`RIGCTLD=/path` to choose one) and the cargo feature
//! `hamlib-oracle`; with the feature on and no binary the tests fail loudly rather than skip.
//! Everything is virtual: no radio, and nothing is ever keyed.
#![cfg(feature = "hamlib-oracle")]
#![allow(clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine};
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Level, Mode, Vfo};
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::virtual_rig::{RigState, VirtualRig};
use serialport::{SerialPort, TTYPort};

const HAMLIB_TS2000: u32 = 2014;
const HAMLIB_KX3: u32 = 2045;
const HAMLIB_IC7300: u32 = 3073;

fn kx3() -> RigDescription {
    rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))
        .expect("kx3 file")
        .description
}

/// Hamlib's name for a mode.
fn hamlib_mode(mode: Mode) -> &'static str {
    match mode {
        Mode::Usb => "USB",
        Mode::Lsb => "LSB",
        Mode::Cw => "CW",
        Mode::CwR => "CWR",
        Mode::Am => "AM",
        Mode::Fm => "FM",
        Mode::Wfm => "WFM",
        Mode::Rtty => "RTTY",
        Mode::RttyR => "RTTYR",
        Mode::Data => "PKTUSB",
        Mode::DataR => "PKTLSB",
    }
}

/// The mode a Hamlib name stands for, when we have one.
fn from_hamlib_mode(name: &str) -> Option<Mode> {
    [
        Mode::Usb,
        Mode::Lsb,
        Mode::Cw,
        Mode::CwR,
        Mode::Am,
        Mode::Fm,
        Mode::Wfm,
        Mode::Rtty,
        Mode::RttyR,
        Mode::Data,
        Mode::DataR,
    ]
    .into_iter()
    .find(|m| hamlib_mode(*m) == name)
}

/// What one side reports for one step.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reading {
    Done,
    Frequency(u64),
    Mode(Mode),
    Ptt(bool),
    Number(i64),
    Failed,
    NoAnswer,
}

/// A virtual rig on the master side of a pty, answering whoever opens the slave.
struct RadioOnPty {
    rig: Arc<Mutex<VirtualRig>>,
    wire: Arc<Mutex<Vec<u8>>>,
    refused: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    slave: String,
}

impl RadioOnPty {
    fn start(desc: &RigDescription) -> RadioOnPty {
        let (mut master, slave) = TTYPort::pair().expect("pty pair");
        let slave_path = slave.name().expect("slave name");
        drop(slave);
        let rig = Arc::new(Mutex::new(
            VirtualRig::from_description(desc).expect("virtual rig"),
        ));
        let wire = Arc::new(Mutex::new(Vec::new()));
        let refused = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (rig_t, wire_t, refused_t, stop_t) = (
            Arc::clone(&rig),
            Arc::clone(&wire),
            Arc::clone(&refused),
            Arc::clone(&stop),
        );
        let thread = thread::spawn(move || {
            master
                .set_timeout(Duration::from_millis(20))
                .expect("timeout");
            let started = Instant::now();
            let mut buffer = [0u8; 256];
            while !stop_t.load(Ordering::Acquire) {
                let Ok(n) = master.read(&mut buffer) else {
                    continue;
                };
                let bytes = buffer.get(..n).unwrap_or(&[]);
                wire_t.lock().expect("lock").extend_from_slice(bytes);
                let reply = rig_t.lock().expect("lock").feed(bytes, started.elapsed());
                if reply == b"?;" || reply.windows(2).any(|w| w == [0xFE, 0xFA]) {
                    refused_t
                        .lock()
                        .expect("lock")
                        .push(String::from_utf8_lossy(bytes).into_owned());
                }
                if !reply.is_empty() {
                    let _ = master.write_all(&reply);
                }
            }
        });
        RadioOnPty {
            rig,
            wire,
            refused,
            stop,
            thread: Some(thread),
            slave: slave_path,
        }
    }

    fn state(&self) -> RigState {
        self.rig.lock().expect("lock").state().clone()
    }
}

impl Drop for RadioOnPty {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A running `rigctld` and a connection to it.
struct Rigctld {
    child: Child,
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

fn rigctld_binary() -> String {
    std::env::var("RIGCTLD").unwrap_or_else(|_| "rigctld".to_string())
}

impl Rigctld {
    fn start(model: u32, slave: &str, baud: u32) -> Rigctld {
        let port = {
            let probe = TcpListener::bind("127.0.0.1:0").expect("free port");
            probe.local_addr().expect("addr").port()
        };
        let child = Command::new(rigctld_binary())
            .args([
                "-m",
                &model.to_string(),
                "-r",
                slave,
                "-s",
                &baud.to_string(),
            ])
            .args(["-T", "127.0.0.1", "-t", &port.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| {
                panic!(
                    "cannot start {:?} ({e}); install Hamlib or set RIGCTLD=/path/to/rigctld",
                    rigctld_binary()
                )
            });
        let started = Instant::now();
        let stream = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => break stream,
                Err(_) if started.elapsed() < Duration::from_secs(10) => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => panic!("rigctld did not listen on {port}: {e}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(6)))
            .expect("timeout");
        let writer = stream.try_clone().expect("clone");
        Rigctld {
            child,
            reader: BufReader::new(stream),
            writer,
        }
    }

    /// Sends one command in rigctld's extended form and returns its `key: value` lines and the
    /// RPRT code.
    fn call(&mut self, command: &str) -> Result<(Vec<(String, String)>, i32), String> {
        writeln!(self.writer, "+{command}").map_err(|e| format!("send: {e}"))?;
        let mut fields = Vec::new();
        loop {
            let mut line = String::new();
            let n = self
                .reader
                .read_line(&mut line)
                .map_err(|e| format!("no answer to {command:?}: {e}"))?;
            if n == 0 {
                return Err("rigctld closed the connection".to_string());
            }
            let line = line.trim_end();
            if let Some(code) = line.strip_prefix("RPRT ") {
                let code = code.trim().parse().map_err(|e| format!("RPRT: {e}"))?;
                return Ok((fields, code));
            }
            match line.split_once(':') {
                Some((key, value)) => {
                    fields.push((key.trim().to_string(), value.trim().to_string()));
                }
                None => fields.push((String::new(), line.trim().to_string())),
            }
        }
    }
}

impl Drop for Rigctld {
    fn drop(&mut self) {
        let _ = writeln!(self.writer, "q");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// The scenario step as Hamlib sees it.
fn through_hamlib(oracle: &mut Rigctld, op: &Op) -> Reading {
    let answered = match op {
        Op::SetFrequency(Vfo::A, hz) => oracle.call(&format!("F {}", hz.0)),
        Op::SetMode(mode) => oracle.call(&format!("M {} 0", hamlib_mode(*mode))),
        Op::GetFrequency(Vfo::A) => oracle.call("f"),
        Op::GetMode => oracle.call("m"),
        Op::GetPtt => oracle.call("t"),
        Op::SetRitOffset(hz) => oracle.call(&format!("J {hz}")),
        Op::GetRitOffset => oracle.call("j"),
        Op::SetLevel(Level::KeyerSpeed, wpm) => oracle.call(&format!("L KEYSPD {wpm}")),
        Op::GetLevel(Level::KeyerSpeed) => oracle.call("l KEYSPD"),
        other => panic!("the harness has no Hamlib command for {other:?}"),
    };
    let Ok((fields, code)) = answered else {
        return Reading::NoAnswer;
    };
    if code != 0 {
        return Reading::Failed;
    }
    match op {
        Op::GetFrequency(_) => field(&fields, "Frequency")
            .and_then(|v| v.parse().ok())
            .map_or(Reading::Failed, Reading::Frequency),
        Op::GetMode => field(&fields, "Mode")
            .and_then(from_hamlib_mode)
            .map_or(Reading::Failed, Reading::Mode),
        Op::GetPtt => field(&fields, "PTT")
            .map(|v| Reading::Ptt(v != "0"))
            .unwrap_or(Reading::Failed),
        Op::GetRitOffset => field(&fields, "RIT")
            .and_then(|v| v.parse().ok())
            .map_or(Reading::Failed, Reading::Number),
        Op::GetLevel(_) => field(&fields, "")
            .and_then(|v| v.parse().ok())
            .map_or(Reading::Failed, Reading::Number),
        _ => Reading::Done,
    }
}

/// The scenario step as radioctl's engine sees it, on a virtual rig.
fn through_radioctl(engine: &mut dyn ProtocolEngine, rig: &mut VirtualRig, op: &Op) -> Reading {
    let now = Duration::from_secs(1);
    let Ok(request) = engine.encode(op) else {
        return Reading::Failed;
    };
    let answer = rig.feed(&request.bytes, now);
    let mut reading = Reading::Done;
    for item in engine.feed(&answer, now) {
        match item {
            Inbound::Observation(Observation::Frequency(_, hz)) => {
                reading = Reading::Frequency(hz.0);
            }
            Inbound::Observation(Observation::Mode(mode)) => reading = Reading::Mode(mode),
            Inbound::Observation(Observation::Ptt(on)) => reading = Reading::Ptt(on),
            Inbound::Observation(Observation::RitOffset(hz)) => {
                reading = Reading::Number(i64::from(hz));
            }
            Inbound::Observation(Observation::Level(_, value)) => {
                reading = Reading::Number(i64::from(value));
            }
            Inbound::Nak => reading = Reading::Failed,
            _ => {}
        }
    }
    reading
}

/// One disagreement.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Divergence {
    step: String,
    hamlib: String,
    radioctl: String,
}

/// Runs `scenario` on both sides and returns the disagreements plus the commands Hamlib sent that
/// the virtual rig refused.
fn compare(
    model: u32,
    desc: &RigDescription,
    baud: u32,
    scenario: &[Op],
) -> (Vec<Divergence>, Vec<String>, String) {
    let radio = RadioOnPty::start(desc);
    let mut oracle = Rigctld::start(model, &radio.slave, baud);
    let mut mine_rig = VirtualRig::from_description(desc).expect("virtual rig");
    let mut engine = engine_for(desc).expect("engine");
    let mut divergences = Vec::new();
    let mut gone = false;
    for op in scenario {
        let theirs = if gone {
            Reading::NoAnswer
        } else {
            through_hamlib(&mut oracle, op)
        };
        gone = gone || theirs == Reading::NoAnswer;
        let ours = through_radioctl(engine.as_mut(), &mut mine_rig, op);
        if theirs != ours {
            divergences.push(Divergence {
                step: format!("{op:?}"),
                hamlib: format!("{theirs:?}"),
                radioctl: format!("{ours:?}"),
            });
        }
    }
    let (state_a, state_b) = (radio.state(), mine_rig.state().clone());
    for (name, a, b) in [
        (
            "final freq_a",
            state_a.freq_a.0.to_string(),
            state_b.freq_a.0.to_string(),
        ),
        (
            "final mode",
            format!("{:?}", state_a.mode),
            format!("{:?}", state_b.mode),
        ),
        (
            "final ptt",
            state_a.ptt.to_string(),
            state_b.ptt.to_string(),
        ),
    ] {
        if a != b {
            divergences.push(Divergence {
                step: name.to_string(),
                hamlib: a,
                radioctl: b,
            });
        }
    }
    let refused = radio.refused.lock().expect("lock").clone();
    let wire = String::from_utf8_lossy(&radio.wire.lock().expect("lock")).into_owned();
    (divergences, refused, wire)
}

fn scenario() -> Vec<Op> {
    vec![
        Op::GetFrequency(Vfo::A),
        Op::GetMode,
        Op::SetFrequency(Vfo::A, Hz(7_074_000)),
        Op::GetFrequency(Vfo::A),
        Op::SetMode(Mode::Lsb),
        Op::GetMode,
        Op::SetFrequency(Vfo::A, Hz(14_060_000)),
        Op::SetMode(Mode::Cw),
        Op::GetFrequency(Vfo::A),
        Op::GetMode,
    ]
}

fn report(name: &str, divergences: &[Divergence], refused: &[String], wire: &str) {
    eprintln!("== {name}: {} divergences", divergences.len());
    for d in divergences {
        eprintln!(
            "   {} : hamlib {} / radioctl {}",
            d.step, d.hamlib, d.radioctl
        );
    }
    eprintln!("   refused by the virtual rig: {refused:?}");
    eprintln!("   hamlib wire: {wire:?}");
}

// Tests [@ANCHOR: radioctl_elecraft_engine_encode]
// Tests [@ANCHOR: radioctl_elecraft_engine_feed]
// Tests [@ANCHOR: radioctl_virtual_elecraft_command]
#[test]
fn kx3_agrees_with_hamlib_on_frequency_mode_ptt_rit_and_keyer_speed() {
    let desc = kx3();
    let mut steps = scenario();
    steps.extend([
        Op::GetPtt,
        Op::SetRitOffset(-120),
        Op::GetRitOffset,
        Op::SetLevel(Level::KeyerSpeed, 25),
        Op::GetLevel(Level::KeyerSpeed),
    ]);
    let (divergences, refused, wire) = compare(HAMLIB_KX3, &desc, 38400, &steps);
    report("KX3", &divergences, &refused, &wire);
    assert!(divergences.is_empty(), "{divergences:#?}");
}

// Tests [@ANCHOR: radioctl_kenwood_engine_encode]
// Tests [@ANCHOR: radioctl_virtual_kenwood_command]
#[test]
fn ts2000_known_gap_hamlib_needs_the_composite_if_answer() {
    // Hamlib's TS-2000 backend reads the frequency from `IF;`. The virtual TS-2000 refuses `IF;`
    // because its field layout is not verified (see kenwood.rs), so every Hamlib frequency
    // read and write fails while radioctl's own engine, which uses FA, succeeds. This test
    // records exactly that gap: it fails if the gap changes in either direction.
    let desc = RigDescription::ts2000();
    let (divergences, refused, wire) = compare(HAMLIB_TS2000, &desc, 57600, &scenario());
    report("TS-2000", &divergences, &refused, &wire);
    let steps: Vec<&str> = divergences.iter().map(|d| d.step.as_str()).collect();
    assert_eq!(
        steps,
        vec![
            "GetFrequency(A)",
            "SetFrequency(A, Hz(7074000))",
            "GetFrequency(A)",
            "SetFrequency(A, Hz(14060000))",
            "GetFrequency(A)",
            "final freq_a",
        ],
        "{divergences:#?}"
    );
    assert!(divergences.iter().all(|d| d.hamlib != d.radioctl));
    assert!(refused.iter().any(|c| c == "IF;"), "{refused:?}");
}

// Tests [@ANCHOR: radioctl_civ_engine_encode]
// Tests [@ANCHOR: radioctl_virtual_civ_dispatch]
#[test]
fn ic7300_agrees_with_hamlib_on_frequency_and_mode() {
    let desc = RigDescription::ic7300();
    let (divergences, refused, wire) = compare(HAMLIB_IC7300, &desc, 19200, &scenario());
    report("IC-7300", &divergences, &refused, &wire);
    assert!(divergences.is_empty(), "{divergences:#?}");
}

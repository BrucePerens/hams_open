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
use radioctl_core::units::{Hz, Level, Meter, Mode, Vfo};
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::virtual_rig::{RigState, VirtualRig};
use serialport::{SerialPort, TTYPort};

const HAMLIB_TS2000: u32 = 2014;
const HAMLIB_TS590S: u32 = 2031;
const HAMLIB_TS590SG: u32 = 2037;
const HAMLIB_TS890S: u32 = 2041;
const HAMLIB_KX3: u32 = 2045;
const HAMLIB_IC7300: u32 = 3073;
const HAMLIB_IC7610: u32 = 3078;
const HAMLIB_IC9700: u32 = 3081;
const HAMLIB_IC705: u32 = 3085;

/// A shipped rig description by file name (without `.toml`).
fn shipped(name: &str) -> RigDescription {
    let path = format!("{}/../../rigs/{name}.toml", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    rigfile::parse(&text).expect("rig file").description
}

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
                if reply == b"?;" || reply.ends_with(&[0xFA, 0xFD]) {
                    let shown = if bytes.contains(&0xFE) {
                        bytes
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    } else {
                        String::from_utf8_lossy(bytes).into_owned()
                    };
                    refused_t.lock().expect("lock").push(shown);
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

/// A spawned `rigctld` that is killed and reaped when this is dropped, on every path out of
/// the harness: a panic while waiting for it to listen, a failed assertion, a normal return.
/// It is created the instant the process exists, before anything that can fail. `rigctld`
/// forks nothing, so killing the one process is the whole job; it is left in the test's own
/// process group on purpose, so that a Ctrl-C to `cargo test` (SIGINT to the foreground group,
/// which runs no destructor) reaches it too.
struct Reaped(Child);

impl Reaped {
    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The pids of every `rigctld` process whose command line names `slave` (so one started by
/// another test, or another session, is not counted): what must be empty once a harness on
/// that pty is gone.
fn rigctld_pids_on(slave: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let cmdline = std::fs::read(entry.path().join("cmdline")).ok()?;
            let args: Vec<&[u8]> = cmdline.split(|b| *b == 0).collect();
            let is_rigctld = args.first().is_some_and(|arg0| arg0.ends_with(b"rigctld"));
            let on_slave = args.contains(&slave.as_bytes());
            (is_rigctld && on_slave).then_some(pid)
        })
        .collect()
}

/// A running `rigctld` and a connection to it.
struct Rigctld {
    child: Reaped,
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

/// Whether process `pid` holds a socket listening on 127.0.0.1:`port` (read from the kernel's
/// own tables: `/proc/net/tcp` for the listeners, `/proc/<pid>/fd` for who holds them).
fn listener_owned_by(pid: u32, port: u16) -> bool {
    let Ok(table) = std::fs::read_to_string("/proc/net/tcp") else {
        return false;
    };
    let local = format!("0100007F:{port:04X}");
    let inodes: Vec<String> = table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let columns: Vec<&str> = line.split_whitespace().collect();
            // Column 1 is the local address, column 3 the state (0A is LISTEN), column 9 the inode.
            let listening =
                columns.get(1) == Some(&local.as_str()) && columns.get(3) == Some(&"0A");
            listening.then(|| columns.get(9).map(|inode| format!("socket:[{inode}]")))?
        })
        .collect();
    let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return false;
    };
    entries.flatten().any(|entry| {
        std::fs::read_link(entry.path())
            .is_ok_and(|target| inodes.iter().any(|inode| target.to_str() == Some(inode)))
    })
}

fn rigctld_binary() -> String {
    std::env::var("RIGCTLD").unwrap_or_else(|_| "rigctld".to_string())
}

/// How long one answer from `rigctld` may take (see the note at its use).
const ANSWER_TIMEOUT: Duration = Duration::from_secs(40);

impl Rigctld {
    fn start(model: u32, slave: &str, baud: u32) -> Rigctld {
        // The port is found by binding to 0 and letting go, so two harnesses (the nine tests of
        // this file run in parallel, and so does radioctld's suite) can be handed the same number;
        // the second rigctld then fails to bind and exits, and a connect to that port reaches the
        // FIRST harness's rigctld, on another radio's pty: every step "NoAnswer", an empty wire
        // (seen once in a whole-workspace run). So: if the child is gone before or right after
        // the connect, the connection was not to it; try again on another port.
        for _attempt in 0..8 {
            if let Some(started) = Self::try_start(model, slave, baud) {
                return started;
            }
        }
        panic!("rigctld could not be started on a port of its own after eight tries");
    }

    /// One attempt: `None` when the spawned rigctld exited (its port was taken) before it
    /// could be reached.
    fn try_start(model: u32, slave: &str, baud: u32) -> Option<Rigctld> {
        let port = {
            let probe = TcpListener::bind("127.0.0.1:0").expect("free port");
            probe.local_addr().expect("addr").port()
        };
        let mut child = Reaped(
            Command::new(rigctld_binary())
                .args([
                    "-m",
                    &model.to_string(),
                    "-r",
                    slave,
                    "-s",
                    &baud.to_string(),
                ])
                .args(["-T", "127.0.0.1", "-t", &port.to_string()])
                // Hamlib's cache expires after 500 ms by default, and what Hamlib then puts on the
                // wire (which VFO it believes is selected, whether it re-reads the other VFO) depends
                // on whether a step came before or after the expiry: on a busy machine the same
                // scenario produced a different wire (seen: the TS-2000 split step read `FA;` where
                // it normally reads `FB;`). With the cache off every answer is read from the radio,
                // so the wire does not depend on the clock.
                .args(["-C", "cache_timeout=0"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap_or_else(|e| {
                    panic!(
                        "cannot start {:?} ({e}); install Hamlib or set RIGCTLD=/path/to/rigctld",
                        rigctld_binary()
                    )
                }),
        );
        // Readiness is a fact about the kernel, not about a connect: wait until THIS child owns
        // the listening socket on `port`, and only then connect. A connect used as the probe
        // can succeed against a listening socket that is not rigctld's. `TcpListener::bind`
        // also listens, so the probe above is a listener, and `Command::spawn` returns as soon
        // as the new process has left the parent's memory, a little BEFORE its exec has closed
        // the inherited descriptors: a child forked by a concurrent harness while the probe is
        // open holds a copy of the listening socket for that moment. The connect then completes
        // (seen: 20 ms after the spawn, long before rigctld can have opened the radio), and when
        // the stray child's exec closes its copy the listener dies and the connection is reset:
        // every step "NoAnswer", an empty wire, in about one run in ten under load (reproduced:
        // 1169 of 3200 `sleep` children, spawned from 8 threads, still held another thread's
        // listening socket when `spawn` returned).
        let started = Instant::now();
        loop {
            if child.0.try_wait().ok().flatten().is_some() {
                return None;
            }
            if listener_owned_by(child.pid(), port) {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(45),
                "rigctld did not listen on {port} within 45 s"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to the rigctld");
        stream
            // Hamlib's FT-817-family and TS-890S backends probe at open with commands the virtual
            // rig refuses or does not know, each with its own retries, so the first answer can take
            // longer than 6 s when the machine is busy (seen: the FT-818 and TS-890S steps all
            // reading NoAnswer in a whole-workspace run, passing alone). A generous bound costs
            // time only when an answer really is missing.
            .set_read_timeout(Some(ANSWER_TIMEOUT))
            .expect("timeout");
        let writer = stream.try_clone().expect("clone");
        Some(Rigctld {
            child,
            reader: BufReader::new(stream),
            writer,
        })
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
        // A polite quit first; `Reaped` kills and reaps whatever is left.
        let _ = writeln!(self.writer, "q");
    }
}

// Tests the harness itself: no `rigctld` outlives it, on the normal path or a panicking one.
#[test]
fn no_rigctld_outlives_its_harness() {
    let desc = kx3();
    let radio = RadioOnPty::start(&desc);
    let slave = radio.slave.clone();
    let pid = {
        let oracle = Rigctld::start(HAMLIB_KX3, &slave, 38400);
        let pid = oracle.child.pid();
        assert_eq!(rigctld_pids_on(&slave), vec![pid]);
        pid
    };
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "rigctld {pid} was not reaped when its harness was dropped"
    );
    assert!(rigctld_pids_on(&slave).is_empty());
    // The panicking path: the guard runs during unwinding, before the test sees the panic.
    let seen = Arc::new(Mutex::new(None));
    let seen_inside = Arc::clone(&seen);
    let slave_inside = slave.clone();
    let outcome = std::panic::catch_unwind(move || {
        let oracle = Rigctld::start(HAMLIB_KX3, &slave_inside, 38400);
        *seen_inside.lock().expect("lock") = Some(oracle.child.pid());
        panic!("a failing assertion in a test");
    });
    assert!(outcome.is_err());
    let pid = seen.lock().expect("lock").expect("rigctld was started");
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "rigctld {pid} outlived a panicking test"
    );
    assert!(rigctld_pids_on(&slave).is_empty());
}

// Tests the harness itself: the readiness check names the process that holds a listening
// socket, so a listener that belongs to somebody else (a forked copy of another harness's probe)
// is never mistaken for rigctld's.
#[test]
fn a_listener_is_attributed_only_to_the_process_that_holds_it() {
    // The other process exists BEFORE the listener does, so it cannot hold a copy of it: a child
    // forked while a listener is open keeps a copy of it until its exec has closed the
    // descriptors, a little after `spawn` has returned (that window is what the check exists
    // to see through).
    let other = Reaped(
        Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .spawn()
            .expect("sleep"),
    );
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    assert!(listener_owned_by(std::process::id(), port));
    assert!(!listener_owned_by(other.pid(), port));
    drop(listener);
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
        Op::SetLevel(Level::AfGain, raw) => {
            oracle.call(&format!("L AF {:.6}", f64::from(*raw) / 255.0))
        }
        Op::GetLevel(Level::AfGain) => oracle.call("l AF"),
        Op::SetSplit(on) => oracle.call(if *on { "S 1 VFOB" } else { "S 0 VFOA" }),
        Op::GetSplit => oracle.call("s"),
        Op::SetFrequency(Vfo::B, hz) => oracle.call(&format!("I {}", hz.0)),
        Op::GetFrequency(Vfo::B) => oracle.call("i"),
        // Hamlib answers a meter as a calibrated float (its per-model tables); radioctl reports
        // the raw deflection. Only "answered" is compared: it proves the virtual rig's `15 1x`
        // replies satisfy Hamlib's read and radioctl's decode alike, nothing about the scale.
        Op::GetMeter(Meter::Swr) => oracle.call("l SWR"),
        Op::GetMeter(Meter::Alc) => oracle.call("l ALC"),
        Op::GetMeter(Meter::Comp) => oracle.call("l COMP_METER"),
        Op::GetMeter(Meter::Vd) => oracle.call("l VD_METER"),
        Op::GetMeter(Meter::Id) => oracle.call("l ID_METER"),
        other => panic!("the harness has no Hamlib command for {other:?}"),
    };
    let Ok((fields, code)) = answered else {
        return Reading::NoAnswer;
    };
    if code != 0 {
        return Reading::Failed;
    }
    match op {
        Op::GetSplit => field(&fields, "Split")
            .and_then(|v| v.parse().ok())
            .map_or(Reading::Failed, Reading::Number),
        Op::GetFrequency(Vfo::B) => field(&fields, "TX Frequency")
            .and_then(|v| v.parse().ok())
            .map_or(Reading::Failed, Reading::Frequency),
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
        Op::GetLevel(Level::AfGain) => field(&fields, "")
            .and_then(|v| v.parse::<f64>().ok())
            .map_or(Reading::Failed, |fraction| {
                Reading::Number((fraction * 255.0).round() as i64)
            }),
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
    let wanted = match request.reply {
        radioctl_core::engine::ReplyKind::Observation(field) => Some(field),
        _ => None,
    };
    let mut reading = Reading::Done;
    for item in engine.feed(&answer, now) {
        match item {
            // A composite answer (Kenwood IF) carries several fields; the step reads only the one
            // it asked for.
            Inbound::Observation(observation)
                if wanted.is_some_and(|field| observation.field() != field) => {}
            Inbound::Observation(Observation::Frequency(_, hz)) => {
                reading = Reading::Frequency(hz.0);
            }
            Inbound::Observation(Observation::Mode(mode)) => reading = Reading::Mode(mode),
            Inbound::Observation(Observation::Ptt(on)) => reading = Reading::Ptt(on),
            Inbound::Observation(Observation::Split(on)) => {
                reading = Reading::Number(i64::from(on));
            }
            Inbound::Observation(Observation::RitOffset(hz)) => {
                reading = Reading::Number(i64::from(hz));
            }
            Inbound::Observation(Observation::Level(_, value)) => {
                reading = Reading::Number(i64::from(value));
            }
            Inbound::Observation(Observation::Meter(..)) => reading = Reading::Done,
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
    let sent = radio.wire.lock().expect("lock").clone();
    let wire = if sent.contains(&0xFE) {
        // CI-V: hex, one frame per FD.
        sent.split_inclusive(|b| *b == 0xFD)
            .map(|frame| {
                frame
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join(" | ")
    } else {
        String::from_utf8_lossy(&sent).into_owned()
    };
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
// Tests [@ANCHOR: radioctl_kenwood_engine_decode_if]
// Tests [@ANCHOR: radioctl_virtual_kenwood_command]
// Tests [@ANCHOR: radioctl_virtual_kenwood_if]
#[test]
fn the_kenwood_files_against_hamlib_record_every_disagreement() {
    // The IF layout this resolves was the cause of the TS-2000 gap the previous version of this
    // test pinned (Hamlib reads the frequency from `IF;`, which the virtual rig refused).
    for (file, model) in [
        ("kenwood-ts-2000", HAMLIB_TS2000),
        ("kenwood-ts-590s", HAMLIB_TS590S),
        ("kenwood-ts-590sg", HAMLIB_TS590SG),
    ] {
        let desc = shipped(file);
        // No RIT offset steps: Hamlib answers `j` from its cache about half the time, and when it
        // reads `IF` its TS-2000 backend takes five digits at index 18 and drops the sign, so a
        // negative offset reads back positive (seen: -120 read as 120). Not pinned here.
        let steps: Vec<Op> = icom_scenario(7_074_000, 14_060_000, 14_025_000)
            .into_iter()
            .filter(|op| !matches!(op, Op::SetRitOffset(_) | Op::GetRitOffset))
            .collect();
        let (divergences, refused, wire) = compare(model, &desc, 57600, &steps);
        report(file, &divergences, &refused, &wire);
        let got: Vec<&str> = divergences.iter().map(|d| d.step.as_str()).collect();
        assert_eq!(
            got,
            known_kenwood_divergences(file),
            "{file}: {divergences:#?}"
        );
    }
}

/// The steps on which Hamlib and radioctl disagree for a Kenwood file.
///
/// The TS-2000 agrees on everything the harness expresses, which closes the `IF;` gap an earlier
/// version of this test pinned. On the TS-590S and TS-590SG two steps differ, both because Hamlib's
/// TS-590 backend does not follow the guide on the wire: it reads and writes AF gain with `AG;`
/// and `AG1;` where the guide requires `AG0;` and `AG0nnn;` (the strict virtual rig refuses both,
/// so Hamlib's set silently fails and it reads back the old 30), and after enabling split it
/// reads the "other" frequency with `FA;` where radioctl reads `FB;`. Hamlib also sends `SF0;`,
/// which the 590 guide does not list. Whether a real TS-590 accepts Hamlib's forms is unknown.
fn known_kenwood_divergences(file: &str) -> Vec<&'static str> {
    match file {
        "kenwood-ts-590s" | "kenwood-ts-590sg" => vec!["GetLevel(AfGain)", "GetFrequency(B)"],
        _ => Vec::new(),
    }
}

// Tests [@ANCHOR: radioctl_ts890_engine_encode]
// Tests [@ANCHOR: radioctl_ts890_engine_decode]
// Tests [@ANCHOR: radioctl_virtual_ts890_command]
#[test]
fn the_ts890_file_against_hamlib_records_every_disagreement() {
    let desc = shipped("kenwood-ts-890s");
    let steps: Vec<Op> = icom_scenario(7_074_000, 14_060_000, 14_025_000);
    let (divergences, refused, wire) = compare(HAMLIB_TS890S, &desc, 115_200, &steps);
    report("kenwood-ts-890s", &divergences, &refused, &wire);
    let got: Vec<&str> = divergences.iter().map(|d| d.step.as_str()).collect();
    assert_eq!(got, known_ts890_divergences(), "{divergences:#?}");
}

/// The steps on which Hamlib's TS-890S backend and radioctl disagree.
///
/// Hamlib's backend reads the frequency and mode with `IF;` and sets the frequency with `SF`,
/// neither of which is in Kenwood's TS-890S PC command guide (which has `FA`, `OM` and no `IF`;
/// the recorded bytes show both refused by the virtual rig). So every frequency and mode step
/// that passes through them diverges, and so do the final frequency and mode. Hamlib also put no
/// `AG` set on the wire (only reads), so its AF level stays at the starting 30; why is not
/// investigated. Whether a real TS-890S answers `IF;` is unknown (the guide does not list it); a
/// bench run would settle it, and radioctl follows the guide until then.
fn known_ts890_divergences() -> Vec<&'static str> {
    vec![
        "GetMode",
        "SetFrequency(A, Hz(7074000))",
        "GetFrequency(A)",
        "SetMode(Lsb)",
        "GetMode",
        "SetFrequency(A, Hz(14060000))",
        "SetMode(Cw)",
        "GetFrequency(A)",
        "GetMode",
        "GetLevel(AfGain)",
        "final freq_a",
        "final mode",
    ]
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

/// The scenario for the Icom rigs: the common one plus PTT read, RIT offset and AF gain, the
/// operations the harness can express through `rigctld` for CI-V.
fn icom_scenario(first: u64, second: u64, other: u64) -> Vec<Op> {
    vec![
        Op::GetFrequency(Vfo::A),
        Op::GetMode,
        Op::SetFrequency(Vfo::A, Hz(first)),
        Op::GetFrequency(Vfo::A),
        Op::SetMode(Mode::Lsb),
        Op::GetMode,
        Op::SetFrequency(Vfo::A, Hz(second)),
        Op::SetMode(Mode::Cw),
        Op::GetFrequency(Vfo::A),
        Op::GetMode,
        Op::GetPtt,
        Op::SetRitOffset(-120),
        Op::GetRitOffset,
        Op::SetLevel(Level::AfGain, 128),
        Op::GetLevel(Level::AfGain),
        Op::GetSplit,
        Op::SetSplit(true),
        Op::GetSplit,
        Op::SetFrequency(Vfo::B, Hz(other)),
        Op::GetFrequency(Vfo::B),
        Op::SetSplit(false),
        Op::GetSplit,
    ]
}

// Tests [@ANCHOR: radioctl_civ_engine_encode]
// Tests [@ANCHOR: radioctl_civ_engine_decode]
// Tests [@ANCHOR: radioctl_virtual_civ_dispatch]
// Tests [@ANCHOR: radioctl_virtual_civ_level]
#[test]
fn the_four_icom_files_against_hamlib_record_every_disagreement() {
    // Recorded, not hidden: whatever differs between Hamlib and radioctl on the shipped
    // descriptions is printed with `--nocapture` and pinned here so a change either way is seen.
    for (file, model) in [
        ("icom-ic-7300", HAMLIB_IC7300),
        ("icom-ic-705", HAMLIB_IC705),
        ("icom-ic-7610", HAMLIB_IC7610),
        ("icom-ic-9700", HAMLIB_IC9700),
    ] {
        let desc = shipped(file);
        // The IC-9700 covers 144, 430 and 1240 MHz only; the others are HF/50 MHz rigs.
        let mut steps = if file == "icom-ic-9700" {
            icom_scenario(144_300_000, 432_100_000, 145_500_000)
        } else {
            icom_scenario(7_074_000, 14_060_000, 14_025_000)
        };
        // Data mode (`M PKTUSB`, command 26 with the DATA flag) and the raw meters, on the
        // files that claim them.
        if desc.mode_code(Mode::Data).is_some() {
            steps.extend([
                Op::SetMode(Mode::Data),
                Op::GetMode,
                Op::SetMode(Mode::Usb),
                Op::GetMode,
            ]);
        }
        if desc
            .capabilities
            .has(radioctl_core::capability::Capability::ReadMeters)
        {
            steps.extend(Meter::ALL.map(Op::GetMeter));
        }
        let (divergences, refused, wire) = compare(model, &desc, 19200, &steps);
        report(file, &divergences, &refused, &wire);
        let steps: Vec<&str> = divergences.iter().map(|d| d.step.as_str()).collect();
        assert_eq!(
            steps,
            known_icom_divergences(file),
            "{file}: {divergences:#?}"
        );
    }
}

/// The steps on which Hamlib and radioctl disagree for an Icom file.
///
/// The IC-7300 and IC-705 agree on everything the harness can express (frequency, mode, PTT read,
/// RIT offset, AF gain, split, the other VFO's frequency). On the IC-7610 Hamlib's flow starts
/// with `07 C2` (dual watch), `07 D2` (band selection) and `07 D0` (select Main), which the
/// virtual rig refuses (the Main/Sub family of command 07 is not implemented), so Hamlib's split
/// and second-frequency steps fail there while radioctl's engine treats VFO B as "the other VFO"
/// (`25 01`). Which of the two matches a real IC-7610 is unknown: an open question, not a result.
/// On the IC-9700 split agrees, but setting the other VFO's frequency fails in Hamlib, which
/// sends `07 D0` (select Main) first, again refused by the virtual rig. (A first run used HF
/// frequencies on the 144 MHz rig, which Hamlib refused outright; the scenario now stays in band.)
fn known_icom_divergences(file: &str) -> Vec<&'static str> {
    match file {
        "icom-ic-7610" => vec![
            "SetSplit(true)",
            "GetSplit",
            "SetFrequency(B, Hz(14025000))",
            "GetFrequency(B)",
            "SetSplit(false)",
        ],
        "icom-ic-9700" => vec!["SetFrequency(B, Hz(145500000))", "GetFrequency(B)"],
        _ => Vec::new(),
    }
}

/// The Icom CI-V descriptions added after the first four, with Hamlib's model number for each.
const ICOM_BATCH_ONE: [(&str, u32); 19] = [
    ("icom-ic-7000", 3060),
    ("icom-ic-7100", 3070),
    ("icom-ic-7200", 3061),
    ("icom-ic-7410", 3067),
    ("icom-ic-7600", 3063),
    ("icom-ic-7700", 3062),
    ("icom-ic-7760", 3092),
    ("icom-ic-7800", 3056),
    ("icom-ic-7851", 3075),
    ("icom-ic-9100", 3068),
    ("icom-ic-r8600", 3079),
    ("icom-ic-r75", 3039),
    ("icom-ic-r9500", 3066),
    ("icom-ic-706", 3009),
    ("icom-ic-706mkii", 3010),
    ("icom-ic-706mkiig", 3011),
    ("icom-ic-718", 3013),
    ("icom-ic-746", 3023),
    ("icom-ic-746pro", 3046),
];

// Tests [@ANCHOR: radioctl_civ_engine_encode]
// Tests [@ANCHOR: radioctl_civ_engine_decode]
// Tests [@ANCHOR: radioctl_virtual_civ_dispatch]
// Tests [@ANCHOR: radioctl_virtual_civ_level]
#[test]
fn the_icom_batch_one_files_against_hamlib_record_every_disagreement() {
    // The same scenario as for the first four Icom files, cut down to what each description
    // claims (a receiver has no PTT or split, an IC-7000 no RIT), plus data mode and the raw
    // meters where the file lists them.
    let mut wrong = Vec::new();
    for (file, model) in ICOM_BATCH_ONE {
        let desc = shipped(file);
        let mut steps: Vec<Op> = icom_scenario(7_074_000, 14_060_000, 14_025_000)
            .into_iter()
            .filter(|op| desc.capabilities.has(op.required_capability()))
            .collect();
        if desc.mode_code(Mode::Data).is_some() {
            steps.extend([
                Op::SetMode(Mode::Data),
                Op::GetMode,
                Op::SetMode(Mode::Usb),
                Op::GetMode,
            ]);
        }
        if desc
            .capabilities
            .has(radioctl_core::capability::Capability::ReadMeters)
        {
            steps.extend(Meter::ALL.map(Op::GetMeter));
        }
        let (divergences, refused, wire) = compare(model, &desc, 19200, &steps);
        report(file, &divergences, &refused, &wire);
        let got: Vec<&str> = divergences.iter().map(|d| d.step.as_str()).collect();
        if got != known_icom_batch_divergences(file) {
            wrong.push(format!("{file}: {divergences:#?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The steps on which Hamlib and radioctl disagree for a file of the first Icom batch.
///
/// Twelve of the nineteen agree on everything the harness can express. The IC-7600, IC-7800 and
/// IC-7850/7851 are Main/Sub rigs like the IC-7610: Hamlib's split flow starts with `07 C2` (dual
/// watch), `07 D2` and `07 D0`, which the virtual rig refuses (the Main/Sub family of command 07
/// is not implemented), so Hamlib's split and second-frequency steps fail there while radioctl
/// treats VFO B as "the other VFO" (`25 01`). Which of the two matches a real radio is unknown:
/// an open question, not a result. The IC-7760 does the same without `07 C2`. On the IC-7700
/// every step agrees but the final mode of the virtual rig differs (Hamlib ends in USB where
/// radioctl ends in CW). The recorded Hamlib frames show no mode write at all (no `06` and no
/// `26 00`) between its two frequency writes, only `1A 06 00 00` (data mode off) and `07 00` /
/// `07 B0` (VFO and memory selection), so on this model's path Hamlib's mode set produced no mode
/// frame; why is an open question, kept visible here.
fn known_icom_batch_divergences(file: &str) -> Vec<&'static str> {
    let main_sub = vec![
        "SetSplit(true)",
        "GetSplit",
        "SetFrequency(B, Hz(14025000))",
        "GetFrequency(B)",
        "SetSplit(false)",
    ];
    match file {
        "icom-ic-7600" | "icom-ic-7800" | "icom-ic-7851" => main_sub,
        "icom-ic-7760" => main_sub[..4].to_vec(),
        "icom-ic-7700" => vec!["final mode"],
        _ => Vec::new(),
    }
}

// Tests [@ANCHOR: radioctl_civ_engine_encode]
// Tests [@ANCHOR: radioctl_civ_engine_decode]
// Tests [@ANCHOR: radioctl_virtual_civ_dispatch]
#[test]
fn the_xiegu_files_against_hamlib_record_every_disagreement() {
    // Only what the small Xiegu descriptions claim: frequency, mode and the PTT read.
    for (file, model) in [
        ("xiegu-g90", 3088),
        ("xiegu-x6100", 3087),
        ("xiegu-x108g", 3076),
        ("xiegu-x5105", 3089),
        ("xiegu-x6200", 3091),
    ] {
        let desc = shipped(file);
        let mut steps = scenario();
        steps.push(Op::GetPtt);
        let (divergences, refused, wire) = compare(model, &desc, 19200, &steps);
        report(file, &divergences, &refused, &wire);
        let got: Vec<&str> = divergences.iter().map(|d| d.step.as_str()).collect();
        assert_eq!(
            got,
            known_xiegu_divergences(file),
            "{file}: {divergences:#?}"
        );
    }
}

/// The steps on which Hamlib and radioctl disagree for a Xiegu file.
///
/// None: the five rigs agree. (Hamlib runs here with its cache off, see `try_start`. With the
/// cache on, Hamlib's X108G flow selected VFO B (`07 01`) before it worked, so its frequency
/// writes landed in the virtual rig's VFO B and the final VFO A frequency differed; that was
/// Hamlib's cached idea of which VFO is selected, which depends on how long ago it last asked,
/// not a property of the radio or of radioctl.)
fn known_xiegu_divergences(_file: &str) -> Vec<&'static str> {
    Vec::new()
}

// Tests [@ANCHOR: radioctl_yaesu_engine_encode]
// Tests [@ANCHOR: radioctl_yaesu_engine_decode]
// Tests [@ANCHOR: radioctl_virtual_yaesu_command]
#[test]
fn the_yaesu_files_against_hamlib_record_every_disagreement() {
    // Frequency, mode, PTT read, AF gain and (FT-991A) split; no RIT offset (Hamlib answers from
    // its cache part of the time, as for the Kenwoods).
    for (file, model) in [
        ("yaesu-ft-991a", 1035),
        ("yaesu-ftdx10", 1042),
        ("yaesu-ftdx101d", 1040),
    ] {
        let desc = shipped(file);
        let steps: Vec<Op> = icom_scenario(7_074_000, 14_060_000, 14_025_000)
            .into_iter()
            .filter(|op| {
                !matches!(op, Op::SetRitOffset(_) | Op::GetRitOffset)
                    && (desc.capabilities.has(op.required_capability()))
            })
            .collect();
        let (divergences, refused, wire) = compare(model, &desc, 38400, &steps);
        report(file, &divergences, &refused, &wire);
        let got: Vec<&str> = divergences.iter().map(|d| d.step.as_str()).collect();
        assert_eq!(
            got,
            known_yaesu_divergences(file),
            "{file}: {divergences:#?}"
        );
    }
}

/// The steps on which Hamlib and radioctl disagree for a Yaesu file.
fn known_yaesu_divergences(_file: &str) -> Vec<&'static str> {
    Vec::new()
}

// Tests [@ANCHOR: radioctl_ylegacy_engine_encode]
// Tests [@ANCHOR: radioctl_ylegacy_engine_feed]
// Tests [@ANCHOR: radioctl_virtual_legacy_block]
#[test]
fn the_yaesu_legacy_files_against_hamlib_record_every_disagreement() {
    // Frequency, mode and the PTT read: what the legacy descriptions can read back. The FT-818 runs
    // (Hamlib's 818 model answers); the FT-817 Hamlib's FT-817 backend sends an EEPROM read (opcode BB, in no manual) at open
    // and its retries (3 s, 5 times) make every step answer nothing (seen: all steps NoAnswer,
    // 20 s per file).
    for (file, model) in [
        ("yaesu-ft-818", 1041),
        ("yaesu-ft-857d", 1022),
        ("yaesu-ft-897d", 1043),
    ] {
        let desc = shipped(file);
        let mut steps = scenario();
        steps.push(Op::GetPtt);
        let (divergences, refused, wire) = compare(model, &desc, 4800, &steps);
        report(file, &divergences, &refused, &wire);
        let got: Vec<&str> = divergences.iter().map(|d| d.step.as_str()).collect();
        assert_eq!(
            got,
            known_yaesu_legacy_divergences(file),
            "{file}: {divergences:#?}"
        );
    }
}

/// The steps on which Hamlib and radioctl disagree for a legacy Yaesu file.
///
/// Hamlib's writes land (the final frequency, mode and PTT states agree) but its reads of
/// frequency and mode fail against the virtual rig, which answers `03` as the manual says (four
/// BCD bytes and the mode). Hamlib also sends EEPROM reads (opcode BB) at open that no manual
/// documents and the virtual rig, built from the manuals only, does not answer. The cause of the
/// failed reads was not isolated; it may be that unanswered probing, or a difference in the answer
/// that only a real radio can settle.
///
/// The FT-818 is different: Hamlib's FT-817-family backend reads its frequency and mode without
/// trouble, and only the PTT read disagrees. That backend (`ft817_get_ptt`, read for facts only)
/// reports transmitting for any TX status byte other than 0xFF, while the manual defines bit 7 as
/// PTT (0 transmitting) and the virtual rig answers by the manual, so an idle virtual rig reads as
/// transmitting to Hamlib. What a real radio returns while receiving would settle which reading is
/// right; radioctl follows the manual.
fn known_yaesu_legacy_divergences(file: &str) -> Vec<&'static str> {
    if file == "yaesu-ft-818" {
        return vec!["GetPtt"];
    }
    vec!["GetFrequency(A)", "GetMode"]
}

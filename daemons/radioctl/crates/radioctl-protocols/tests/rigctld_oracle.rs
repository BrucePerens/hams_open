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
                Err(_) if started.elapsed() < Duration::from_secs(45) => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => panic!("rigctld did not listen on {port}: {e}"),
            }
        };
        stream
            // Hamlib's FT-817-family and TS-890S backends probe at open with commands the virtual
            // rig refuses or does not know, each with its own retries, so the first answer can take
            // longer than 6 s when the machine is busy (seen: the FT-818 and TS-890S steps all
            // reading NoAnswer in a whole-workspace run, passing alone). A generous bound costs
            // time only when an answer really is missing.
            .set_read_timeout(Some(Duration::from_secs(30)))
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
        Op::SetLevel(Level::AfGain, raw) => {
            oracle.call(&format!("L AF {:.6}", f64::from(*raw) / 255.0))
        }
        Op::GetLevel(Level::AfGain) => oracle.call("l AF"),
        Op::SetSplit(on) => oracle.call(if *on { "S 1 VFOB" } else { "S 0 VFOA" }),
        Op::GetSplit => oracle.call("s"),
        Op::SetFrequency(Vfo::B, hz) => oracle.call(&format!("I {}", hz.0)),
        Op::GetFrequency(Vfo::B) => oracle.call("i"),
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
        let steps = if file == "icom-ic-9700" {
            icom_scenario(144_300_000, 432_100_000, 145_500_000)
        } else {
            icom_scenario(7_074_000, 14_060_000, 14_025_000)
        };
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
/// The G90, X6100 and X6200 agree. The X5105 behaves as the X108G, whose settings Hamlib shares
/// with it. On the X108G Hamlib selects VFO B (`07 01`) before it works, so its
/// frequency writes land in the virtual rig's VFO B while radioctl's engine, which assumes VFO A
/// is selected and never sends `07`, writes VFO A: the virtual rig's VFO A ends at a different
/// frequency. Which VFO a real X108G treats as "the" VFO is unknown.
fn known_xiegu_divergences(file: &str) -> Vec<&'static str> {
    match file {
        "xiegu-x108g" | "xiegu-x5105" => vec!["final freq_a"],
        _ => Vec::new(),
    }
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

// SPDX-License-Identifier: AGPL-3.0-or-later
//! A read-and-set console for the first day with a real radio: any rig description, any transport,
//! one command per line on standard input, every reply, status-change event and scope frame
//! printed with its time and copied to a log file. It is a receive-only tool: it has no key, no
//! Morse and no way to open the transmit gate, so the radio cannot be keyed from here (use
//! `radioctld` with `transmit = true` for that, deliberately).
//!
//! Usage: `bench_console [--unmanaged] [--log FILE] RIG_ID TARGET [BAUD]`
//!
//! - `RIG_ID` names a file in `rigs/` (`icom-ic-7300`; see `RADIOCTL_RIGS`).
//! - `TARGET` is `virtual` (an in-memory radio), a serial path (`/dev/ttyUSB0`) or
//!   `usb:VID:PID[:SERIAL]` (hexadecimal ids). `BAUD` defaults to 19200.
//! - `--unmanaged` is for a port without DTR and RTS (a pty); leave it off for a real radio.
//!
//! Type `help` for the commands.
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, Request};
use radioctl_core::error::RigError;
use radioctl_core::op::{Field, Op};
use radioctl_core::rig::{spawn_current, RigHandle, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::scope::ScopeAxis;
use radioctl_core::serial::{open_serial, LineControl, PortTarget, SerialConfig};
use radioctl_core::units::{Hz, Level, Mode, Vfo};
use radioctl_core::usb::UsbMatcher;
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::runtime::Handle;
use tokio::sync::mpsc;

/// `spawn` wants a sized engine type; this lets the engine of any family go through it.
struct AnyEngine(Box<dyn ProtocolEngine>);

impl ProtocolEngine for AnyEngine {
    fn description(&self) -> &RigDescription {
        self.0.description()
    }

    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        self.0.encode(op)
    }

    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        self.0.feed(bytes, now)
    }
}

/// The directory of rig descriptions: `$RADIOCTL_RIGS`, else `./rigs` when it exists, else the
/// checkout this program was built from.
fn rigs_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RADIOCTL_RIGS") {
        return PathBuf::from(dir);
    }
    let here = PathBuf::from("rigs");
    if here.is_dir() {
        return here;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs")
}

/// Prints a line with the time since start and copies it to the log file.
#[derive(Clone)]
struct Out {
    started: Instant,
    log: Arc<Mutex<Option<File>>>,
}

impl Out {
    fn line(&self, text: &str) {
        let line = format!("{:9.3} {text}", self.started.elapsed().as_secs_f64());
        println!("{line}");
        if let Ok(mut guard) = self.log.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = writeln!(file, "{line}");
            }
        }
    }
}

const HELP: &str = "\
reads:   freq [a|b] | mode | signal | power | swr | ptt | split | txvfo | rit | xit | ritoffset
         filter | level NAME | id | firmware | cached | stats
sets:    setfreq a|b HZ | setmode NAME | setsplit 0|1 | setrit 0|1 | setxit 0|1 | setritoffset HZ
         setfilter HZ | setlevel NAME RAW | transceive 0|1
scope:   scope on|off|state | axis [RX] | setaxis RX center|fixed|scroll-center|scroll-fixed
         span [RX] | setspan RX HZ | setedges RX SET LOW HIGH | ref [RX] | setref RX DB
         frames N [FILE.csv]   (collect N sweeps; needs scope on)
other:   sleep SECONDS | help | quit
There is no key, Morse or gate command: this console cannot transmit.
Levels: af-gain rf-gain mic-gain monitor-level keyer-speed rf-power attenuator preamp";

fn vfo(word: Option<&&str>) -> Option<Vfo> {
    match word.map(|w| w.to_ascii_lowercase()).as_deref() {
        None | Some("a") => Some(Vfo::A),
        Some("b") => Some(Vfo::B),
        _ => None,
    }
}

fn flag(word: &str) -> Option<bool> {
    match word {
        "1" | "on" => Some(true),
        "0" | "off" => Some(false),
        _ => None,
    }
}

fn axis(word: &str) -> Option<ScopeAxis> {
    match word {
        "center" => Some(ScopeAxis::Center),
        "fixed" => Some(ScopeAxis::Fixed),
        "scroll-center" => Some(ScopeAxis::ScrollCenter),
        "scroll-fixed" => Some(ScopeAxis::ScrollFixed),
        _ => None,
    }
}

/// The operation a command line asks for, or why it cannot be one.
fn parse_op(words: &[&str]) -> Result<Op, String> {
    let num = |i: usize| -> Result<u64, String> {
        words
            .get(i)
            .and_then(|w| w.parse().ok())
            .ok_or_else(|| format!("argument {i} must be a number"))
    };
    let rx = |i: usize| -> Result<u8, String> {
        match words.get(i) {
            None => Ok(0),
            Some(w) => w.parse().map_err(|_| "RX must be 0 or 1".to_string()),
        }
    };
    let on = |i: usize| -> Result<bool, String> {
        words
            .get(i)
            .and_then(|w| flag(w))
            .ok_or_else(|| "argument must be 0 or 1".to_string())
    };
    let level = |i: usize| -> Result<Level, String> {
        words
            .get(i)
            .and_then(|w| Level::from_name(w))
            .ok_or_else(|| "unknown level name (see help)".to_string())
    };
    let which = || vfo(words.get(1)).ok_or_else(|| "VFO must be a or b".to_string());
    Ok(match words.first().copied().unwrap_or("") {
        "freq" => Op::GetFrequency(which()?),
        "setfreq" => Op::SetFrequency(vfo(words.get(1)).ok_or("VFO must be a or b")?, Hz(num(2)?)),
        "mode" => Op::GetMode,
        "setmode" => Op::SetMode(
            words
                .get(1)
                .and_then(|w| Mode::from_name(w))
                .ok_or("unknown mode name (lsb usb am cw rtty fm cw-r rtty-r ...)")?,
        ),
        "signal" => Op::GetSignal,
        "power" => Op::GetPower,
        "swr" => Op::GetSwr,
        "ptt" => Op::GetPtt,
        "split" => Op::GetSplit,
        "txvfo" => Op::GetTxVfo,
        "setsplit" => Op::SetSplit(on(1)?),
        "rit" => Op::GetRit,
        "setrit" => Op::SetRit(on(1)?),
        "xit" => Op::GetXit,
        "setxit" => Op::SetXit(on(1)?),
        "ritoffset" => Op::GetRitOffset,
        "setritoffset" => Op::SetRitOffset(
            words
                .get(1)
                .and_then(|w| w.parse().ok())
                .ok_or("offset must be a whole number of hertz")?,
        ),
        "filter" => Op::GetFilterWidth,
        "setfilter" => Op::SetFilterWidth(Hz(num(1)?)),
        "level" => Op::GetLevel(level(1)?),
        "setlevel" => Op::SetLevel(
            level(1)?,
            u16::try_from(num(2)?).map_err(|_| "RAW must fit in 16 bits")?,
        ),
        "id" => Op::GetModelId,
        "firmware" => Op::GetFirmware,
        "transceive" => Op::SetTransceive(on(1)?),
        "scope" => match words.get(1).copied() {
            Some("on") => Op::SetScopeOutput(true),
            Some("off") => Op::SetScopeOutput(false),
            Some("state") => Op::GetScopeOutput,
            _ => return Err("scope on|off|state".into()),
        },
        "axis" => Op::GetScopeAxis(rx(1)?),
        "setaxis" => Op::SetScopeAxis(
            rx(1)?,
            words
                .get(2)
                .and_then(|w| axis(w))
                .ok_or("axis: center fixed scroll-center scroll-fixed")?,
        ),
        "span" => Op::GetScopeSpan(rx(1)?),
        "setspan" => Op::SetScopeSpan(rx(1)?, Hz(num(2)?)),
        "setedges" => Op::SetScopeEdges(
            rx(1)?,
            u8::try_from(num(2)?).map_err(|_| "SET must fit in a byte")?,
            Hz(num(3)?),
            Hz(num(4)?),
        ),
        "ref" => Op::GetScopeReference(rx(1)?),
        "setref" => Op::SetScopeReference(
            rx(1)?,
            words
                .get(2)
                .and_then(|w| w.parse().ok())
                .ok_or("DB must be a whole number")?,
        ),
        "" => return Err(String::new()),
        other => return Err(format!("unknown command {other:?}; try help")),
    })
}

async fn open_rig(
    rig_id: &str,
    target: &str,
    baud: u32,
    unmanaged: bool,
    options: RigOptions,
    out: &Out,
) -> Result<RigHandle, Box<dyn Error>> {
    let text = std::fs::read_to_string(rigs_dir().join(format!("{rig_id}.toml")))?;
    let description = rigfile::parse(&text)?.description;
    let engine = AnyEngine(engine_for(&description)?);
    if target == "virtual" {
        let radio = VirtualRig::from_description(&description)?;
        let (stream, control) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);
        // A radio with its waveform output on sends sweeps by itself, about ten a second.
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                control.scope_sweep();
            }
        });
        return Ok(spawn_current(engine, stream, options)?);
    }
    let port = match target.strip_prefix("usb:") {
        Some(ids) => {
            let mut parts = ids.splitn(3, ':');
            let vid = u16::from_str_radix(parts.next().unwrap_or(""), 16)?;
            let pid = u16::from_str_radix(parts.next().unwrap_or(""), 16)?;
            PortTarget::Usb(UsbMatcher {
                vid,
                pid,
                serial: parts.next().map(str::to_string),
            })
        }
        None => PortTarget::Path(PathBuf::from(target)),
    };
    let mut config = SerialConfig::new(port, baud);
    config.first_open_attempts = 3;
    if unmanaged {
        config.lines = LineControl::Unmanaged;
    }
    let link = open_serial(&Handle::current(), config).await?;
    out.line(&format!("link: {:?}", link.status()));
    let mut options = options;
    options.link_watch = Some(link.watch());
    let (transport, monitor) = link.into_parts();
    // The monitor (control lines, status, counters) lives as long as the program.
    std::mem::forget(monitor);
    Ok(spawn_current(engine, transport, options)?)
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut unmanaged = false;
    let mut log_path = None;
    while let Some(first) = args.first().map(String::as_str) {
        match first {
            "--unmanaged" => {
                unmanaged = true;
                args.remove(0);
            }
            "--log" if args.len() > 1 => {
                log_path = Some(args.remove(1));
                args.remove(0);
            }
            _ => break,
        }
    }
    let [rig_id, target, rest @ ..] = args.as_slice() else {
        return Err("usage: bench_console [--unmanaged] [--log FILE] RIG_ID TARGET [BAUD]".into());
    };
    let baud: u32 = rest.first().map_or(Ok(19_200), |b| b.parse())?;
    let out = Out {
        started: Instant::now(),
        log: Arc::new(Mutex::new(match log_path {
            Some(path) => Some(File::create(path)?),
            None => None,
        })),
    };
    out.line(&format!(
        "bench_console: {rig_id} on {target} at {baud} baud (receive only)"
    ));

    let rig = open_rig(rig_id, target, baud, unmanaged, RigOptions::default(), &out).await?;

    // Every status change as it happens, with its source.
    let mut events = rig.events();
    let events_out = out.clone();
    tokio::spawn(async move {
        while let Some(event) = events.next().await {
            for change in &event.changes {
                events_out.line(&format!(
                    "event source={:?} {:?}: {:?} -> {:?}",
                    event.source, change.field, change.old, change.new
                ));
            }
        }
    });

    // Standard input on its own thread so the actor and the event printer keep running.
    let (lines_tx, mut lines_rx) = mpsc::channel::<String>(16);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if lines_tx.blocking_send(line).is_err() {
                break;
            }
        }
    });

    while let Some(line) = lines_rx.recv().await {
        let words: Vec<&str> = line.split_whitespace().collect();
        out.line(&format!("> {line}"));
        match words.first().copied().unwrap_or("") {
            "quit" | "exit" => break,
            "help" => {
                println!("{HELP}");
                continue;
            }
            "cached" => {
                for field in [
                    Field::Frequency(Vfo::A),
                    Field::Frequency(Vfo::B),
                    Field::Mode,
                    Field::Ptt,
                    Field::Signal,
                    Field::Power,
                    Field::Split,
                    Field::Swr,
                    Field::FilterWidth,
                    Field::ModelId,
                    Field::Firmware,
                ] {
                    if let Some(cached) = rig.cached(field) {
                        out.line(&format!("cached {field:?}: {:?}", cached.value));
                    }
                }
                continue;
            }
            "stats" => {
                out.line(&format!("{:?} link_up={}", rig.stats(), rig.link_up()));
                continue;
            }
            "sleep" => {
                let seconds: f64 = words.get(1).and_then(|w| w.parse().ok()).unwrap_or(1.0);
                tokio::time::sleep(Duration::from_secs_f64(seconds.clamp(0.0, 600.0))).await;
                continue;
            }
            "frames" => {
                let count: usize = words.get(1).and_then(|w| w.parse().ok()).unwrap_or(5);
                let mut csv = match words.get(2) {
                    Some(path) => Some(File::create(path)?),
                    None => None,
                };
                let mut stream = rig.scope_frames();
                let started = Instant::now();
                let mut got = 0;
                while got < count {
                    match tokio::time::timeout(Duration::from_secs(10), stream.next()).await {
                        Ok(Some(frame)) => {
                            got += 1;
                            let top = frame.samples.iter().max().copied().unwrap_or(0);
                            out.line(&format!(
                                "scope seq={} rx={} axis={:?} {}..{} Hz span={} samples={} max={} amplitude_max={} out_of_range={}",
                                frame.seq,
                                frame.receiver,
                                frame.axis,
                                frame.low.0,
                                frame.high.0,
                                frame.span().0,
                                frame.samples.len(),
                                top,
                                frame.amplitude_max,
                                frame.out_of_range
                            ));
                            if let Some(file) = csv.as_mut() {
                                let samples: Vec<String> =
                                    frame.samples.iter().map(u8::to_string).collect();
                                writeln!(
                                    file,
                                    "{},{},{},{},{}",
                                    frame.seq,
                                    frame.low.0,
                                    frame.high.0,
                                    frame.amplitude_max,
                                    samples.join(",")
                                )?;
                            }
                        }
                        _ => {
                            out.line("scope: no frame within 10 s (is the scope on?)");
                            break;
                        }
                    }
                }
                let seconds = started.elapsed().as_secs_f64();
                if got > 1 {
                    out.line(&format!(
                        "scope: {got} frames in {seconds:.2} s = {:.2} frames/s",
                        got as f64 / seconds
                    ));
                }
                continue;
            }
            _ => {}
        }
        match parse_op(&words) {
            Err(message) if message.is_empty() => {}
            Err(message) => out.line(&format!("? {message}")),
            Ok(op) => {
                let sent = Instant::now();
                match rig.execute(op).await {
                    Ok(Some(observation)) => out.line(&format!(
                        "= {observation:?} ({} ms)",
                        sent.elapsed().as_millis()
                    )),
                    Ok(None) => out.line(&format!("= ok ({} ms)", sent.elapsed().as_millis())),
                    Err(error) => {
                        out.line(&format!(
                            "! {:?}: {} ({} ms)",
                            error.kind,
                            error.detail,
                            sent.elapsed().as_millis()
                        ));
                        for wire in &error.transcript_tail {
                            out.line(&format!("!   {wire}"));
                        }
                    }
                }
            }
        }
    }
    out.line(&format!("final stats: {:?}", rig.stats()));
    Ok(())
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! `soak <hours> <power_cycle_every_minutes> <hub:port> <scope_host:port> <run_dir>`: the long run that backs the reliability claim, with an
//! investigation record for every anomaly (docs/proposals/VNWA_SOAK_WITH_ANOMALY_INVESTIGATION.md in hams_com).
//!
//! Each round it (1) sweeps S11 and S21 over 100 kHz to 150 MHz in 60 points and judges the sweep against TWO baselines, the first of the run and
//! the one measured right after the last power cycle, each within 1 % and 1 degree (the goal's agreement target; the unit's own temperature, request 0x85,
//! is logged with every sweep); a sweep outside tolerance is run again at once and both are recorded, (2) measures the frequency meter on the TX-to-RX
//! loopback at 10 MHz (offset under 0.01 Hz), (3) switches the generator on at 10 MHz, -6 dB and -12 dB and off and reads the level on the
//! scope (CH1, 1 MOhm, through the pad and tee) within 8 %, (4) every so many minutes cold-resets the unit through the hub and takes a new
//! baseline. A step that returns a clear error is an ERROR event; a step over its deadline is a HANG (stack waits and journal saved, exit 3).
//!
//! Nothing is silent: every failed attempt behind a recovered point (`RECOVERED` lines, with the error text, raw channel levels and the last
//! transfers), every power cycle the recovery did, every overrun, every kernel USB disconnect that no power cycle of ours explains, every change of
//! the USB topology and every out-of-tolerance reading is one line in `events.jsonl` in the run directory (with epoch and ISO time, class,
//! summary, and the kernel journal for five seconds either side). Side channels in the run directory: `journal.log` (`journalctl -k -f`), `host.log`
//! (load and temperatures once a minute), `lsusb_*.txt`. A file `PAUSE` in the run directory stops the run after the current round until it is
//! removed (an investigator may then use the hardware); the run then takes fresh baselines. A file `STOP` ends the run.
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use vnwa_open::audio::Complex;
use vnwa_open::generator;
use vnwa_open::power::{cold_reset, HubPort};
use vnwa_open::sweep::{agreement, Config, Point, Vna};
use vnwa_open::usb::Vnwa;

/// The sweep tolerance against a baseline: the worst point of a sweep may differ by this much. It is the goal's agreement target with the vendor
/// program (1 % and 1 degree): a slow change inside it, such as the 0.2 to 0.9 % the first soak showed over hours, is not a failure; outside it is a
/// WRONG number and an event. The unit's temperature, the worst IF error and level change are logged with every sweep for whoever looks at the drift.
const TOL_MAG: f64 = 1.0;
const TOL_PH: f64 = 1.0;
/// A round with more re-measures than this is an event (the first soak averaged about 3.7 per round).
const RETRIES_HIGH: u32 = 12;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_f() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// "2026-10-29T09:12:03-07:00"-style time from `date`, so the file needs no time library.
fn iso(epoch: u64) -> String {
    Command::new("date")
        .args(["-d", &format!("@{epoch}"), "+%Y-%m-%dT%H:%M:%S%z"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn log(kind: &str, msg: &str) {
    println!("{} {kind} {msg}", now());
    let _ = std::io::stdout().flush();
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// State shared with the side-channel threads and the watchdog.
struct Shared {
    dir: PathBuf,
    events: Mutex<std::fs::File>,
    counts: Mutex<BTreeMap<String, u32>>,
    /// Kernel journal lines with their epoch, newest last.
    journal: Mutex<VecDeque<(f64, String)>>,
    /// Epochs of kernel "USB disconnect" lines of the unit.
    disconnects: Mutex<Vec<f64>>,
    /// Intervals (epoch start, end) in which a power cycle of ours was under way.
    windows: Mutex<Vec<(f64, f64)>>,
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

fn shared() -> &'static Arc<Shared> {
    SHARED.get().expect("run directory not set up")
}

/// The kernel journal from `before` seconds before `t` to `after` seconds after, at most 60 lines.
fn journal_slice(t: f64, before: f64, after: f64) -> String {
    let j = shared().journal.lock().unwrap();
    j.iter()
        .filter(|(e, _)| *e >= t - before && *e <= t + after)
        .take(60)
        .map(|(_, l)| l.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Records one event: a line in `events.jsonl` and an EVENT line in the log. `detail` is kept whole.
fn event(class: &str, summary: &str, detail: &str, t: f64) {
    let sh = shared();
    *sh.counts
        .lock()
        .unwrap()
        .entry(class.to_string())
        .or_insert(0) += 1;
    let line = format!(
        "{{\"epoch\":{:.3},\"iso\":{},\"class\":{},\"summary\":{},\"detail\":{},\"journal_5s\":{}}}\n",
        t,
        json_str(&iso(t as u64)),
        json_str(class),
        json_str(summary),
        json_str(detail),
        json_str(&journal_slice(t, 5.0, 5.0)),
    );
    let _ = sh.events.lock().unwrap().write_all(line.as_bytes());
    log("EVENT", &format!("{class}: {summary}"));
}

fn lsusb_normalised() -> String {
    let out = Command::new("lsusb")
        .arg("-t")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    // Device numbers change at every enumeration; the tree must not.
    out.lines()
        .map(|l| match l.find("Dev ") {
            Some(i) => match l[i..].find(',') {
                Some(j) => format!("{}{}", &l[..i], &l[i + j + 1..]),
                None => l.to_string(),
            },
            None => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Saves `lsusb -t` as `lsusb_<tag>.txt` and records an event if the tree differs from the first one.
fn lsusb_check(tag: &str, first: &mut Option<String>) {
    let tree = lsusb_normalised();
    let raw = Command::new("lsusb")
        .arg("-t")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let _ = std::fs::write(shared().dir.join(format!("lsusb_{tag}.txt")), raw);
    match first {
        None => *first = Some(tree),
        Some(f) if *f != tree => event(
            "usb_topology_changed",
            &format!("lsusb -t after {tag} differs from the first"),
            &format!("first:\n{f}\n\nnow:\n{tree}"),
            now_f(),
        ),
        _ => {}
    }
}

fn start_journal(hub: &HubPort) {
    let needle = format!("usb {}.{}", hub.hub, hub.port);
    let sh = shared().clone();
    std::thread::spawn(move || {
        let mut file = std::fs::File::create(sh.dir.join("journal.log")).expect("journal.log");
        let mut child = match Command::new("sudo")
            .args([
                "-n",
                "journalctl",
                "-k",
                "-f",
                "-n",
                "0",
                "-o",
                "short-unix",
            ])
            .stdout(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                event(
                    "side_channel_failed",
                    "journalctl could not be started",
                    &e.to_string(),
                    now_f(),
                );
                return;
            }
        };
        let out = child.stdout.take().expect("journal stdout");
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let _ = writeln!(file, "{line}");
            let t: f64 = line
                .split_whitespace()
                .next()
                .and_then(|x| x.parse().ok())
                .unwrap_or_else(now_f);
            if line.contains(&needle) && line.contains("USB disconnect") {
                sh.disconnects.lock().unwrap().push(t);
            }
            let mut j = sh.journal.lock().unwrap();
            j.push_back((t, line));
            while j.len() > 30_000 {
                j.pop_front();
            }
        }
        event(
            "side_channel_failed",
            "the journalctl pipe closed",
            "no further kernel messages are recorded",
            now_f(),
        );
    });
}

fn thermal() -> String {
    let mut o = String::new();
    if let Ok(rd) = std::fs::read_dir("/sys/class/thermal") {
        let mut zones: Vec<_> = rd.flatten().collect();
        zones.sort_by_key(|e| e.file_name());
        for z in zones {
            let p = z.path();
            let (ty, t) = (
                std::fs::read_to_string(p.join("type")).unwrap_or_default(),
                std::fs::read_to_string(p.join("temp")).unwrap_or_default(),
            );
            if let Ok(m) = t.trim().parse::<i64>() {
                o.push_str(&format!(
                    " {}={:.1}",
                    ty.trim().replace(' ', "_"),
                    m as f64 / 1000.0
                ));
            }
        }
    }
    o
}

fn start_host_monitor() {
    let dir = shared().dir.clone();
    std::thread::spawn(move || {
        let mut f = std::fs::File::create(dir.join("host.log")).expect("host.log");
        loop {
            let load = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
            let _ = writeln!(f, "{} load {} thermal_C{}", now(), load.trim(), thermal());
            std::thread::sleep(Duration::from_secs(60));
        }
    });
}

struct Scope {
    r: BufReader<TcpStream>,
    w: TcpStream,
}

impl Scope {
    fn open(addr: &str) -> Result<Scope, String> {
        let s = TcpStream::connect(addr).map_err(|e| format!("scope {addr}: {e}"))?;
        s.set_read_timeout(Some(Duration::from_secs(5))).ok();
        Ok(Scope {
            r: BufReader::new(s.try_clone().map_err(|e| e.to_string())?),
            w: s,
        })
    }
    fn send(&mut self, c: &str) -> Result<(), String> {
        self.w
            .write_all(format!("{c}\n").as_bytes())
            .map_err(|e| e.to_string())
    }
    fn q(&mut self, c: &str) -> Result<String, String> {
        self.send(c)?;
        let mut l = String::new();
        self.r.read_line(&mut l).map_err(|e| format!("{c}: {e}"))?;
        Ok(l.trim().to_string())
    }
    /// The rms of the AC part of one acquisition of CH1 in volts (8-bit samples scaled by the vertical scale), read as a waveform block so
    /// that it does not depend on the scope's own measurement engine or trigger.
    fn rms(&mut self) -> Result<f64, String> {
        let vdiv = {
            let r = self.q("C1:VDIV?")?;
            let v = r.rsplit(' ').next().unwrap_or("");
            v.trim_end_matches(|c: char| !(c.is_ascii_digit() || c == '.'))
                .parse::<f64>()
                .map_err(|_| format!("vdiv {r:?}"))?
        };
        self.send("C1:WF? DAT2")?;
        let mut byte = [0u8; 1];
        use std::io::Read;
        while byte[0] != b'#' {
            self.r
                .read_exact(&mut byte)
                .map_err(|e| format!("waveform: {e}"))?;
        }
        self.r.read_exact(&mut byte).map_err(|e| e.to_string())?;
        let nd = (byte[0] as char).to_digit(10).ok_or("bad block header")? as usize;
        let mut n = vec![0u8; nd];
        self.r.read_exact(&mut n).map_err(|e| e.to_string())?;
        let len: usize = String::from_utf8_lossy(&n)
            .parse()
            .map_err(|_| "bad block length")?;
        let mut data = vec![0u8; len];
        self.r
            .read_exact(&mut data)
            .map_err(|e| format!("waveform data: {e}"))?;
        let mut tail = [0u8; 2];
        let _ = self.r.read(&mut tail);
        if data.len() < 1000 {
            return Err(format!("only {} samples", data.len()));
        }
        let v: Vec<f64> = data
            .iter()
            .map(|b| f64::from(*b as i8) * vdiv / 25.0)
            .collect();
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        let var = v.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / v.len() as f64;
        Ok(var.sqrt())
    }
}

fn worst(base: &[Point], now: &[Point]) -> (f64, f64) {
    let (mut m, mut p) = (0.0f64, 0.0f64);
    for (a, b) in base.iter().zip(now) {
        for (x, y) in [(a.s11, b.s11), (a.s21, b.s21)] {
            let (dm, dp) = agreement(Complex { re: x.re, im: x.im }, y);
            m = m.max(dm.abs());
            p = p.max(dp.abs());
        }
    }
    (m, p)
}

/// The tolerance of a sweep against a baseline (percent, degrees).
fn tolerance() -> (f64, f64) {
    (TOL_MAG, TOL_PH)
}

/// The class of a recovered failure, from the error texts of its attempts.
fn classify(attempts: &[String]) -> &'static str {
    let all = attempts.join(" ");
    if all.contains("power cycle by the recovery") {
        "unscheduled_power_cycle"
    } else if all.contains("audio overrun") {
        "audio_overrun"
    } else if all.contains("no signal on the reference") {
        "silent_state"
    } else if all.contains("is not steady") {
        "unsteady_tone"
    } else if all.contains("No such device") {
        "usb_device_gone"
    } else if all.contains("Input/Output") || all.contains("still failing after") {
        "usb_transfer_error"
    } else if all.contains("Pipe") {
        "usb_stall"
    } else if all.contains("frames arrived in time") {
        "audio_timeout"
    } else {
        "other_failure"
    }
}

fn temp(vna: &Vna) -> Option<u16> {
    match vna.temperature_counts() {
        Ok(t) => Some(t),
        Err(e) => {
            event(
                "temperature_read_failed",
                "the unit's temperature could not be read",
                &e,
                now_f(),
            );
            None
        }
    }
}

/// Moves the new failure and power records of `vna` into the event file. Returns how many power cycles the recovery did.
fn drain(vna: &Vna, round: u32, failures_seen: &mut usize, power_seen: &mut usize) -> usize {
    for f in vna.failure_log().iter().skip(*failures_seen) {
        let class = classify(&f.attempts);
        log(
            "RECOVERED",
            &format!(
                "round {round}: {} ({} entries): {}",
                f.what,
                f.attempts.len(),
                f.attempts.join(" ;; ")
            ),
        );
        event(
            class,
            &format!(
                "round {round}: {} needed {} failed attempt(s)/step(s)",
                f.what,
                f.attempts.len()
            ),
            &f.attempts.join("\n"),
            f.epoch_s as f64,
        );
    }
    *failures_seen = vna.failure_log().len();
    let new = vna.power_log().len().saturating_sub(*power_seen);
    for p in vna.power_log().iter().skip(*power_seen) {
        shared()
            .windows
            .lock()
            .unwrap()
            .push((p.epoch_s as f64 - p.seconds - 3.0, p.epoch_s as f64 + 3.0));
        event(
            "unscheduled_power_cycle_record",
            &format!(
                "round {round}: the recovery cold reset the unit ({:.1} s to be back)",
                p.seconds
            ),
            &format!("reason (the failed attempts that led to it): {}", p.reason),
            p.epoch_s as f64,
        );
    }
    *power_seen = vna.power_log().len();
    new
}

/// Kernel disconnects of the unit that no power cycle of ours (scheduled, or by the recovery) explains.
fn check_disconnects(seen: &mut usize) {
    std::thread::sleep(Duration::from_secs(1));
    let sh = shared();
    let all: Vec<f64> = sh.disconnects.lock().unwrap().clone();
    for t in all.iter().skip(*seen) {
        let explained = sh
            .windows
            .lock()
            .unwrap()
            .iter()
            .any(|(a, b)| *t >= *a - 3.0 && *t <= *b + 3.0);
        if !explained {
            event(
                "unexpected_disconnect",
                "the kernel reports the unit disconnecting with no power cycle of ours under way",
                "over-current, hub or cable fault? see the journal lines; check the hub with uhubctl",
                *t,
            );
        }
    }
    *seen = all.len();
}

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let hours: f64 = a
        .get(1)
        .and_then(|s| s.parse().ok())
        .ok_or("usage: soak <hours> <cycle_minutes> <hub:port> <scope> <run_dir>")?;
    let cycle_min: f64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let (hubname, port) = a.get(3).and_then(|h| h.split_once(':')).ok_or("hub:port")?;
    let hub = HubPort {
        hub: hubname.to_string(),
        port: port.parse().map_err(|_| "port")?,
    };
    let scope_addr = a.get(4).cloned().ok_or("scope host:port")?;
    let dir = PathBuf::from(a.get(5).cloned().unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let events = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("events.jsonl"))
        .map_err(|e| format!("events.jsonl: {e}"))?;
    let md = dir.join("events.md");
    if !md.exists() {
        let _ = std::fs::write(&md, "# Event notes (written by the watcher: what happened, the evidence lines, a hypothesis)\n\n");
    }
    SHARED
        .set(Arc::new(Shared {
            dir: dir.clone(),
            events: Mutex::new(events),
            counts: Mutex::new(BTreeMap::new()),
            journal: Mutex::new(VecDeque::new()),
            disconnects: Mutex::new(Vec::new()),
            windows: Mutex::new(Vec::new()),
        }))
        .map_err(|_| "shared")?;
    start_journal(&hub);
    start_host_monitor();
    std::thread::sleep(Duration::from_secs(2));
    let stop_file = dir.join("STOP");
    let pause_file = dir.join("PAUSE");
    let started = Instant::now();
    let end = started + Duration::from_secs_f64(hours * 3600.0);
    // Watchdog: no step may run longer than its deadline (the deadline is set by the main loop before each step).
    let deadline = Arc::new(AtomicU64::new(u64::MAX));
    {
        let d = deadline.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5));
            if now() > d.load(Ordering::Relaxed) {
                let mut waits = String::new();
                if let Ok(rd) = std::fs::read_dir("/proc/self/task") {
                    for t in rd.flatten() {
                        let p = t.path();
                        let name = std::fs::read_to_string(p.join("comm")).unwrap_or_default();
                        let w = std::fs::read_to_string(p.join("wchan")).unwrap_or_default();
                        waits.push_str(&format!(
                            "thread {} {}: waiting in {}\n",
                            t.file_name().to_string_lossy(),
                            name.trim(),
                            w.trim()
                        ));
                    }
                }
                event(
                    "hang",
                    "a step exceeded its deadline; exiting for restart",
                    &waits,
                    now_f(),
                );
                std::process::exit(3);
            }
        });
    }
    let step = |secs: u64| deadline.store(now() + secs, Ordering::Relaxed);
    let cfg = Config {
        if_hz: 4000.0,
        hub: Some(hub.clone()),
        ..Config::default()
    };
    log(
        "START",
        &format!(
            "hours {hours} cycle_min {cycle_min} hub {}:{} run_dir {}",
            hub.hub,
            hub.port,
            dir.display()
        ),
    );
    let mut first_tree = None;
    lsusb_check("start", &mut first_tree);
    let (mut sweeps, mut wrong, mut errors, mut cycles) = (0u32, 0u32, 0u32, 0u32);
    let (mut failures_seen, mut power_seen, mut disc_seen) = (0usize, 0usize, 0usize);
    step(300);
    let mut vna = Vna::open(cfg.clone())?;
    let fw = vna.firmware_version()?;
    log("FIRMWARE", &format!("v{}.{}", fw.0, fw.1));
    // base1: the first sweep of the run (or of the last pause); base2: the sweep right after the last power cycle.
    let mut t0 = temp(&vna).unwrap_or(0);
    let mut base1 = vna.sweep(100e3, 150e6, 60)?;
    let mut t1 = t0;
    let mut base2 = base1.clone();
    let mut t2 = t0;
    log(
        "BASELINE",
        &format!("{} points, unit temperature {t0} counts", base1.len()),
    );
    drain(&vna, 0, &mut failures_seen, &mut power_seen);
    let mut last_cycle = Instant::now();
    let mut round = 0u32;
    let mut remeasured_total = 0u32;
    let mut overruns_total = 0u64;
    while Instant::now() < end {
        if stop_file.exists() {
            log("STOP", "the STOP file exists: ending the run");
            break;
        }
        if pause_file.exists() {
            let t = now_f();
            event(
                "paused",
                "the PAUSE file exists: the run waits so the hardware can be used",
                "",
                t,
            );
            step(u64::MAX / 4);
            while pause_file.exists() {
                std::thread::sleep(Duration::from_secs(5));
            }
            step(300);
            drop(vna);
            vna = Vna::open(cfg.clone())?;
            failures_seen = 0;
            power_seen = 0;
            t0 = temp(&vna).unwrap_or(0);
            base1 = vna.sweep(100e3, 150e6, 60)?;
            t1 = t0;
            base2 = base1.clone();
            t2 = t0;
            event(
                "resumed",
                &format!(
                    "paused for {:.0} s; fresh baselines taken at {t0} counts",
                    now_f() - t
                ),
                "",
                now_f(),
            );
        }
        round += 1;
        let round_started = Instant::now();
        let rec0 = vna.recovery();
        // (1) sweep, judged against both baselines with the temperature rule
        step(600);
        let ta = temp(&vna).unwrap_or(0);
        let mut result = vna.sweep(100e3, 150e6, 60);
        let mut straddled = false;
        let new_cycles = drain(&vna, round, &mut failures_seen, &mut power_seen);
        if new_cycles > 0 {
            straddled = true;
        }
        if let Ok(p) = &result {
            sweeps += 1;
            let tb = temp(&vna).unwrap_or(ta);
            let (m2, ph2) = worst(&base2, p);
            let (m1, ph1) = worst(&base1, p);
            let (tol2m, tol2p) = tolerance();
            let (tol1m, tol1p) = tolerance();
            let r = vna.recovery();
            let worst_if = p.iter().map(|x| x.if_error_hz).fold(0.0, f64::max);
            let worst_step = p.iter().map(|x| x.level_change).fold(0.0, f64::max);
            log(
                "SWEEP",
                &format!(
                    "round {round}: worst IF error {worst_if:.3} Hz, worst level change {:.2} %; temperature {ta}->{tb} counts; vs last-cycle baseline (at {t2}) {m2:.3} % {ph2:.3} deg (tolerance {tol2m:.2} % {tol2p:.2} deg); vs first baseline (at {t1}) {m1:.3} % {ph1:.3} deg (tolerance {tol1m:.2} % {tol1p:.2} deg); recovery remeasured {} resends {} cycles {} overruns reported {} points retried for them {}",
                    worst_step * 100.0,
                    r.remeasured, r.transfer_resends, r.power_cycles, r.overruns_reported, r.overrun_retries
                ),
            );
            if straddled {
                event(
                    "sweep_straddled_power_cycle",
                    &format!("round {round}: the sweep spans a power cycle by the recovery, so it is not judged; new baseline taken"),
                    &format!("worst vs last-cycle baseline {m2:.3} % {ph2:.3} deg, vs first {m1:.3} % {ph1:.3} deg"),
                    now_f(),
                );
            } else if m2 > tol2m || ph2 > tol2p {
                wrong += 1;
                step(600);
                let again = vna.sweep(100e3, 150e6, 60);
                drain(&vna, round, &mut failures_seen, &mut power_seen);
                let tc = temp(&vna).unwrap_or(tb);
                let verdict = match &again {
                    Ok(q) => {
                        let (m, ph) = worst(&base2, q);
                        let (tm, tp) = tolerance();
                        format!("repeated at once: {m:.3} % {ph:.3} deg (tolerance {tm:.2} % {tp:.2} deg), temperature {tc} counts: {}", if m > tm || ph > tp { "STILL OUT of tolerance (a fault or a real change)" } else { "back in tolerance (noise)" })
                    }
                    Err(e) => format!("the repeat failed: {e}"),
                };
                event(
                    "wrong_number",
                    &format!("round {round}: sweep {m2:.3} % {ph2:.3} deg from the last-cycle baseline (tolerance {tol2m:.2} % {tol2p:.2} deg, unit at {tb} counts, baseline at {t2})"),
                    &format!("vs first baseline {m1:.3} % {ph1:.3} deg; {verdict}"),
                    now_f(),
                );
                if again.is_ok() {
                    result = again;
                }
            }
            if m1 > tol1m || ph1 > tol1p {
                // The long-term comparison with the first baseline leaves the tolerance.
                event(
                    "long_term_drift",
                    &format!("round {round}: {m1:.3} % {ph1:.3} deg from the first baseline (tolerance {tol1m:.2} % {tol1p:.2} deg at {tb} counts, baseline at {t1})"),
                    "",
                    now_f(),
                );
            }
        }
        match &result {
            Ok(_) => {}
            Err(e) => {
                errors += 1;
                event(
                    "unrecovered_failure",
                    &format!("round {round}: sweep failed and was not recovered"),
                    e,
                    now_f(),
                );
                step(300);
                drop(vna);
                vna = match Vna::open(cfg.clone()) {
                    Ok(v) => v,
                    Err(e2) => {
                        event(
                            "reopen_failed",
                            &format!("reopen after a failed sweep: {e2}"),
                            "",
                            now_f(),
                        );
                        std::thread::sleep(Duration::from_secs(20));
                        match Vna::open(cfg.clone()) {
                            Ok(v) => v,
                            Err(e3) => {
                                event(
                                    "reopen_failed",
                                    &format!("second reopen failed: {e3}"),
                                    "the unit does not come back; exiting for the wrapper",
                                    now_f(),
                                );
                                std::process::exit(4);
                            }
                        }
                    }
                };
                failures_seen = 0;
                power_seen = 0;
            }
        }
        if new_cycles > 0 {
            // The recovery cycled the unit: topology check, and a new baseline for the cold unit.
            lsusb_check(&format!("recovery_cycle_round_{round}"), &mut first_tree);
            step(600);
            t2 = temp(&vna).unwrap_or(0);
            if let Ok(b) = vna.sweep(100e3, 150e6, 60) {
                base2 = b;
            }
            drain(&vna, round, &mut failures_seen, &mut power_seen);
        }
        // (2) meter
        step(120);
        match vna.meter(10e6, 60.0) {
            Ok(r) if r.offset_hz.abs() < 0.01 => log(
                "METER",
                &format!("round {round}: offset {:+.5} Hz", r.offset_hz),
            ),
            Ok(r) => {
                wrong += 1;
                event(
                    "wrong_number",
                    &format!(
                        "round {round}: meter offset {:+.5} Hz on a loopback (limit 0.01)",
                        r.offset_hz
                    ),
                    "",
                    now_f(),
                );
            }
            Err(e) => {
                errors += 1;
                event(
                    "unrecovered_failure",
                    &format!("round {round}: meter failed and was not recovered"),
                    &e,
                    now_f(),
                );
            }
        }
        drain(&vna, round, &mut failures_seen, &mut power_seen);
        // (3) generator and scope; the VNA's codecs are not needed, so a second handle on the control endpoint is enough.
        step(180);
        let gen = || -> Result<(), String> {
            // The meter's burst runs for about 2 s after its last audio; a generator command sent into it is overwritten.
            std::thread::sleep(Duration::from_secs(3));
            let usb = Vnwa::open()?;
            let mut sc = Scope::open(&scope_addr)?;
            // The scope's state is set explicitly each time (other users leave it at other memory depths and scales).
            sc.send("ACQW SAMPLING")?;
            sc.send("MSIZ 14K")?;
            sc.send("C1:CPL D1M")?;
            sc.send("C1:VDIV 0.005")?;
            sc.send("TDIV 100NS")?;
            sc.send("TRMD AUTO")?;
            sc.send("WFSU SP,1,NP,0,FP,0")?; // every point, no decimation
            std::thread::sleep(Duration::from_secs(2));
            // The scope may have been left stopped (or at another time base) by someone else: a stopped scope returns its frozen last
            // waveform, which read as the same level for every generator state in a smoke run on 2026-10-29. Check the state, and ask again.
            let mut state = String::new();
            for _ in 0..10 {
                state = format!("{} {}", sc.q("SAST?")?, sc.q("TDIV?")?);
                if !state.contains("Stop") && state.contains("1.00E-07") {
                    break;
                }
                sc.send("TDIV 100NS")?;
                sc.send("TRMD AUTO")?;
                std::thread::sleep(Duration::from_secs(1));
            }
            if state.contains("Stop") || !state.contains("1.00E-07") {
                return Err(format!(
                    "the scope would not run at 100 ns per division: {state}"
                ));
            }
            generator::on(&usb, 10e6, -6.0)?;
            std::thread::sleep(Duration::from_secs(3));
            let v6 = sc.rms()?;
            generator::set_attenuation(&usb, -12.0)?;
            std::thread::sleep(Duration::from_secs(3));
            let v12 = sc.rms()?;
            generator::off(&usb)?;
            std::thread::sleep(Duration::from_secs(3));
            let off = sc.rms()?;
            // 10.33 mV rms at 0 dB (measured 2026-10-28 with the host FFT): -6 dB is 5.18 mV, -12 dB 2.60 mV; the scope noise floor is 0.08 mV.
            let ok = (v6 / 5.18e-3 - 1.0).abs() < 0.08
                && (v12 / 2.60e-3 - 1.0).abs() < 0.08
                && off < 0.4e-3;
            let msg = format!(
                "round {round}: generator -6 dB {:.3} mV, -12 dB {:.3} mV, off {:.3} mV",
                v6 * 1e3,
                v12 * 1e3,
                off * 1e3
            );
            log("GENERATOR", &msg);
            if ok {
                Ok(())
            } else {
                Err(format!("level: {msg}"))
            }
        };
        match gen() {
            Ok(()) => {}
            Err(e) if e.starts_with("level: ") => {
                wrong += 1;
                event(
                    "wrong_number",
                    &e[7..],
                    "generator level outside 8 % (or not off) on the scope",
                    now_f(),
                );
            }
            Err(e) => {
                errors += 1;
                event(
                    "generator_step_failed",
                    &format!("round {round}: {e}"),
                    "",
                    now_f(),
                );
                // Leave nothing transmitting: switch off through a fresh handle if possible.
                if let Ok(u) = Vnwa::open() {
                    let _ = generator::off(&u);
                }
            }
        }
        // (4) periodic cold reset, then a new baseline of the cold unit
        if last_cycle.elapsed() >= Duration::from_secs_f64(cycle_min * 60.0) {
            step(600);
            let rec = vna.recovery();
            remeasured_total += rec.remeasured;
            overruns_total += rec.overruns_reported;
            drop(vna);
            cycles += 1;
            let t_off = now_f();
            let before_lines = shared().disconnects.lock().unwrap().len();
            shared()
                .windows
                .lock()
                .unwrap()
                .push((t_off - 1.0, t_off + 120.0));
            let reset = cold_reset(&hub);
            let back = now_f();
            match reset.and_then(|_| Vna::open(cfg.clone())) {
                Ok(v) => {
                    vna = v;
                    failures_seen = 0;
                    power_seen = 0;
                    shared()
                        .windows
                        .lock()
                        .unwrap()
                        .push((t_off - 1.0, back + 3.0));
                    let first_start = Instant::now();
                    t2 = temp(&vna).unwrap_or(0);
                    match vna.sweep(100e3, 150e6, 60) {
                        Ok(b) => base2 = b,
                        Err(e) => event(
                            "unrecovered_failure",
                            &format!("the sweep after scheduled cold reset {cycles} failed"),
                            &e,
                            now_f(),
                        ),
                    }
                    let r = vna.recovery();
                    drain(&vna, round, &mut failures_seen, &mut power_seen);
                    event(
                        "scheduled_power_cycle",
                        &format!("cold reset {cycles}: {:.1} s from power-off until the unit and codecs were open, {:.1} s more for the first good sweep ({} re-measures, {} resends, {} overrun reports), unit at {t2} counts", back - t_off, first_start.elapsed().as_secs_f64(), r.remeasured, r.transfer_resends, r.overruns_reported),
                        &format!("disconnect lines seen so far {}", shared().disconnects.lock().unwrap().len() - before_lines),
                        now_f(),
                    );
                    lsusb_check(&format!("cycle_{cycles}"), &mut first_tree);
                    log("CYCLE", &format!("cold reset {cycles} done"));
                }
                Err(e) => {
                    event(
                        "cold_reset_failed",
                        &format!("scheduled cold reset {cycles}: {e}"),
                        "",
                        now_f(),
                    );
                    std::process::exit(4);
                }
            }
            last_cycle = Instant::now();
        }
        check_disconnects(&mut disc_seen);
        // Round-level checks: retries and overruns.
        let rec1 = vna.recovery();
        let d_rem = rec1.remeasured.saturating_sub(rec0.remeasured);
        let d_ovr = rec1
            .overruns_reported
            .saturating_sub(rec0.overruns_reported);
        let secs = round_started.elapsed().as_secs_f64();
        if d_rem > RETRIES_HIGH {
            event("retries_high", &format!("round {round}: {d_rem} re-measures in one round (bound {RETRIES_HIGH}, the first soak averaged 3.7)"), "", now_f());
        }
        if d_ovr as f64 / secs > 10.0 {
            event(
                "overrun_storm",
                &format!("round {round}: {d_ovr} overrun reports in {secs:.0} s"),
                "more than 10 per second is a bug in the harness",
                now_f(),
            );
        }
        log(
            "STATS",
            &format!(
                "elapsed {:.2} h rounds {round} sweeps {sweeps} wrong {wrong} errors {errors} cycles {cycles}; this round {secs:.0} s, re-measures {d_rem}, overrun reports {d_ovr}; so far re-measures {} overrun reports {}",
                started.elapsed().as_secs_f64() / 3600.0,
                remeasured_total + rec1.remeasured,
                overruns_total + rec1.overruns_reported
            ),
        );
    }
    step(120);
    if let Ok(u) = Vnwa::open() {
        let _ = generator::off(&u);
    }
    let counts = shared().counts.lock().unwrap().clone();
    for (c, n) in &counts {
        log("SUMMARY", &format!("event class {c}: {n}"));
    }
    log("END", &format!("rounds {round} sweeps {sweeps} wrong {wrong} errors {errors} cycles {cycles} events {}", counts.values().sum::<u32>()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sweep_tolerance_is_the_agreement_target_with_the_vendor_program() {
        assert_eq!(tolerance(), (1.0, 1.0));
    }

    #[test]
    fn every_event_class_is_recognised_from_the_error_text_of_an_attempt() {
        let c = |t: &str| classify(&[t.to_string()]);
        assert_eq!(
            c("point at 1 Hz: no signal on the reference channel(s) at the IF (codec 1 left 0.4"),
            "silent_state"
        );
        assert_eq!(
            c("point at 1 Hz is not steady: IF error 0.03 Hz, level change 12.3 %"),
            "unsteady_tone"
        );
        assert_eq!(
            c("audio overrun: 2 buffer overrun(s) reported inside the 300 ms"),
            "audio_overrun"
        );
        assert_eq!(
            c("read request 0x0d: still failing after 3 resends (Input/Output Error)"),
            "usb_transfer_error"
        );
        assert_eq!(
            c("request 0x4c: No such device (it may be unplugged)"),
            "usb_device_gone"
        );
        assert_eq!(
            c("audio: 3 of 5760 frames arrived in time (10 overrun reports so far"),
            "audio_timeout"
        );
        // A recovery that power cycled is that class whatever failed first.
        assert_eq!(
            classify(&[
                "x: no signal on the reference".to_string(),
                "power cycle by the recovery done (14.2 s)".to_string()
            ]),
            "unscheduled_power_cycle"
        );
        assert_eq!(c("something new"), "other_failure");
    }

    #[test]
    fn event_text_is_escaped_into_one_json_line() {
        assert_eq!(
            json_str("a\"b\\c\nd\te\u{1}"),
            "\"a\\\"b\\\\c\\nd\\te\\u0001\""
        );
    }
}

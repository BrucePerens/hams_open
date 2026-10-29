// SPDX-License-Identifier: AGPL-3.0-or-later
//! `soak <hours> <power_cycle_every_minutes> <hub:port> <scope_host:port> <log_dir>`: the long run that backs the reliability claim.
//! Each round it (1) sweeps S11 and S21 over 100 kHz to 150 MHz in 60 points and compares with the first sweep of the run (any point more than
//! 0.3 % or 0.3 degrees away is a WRONG RESULT), (2) measures the frequency meter on the TX-to-RX loopback at 10 MHz (offset must be
//! under 0.01 Hz), (3) switches the generator on at 10 MHz, -6 dB and -12 dB and off, and reads the level on the scope (CH1, 1 MOhm, through
//! the pad and tee) within 8 %, and (4) every so many minutes cold-resets the unit through the hub and repeats (1) at once. A step that
//! returns a clear error is logged as ERROR (and counted); a step that takes longer than its deadline is a HANG: the process logs it and
//! exits with status 3 so that a wrapper restarts it. Every line goes to stdout with a time stamp.
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use vnwa_open::audio::Complex;
use vnwa_open::generator;
use vnwa_open::power::{cold_reset, HubPort};
use vnwa_open::sweep::{agreement, Config, Point, Vna};
use vnwa_open::usb::Vnwa;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn log(kind: &str, msg: &str) {
    println!("{} {kind} {msg}", now());
    let _ = std::io::stdout().flush();
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

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let hours: f64 = a
        .get(1)
        .and_then(|s| s.parse().ok())
        .ok_or("usage: soak <hours> <cycle_minutes> <hub:port> <scope> <log_dir>")?;
    let cycle_min: f64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let (hub, port) = a.get(3).and_then(|h| h.split_once(':')).ok_or("hub:port")?;
    let hub = HubPort {
        hub: hub.to_string(),
        port: port.parse().map_err(|_| "port")?,
    };
    let scope_addr = a.get(4).cloned().ok_or("scope host:port")?;
    let log_dir = a.get(5).cloned().unwrap_or_else(|| ".".into());
    let stop_file = std::path::Path::new(&log_dir).join("STOP");
    let started = Instant::now();
    let end = started + Duration::from_secs_f64(hours * 3600.0);
    let beat = Arc::new(AtomicU64::new(now()));
    // Watchdog: no step may run longer than its deadline (the deadline is set by the main loop before each step).
    let deadline = Arc::new(AtomicU64::new(u64::MAX));
    {
        let d = deadline.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5));
            if now() > d.load(Ordering::Relaxed) {
                log("HANG", "a step exceeded its deadline; exiting for restart");
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
            "hours {hours} cycle_min {cycle_min} hub {}:{}",
            hub.hub, hub.port
        ),
    );
    let (mut sweeps, mut wrong, mut errors, mut cycles) = (0u32, 0u32, 0u32, 0u32);
    step(300);
    let mut vna = Vna::open(cfg.clone())?;
    let base = vna.sweep(100e3, 150e6, 60)?;
    log("BASELINE", &format!("{} points", base.len()));
    let mut last_cycle = Instant::now();
    let mut round = 0u32;
    while Instant::now() < end {
        if stop_file.exists() {
            log("STOP", "the STOP file exists: ending the run");
            break;
        }
        round += 1;
        // (1) sweep
        step(400);
        match vna.sweep(100e3, 150e6, 60) {
            Ok(p) => {
                sweeps += 1;
                let (m, ph) = worst(&base, &p);
                let r = vna.recovery();
                if m > 0.3 || ph > 0.3 {
                    wrong += 1;
                    log("WRONG", &format!("round {round}: sweep differs from the baseline by {m:.3} % and {ph:.3} deg"));
                } else {
                    log("OK", &format!("round {round}: sweep within {m:.3} % {ph:.3} deg; recovery remeasured {} resends {} cycles {}", r.remeasured, r.transfer_resends, r.power_cycles));
                }
            }
            Err(e) => {
                errors += 1;
                log(
                    "ERROR",
                    &format!(
                        "round {round}: sweep: {}",
                        e.chars().take(300).collect::<String>()
                    ),
                );
                step(300);
                vna = match Vna::open(cfg.clone()) {
                    Ok(v) => v,
                    Err(e2) => {
                        log("ERROR", &format!("reopen: {e2}"));
                        std::thread::sleep(Duration::from_secs(20));
                        continue;
                    }
                };
            }
        }
        // (2) meter
        step(120);
        match vna.meter(10e6, 60.0) {
            Ok(r) if r.offset_hz.abs() < 0.01 => log(
                "OK",
                &format!("round {round}: meter offset {:+.5} Hz", r.offset_hz),
            ),
            Ok(r) => {
                wrong += 1;
                log(
                    "WRONG",
                    &format!(
                        "round {round}: meter offset {:+.5} Hz on a loopback",
                        r.offset_hz
                    ),
                );
            }
            Err(e) => {
                errors += 1;
                log(
                    "ERROR",
                    &format!(
                        "round {round}: meter: {}",
                        e.chars().take(300).collect::<String>()
                    ),
                );
            }
        }
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
            if ok {
                log("OK", &msg)
            } else {
                log("WRONG", &msg)
            }
            if ok {
                Ok(())
            } else {
                Err("level".into())
            }
        };
        match gen() {
            Ok(()) => {}
            Err(e) if e == "level" => wrong += 1,
            Err(e) => {
                errors += 1;
                log("ERROR", &format!("round {round}: generator: {e}"));
                // Leave nothing transmitting: switch off through a fresh handle if possible.
                if let Ok(u) = Vnwa::open() {
                    let _ = generator::off(&u);
                }
            }
        }
        // (4) periodic cold reset
        if last_cycle.elapsed() >= Duration::from_secs_f64(cycle_min * 60.0) {
            step(300);
            drop(vna);
            cycles += 1;
            match cold_reset(&hub).and_then(|_| Vna::open(cfg.clone())) {
                Ok(v) => {
                    vna = v;
                    log("CYCLE", &format!("cold reset {cycles} done"));
                }
                Err(e) => {
                    log("ERROR", &format!("cold reset {cycles}: {e}"));
                    std::process::exit(4);
                }
            }
            last_cycle = Instant::now();
        }
        beat.store(now(), Ordering::Relaxed);
        log("STATS", &format!("elapsed {:.2} h rounds {round} sweeps {sweeps} wrong {wrong} errors {errors} cycles {cycles}", started.elapsed().as_secs_f64() / 3600.0));
    }
    step(120);
    if let Ok(u) = Vnwa::open() {
        let _ = generator::off(&u);
    }
    log(
        "END",
        &format!("rounds {round} sweeps {sweeps} wrong {wrong} errors {errors} cycles {cycles}"),
    );
    Ok(())
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! `mod_probe <cw|am|fm> <mod_hz> <depth_pct|deviation_hz> <attenuation_db> <silent_s> <poll>`: switches the generator on at 10 MHz, starts the modulation, and
//! measures on the scope (CH1, 1 MOhm, through the pad and tee; 14 ms window, 1.4 M points) the carrier and the envelope: its depth and frequency, at 0.5 s
//! and again after `silent_s` seconds during which nothing is sent to the unit (`poll` = 1 sends a read of request 0x91 every 10 ms instead, as the vendor
//! program does). Then it tries the request that switches the generator off (0x91) once and measures again. The unit must be power cycled afterwards.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use rusb::UsbContext;
use vnwa_open::generator::{self, Modulation};
use vnwa_open::usb::Vnwa;

struct Scope {
    r: BufReader<TcpStream>,
    w: TcpStream,
}

impl Scope {
    fn open(addr: &str) -> Result<Scope, String> {
        let s = TcpStream::connect(addr).map_err(|e| format!("scope {addr}: {e}"))?;
        s.set_read_timeout(Some(Duration::from_secs(20))).ok();
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
    /// One acquisition of CH1 as volts, and the sample interval in seconds.
    fn capture(&mut self) -> Result<(Vec<f64>, f64), String> {
        let vdiv: f64 = {
            let r = self.q("C1:VDIV?")?;
            r.rsplit(' ')
                .next()
                .unwrap_or("")
                .trim_end_matches(|c: char| !(c.is_ascii_digit() || c == '.'))
                .parse()
                .map_err(|_| format!("vdiv {r:?}"))?
        };
        let sara = self.q("SARA?")?;
        let rate: f64 = {
            // "SARA 5.00E+07Sa/s"
            let v = sara
                .rsplit(' ')
                .next()
                .unwrap_or("")
                .trim_end_matches("Sa/s");
            v.parse::<f64>().map_err(|_| format!("sara {sara:?}"))?
        };
        // The scope returns only the screen's 1,400 points while it runs; stopped, it returns the whole memory (700,000 points at 1 ms per division).
        self.send("TRMD AUTO")?;
        std::thread::sleep(Duration::from_millis(1000));
        self.send("TRMD STOP")?;
        std::thread::sleep(Duration::from_millis(500));
        self.send("C1:WF? DAT2")?;
        let mut byte = [0u8; 1];
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
        Ok((
            data.iter()
                .map(|b| f64::from(*b as i8) * vdiv / 25.0)
                .collect(),
            1.0 / rate,
        ))
    }
}

/// Hann-windowed single-bin DFT amplitude of `v` at `f` (Hz), in the units of `v`.
fn bin_amp(v: &[f64], dt: f64, f: f64) -> f64 {
    thread_local! {
        static HANN: std::cell::RefCell<Vec<f64>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    HANN.with(|h| {
        let mut h = h.borrow_mut();
        if h.len() != v.len() {
            let n = v.len() as f64;
            *h = (0..v.len())
                .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n).cos())
                .collect();
        }
        bin_amp_with(v, &h, dt, f)
    })
}

fn bin_amp_with(v: &[f64], win: &[f64], dt: f64, f: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f * dt;
    let (mut re, mut im, mut norm) = (0.0, 0.0, 0.0);
    // The recurrence keeps the sin/cos cost out of the loop.
    let (c, s) = (w.cos(), w.sin());
    let (mut cr, mut ci) = (1.0f64, 0.0f64);
    for (i, x) in v.iter().enumerate() {
        let h = win[i];
        re += x * h * cr;
        im -= x * h * ci;
        norm += h;
        let (nr, ni) = (cr * c - ci * s, cr * s + ci * c);
        cr = nr;
        ci = ni;
    }
    2.0 * (re * re + im * im).sqrt() / norm
}

/// The peak of `bin_amp` within `centre +- span` in steps of `step`: (frequency, amplitude).
fn peak(v: &[f64], dt: f64, centre: f64, span: f64, step: f64) -> (f64, f64) {
    let mut best = (centre, 0.0);
    let mut f = centre - span;
    while f <= centre + span {
        let a = bin_amp(v, dt, f);
        if a > best.1 {
            best = (f, a);
        }
        f += step;
    }
    best
}

/// Carrier rms (mV), the modulation depth in percent and the modulation frequency (Hz) from the sidebands of a capture: the carrier is found near
/// 10 MHz, the sidebands within +-1 Hz-steps of 10 Hz around where `expected_hz` puts them. Depth from AM sidebands: (lower + upper) / carrier.
fn analyse(v: &[f64], dt: f64, expected_hz: f64) -> (f64, f64, f64) {
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let v: Vec<f64> = v.iter().map(|x| x - mean).collect();
    let rms = (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt();
    // The scope's sample clock is not locked to the unit: the carrier reads a few hundred Hz off 10 MHz.
    let (fc, ac) = peak(&v, dt, 10e6, 2_000.0, 100.0);
    let (fc, ac) = peak(&v, dt, fc, 100.0, 10.0).max_by_amp(ac, fc);
    if expected_hz < 1.0 {
        return (rms * 1e3, 0.0, 0.0);
    }
    let span = (expected_hz / 3.0).min(150.0);
    let (fl, al) = peak(&v, dt, fc - expected_hz, span, 5.0);
    let (fu, au) = peak(&v, dt, fc + expected_hz, span, 5.0);
    ((rms * 1e3), 100.0 * (al + au) / ac, ((fu - fl) / 2.0))
}

/// Bessel function of the first kind J_n(x) by numerical integration.
fn bessel(n: i32, x: f64) -> f64 {
    let steps = 200;
    let mut sum = 0.0;
    for k in 0..steps {
        let t = std::f64::consts::PI * (k as f64 + 0.5) / steps as f64;
        sum += (f64::from(n) * t - x * t.sin()).cos();
    }
    sum / steps as f64
}

/// For a capture of an FM carrier: the amplitudes of the lines at the carrier +- n times the modulation frequency, and the modulation index that
/// fits them best against |J_n(beta)| (so the deviation is beta times the modulation frequency).
fn fm_fit(v: &[f64], dt: f64, mod_hz: f64, fc: f64) -> (f64, f64) {
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let v: Vec<f64> = v.iter().map(|x| x - mean).collect();
    let n_max = 20;
    let lines: Vec<f64> = (-n_max..=n_max)
        .map(|n| peak(&v, dt, fc + f64::from(n) * mod_hz, 12.0, 4.0).1)
        .collect();
    let norm = lines.iter().map(|a| a * a).sum::<f64>().sqrt();
    let mut best = (f64::MAX, 0.0);
    let mut beta = 0.5;
    while beta <= 40.0 {
        let model: Vec<f64> = (-n_max..=n_max).map(|n| bessel(n, beta).abs()).collect();
        let mn = model.iter().map(|a| a * a).sum::<f64>().sqrt();
        let err: f64 = lines
            .iter()
            .zip(&model)
            .map(|(a, b)| (a / norm - b / mn).powi(2))
            .sum();
        if err < best.0 {
            best = (err, beta);
        }
        beta += 0.1;
    }
    (best.1, best.0.sqrt())
}

trait MaxByAmp {
    fn max_by_amp(self, a: f64, f: f64) -> (f64, f64);
}
impl MaxByAmp for (f64, f64) {
    fn max_by_amp(self, a: f64, f: f64) -> (f64, f64) {
        if self.1 >= a {
            self
        } else {
            (f, a)
        }
    }
}

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let kind = a.get(1).map(String::as_str).unwrap_or("am").to_string();
    let num = |i: usize, d: f64| a.get(i).and_then(|s| s.parse::<f64>().ok()).unwrap_or(d);
    let (mod_hz, p, att, silent, poll) = (
        num(2, 1000.0),
        num(3, 50.0),
        num(4, -6.0),
        num(5, 3.0),
        num(6, 0.0) > 0.5,
    );
    let mut sc = Scope::open("192.168.10.51:5025")?;
    for c in [
        "ACQW SAMPLING",
        "MSIZ 1.4M",
        "C1:CPL D1M",
        "C1:VDIV 0.005",
        "TDIV 1MS",
        "TRMD AUTO",
        "WFSU SP,1,NP,0,FP,0",
    ] {
        sc.send(c)?;
    }
    let usb = Vnwa::open()?;
    generator::on(&usb, 10e6, att)?;
    std::thread::sleep(Duration::from_secs(2));
    let carrier = std::cell::Cell::new(10e6);
    let measure = |sc: &mut Scope, when: &str| match sc.capture() {
        Ok((v, dt)) => {
            let (rms, depth, f) = analyse(&v, dt, if kind == "am" { mod_hz } else { 0.0 });
            if kind == "fm" {
                if when == "CW before" {
                    let mean = v.iter().sum::<f64>() / v.len() as f64;
                    let m: Vec<f64> = v.iter().map(|x| x - mean).collect();
                    let (f1, _) = peak(&m, dt, 10e6, 3_000.0, 100.0);
                    carrier.set(peak(&m, dt, f1, 100.0, 5.0).0);
                }
                let (beta, err) = fm_fit(&v, dt, mod_hz, carrier.get());
                println!("  scope {when}: FM line pattern fits modulation index {beta:.1} (deviation {:.0} Hz at {mod_hz} Hz), misfit {err:.2}", beta * mod_hz);
            }
            println!("  scope {when}: carrier {rms:.2} mV rms, AM depth from the sidebands {depth:.1} %, modulation frequency {f:.0} Hz ({} points)", v.len());
        }
        Err(e) => println!("  scope {when}: {e}"),
    };
    measure(&mut sc, "CW before");
    let t = Instant::now();
    match kind.as_str() {
        "am" => generator::modulate(
            &usb,
            10e6,
            att,
            Modulation::Am {
                mod_hz,
                depth_pct: p,
            },
        )?,
        "fm" => generator::modulate(
            &usb,
            10e6,
            att,
            Modulation::Fm {
                mod_hz,
                deviation_hz: p,
            },
        )?,
        _ => {}
    }
    println!(
        "{kind} {mod_hz} Hz param {p} at {att} dB: start returned after {:.0} ms",
        t.elapsed().as_secs_f64() * 1000.0
    );
    measure(&mut sc, "0.5 s after the start");
    let end = Instant::now() + Duration::from_secs_f64(silent);
    let (mut polls, mut ok) = (0u32, 0u32);
    while Instant::now() < end {
        if poll {
            polls += 1;
            if usb.read(0x91, 0, 0, 8).is_ok() {
                ok += 1;
            }
            std::thread::sleep(Duration::from_millis(10));
        } else {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    if poll {
        println!("  polled 0x91 {polls} times in {silent} s: {ok} answered");
    }
    measure(
        &mut sc,
        &format!(
            "after {silent} s {}",
            if poll { "of polling" } else { "of silence" }
        ),
    );
    if num(7, 0.0) > 0.5 {
        // A USB reset of the control chip only (no power change): does the modulation or the stall survive it?
        match rusb::Context::new()
            .ok()
            .and_then(|c| c.open_device_with_vid_pid(0x20a0, 0x4118))
        {
            Some(h) => println!("  usb reset of the control chip: {:?}", h.reset()),
            None => println!("  usb reset: cannot open the device"),
        }
        std::thread::sleep(Duration::from_secs(2));
        measure(&mut sc, "after the USB reset");
    }
    if num(8, 0.0) > 0.5 {
        for (req, v) in [
            (0x91u8, 0u16),
            (0x58, 0),
            (0x66, 0),
            (0x0e, 0),
            (0x04, 0x090a),
            (0x3a, 0),
            (0x0d, 0x0303),
            (0x71, 1),
            (0x73, 0),
            (0x60, 1),
            (0x72, 0),
            (0x9e, 0),
            (0x95, 0),
            (0x90, 0),
            (0x00, 0),
            (0xff, 3),
            (0x85, 3),
        ] {
            match usb.read(req, v, if req == 0xff { 0x7dc0 } else { 0 }, 8) {
                Ok(b) => println!("  request {req:#04x} wValue {v:#06x}: answered {b:02x?}"),
                Err(e) => println!(
                    "  request {req:#04x} wValue {v:#06x}: {}",
                    e.chars().take(70).collect::<String>()
                ),
            }
        }
    }
    let first = usb.read(0x91, 0, 0, 8);
    println!(
        "  one read of 0x91: {}",
        match &first {
            Ok(b) => format!("answered {b:02x?}"),
            Err(e) => e.chars().take(80).collect(),
        }
    );
    if first.is_ok() {
        for (r, v) in [(0x58u16, 0u16), (0x66, 0)] {
            println!(
                "  stop step {r:#04x}: {:?}",
                usb.read(r as u8, v, 0, 8).map(|b| b.len())
            );
        }
    }
    measure(&mut sc, "after the stop attempt");
    println!("  resends {}", usb.resends());
    Ok(())
}

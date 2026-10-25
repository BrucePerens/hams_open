// SPDX-License-Identifier: AGPL-3.0-or-later
//! rsp_measure: characterise a running RSP2pro using only its own internal signals (the
//! 24 MHz reference harmonics and the noise floor). Nothing may be connected to the antenna
//! ports; bias-T stays off throughout.
//!
//!     cargo run --release --example rsp_measure [-- --clock-seconds 120]

use rsp_open::dsp::{fft, median};
use rsp_open::{Antenna, Bandwidth, IqI16, Rsp2, SampleIter, Settings};
use std::f64::consts::PI;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const N: usize = 262_144;
const NFFT: usize = 16_384;
const REPEAT: usize = 3;

fn grab(it: &mut SampleIter, skip: usize, n: usize) -> Vec<IqI16> {
    let mut skipped = 0;
    let mut out = Vec::with_capacity(n);
    for c in it.by_ref() {
        if skipped < skip {
            skipped += c.len();
            continue;
        }
        out.extend_from_slice(&c);
        if out.len() >= n {
            out.truncate(n);
            break;
        }
    }
    out
}

/// Linear power spectrum (not dB), Hann window, mean removed, shifted (bin n/2 = 0 Hz),
/// normalised so that a full-scale complex tone integrates to 1.0.
fn spectrum(x: &[IqI16]) -> Vec<f64> {
    let n = NFFT;
    let (mi, mq) = x
        .iter()
        .fold((0.0, 0.0), |(a, b), s| (a + s.i as f64, b + s.q as f64));
    let (mi, mq) = (mi / x.len() as f64, mq / x.len() as f64);
    let win: Vec<f64> = (0..n)
        .map(|k| 0.5 - 0.5 * (2.0 * PI * k as f64 / n as f64).cos())
        .collect();
    let w2: f64 = win.iter().map(|w| w * w).sum();
    let mut acc = vec![0.0; n];
    let segs = x.len() / n;
    let (mut re, mut im) = (vec![0.0; n], vec![0.0; n]);
    for s in 0..segs {
        for k in 0..n {
            let v = x[s * n + k];
            re[k] = (v.i as f64 - mi) / 32768.0 * win[k];
            im[k] = (v.q as f64 - mq) / 32768.0 * win[k];
        }
        fft(&mut re, &mut im);
        for k in 0..n {
            acc[k] += re[k] * re[k] + im[k] * im[k];
        }
    }
    (0..n)
        .map(|k| acc[(k + n / 2) % n] / segs as f64 / (w2 * n as f64))
        .collect()
}

fn db(x: f64) -> f64 {
    10.0 * x.max(1e-30).log10()
}

struct Obs {
    /// Spur power dBFS (integrated over +-4 bins, floor subtracted).
    spur_dbfs: f64,
    /// Noise floor, dBFS per Hz (median bin).
    floor_dbfs_hz: f64,
    mean_i: f64,
    mean_q: f64,
    max_abs: u16,
    clip_frac: f64,
    rms: f64,
}

fn observe(x: &[IqI16], fs: f64, spur_off: Option<f64>) -> Obs {
    let sp = spectrum(x);
    let n = sp.len();
    let bin = fs / n as f64;
    // Floor: median of bins away from DC.
    let away: Vec<f64> = sp
        .iter()
        .enumerate()
        .filter(|(k, _)| ((*k as f64 - (n / 2) as f64) * bin).abs() > 20e3)
        .map(|(_, v)| *v)
        .collect();
    let floor_bin = median(&away);
    let mut spur = f64::NAN;
    if let Some(off) = spur_off {
        let c = (n / 2) as f64 + off / bin;
        let lo = (c - 2_000.0 / bin).max(0.0) as usize;
        let hi = ((c + 2_000.0 / bin) as usize).min(n - 1);
        let k = (lo..=hi)
            .max_by(|&a, &b| sp[a].partial_cmp(&sp[b]).unwrap())
            .unwrap();
        let p: f64 = (k.saturating_sub(4)..=(k + 4).min(n - 1))
            .map(|j| sp[j] - floor_bin)
            .sum();
        spur = db(p.max(1e-30));
    }
    let (si, sq) = x
        .iter()
        .fold((0.0, 0.0), |(a, b), s| (a + s.i as f64, b + s.q as f64));
    let (mi, mq) = (si / x.len() as f64, sq / x.len() as f64);
    let max_abs = x
        .iter()
        .map(|s| s.i.unsigned_abs().max(s.q.unsigned_abs()))
        .max()
        .unwrap_or(0);
    let clip = x
        .iter()
        .filter(|s| s.i.unsigned_abs() >= 32000 || s.q.unsigned_abs() >= 32000)
        .count() as f64
        / x.len() as f64;
    let rms = (x
        .iter()
        .map(|s| (s.i as f64 - mi).powi(2) + (s.q as f64 - mq).powi(2))
        .sum::<f64>()
        / x.len() as f64)
        .sqrt();
    Obs {
        spur_dbfs: spur,
        floor_dbfs_hz: db(floor_bin / bin),
        mean_i: mi,
        mean_q: mq,
        max_abs,
        clip_frac: clip,
        rms,
    }
}

fn stats(v: &[f64]) -> (f64, f64) {
    let m = v.iter().sum::<f64>() / v.len() as f64;
    let s = (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len().max(2) - 1) as f64).sqrt();
    (m, s)
}

/// Repeat a capture REPEAT times; return (spur mean, spur sd, floor mean, floor sd, last obs).
fn rep(rx: &Rsp2, it: &mut SampleIter, spur_off: Option<f64>) -> (f64, f64, f64, f64, Obs) {
    let mut sp = Vec::new();
    let mut fl = Vec::new();
    let mut last = None;
    for _ in 0..REPEAT {
        let x = grab(it, 60_000, N);
        let o = observe(&x, rx.output_rate(), spur_off);
        sp.push(o.spur_dbfs);
        fl.push(o.floor_dbfs_hz);
        last = Some(o);
    }
    let (a, b) = stats(&sp);
    let (c, d) = stats(&fl);
    (a, b, c, d, last.unwrap())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let clock_s: f64 = args
        .iter()
        .position(|a| a == "--clock-seconds")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(120.0);

    let mut rx = Rsp2::open_with(Settings {
        rf_hz: 95_800_000,
        if_gr_db: 40,
        bandwidth: Bandwidth::K1536,
        ..Settings::default()
    })?;
    println!(
        "# rsp_measure on RSP2pro serial {:?} (request 0x48 returned 0x{:02x})",
        rx.serial, rx.read48
    );
    println!("Levels are dBFS (full scale = 0 dB for a complex tone); floor is dBFS/Hz. +- = 1 sd over {REPEAT} captures of {N} samples.\n");
    let mut it = rx.start_stream_iter(512)?;

    // 1. LNA-state steps per band, IF GR 40, Antenna A, using the reference harmonic.
    println!("## 1. LNA-state steps (IF GR 40 dB, Antenna A)\n");
    let spur_points: [(&str, u64, f64); 8] = [
        ("HF AM 24 MHz", 23_800_000, 200e3),
        ("HF AM 48 MHz", 47_800_000, 200e3),
        ("VHF 96 MHz", 95_800_000, 200e3),
        ("B3 144 MHz", 143_800_000, 200e3),
        ("AM-down 264 MHz", 264_200_000, -200e3),
        ("B4/5 480 MHz", 479_800_000, 200e3),
        ("L 1200 MHz", 1_199_800_000, 200e3),
        ("L 1800 MHz", 1_799_800_000, 200e3),
    ];
    for (name, rf, off) in spur_points {
        rx.set_frequency(rf)?;
        println!(
            "{name} (tuned {:.3} MHz): state: spur dBFS / floor dBFS/Hz",
            rf as f64 / 1e6
        );
        let mut s0 = None;
        for l in 0..rx.lna_states() {
            rx.set_gain(40, l)?;
            let (sm, ss, fm, fs_, _) = rep(&rx, &mut it, Some(off));
            let d = s0.map(|x: f64| sm - x).unwrap_or(0.0);
            s0.get_or_insert(sm);
            println!(
                "  LNA {l}: spur {sm:7.2} +-{ss:4.2} (rel {d:+6.2})  floor {fm:7.2} +-{fs_:4.2}"
            );
        }
        rx.set_gain(40, 0)?;
    }

    // 2. IF gain reduction linearity at 95.8 MHz.
    println!("\n## 2. IF gain reduction steps (95.8 MHz, LNA 0)\n");
    rx.set_frequency(95_800_000)?;
    let mut prev: Option<f64> = None;
    for gr in [20u8, 25, 30, 35, 40, 45, 50, 55, 59] {
        rx.set_gain(gr, 0)?;
        let (sm, ss, fm, _, o) = rep(&rx, &mut it, Some(200e3));
        let d = prev.map(|p| sm - p).unwrap_or(0.0);
        prev = Some(sm);
        println!("  GR {gr:2}: spur {sm:7.2} +-{ss:4.2} (step {d:+6.2})  floor {fm:7.2}  rms {:7.1}  max|x| {}  DC ({:+.1}, {:+.1})", o.rms, o.max_abs, o.mean_i, o.mean_q);
    }
    rx.set_gain(40, 0)?;

    // 3. Band-switch edges: floor (and harmonic where one is near) just below and above.
    println!("\n## 3. Band-switch edges (IF GR 40, LNA 0): floor below/above, harmonic levels\n");
    for (edge, note) in [
        (12_000_000u64, ""),
        (30_000_000, ""),
        (60_000_000, ""),
        (120_000_000, "5 x 24 MHz at the edge"),
        (250_000_000, ""),
        (300_000_000, ""),
        (1_000_000_000, ""),
    ] {
        let mut row = String::new();
        for rf in [edge - 300_000, edge + 300_000] {
            rx.set_frequency(rf)?;
            let (_, _, fm, fsd, _) = rep(&rx, &mut it, None);
            row += &format!("  {:9.3} MHz floor {fm:7.2} +-{fsd:4.2}", rf as f64 / 1e6);
        }
        println!("{:7.1} MHz:{row} {note}", edge as f64 / 1e6);
    }
    for (rf, off) in [(119_800_000u64, 200e3), (120_200_000, -200e3)] {
        rx.set_frequency(rf)?;
        let (sm, ss, _, _, _) = rep(&rx, &mut it, Some(off));
        println!(
            "  120 MHz harmonic seen from {:.1} MHz: {sm:7.2} +-{ss:4.2} dBFS",
            rf as f64 / 1e6
        );
    }
    println!("  (375-420 MHz: not measured; this driver does not implement the 144/168 MHz first-IF mode)");

    // 4. Harmonic survey: every k x 24 MHz (k = 1..83) tuned 200 kHz below.
    println!("\n## 4. Reference-harmonic survey (tuned k*24 MHz - 200 kHz; IF GR 40, LNA 0)\n");
    let mut line = String::new();
    for k in 1..=83u64 {
        let h = k * 24_000_000;
        if (375_000_000..420_000_000 + 200_000).contains(&h) {
            continue;
        }
        if rx.set_frequency(h - 200_000).is_err() {
            continue;
        }
        let x = grab(&mut it, 60_000, N);
        let o = observe(&x, rx.output_rate(), Some(200e3));
        line += &format!(
            "{:5}:{:6.1} ",
            h / 1_000_000,
            o.spur_dbfs - (o.floor_dbfs_hz + 10.0 * (rx.output_rate() / NFFT as f64).log10())
        );
        if k % 6 == 0 {
            println!("{line}");
            line.clear();
        }
    }
    println!("{line}\n  (values: harmonic MHz: spur level over one-bin floor, dB)");

    // 5. Notch on/off.
    println!("\n## 5. Broadcast notch (IF GR 40, LNA 0)\n");
    for (rf, off, what) in [
        (95_800_000u64, Some(200e3), "FM band, 96 MHz harmonic"),
        (1_000_000, None, "MW 1 MHz floor"),
        (23_800_000, Some(200e3), "HF 24 MHz harmonic"),
    ] {
        rx.set_frequency(rf)?;
        let mut r = String::new();
        for on in [false, true] {
            rx.set_notch(on)?;
            let (sm, ss, fm, fsd, _) = rep(&rx, &mut it, off);
            r += &format!(
                "  notch {}: spur {sm:7.2} +-{ss:4.2} floor {fm:7.2} +-{fsd:4.2}",
                if on { "on " } else { "off" }
            );
        }
        rx.set_notch(false)?;
        println!("{what}:{r}");
    }

    // 6. Antenna A / B / Hi-Z floors.
    println!("\n## 6. Antenna ports, nothing attached (IF GR 40, LNA 0)\n");
    for rf in [
        1_000_000u64,
        10_000_000,
        30_000_000,
        100_000_000,
        446_000_000,
    ] {
        rx.set_frequency(rf)?;
        let mut r = String::new();
        for ant in [Antenna::A, Antenna::B, Antenna::HiZ] {
            if ant == Antenna::HiZ && rf >= 60_000_000 {
                continue;
            }
            rx.select_antenna(ant)?;
            let (_, _, fm, fsd, _) = rep(&rx, &mut it, None);
            r += &format!("  {ant:?}: {fm:7.2} +-{fsd:4.2}");
        }
        rx.select_antenna(Antenna::A)?;
        println!("{:8.3} MHz floor dBFS/Hz:{r}", rf as f64 / 1e6);
    }

    // 7. ADC: floor at maximum gain reduction, per format; DC and clipping at minimum GR.
    println!("\n## 7. ADC noise and DC/clipping\n");
    rx.set_frequency(100_000_000)?;
    let maxl = rx.lna_states() - 1;
    for (gr, l) in [(59u8, maxl), (59, 0), (20, 0), (0, 0)] {
        rx.set_gain(gr, l)?;
        let x = grab(&mut it, 60_000, N);
        let o = observe(&x, rx.output_rate(), None);
        // ENOB estimate: SNR of a full-scale complex tone against the total in-band noise.
        let noise_dbfs = o.floor_dbfs_hz + 10.0 * rx.output_rate().log10();
        println!(
            "  100 MHz GR {gr:2} LNA {l}: floor {:7.2} dBFS/Hz, total noise {noise_dbfs:6.1} dBFS (ENOB bound {:4.1} bits), rms {:7.1} LSB16, DC ({:+.1}, {:+.1}) LSB16, max|x| {}, clipped {:.2e}",
            o.floor_dbfs_hz, (-noise_dbfs - 1.76) / 6.02, o.rms, o.mean_i, o.mean_q, o.max_abs, o.clip_frac
        );
    }
    rx.set_gain(40, 0)?;
    drop(it);
    rx.stop_stream()?;
    rx.set_gain(59, maxl)?;
    for fs in [
        2_000_000u32,
        2_048_000,
        6_000_000,
        8_000_000,
        9_000_000,
        10_000_000,
    ] {
        rx.set_sample_rate(fs)?;
        let mut it = rx.start_stream_iter(512)?;
        let x = grab(&mut it, 100_000, N);
        drop(it);
        rx.stop_stream()?;
        let o = observe(&x, rx.output_rate(), None);
        let noise_dbfs = o.floor_dbfs_hz + 10.0 * rx.output_rate().log10();
        println!(
            "  ADC {:6.3} MHz {:?} (GR 59, LNA max): floor {:7.2} dBFS/Hz, total {noise_dbfs:6.1} dBFS, ENOB bound {:4.1}, rms {:6.1} LSB16",
            fs as f64 / 1e6, rx.stream_config().format, o.floor_dbfs_hz, (-noise_dbfs - 1.76) / 6.02, o.rms
        );
    }
    rx.set_sample_rate(2_000_000)?;
    rx.set_gain(40, 0)?;

    // 8. Sample clock against the host clock (NTP-disciplined CLOCK_MONOTONIC).
    println!("\n## 8. Sample clock vs host clock ({clock_s:.0} s)\n");
    let pts: Arc<Mutex<Vec<(f64, f64)>>> = Arc::new(Mutex::new(Vec::new()));
    let p2 = pts.clone();
    let t0 = Instant::now();
    let mut nth = 0u64;
    rx.start_stream(move |_, st| {
        nth += 1;
        if nth.is_multiple_of(50) {
            p2.lock().unwrap().push((
                t0.elapsed().as_secs_f64(),
                (st.samples + st.lost_samples) as f64,
            ));
        }
    })?;
    std::thread::sleep(std::time::Duration::from_secs_f64(clock_s));
    rx.stop_stream()?;
    let st = rx.stream_stats();
    let v = pts.lock().unwrap().clone();
    let v: Vec<(f64, f64)> = v.into_iter().filter(|(t, _)| *t > 2.0).collect();
    let n = v.len() as f64;
    let (mt, ms) = v.iter().fold((0.0, 0.0), |(a, b), (t, s)| (a + t, b + s));
    let (mt, ms) = (mt / n, ms / n);
    let sxx: f64 = v.iter().map(|(t, _)| (t - mt).powi(2)).sum();
    let sxy: f64 = v.iter().map(|(t, s)| (t - mt) * (s - ms)).sum();
    let slope = sxy / sxx;
    let resid: f64 = v
        .iter()
        .map(|(t, s)| (s - ms - slope * (t - mt)).powi(2))
        .sum::<f64>()
        / (n - 2.0);
    let se = (resid / sxx).sqrt();
    let ppm = (slope / rx.output_rate() - 1.0) * 1e6;
    println!(
        "  {} points, slope {:.3} samples/s, sample clock {ppm:+.2} ppm vs host (+-{:.2} ppm 1-sigma from the fit; host clock error not included), gaps {}",
        v.len(), slope, se / rx.output_rate() * 1e6, st.gaps
    );

    rx.close()?;
    println!("\nDone; receiver closed (tuner standby, bias-T off, alt 0).");
    Ok(())
}

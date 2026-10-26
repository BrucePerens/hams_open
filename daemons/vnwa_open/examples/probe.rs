// SPDX-License-Identifier: AGPL-3.0-or-later
//! Probe of a connected VNWA: lists the audio inputs, sends one set-up block and measures the tone at the IF on each
//! codec. `cargo run --example probe -- [freq_hz] [if_hz] [point_ms] [rf_level] [codec-name-part]`.

use std::time::Duration;

use vnwa_open::audio::{measure_point, tone_at_if, Reference};
use vnwa_open::capture::{input_names, Capture, SAMPLE_RATE};
use vnwa_open::setup::{setup_block, SetupParams};
use vnwa_open::usb::Vnwa;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize, d: f64| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (freq, if_hz, point_ms, level) = (
        num(1, 10e6),
        num(2, 12000.0),
        num(3, 1.333),
        num(4, 16383.0) as u16,
    );
    let codec = a.get(5).cloned().unwrap_or_else(|| "CODEC".into());

    println!("audio inputs:");
    for n in input_names()? {
        println!("  {n}");
    }
    let usb = Vnwa::open()?;
    let cap = Capture::open(&codec)?;
    let block = setup_block(&SetupParams {
        freq_hz: freq,
        previous_hz: freq,
        n_rf: 10,
        n_lo: 9,
        if_hz,
        point_ms,
        counter: 3,
        mode: 0,
        rf_level: level,
    })?;
    let mark = cap.mark();
    // The vendor program queries the level (0x58) before each block and reads 0x0d (wValue 0x0303) after it.
    println!("0x58 -> {:02x?}", usb.read(0x58, level, level, 64));
    usb.send_all(&block)?;
    let t = std::time::Instant::now();
    let r = usb.read(0x0d, 0x0303, 0, 64);
    println!("0x0d -> {r:02x?} after {:?}", t.elapsed());
    let frames = cap.frames_after(mark, 0, SAMPLE_RATE as usize, Duration::from_secs(5))?;
    for (i, w) in frames.chunks(480).enumerate().take(60) {
        let rms = |c: usize| {
            (w.iter().map(|f| (f[c] as f64).powi(2)).sum::<f64>() / w.len() as f64).sqrt()
        };
        if rms(0) > 20.0 || rms(1) > 20.0 {
            println!(
                "  t={:>4} ms  rms left {:8.1}  right {:8.1}",
                i * 10,
                rms(0),
                rms(1)
            );
        }
    }
    let fsr = SAMPLE_RATE as f64;
    for (i, w) in frames.chunks(960).enumerate().take(20) {
        let l: Vec<f64> = w.iter().map(|f| f[0] as f64).collect();
        let r: Vec<f64> = w.iter().map(|f| f[1] as f64).collect();
        for (name, reference) in [
            ("ref right", Reference::Right),
            ("ref left", Reference::Left),
        ] {
            if let Ok(c) = measure_point(&l, &r, fsr, if_hz, 0, 0, reference, 100.0) {
                let v = vnwa_open::audio::vendor_convention(c);
                print!(
                    "  t={:>3} ms {name}: {:.4} @ {:7.2} deg |",
                    i * 20,
                    v.abs(),
                    v.phase_deg()
                );
            }
        }
        println!();
    }
    let left: Vec<f64> = frames.iter().map(|f| f[0] as f64).collect();
    let right: Vec<f64> = frames.iter().map(|f| f[1] as f64).collect();
    let fs = SAMPLE_RATE as f64;
    let (l, r) = (tone_at_if(&left, fs, if_hz), tone_at_if(&right, fs, if_hz));
    println!(
        "{freq} Hz, IF {if_hz} Hz: left {:.1} @ {:.1} deg, right {:.1} @ {:.1} deg",
        l.abs(),
        l.phase_deg(),
        r.abs(),
        r.phase_deg()
    );
    let rms = |v: &[f64]| (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt();
    println!("rms left {:.1}, right {:.1}", rms(&left), rms(&right));
    if let Ok(s21) = measure_point(&left, &right, fs, if_hz, 0, 0, Reference::Right, 1.0) {
        println!(
            "ratio left/right: {:.4} @ {:.2} deg",
            s21.abs(),
            s21.phase_deg()
        );
    }
    Ok(())
}

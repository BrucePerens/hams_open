// SPDX-License-Identifier: AGPL-3.0-or-later
//! `short_check [start_hz] [stop_hz] [points]` (default 100 kHz to 150 MHz, 30 points): the S11 check with an SMA male short. The unit's
//! TX port (SMA female) is measured bare (an open), then with the short screwed straight on; the ratio short / open is the response-
//! calibrated reflection of the short, which should be close to magnitude 1 and phase 180 degrees. Nothing else is connected to the TX
//! port; the RX port is not used.
use std::io::BufRead;
use vnwa_open::audio::Complex;
use vnwa_open::sweep::{Config, Vna};

fn wait(msg: &str) {
    println!("{msg} then press Enter");
    let _ = std::io::stdin().lock().lines().next();
}

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize, d: f64| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (start, stop, n) = (num(1, 100e3), num(2, 150e6), num(3, 30.0) as usize);
    let cfg = Config {
        if_hz: 4000.0,
        ..Config::default()
    };
    wait("Remove everything from the VNA's TX port (the bare port is the open)");
    let mut vna = Vna::open(cfg.clone())?;
    let open = vna.sweep(start, stop, n)?;
    drop(vna);
    wait("Screw the SMA male short onto the TX port");
    let mut vna = Vna::open(cfg)?;
    let short = vna.sweep(start, stop, n)?;
    println!(
        "{:>12}  {:>9}  {:>10}",
        "freq_hz", "|short/open|", "phase deg"
    );
    let (mut worst_mag, mut worst_ph) = (0.0f64, 0.0f64);
    for (o, s) in open.iter().zip(&short) {
        let r: Complex = s.s11 / o.s11;
        let ph = r.phase_deg();
        let dev = (ph.abs() - 180.0).abs();
        worst_mag = worst_mag.max((r.abs() - 1.0).abs() * 100.0);
        worst_ph = worst_ph.max(dev);
        println!("{:>12.0}  {:>11.4}  {:>10.2}", s.freq_hz, r.abs(), ph);
    }
    println!("worst: magnitude {worst_mag:.2} % from 1, phase {worst_ph:.2} degrees from 180");
    println!("(a real short against a real open differs from the ideal by the open's fringing capacitance and the short's inductance; expect a few percent and a few degrees at 150 MHz)");
    Ok(())
}

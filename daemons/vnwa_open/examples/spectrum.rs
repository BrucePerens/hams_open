// SPDX-License-Identifier: AGPL-3.0-or-later
//! `spectrum <start_hz> <stop_hz> <points> <band_hz> [tracking_level]`: the unit as a rudimentary spectrum analyzer (uncalibrated, counts): prints each LO frequency
//! with the rms of the audio within the band. With a tracking level above 0 the TX chip runs 12 kHz below the LO (a loopback then shows a line at 12 kHz).
use vnwa_open::sweep::{Config, Vna};

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize| {
        a.get(i)
            .and_then(|s| s.parse::<f64>().ok())
            .ok_or_else(|| format!("argument {i}: a number"))
    };
    let (start, stop, points, band) = (num(1)?, num(2)?, num(3)? as usize, num(4)?);
    let tracking = a.get(5).and_then(|s| s.parse::<u16>().ok()).unwrap_or(0);
    let mut vna = Vna::open(Config {
        if_hz: 12_000.0,
        ..Config::default()
    })?;
    let t = std::time::Instant::now();
    let sp = vna.spectrum(start, stop, points, band, tracking)?;
    for p in &sp {
        println!("{:.0} {:.2}", p.freq_hz, p.rms);
    }
    eprintln!(
        "{} points in {:.1} s; recovery {:?}",
        sp.len(),
        t.elapsed().as_secs_f64(),
        vna.recovery()
    );
    Ok(())
}

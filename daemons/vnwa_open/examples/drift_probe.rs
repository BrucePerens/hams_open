// SPDX-License-Identifier: AGPL-3.0-or-later
//! `drift_probe <minutes> <interval_s> [cold]`: sweeps 100 kHz to 150 MHz in 60 points every interval and prints a CSV line each time: elapsed seconds,
//! the unit's temperature counts, and the mean signed change of |S11| and |S21| (percent) and of their phases (degrees) against the FIRST sweep of the run.
//! With `cold` the unit is power cycled first, so the first sweep is the first one after power-up.
use std::time::{Duration, Instant};

use vnwa_open::audio::Complex;
use vnwa_open::sweep::{agreement, Config, Vna};

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let minutes: f64 = a
        .get(1)
        .and_then(|s| s.parse().ok())
        .ok_or("usage: drift_probe <minutes> <interval_s> [cold]")?;
    let interval: f64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(30.0);
    if a.iter().any(|s| s == "cold") {
        vnwa_open::power::cold_reset(&vnwa_open::power::HubPort {
            hub: "3-2".into(),
            port: 2,
        })?;
    }
    let mut vna = Vna::open(Config {
        if_hz: 4000.0,
        ..Config::default()
    })?;
    let fw = vna.firmware_version()?;
    println!("# firmware v{}.{}", fw.0, fw.1);
    println!("# seconds,temperature_counts,s11_mag_pct,s11_phase_deg,s21_mag_pct,s21_phase_deg,remeasured,overruns");
    let start = Instant::now();
    let mut base = None;
    while start.elapsed() < Duration::from_secs_f64(minutes * 60.0) {
        let t0 = Instant::now();
        let temp = vna.temperature_counts()?;
        let sw = vna.sweep(100e3, 150e6, 60)?;
        let b = base.get_or_insert_with(|| sw.clone());
        let mean = |f: &dyn Fn(&vnwa_open::sweep::Point) -> Complex| -> (f64, f64) {
            let (mut m, mut p) = (0.0, 0.0);
            for (x, y) in b.iter().zip(&sw) {
                let (dm, dp) = agreement(f(x), f(y));
                m += dm;
                p += dp;
            }
            (m / sw.len() as f64, p / sw.len() as f64)
        };
        let (m11, p11) = mean(&|p| p.s11);
        let (m21, p21) = mean(&|p| p.s21);
        let r = vna.recovery();
        println!(
            "{:.0},{temp},{m11:+.3},{p11:+.3},{m21:+.3},{p21:+.3},{},{}",
            start.elapsed().as_secs_f64(),
            r.remeasured,
            r.overruns_reported
        );
        let left = Duration::from_secs_f64(interval).saturating_sub(t0.elapsed());
        std::thread::sleep(left);
    }
    Ok(())
}

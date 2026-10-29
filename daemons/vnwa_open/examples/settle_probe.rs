// SPDX-License-Identifier: AGPL-3.0-or-later
//! `settle_probe <trials> <discards> <first_wait_ms> <idle_s>`: each trial waits `idle_s` seconds with the unit idle, then sweeps 12 points from 100 kHz to
//! 150 MHz with `discards` throw-away points and a wait first, then measures 100 kHz again. Prints the first point's error against that repeat
//! (percent, degrees) and the attempt counts, to see whether a settle step helps.
use vnwa_open::sweep::{agreement, Config, Vna};

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let n = |i: usize, d: f64| a.get(i).and_then(|s| s.parse::<f64>().ok()).unwrap_or(d);
    let (trials, discards, wait, idle) =
        (n(1, 20.0) as usize, n(2, 0.0) as u32, n(3, 0.0), n(4, 10.0));
    let mut vna = Vna::open(Config {
        if_hz: 4000.0,
        discard_points: discards,
        first_wait_ms: wait,
        ..Config::default()
    })?;
    let (mut e_mag, mut e_ph) = (Vec::new(), Vec::new());
    let t0 = std::time::Instant::now();
    for _ in 0..trials {
        std::thread::sleep(std::time::Duration::from_secs_f64(idle));
        let sw = vna.sweep(100e3, 150e6, 12)?;
        let again = vna.measure(100e3, 100e3)?;
        for (x, y) in [(sw[0].s21, again.s21), (sw[0].s11, again.s11)] {
            let (m, p) = agreement(y, x);
            e_mag.push(m);
            e_ph.push(p);
        }
    }
    let t = vna.attempt_timings();
    let bad = t.iter().filter(|x| !x.1).count();
    let rms = |v: &[f64]| (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt();
    println!(
        "discards {discards} wait {wait} ms idle {idle} s: {} attempts, {bad} failed first try ({:.2} %); first point vs repeat: rms {:.3} % {:.3} deg, worst {:.3} % {:.3} deg; {:.0} s total",
        t.len(),
        100.0 * bad as f64 / t.len() as f64,
        rms(&e_mag),
        rms(&e_ph),
        e_mag.iter().fold(0.0f64, |a, b| a.max(b.abs())),
        e_ph.iter().fold(0.0f64, |a, b| a.max(b.abs())),
        t0.elapsed().as_secs_f64()
    );
    for f in vna.failure_log().iter().take(40) {
        println!(
            "  failure: {}",
            f.attempts
                .join(" ;; ")
                .chars()
                .take(260)
                .collect::<String>()
        );
    }
    Ok(())
}

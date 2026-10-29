// SPDX-License-Identifier: AGPL-3.0-or-later
//! `meter <center_hz> [point_ms] [repeats]`: the frequency meter on whatever is connected to the RX input (the unit's own TX through a
//! cable gives an offset of zero).
use vnwa_open::sweep::{Config, Vna};

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize, d: f64| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (f, ms, n) = (num(1, 10e6), num(2, 60.0), num(3, 1.0) as usize);
    let mut vna = Vna::open(Config {
        if_hz: 4000.0,
        ..Config::default()
    })?;
    for _ in 0..n {
        let r = vna.meter(f, ms)?;
        println!(
            "centre {:.1} Hz: offset {:+.5} Hz (codec 1 {:+.5}, codec 2 {:+.5}, se {:.5}) over {:.2} s; TX word {:.4} Hz; reading {:.4} Hz",
            r.center_hz, r.offset_hz, r.offset_s11_hz, r.offset_s21_hz, r.stderr_hz, r.seconds, r.tx_actual_hz, r.frequency_hz()
        );
    }
    Ok(())
}

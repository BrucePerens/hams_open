// SPDX-License-Identifier: AGPL-3.0-or-later
//! `attempt_timing <sweeps>`: sweeps 100 kHz to 150 MHz in 60 points and prints, for succeeded and failed attempts, how long the set-up block
//! and the 0x0d read took to send. A failed attempt that follows a slow send says the analysis window was placed from a wrong instant.
use vnwa_open::sweep::{Config, Vna};

fn main() -> Result<(), String> {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("usage: attempt_timing <sweeps>")?;
    let mut vna = Vna::open(Config {
        if_hz: 4000.0,
        ..Config::default()
    })?;
    for _ in 0..n {
        vna.sweep(100e3, 150e6, 60)?;
    }
    let t = vna.attempt_timings();
    let stat = |ok: bool| {
        let mut v: Vec<f64> = t.iter().filter(|x| x.1 == ok).map(|x| x.0).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = |f: f64| {
            v.get(((v.len() as f64 - 1.0) * f) as usize)
                .copied()
                .unwrap_or(f64::NAN)
        };
        println!(
            "{} attempts {}: send ms min {:.1} median {:.1} 90% {:.1} 99% {:.1} max {:.1}",
            v.len(),
            if ok { "ok" } else { "FAILED" },
            q(0.0),
            q(0.5),
            q(0.9),
            q(0.99),
            q(1.0)
        );
    };
    stat(true);
    stat(false);
    for (ms, _) in t.iter().filter(|x| !x.1).take(30) {
        println!("failed attempt: send {ms:.1} ms");
    }
    println!("{:?}", vna.recovery());
    Ok(())
}

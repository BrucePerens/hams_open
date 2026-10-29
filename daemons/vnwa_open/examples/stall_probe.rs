// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stall_probe <sweeps> <min_gap_ms> [pause_ms ...]`: sweeps 100 kHz to 150 MHz in 60 points and counts the attempts and how many failed at the first try, with the 0x0d read
//! retried after the given pauses and a minimum time between sends. Prints the failures' requests and errors.
use vnwa_open::sweep::{Config, Vna};

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let sweeps: usize = a
        .get(1)
        .and_then(|s| s.parse().ok())
        .ok_or("usage: stall_probe <sweeps> <min_gap_ms> [pause_ms ...]")?;
    let gap: f64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let pauses: Vec<u64> = a.iter().skip(3).filter_map(|s| s.parse().ok()).collect();
    let mut vna = Vna::open(Config {
        if_hz: 4000.0,
        min_gap_ms: gap,
        stall_pauses_ms: pauses.clone(),
        ..Config::default()
    })?;
    let t0 = std::time::Instant::now();
    for _ in 0..sweeps {
        vna.sweep(100e3, 150e6, 60)?;
    }
    let t = vna.attempt_timings();
    let bad = t.iter().filter(|x| !x.1).count();
    let r = vna.recovery();
    println!(
        "min_gap {gap} ms pauses {pauses:?}: {} attempts, {bad} failed first try ({:.2} %), {} resends, {:.0} s ({:.0} ms per point)",
        t.len(),
        100.0 * bad as f64 / t.len() as f64,
        r.transfer_resends,
        t0.elapsed().as_secs_f64(),
        1000.0 * t0.elapsed().as_secs_f64() / (sweeps * 60) as f64
    );
    let mut kinds = std::collections::BTreeMap::new();
    for f in vna.failure_log() {
        for att in &f.attempts {
            let head: String = att.chars().take(95).collect();
            *kinds.entry(head).or_insert(0) += 1;
        }
    }
    for (k, v) in kinds {
        println!("  {v} x {k}");
    }
    Ok(())
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! `cargo run --example sweep -- start_hz stop_hz points [if_hz] [point_ms] [settle_ms] [window_ms]`: an uncalibrated S11 and S21 sweep.
use vnwa_open::sweep::{Config, Vna};

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize, d: f64| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let cfg = Config {
        if_hz: num(4, 12000.0),
        point_ms: num(5, 60.0),
        settle_ms: a.get(6).and_then(|s| s.parse().ok()),
        window_ms: a.get(7).and_then(|s| s.parse().ok()),
        // VNWA_HUB=3-2:2 names the power-switching hub port, so a point that cannot be recovered gets a cold reset.
        hub: std::env::var("VNWA_HUB").ok().and_then(|h| {
            let (hub, port) = h.split_once(':')?;
            Some(vnwa_open::power::HubPort {
                hub: hub.to_string(),
                port: port.parse().ok()?,
            })
        }),
        ..Config::default()
    };
    let mut vna = Vna::open(cfg)?;
    for p in vna.sweep(num(1, 10e6), num(2, 10e6), num(3, 1.0) as usize)? {
        println!(
            "{:>12.0} Hz  S21 {:.4} @ {:8.2} deg   S11 {:.4} @ {:8.2} deg",
            p.freq_hz,
            p.s21.abs(),
            p.s21.phase_deg(),
            p.s11.abs(),
            p.s11.phase_deg()
        );
    }
    let r = vna.recovery();
    eprintln!(
        "recovery: {} remeasured, {} transfer resends, {} power cycles",
        r.remeasured, r.transfer_resends, r.power_cycles
    );
    Ok(())
}

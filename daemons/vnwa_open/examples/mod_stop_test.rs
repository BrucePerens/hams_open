// SPDX-License-Identifier: AGPL-3.0-or-later
//! `mod_stop_test <rounds>`: starts AM and FM at assorted settings, waits, ends the modulation with `generator::end_modulation` (a USB reset), then checks that
//! the unit answers and still sweeps (one point at 10 MHz). Prints one line per round and the totals. Nothing is power cycled.
use vnwa_open::generator::{self, Modulation};
use vnwa_open::sweep::{Config, Vna};
use vnwa_open::usb::Vnwa;

fn main() -> Result<(), String> {
    let rounds: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let settings = [
        Modulation::Am {
            mod_hz: 1000.0,
            depth_pct: 50.0,
        },
        Modulation::Fm {
            mod_hz: 1000.0,
            deviation_hz: 10_000.0,
        },
        Modulation::Am {
            mod_hz: 3000.0,
            depth_pct: 100.0,
        },
        Modulation::Fm {
            mod_hz: 500.0,
            deviation_hz: 5_000.0,
        },
        Modulation::Am {
            mod_hz: 50.0,
            depth_pct: 10.0,
        },
    ];
    let offset: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut bad = 0;
    for i in 0..rounds {
        let m = settings[(i + offset) % settings.len()];
        let usb = Vnwa::open()?;
        generator::on(&usb, 10e6, -20.0)?;
        generator::modulate(&usb, 10e6, -20.0, m)?;
        std::thread::sleep(std::time::Duration::from_secs(1 + (i as u64 % 3) * 5));
        let stalled = !generator::answers(&usb);
        let t = std::time::Instant::now();
        drop(usb);
        let ended = generator::end_modulation();
        let ended_in = t.elapsed().as_secs_f64();
        std::thread::sleep(std::time::Duration::from_secs(2));
        let sweep = Vna::open(Config {
            if_hz: 4000.0,
            ..Config::default()
        })
        .and_then(|mut v| v.sweep(10e6, 10e6, 1));
        let ok = stalled && ended.is_ok() && sweep.is_ok();
        if !ok {
            bad += 1;
        }
        println!(
            "round {i}: {m:?}: stalled while modulating {stalled}; end_modulation {} in {ended_in:.1} s; sweep after: {}",
            match &ended { Ok(()) => "ok".to_string(), Err(e) => e.clone() },
            match &sweep { Ok(p) => format!("ok, |S21| {:.3}", p[0].s21.abs()), Err(e) => e.chars().take(80).collect() }
        );
    }
    println!("{} rounds, {bad} not clean", rounds);
    Ok(())
}

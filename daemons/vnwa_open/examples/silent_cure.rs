// SPDX-License-Identifier: AGPL-3.0-or-later
//! `silent_cure`: with the unit in its "silent state" (reference channels read about 0.1 count), tries a USB reset of the control chip (no power change) and measures one point again.
use vnwa_open::sweep::{Config, Vna};
use vnwa_open::usb::Vnwa;

fn main() -> Result<(), String> {
    let cfg = Config {
        if_hz: 4000.0,
        retries: 0,
        overrun_retries: 3,
        ..Config::default()
    };
    let before = Vna::open(cfg.clone())?.sweep(10e6, 10e6, 1);
    println!(
        "before: {}",
        match &before {
            Ok(p) => format!("ok |S21| {:.3}", p[0].s21.abs()),
            Err(e) => e.chars().take(110).collect(),
        }
    );
    let usb = Vnwa::open()?;
    println!("usb reset: {:?}", usb.reset_device());
    drop(usb);
    for i in 1..=3 {
        std::thread::sleep(std::time::Duration::from_secs(4));
        match Vna::open(cfg.clone()).and_then(|mut v| v.sweep(10e6, 10e6, 1)) {
            Ok(p) => {
                println!("after reset, try {i}: ok |S21| {:.3}", p[0].s21.abs());
                return Ok(());
            }
            Err(e) => println!(
                "after reset, try {i}: {}",
                e.chars().take(110).collect::<String>()
            ),
        }
    }
    Ok(())
}

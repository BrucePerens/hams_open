// SPDX-License-Identifier: AGPL-3.0-or-later
//! `power_cycle <hub> <port>`: cold-resets the unit through its power-switching hub port (`sudo -n uhubctl`), waits for the
//! unit and its codecs, and prints which ALSA card is S11 and which is S21 afterwards.
use vnwa_open::power::{cold_reset, HubPort};

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let port = HubPort {
        hub: a.get(1).ok_or("usage: power_cycle <hub> <port>")?.clone(),
        port: a
            .get(2)
            .and_then(|p| p.parse().ok())
            .ok_or("port must be a number")?,
    };
    let started = std::time::Instant::now();
    cold_reset(&port)?;
    let c = vnwa_open::topology::codecs()?;
    println!(
        "back after {:.1} s: S11 {} S21 {}",
        started.elapsed().as_secs_f64(),
        c.s11,
        c.s21
    );
    Ok(())
}

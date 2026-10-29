// SPDX-License-Identifier: AGPL-3.0-or-later
//! `meter_probe <freq_hz> <counter> <point_ms>`: sends one set-up block with the given step counter at a fixed frequency (the vendor's
//! frequency meter uses counter 31) and prints the reference tone's level in 100 ms slices for three seconds, to see how long the
//! unit emits.
use vnwa_open::audio::tone_at_if;
use vnwa_open::capture::{Capture, SAMPLE_RATE};
use vnwa_open::setup::{setup_block, SetupParams};
use vnwa_open::usb::Vnwa;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize, d: f64| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (f, counter, ms) = (num(1, 10e6), num(2, 31.0) as u8, num(3, 60.0));
    let c = vnwa_open::topology::codecs()?;
    let cap = Capture::open(&c.s11)?;
    let usb = Vnwa::open()?;
    let block = setup_block(&SetupParams {
        freq_hz: f,
        previous_hz: f,
        n_rf: 10,
        n_lo: 9,
        if_hz: 4000.0,
        point_ms: ms,
        counter,
        mode: 0,
        rf_level: 0x3fff,
    })?;
    usb.read(0x58, 0x3fff, 0x3fff, 64)?;
    let m = cap.mark();
    usb.send_all(&block)?;
    usb.read(0x0d, 0x0303, 0, 64)?;
    let fr = cap.frames_after(
        m,
        0,
        3 * SAMPLE_RATE as usize,
        std::time::Duration::from_secs(6),
    )?;
    let slice = SAMPLE_RATE as usize / 10;
    for (i, s) in fr.chunks(slice).enumerate() {
        let x: Vec<f64> = s.iter().map(|v| v[0] as f64).collect();
        println!(
            "{:>5} ms ref {:8.1}",
            i * 100,
            tone_at_if(&x, SAMPLE_RATE as f64, 4000.0).abs()
        );
    }
    Ok(())
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! `meter_offset_probe <freq_hz> <offset_hz>`: sends a fixed-frequency measurement block (counter 31, 60 ms per point), and half a second
//! later retunes the generator word (request 0x4c) to `freq_hz + offset_hz`, then prints in 100 ms slices the frequency of the tone in the
//! measured channel of codec 2 and in its reference channel (from the phase advance across the slice), to see whether the unit takes
//! the retune during a burst.
use vnwa_open::audio::tone_at_if;
use vnwa_open::capture::{Capture, SAMPLE_RATE};
use vnwa_open::protocol::generator_frequency;
use vnwa_open::setup::{setup_block, SetupParams};
use vnwa_open::usb::Vnwa;

fn tone_hz(x: &[f64], fs: f64) -> f64 {
    let half = x.len() / 2;
    let (a, b) = (
        tone_at_if(&x[..half], fs, 4000.0),
        tone_at_if(&x[half..2 * half], fs, 4000.0),
    );
    let mut d = b.phase_deg() - a.phase_deg();
    while d > 180.0 {
        d -= 360.0;
    }
    while d <= -180.0 {
        d += 360.0;
    }
    4000.0 + d / 360.0 / (half as f64 / fs)
}

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize, d: f64| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (f, off) = (num(1, 10e6), num(2, 100.0));
    let c = vnwa_open::topology::codecs()?;
    let cap = Capture::open(&c.s21)?;
    let usb = Vnwa::open()?;
    let block = setup_block(&SetupParams {
        freq_hz: f,
        previous_hz: f,
        n_rf: 10,
        n_lo: 9,
        if_hz: 4000.0,
        point_ms: 60.0,
        counter: 31,
        mode: 0,
        rf_level: 0x3fff,
    })?;
    usb.read(0x58, 0x3fff, 0x3fff, 64)?;
    let m = cap.mark();
    usb.send_all(&block)?;
    usb.read(0x0d, 0x0303, 0, 64)?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    usb.send(&generator_frequency(f + off, 0))?;
    let fr = cap.frames_after(
        m,
        0,
        2 * SAMPLE_RATE as usize,
        std::time::Duration::from_secs(6),
    )?;
    let fs = SAMPLE_RATE as f64;
    let slice = SAMPLE_RATE as usize / 10;
    for (i, s) in fr.chunks_exact(slice).enumerate() {
        let meas: Vec<f64> = s.iter().map(|v| v[0] as f64).collect();
        let refc: Vec<f64> = s.iter().map(|v| v[1] as f64).collect();
        println!(
            "{:>5} ms  meas {:9.3} Hz  ref {:9.3} Hz  (levels {:7.1} {:7.1})",
            i * 100,
            tone_hz(&meas, fs),
            tone_hz(&refc, fs),
            tone_at_if(&meas, fs, 4000.0).abs(),
            tone_at_if(&refc, fs, 4000.0).abs()
        );
    }
    Ok(())
}

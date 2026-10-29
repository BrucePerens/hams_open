// SPDX-License-Identifier: AGPL-3.0-or-later
//! `channels <freq_hz> [if_hz] [point_ms]`: one point, printing the tone at the IF in all four codec channels (amplitude and
//! phase), so the ratios the S-parameters are made of can be compared.
use vnwa_open::audio::tone_at_if;
use vnwa_open::capture::{Capture, SAMPLE_RATE};
use vnwa_open::setup::{setup_block, SetupParams};
use vnwa_open::usb::Vnwa;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize, d: f64| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (f, if_hz, ms) = (num(1, 10e6), num(2, 4000.0), num(3, 60.0));
    let c = vnwa_open::topology::codecs()?;
    let (c1, c2) = (Capture::open(&c.s11)?, Capture::open(&c.s21)?);
    let usb = Vnwa::open()?;
    let block = setup_block(&SetupParams {
        freq_hz: f,
        previous_hz: f,
        n_rf: 10,
        n_lo: 9,
        if_hz,
        point_ms: ms,
        counter: 3,
        mode: 0,
        rf_level: 0x3fff,
    })?;
    usb.read(0x58, 0x3fff, 0x3fff, 64)?;
    let (m1, m2) = (c1.mark(), c2.mark());
    usb.send_all(&block)?;
    usb.read(0x0d, 0x0303, 0, 64)?;
    let skip = ((ms + 20.0) / 1000.0 * SAMPLE_RATE as f64) as usize;
    let n = (2.0 * ms / 1000.0 * SAMPLE_RATE as f64) as usize;
    let t = std::time::Duration::from_secs(3);
    let f1 = c1.frames_after(m1, skip, n, t)?;
    let f2 = c2.frames_after(m2, skip, n, t)?;
    let fs = SAMPLE_RATE as f64;
    for (name, fr) in [("codec1", f1), ("codec2", f2)] {
        for ch in 0..2 {
            let x: Vec<f64> = fr.iter().map(|s| s[ch] as f64).collect();
            let z = tone_at_if(&x, fs, if_hz);
            println!(
                "{name} {} {:.2} {:.4}",
                ["L", "R"][ch],
                z.abs(),
                z.phase_deg()
            );
        }
    }
    Ok(())
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! `burst freq_hz previous_hz [point_ms]`: sends one block and prints, per 10 ms of the burst, the codec-2 level and the frequency of
//! the tone near the IF (from the phase advance of the reference channel between windows), to see what the unit does during a burst.
use std::time::Duration;
use vnwa_open::audio::tone_at_if;
use vnwa_open::capture::{Capture, SAMPLE_RATE};
use vnwa_open::setup::{setup_block, SetupParams};
use vnwa_open::usb::Vnwa;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize, d: f64| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (freq, previous, point_ms) = (num(1, 100e6), num(2, 100e6), num(3, 60.0));
    let if_hz = 12000.0;
    let usb = Vnwa::open()?;
    let cap = Capture::open("hw:CARD=CODEC_1,")?;
    let block = setup_block(&SetupParams {
        freq_hz: freq,
        previous_hz: previous,
        n_rf: 10,
        n_lo: 9,
        if_hz,
        point_ms,
        counter: 3,
        mode: 0,
        rf_level: 0x3fff,
    })?;
    let mark = cap.mark();
    let _ = usb.read(0x58, 0x3fff, 0x3fff, 64);
    usb.send_all(&block)?;
    let _ = usb.read(0x0d, 0x0303, 0, 64);
    let frames = cap.frames_after(
        mark,
        0,
        (SAMPLE_RATE as f64 * (6.0 * point_ms + 80.0) / 1000.0) as usize,
        Duration::from_secs(5),
    )?;
    let fs = SAMPLE_RATE as f64;
    let right: Vec<f64> = frames.iter().map(|f| f[1] as f64).collect();
    let left: Vec<f64> = frames.iter().map(|f| f[0] as f64).collect();
    let win = 480; // 10 ms = 120 IF periods
    let mut last: Option<(f64, f64)> = None;
    for (i, (l, r)) in left.chunks(win).zip(right.chunks(win)).enumerate() {
        if l.len() < win {
            break;
        }
        let (tl, tr) = (tone_at_if(l, fs, if_hz), tone_at_if(r, fs, if_hz));
        let ratio = tl.divided_by(tr);
        // The reference channel's phase advance over one window, against that at exactly the IF, gives its frequency offset.
        let ph = tr.phase_deg();
        let df = match last {
            Some((p, _)) => {
                let mut d = ph - p;
                while d > 180.0 {
                    d -= 360.0;
                }
                while d < -180.0 {
                    d += 360.0;
                }
                d / 360.0 / (win as f64 / fs)
            }
            None => f64::NAN,
        };
        last = Some((ph, tr.abs()));
        println!(
            "t={:>4} ms ref {:8.1} meas {:8.1} ratio {:.4} @ {:8.2} deg  IF offset {:7.2} Hz",
            i * 10,
            tr.abs(),
            tl.abs(),
            ratio.abs(),
            ratio.phase_deg(),
            df
        );
    }
    Ok(())
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! `sa_probe <word_hz> <step> <if_hz> <point_ms> [tail_hex]`: sends one set-up block shaped like the vendor program's spectrum-analyzer point (the 0x49 sweep word
//! computed for `word_hz`, step counter `step`, offset tail `tail_hex` or zero), records both codecs for 300 ms and prints the strongest spectral lines of each
//! channel. The unit's TX is cabled to its RX. For finding out what the SA-mode block does; nothing is calibrated.
use std::time::Duration;

use vnwa_open::capture::{Capture, SAMPLE_RATE};
use vnwa_open::protocol::{sweep_word, Request, VENDOR_OUT};
use vnwa_open::setup::{setup_block, SetupParams};
use vnwa_open::usb::Vnwa;

fn lines(x: &[f64], n: usize) -> Vec<(f64, f64)> {
    let fs = f64::from(SAMPLE_RATE);
    let len = x.len() as f64;
    let mean = x.iter().sum::<f64>() / len;
    let mut spec = Vec::new();
    let mut f = 50.0;
    while f < 23_500.0 {
        let w = 2.0 * std::f64::consts::PI * f / fs;
        let (mut re, mut im) = (0.0, 0.0);
        for (i, v) in x.iter().enumerate() {
            let h = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / len).cos();
            re += (v - mean) * h * (w * i as f64).cos();
            im -= (v - mean) * h * (w * i as f64).sin();
        }
        spec.push((f, 4.0 * (re * re + im * im).sqrt() / len));
        f += 25.0;
    }
    let mut peaks: Vec<(f64, f64)> = Vec::new();
    for i in 1..spec.len() - 1 {
        if spec[i].1 > spec[i - 1].1 && spec[i].1 >= spec[i + 1].1 && spec[i].1 > 20.0 {
            peaks.push(spec[i]);
        }
    }
    peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    peaks.truncate(n);
    peaks
}

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize, d: f64| a.get(i).and_then(|s| s.parse::<f64>().ok()).unwrap_or(d);
    let (word_hz, step, if_hz, ms) = (
        num(1, 10e6),
        num(2, 201.0) as u8,
        num(3, 12_000.0),
        num(4, 5.0),
    );
    let level = num(6, 16383.0) as u16;
    let tail: Vec<u8> = match a.get(5) {
        Some(h) => (0..h.len() / 2)
            .map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap_or(0))
            .collect(),
        None => vec![0; 6],
    };
    let codecs = vnwa_open::topology::codecs()?;
    let (c1, c2) = (Capture::open(&codecs.s11)?, Capture::open(&codecs.s21)?);
    let usb = Vnwa::open()?;
    let mut block = setup_block(&SetupParams {
        freq_hz: word_hz,
        previous_hz: word_hz,
        n_rf: 10,
        n_lo: 9,
        if_hz,
        point_ms: ms,
        counter: 3,
        mode: 0,
        rf_level: 0x3fff,
    })?;
    // The vendor's SA block: the 0x49 payload with its own step counter and tail.
    let mut p = vec![step, 0, 10, 9];
    p.extend_from_slice(&sweep_word(word_hz, 10, 9).to_le_bytes()[..5]);
    p.extend_from_slice(&tail);
    p.resize(15, 0);
    let i49 = block.iter().position(|r| r.request == 0x49).unwrap();
    block[i49] = Request {
        request_type: VENDOR_OUT,
        request: 0x49,
        value: 0,
        index: 0,
        data: p,
    };
    usb.read(0x58, level, level, 64)?;
    std::thread::sleep(Duration::from_millis(300));
    let (m1, m2) = (c1.mark(), c2.mark());
    usb.send_all(&block)?;
    usb.read(0x0d, 0x0303, 0, 64)?;
    let n = (2.0 * ms / 1000.0 * f64::from(SAMPLE_RATE)) as usize;
    let skip = (f64::from(SAMPLE_RATE) * (ms + 11.0) / 1000.0) as usize;
    for (name, c, m) in [("codec 1", &c1, m1), ("codec 2", &c2, m2)] {
        let f = c.frames_after(m, skip, n, Duration::from_secs(3))?;
        for (ch, label) in [(0, "left"), (1, "right")] {
            let x: Vec<f64> = f.iter().map(|v| f64::from(v[ch])).collect();
            let rms = (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
            println!(
                "{name} {label}: rms {rms:.0}; lines (Hz, amplitude): {:?}",
                lines(&x, 4)
                    .iter()
                    .map(|(f, a)| (f.round() as i64, a.round() as i64))
                    .collect::<Vec<_>>()
            );
        }
    }
    Ok(())
}

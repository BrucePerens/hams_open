// SPDX-License-Identifier: AGPL-3.0-or-later
//! rsp_spectrum: tune a running RSP2pro or RSP1A and print a coarse spectrum.
//!
//!     cargo run --release --example rsp_spectrum -- 95.8 [IF_GR_DB [LNA_STATE [ADC_MHZ [MODEL]]]]
//!
//! MODEL is `rsp2`, `rsp1a`, `cold`, `cold2` or `cold1a` (the first RSP in the boot-ROM state, bcdDevice 2.00, driven
//! without any firmware; board controls are unavailable); without it the first running RSP2/RSP2pro or RSP1A is used.
//! Bias-T stays off. Prints 64 rows, one per 1/64 of the sampled band, as dBFS bars.

use rsp_open::dsp::{median, power_spectrum_db};
use rsp_open::{Model, Rsp, Settings};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mhz: f64 = a.first().map(|s| s.parse()).transpose()?.unwrap_or(95.8);
    let gr: u8 = a.get(1).map(|s| s.parse()).transpose()?.unwrap_or(40);
    let lna: u8 = a.get(2).map(|s| s.parse()).transpose()?.unwrap_or(0);
    let adc: f64 = a.get(3).map(|s| s.parse()).transpose()?.unwrap_or(2.0);

    let mut rx = match a.get(4).map(|s| s.as_str()) {
        None => Rsp::open_with(Settings::default())?,
        Some("rsp2") => Rsp::open_model_with(Model::Rsp2, Settings::default())?,
        Some("rsp1a") => Rsp::open_model_with(Model::Rsp1a, Settings::default())?,
        Some("cold") => Rsp::open_boot_rom_with(Settings::default())?,
        Some("cold2") => Rsp::open_boot_rom_model_with(Model::Rsp2, Settings::default())?,
        Some("cold1a") => Rsp::open_boot_rom_model_with(Model::Rsp1a, Settings::default())?,
        Some(other) => return Err(format!("unknown model {other:?} (rsp2 or rsp1a)").into()),
    };
    rx.set_sample_rate((adc * 1e6).round() as u32)?;
    let lo = rx.set_frequency((mhz * 1e6).round() as u64)?;
    rx.set_gain(gr, lna)?;
    let fs = rx.output_rate();
    println!(
        "{} (request 0x48 returned 0x{:02x}): tuned {:.6} MHz (LO {:.3} Hz), {:.3} MS/s, IF GR {gr}, LNA {lna}",
        rx.model().name(),
        rx.read48,
        mhz,
        lo,
        fs / 1e6
    );

    let it = rx.start_stream_iter(256)?;
    let mut x = Vec::new();
    let mut skipped = 0usize;
    for c in it {
        if skipped < 100_000 {
            skipped += c.len();
            continue;
        }
        x.extend_from_slice(&c);
        if x.len() >= 131_072 {
            break;
        }
    }
    let st = rx.stream_stats();
    rx.stop_stream()?;
    rx.close()?;

    let n = 4096;
    let s = power_spectrum_db(&x, n);
    let floor = median(&s);
    let rows = 64;
    for r in 0..rows {
        let seg = &s[r * n / rows..(r + 1) * n / rows];
        let p = seg.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let f = mhz + ((r as f64 + 0.5) / rows as f64 - 0.5) * fs / 1e6;
        let bar = ((p - floor).max(0.0) / 1.0) as usize;
        println!("{f:11.4} MHz {p:7.1} dBFS |{}", "#".repeat(bar.min(70)));
    }
    println!(
        "floor (median bin) {floor:.1} dBFS; {} blocks, {} counter gaps",
        st.blocks, st.gaps
    );
    Ok(())
}

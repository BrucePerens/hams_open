// SPDX-License-Identifier: AGPL-3.0-or-later
//! rsp_coldcheck: one pass/fail check of a receiver, for stress tests and power-cycle rigs.
//! It opens the first RSP in the boot-ROM state (bcdDevice 2.00, no firmware; this is what a
//! receiver is after power is applied), starts the stream, tunes to frequencies where the
//! receiver's own 24 MHz reference puts a harmonic exactly 200 kHz from the tuning, and checks
//! that the harmonic lands where it should. It prints one JSON line and exits 0 (all checks
//! passed) or 1. A stream that delivers nothing is reported as `stalled`, never waited on.
//!
//!     cargo run --release --example rsp_coldcheck -- [rsp2|rsp1a|rsp1] [--running]
//!
//! With `--running` the first receiver that already has firmware (bcdDevice 2.07) is used
//! instead. Nothing may be connected to the antenna ports. Only requests that `rsp_open`
//! documents are sent; no USB reset is ever issued.

use rsp_open::dsp::{peak_near, power_spectrum_db};
use rsp_open::usb::list_devices;
use rsp_open::{IqI16, Model, Rsp, SampleIter, Settings};
use std::time::{Duration, Instant};

const NFFT: usize = 16_384;
const WANT: usize = 262_144;
const SKIP: usize = 100_000;
/// (RF Hz, expected harmonic offset Hz): 24 MHz multiples 200 kHz away.
const CHECKS: [(u64, f64); 6] = [
    (95_800_000, 200e3),
    (23_800_000, 200e3),
    (264_200_000, -200e3),
    (383_800_000, 200e3),
    (407_800_000, 200e3),
    (1_200_200_000, -200e3),
];

fn grab(it: &mut SampleIter, deadline: Instant) -> Option<Vec<IqI16>> {
    let mut skipped = 0;
    let mut out = Vec::with_capacity(WANT);
    while Instant::now() < deadline {
        let c = it.next()?;
        if skipped < SKIP {
            skipped += c.len();
            continue;
        }
        out.extend_from_slice(&c);
        if out.len() >= WANT {
            out.truncate(WANT);
            return Some(out);
        }
    }
    None
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn fail(model: &str, bcd: u16, msg: &str) -> ! {
    println!(
        "{{\"ok\":false,\"model\":\"{}\",\"bcd\":\"{:04x}\",\"error\":\"{}\"}}",
        json_escape(model),
        bcd,
        json_escape(msg)
    );
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let running = args.iter().any(|a| a == "--running");
    let model = match args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(|s| s.as_str())
    {
        None => None,
        Some("rsp2") => Some(Model::Rsp2),
        Some("rsp1a") => Some(Model::Rsp1a),
        Some("rsp1") => Some(Model::Rsp1),
        Some(o) => fail("", 0, &format!("unknown model {o:?} (rsp2, rsp1a or rsp1)")),
    };
    let want_bcd = if running {
        rsp_open::BCD_RUNNING
    } else {
        rsp_open::BCD_BOOT_ROM
    };
    let seen = list_devices().unwrap_or_default();
    let dev = seen
        .iter()
        .find(|d| d.bcd_device == want_bcd && model.is_none_or(|m| d.model == m));
    let Some(dev) = dev else {
        let have: Vec<String> = seen
            .iter()
            .map(|d| format!("{:04x}:{:04x} bcd {:04x}", d.vid, d.pid, d.bcd_device))
            .collect();
        fail(
            "",
            0,
            &format!(
                "no receiver at bcdDevice {want_bcd:04x} on the bus (present: {})",
                have.join(", ")
            ),
        );
    };
    let (m, bcd) = (dev.model, dev.bcd_device);
    let opened = if running {
        Rsp::open_model_with(m, Settings::default())
    } else {
        Rsp::open_boot_rom_model_with(m, Settings::default())
    };
    let mut rx = match opened {
        Ok(r) => r,
        Err(e) => fail(m.name(), bcd, &format!("open: {e}")),
    };
    let serial = rx.serial.clone().unwrap_or_default();
    let mut it = match rx.start_stream_iter(256) {
        Ok(i) => i,
        Err(e) => fail(m.name(), bcd, &format!("start: {e}")),
    };
    let mut ok = true;
    let mut items = Vec::new();
    for (rf, off) in CHECKS {
        if let Err(e) = rx.set_frequency(rf) {
            fail(m.name(), bcd, &format!("tune {rf}: {e}"));
        }
        let before = rx.stream_stats();
        let Some(x) = grab(&mut it, Instant::now() + Duration::from_secs(3)) else {
            fail(
                m.name(),
                bcd,
                &format!("stalled: no samples within 3 s at {rf} Hz"),
            );
        };
        let after = rx.stream_stats();
        let s = power_spectrum_db(&x, NFFT);
        let (f, _lvl, over) = peak_near(&s, rx.output_rate(), off, 20_000.0, 10_000.0);
        let good = (f - off).abs() < 400.0 && over > 20.0;
        ok &= good;
        items.push(format!(
            "{{\"rf\":{rf},\"peak_hz\":{f:.1},\"over_db\":{over:.1},\"gaps\":{},\"pass\":{good}}}",
            after.gaps - before.gaps
        ));
    }
    drop(it);
    let _ = rx.stop_stream();
    let _ = rx.close();
    println!(
        "{{\"ok\":{ok},\"model\":\"{}\",\"bcd\":\"{:04x}\",\"serial\":\"{}\",\"checks\":[{}]}}",
        json_escape(m.name()),
        bcd,
        json_escape(&serial),
        items.join(",")
    );
    std::process::exit(if ok { 0 } else { 1 });
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! Receive-only capture: tunes, starts the stream, reads for a few seconds, prints the measured rate, timeouts, mean power and
//! CPU time, and optionally writes interleaved little-endian f32 I/Q (cf32). Nothing here transmits; the antenna input stays as
//! the owner left it.
//!
//! `capture --freq HZ --rate HZ --seconds N [--out FILE] [--fpga-loaded]`. `--fpga-loaded` is the caller's statement that an
//! FDM-S1/S2's FPGA image was loaded by other means; without it an S1/S2 refuses to start.
use elad_usb::fpga::FpgaState;
use elad_usb::radio::{OpenOptions, Radio};
use elad_usb::usb::{find, UsbTransport};
use std::io::Write;
use std::time::{Duration, Instant};

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter()
        .position(|x| x == name)
        .and_then(|i| a.get(i + 1).cloned())
}

fn cpu_seconds() -> f64 {
    std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|s| {
            let rest = s.rsplit(')').next()?.to_string();
            let f: Vec<&str> = rest.split_whitespace().collect();
            let ticks: f64 = f.get(11)?.parse::<f64>().ok()? + f.get(12)?.parse::<f64>().ok()?;
            Some(ticks / 100.0)
        })
        .unwrap_or(0.0)
}

fn main() {
    let freq: f64 = arg("--freq")
        .and_then(|v| v.parse().ok())
        .unwrap_or(7_100_000.0);
    let rate: u32 = arg("--rate")
        .and_then(|v| v.parse().ok())
        .unwrap_or(192_000);
    let secs: f64 = arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(3.0);
    let out_path = arg("--out");
    let fpga = if std::env::args().any(|a| a == "--fpga-loaded") {
        FpgaState::LoadedExternally
    } else {
        FpgaState::Unknown
    };
    let found = find().unwrap_or_default();
    let Some(f) = found.first() else {
        eprintln!("no ELAD receiver on the bus");
        std::process::exit(1);
    };
    let transport = UsbTransport::open(f).unwrap_or_else(|e| {
        eprintln!("open: {e}");
        std::process::exit(2)
    });
    let mut radio = Radio::open(
        transport,
        f.model,
        OpenOptions {
            fpga,
            sample_rate_hz: rate,
        },
    )
    .unwrap_or_else(|e| {
        eprintln!("open: {e}");
        std::process::exit(2)
    });
    if let Err(e) = radio.tune(freq).and_then(|_| radio.start()) {
        eprintln!("start: {e}");
        std::process::exit(3);
    }
    let mut file = out_path
        .as_ref()
        .and_then(|p| std::fs::File::create(p).ok());
    let begin = Instant::now();
    let cpu0 = cpu_seconds();
    let mut buf: Vec<f32> = Vec::new();
    let mut power = 0.0f64;
    let mut n = 0u64;
    let mut verdict = "ok".to_string();
    while begin.elapsed() < Duration::from_secs_f64(secs) {
        buf.clear();
        match radio.read(&mut buf, Duration::from_millis(500)) {
            Ok(_) => {
                for p in buf.as_chunks::<2>().0 {
                    power += f64::from(p[0]) * f64::from(p[0]) + f64::from(p[1]) * f64::from(p[1]);
                    n += 1;
                }
                if let Some(fl) = file.as_mut() {
                    let bytes: Vec<u8> = buf.iter().flat_map(|v| v.to_le_bytes()).collect();
                    let _ = fl.write_all(&bytes);
                }
            }
            Err(e) => {
                verdict = format!("{e}");
                if matches!(e, elad_usb::Error::NoSamples) {
                    break;
                }
            }
        }
    }
    let elapsed = begin.elapsed().as_secs_f64();
    let cpu = cpu_seconds() - cpu0;
    let _ = radio.stop();
    let s = radio.stats();
    println!("verdict: {verdict}");
    println!(
        "elapsed {elapsed:.2} s, samples {n}, measured rate {:.0} Hz (asked {rate})",
        n as f64 / elapsed
    );
    println!(
        "bytes {}, reads {}, timeouts {}, empty reads {}",
        s.bytes, s.reads, s.timeouts, s.empty_reads
    );
    if n > 0 {
        println!("mean power {:.1} dBFS", 10.0 * (power / n as f64).log10());
    }
    println!(
        "process cpu {:.2} s ({:.1} % of one core)",
        cpu,
        100.0 * cpu / elapsed
    );
}

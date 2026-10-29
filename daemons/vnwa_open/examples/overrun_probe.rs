// SPDX-License-Identifier: AGPL-3.0-or-later
//! `overrun_probe <seconds> [period_frames] [--sweep]`: opens both codecs (optionally with a fixed ALSA period), runs for the given
//! time (idle, or while sweeping S11 and S21 with the unit) and prints the overruns per codec with their timestamps and the rate.
use std::time::{Duration, Instant};

use vnwa_open::capture::{Capture, Options};

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let secs: f64 = a
        .get(1)
        .and_then(|s| s.parse().ok())
        .ok_or("usage: overrun_probe <seconds> [period_frames] [--sweep]")?;
    let period: Option<u32> = a.get(2).and_then(|s| s.parse().ok());
    let cycle = a.iter().any(|s| s == "--cycle" || s == "--cycle-closed");
    let close_first = a.iter().any(|s| s == "--cycle-closed");
    let sweep = a.iter().any(|s| s == "--sweep");
    let verbose = a.iter().any(|s| s == "-v");
    let o = Options {
        period_frames: period,
    };
    let start = Instant::now();
    if sweep {
        let cfg = vnwa_open::sweep::Config {
            if_hz: 4000.0,
            overrun_retries: 1000,
            capture: o,
            ..Default::default()
        };
        let mut vna = vnwa_open::sweep::Vna::open(cfg)?;
        let (mut sweeps, mut errs) = (0, 0);
        while start.elapsed() < Duration::from_secs_f64(secs) {
            match vna.sweep(100e3, 150e6, 20) {
                Ok(_) => sweeps += 1,
                Err(e) => {
                    errs += 1;
                    println!("sweep error: {}", e.chars().take(200).collect::<String>())
                }
            }
        }
        let el = start.elapsed().as_secs_f64();
        let r = vna.recovery();
        println!("period {period:?}: {sweeps} sweeps ({errs} failed) in {el:.1} s; overruns reported {} = {:.3}/s; points re-measured for overruns {}; {r:?}", r.overruns_reported, r.overruns_reported as f64 / el, r.overrun_retries);
    } else {
        let c = vnwa_open::topology::codecs()?;
        let (mut c1, mut c2) = (
            Capture::open_with(&c.s11, o)?,
            Capture::open_with(&c.s21, o)?,
        );
        std::thread::sleep(Duration::from_secs_f64(secs));
        if cycle {
            // Cold reset with the streams open (--cycle) or closed first (--cycle-closed): count the reports during the power cycle.
            let before = c1.overrun_count() + c2.overrun_count();
            if close_first {
                c1.close();
                c2.close();
            }
            let hub = vnwa_open::power::HubPort {
                hub: "3-2".into(),
                port: 2,
            };
            let t = Instant::now();
            vnwa_open::power::cold_reset(&hub)?;
            println!(
                "cold reset took {:.1} s; overruns reported during it: {} (streams {})",
                t.elapsed().as_secs_f64(),
                c1.overrun_count() + c2.overrun_count() - before,
                if close_first {
                    "closed first"
                } else {
                    "left open"
                }
            );
        }
        let el = start.elapsed().as_secs_f64();
        for (n, c) in [("S11 codec", &c1), ("S21 codec", &c2)] {
            println!(
                "{n}: {} overruns in {el:.1} s = {:.3}/s",
                c.overrun_count(),
                c.overrun_count() as f64 / el
            );
            if verbose {
                for o in c.overruns().iter().take(40) {
                    println!(
                        "  at frame {} ({:.3} s)",
                        o.at_frame,
                        o.at_frame as f64 / 48000.0
                    );
                }
            }
        }
    }
    Ok(())
}

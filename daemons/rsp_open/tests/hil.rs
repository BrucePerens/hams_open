// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware-in-the-loop tests. Run only with a running RSP2pro attached (nothing connected to
//! the antenna ports; bias-T stays off):
//!
//!     cargo test --test hil -- --ignored --nocapture --test-threads=1
//!
//! The receiver's own 24 MHz reference harmonics are used as test signals: tuned to
//! `f`, the harmonic `k * 24 MHz` must appear at `k * 24 MHz - f`.

use rsp_open::dsp::{peak_near, power_spectrum_db};
use rsp_open::usb::list_devices;
use rsp_open::{Bandwidth, Error, IqI16, Rsp2, SampleIter, Settings};

const NWANT: usize = 262_144;
const NFFT: usize = 16_384;

fn running_device() -> bool {
    match list_devices() {
        Ok(d) => d
            .iter()
            .any(|d| d.pid == rsp_open::PID_RSP2 && d.is_running()),
        Err(_) => false,
    }
}

/// Discard `skip` samples, then collect `n`.
fn grab(it: &mut SampleIter, skip: usize, n: usize) -> Vec<IqI16> {
    let mut skipped = 0;
    let mut out = Vec::with_capacity(n);
    for chunk in it.by_ref() {
        if skipped < skip {
            skipped += chunk.len();
            continue;
        }
        out.extend_from_slice(&chunk);
        if out.len() >= n {
            out.truncate(n);
            break;
        }
    }
    out
}

struct Meas {
    peak_hz: f64,
    over_db: f64,
    gaps: u64,
}

fn measure(rx: &mut Rsp2, it: &mut SampleIter, rf: u64, expect_off: f64) -> Meas {
    rx.set_frequency(rf).expect("tune");
    let before = rx.stream_stats();
    let x = grab(it, 100_000, NWANT);
    let after = rx.stream_stats();
    assert_eq!(x.len(), NWANT, "short capture at {rf}");
    let fs = rx.output_rate();
    let s = power_spectrum_db(&x, NFFT);
    let (f, _lvl, over) = peak_near(&s, fs, expect_off, 20_000.0, 10_000.0);
    Meas {
        peak_hz: f,
        over_db: over,
        gaps: after.gaps - before.gaps,
    }
}

fn open(settings: Settings) -> Option<Rsp2> {
    if !running_device() {
        eprintln!("no running RSP2pro: skipping");
        return None;
    }
    match Rsp2::open_with(settings) {
        Ok(r) => Some(r),
        Err(Error::NotRunning) => {
            eprintln!("RSP2pro present but not running: skipping");
            None
        }
        Err(e) => panic!("open: {e}"),
    }
}

/// The an earlier stand-alone verification program tunings (protocol document 1.12), through this driver.
#[test]
#[ignore]
// Tests [@ANCHOR: rsp_device_rsp2_open_with]
// Tests [@ANCHOR: rsp_usb_context_new]
// Tests [@ANCHOR: rsp_usb_with_devices]
// Tests [@ANCHOR: rsp_usb_usbtransport_open]
// Tests [@ANCHOR: rsp_usb_usbtransport_serial]
// Tests [@ANCHOR: rsp_usb_usbtransport_vendor_out]
// Tests [@ANCHOR: rsp_usb_usbtransport_vendor_in]
// Tests [@ANCHOR: rsp_usb_usbtransport_set_alt]
// Tests [@ANCHOR: rsp_usb_usbtransport_start_iso]
// Tests [@ANCHOR: rsp_usb_iso_callback]
// Tests [@ANCHOR: rsp_usb_usbtransport_stop_iso]
// Tests [@ANCHOR: rsp_usb_usbtransport_release]
fn hil_vhf_reference_spurs() {
    let s = Settings {
        if_gr_db: 30,
        lna_state: 0,
        bandwidth: Bandwidth::K1536,
        rf_hz: 95_800_000,
        ..Settings::default()
    };
    let Some(mut rx) = open(s) else { return };
    println!("serial {:?}, read48 0x{:02x}", rx.serial, rx.read48);
    let mut it = rx.start_stream_iter(256).unwrap();
    let cases: [(u64, f64); 5] = [
        (95_800_000, 200e3),
        (95_900_000, 100e3),
        (96_300_000, -300e3),
        (72_300_000, -300e3),
        (119_800_000, 200e3),
    ];
    let mut failures = Vec::new();
    for (rf, off) in cases {
        let m = measure(&mut rx, &mut it, rf, off);
        println!(
            "{:>9.3} MHz: expected {:+8.1} kHz, peak {:+9.3} kHz, {:5.1} dB over floor, {} gaps",
            rf as f64 / 1e6,
            off / 1e3,
            m.peak_hz / 1e3,
            m.over_db,
            m.gaps
        );
        if (m.peak_hz - off).abs() > 2_000.0 || m.over_db < 25.0 {
            failures.push(rf);
        }
    }
    // 100 MHz: no harmonic of 24 MHz within +-1 MHz.
    rx.set_frequency(100_000_000).unwrap();
    let x = grab(&mut it, 100_000, NWANT);
    let sp = power_spectrum_db(&x, NFFT);
    let (f, _, over) = peak_near(&sp, 2e6, 0.0, 700_000.0, 10_000.0);
    println!(
        "  100.000 MHz: strongest {:+9.3} kHz at {:5.1} dB over floor (no harmonic expected)",
        f / 1e3,
        over
    );
    let st = rx.stream_stats();
    println!(
        "stream: {} blocks, {} gaps, {} lost samples, dropped chunks {}",
        st.blocks,
        st.gaps,
        st.lost_samples,
        it.dropped()
    );
    rx.stop_stream().unwrap();
    rx.close().unwrap();
    assert!(failures.is_empty(), "spur check failed at {failures:?}");
    assert!(over < 25.0, "unexpected strong peak at 100 MHz");
}

/// Other bands and the other stream formats; reports, asserts only the spur position.
#[test]
#[ignore]
fn hil_other_bands_and_formats() {
    let s = Settings {
        if_gr_db: 30,
        rf_hz: 95_800_000,
        ..Settings::default()
    };
    let Some(mut rx) = open(s) else { return };
    let mut it = rx.start_stream_iter(256).unwrap();
    let mut bad = Vec::new();
    for (rf, off, what) in [
        (143_800_000u64, 200e3, "B3 (6 x 24 MHz)"),
        (23_800_000, 200e3, "HF AM mode (24 MHz)"),
        (47_800_000, 200e3, "HF AM mode (48 MHz)"),
        (264_200_000, -200e3, "AM down-conversion (264 MHz)"),
        (479_800_000, 200e3, "B4/5 (480 MHz)"),
        (1_199_800_000, 200e3, "L band (1200 MHz)"),
    ] {
        let m = measure(&mut rx, &mut it, rf, off);
        println!(
            "{what:30} {:>10.3} MHz: expected {:+7.1} kHz, peak {:+9.3} kHz, {:5.1} dB, {} gaps",
            rf as f64 / 1e6,
            off / 1e3,
            m.peak_hz / 1e3,
            m.over_db,
            m.gaps
        );
        if (m.peak_hz - off).abs() > 2_000.0 || m.over_db < 15.0 {
            bad.push(what);
        }
    }
    drop(it);
    rx.set_frequency(95_800_000).unwrap();
    for fs in [
        2_048_000u32,
        3_000_000,
        6_000_000,
        8_000_000,
        9_000_000,
        10_000_000,
    ] {
        rx.stop_stream().unwrap();
        rx.set_sample_rate(fs).unwrap();
        let mut it = rx.start_stream_iter(512).unwrap();
        let st0 = rx.stream_stats();
        let x = grab(&mut it, 200_000, NWANT);
        let st1 = rx.stream_stats();
        let sp = power_spectrum_db(&x, NFFT);
        let (f, _, over) = peak_near(&sp, rx.output_rate(), 200e3, 20_000.0, 10_000.0);
        let maxabs = x
            .iter()
            .map(|s| s.i.unsigned_abs().max(s.q.unsigned_abs()))
            .max()
            .unwrap_or(0);
        println!(
            "ADC {:>6.3} MHz {:?}: peak {:+9.3} kHz {:5.1} dB, gaps {}, dropped {}, max |sample| {}",
            fs as f64 / 1e6, rx.stream_config().format, f / 1e3, over, st1.gaps - st0.gaps, it.dropped(), maxabs
        );
        if (f - 200e3).abs() > 2_000.0 || over < 15.0 {
            bad.push("format");
        }
    }
    rx.stop_stream().unwrap();
    rx.close().unwrap();
    assert!(bad.is_empty(), "failed: {bad:?}");
}

/// A boot-ROM device must be refused before anything is sent. Only checks the descriptor path
/// on whatever device is present; it does not need (and never makes) a cold device.
#[test]
#[ignore]
fn hil_state_check() {
    let devs = list_devices().unwrap();
    for d in devs {
        println!("{:?}", d);
        let r = rsp_open::check_state(d.pid, d.bcd_device);
        if d.bcd_device == rsp_open::BCD_BOOT_ROM {
            assert!(matches!(r, Err(Error::NotRunning)));
        }
    }
}

/// Reports counter gaps caused by control traffic while streaming (in-band retunes, band
/// changes, board-byte writes). Asserts only that streaming survives.
#[test]
#[ignore]
fn hil_gaps_during_control() {
    let Some(mut rx) = open(Settings {
        rf_hz: 95_800_000,
        ..Settings::default()
    }) else {
        return;
    };
    let mut it = rx.start_stream_iter(1024).unwrap();
    let _ = grab(&mut it, 0, 200_000);
    let report =
        |what: &str, rx: &mut Rsp2, it: &mut SampleIter, f: &mut dyn FnMut(&mut Rsp2, usize)| {
            let s0 = rx.stream_stats();
            for k in 0..20 {
                f(rx, k);
                let _ = grab(it, 0, 20_000);
            }
            let s1 = rx.stream_stats();
            println!(
                "{what:28}: {} gaps, {} lost samples over 20 operations",
                s1.gaps - s0.gaps,
                s1.lost_samples - s0.lost_samples
            );
        };
    report("in-band retune (r3, r2)", &mut rx, &mut it, &mut |rx, k| {
        rx.set_frequency(95_800_000 + (k as u64 % 2) * 100_000)
            .unwrap();
    });
    report("band change (full tune)", &mut rx, &mut it, &mut |rx, k| {
        rx.set_frequency(if k % 2 == 0 { 143_800_000 } else { 95_800_000 })
            .unwrap();
    });
    report("gain (r1 + board byte)", &mut rx, &mut it, &mut |rx, k| {
        rx.set_gain(40 + (k as u8 % 2), 0).unwrap();
    });
    report(
        "antenna A/B (board byte)",
        &mut rx,
        &mut it,
        &mut |rx, k| {
            rx.select_antenna(if k % 2 == 0 {
                rsp_open::Antenna::B
            } else {
                rsp_open::Antenna::A
            })
            .unwrap();
        },
    );
    let st = rx.stream_stats();
    assert!(st.blocks > 1000);
    drop(it);
    rx.stop_stream().unwrap();
    rx.close().unwrap();
}

/// Paths that change the stream configuration while streaming: sample rate, low-IF modes,
/// ppm, bandwidth, ext-ref output. Reports where the 96 MHz harmonic lands (tuned 95.8 MHz).
#[test]
#[ignore]
// Tests [@ANCHOR: rsp_usb_usbtransport_set_alt]
// Tests [@ANCHOR: rsp_usb_usbtransport_start_iso]
// Tests [@ANCHOR: rsp_usb_usbtransport_stop_iso]
// Tests [@ANCHOR: rsp_device_rsp2_set_sampling]
// Tests [@ANCHOR: rsp_device_rsp2_restart_stream]
// Tests [@ANCHOR: rsp_device_rsp2_set_ppm]
fn hil_live_reconfiguration() {
    let Some(mut rx) = open(Settings {
        rf_hz: 95_800_000,
        if_gr_db: 30,
        ..Settings::default()
    }) else {
        return;
    };
    let got = std::sync::Arc::new(std::sync::Mutex::new(Vec::<IqI16>::new()));
    let g2 = got.clone();
    rx.start_stream(move |s, _| {
        let mut g = g2.lock().unwrap();
        if g.len() < 2_000_000 {
            g.extend_from_slice(s);
        }
    })
    .unwrap();
    let look = |rx: &mut Rsp2, what: &str| {
        std::thread::sleep(std::time::Duration::from_millis(150));
        got.lock().unwrap().clear();
        let s0 = rx.stream_stats();
        let t0 = std::time::Instant::now();
        while got.lock().unwrap().len() < NWANT && t0.elapsed().as_secs() < 5 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let s1 = rx.stream_stats();
        let x: Vec<IqI16> = got.lock().unwrap().iter().take(NWANT).copied().collect();
        if x.len() < NWANT {
            println!("{what:34}: only {} samples (stream stalled?)", x.len());
            return (f64::NAN, f64::NAN);
        }
        let fs = rx.output_rate();
        let sp = power_spectrum_db(&x, NFFT);
        let (f, _, over) = peak_near(&sp, fs, 0.0, fs * 0.45, 10_000.0);
        println!(
            "{what:34}: out {:.3} MS/s, strongest {:+9.3} kHz {:5.1} dB, gaps {} lost {}",
            fs / 1e6,
            f / 1e3,
            over,
            s1.gaps - s0.gaps,
            s1.lost_samples - s0.lost_samples
        );
        (f, over)
    };
    let (f, _) = look(&mut rx, "2 MHz zero IF");
    assert!((f - 200e3).abs() < 2e3);
    rx.set_sample_rate(6_000_000).unwrap();
    let (f, _) = look(&mut rx, "6 MHz zero IF (changed live)");
    assert!((f - 200e3).abs() < 2e3);
    rx.set_sample_rate(2_000_000).unwrap();
    let (f, _) = look(&mut rx, "2 MHz zero IF (changed live)");
    assert!((f - 200e3).abs() < 2e3);
    rx.set_ppm(10.0).unwrap();
    let (f, _) = look(&mut rx, "ppm +10");
    assert!((f - 200e3).abs() < 2e3);
    rx.set_ppm(0.0).unwrap();
    rx.set_bandwidth(Bandwidth::K600).unwrap();
    look(&mut rx, "BW 600 kHz");
    rx.set_bandwidth(Bandwidth::K1536).unwrap();
    rx.set_ext_ref_out(true).unwrap();
    look(&mut rx, "ext ref output on");
    rx.set_ext_ref_out(false).unwrap();
    for (mode, name) in [
        (rsp_open::IfMode::Low450k, "low IF 450 kHz (ADC 2 MHz)"),
        (rsp_open::IfMode::Low1620k, "low IF 1.62 MHz (ADC 6 MHz)"),
        (
            rsp_open::IfMode::Low2048k,
            "low IF 2.048 MHz (ADC 8.192 MHz)",
        ),
    ] {
        rx.set_if_mode(mode).unwrap();
        rx.set_bandwidth(Bandwidth::K1536).unwrap();
        look(&mut rx, name);
    }
    rx.set_if_mode(rsp_open::IfMode::Zero).unwrap();
    let (f, _) = look(&mut rx, "back to zero IF");
    assert!((f - 200e3).abs() < 2e3);
    rx.stop_stream().unwrap();
    rx.close().unwrap();
}

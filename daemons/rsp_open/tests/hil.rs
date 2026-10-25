// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware-in-the-loop tests. Run only with a running RSP2pro or RSP1A attached (nothing
//! connected to the antenna ports; bias-T stays off; no test ever switches it on). Tests for
//! a model that is not on the bus skip themselves. Only PIDs 0x3010 and 0x3000 are opened, and
//! only at bcdDevice 2.07:
//!
//!     cargo test --test hil -- --ignored --nocapture --test-threads=1
//!     cargo test --test hil hil_rsp1a -- --ignored --nocapture --test-threads=1
//!
//! The receiver's own 24 MHz reference harmonics are used as test signals: tuned to
//! `f`, the harmonic `k * 24 MHz` must appear at `k * 24 MHz - f`.

use rsp_open::dsp::{peak_near, power_spectrum_db};
use rsp_open::usb::list_devices;
use rsp_open::{Bandwidth, Error, IqI16, Model, Rsp, Rsp2, SampleIter, Settings};

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
    match Rsp2::open_model_with(Model::Rsp2, settings) {
        Ok(r) => Some(r),
        Err(Error::NotRunning) => {
            eprintln!("RSP2pro present but not running: skipping");
            None
        }
        Err(e) => panic!("open: {e}"),
    }
}

/// The tunings of the earlier stand-alone verification program (protocol document 1.12),
/// through this driver.
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

/// The RSP1A, only if it is on the bus with its firmware running (bcdDevice 2.07). Any other
/// state is refused here, before the driver is asked to open it.
fn open_rsp1a(settings: Settings) -> Option<Rsp> {
    let devs = list_devices().expect("list devices");
    let Some(d) = devs.iter().find(|d| d.pid == rsp_open::PID_RSP1A) else {
        eprintln!("no RSP1A on the bus: skipping");
        return None;
    };
    if d.bcd_device != rsp_open::BCD_RUNNING {
        eprintln!(
            "RSP1A reports bcdDevice {:04x}, not 0207: refusing to touch it",
            d.bcd_device
        );
        return None;
    }
    let rx = Rsp::open_model_with(Model::Rsp1a, settings).expect("open RSP1A");
    assert_eq!(rx.model(), Model::Rsp1a);
    assert!(!rx.settings().bias_t);
    Some(rx)
}

/// RSP1A: the 24 MHz reference harmonics in every band filter range this driver offers, the
/// 2 MHz filter edge, and a tuning with no harmonic nearby.
#[test]
#[ignore]
// Tests [@ANCHOR: rsp_device_rsp_open_model_with]
// Tests [@ANCHOR: rsp_device_rsp_open_device]
fn hil_rsp1a_reference_spurs() {
    let Some(mut rx) = open_rsp1a(Settings {
        if_gr_db: 30,
        rf_hz: 95_800_000,
        bandwidth: Bandwidth::K1536,
        ..Settings::default()
    }) else {
        return;
    };
    println!("RSP1A open, request 0x48 returned 0x{:02x}", rx.read48);
    let mut it = rx.start_stream_iter(256).unwrap();
    let mut bad = Vec::new();
    for (rf, off, what, min_db) in [
        (95_800_000u64, 200e3, "VHF (96 MHz)", 20.0),
        (95_900_000, 100e3, "VHF (96 MHz)", 20.0),
        (96_300_000, -300e3, "VHF (96 MHz)", 20.0),
        (72_300_000, -300e3, "VHF (72 MHz)", 20.0),
        (119_800_000, 200e3, "VHF (120 MHz)", 20.0),
        (143_800_000, 200e3, "B3 (144 MHz)", 10.0),
        (23_800_000, 200e3, "HF 12-30 MHz filter (24 MHz)", 10.0),
        (47_800_000, 200e3, "HF 30-60 MHz filter (48 MHz)", 10.0),
        (264_200_000, -200e3, "AM down-conversion (264 MHz)", 10.0),
        (312_200_000, -200e3, "300-375 MHz filter (312 MHz)", 10.0),
        (479_800_000, 200e3, "B4/5 (480 MHz)", 10.0),
        (1_199_800_000, 200e3, "L band (1200 MHz)", 10.0),
    ] {
        let m = measure(&mut rx, &mut it, rf, off);
        println!(
            "{what:32} {:>10.3} MHz: expected {:+7.1} kHz, peak {:+9.3} kHz, {:5.1} dB, {} gaps",
            rf as f64 / 1e6,
            off / 1e3,
            m.peak_hz / 1e3,
            m.over_db,
            m.gaps
        );
        if (m.peak_hz - off).abs() > 2_000.0 || m.over_db < min_db {
            bad.push(rf);
        }
    }
    // Below 2 MHz (the RSP1A's extra filter) and in the 2-12 MHz filter: no 24 MHz harmonic
    // is in reach, so report the strongest line and require only a clean stream.
    for rf in [1_000_000u64, 1_990_000, 2_010_000, 7_000_000, 100_000_000] {
        rx.set_frequency(rf).unwrap();
        let s0 = rx.stream_stats();
        let x = grab(&mut it, 100_000, NWANT);
        let s1 = rx.stream_stats();
        assert_eq!(x.len(), NWANT, "short capture at {rf}");
        let sp = power_spectrum_db(&x, NFFT);
        let (f, _, over) = peak_near(&sp, rx.output_rate(), 0.0, 700_000.0, 10_000.0);
        println!(
            "{:>10.3} MHz: strongest {:+9.3} kHz at {:5.1} dB over floor (no harmonic expected), {} gaps",
            rf as f64 / 1e6,
            f / 1e3,
            over,
            s1.gaps - s0.gaps
        );
    }
    let st = rx.stream_stats();
    println!(
        "stream: {} blocks, {} gaps, {} lost samples, dropped chunks {}",
        st.blocks,
        st.gaps,
        st.lost_samples,
        it.dropped()
    );
    drop(it);
    rx.stop_stream().unwrap();
    rx.close().unwrap();
    assert!(bad.is_empty(), "spur check failed at {bad:?}");
}

/// RSP1A: FM/MW notch, DAB notch and every LNA state and some IF gain steps while streaming,
/// reporting the 96 MHz harmonic's level (tuned 95.8 MHz) and counter gaps. Bias-T is never
/// switched. Asserts that the stream survives, that the harmonic is where expected at state
/// 0 and that IF gain reduction lowers it.
#[test]
#[ignore]
// Tests [@ANCHOR: rsp_device_rsp_set_dab_notch]
// Tests [@ANCHOR: rsp_device_rsp_write_gain]
fn hil_rsp1a_toggles_and_gain() {
    let Some(mut rx) = open_rsp1a(Settings {
        if_gr_db: 30,
        rf_hz: 95_800_000,
        ..Settings::default()
    }) else {
        return;
    };
    let mut it = rx.start_stream_iter(512).unwrap();
    let mut look = |rx: &mut Rsp, what: &str| {
        let s0 = rx.stream_stats();
        let x = grab(&mut it, 50_000, NWANT);
        let s1 = rx.stream_stats();
        assert_eq!(x.len(), NWANT, "short capture: {what}");
        let sp = power_spectrum_db(&x, NFFT);
        let (f, lvl, over) = peak_near(&sp, rx.output_rate(), 200e3, 20_000.0, 10_000.0);
        println!(
            "{what:28}: peak {:+9.3} kHz, {:6.1} dB, {:5.1} dB over floor, {} gaps",
            f / 1e3,
            lvl,
            over,
            s1.gaps - s0.gaps
        );
        (f, lvl)
    };
    let (f0, l30) = look(&mut rx, "LNA 0, IF GR 30");
    assert!((f0 - 200e3).abs() < 2e3, "harmonic at {f0}");
    rx.set_notch(true).unwrap();
    look(&mut rx, "FM/MW notch on");
    rx.set_notch(false).unwrap();
    rx.set_dab_notch(true).unwrap();
    look(&mut rx, "DAB notch on");
    rx.set_dab_notch(false).unwrap();
    look(&mut rx, "notches off");
    for l in 0..rx.lna_states() {
        rx.set_gain(30, l).unwrap();
        look(&mut rx, &format!("LNA state {l}"));
    }
    let mut l50 = f64::NAN;
    for gr in [20u8, 40, 50, 59] {
        rx.set_gain(gr, 0).unwrap();
        let (_, lvl) = look(&mut rx, &format!("LNA 0, IF GR {gr}"));
        if gr == 50 {
            l50 = lvl;
        }
    }
    let st = rx.stream_stats();
    println!("stream: {} blocks, {} gaps", st.blocks, st.gaps);
    assert!(!rx.settings().bias_t);
    drop(it);
    rx.stop_stream().unwrap();
    rx.close().unwrap();
    assert!(st.blocks > 1000);
    assert!(
        l50 < l30 - 10.0,
        "IF GR 50 ({l50}) not >10 dB below GR 30 ({l30})"
    );
}

/// A receiver in the boot-ROM state (bcdDevice 2.00): no firmware is uploaded, requests 0x48
/// and 0x49 are not sent. The tuner and stream must still work: the 24 MHz reference harmonics
/// appear where the tuning words put them (protocol document 5).
#[test]
#[ignore]
// Tests [@ANCHOR: rsp_device_rsp_open_boot_rom_with]
fn hil_boot_rom_reference_spurs() {
    let cold = list_devices()
        .map(|d| {
            d.iter()
                .any(|d| d.pid == rsp_open::PID_RSP2 && d.bcd_device == rsp_open::BCD_BOOT_ROM)
        })
        .unwrap_or(false);
    if !cold {
        eprintln!("no boot-ROM RSP2pro: skipping");
        return;
    }
    let mut rx = Rsp2::open_boot_rom_with(Settings::default()).expect("open");
    assert!(rx.is_boot_rom());
    assert!(rx.set_notch(true).is_err());
    assert!(rx.set_bias_t(true).is_err());
    let mut it = rx.start_stream_iter(256).expect("start");
    for (rf, off) in [
        (95_800_000u64, 200e3),
        (95_900_000, 100e3),
        (96_300_000, -300e3),
    ] {
        let m = measure(&mut rx, &mut it, rf, off);
        eprintln!(
            "{rf}: peak {:.1} Hz, {:.1} dB, gaps {}",
            m.peak_hz, m.over_db, m.gaps
        );
        assert!(
            (m.peak_hz - off).abs() < 400.0,
            "peak at {} for {rf}",
            m.peak_hz
        );
        assert!(m.over_db > 20.0, "only {} dB over floor at {rf}", m.over_db);
    }
    // Other bands: reported, not asserted. The board's band filters are not switched in this
    // state, so the levels show what the power-on filter state passes.
    for (rf, off) in [
        (23_800_000u64, 200e3),
        (71_800_000, 200e3),
        (119_800_000, 200e3),
        (264_200_000, -200e3),
        (480_200_000, -200e3),
        (1_200_200_000, -200e3),
    ] {
        let m = measure(&mut rx, &mut it, rf, off);
        eprintln!(
            "{rf}: peak {:.1} Hz, {:.1} dB, gaps {}",
            m.peak_hz, m.over_db, m.gaps
        );
    }
    drop(it);
    rx.stop_stream().unwrap();
    rx.close().unwrap();
}

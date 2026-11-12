// SPDX-License-Identifier: AGPL-3.0-or-later
use super::testkit::*;
use super::*;

fn bench_cfg(dir: &Path, extra: &str) -> Vec<Instrument> {
    crate::config::parse(&format!(
        r#"{{"instruments":{{"rx":{{"driver":"elad","tier":"read_config","capture_dir":"{}"{extra}}}}}}}"#,
        dir.display()
    ))
    .unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = crate::testdir::scratch(&format!("bench_elad_{name}"));
    d
}

#[test]
fn config_defaults_and_refusals() {
    // Tests [@ANCHOR: elad_config]
    let d = parse_config(&json!({})).unwrap();
    assert_eq!(
        (d.fpga_loaded, d.capture_dir.clone(), d.max_capture_s),
        (false, None, 5.0)
    );
    let c = parse_config(
        &json!({"fpga_loaded": true, "capture_dir": "/var/tmp/x", "max_capture_s": 10}),
    )
    .unwrap();
    assert!(c.fpga_loaded);
    assert_eq!(c.capture_dir.as_deref(), Some(Path::new("/var/tmp/x")));
    assert!(parse_config(&json!({"capture_dir": "relative/dir"})).is_err());
    assert!(parse_config(&json!({"max_capture_s": 0.0})).is_err());
    assert!(parse_config(&json!({"max_capture_s": 31})).is_err());
    assert!(parse_config(&json!({"fpga_loaded": "yes"})).is_err());
}

// Tests [@ANCHOR: bench_mcp_elad_testkit_fake_bench]
#[test]
fn list_reports_the_receivers_and_whether_they_stream() {
    // Tests [@ANCHOR: bench_mcp_elad_list]
    let b = FakeBench::new(vec![s2()]);
    let insts = bench_cfg(&tmp("list"), "");
    let r = call_with(&b, &insts, "bench_elad_list", &json!({})).unwrap();
    assert_eq!(r["count"], 1);
    assert_eq!(r["receivers"][0]["usb_id"], "1721:061c");
    assert_eq!(r["receivers"][0]["model"], "FDM-S2");
    assert_eq!(r["receivers"][0]["needs_fpga_image"], true);
    assert_eq!(r["receivers"][0]["streams_under_this_config"], false);
    assert!(b.opened.lock().unwrap().is_empty(), "list opens nothing");
    let insts = bench_cfg(&tmp("list2"), r#","fpga_loaded":true"#);
    let r = call_with(&b, &insts, "bench_elad_list", &json!({})).unwrap();
    assert_eq!(r["receivers"][0]["streams_under_this_config"], true);
}

#[test]
fn identify_reads_the_recorded_unit_and_sends_only_reads() {
    // Tests [@ANCHOR: bench_mcp_elad_identify]
    let b = FakeBench::new(vec![s2()]);
    let insts = bench_cfg(&tmp("ident"), "");
    let r = call_with(&b, &insts, "bench_elad_identify", &json!({})).unwrap();
    assert_eq!(r["firmware"], "3.0");
    assert_eq!(r["hardware"], "1.3");
    assert_eq!(r["serial"], "SN-0000");
    assert_eq!(r["rate_correction_hz"], -784);
    assert_eq!(r["offset_att_db"], 12.5);
    assert!(
        b.sent_requests().is_empty(),
        "identify sends no control OUT request"
    );
    let none = FakeBench::new(vec![]);
    assert!(call_with(&none, &insts, "bench_elad_identify", &json!({}))
        .unwrap_err()
        .contains("no ELAD receiver"));
}

#[test]
fn tier_instrument_and_unknown_tool_are_checked() {
    // Tests [@ANCHOR: bench_mcp_elad_call_with]
    // Tests [@ANCHOR: bench_mcp_elad_instrument]
    let b = FakeBench::new(vec![s2()]);
    let ro = crate::config::parse(
        r#"{"instruments":{"rx":{"driver":"elad","tier":"read_only","capture_dir":"/var/tmp/x"}}}"#,
    )
    .unwrap();
    assert!(call_with(&b, &ro, "bench_elad_list", &json!({})).is_ok());
    let e = call_with(
        &b,
        &ro,
        "bench_elad_capture",
        &json!({"freq_hz": 7.1e6, "seconds": 0.1, "name": "a"}),
    )
    .unwrap_err();
    assert!(e.contains("needs tier"), "{e}");
    assert!(call_with(&b, &ro, "bench_elad_nope", &json!({}))
        .unwrap_err()
        .contains("unknown tool"));
    assert!(
        call_with(&b, &ro, "bench_elad_list", &json!({"instrument": "other"}))
            .unwrap_err()
            .contains("no elad instrument named")
    );
    assert!(call_with(&b, &[], "bench_elad_list", &json!({}))
        .unwrap_err()
        .contains("no elad instrument is configured"));
}

#[test]
fn a_capture_without_the_declared_fpga_is_refused_before_streaming() {
    // Tests [@ANCHOR: elad_acquire]
    let dir = tmp("nofpga");
    let b = FakeBench::new(vec![s2()]);
    let e = call_with(
        &b,
        &bench_cfg(&dir, ""),
        "bench_elad_capture",
        &json!({"freq_hz": 7.1e6, "seconds": 0.1, "name": "x.cf32"}),
    )
    .unwrap_err();
    assert!(e.contains("FPGA image"), "{e}");
    assert!(!dir.join("x.cf32").exists(), "no file");
    assert!(
        !b.sent_requests().contains(&elad_usb::protocol::REQ_FIFO),
        "the FIFO was never started"
    );
}

#[test]
fn a_silent_unit_is_reported_as_a_missing_fpga_not_as_an_empty_capture() {
    // Tests [@ANCHOR: elad_acquire]
    let dir = tmp("silent");
    let mut b = FakeBench::new(vec![s2()]);
    b.silent = true;
    let e = call_with(
        &b,
        &bench_cfg(&dir, r#","fpga_loaded":true"#),
        "bench_elad_capture",
        &json!({"freq_hz": 7.1e6, "seconds": 0.1, "name": "x.cf32"}),
    )
    .unwrap_err();
    assert!(e.contains("FPGA image is probably not loaded"), "{e}");
    assert!(!dir.join("x.cf32").exists());
    let sent = b.sent_requests();
    assert_eq!(
        sent.last(),
        Some(&elad_usb::protocol::REQ_FIFO),
        "the stream was stopped again"
    );
}

// Tests [@ANCHOR: bench_mcp_elad_testkit_synth]
#[test]
fn capture_writes_cf32_and_a_summary_that_finds_the_tone() {
    // Tests [@ANCHOR: bench_mcp_elad_capture]
    // Tests [@ANCHOR: elad_summary]
    // Tests [@ANCHOR: elad_new_file]
    let dir = tmp("cap");
    let b = FakeBench::new(vec![s2()]);
    let insts = bench_cfg(&dir, r#","fpga_loaded":true"#);
    let r = call_with(
        &b,
        &insts,
        "bench_elad_capture",
        &json!({"freq_hz": 7.1e6, "seconds": 0.25, "name": "t.cf32", "rate_hz": 192000}),
    )
    .unwrap();
    let n = r["samples"].as_u64().unwrap() as usize;
    assert_eq!(n, 48_000);
    let bytes = std::fs::read(dir.join("t.cf32")).unwrap();
    assert_eq!(bytes.len(), n * 8);
    assert_eq!(r["file_bytes"].as_u64().unwrap() as usize, n * 8);
    let rms = r["rms"].as_f64().unwrap();
    assert!((rms - 0.1).abs() < 0.001, "tone amplitude 0.1, rms {rms}");
    assert_eq!(r["clipped_samples"], 0);
    assert!(r["dc_i"].as_f64().unwrap().abs() < 0.01);
    let sent = b.sent_requests();
    assert!(
        sent.contains(&elad_usb::protocol::REQ_TUNE)
            && sent.contains(&elad_usb::protocol::REQ_FIFO)
    );
    assert!(
        sent.iter().all(|r| [0xF2, 0xF7, 0xE9].contains(r)),
        "only receive requests: {sent:?}"
    );
    // an existing file is never overwritten
    let e = call_with(
        &b,
        &insts,
        "bench_elad_capture",
        &json!({"freq_hz": 7.1e6, "seconds": 0.1, "name": "t.cf32"}),
    )
    .unwrap_err();
    assert!(e.contains("already exists"), "{e}");
    for bad in ["", "../x", ".hidden", "a/b", "sp ace"] {
        let e = call_with(
            &b,
            &insts,
            "bench_elad_capture",
            &json!({"freq_hz": 7.1e6, "seconds": 0.1, "name": bad}),
        )
        .unwrap_err();
        assert!(e.contains("name"), "{bad:?}: {e}");
    }
    let nodir =
        crate::config::parse(r#"{"instruments":{"rx":{"driver":"elad","tier":"read_config"}}}"#)
            .unwrap();
    assert!(call_with(
        &b,
        &nodir,
        "bench_elad_capture",
        &json!({"freq_hz": 7.1e6, "seconds": 0.1, "name": "z"})
    )
    .unwrap_err()
    .contains("capture_dir"));
}

#[test]
fn the_six_point_one_msps_rate_decodes_sixteen_bit_samples() {
    // Tests [@ANCHOR: elad_target_samples]
    let dir = tmp("rate6");
    let b = FakeBench::new(vec![s2()]);
    let insts = bench_cfg(&dir, r#","fpga_loaded":true"#);
    let r = call_with(
        &b,
        &insts,
        "bench_elad_capture",
        &json!({"freq_hz": 14.2e6, "seconds": 0.05, "name": "w.cf32", "rate_hz": 6144000}),
    )
    .unwrap();
    assert_eq!(r["rate_hz"], 6_144_000);
    assert!((r["rms"].as_f64().unwrap() - 0.1).abs() < 0.002);
    let e = call_with(
        &b,
        &insts,
        "bench_elad_capture",
        &json!({"freq_hz": 14.2e6, "seconds": 4.0, "name": "big.cf32", "rate_hz": 6144000}),
    )
    .unwrap_err();
    assert!(e.contains("fit in one call"), "{e}");
    let e = call_with(
        &b,
        &insts,
        "bench_elad_capture",
        &json!({"freq_hz": 14.2e6, "seconds": 9.0, "name": "long.cf32"}),
    )
    .unwrap_err();
    assert!(e.contains("max_capture_s"), "{e}");
}

#[test]
fn arguments_are_checked_before_the_receiver_is_touched() {
    // Tests [@ANCHOR: elad_args]
    // Tests [@ANCHOR: elad_flag]
    // Tests [@ANCHOR: elad_rate_arg]
    let dir = tmp("args");
    let b = FakeBench::new(vec![s2()]);
    let insts = bench_cfg(&dir, r#","fpga_loaded":true"#);
    let run = |a: Value| call_with(&b, &insts, "bench_elad_capture", &a).unwrap_err();
    assert!(run(json!({"seconds": 0.1, "name": "a"})).contains("freq_hz is required"));
    assert!(run(json!({"freq_hz": "7", "seconds": 0.1, "name": "a"}))
        .contains("freq_hz must be a number"));
    assert!(
        run(json!({"freq_hz": 7.1e6, "seconds": 0.1, "name": "a", "rate_hz": 500000}))
            .contains("is not one of the FDM-S2 rates")
    );
    assert!(
        run(json!({"freq_hz": 7.1e6, "seconds": 0.1, "name": "a", "filters": "no"}))
            .contains("filters must be true or false")
    );
    assert!(run(json!({"freq_hz": 60e6, "seconds": 0.1, "name": "a"})).contains("outside"));
    assert!(run(json!({"freq_hz": f64::NAN, "seconds": 0.1, "name": "a"})).contains("freq_hz"));
    assert!(
        b.sent_requests().is_empty() || b.sent_requests().iter().all(|r| [0xF2, 0xF7].contains(r)),
        "no stream was started"
    );
}

#[test]
fn front_end_switches_reach_the_receiver_and_a_duo_refuses_them() {
    // Tests [@ANCHOR: elad_acquire]
    let dir = tmp("fe");
    let b = FakeBench::new(vec![s2()]);
    let insts = bench_cfg(&dir, r#","fpga_loaded":true"#);
    call_with(&b, &insts, "bench_elad_capture", &json!({"freq_hz": 7.1e6, "seconds": 0.05, "name": "a", "attenuator": true, "filters": false})).unwrap();
    use elad_usb::transport::Op;
    let codes: Vec<u16> = b.opened.lock().unwrap()[0]
        .ops()
        .into_iter()
        .filter_map(|o| {
            if let Op::Out {
                request: 0xF7,
                value,
                ..
            } = o
            {
                Some(value)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        codes.last(),
        Some(&0x09),
        "filters bypassed (0x01) with the attenuator (0x08)"
    );
}

#[test]
fn spectrum_finds_the_tone_and_writes_a_csv() {
    // Tests [@ANCHOR: bench_mcp_elad_spectrum]
    // Tests [@ANCHOR: elad_fft_len]
    let dir = tmp("spec");
    let b = FakeBench::new(vec![s2()]);
    let insts = bench_cfg(&dir, r#","fpga_loaded":true"#);
    let r = call_with(
        &b,
        &insts,
        "bench_elad_spectrum",
        &json!({"freq_hz": 7.1e6, "name": "s.csv", "fft_len": 4096}),
    )
    .unwrap();
    assert_eq!(r["bins"], 4096);
    let p = &r["peaks"][0];
    let off = p["offset_hz"].as_f64().unwrap();
    assert!(
        (off - 10_000.0).abs() < 100.0,
        "tone at +10 kHz, found at {off}"
    );
    let level = p["power_dbfs"].as_f64().unwrap();
    assert!(
        (level - (-20.0)).abs() < 1.6,
        "amplitude 0.1 is -20 dBFS, got {level}"
    );
    let csv = std::fs::read_to_string(dir.join("s.csv")).unwrap();
    assert_eq!(csv.lines().count(), 4097);
    assert!(csv.starts_with("offset_hz,freq_hz,power_dbfs"));
    let e = call_with(
        &b,
        &insts,
        "bench_elad_spectrum",
        &json!({"freq_hz": 7.1e6, "name": "s2.csv", "fft_len": 3000}),
    )
    .unwrap_err();
    assert!(e.contains("power of two"), "{e}");
}

#[test]
fn level_returns_the_tone_in_the_band_and_an_unverified_dbm() {
    // Tests [@ANCHOR: bench_mcp_elad_level]
    let dir = tmp("lvl");
    let mut b = FakeBench::new(vec![s2()]);
    b.tone_hz = 500.0;
    let insts = bench_cfg(&dir, r#","fpga_loaded":true"#);
    let r = call_with(&b, &insts, "bench_elad_level", &json!({"freq_hz": 7.1e6})).unwrap();
    let peak = r["peak_bin_dbfs"].as_f64().unwrap();
    assert!((peak - (-20.0)).abs() < 1.6, "got {peak}");
    assert!(r["band_power_dbfs"].as_f64().unwrap() >= peak);
    let est = r["level_dbm_estimate_unverified"].as_f64().unwrap();
    assert!((est - (peak + r["calibration_offset_db"].as_f64().unwrap())).abs() < 1e-9);
    assert!(call_with(
        &b,
        &insts,
        "bench_elad_level",
        &json!({"freq_hz": 7.1e6, "bandwidth_hz": 5})
    )
    .is_err());
}

#[test]
fn tool_defs_list_only_what_the_tier_allows() {
    let ro = crate::config::parse(r#"{"instruments":{"rx":{"driver":"elad","tier":"read_only"}}}"#)
        .unwrap();
    let names: Vec<String> = tool_defs(&ro).iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names, vec!["bench_elad_list", "bench_elad_identify"]);
    let rc = bench_cfg(&tmp("defs"), "");
    assert_eq!(tool_defs(&rc).len(), 8);
    assert!(tool_defs(&[]).is_empty());
}

/// Expected noise floor in dBFS for Gaussian noise of `sigma` per component through a Hann-windowed FFT of `n` points.
fn expected_floor_db(sigma: f64, n: usize) -> f64 {
    10.0 * (2.0 * sigma * sigma * 1.5 / n as f64).log10()
}

fn noisy_bench() -> FakeBench {
    let mut b = FakeBench::new(vec![s2()]);
    b.noise_rms = 0.01;
    b
}

#[test]
fn tune_applies_the_state_and_streams_nothing() {
    // Tests [@ANCHOR: bench_mcp_elad_tune]
    // Tests [@ANCHOR: elad_open_tuned]
    let b = FakeBench::new(vec![s2()]);
    let insts = bench_cfg(&tmp("tune"), "");
    let r = call_with(
        &b,
        &insts,
        "bench_elad_tune",
        &json!({"freq_hz": 7.1e6, "attenuator": true}),
    )
    .unwrap();
    assert_eq!(r["streaming"], false);
    assert_eq!(r["tuned_hz"], 7.1e6);
    let word = elad_usb::protocol::tuning_word(7.1e6, 122.88e6);
    assert_eq!(r["tuning_word"], word);
    let sent = b.sent_requests();
    assert!(sent.contains(&elad_usb::protocol::REQ_TUNE));
    assert!(sent.contains(&elad_usb::protocol::REQ_FRONT_END));
    assert!(
        !sent.contains(&elad_usb::protocol::REQ_FIFO),
        "tune must not start the FIFO: {sent:?}"
    );
    let e = call_with(&b, &insts, "bench_elad_tune", &json!({"freq_hz": 99e6})).unwrap_err();
    assert!(e.contains("out of range"), "{e}");
}

#[test]
fn detect_finds_a_tone_and_reports_its_frequency_and_snr_against_the_known_values() {
    // Tests [@ANCHOR: bench_mcp_elad_detect]
    // Tests [@ANCHOR: elad_refine_peak]
    // Tests [@ANCHOR: elad_start_stream]
    // Tests [@ANCHOR: elad_stream_samples]
    let mut b = noisy_bench();
    // 7.1123 MHz is 12,300 Hz above the centre and between bins (bin 46.875 Hz).
    b.tones_rf = vec![(7_112_300.0 + 7.0, 0.05)];
    let insts = bench_cfg(&tmp("detect"), r#","fpga_loaded":true"#);
    let r = call_with(&b, &insts, "bench_elad_detect", &json!({"freq_hz": 7.1e6})).unwrap();
    assert_eq!(r["detected"], true, "{r}");
    let s = &r["signals"][0];
    let f = s["freq_hz"].as_f64().unwrap();
    assert!(
        (f - 7_112_307.0).abs() < 5.0,
        "frequency {f} should be within 5 Hz of 7112307"
    );
    let floor = r["noise_floor_dbfs"].as_f64().unwrap();
    assert!(
        (floor - expected_floor_db(0.01, 4096)).abs() < 1.0,
        "floor {floor}"
    );
    let power = s["power_dbfs"].as_f64().unwrap();
    assert!(
        (power - 20.0 * 0.05f64.log10()).abs() < 1.6,
        "power {power}"
    );
    let snr = s["snr_db"].as_f64().unwrap();
    let want = 20.0 * 0.05f64.log10() - expected_floor_db(0.01, 4096);
    assert!((snr - want).abs() < 2.0, "snr {snr}, expected about {want}");
}

#[test]
fn detect_reports_nothing_in_noise_and_ignores_a_tone_outside_the_usable_span() {
    // Tests [@ANCHOR: bench_mcp_elad_detect]
    let mut quiet = noisy_bench();
    quiet.tones_rf = vec![(5.0e6, 0.05)];
    let insts = bench_cfg(&tmp("detect2"), r#","fpga_loaded":true"#);
    let r = call_with(
        &quiet,
        &insts,
        "bench_elad_detect",
        &json!({"freq_hz": 7.1e6}),
    )
    .unwrap();
    assert_eq!(r["detected"], false, "{r}");
    // A tone 90 kHz off a 192 kHz span is beyond the usable 80 % (76.8 kHz half span).
    let mut edge = noisy_bench();
    edge.tones_rf = vec![(7.1e6 + 90_000.0, 0.05)];
    let r = call_with(
        &edge,
        &insts,
        "bench_elad_detect",
        &json!({"freq_hz": 7.1e6}),
    )
    .unwrap();
    assert_eq!(r["detected"], false, "{r}");
    assert!(call_with(
        &quiet,
        &insts,
        "bench_elad_detect",
        &json!({"freq_hz": 7.1e6, "threshold_db": 1})
    )
    .is_err());
}

// Tests [@ANCHOR: bench_mcp_elad_measure_sweep_plan]
#[test]
fn sweep_finds_each_tone_at_its_frequency_across_steps_and_writes_the_csv() {
    // Tests [@ANCHOR: bench_mcp_elad_sweep]
    // Tests [@ANCHOR: elad_sweep_plan]
    // Tests [@ANCHOR: elad_sweep_peaks]
    let mut b = noisy_bench();
    // Both on bins of the 1024-point sweep spectrum (187.5 Hz) and away from the step centres (7038400 + 76800 k), where the DC bins are
    // not reported.
    let t1 = 7_100_000.0 + 187.5 * 400.0;
    let t2 = 7_240_000.0 + 187.5 * 100.0;
    b.tones_rf = vec![(t1, 0.05), (t2, 0.02)];
    let dir = tmp("sweep");
    let insts = bench_cfg(&dir, r#","fpga_loaded":true"#);
    let r = call_with(
        &b,
        &insts,
        "bench_elad_sweep",
        &json!({"start_hz": 7.0e6, "stop_hz": 7.3e6, "name": "sw.csv"}),
    )
    .unwrap();
    let sigs = r["signals"].as_array().unwrap();
    assert!(sigs.len() >= 2, "{r}");
    let near = |want: f64| {
        sigs.iter()
            .find(|s| (s["freq_hz"].as_f64().unwrap() - want).abs() <= 190.0)
            .unwrap_or_else(|| panic!("no signal near {want} in {r}"))
            .clone()
    };
    let s1 = near(t1);
    let s2_ = near(t2);
    let d = s1["power_dbfs"].as_f64().unwrap() - s2_["power_dbfs"].as_f64().unwrap();
    assert!(
        (d - 20.0 * (0.05f64 / 0.02).log10()).abs() < 1.5,
        "level ratio {d} dB"
    );
    let csv = std::fs::read_to_string(dir.join("sw.csv")).unwrap();
    assert!(csv.starts_with("freq_hz,power_dbfs\n"));
    let first: f64 = csv
        .lines()
        .nth(1)
        .unwrap()
        .split(',')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let last: f64 = csv
        .lines()
        .last()
        .unwrap()
        .split(',')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        first <= 7.0e6 + 200.0 && last >= 7.3e6 - 200.0,
        "{first} {last}"
    );
    let floor = r["noise_floor_dbfs"].as_f64().unwrap();
    assert!(
        (floor - expected_floor_db(0.01, 1024)).abs() < 1.5,
        "floor {floor}"
    );
    // Never overwrites.
    assert!(call_with(
        &b,
        &insts,
        "bench_elad_sweep",
        &json!({"start_hz": 7.0e6, "stop_hz": 7.3e6, "name": "sw.csv"})
    )
    .is_err());
}

#[test]
fn a_sweep_is_bounded_and_range_checked_before_the_receiver_is_opened() {
    // Tests [@ANCHOR: elad_sweep_plan]
    let b = noisy_bench();
    let insts = bench_cfg(&tmp("sweepbad"), r#","fpga_loaded":true"#);
    let call = |args: Value| call_with(&b, &insts, "bench_elad_sweep", &args).unwrap_err();
    assert!(
        call(json!({"start_hz": 7.3e6, "stop_hz": 7.0e6, "name": "a"})).contains("above start_hz")
    );
    assert!(call(json!({"start_hz": 1e6, "stop_hz": 50e6, "name": "a"})).contains("steps"));
    assert!(
        call(json!({"start_hz": 7e6, "stop_hz": 7.3e6, "step_hz": 1e6, "name": "a"}))
            .contains("step_hz")
    );
    // a band that passes the step and time limits is still refused when it ends above the receiver's range
    assert!(call(
        json!({"start_hz": 7e6, "stop_hz": 60e6, "step_hz": 150000, "rate_hz": 192000, "name": "a"})
    )
    .contains("is outside the"));
    assert!(call(
        json!({"start_hz": 10, "stop_hz": 100000, "name": "a", "rate_hz": 6144000, "step_hz": 100})
    )
    .contains("steps"));
    assert!(
        b.opened.lock().unwrap().is_empty(),
        "nothing was opened for a refused sweep"
    );
}

#[test]
fn the_lock_keeps_a_second_owner_out_while_the_radio_lives() {
    // Tests [@ANCHOR: elad_take_lock]
    // Tests [@ANCHOR: elad_open_locked]
    let d = tmp("lockdir");
    let f = s2();
    let lock = take_lock_in(&d, &f).unwrap();
    let ft = elad_usb::transport::FakeTransport::new();
    let radio = open_locked(
        lock,
        Box::new(ft),
        &f,
        OpenOptions {
            fpga: FpgaState::Unknown,
            sample_rate_hz: 192_000,
        },
    )
    .unwrap();
    let e = take_lock_in(&d, &f).unwrap_err();
    assert!(
        e.contains("in use by pid") && e.contains("bench_mcp"),
        "{e}"
    );
    drop(radio);
    assert!(take_lock_in(&d, &f).is_ok(), "released with the radio");
}

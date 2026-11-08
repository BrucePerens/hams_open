// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware-free tests of the Glasgow module: parsers against recordings, the production code path against a simulated device
//! (`testkit.rs`) with the real helper script, and the real process runner against shell scripts. Every test uses its own instrument
//! name, because the session record is keyed by it and tests run in parallel.

use super::runner::{self, RealRunner, Req, Runner};
use super::testkit::{fixture, Sim, SERIAL};
use super::*;
use serde_json::json;
use std::sync::Arc;

fn bench(name: &str, extra: &str) -> Vec<Instrument> {
    crate::config::parse(&format!(
        r#"{{"instruments":{{"{name}":{{"driver":"glasgow","tier":"full","serial":"{SERIAL}"{extra}}}}}}}"#
    ))
    .unwrap()
}

struct Rig {
    sim: Arc<Sim>,
    ctx: Ctx,
    insts: Vec<Instrument>,
    dir: PathBuf,
}

fn rig(name: &str, extra: &str) -> Rig {
    let dir =
        std::env::temp_dir().join(format!("bench_glasgow_test_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sys/1-1")).unwrap();
    std::fs::create_dir_all(dir.join("out")).unwrap();
    for (f, v) in [
        ("idVendor", "20b7"),
        ("idProduct", "9db1"),
        ("speed", "480"),
        ("product", "Glasgow Interface Explorer"),
        ("manufacturer", "1BitSquared"),
        ("busnum", "1"),
        ("devnum", "4"),
    ] {
        std::fs::write(dir.join("sys/1-1").join(f), format!("{v}\n")).unwrap();
    }
    let sim = Arc::new(Sim::new(dir.clone()));
    let ctx = Ctx {
        runner: sim.clone(),
        sysfs: dir.join("sys"),
        out_dir: dir.join("out"),
        lock_path: dir.join("lock"),
    };
    Rig {
        sim,
        ctx,
        insts: bench(name, extra),
        dir,
    }
}

impl Rig {
    fn call(&self, tool: &str, args: Value) -> Result<Value, String> {
        call_with(&self.ctx, &self.insts, tool, &args)
    }
    fn set_port(&self, port: &str, v: f64) -> Value {
        self.call(
            "bench_glasgow_port_set",
            json!({"port": port, "volts_v": v, "confirm": true}),
        )
        .unwrap()
    }
    fn speed(&self, mbps: &str) {
        std::fs::write(self.dir.join("sys/1-1/speed"), mbps).unwrap();
    }
}

// Tests [@ANCHOR: glasgow_config]
#[test]
fn config_has_defaults_and_refuses_bad_values() {
    let v = bench("g_cfg", "");
    assert_eq!(v[0].glasgow, Config::default());
    let v = bench(
        "g_cfg2",
        r#","venv":"/opt/g","max_capture_s":10,"max_capture_mb":8,"op_timeout_s":30,"build_timeout_s":90,"max_trip_a":0.2,"max_volts":3.6"#,
    );
    assert_eq!(v[0].glasgow.venv, Some(PathBuf::from("/opt/g")));
    assert_eq!(
        (
            v[0].glasgow.max_capture_s,
            v[0].glasgow.max_capture_mb,
            v[0].glasgow.max_trip_a
        ),
        (10.0, 8, 0.2)
    );
    for bad in [
        r#","venv":"relative/dir""#,
        r#","max_capture_s":0"#,
        r#","max_capture_s":500"#,
        r#","max_trip_a":0.5"#,
        r#","op_timeout_s":1"#,
        r#","max_volts":6.0"#,
        r#","serial":"nonsense""#,
    ] {
        let r = crate::config::parse(&format!(
            r#"{{"instruments":{{"g":{{"driver":"glasgow"{bad}}}}}}}"#
        ));
        assert!(r.is_err(), "{bad} must be refused");
    }
}

// Tests [@ANCHOR: glasgow_base_args]
// Tests [@ANCHOR: glasgow_real_ctx]
#[test]
fn the_serial_is_passed_on_and_the_real_context_lives_in_the_install() {
    let v = bench("g_real", r#","venv":"/opt/gx""#);
    assert_eq!(
        base_args(&v[0]),
        vec!["--serial".to_string(), SERIAL.to_string()]
    );
    let none = crate::config::parse(r#"{"instruments":{"g":{"driver":"glasgow"}}}"#).unwrap();
    assert!(base_args(&none[0]).is_empty());
    let c = real_ctx(&v[0]);
    assert_eq!(
        (c.lock_path.clone(), c.sysfs.clone()),
        (
            PathBuf::from("/opt/gx/glasgow.lock"),
            PathBuf::from("/sys/bus/usb/devices")
        )
    );
}

// Tests [@ANCHOR: glasgow_install_dir]
// Tests [@ANCHOR: glasgow_install_paths]
#[test]
fn install_dir_is_configured_or_the_pinned_default() {
    let c = Config {
        venv: Some(PathBuf::from("/opt/x")),
        ..Config::default()
    };
    assert_eq!(install_dir(&c), PathBuf::from("/opt/x"));
    assert!(install_dir(&Config::default()).ends_with(".local/share/bench_mcp/glasgow-70cb38c9"));
    let (prog, cache) = runner::install_paths(Path::new("/opt/x"));
    assert_eq!(
        (prog, cache),
        (
            PathBuf::from("/opt/x/venv/bin/glasgow"),
            PathBuf::from("/opt/x/cache")
        )
    );
}

// Tests [@ANCHOR: glasgow_parse_voltage_table]
// Tests [@ANCHOR: glasgow_parse_num]
#[test]
fn the_voltage_table_of_a_real_device_parses_and_garbage_does_not() {
    let rows = parse::voltage_table(&fixture("voltage_idle.txt")).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (
            rows[0].port.as_str(),
            rows[0].vsupply_v,
            rows[0].vlimit_v,
            rows[0].ialert_a
        ),
        ("A", 0.0, 5.5, Some(0.1))
    );
    assert!(!rows[1].fault && !rows[1].short);
    let t = "Port\tVsupply\tVlimit\tIsupply\tIalert\tVsense\tVsense(alert)\nA\t3.300\t3.300\t(SHORT)\t0.100\t0.000\t3.0-3.6\t(FAULT)\n";
    let r = parse::voltage_table(t).unwrap();
    assert!(r[0].short && r[0].fault && r[0].isupply_a.is_none());
    assert!(parse::voltage_table("Port\tVsupply\nA\tnan\t5\n").is_err());
    assert!(parse::voltage_table("Port\tVsupply\nA\tx\ty\n").is_err());
    assert!(parse::voltage_table("nothing here").is_err());
    assert!(parse::voltage_table("A\t1.0\n").is_err());
}

// Tests [@ANCHOR: glasgow_parse_device_list]
// Tests [@ANCHOR: glasgow_serial_ok]
// Tests [@ANCHOR: glasgow_revision_limits]
// Tests [@ANCHOR: glasgow_parse_log_problems]
#[test]
fn serials_revisions_and_log_problems() {
    assert_eq!(
        parse::device_list(&fixture("list.txt")),
        vec![SERIAL.to_string()]
    );
    assert!(parse::device_list("W: glasgow.cli: no devices available\n").is_empty());
    assert!(
        parse::serial_ok(SERIAL)
            && !parse::serial_ok("C3-2020")
            && !parse::serial_ok("E3-20200101T000000Z")
    );
    assert_eq!(
        parse::revision_limits(SERIAL),
        Some(("C3".into(), 1.65, 5.0))
    );
    assert_eq!(
        parse::revision_limits("D0-20250101T000000Z").unwrap().2,
        5.5
    );
    assert!(parse::revision_limits("junk").is_none());
    let p = parse::log_problems(&fixture("list_stderr.txt"));
    assert_eq!(p.len(), 1);
    assert!(p[0].starts_with("W: ") && p[0].contains("glasgow flash"));
}

// Tests [@ANCHOR: glasgow_usb_device]
#[test]
fn the_usb_device_and_its_link_speed_come_from_sysfs() {
    let r = rig("g_usb", "");
    let u = parse::usb_device(&r.ctx.sysfs).unwrap();
    assert_eq!(
        (
            u["link_speed_mbps"].as_f64(),
            u["high_speed"].as_bool(),
            u["usb_id"].as_str()
        ),
        (Some(480.0), Some(true), Some("20b7:9db1"))
    );
    r.speed("12\n");
    assert_eq!(
        parse::usb_device(&r.ctx.sysfs).unwrap()["high_speed"],
        json!(false)
    );
    assert!(parse::usb_device(&r.dir.join("nonexistent")).is_none());
}

// Tests [@ANCHOR: glasgow_parse_pins]
#[test]
fn pin_specs_expand_and_bad_ones_are_refused() {
    assert_eq!(
        parse::pins("A0:2,B7").unwrap(),
        vec![('A', 0), ('A', 1), ('A', 2), ('B', 7)]
    );
    for bad in [
        "",
        "C0",
        "A8",
        "A3:1",
        "A0:3,A2",
        "A0:",
        "a0",
        "A0:7,B0:7,A1",
    ] {
        assert!(parse::pins(bad).is_err(), "{bad:?}");
    }
    assert_eq!(parse::pins("A0:7,B0:7").unwrap().len(), 16);
}

// Tests [@ANCHOR: glasgow_parse_jtag_taps]
// Tests [@ANCHOR: glasgow_parse_result_line]
// Tests [@ANCHOR: glasgow_parse_hex_bytes]
#[test]
fn applet_log_lines_result_lines_and_hex_parse() {
    let taps = parse::jtag_taps(&fixture("jtag_scan_log.txt"));
    assert_eq!(taps.len(), 1);
    assert_eq!(
        (
            taps[0]["idcode"].as_str(),
            taps[0]["version"].as_u64(),
            taps[0]["part"].as_u64(),
            taps[0]["manufacturer_id"].as_u64()
        ),
        (Some("0x4ba00477"), Some(4), Some(0xba00), Some(0x23b))
    );
    let bypass = parse::jtag_taps("I: x: TAP #1: IR[4] BYPASS\n");
    assert_eq!(bypass[0]["bypass"], json!(true));
    assert_eq!(
        parse::result_line("noise\nBENCH_GLASGOW_RESULT {\"a\": 1}\n").unwrap(),
        json!({"a": 1})
    );
    assert!(parse::result_line("no result").is_err());
    assert!(parse::result_line("BENCH_GLASGOW_RESULT {bad").is_err());
    assert_eq!(
        parse::hex_bytes("0x48, 65 6c").unwrap(),
        vec![0x48, 0x65, 0x6c]
    );
    assert!(parse::hex_bytes("abc").is_err() && parse::hex_bytes("zz").is_err());
    assert!(parse::hex_bytes("").unwrap().is_empty());
}

// Tests [@ANCHOR: glasgow_vcd_summary]
#[test]
fn a_vcd_capture_is_summarised_per_pin() {
    let s = parse::vcd_summary(&fixture("capture.vcd")).unwrap();
    assert_eq!(s["duration_ns"], json!(5_000_000));
    let clk = &s["pins"]["clk"];
    assert_eq!(clk["transitions"], json!(10));
    assert_eq!(clk["min_pulse_ns"], json!(500_000));
    let f = clk["rising_edge_frequency_hz"].as_f64().unwrap();
    assert!((f - 1000.0).abs() < 1.0, "{f}");
    let burst = &s["pins"]["burst"];
    assert_eq!(
        (
            burst["transitions"].as_u64(),
            burst["min_pulse_ns"].as_u64(),
            burst["unknown_samples"].as_u64()
        ),
        (Some(4), Some(100_000), Some(0))
    );
    assert!(parse::vcd_summary("$enddefinitions $end\n").is_err());
    let x = parse::vcd_summary("$var wire 1 ! a $end\n$enddefinitions $end\n#0\n0!\n#10\nx!\n")
        .unwrap();
    assert_eq!(x["pins"]["a"]["unknown_samples"], json!(1));
}

// Tests [@ANCHOR: glasgow_uart_csv_summary]
#[test]
fn a_uart_capture_is_summarised_per_channel() {
    let s = parse::uart_csv_summary(&fixture("uart_analyzer.csv"), 4096).unwrap();
    assert_eq!(
        (
            s["rx"]["bytes"].as_u64(),
            s["rx"]["hex"].as_str(),
            s["rx"]["frame_errors"].as_u64()
        ),
        (Some(9), Some("48656c6c6f0d0a4f4b"), Some(1))
    );
    assert_eq!(s["rx"]["text"], json!("Hello\\r\\nOK"));
    assert_eq!(s["tx"]["text"], json!("AT\\r"));
    let cut = parse::uart_csv_summary("rx,0102030405\n", 2).unwrap();
    assert_eq!(
        (cut["rx"]["truncated"].as_bool(), cut["rx"]["hex"].as_str()),
        (Some(true), Some("0102"))
    );
    assert!(
        parse::uart_csv_summary("zz,01\n", 10).is_err()
            && parse::uart_csv_summary("rx,xyz\n", 10).is_err()
            && parse::uart_csv_summary("nocomma\n", 10).is_err()
    );
}

fn script(dir: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join("fake_glasgow");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn rq(args: &[&str], secs: u64) -> Req {
    Req {
        args: args.iter().map(|s| s.to_string()).collect(),
        timeout: Duration::from_secs(secs),
        stop_after: None,
        watch: None,
        nice: false,
    }
}

// Tests [@ANCHOR: glasgow_real_runner]
// Tests [@ANCHOR: glasgow_runner_command]
#[test]
fn the_real_runner_captures_output_stops_with_sigint_and_never_kills() {
    let dir = std::env::temp_dir().join(format!("bench_glasgow_runner_{}", std::process::id()));
    let prog = script(
        &dir,
        r#"case "$1" in
 echo) echo out-line; echo err-line >&2; exit 3;;
 sleep) exec sleep 30;;
 deaf) trap '' INT; exec sleep 30;;
 big) head -c 3000000 /dev/zero > "$2"; exec sleep 30;;
 env) echo "$XDG_CACHE_HOME $PYTHONNOUSERSITE";;
esac"#,
    );
    let r = RealRunner {
        program: prog,
        cache_dir: dir.join("cache"),
    };
    let o = r.run(&rq(&["echo"], 10)).unwrap();
    assert_eq!(
        (
            o.code,
            o.stdout.trim(),
            o.stderr.trim(),
            o.timed_out,
            o.stopped
        ),
        (Some(3), "out-line", "err-line", false, false)
    );
    let o = r.run(&rq(&["env"], 10)).unwrap();
    assert_eq!(
        o.stdout.trim(),
        format!("{} 1", dir.join("cache").display())
    );
    // stop_after: SIGINT, counted as a deliberate stop.
    let t = Instant::now();
    let o = r
        .run(&Req {
            stop_after: Some(Duration::from_millis(300)),
            ..rq(&["sleep"], 20)
        })
        .unwrap();
    assert!(
        o.stopped && !o.timed_out && !o.stuck && t.elapsed() < Duration::from_secs(5),
        "{o:?}"
    );
    // The hard timeout ends it the same way and says so.
    let o = r.run(&rq(&["sleep"], 1)).unwrap();
    assert!(o.timed_out && !o.stopped && !o.stuck, "{o:?}");
    // A growing file past the limit stops the child.
    let big = dir.join("big.out");
    let o = r
        .run(&Req {
            watch: Some((big.clone(), 1_000_000)),
            ..rq(&["big", big.to_str().unwrap()], 20)
        })
        .unwrap();
    assert!(o.stopped, "{o:?}");
    // A child that ignores SIGINT is ended with SIGTERM after 10 s.
    let t = Instant::now();
    let o = r.run(&rq(&["deaf"], 1)).unwrap();
    assert!(
        o.timed_out && !o.stuck && t.elapsed() > Duration::from_secs(9),
        "{o:?}"
    );
    let missing = RealRunner {
        program: dir.join("absent"),
        cache_dir: dir.clone(),
    };
    assert!(missing
        .run(&rq(&["list"], 5))
        .unwrap_err()
        .contains("install_glasgow.py"));
    let _ = std::fs::remove_dir_all(&dir);
}

// Tests [@ANCHOR: glasgow_tool_defs]
// Tests [@ANCHOR: glasgow_instrument]
#[test]
fn tools_are_listed_by_tier_and_the_instrument_is_chosen_exactly() {
    let all = |t: &str| {
        let v = crate::config::parse(&format!(
            r#"{{"instruments":{{"g":{{"driver":"glasgow","tier":"{t}"}}}}}}"#
        ))
        .unwrap();
        tool_defs(&v)
            .iter()
            .map(|t| t.name.to_string())
            .collect::<Vec<_>>()
    };
    let ro = all("read_only");
    assert_eq!(
        ro,
        [
            "bench_glasgow_list",
            "bench_glasgow_status",
            "bench_glasgow_applets"
        ]
    );
    let rc = all("read_config");
    assert!(
        rc.contains(&"bench_glasgow_safe_state".to_string())
            && rc.contains(&"bench_glasgow_logic_capture".to_string())
            && !rc.contains(&"bench_glasgow_port_set".to_string())
    );
    let full = all("full");
    assert!(
        full.contains(&"bench_glasgow_port_set".to_string())
            && full.contains(&"bench_glasgow_pattern".to_string())
            && full.len() == 17
    );
    let two = crate::config::parse(
        r#"{"instruments":{"a":{"driver":"glasgow"},"b":{"driver":"glasgow"}}}"#,
    )
    .unwrap();
    assert!(instrument(&two, &json!({}))
        .unwrap_err()
        .contains("name the instrument"));
    assert_eq!(
        instrument(&two, &json!({"instrument": "b"})).unwrap().name,
        "b"
    );
    assert!(instrument(&two, &json!({"instrument": "zz"})).is_err());
    assert!(instrument(&[], &json!({}))
        .unwrap_err()
        .contains("no glasgow instrument"));
}

// Tests [@ANCHOR: glasgow_call_with]
#[test]
fn a_tool_above_the_tier_or_unknown_is_refused_before_any_command() {
    let mut r = rig("g_tier", "");
    r.insts = crate::config::parse(&format!(r#"{{"instruments":{{"g_tier":{{"driver":"glasgow","tier":"read_only","serial":"{SERIAL}"}}}}}}"#)).unwrap();
    let e = r
        .call(
            "bench_glasgow_port_set",
            json!({"port": "A", "volts_v": 3.3, "confirm": true}),
        )
        .unwrap_err();
    assert!(e.contains("needs tier"), "{e}");
    assert!(r
        .call("bench_glasgow_nope", json!({}))
        .unwrap_err()
        .contains("unknown tool"));
    assert!(r.sim.calls().is_empty());
}

// Tests [@ANCHOR: glasgow_list]
// Tests [@ANCHOR: glasgow_device_lock]
#[test]
fn list_reports_the_device_the_link_and_the_install() {
    let dir = std::env::temp_dir().join(format!("bench_glasgow_install_{}", std::process::id()));
    std::fs::create_dir_all(dir.join("venv/bin")).unwrap();
    std::fs::create_dir_all(dir.join("cache/GlasgowEmbedded/bitstreams")).unwrap();
    std::fs::write(dir.join("venv/bin/glasgow"), "").unwrap();
    std::fs::write(
        dir.join("INSTALLED.json"),
        r#"{"glasgow_commit":"70cb38c9"}"#,
    )
    .unwrap();
    let r = rig("g_list", &format!(r#","venv":"{}""#, dir.display()));
    let v = r.call("bench_glasgow_list", json!({})).unwrap();
    assert_eq!(
        (
            v["installed"].as_bool(),
            v["devices"][0]["serial"].as_str(),
            v["devices"][0]["revision"].as_str()
        ),
        (Some(true), Some(SERIAL), Some("C3"))
    );
    assert_eq!(
        (
            v["usb"]["link_speed_mbps"].as_f64(),
            v["warning"].is_null(),
            v["gateware_cache_present"].as_bool(),
            v["install"]["glasgow_commit"].as_str()
        ),
        (Some(480.0), true, Some(true), Some("70cb38c9"))
    );
    assert_eq!(v["pinned_glasgow_commit"], json!(PINNED_COMMIT));
    assert_eq!(v["log_problems"].as_array().unwrap().len(), 1);
    r.speed("12\n");
    assert!(r.call("bench_glasgow_list", json!({})).unwrap()["warning"]
        .as_str()
        .unwrap()
        .contains("full speed"));
    // A second holder of the lock keeps the call out.
    let _held = device_lock(&r.ctx.lock_path).unwrap();
    assert!(r
        .call("bench_glasgow_list", json!({}))
        .unwrap_err()
        .contains("One session at a time"));
    assert!(device_lock(&r.ctx.lock_path).is_err());
    drop(_held);
    assert!(device_lock(&r.ctx.lock_path).is_ok());
    // No install: still answers, from sysfs alone, and runs nothing.
    let none = rig("g_list2", r#","venv":"/nonexistent/bench_glasgow""#);
    let v = none.call("bench_glasgow_list", json!({})).unwrap();
    assert_eq!(
        (v["installed"].as_bool(), none.sim.calls().len()),
        (Some(false), 0)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// Tests [@ANCHOR: glasgow_status]
// Tests [@ANCHOR: glasgow_read_ports]
// Tests [@ANCHOR: glasgow_run]
#[test]
fn status_reads_the_ports_and_fails_loudly_when_it_cannot() {
    let r = rig("g_status", "");
    let v = r.call("bench_glasgow_status", json!({})).unwrap();
    assert_eq!(
        (
            v["outputs_enabled"].as_bool(),
            v["ports"][0]["limit_v"].as_f64(),
            v["recorded_by_this_server"].is_null()
        ),
        (Some(false), Some(5.5), true)
    );
    assert!(v["pins"].as_str().unwrap().contains("high-impedance"));
    r.sim.st.lock().unwrap().garbage_voltage = true;
    assert!(r
        .call("bench_glasgow_status", json!({}))
        .unwrap_err()
        .contains("Next:"));
    r.sim.st.lock().unwrap().garbage_voltage = false;
    r.sim.st.lock().unwrap().missing = true;
    assert!(r
        .call("bench_glasgow_status", json!({}))
        .unwrap_err()
        .contains("70-glasgow-bench.rules"));
    r.sim.st.lock().unwrap().missing = false;
    r.sim.st.lock().unwrap().hang_applet = false;
    let r2 = rig("g_status2", "");
    r2.sim.st.lock().unwrap().fail_command = Some("voltage".into());
    assert!(r2
        .call("bench_glasgow_status", json!({}))
        .unwrap_err()
        .contains("failed"));
}

// Tests [@ANCHOR: glasgow_flag]
// Tests [@ANCHOR: glasgow_number]
#[test]
fn flags_and_numbers_are_strict() {
    assert_eq!(flag(&json!({}), "x"), Ok(false));
    assert_eq!(flag(&json!({"x": true}), "x"), Ok(true));
    assert!(flag(&json!({"x": "yes"}), "x").is_err());
    assert_eq!(
        number(&json!({"n": 5}), "n", 1.0, 9.0, "why", None),
        Ok(5.0)
    );
    assert_eq!(number(&json!({}), "n", 1.0, 9.0, "why", Some(2.0)), Ok(2.0));
    assert!(number(&json!({}), "n", 1.0, 9.0, "why", None).is_err());
    let e = number(&json!({"n": 10}), "n", 1.0, 9.0, "the cap", None).unwrap_err();
    assert!(e.contains("1..=9") && e.contains("the cap"));
    assert!(number(&json!({"n": "x"}), "n", 1.0, 9.0, "w", None).is_err());
}

// Tests [@ANCHOR: glasgow_port_set]
// Tests [@ANCHOR: glasgow_revision]
// Tests [@ANCHOR: glasgow_sessions]
#[test]
fn a_port_is_set_with_the_limit_first_and_read_back() {
    let r = rig("g_set", "");
    let d = r
        .call(
            "bench_glasgow_port_set",
            json!({"port": "A", "volts_v": 3.3, "dry_run": true}),
        )
        .unwrap();
    assert_eq!(d["dry_run"], json!(true));
    assert!(r.sim.calls().is_empty(), "a dry run sends nothing");
    let e = r
        .call(
            "bench_glasgow_port_set",
            json!({"port": "A", "volts_v": 3.3}),
        )
        .unwrap_err();
    assert!(e.contains("confirm: true"));
    assert!(r.sim.calls().is_empty());
    let v = r.set_port("A", 3.3);
    assert_eq!(
        (
            v["set_v"].as_f64(),
            v["outputs_enabled"].as_bool(),
            v["limit_v"].as_f64()
        ),
        (Some(3.3), Some(true), Some(3.3))
    );
    let calls = r.sim.calls();
    let pos = |needle: &str| {
        calls
            .iter()
            .position(|c| c.contains(needle))
            .unwrap_or_else(|| panic!("{needle} in {calls:?}"))
    };
    assert!(
        pos("run control-gpio --pins A0:7") < pos("voltage-limit A 3.30"),
        "pins are made inputs before the rail comes up"
    );
    assert!(
        pos("voltage-limit A 3.30") < pos("voltage A 3.30 0.100"),
        "raising: the limit first"
    );
    let s = r.call("bench_glasgow_status", json!({})).unwrap();
    assert_eq!(
        (
            s["outputs_enabled"].as_bool(),
            s["recorded_by_this_server"][0]["set_v"].as_f64()
        ),
        (Some(true), Some(3.3))
    );
    // Lowering sets the supply before the limit.
    r.sim.st.lock().unwrap().calls.clear();
    r.set_port("A", 1.8);
    let calls = r.sim.calls();
    let pos = |needle: &str| calls.iter().position(|c| c.contains(needle)).unwrap();
    assert!(
        pos("voltage A 1.80") < pos("voltage-limit A 1.80"),
        "lowering: the supply first: {calls:?}"
    );
    // Zero switches the port off and forgets it without a confirm.
    r.call("bench_glasgow_port_set", json!({"port": "A", "volts_v": 0}))
        .unwrap();
    assert!(
        r.call("bench_glasgow_status", json!({})).unwrap()["recorded_by_this_server"].is_null()
    );
    // Limits: the config's cap, the revision's range, the trip current, the port name.
    let r2 = rig("g_set2", r#","max_volts":3.6"#);
    for bad in [
        json!({"port": "A", "volts_v": 3.7, "confirm": true}),
        json!({"port": "A", "volts_v": 1.0, "confirm": true}),
        json!({"port": "A", "volts_v": 3.3, "trip_current_a": 0.2, "confirm": true}),
        json!({"port": "C", "volts_v": 3.3, "confirm": true}),
        json!({"port": "A", "volts_v": f64::NAN, "confirm": true}),
        json!({"port": "A", "confirm": true}),
    ] {
        assert!(
            r2.call("bench_glasgow_port_set", bad.clone()).is_err(),
            "{bad}"
        );
    }
    assert!(
        r2.sim.calls().is_empty() || r2.sim.calls().iter().all(|c| c == "list"),
        "{:?}",
        r2.sim.calls()
    );
    let r3 = rig("g_set3", "");
    assert!(r3
        .call(
            "bench_glasgow_port_set",
            json!({"port": "A", "volts_v": 5.2, "confirm": true})
        )
        .unwrap_err()
        .contains("revision C3"));
}

// Tests [@ANCHOR: glasgow_fail_closed]
// Tests [@ANCHOR: glasgow_safe]
#[test]
fn a_readback_that_disagrees_switches_everything_off() {
    let r = rig("g_lie", "");
    r.sim.st.lock().unwrap().lie_supply = Some(4.9);
    let e = r
        .call(
            "bench_glasgow_port_set",
            json!({"port": "A", "volts_v": 3.3, "confirm": true}),
        )
        .unwrap_err();
    assert!(
        e.contains("reads supply 4.900 V") && e.contains("switched off"),
        "{e}"
    );
    assert!(r.sim.st.lock().unwrap().ports["A"].0 == 0.0);
    assert!(
        r.call("bench_glasgow_status", json!({})).unwrap()["recorded_by_this_server"].is_null()
    );
    // And if safe itself fails, the answer says what a person must do.
    let r2 = rig("g_lie2", "");
    r2.sim.st.lock().unwrap().lie_supply = Some(4.9);
    r2.sim.st.lock().unwrap().fail_safe = true;
    let e = r2
        .call(
            "bench_glasgow_port_set",
            json!({"port": "A", "volts_v": 3.3, "confirm": true}),
        )
        .unwrap_err();
    assert!(
        e.contains("SAFE STATE NOT CONFIRMED") && e.contains("unplug"),
        "{e}"
    );
    // A failing command during the set also fails closed.
    let r3 = rig("g_lie3", "");
    r3.sim.st.lock().unwrap().fail_command = Some("voltage".into());
    let e = r3
        .call(
            "bench_glasgow_port_set",
            json!({"port": "B", "volts_v": 2.5, "confirm": true}),
        )
        .unwrap_err();
    assert!(e.contains("failed"), "{e}");
}

// Tests [@ANCHOR: glasgow_safe_state]
#[test]
fn safe_state_proves_zero_volts_or_says_what_to_do() {
    let r = rig("g_safe", "");
    r.set_port("A", 3.3);
    r.set_port("B", 2.5);
    let v = r.call("bench_glasgow_safe_state", json!({})).unwrap();
    assert_eq!(
        (v["outputs_enabled"].as_bool(), v["verified"].as_bool()),
        (Some(false), Some(true))
    );
    assert!(r.sim.st.lock().unwrap().ports.values().all(|p| p.0 == 0.0));
    assert!(
        r.call("bench_glasgow_status", json!({})).unwrap()["recorded_by_this_server"].is_null()
    );
    r.sim.st.lock().unwrap().fail_safe = true;
    let e = r.call("bench_glasgow_safe_state", json!({})).unwrap_err();
    assert!(
        e.contains("SAFE STATE NOT CONFIRMED") && e.contains("Do this now"),
        "{e}"
    );
}

// Tests [@ANCHOR: glasgow_shutdown]
#[test]
fn shutdown_switches_off_what_this_process_powered() {
    let r = rig("g_shutdown", "");
    r.set_port("A", 3.3);
    assert_eq!(r.sim.st.lock().unwrap().ports["A"].0, 3.3);
    shutdown_prefix("g_shutdown");
    assert_eq!(r.sim.st.lock().unwrap().ports["A"].0, 0.0);
    assert!(
        r.call("bench_glasgow_status", json!({})).unwrap()["recorded_by_this_server"].is_null()
    );
}

// Tests [@ANCHOR: glasgow_watchdog]
#[test]
fn the_max_on_s_watchdog_switches_the_ports_off_by_itself() {
    let r = rig("g_dog", r#","max_on_s":1"#);
    r.set_port("A", 3.3);
    assert_eq!(r.sim.st.lock().unwrap().ports["A"].0, 3.3);
    let t = Instant::now();
    while r.sim.st.lock().unwrap().ports["A"].0 != 0.0 && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        r.sim.st.lock().unwrap().ports["A"].0,
        0.0,
        "the watchdog never fired"
    );
    assert!(
        r.call("bench_glasgow_status", json!({})).unwrap()["recorded_by_this_server"].is_null()
    );
}

// Tests [@ANCHOR: glasgow_applets]
#[test]
fn the_applet_list_names_every_tool_and_what_is_not_provided() {
    let r = rig("g_applets", "");
    let v = r.call("bench_glasgow_applets", json!({})).unwrap();
    let text = v.to_string();
    for t in [
        "i2c-controller",
        "spi-controller",
        "uart-analyzer",
        "analyzer",
        "control-gpio",
        "jtag-probe",
        "swd-probe",
    ] {
        assert!(text.contains(t), "{t}");
    }
    assert!(text.contains("flash") && text.contains("free-form"));
    assert!(r.sim.calls().is_empty());
}

// Tests [@ANCHOR: glasgow_pin_arg]
// Tests [@ANCHOR: glasgow_plan]
#[test]
fn applet_arguments_are_checked_before_anything_is_sent() {
    let r = rig("g_args", "");
    r.set_port("A", 3.3);
    r.sim.st.lock().unwrap().calls.clear();
    let bad = [
        ("bench_glasgow_i2c_scan", json!({"scl": "A0", "sda": "A0"})),
        ("bench_glasgow_i2c_scan", json!({"scl": "C0"})),
        ("bench_glasgow_i2c_scan", json!({"scl": "A0:1"})),
        ("bench_glasgow_i2c_scan", json!({"frequency_khz": 5000})),
        (
            "bench_glasgow_i2c_scan",
            json!({"from_address": 100, "to_address": 20}),
        ),
        ("bench_glasgow_i2c_read", json!({})),
        ("bench_glasgow_i2c_read", json!({"address": 3})),
        ("bench_glasgow_i2c_read", json!({"address": 80, "count": 0})),
        (
            "bench_glasgow_i2c_read",
            json!({"address": 80, "register_hex": "zz"}),
        ),
        (
            "bench_glasgow_i2c_write",
            json!({"address": 80, "data_hex": ""}),
        ),
        ("bench_glasgow_spi_jedec_id", json!({"mode": 4})),
        ("bench_glasgow_uart_transact", json!({"baud": 10})),
        ("bench_glasgow_uart_transact", json!({"parity": "mark"})),
        (
            "bench_glasgow_uart_transact",
            json!({"append": "tab", "send_text": "x", "confirm": true}),
        ),
        ("bench_glasgow_uart_transact", json!({"timeout_s": 31})),
        ("bench_glasgow_uart_sniff", json!({"duration_s": 99})),
        ("bench_glasgow_logic_capture", json!({"pins": "A0:9"})),
        (
            "bench_glasgow_logic_capture",
            json!({"pins": "A0:1", "names": "x"}),
        ),
        ("bench_glasgow_logic_capture", json!({"pull": "sideways"})),
        (
            "bench_glasgow_gpio",
            json!({"pins": "A0:1", "actions": ["A5=1"]}),
        ),
        (
            "bench_glasgow_gpio",
            json!({"pins": "A0:1", "actions": ["A0=X"]}),
        ),
        ("bench_glasgow_gpio", json!({"pins": "A0:1", "actions": []})),
        ("bench_glasgow_gpio", json!({"actions": ["A0"]})),
        (
            "bench_glasgow_pattern",
            json!({"pins": "A0:1", "steps": [{"value": 4, "hold_ms": 1}]}),
        ),
        (
            "bench_glasgow_pattern",
            json!({"pins": "A0:1", "steps": [{"value": 1, "hold_ms": 0.1}]}),
        ),
        (
            "bench_glasgow_pattern",
            json!({"pins": "A0:1", "steps": [{"value": 1, "hold_ms": 5000}], "repeat": 100}),
        ),
        (
            "bench_glasgow_pattern",
            json!({"pins": "A0:8", "steps": [{"value": 1, "hold_ms": 1}]}),
        ),
        ("bench_glasgow_jtag_scan", json!({"tdo": "A0"})),
        ("bench_glasgow_swd_idcode", json!({"frequency_khz": 1})),
    ];
    for (tool, a) in bad {
        assert!(r.call(tool, a.clone()).is_err(), "{tool} {a}");
    }
    assert!(r.sim.calls().is_empty(), "{:?}", r.sim.calls());
}

// Tests [@ANCHOR: glasgow_execute]
// Tests [@ANCHOR: glasgow_distinct_pins]
// Tests [@ANCHOR: glasgow_done]
// Tests [@ANCHOR: glasgow_volt_spec]
// Tests [@ANCHOR: glasgow_ops_call]
// Tests [@ANCHOR: glasgow_park]
#[test]
fn an_applet_tool_needs_the_port_set_uses_that_voltage_and_parks_afterwards() {
    let r = rig("g_i2c", "");
    let e = r.call("bench_glasgow_i2c_scan", json!({})).unwrap_err();
    assert!(
        e.contains("no voltage set") && e.contains("bench_glasgow_port_set"),
        "{e}"
    );
    assert!(r.sim.calls().is_empty());
    // A dry run names the command, the voltage it would use, and that the port is not set.
    let d = r
        .call("bench_glasgow_i2c_scan", json!({"dry_run": true}))
        .unwrap();
    assert_eq!(
        (
            d["dry_run"].as_bool(),
            d["ports_set_by_this_server"].as_bool()
        ),
        (Some(true), Some(false))
    );
    assert!(d["would_run"].as_str().unwrap().contains("A=<not set>"));
    r.set_port("A", 3.3);
    r.sim.st.lock().unwrap().calls.clear();
    let v = r.call("bench_glasgow_i2c_scan", json!({})).unwrap();
    assert_eq!(v["addresses"], json!([32, 80]));
    assert_eq!(
        (
            v["pins_state"].as_str().map(|s| s.starts_with("parked")),
            v["outputs_enabled"].as_bool(),
            v["applet"].as_str()
        ),
        (Some(true), Some(true), Some("i2c-controller"))
    );
    let calls = r.sim.calls();
    assert!(
        calls.iter().any(|c| c.starts_with("script ")
            && c.contains("i2c-controller -V A=3.30 --scl A0 --sda A1 -f 100 -- i2c_scan")),
        "{calls:?}"
    );
    assert!(
        calls
            .last()
            .unwrap()
            .contains("run control-gpio --pins A0:7 A0=Z A1=Z"),
        "the pins are parked last: {calls:?}"
    );
    // The voltage is the recorded one: a tool cannot ask for another.
    let d = r
        .call(
            "bench_glasgow_i2c_scan",
            json!({"dry_run": true, "volts_v": 5.0}),
        )
        .unwrap();
    assert!(d["would_run"].as_str().unwrap().contains("-V A=3.30"));
    // The device no longer as set: fail closed, ports off, nothing run.
    r.sim.st.lock().unwrap().lie_supply = Some(5.0);
    r.sim.st.lock().unwrap().calls.clear();
    let e = r.call("bench_glasgow_i2c_scan", json!({})).unwrap_err();
    assert!(
        e.contains("is not as it was set") && e.contains("switched off"),
        "{e}"
    );
    assert!(!r.sim.calls().iter().any(|c| c.starts_with("script")));
    // Port A set, port B not: a pin on B is refused.
    let r2 = rig("g_i2c2", "");
    r2.set_port("A", 3.3);
    assert!(r2
        .call("bench_glasgow_i2c_scan", json!({"sda": "B0"}))
        .unwrap_err()
        .contains("port B has no voltage"));
}

#[test]
fn a_port_past_its_max_on_s_is_switched_off_by_the_next_call() {
    let r = rig("g_expire", "");
    r.set_port("A", 3.3);
    with_session(|s| {
        for p in s.get_mut("g_expire").unwrap().ports.values_mut() {
            p.deadline = Instant::now() - Duration::from_secs(1);
        }
    });
    let e = r.call("bench_glasgow_i2c_scan", json!({})).unwrap_err();
    assert!(
        e.contains("passed its max_on_s") && e.contains("switched off"),
        "{e}"
    );
    assert_eq!(r.sim.st.lock().unwrap().ports["A"].0, 0.0);
}

#[test]
fn bulk_tools_refuse_a_full_speed_link_and_register_tools_do_not() {
    let r = rig("g_slow", "");
    r.set_port("A", 3.3);
    r.speed("12\n");
    r.sim.st.lock().unwrap().calls.clear();
    let e = r.call("bench_glasgow_i2c_scan", json!({})).unwrap_err();
    assert!(
        e.contains("full speed") && e.contains("USB 2.0 cable") && e.contains("Nothing was run"),
        "{e}"
    );
    assert!(!r
        .sim
        .calls()
        .iter()
        .any(|c| c.starts_with("script") || c.starts_with("run i2c")));
    // The GPIO tool uses only register transfers; it ran on a full-speed link on the bench.
    assert!(r
        .call(
            "bench_glasgow_gpio",
            json!({"pins": "A0:3", "actions": ["A2"]})
        )
        .is_ok());
}

#[test]
fn i2c_read_and_write_report_acknowledgement_and_writes_need_confirm() {
    let r = rig("g_i2c_rw", "");
    r.set_port("A", 3.3);
    let v = r
        .call(
            "bench_glasgow_i2c_read",
            json!({"address": 80, "register_hex": "00", "count": 3}),
        )
        .unwrap();
    assert_eq!(
        (v["acknowledged"].as_bool(), v["hex"].as_str()),
        (Some(true), Some("010203"))
    );
    let v = r
        .call("bench_glasgow_i2c_read", json!({"address": 81}))
        .unwrap();
    assert_eq!(v["acknowledged"], json!(false));
    assert!(v["error"].as_str().unwrap().contains("I2CNotAcknowledged"));
    let e = r
        .call(
            "bench_glasgow_i2c_write",
            json!({"address": 80, "data_hex": "00 ff"}),
        )
        .unwrap_err();
    assert!(e.contains("confirm: true"));
    assert!(r
        .call(
            "bench_glasgow_i2c_write",
            json!({"address": 80, "data_hex": "00 ff", "dry_run": true})
        )
        .unwrap()["dry_run"]
        .as_bool()
        .unwrap());
    let v = r
        .call(
            "bench_glasgow_i2c_write",
            json!({"address": 80, "data_hex": "00 ff", "confirm": true}),
        )
        .unwrap();
    assert_eq!(
        (v["acknowledged"].as_bool(), v["written"].as_u64()),
        (Some(true), Some(2))
    );
    let v = r
        .call(
            "bench_glasgow_i2c_write",
            json!({"address": 99, "data_hex": "00", "confirm": true}),
        )
        .unwrap();
    assert_eq!(v["acknowledged"], json!(false));
}

#[test]
fn spi_id_jtag_and_swd_results_are_structured() {
    let r = rig("g_misc", "");
    r.set_port("A", 3.3);
    let v = r.call("bench_glasgow_spi_jedec_id", json!({})).unwrap();
    assert_eq!(
        (
            v["manufacturer_id"].as_str(),
            v["memory_type"].as_str(),
            v["capacity_id"].as_str(),
            v["all_ones"].as_bool()
        ),
        (Some("0xef"), Some("0x40"), Some("0x18"), Some(false))
    );
    r.sim.st.lock().unwrap().spi_reply = "00ffffff".into();
    assert_eq!(
        r.call("bench_glasgow_spi_jedec_id", json!({})).unwrap()["all_ones"],
        json!(true)
    );
    r.sim.st.lock().unwrap().spi_reply = "00ef".into();
    assert!(r
        .call("bench_glasgow_spi_jedec_id", json!({}))
        .unwrap_err()
        .contains("expected 4"));
    let v = r.call("bench_glasgow_jtag_scan", json!({})).unwrap();
    assert_eq!(
        (v["tap_count"].as_u64(), v["taps"][0]["idcode"].as_str()),
        (Some(1), Some("0x4ba00477"))
    );
    let v = r.call("bench_glasgow_swd_idcode", json!({})).unwrap();
    assert_eq!(
        (v["dpidr"].as_str(), v["partno"].as_u64()),
        (Some("0x2ba01477"), Some(0xba))
    );
}

#[test]
fn uart_transact_sends_only_with_confirm_and_collects_the_reply() {
    let r = rig("g_uart", "");
    r.set_port("A", 3.3);
    r.sim.st.lock().unwrap().uart_reply = "4f4b0d0a".into();
    let e = r
        .call(
            "bench_glasgow_uart_transact",
            json!({"send_text": "AT", "append": "cr"}),
        )
        .unwrap_err();
    assert!(e.contains("confirm: true"));
    let v = r.call("bench_glasgow_uart_transact", json!({"send_text": "AT", "append": "cr", "confirm": true, "idle_ms": 100, "timeout_s": 2})).unwrap();
    assert_eq!(
        (
            v["hex"].as_str(),
            v["received"].as_u64(),
            v["sent"].as_u64(),
            v["ended_by"].as_str()
        ),
        (Some("4f4b0d0a"), Some(4), Some(3), Some("idle"))
    );
    let v = r
        .call(
            "bench_glasgow_uart_transact",
            json!({"until_hex": "0d0a", "timeout_s": 2}),
        )
        .unwrap();
    assert_eq!(
        (v["ended_by"].as_str(), v["sent"].as_u64()),
        (Some("until"), Some(0))
    );
    // A listen-only call needs no confirm, and a silent line ends by timeout, not by invention.
    r.sim.st.lock().unwrap().uart_reply = String::new();
    let v = r
        .call("bench_glasgow_uart_transact", json!({"timeout_s": 0.3}))
        .unwrap();
    assert_eq!(
        (v["received"].as_u64(), v["ended_by"].as_str()),
        (Some(0), Some("timeout"))
    );
}

#[test]
fn captures_write_a_file_and_return_a_summary() {
    let r = rig("g_cap", "");
    r.set_port("A", 3.3);
    let v = r
        .call(
            "bench_glasgow_logic_capture",
            json!({"pins": "A0:1", "duration_s": 1, "pull": "up", "names": "clk,burst"}),
        )
        .unwrap();
    let file = v["file"].as_str().unwrap();
    assert!(Path::new(file).exists() && file.ends_with(".vcd"));
    assert_eq!(
        (
            v["summary"]["pins"]["clk"]["transitions"].as_u64(),
            v["overrun"].as_bool(),
            v["stopped_by"].as_str()
        ),
        (Some(10), Some(false), Some("duration or size limit"))
    );
    let c = r
        .sim
        .calls()
        .into_iter()
        .find(|c| c.starts_with("run analyzer"))
        .unwrap();
    assert!(
        c.contains("-V A=3.30 --i A0:1 --pin-names clk,burst --pull-ups"),
        "{c}"
    );
    let v = r
        .call(
            "bench_glasgow_uart_sniff",
            json!({"duration_s": 1, "baud": 9600}),
        )
        .unwrap();
    assert_eq!(v["channels"]["rx"]["bytes"], json!(9));
    assert!(v["file"].as_str().unwrap().ends_with(".csv"));
    // A capture file that was never written is an error, not an empty success.
    r.sim.st.lock().unwrap().vcd = "$enddefinitions $end\n".into();
    assert!(r
        .call("bench_glasgow_logic_capture", json!({"duration_s": 1}))
        .unwrap_err()
        .contains("no timestamps"));
}

#[test]
fn gpio_and_pattern_release_every_pin_even_when_the_run_fails() {
    let r = rig("g_gpio", "");
    r.set_port("A", 3.3);
    assert!(r
        .call(
            "bench_glasgow_gpio",
            json!({"pins": "A0:3", "actions": ["A0=1"]})
        )
        .unwrap_err()
        .contains("confirm: true"));
    let v = r.call("bench_glasgow_gpio", json!({"pins": "A0:3", "actions": ["A0=1", "A1=L", "A2"], "confirm": true, "hold_s": 0.2})).unwrap();
    assert_eq!(
        (
            v["released"].as_bool(),
            v["reads"]["A2"].as_bool(),
            v["held_s"].as_f64()
        ),
        (Some(true), Some(true), Some(0.2))
    );
    assert!(!r.sim.st.lock().unwrap().driven, "the park ended the drive");
    // Reads and pulls need no confirm.
    assert!(r
        .call(
            "bench_glasgow_gpio",
            json!({"pins": "A0:3", "actions": ["A1=H", "A2"]})
        )
        .is_ok());
    let steps =
        json!([{"value": 1, "hold_ms": 5}, {"value": 2, "hold_ms": 5}, {"value": 0, "hold_ms": 5}]);
    assert!(r
        .call(
            "bench_glasgow_pattern",
            json!({"pins": "A0:3", "steps": steps.clone()})
        )
        .unwrap_err()
        .contains("confirm: true"));
    let v = r
        .call(
            "bench_glasgow_pattern",
            json!({"pins": "A0:3", "steps": steps.clone(), "repeat": 3, "confirm": true}),
        )
        .unwrap();
    assert_eq!(
        (v["steps_run"].as_u64(), v["released"].as_bool()),
        (Some(9), Some(true))
    );
    let released = std::fs::read_to_string(r.dir.join("released.txt")).unwrap();
    assert_eq!(
        released.lines().count(),
        4,
        "every one of the 4 pins was set back to an input: {released:?}"
    );
    // A failure in the middle still releases them, and parks.
    std::fs::remove_file(r.dir.join("released.txt")).unwrap();
    r.sim.st.lock().unwrap().fail_pattern = true;
    let e = r
        .call(
            "bench_glasgow_pattern",
            json!({"pins": "A0:3", "steps": steps, "confirm": true}),
        )
        .unwrap_err();
    assert!(e.contains("exited") && e.contains("parked"), "{e}");
    assert_eq!(
        std::fs::read_to_string(r.dir.join("released.txt"))
            .unwrap()
            .lines()
            .count(),
        4
    );
    assert!(r.sim.calls().last().unwrap().contains("A0=Z"));
}

#[test]
fn a_hung_or_failed_applet_ends_with_the_pins_parked() {
    let r = rig("g_hang", "");
    r.set_port("A", 3.3);
    r.sim.st.lock().unwrap().hang_applet = true;
    let e = r.call("bench_glasgow_i2c_scan", json!({})).unwrap_err();
    assert!(e.contains("did not finish") && e.contains("parked"), "{e}");
    assert!(
        r.sim.calls().last().unwrap().contains("A0=Z"),
        "{:?}",
        r.sim.calls()
    );
    r.sim.st.lock().unwrap().hang_applet = false;
    r.sim.st.lock().unwrap().applet_exit = 1;
    r.sim.st.lock().unwrap().applet_stderr = "E: g.a.i.i2c_controller: bus stuck\n".into();
    let e = r.call("bench_glasgow_i2c_scan", json!({})).unwrap_err();
    assert!(e.contains("bus stuck") && e.contains("parked"), "{e}");
    // The park itself failing is a fail-closed event.
    r.sim.st.lock().unwrap().applet_exit = 0;
    r.sim.st.lock().unwrap().fail_command = Some("run".into());
    let e = r.call("bench_glasgow_spi_jedec_id", json!({})).unwrap_err();
    assert!(
        e.contains("could not be parked") || e.contains("exited"),
        "{e}"
    );
}

// Tests [@ANCHOR: glasgow_ops_main]
// Tests [@ANCHOR: glasgow_ops_i2c_scan]
// Tests [@ANCHOR: glasgow_ops_i2c_read]
// Tests [@ANCHOR: glasgow_ops_i2c_write]
// Tests [@ANCHOR: glasgow_ops_uart_transact]
// Tests [@ANCHOR: glasgow_ops_gpio_pattern]
// Tests [@ANCHOR: glasgow_ops_swd_idcode]
#[test]
fn the_helper_script_runs_through_its_entry_in_every_applet_context() {
    let r = rig("g_helper", "");
    r.set_port("A", 3.3);
    r.sim.st.lock().unwrap().uart_reply = "6869".into();
    assert_eq!(
        r.call(
            "bench_glasgow_i2c_scan",
            json!({"from_address": 0x20, "to_address": 0x30})
        )
        .unwrap()["addresses"],
        json!([32])
    );
    assert_eq!(
        r.call("bench_glasgow_i2c_read", json!({"address": 80, "count": 2}))
            .unwrap()["hex"],
        json!("0102")
    );
    assert_eq!(
        r.call(
            "bench_glasgow_i2c_write",
            json!({"address": 80, "data_hex": "01", "confirm": true})
        )
        .unwrap()["written"],
        json!(1)
    );
    assert_eq!(
        r.call(
            "bench_glasgow_uart_transact",
            json!({"timeout_s": 1, "idle_ms": 50})
        )
        .unwrap()["hex"],
        json!("6869")
    );
    assert_eq!(
        r.call(
            "bench_glasgow_pattern",
            json!({"pins": "A0", "steps": [{"value": 1, "hold_ms": 1}], "confirm": true})
        )
        .unwrap()["steps_run"],
        json!(1)
    );
    assert!(r.call("bench_glasgow_swd_idcode", json!({})).unwrap()["dpidr"].is_string());
    let entry = std::fs::read_to_string(r.dir.join("out/glasgow_entry.py")).unwrap();
    assert!(entry.contains("await _m.main(globals())") && entry.contains("bench_glasgow_ops.py"));
    assert!(r.dir.join("out/bench_glasgow_ops.py").exists());
}

#[test]
fn the_installer_and_pin_tools_pass_their_own_tests() {
    let out = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/glasgow/test_glasgow_tools.py"
        ))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// Tests [@ANCHOR: glasgow_prepare]
#[test]
fn prepare_builds_the_gateware_without_the_device_or_a_voltage() {
    let r = rig("g_prep", "");
    let v = r
        .call(
            "bench_glasgow_prepare",
            json!({"tool": "uart", "rx": "A2", "tx": "A3", "parity": "even"}),
        )
        .unwrap();
    assert_eq!(
        (
            v["built"].as_bool(),
            v["applet"].as_str(),
            v["revision"].as_str()
        ),
        (Some(true), Some("uart"), Some("C3"))
    );
    let c = r
        .sim
        .calls()
        .into_iter()
        .find(|c| c.starts_with("build"))
        .unwrap();
    assert!(
        c.contains("--rev C3 -t bitstream -f /dev/shm/bench_glasgow_build_")
            && c.ends_with("uart --rx A2 --tx A3 --parity even --stop-bits 1"),
        "{c}"
    );
    assert!(
        !c.contains(" -b ") && !c.contains("-V"),
        "runtime settings and voltages do not belong in a build: {c}"
    );
    assert!(
        !r.sim.calls().iter().any(|c| c.contains("voltage")),
        "a build never touches the device"
    );
    let v = r
        .call(
            "bench_glasgow_prepare",
            json!({"tool": "logic", "pins": "A0:7", "revision": "D0"}),
        )
        .unwrap();
    assert_eq!(v["revision"], json!("D0"));
    assert!(r
        .sim
        .calls()
        .into_iter()
        .any(|c| c.ends_with("analyzer --i A0:7")));
    assert!(r
        .call("bench_glasgow_prepare", json!({"tool": "flash"}))
        .is_err());
    assert!(r
        .call(
            "bench_glasgow_prepare",
            json!({"tool": "uart", "revision": "Z9"})
        )
        .is_err());
    assert!(r.call("bench_glasgow_prepare", json!({})).is_err());
    r.sim.st.lock().unwrap().fail_command = Some("build".into());
    assert!(r
        .call("bench_glasgow_prepare", json!({"tool": "i2c"}))
        .unwrap_err()
        .contains("the build exited"));
}

#[test]
fn the_requirements_are_pinned_and_hashed() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/glasgow/requirements.txt"
    ))
    .unwrap();
    let body: Vec<&str> = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .collect();
    let mut pinned = 0;
    for l in &body {
        if !l.starts_with(' ') {
            assert!(
                l.contains("==") && l.ends_with('\\'),
                "not an exact pin: {l}"
            );
            pinned += 1;
        } else {
            assert!(
                l.trim_start().starts_with("--hash=sha256:")
                    && l.trim_start().trim_end_matches(" \\").len() == 14 + 64,
                "not a sha256: {l}"
            );
        }
    }
    assert!(pinned >= 20);
    assert!(
        !text.contains("git+") && !text.contains("http"),
        "no unpinned sources"
    );
    let installer = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/glasgow/install_glasgow.py"
    ))
    .unwrap();
    assert!(
        installer.contains("--require-hashes")
            && installer.contains("ARCHIVE_SHA256")
            && installer.contains(PINNED_COMMIT)
    );
}

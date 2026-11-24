// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests against a fake cable: a `Runner` that answers like openFPGALoader and records every command line.
use super::*;
use std::sync::Mutex;

const CABLE: &str = include_str!("../../tests/usb_blaster_fixtures/scan_cable.txt");
const CHAIN: &str = include_str!("../../tests/usb_blaster_fixtures/detect_chain.txt");
const NO_TDO: &str = include_str!("../../tests/usb_blaster_fixtures/detect_error.txt");

/// The fake cable: `cables` answers `--scan-usb`, `chain` answers `--detect`, anything else (a load or a write) succeeds with
/// `done` and exit code `load_code`. Every argument list is recorded.
struct Fake {
    cables: &'static str,
    chain: &'static str,
    chain_code: i32,
    load_code: i32,
    seen: Mutex<Vec<Vec<String>>>,
}

fn fake(chain: &'static str) -> Fake {
    Fake {
        cables: CABLE,
        chain,
        chain_code: 0,
        load_code: 0,
        seen: Mutex::new(vec![]),
    }
}

impl Runner for Fake {
    fn run(&self, args: &[String], _t: Duration) -> Result<Out, String> {
        self.seen.lock().unwrap().push(args.to_vec());
        let text = |t: &str, code| {
            Ok(Out {
                code: Some(code),
                text: t.into(),
                timed_out: false,
            })
        };
        if args.iter().any(|a| a == "--scan-usb") {
            text(self.cables, 0)
        } else if args.iter().any(|a| a == "--detect") {
            text(self.chain, self.chain_code)
        } else {
            text("Load SRAM: Done\n", self.load_code)
        }
    }
}

fn bench(extra: &str) -> Vec<Instrument> {
    crate::config::parse(&format!(
        r#"{{"instruments":{{"j":{{"driver":"usb_blaster","tier":"full"{extra}}}}}}}"#
    ))
    .unwrap()
}

fn file(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("bench_usb_blaster_test_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join(name);
    std::fs::write(&p, b"bits").unwrap();
    p.to_string_lossy().into_owned()
}

fn loads(f: &Fake) -> usize {
    f.seen
        .lock()
        .unwrap()
        .iter()
        .filter(|a| a.iter().any(|x| x == "-m" || x == "-f"))
        .count()
}

// Tests [@ANCHOR: usb_blaster_config]
#[test]
fn config_has_safe_defaults_and_refuses_bad_values() {
    let v = bench("");
    assert_eq!(v[0].usb_blaster, Config::default());
    assert!(!v[0].usb_blaster.allow_flash);
    let v = bench(r#","cable":"usb-blasterII","allow_flash":true,"program":"/opt/ofl""#);
    assert!(v[0].usb_blaster.allow_flash && v[0].usb_blaster.cable == "usb-blasterII");
    for bad in [
        r#","cable":"ftdi""#,
        r#","allow_flash":"yes""#,
        r#","program":"""#,
    ] {
        let t = format!(r#"{{"instruments":{{"j":{{"driver":"usb_blaster"{bad}}}}}}}"#);
        assert!(crate::config::parse(&t).is_err(), "{bad}");
    }
}

// Tests [@ANCHOR: usb_blaster_parse_chain]
// Tests [@ANCHOR: usb_blaster_parse_cables]
#[test]
fn listings_are_parsed_into_devices_and_cables() {
    let d = parse_chain(CHAIN);
    assert_eq!(d.len(), 2);
    assert_eq!(
        (d[0].idcode.as_str(), d[0].model.as_str(), d[0].irlength),
        ("0x20f10dd", "10CL025", Some(10))
    );
    assert_eq!(d[1].family, "max 10");
    assert!(parse_chain(NO_TDO).is_empty());
    let c = parse_cables(CABLE);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["vid_pid"], "0x09fb:0x6001");
}

// Tests [@ANCHOR: bench_mcp_usb_blaster_scan]
// Tests [@ANCHOR: usb_blaster_chain]
// Tests [@ANCHOR: bench_mcp_usb_blaster_call_with]
// Tests [@ANCHOR: bench_mcp_usb_blaster_instrument]
#[test]
fn scan_returns_cables_and_devices_and_reports_a_dead_chain_with_the_next_step() {
    let f = fake(CHAIN);
    let v = call_with(&f, &bench(""), "bench_jtag_scan", &json!({})).unwrap();
    assert_eq!(v["devices"][1]["model"], "10M08");
    assert_eq!(v["cables"][0]["probe"], "usb-blaster");
    assert_eq!(loads(&f), 0, "a scan never loads");
    let mut f = fake(NO_TDO);
    f.chain_code = 1;
    let e = call_with(&f, &bench(""), "bench_jtag_scan", &json!({})).unwrap_err();
    assert!(
        e.contains("TDO is stuck at 0") && e.contains("Next:"),
        "{e}"
    );
    let mut f = fake(CHAIN);
    f.cables = "Bus device vid:pid probe type manufacturer serial product\n";
    let e = call_with(&f, &bench(""), "bench_jtag_scan", &json!({})).unwrap_err();
    assert!(e.contains("no USB-Blaster cable"), "{e}");
    let e = call_with(
        &f,
        &bench(""),
        "bench_jtag_scan",
        &json!({"instrument": "x"}),
    )
    .unwrap_err();
    assert!(e.contains("no usb_blaster instrument named"), "{e}");
}

// Tests [@ANCHOR: bench_mcp_usb_blaster_sram_load]
// Tests [@ANCHOR: usb_blaster_file]
// Tests [@ANCHOR: bench_mcp_usb_blaster_flag]
#[test]
fn sram_load_is_gated_and_only_ever_uses_the_sram_option() {
    let sof = file("a.sof");
    let f = fake(CHAIN);
    let e = call_with(
        &f,
        &bench(""),
        "bench_jtag_sram_load",
        &json!({"file": sof}),
    )
    .unwrap_err();
    assert!(e.contains("confirm_sram_volatile"), "{e}");
    let v = call_with(
        &f,
        &bench(""),
        "bench_jtag_sram_load",
        &json!({"file": sof, "dry_run": true}),
    )
    .unwrap();
    assert_eq!(v["dry_run"], true);
    assert_eq!(
        f.seen.lock().unwrap().len(),
        0,
        "dry_run and a missing confirm send nothing"
    );
    let v = call_with(
        &f,
        &bench(""),
        "bench_jtag_sram_load",
        &json!({"file": sof, "confirm_sram_volatile": true}),
    )
    .unwrap();
    assert_eq!(v["loaded"], "sram");
    let seen = f.seen.lock().unwrap().clone();
    let load = seen.last().unwrap();
    assert!(
        load.contains(&"-m".to_string()) && !load.contains(&"-f".to_string()),
        "{load:?}"
    );
    for bad in [
        json!({"file": "rel.sof"}),
        json!({"file": file("a.jic")}),
        json!({"file": "/nonexistent/x.sof"}),
        json!({"file": sof, "confirm_sram_volatile": "yes"}),
    ] {
        let mut a = bad.clone();
        a["confirm_sram_volatile"] = a
            .get("confirm_sram_volatile")
            .cloned()
            .unwrap_or(json!(true));
        assert!(
            call_with(&fake(CHAIN), &bench(""), "bench_jtag_sram_load", &a).is_err(),
            "{bad}"
        );
    }
    let dead = fake(NO_TDO);
    let mut dead = Fake {
        chain_code: 1,
        ..dead
    };
    dead.load_code = 0;
    let e = call_with(
        &dead,
        &bench(""),
        "bench_jtag_sram_load",
        &json!({"file": sof, "confirm_sram_volatile": true}),
    )
    .unwrap_err();
    assert!(e.contains("no device answered") && loads(&dead) == 0, "{e}");
    let mut failing = fake(CHAIN);
    failing.load_code = 1;
    let e = call_with(
        &failing,
        &bench(""),
        "bench_jtag_sram_load",
        &json!({"file": sof, "confirm_sram_volatile": true}),
    )
    .unwrap_err();
    assert!(e.contains("failed"), "{e}");
}

// Tests [@ANCHOR: bench_mcp_usb_blaster_flash_write]
// Tests [@ANCHOR: bench_mcp_usb_blaster_tool_defs]
#[test]
fn flash_write_is_off_by_default_and_needs_the_owner_for_every_call() {
    let jic = file("a.jic");
    let names = |i: &[Instrument]| {
        tool_defs(i)
            .iter()
            .map(|t| t.name.to_string())
            .collect::<Vec<_>>()
    };
    assert!(!names(&bench("")).contains(&"bench_jtag_flash_write".to_string()));
    assert!(names(&bench(r#","allow_flash":true"#)).contains(&"bench_jtag_flash_write".to_string()));
    let ro = crate::config::parse(
        r#"{"instruments":{"j":{"driver":"usb_blaster","tier":"read_config","allow_flash":true}}}"#,
    )
    .unwrap();
    assert_eq!(
        names(&ro),
        ["bench_jtag_scan"],
        "a tool above the tier is not listed"
    );
    let all = json!({"file": jic, "owner_approval": "yes, write it", "confirm_flash": "WRITE FLASH a.jic"});
    let f = fake(CHAIN);
    // off in the config: refused, even with every argument right
    let e = call_with(&f, &bench(""), "bench_jtag_flash_write", &all).unwrap_err();
    assert!(e.contains("allow_flash"), "{e}");
    let on = bench(r#","allow_flash":true"#);
    for missing in ["owner_approval", "confirm_flash"] {
        let mut a = all.clone();
        a.as_object_mut().unwrap().remove(missing);
        assert!(
            call_with(&f, &on, "bench_jtag_flash_write", &a).is_err(),
            "{missing}"
        );
    }
    let mut wrong = all.clone();
    wrong["confirm_flash"] = json!("WRITE FLASH other.jic");
    assert!(call_with(&f, &on, "bench_jtag_flash_write", &wrong).is_err());
    assert_eq!(f.seen.lock().unwrap().len(), 0, "nothing reached the cable");
    let v = call_with(
        &f,
        &on,
        "bench_jtag_flash_write",
        &json!({"file": jic, "dry_run": true}),
    )
    .unwrap();
    assert_eq!(v["dry_run"], true);
    assert_eq!(f.seen.lock().unwrap().len(), 0);
    let v = call_with(&f, &on, "bench_jtag_flash_write", &all).unwrap();
    assert_eq!(v["written"], "flash");
    assert_eq!(loads(&f), 1);
    let mut failing = fake(CHAIN);
    failing.load_code = 1;
    let e = call_with(&failing, &on, "bench_jtag_flash_write", &all).unwrap_err();
    assert!(e.contains("FAILED") && e.contains("half written"), "{e}");
}

// Tests [@ANCHOR: usb_blaster_real_runner]
#[test]
fn the_real_runner_reports_exit_codes_output_and_a_missing_program() {
    let r = RealRunner {
        program: "sh".into(),
    };
    let ok = r
        .run(&s(&["-c", "echo hi; exit 3"]), Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        (ok.code, ok.text.trim(), ok.timed_out),
        (Some(3), "hi", false)
    );
    let slow = r
        .run(&s(&["-c", "sleep 5"]), Duration::from_millis(200))
        .unwrap();
    assert!(slow.timed_out);
    let e = RealRunner {
        program: "/nonexistent/openFPGALoader".into(),
    }
    .run(&[], Duration::from_secs(1))
    .unwrap_err();
    assert!(e.contains("Next:"), "{e}");
}

// Tests [@ANCHOR: bench_mcp_usb_blaster_call]
#[test]
fn the_production_entry_starts_the_configured_program_and_says_what_is_missing() {
    let none: Vec<Instrument> = vec![];
    assert!(call(&none, "bench_jtag_scan", &json!({}))
        .unwrap_err()
        .contains("no usb_blaster instrument"));
    let b = bench(r#","program":"/nonexistent/openFPGALoader""#);
    let e = call(&b, "bench_jtag_scan", &json!({})).unwrap_err();
    assert!(e.contains("cannot start"), "{e}");
}

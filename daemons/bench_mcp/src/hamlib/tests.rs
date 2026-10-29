// SPDX-License-Identifier: AGPL-3.0-or-later
//! The module's tests. Happy paths run against the real Hamlib dummy rig through a managed rigctld on a free localhost port (the
//! production path); faults run against the fake rigctld of `testkit`.

use super::proto::{Client, RigError};
use super::testkit::*;
use super::*;

/// A fresh directory for a test's state files and logs; removed by `Dir::drop`.
pub struct Dir(pub std::path::PathBuf);

impl Dir {
    pub fn new(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!(
            "bench_mcp-hamlib-test-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        // Stop any daemon a failed test left behind (the state files name them).
        if let Ok(rd) = std::fs::read_dir(&self.0) {
            for e in rd.flatten() {
                if let Some(name) = e
                    .file_name()
                    .to_string_lossy()
                    .strip_suffix(".rigctld.json")
                {
                    rigd::kill_slot(name);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A bench with one rig named `name`: the dummy rig through a managed rigctld, state under `dir`.
pub fn dummy_bench(name: &str, dir: &Dir, tier: &str, extra: &str) -> Vec<Instrument> {
    crate::config::parse(&format!(
        r#"{{"instruments":{{"{name}":{{"driver":"hamlib","tier":"{tier}","model":1,"managed_dir":"{}"{extra}}}}}}}"#,
        dir.0.display()
    ))
    .unwrap()
}

/// A bench whose rig is an already running rigctld (the fake) on `port`.
pub fn fake_bench(name: &str, port: u16, tier: &str, extra: &str) -> Vec<Instrument> {
    crate::config::parse(&format!(r#"{{"instruments":{{"{name}":{{"driver":"hamlib","tier":"{tier}","rigctld_port":{port}{extra}}}}}}}"#)).unwrap()
}

fn parse(extra: &str) -> Result<Config, String> {
    parse_config(&serde_json::from_str::<Value>(&format!("{{{extra}}}")).unwrap())
}

// Tests [@ANCHOR: hamlib_config]
#[test]
fn config_takes_a_managed_or_an_existing_rigctld_and_refuses_the_rest() {
    let c = parse(r#""model":1"#).unwrap();
    assert_eq!(
        c.source,
        Source::Managed(Managed {
            model: 1,
            device: None,
            speed: None,
            conf: vec![],
            port: None
        })
    );
    assert_eq!(c.reply_timeout_s, 3);
    let c = parse(r#""model":1021,"device":"/dev/serial/by-id/usb-x","speed":19200,"set_conf":{"serial_handshake":"None","timeout":500},"reply_timeout_s":5,"rx_ranges_hz":[[1e5,3e7]]"#).unwrap();
    let Source::Managed(m) = &c.source else {
        panic!()
    };
    assert_eq!(
        (m.speed, m.conf.len(), c.rx_ranges_hz.len()),
        (Some(19200), 2, 1)
    );
    assert_eq!(
        parse(r#""rigctld_port":4532"#).unwrap().source,
        Source::Existing {
            host: "127.0.0.1".into(),
            port: 4532
        }
    );
    for bad in [
        r#""#,
        r#""model":1,"rigctld_port":4532"#,
        r#""model":0"#,
        r#""model":1,"device":"/etc/passwd""#,
        r#""model":1,"device":"/dev/../etc/x""#,
        r#""model":2,"device":"192.0.2.9:4532""#,
        r#""model":1,"speed":12345"#,
        r#""model":1,"set_conf":{"a":"b c"}"#,
        r#""model":1,"set_conf":{"a,b":"c"}"#,
        r#""model":1,"listen_port":80"#,
        r#""rigctld_port":4532,"rigctld_host":"192.0.2.9""#,
        r#""model":1,"reply_timeout_s":0"#,
        r#""model":1,"rx_ranges_hz":[[10,5]]"#,
        r#""model":1,"managed_dir":"relative""#,
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
    // A remote daemon needs the owner's explicit word, and is then allowed.
    assert!(
        parse(r#""rigctld_port":4532,"rigctld_host":"192.0.2.9","allow_remote_rigctld":true"#)
            .is_ok()
    );
    assert!(parse(r#""model":2,"device":"127.0.0.1:4533""#).is_ok());
}

// Tests [@ANCHOR: hamlib_command]
#[test]
fn a_command_gets_its_reply_and_a_rig_error_is_an_error_with_a_next_step() {
    let fake = FakeRigctld::serve(|c| match c {
        "get_freq" => ok(&["Frequency: 7100000"]),
        "get_mode" => err(-5),
        _ => err(-11),
    });
    let mut c = Client::connect("127.0.0.1", fake.port, Duration::from_secs(2)).unwrap();
    assert_eq!(
        c.command("get_freq")
            .unwrap()
            .number(Some("Frequency"))
            .unwrap(),
        7.1e6
    );
    let e = c.command("get_mode").unwrap_err();
    assert_eq!(
        e,
        RigError::Rig {
            code: -5,
            cmd: "get_mode".into()
        }
    );
    assert!(e.to_string().contains("Next:"), "{e}");
    assert!(c.command("get_split_vfo").unwrap_err().unsupported());
    assert!(
        c.command("get_freq\nT 1").is_err(),
        "a newline must never become a second command"
    );
    assert_eq!(fake.received(), ["get_freq", "get_mode", "get_split_vfo"]);
}

// Tests [@ANCHOR: hamlib_read_retry]
#[test]
fn a_read_is_tried_once_more_over_a_new_connection_but_a_silent_rig_is_an_error_not_a_hang() {
    let mut first = true;
    let flaky = FakeRigctld::serve(move |c| {
        if c == "get_freq" && std::mem::take(&mut first) {
            return Reply::Hangup;
        }
        ok(&["Frequency: 14074000"])
    });
    let mut c = Client::connect("127.0.0.1", flaky.port, Duration::from_secs(2)).unwrap();
    assert_eq!(
        c.read_number("get_freq", Some("Frequency")).unwrap(),
        14.074e6
    );
    assert_eq!(flaky.received(), ["get_freq", "get_freq"]);
    let silent = FakeRigctld::serve(|_| Reply::Silent);
    let mut c = Client::connect("127.0.0.1", silent.port, Duration::from_secs(1)).unwrap();
    let t = std::time::Instant::now();
    let e = c.read("get_freq").unwrap_err();
    assert!(
        matches!(e, RigError::Link(_)) && e.to_string().contains("Next:"),
        "{e}"
    );
    assert!(
        t.elapsed() < Duration::from_secs(6),
        "two tries of one second, not a hang"
    );
    // A write is not repeated.
    let writes = FakeRigctld::serve(|_| Reply::Silent);
    let mut c = Client::connect("127.0.0.1", writes.port, Duration::from_secs(1)).unwrap();
    assert!(c.command("set_freq 7100000").is_err());
    assert_eq!(writes.received(), ["set_freq 7100000"]);
}

// Tests [@ANCHOR: hamlib_rigctld_args]
#[test]
fn the_daemon_command_line_binds_localhost_and_carries_only_the_owners_settings() {
    let m = Managed {
        model: 1021,
        device: Some("/dev/ttyUSB0".into()),
        speed: Some(9600),
        conf: vec![("a".into(), "b".into()), ("c".into(), "1".into())],
        port: None,
    };
    assert_eq!(
        rigd::args(&m, 5001),
        [
            "-m",
            "1021",
            "-r",
            "/dev/ttyUSB0",
            "-s",
            "9600",
            "-t",
            "5001",
            "-T",
            "127.0.0.1",
            "--set-conf=a=b,c=1"
        ]
    );
    let dummy = Managed {
        model: 1,
        device: None,
        speed: None,
        conf: vec![],
        port: None,
    };
    let a = rigd::args(&dummy, 5002);
    assert_eq!(a, ["-m", "1", "-t", "5002", "-T", "127.0.0.1"]);
}

// Tests [@ANCHOR: hamlib_open]
// Tests [@ANCHOR: hamlib_rigctld_start]
// Tests [@ANCHOR: hamlib_rigctld_ensure]
// Tests [@ANCHOR: hamlib_rigctld_tool]
#[test]
fn the_managed_daemon_starts_on_demand_stops_on_request_and_starts_again() {
    let dir = Dir::new("daemon");
    let b = dummy_bench("t_daemon", &dir, "read_config", "");
    let st = call(&b, "bench_hamlib_rigctld", &json!({"action": "status"})).unwrap();
    assert_eq!(st["running"], false);
    let st = call(&b, "bench_hamlib_rigctld", &json!({"action": "start"})).unwrap();
    assert_eq!(st["running"], true);
    assert_eq!(st["listens_on"], "127.0.0.1");
    let port = st["port"].as_u64().unwrap() as u16;
    let mut c = open(&b[0]).unwrap();
    assert_eq!(c.read_number("get_freq", Some("Frequency")).unwrap(), 145e6);
    // Asking again does not start a second daemon.
    assert_eq!(
        call(&b, "bench_hamlib_rigctld", &json!({"action": "start"})).unwrap()["port"],
        port
    );
    let st = call(&b, "bench_hamlib_rigctld", &json!({"action": "stop"})).unwrap();
    assert_eq!(st["was_running"], true);
    assert!(Client::connect("127.0.0.1", port, Duration::from_secs(1))
        .and_then(|mut c| c.command("get_freq"))
        .is_err());
    let st = call(&b, "bench_hamlib_rigctld", &json!({"action": "restart"})).unwrap();
    assert_eq!(st["running"], true);
    // Other calls start it by themselves.
    call(&b, "bench_hamlib_rigctld", &json!({"action": "stop"})).unwrap();
    assert!(call(&b, "bench_hamlib_get", &json!({"item": "frequency"})).is_ok());
    call(&b, "bench_hamlib_rigctld", &json!({"action": "stop"})).unwrap();
    assert!(call(&b, "bench_hamlib_rigctld", &json!({"action": "nonsense"})).is_err());
}

// Tests [@ANCHOR: hamlib_device_check]
#[test]
fn a_missing_or_unusable_serial_device_is_reported_before_a_daemon_is_started() {
    let dir = Dir::new("badrig");
    // Model 1021 needs a serial device; one that does not exist is reported at once, with the next step.
    let b = crate::config::parse(&format!(
        r#"{{"instruments":{{"t_bad":{{"driver":"hamlib","tier":"read_only","model":1021,"device":"/dev/bench_mcp_no_such_port","managed_dir":"{}"}}}}}}"#,
        dir.0.display()
    ))
    .unwrap();
    let e = call(&b, "bench_hamlib_status", &json!({})).unwrap_err();
    assert!(e.contains("cannot be used") && e.contains("Next:"), "{e}");
    assert!(rigd::check_device("/dev/null").is_ok());
    assert!(rigd::check_device("/dev/bench_mcp_no_such_port").is_err());
    let e = rigd::check_device("/dev/mem").unwrap_err();
    assert!(
        e.contains("may not read and write") || e.contains("cannot be used"),
        "{e}"
    );
}

// Tests [@ANCHOR: hamlib_reap_stale]
#[test]
fn a_daemon_left_by_a_dead_server_is_stopped_but_a_live_owners_is_never_touched() {
    let dir = Dir::new("reap");
    let leftover = |argv0: &str| {
        std::process::Command::new("bash")
            .args(["-c", &format!("exec -a {argv0} sleep 30")])
            .spawn()
            .unwrap()
    };
    let write = |pid: u32, owner: u32| {
        std::fs::write(
            dir.0.join("t_reap.rigctld.json"),
            json!({"owner_pid": owner, "rigctld_pid": pid, "port": 1}).to_string(),
        )
        .unwrap();
    };
    // Dead owner (a pid that cannot exist): the leftover daemon is stopped and the state file removed.
    let mut old = leftover("rigctld-leftover");
    write(old.id(), 4_000_000);
    rigd::reap_stale("t_reap", &dir.0).unwrap();
    assert!(
        old.wait().is_ok_and(|s| !s.success()),
        "the leftover was killed"
    );
    assert!(!dir.0.join("t_reap.rigctld.json").exists());
    // Live owner that is a bench_mcp: refused, nothing killed.
    let mut owner = leftover("bench_mcp-other");
    let mut daemon = leftover("rigctld-live");
    write(daemon.id(), owner.id());
    let e = rigd::reap_stale("t_reap", &dir.0).unwrap_err();
    assert!(e.contains("another bench_mcp"), "{e}");
    assert!(
        daemon.try_wait().unwrap().is_none(),
        "the live owner's daemon is untouched"
    );
    let _ = (owner.kill(), daemon.kill(), owner.wait(), daemon.wait());
}

// Tests [@ANCHOR: hamlib_release_after_crash]
#[test]
fn releasing_after_a_crash_sends_ptt_off_to_the_daemon() {
    let fake = FakeRigctld::serve(|_| ok(&[]));
    assert!(tx::release_after_crash("r", fake.port));
    assert_eq!(fake.received(), ["set_ptt 0"]);
    assert!(
        !tx::release_after_crash("r", 1),
        "nothing listens: quiet false"
    );
}

// Tests [@ANCHOR: hamlib_status]
// Tests [@ANCHOR: hamlib_capability_query]
// Tests [@ANCHOR: hamlib_call]
#[test]
fn status_of_the_dummy_rig_reports_what_it_has_and_what_it_lacks() {
    let dir = Dir::new("status");
    let b = dummy_bench("t_status", &dir, "read_only", "");
    let v = call(&b, "bench_hamlib_status", &json!({})).unwrap();
    assert_eq!(v["frequency_hz"], 145e6);
    assert_eq!(v["mode"], "FM");
    assert_eq!(v["passband_hz"], 15000.0);
    assert_eq!(v["vfo"], "VFOA");
    assert_eq!(v["split"]["enabled"], false);
    assert_eq!(v["power_on"], true);
    assert!(v["levels"]["STRENGTH"]["s_units"].as_f64().is_some(), "{v}");
    assert!(v["levels"]["AGC"]["name"].is_string());
    assert!(v["functions"]["NB"].is_boolean());
    assert!(v["ptt"].as_str().unwrap().starts_with("unknown") || v["ptt"] == "receive");
    assert!(v["unavailable"].is_object());
    // A tool above the tier is refused by the server.
    let e = call(&b, "bench_hamlib_rigctld", &json!({"action": "status"})).unwrap_err();
    assert!(e.contains("needs tier"), "{e}");
    assert!(call(&b, "bench_nonsense", &json!({})).is_err());
    rigd::stop("t_status", &b[0].hamlib).unwrap();
}

// Tests [@ANCHOR: hamlib_get]
// Tests [@ANCHOR: hamlib_signal_readings]
#[test]
fn single_values_and_signal_readings_come_from_the_scripted_receiver() {
    let mut radio = Radio::new(&[-30.0, -24.0, -36.0]);
    let fake = FakeRigctld::serve(move |c| radio.handle(c));
    let b = fake_bench("t_get", fake.port, "read_only", "");
    let get = |args: Value| call(&b, "bench_hamlib_get", &args);
    assert_eq!(
        get(json!({"item": "frequency"})).unwrap()["frequency_hz"],
        7.1e6
    );
    assert_eq!(get(json!({"item": "mode"})).unwrap()["mode"], "USB");
    assert_eq!(get(json!({"item": "ptt"})).unwrap()["ptt"], "receive");
    let s = get(json!({"item": "signal", "count": 3, "interval_ms": 0})).unwrap();
    assert_eq!(s["mean_db_relative_to_s9"], -30.0);
    assert_eq!(s["spread_db"], 12.0);
    assert_eq!(s["s_units"], 4.0);
    assert_eq!(
        get(json!({"item": "level", "name": "strength"})).unwrap()["value"]["text"],
        "S4"
    );
    assert_eq!(
        get(json!({"item": "function", "name": "NB"})).unwrap()["on"],
        false
    );
    // What the rig lacks is an error that says so; bad arguments are refused before a byte is sent.
    assert!(get(json!({"item": "split"}))
        .unwrap_err()
        .contains("RPRT -11"));
    for bad in [
        json!({}),
        json!({"item": "level"}),
        json!({"item": "level", "name": "A B"}),
        json!({"item": "signal", "count": 99}),
        json!({"item": "bogus"}),
    ] {
        assert!(get(bad).is_err());
    }
    let before = fake.received().len();
    assert!(get(json!({"item": "level", "name": "x;y"})).is_err());
    assert_eq!(fake.received().len(), before);
}

// Tests [@ANCHOR: hamlib_rigs]
#[test]
fn rigs_lists_every_rig_with_its_state_and_starts_nothing() {
    let dir = Dir::new("rigs");
    let dummy = dummy_bench("t_rigs_a", &dir, "read_only", "");
    let r = call(&dummy, "bench_hamlib_rigs", &json!({})).unwrap();
    assert_eq!(r["rigs"][0]["rigctld"]["running"], false);
    assert!(r["rigs"][0]["rig"]
        .as_str()
        .unwrap()
        .contains("not started"));
    let fake = FakeRigctld::serve(|_| ok(&["Frequency: 3573000"]));
    let b = fake_bench("t_rigs_b", fake.port, "read_config", "");
    let r = call(&b, "bench_hamlib_rigs", &json!({})).unwrap();
    assert_eq!(r["rigs"][0]["rig"]["frequency_hz"], 3.573e6);
    assert_eq!(r["rigs"][0]["tier"], "ReadConfig");
    assert!(call(&b, "bench_hamlib_rigs", &json!({"instrument": "nope"})).is_err());
}

#[test]
fn a_rig_that_does_not_answer_is_an_error_and_never_a_stale_value() {
    let fake = FakeRigctld::serve(|_| Reply::Silent);
    let b = fake_bench(
        "t_silent",
        fake.port,
        "read_only",
        r#","reply_timeout_s":1"#,
    );
    let e = call(&b, "bench_hamlib_status", &json!({})).unwrap_err();
    assert!(
        e.contains("Next:") && e.contains("bench_hamlib_diagnose"),
        "{e}"
    );
    let e = call(&b, "bench_hamlib_get", &json!({"item": "frequency"})).unwrap_err();
    assert!(e.contains("Next:"), "{e}");
}

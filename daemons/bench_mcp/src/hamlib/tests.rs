// SPDX-License-Identifier: AGPL-3.0-or-later
//! The module's tests. Happy paths run against the real Hamlib dummy rig through a managed rigctld on a free localhost port (the
//! production path); faults run against the fake rigctld of `testkit`.

use super::proto::{Client, RigError};
use super::testkit::*;
use super::*;

/// A fresh directory for a test's state files and logs; removed by `Dir::drop`.
pub struct Dir(pub std::path::PathBuf);

#[cfg(test)]
impl Dir {
    #[cfg(test)]
    pub fn new(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!(
            "bench_mcp-hamlib-test-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
}

#[cfg(test)]
impl Drop for Dir {
    #[cfg(test)]
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
#[cfg(test)]
pub fn dummy_bench(name: &str, dir: &Dir, tier: &str, extra: &str) -> Vec<Instrument> {
    crate::config::parse(&format!(
        r#"{{"instruments":{{"{name}":{{"driver":"hamlib","tier":"{tier}","model":1,"managed_dir":"{}"{extra}}}}}}}"#,
        dir.0.display()
    ))
    .unwrap()
}

/// A bench whose rig is an already running rigctld (the fake) on `port`.
#[cfg(test)]
pub fn fake_bench(name: &str, port: u16, tier: &str, extra: &str) -> Vec<Instrument> {
    crate::config::parse(&format!(r#"{{"instruments":{{"{name}":{{"driver":"hamlib","tier":"{tier}","rigctld_port":{port}{extra}}}}}}}"#)).unwrap()
}

#[cfg(test)]
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
// Tests [@ANCHOR: hamlib_connect]
// Tests [@ANCHOR: hamlib_reopen]
// Tests [@ANCHOR: hamlib_exchange]
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
// Tests [@ANCHOR: hamlib_managed_dir]
// Tests [@ANCHOR: hamlib_endpoint]
// Tests [@ANCHOR: hamlib_kill_slot]
// Tests [@ANCHOR: hamlib_rigctld_explicit_start]
// Tests [@ANCHOR: hamlib_rigctld_status]
// Tests [@ANCHOR: hamlib_rigctld_stop]
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
        let child = std::process::Command::new("bash")
            .args(["-c", &format!("exec -a {argv0} sleep 30")])
            .spawn()
            .unwrap();
        // Wait until the shell has become the program: during the exec the command line reads empty.
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while !std::fs::read(format!("/proc/{}/cmdline", child.id()))
            .map(|b| String::from_utf8_lossy(&b).starts_with(argv0))
            .unwrap_or(false)
        {
            assert!(std::time::Instant::now() < until, "{argv0} did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        child
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
// Tests [@ANCHOR: hamlib_tolerate]
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
// Tests [@ANCHOR: hamlib_level_json]
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

// Tests [@ANCHOR: hamlib_freq_guard]
// Tests [@ANCHOR: hamlib_rx_ranges]
// Tests [@ANCHOR: hamlib_combine_ranges]
#[test]
fn a_frequency_outside_the_receive_limits_is_refused_with_the_limit_and_its_source() {
    let both = control::combine_ranges(&[(150e3, 1.5e9)], &[(1e6, 30e6), (144e6, 148e6)]);
    assert_eq!(both.0, [(1e6, 30e6), (144e6, 148e6)]);
    assert!(both.1.contains("narrowed"));
    assert!(control::combine_ranges(&[], &[]).0.is_empty());
    assert_eq!(control::combine_ranges(&[(1.0, 2.0)], &[]).0, [(1.0, 2.0)]);
    assert!(control::check_freq(7.1e6, &both.0, &both.1).is_ok());
    let e = control::check_freq(100e6, &both.0, &both.1).unwrap_err();
    assert!(
        e.contains("outside the receive range") && e.contains("rx_ranges_hz"),
        "{e}"
    );
    assert!(control::check_freq(f64::NAN, &[], "").is_err());
    assert!(control::check_freq(-1.0, &[], "").is_err());
    // The ranges come from the rig itself.
    let dir = Dir::new("ranges");
    let b = dummy_bench(
        "t_ranges",
        &dir,
        "read_config",
        r#","rx_ranges_hz":[[1000000,30000000]]"#,
    );
    let mut c = open(&b[0]).unwrap();
    let (r, src) = control::rx_ranges(&mut c, &b[0].hamlib).unwrap();
    assert_eq!(r, [(1e6, 30e6)]);
    assert!(src.contains("narrowed"), "{src}");
    let e = call(&b, "bench_hamlib_set", &json!({"frequency_hz": 145e6})).unwrap_err();
    assert!(e.contains("outside the receive range"), "{e}");
}

// Tests [@ANCHOR: hamlib_set_plan]
#[test]
fn set_arguments_are_checked_before_anything_is_sent_and_transmit_side_settings_are_refused() {
    let fake = FakeRigctld::serve(|_| ok(&[]));
    let b = fake_bench("t_plan", fake.port, "read_config", "");
    let set = |a: Value| call(&b, "bench_hamlib_set", &a);
    for (bad, why) in [
        (json!({}), "nothing to set"),
        (json!({"frequency_hz": -5}), "outside"),
        (json!({"frequency_hz": "7"}), "number"),
        (json!({"mode": "XYZ"}), "not a Hamlib mode"),
        (json!({"passband_hz": 2400}), "needs mode"),
        (json!({"vfo": "VFOZ"}), "not one of"),
        (json!({"rit_hz": 50000}), "outside"),
        (json!({"levels": {"RFPOWER": 1}}), "transmitter"),
        (json!({"levels": {"MICGAIN": 0.5}}), "transmitter"),
        (json!({"levels": {"AF": 3}}), "from 0 to 1"),
        (json!({"levels": {"BOGUS": 1}}), "not settable"),
        (json!({"functions": {"TUNER": true}}), "transmit"),
        (json!({"functions": {"VOX": true}}), "transmit"),
        (json!({"functions": {"NB": "yes"}}), "true or false"),
        (json!({"split": {"tx_vfo": "VFOB"}}), "enabled"),
    ] {
        let e = set(bad.clone()).unwrap_err();
        assert!(e.contains(why), "{bad}: {e}");
    }
    assert!(
        fake.received().is_empty(),
        "nothing was sent for a bad argument: {:?}",
        fake.received()
    );
}

// Tests [@ANCHOR: hamlib_set]
#[test]
fn set_changes_the_dummy_rig_and_reads_every_change_back() {
    let dir = Dir::new("set");
    let b = dummy_bench("t_set", &dir, "read_config", "");
    let v = call(
        &b,
        "bench_hamlib_set",
        &json!({"frequency_hz": 7_100_000, "mode": "usb", "passband_hz": 2400, "rit_hz": 100, "split": {"enabled": true, "tx_vfo": "VFOB"}, "levels": {"RF": 0.5, "ATT": 0}, "functions": {"NB": true}}),
    )
    .unwrap();
    let ch = v["changes"].as_array().unwrap();
    assert_eq!(ch[0]["read_back"], 7.1e6);
    assert_eq!(ch[1]["read_back"], "USB");
    assert_eq!(ch[1]["passband_hz"], "2400");
    assert!(ch
        .iter()
        .any(|c| c["set"] == "rit_hz" && c["read_back"] == 100.0));
    assert!(ch
        .iter()
        .any(|c| c["set"] == "split" && c["read_back"] == true));
    assert!(ch
        .iter()
        .any(|c| c["set"] == "function NB" && c["read_back"] == true));
    // The information tools are allowed at a higher tier.
    assert_eq!(
        call(&b, "bench_hamlib_get", &json!({"item": "frequency"})).unwrap()["frequency_hz"],
        7.1e6
    );
}

// Tests [@ANCHOR: hamlib_scan]
// Tests [@ANCHOR: hamlib_scan_steps]
#[test]
fn a_scan_lists_signal_per_step_ranks_the_strongest_and_restores_the_rig() {
    let mut radio = Radio::new(&[-50.0, -20.0, -40.0, -60.0, -45.0]);
    let fake = FakeRigctld::serve(move |c| radio.handle(c));
    let b = fake_bench("t_scan", fake.port, "read_config", "");
    let v = call(&b, "bench_hamlib_scan", &json!({"start_hz": 7_000_000, "stop_hz": 7_100_000, "step_hz": 25_000, "dwell_ms": 20, "top": 2})).unwrap();
    assert_eq!(v["steps"], 5);
    assert_eq!(v["stopped_early"], false);
    assert_eq!(v["strongest"][0]["frequency_hz"], 7_025_000.0);
    assert_eq!(v["strongest"].as_array().unwrap().len(), 2);
    assert_eq!(v["restored"]["frequency"], true);
    let rec = fake.received();
    assert_eq!(rec.last().unwrap(), "set_mode USB 2400");
    assert!(
        rec.contains(&"set_freq 7100000".to_string()),
        "back to the frequency the rig had: {rec:?}"
    );
    // It stops at the first strong signal when asked.
    let mut radio = Radio::new(&[-50.0, -10.0, -40.0]);
    let fake = FakeRigctld::serve(move |c| radio.handle(c));
    let b = fake_bench("t_scan2", fake.port, "read_config", "");
    let v = call(&b, "bench_hamlib_scan", &json!({"start_hz": 7_000_000, "stop_hz": 7_100_000, "step_hz": 25_000, "dwell_ms": 20, "stop_when_above_db": -20})).unwrap();
    assert_eq!(
        (v["steps"].as_u64(), v["stopped_early"].as_bool()),
        (Some(2), Some(true))
    );
    // Limits: too many steps, too long, backwards.
    for bad in [
        json!({"start_hz": 1e6, "stop_hz": 2e6, "step_hz": 100}),
        json!({"start_hz": 1e6, "stop_hz": 1.2e6, "step_hz": 1000, "dwell_ms": 5000}),
        json!({"start_hz": 2e6, "stop_hz": 1e6, "step_hz": 1000}),
        json!({"start_hz": 1e6, "stop_hz": 2e6}),
    ] {
        assert!(call(&b, "bench_hamlib_scan", &bad).is_err(), "{bad}");
    }
}

#[test]
fn a_scan_that_fails_midway_still_puts_the_rig_back() {
    let mut radio = Radio::new(&[-50.0]);
    let mut sets = 0;
    let fake = FakeRigctld::serve(move |c| {
        if c.starts_with("set_freq") {
            sets += 1;
            if sets == 3 {
                return err(-6);
            }
        }
        radio.handle(c)
    });
    let b = fake_bench("t_scanfail", fake.port, "read_config", "");
    let e = call(
        &b,
        "bench_hamlib_scan",
        &json!({"start_hz": 7_000_000, "stop_hz": 7_100_000, "step_hz": 25_000, "dwell_ms": 20}),
    )
    .unwrap_err();
    assert!(e.contains("7050000"), "{e}");
    assert!(
        fake.received()
            .iter()
            .rev()
            .take(2)
            .any(|c| c == "set_freq 7100000"),
        "the original frequency was restored: {:?}",
        fake.received()
    );
}

// Tests [@ANCHOR: hamlib_memory]
// Tests [@ANCHOR: hamlib_ptt_is_on]
#[test]
fn a_memory_channel_is_recalled_and_a_keyed_rig_is_left_alone() {
    let dir = Dir::new("mem");
    let b = dummy_bench("t_mem", &dir, "read_config", "");
    let v = call(&b, "bench_hamlib_memory", &json!({"channel": 3})).unwrap();
    assert_eq!(v["channel"], 3.0);
    assert!(v["frequency_hz"].as_f64().is_some());
    assert!(call(&b, "bench_hamlib_memory", &json!({})).is_err());
    assert!(call(&b, "bench_hamlib_memory", &json!({"channel": 2.5})).is_err());
    let mut radio = Radio::new(&[-50.0]);
    radio.ptt = true;
    let fake = FakeRigctld::serve(move |c| radio.handle(c));
    let b = fake_bench("t_keyed", fake.port, "read_config", "");
    for (tool, args) in [
        ("bench_hamlib_memory", json!({"channel": 1})),
        ("bench_hamlib_set", json!({"frequency_hz": 7.0e6})),
        (
            "bench_hamlib_scan",
            json!({"start_hz": 7e6, "stop_hz": 7.1e6, "step_hz": 5e4}),
        ),
    ] {
        let e = call(&b, tool, &args).unwrap_err();
        assert!(e.contains("PTT on"), "{tool}: {e}");
    }
    assert!(
        fake.received().iter().all(|c| !c.starts_with("set_")),
        "a keyed rig is not changed: {:?}",
        fake.received()
    );
}

// Tests [@ANCHOR: hamlib_device_holders]
// Tests [@ANCHOR: hamlib_diagnose]
// Tests [@ANCHOR: hamlib_lines_note]
// Tests [@ANCHOR: hamlib_speed_in]
#[test]
fn diagnose_reports_the_model_the_device_and_who_holds_it_without_opening_the_port() {
    let dir = Dir::new("diag");
    let file = dir.0.join("tty");
    std::fs::write(&file, "").unwrap();
    let mut holder = std::process::Command::new("bash")
        .args(["-c", &format!("exec 3<>{}; exec sleep 20", file.display())])
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let h = diag::holders(&file);
    assert!(
        h.iter().any(|x| x.starts_with(&holder.id().to_string())),
        "{h:?}"
    );
    let _ = (holder.kill(), holder.wait());
    assert!(diag::speed_in("Serial speed: 4800..19200 baud, 8N1", 9600));
    assert!(!diag::speed_in(
        "Serial speed: 4800..4800 baud, 8N2, ctrl=NONE",
        9600
    ));
    let b = dummy_bench("t_diag", &dir, "read_only", "");
    let v = call(&b, "bench_hamlib_diagnose", &json!({})).unwrap();
    let checks = v["checks"].as_array().unwrap();
    let find = |n: &str| {
        checks
            .iter()
            .find(|c| c["check"] == n)
            .unwrap_or_else(|| panic!("{n} in {v}"))
            .clone()
    };
    assert_eq!(find("rigctld program")["ok"], true);
    assert_eq!(find("model")["ok"], true);
    assert_eq!(find("rigctld running")["ok"], false);
    // No daemon was started by looking.
    assert_eq!(rigd::status("t_diag", &b[0].hamlib)["running"], false);
    // A serial rig whose speed the backend does not allow, and a missing device, are findings.
    let serial = crate::config::parse(&format!(
        r#"{{"instruments":{{"t_diag2":{{"driver":"hamlib","tier":"read_only","model":1021,"device":"/dev/bench_mcp_no_such_port","speed":9600,"managed_dir":"{}"}}}}}}"#,
        dir.0.display()
    ))
    .unwrap();
    let v = call(&serial, "bench_hamlib_diagnose", &json!({})).unwrap();
    let checks = v["checks"].as_array().unwrap();
    assert!(
        checks
            .iter()
            .any(|c| c["check"] == "serial speed" && c["ok"] == false),
        "{v}"
    );
    assert!(
        checks
            .iter()
            .any(|c| c["check"] == "device exists" && c["ok"] == false),
        "{v}"
    );
    assert!(
        checks
            .iter()
            .any(|c| c["check"] == "DTR and RTS lines" && c["ok"] == false),
        "{v}"
    );
    assert!(v["failing"].as_u64().unwrap() >= 3);
}

// Tests [@ANCHOR: hamlib_instrument]
#[test]
fn with_several_rigs_the_call_must_name_one() {
    let fake = FakeRigctld::serve(|_| ok(&["Frequency: 7000000"]));
    let b = crate::config::parse(&format!(
        r#"{{"instruments":{{"a":{{"driver":"hamlib","tier":"read_only","rigctld_port":{0}}},"b":{{"driver":"hamlib","tier":"read_only","rigctld_port":{0}}}}}}}"#,
        fake.port
    ))
    .unwrap();
    let e = call(&b, "bench_hamlib_get", &json!({"item": "frequency"})).unwrap_err();
    assert!(e.contains("name the rig: a, b"), "{e}");
    assert!(call(
        &b,
        "bench_hamlib_get",
        &json!({"item": "frequency", "instrument": "b"})
    )
    .is_ok());
    let e = call(
        &b,
        "bench_hamlib_get",
        &json!({"item": "frequency", "instrument": "zzz"}),
    )
    .unwrap_err();
    assert!(e.contains("no hamlib instrument named"), "{e}");
}

// Tests [@ANCHOR: hamlib_safe_name]
#[test]
fn names_used_in_file_names_cannot_climb_directories() {
    assert_eq!(safe_name("rig-1_a"), "rig-1_a");
    assert_eq!(safe_name("../etc/passwd"), "___etc_passwd");
    assert_eq!(safe_name("a b"), "a_b");
}

// Tests [@ANCHOR: hamlib_log_tail]
#[test]
fn a_daemon_that_exits_at_start_is_reported_with_the_end_of_its_log() {
    let dir = Dir::new("exits");
    // Hamlib has no model with this number: rigctld says so and exits.
    let b = crate::config::parse(&format!(
        r#"{{"instruments":{{"t_exits":{{"driver":"hamlib","tier":"read_only","model":999999,"managed_dir":"{}"}}}}}}"#,
        dir.0.display()
    ))
    .unwrap();
    let e = call(&b, "bench_hamlib_status", &json!({})).unwrap_err();
    assert!(
        e.contains("exited") && e.contains("its log says") && e.contains("Next:"),
        "{e}"
    );
    assert!(std::fs::read_to_string(dir.0.join("t_exits.rigctld.log")).is_ok_and(|l| !l.is_empty()));
}

// Tests [@ANCHOR: hamlib_shutdown]
#[test]
fn shutting_down_stops_the_daemons_it_is_asked_to_and_no_others() {
    let dir = Dir::new("shutdown");
    let a = dummy_bench("t_shut_a", &dir, "read_config", "");
    let b = dummy_bench("t_keep_b", &dir, "read_config", "");
    let port_of = |bench: &[Instrument]| {
        call(bench, "bench_hamlib_rigctld", &json!({"action": "start"})).unwrap()["port"]
            .as_u64()
            .unwrap() as u16
    };
    let (pa, pb) = (port_of(&a), port_of(&b));
    rigd::shutdown_prefix("t_shut_");
    let alive = |p: u16| {
        Client::connect("127.0.0.1", p, Duration::from_secs(1))
            .and_then(|mut c| c.command("get_freq"))
            .is_ok()
    };
    assert!(!alive(pa), "the named rig's daemon is stopped");
    assert!(alive(pb), "another rig's daemon is left running");
    rigd::stop("t_keep_b", &b[0].hamlib).unwrap();
}

// Tests [@ANCHOR: hamlib_apply]
#[test]
fn a_change_that_fails_says_what_was_already_applied() {
    let mut radio = Radio::new(&[-50.0]);
    let fake = FakeRigctld::serve(move |c| {
        if c.starts_with("set_mode") {
            err(-1)
        } else {
            radio.handle(c)
        }
    });
    let b = fake_bench("t_apply", fake.port, "read_config", "");
    let e = call(
        &b,
        "bench_hamlib_set",
        &json!({"frequency_hz": 7.2e6, "mode": "USB"}),
    )
    .unwrap_err();
    assert!(
        e.contains("set mode failed") && e.contains("already applied before it: frequency_hz"),
        "{e}"
    );
}

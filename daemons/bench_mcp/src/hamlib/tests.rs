// SPDX-License-Identifier: AGPL-3.0-or-later
//! The module's tests. Happy paths run against the real Hamlib dummy rig through a managed rigctld on a free localhost port (the
//! production path); faults run against the fake rigctld of `testkit`.

use super::proto::{Client, RigError};
use super::testkit::*;
use super::*;
use std::sync::{Arc, Mutex};

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
    let fake = FakeRigctld::serve(|c| {
        if c == "get_ptt" {
            ok(&["PTT: 0"])
        } else {
            ok(&[])
        }
    });
    assert!(tx::release_after_crash("r", fake.port));
    assert_eq!(fake.received(), ["set_ptt 0", "get_ptt"]);
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

// ---- the station table, the load check and the baseline ----

const TRANSMIT: &str = r#","transmit":{"dummy_load":true,"attested_by":"K6BP","attested_on":"2026-10-29","cabling":"50 ohm load on the antenna port","max_power_w":5,"max_seconds":10,"frequency_ranges":[[7000000,7100000]],"allowed_modes":["CW","USB"],"load_check":{"readings":3,"dwell_ms":50,"references":4}}"#;

#[cfg(test)]
fn sim_rig(
    name: &str,
    dir: &Dir,
    signals: Vec<Signal>,
) -> (FakeRigctld, Arc<Mutex<Sim>>, Vec<Instrument>) {
    let mut sim = Sim::new();
    sim.signals = signals;
    let (fake, shared) = serve_sim(sim);
    let b = crate::config::parse(&format!(
        r#"{{"instruments":{{"{name}":{{"driver":"hamlib","tier":"full","rigctld_port":{},"managed_dir":"{}"{TRANSMIT}}}}}}}"#,
        fake.port,
        dir.0.display()
    ))
    .unwrap();
    (fake, shared, b)
}

#[cfg(test)]
fn client_of(inst: &Instrument) -> Client {
    open(inst).unwrap()
}

// Tests [@ANCHOR: hamlib_stations_tool]
// Tests [@ANCHOR: hamlib_station_set_tool]
#[test]
fn the_station_tools_list_the_table_and_the_owner_edits_it() {
    let dir = Dir::new("stations");
    let b = dummy_bench("t_stations", &dir, "read_config", "");
    let v = call(&b, "bench_hamlib_stations", &json!({})).unwrap();
    assert_eq!(
        v["saved"], false,
        "defaults until the owner changes something"
    );
    let n = v["stations"].as_array().unwrap().len();
    assert!(n > 20 && v["note"].as_str().unwrap().contains("Never assume"));
    let v = call(&b, "bench_hamlib_stations", &json!({"frequency_hz": 7.1e6})).unwrap();
    assert!(v["references_now"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"].as_str().unwrap().starts_with("wwv")));
    call(
        &b,
        "bench_hamlib_station_set",
        &json!({"action": "retire", "id": "wwv-2_5", "note": "off the air, checked 2026-10-29"}),
    )
    .unwrap();
    call(&b, "bench_hamlib_station_set", &json!({"action": "add", "id": "rpt-1", "frequency_hz": 146.94e6, "mode": "FM", "audio": "carrier", "note": "local repeater, heard 2026-10-29"})).unwrap();
    let v = call(&b, "bench_hamlib_stations", &json!({})).unwrap();
    assert_eq!(v["saved"], true);
    assert_eq!(
        v["stations"].as_array().unwrap().len(),
        n,
        "one retired (hidden), one added"
    );
    assert!(!v["stations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == "wwv-2_5"));
    let v = call(
        &b,
        "bench_hamlib_stations",
        &json!({"include_retired": true}),
    )
    .unwrap();
    assert!(v["stations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == "wwv-2_5" && s["retired"] == true));
    assert!(call(
        &b,
        "bench_hamlib_station_set",
        &json!({"action": "remove", "id": "wwv-5"})
    )
    .is_err());
    assert!(call(&b, "bench_hamlib_station_set", &json!({"id": "x"})).is_err());
    let ro = dummy_bench("t_stations_ro", &dir, "read_only", "");
    assert!(call(
        &ro,
        "bench_hamlib_station_set",
        &json!({"action": "note", "id": "wwv-5", "note": "n"})
    )
    .unwrap_err()
    .contains("needs tier"));
}

// Tests [@ANCHOR: hamlib_load_baseline_tool]
// Tests [@ANCHOR: hamlib_baseline_problems]
#[test]
fn the_baseline_needs_the_confirmation_the_attested_range_and_a_quiet_path() {
    let dir = Dir::new("baseline");
    let (fake, sim, b) = sim_rig("t_bl", &dir, vec![]);
    let audio = SimAudio(sim.clone());
    let cfg = &b[0].hamlib;
    let mut c = client_of(&b[0]);
    let go =
        |c: &mut Client, args: Value| loadtools::load_baseline(c, cfg, Some(&audio), "t_bl", &args);
    // The plan changes and sends nothing.
    let n = fake.received().len();
    let v = go(&mut c, json!({"frequency_hz": 7.05e6, "dry_run": true})).unwrap();
    assert_eq!(v["dry_run"], true);
    assert!(
        v["would"].as_array().unwrap().len() >= 3
            && v["cabling"].as_str().unwrap().contains("50 ohm")
    );
    assert_eq!(fake.received().len(), n);
    // Without the person's confirmation nothing is measured or stored.
    let e = go(&mut c, json!({"frequency_hz": 7.05e6})).unwrap_err();
    assert!(
        e.contains("confirm_dummy_load: true") && e.contains("50 ohm"),
        "{e}"
    );
    // A frequency outside the owner's ranges has no baseline.
    let e = go(
        &mut c,
        json!({"frequency_hz": 14.1e6, "confirm_dummy_load": true}),
    )
    .unwrap_err();
    assert!(e.contains("transmit.frequency_ranges"), "{e}");
    assert!(go(&mut c, json!({"confirm_dummy_load": true})).is_err());
    assert!(!load::baseline_file(&dir.0, "t_bl").exists());
    // A quiet path records it, and the rig is put back as it was.
    let v = go(
        &mut c,
        json!({"frequency_hz": 7.05e6, "confirm_dummy_load": true}),
    )
    .unwrap();
    assert_eq!(
        (v["recorded"].clone(), v["attested_by"].clone()),
        (json!(true), json!("K6BP"))
    );
    assert!(v["readings"].as_array().unwrap().len() >= 3);
    {
        let s = sim.lock().unwrap();
        assert_eq!(
            (s.freq, s.mode.as_str(), s.af, s.att, s.preamp),
            (7_100_000.0, "USB", 0.3, 10.0, 0.0),
            "restored"
        );
    }
    assert!(load::baseline_file(&dir.0, "t_bl").exists());
    // A time station in the audio, even with the S-meter at its floor, means no baseline: ticks 40 dB under what the meter shows.
    let dir2 = Dir::new("baseline2");
    let (_f2, sim2, b2) = sim_rig(
        "t_bl2",
        &dir2,
        vec![Signal {
            hz: 10e6,
            db: 0.0,
            audio: "wwv",
        }],
    );
    let audio2 = SimAudio(sim2.clone());
    let mut c2 = client_of(&b2[0]);
    let e = loadtools::load_baseline(
        &mut c2,
        &b2[0].hamlib,
        Some(&audio2),
        "t_bl2",
        &json!({"frequency_hz": 7.05e6, "confirm_dummy_load": true}),
    )
    .unwrap_err();
    assert!(
        e.contains("no baseline was recorded")
            && e.contains("time station")
            && e.contains("not proof"),
        "{e}"
    );
    assert!(!load::baseline_file(&dir2.0, "t_bl2").exists());
    // A dead audio input and an unreadable S-meter also stop it.
    sim2.lock().unwrap().signals.clear();
    sim2.lock().unwrap().audio_dead = true;
    let e = loadtools::load_baseline(
        &mut c2,
        &b2[0].hamlib,
        Some(&audio2),
        "t_bl2",
        &json!({"frequency_hz": 7.05e6, "confirm_dummy_load": true}),
    )
    .unwrap_err();
    assert!(e.contains("only zeros"), "{e}");
    sim2.lock().unwrap().audio_dead = false;
    sim2.lock().unwrap().meter_broken = true;
    let e = loadtools::load_baseline(
        &mut c2,
        &b2[0].hamlib,
        Some(&audio2),
        "t_bl2",
        &json!({"frequency_hz": 7.05e6, "confirm_dummy_load": true}),
    )
    .unwrap_err();
    assert!(e.contains("S-meter cannot be read"), "{e}");
    assert_eq!(
        sim2.lock().unwrap().freq,
        7_100_000.0,
        "restored after the failure too"
    );
}

// Tests [@ANCHOR: hamlib_load_check_tool]
// Tests [@ANCHOR: hamlib_load_measure]
// Tests [@ANCHOR: hamlib_load_read_point]
// Tests [@ANCHOR: hamlib_load_note_heard]
// Tests [@ANCHOR: hamlib_load_audio_seconds]
// Tests [@ANCHOR: hamlib_load_mode_for]
// Tests [@ANCHOR: hamlib_load_read_original]
// Tests [@ANCHOR: hamlib_load_restore]
// Tests [@ANCHOR: hamlib_loadtools_measure]
// Tests [@ANCHOR: hamlib_loadtools_prepare]
// Tests [@ANCHOR: hamlib_load_gate]
#[test]
fn the_load_check_passes_at_the_baseline_and_refuses_a_signal_a_station_a_dead_input_and_a_broken_meter(
) {
    let dir = Dir::new("check");
    let (_fake, sim, b) = sim_rig("t_chk", &dir, vec![]);
    let audio = SimAudio(sim.clone());
    let cfg = &b[0].hamlib;
    let mut c = client_of(&b[0]);
    let check = |c: &mut Client| {
        loadtools::load_check(
            c,
            cfg,
            Some(&audio),
            "t_chk",
            &json!({"frequency_hz": 7.05e6}),
        )
    };
    // Before a baseline exists the check only reports.
    let v = check(&mut c).unwrap();
    assert_eq!(v["verdict"], "no_baseline");
    assert!(v["note"].as_str().unwrap().contains("not proof"));
    loadtools::load_baseline(
        &mut c,
        cfg,
        Some(&audio),
        "t_chk",
        &json!({"frequency_hz": 7.05e6, "confirm_dummy_load": true}),
    )
    .unwrap();
    let v = check(&mut c).unwrap();
    assert_eq!(v["verdict"], "consistent_with_a_load", "{v}");
    assert_eq!(v["baseline"]["attested_by"], "K6BP");
    assert!(v["compared_with_baseline"].as_u64().unwrap() >= 4);
    // WWV 10 MHz appears (S-meter 30 dB up, ticks in the audio): refuse, say which, and remember it was heard.
    sim.lock().unwrap().signals = vec![
        Signal {
            hz: 10e6,
            db: 30.0,
            audio: "wwv",
        },
        Signal {
            hz: 5e6,
            db: 30.0,
            audio: "wwv",
        },
        Signal {
            hz: 15e6,
            db: 30.0,
            audio: "wwv",
        },
        Signal {
            hz: 2.5e6,
            db: 30.0,
            audio: "wwv",
        },
        Signal {
            hz: 20e6,
            db: 30.0,
            audio: "wwv",
        },
    ];
    let v = check(&mut c).unwrap();
    assert_eq!(v["verdict"], "refuse", "{v}");
    let why = v["reasons"].to_string();
    assert!(
        why.contains("time station") && why.contains("over the baseline"),
        "{why}"
    );
    assert!(
        !v["stations_heard_now"].as_array().unwrap().is_empty(),
        "{v}"
    );
    let table = call(&b, "bench_hamlib_stations", &json!({})).unwrap();
    assert!(
        table["stations"].as_array().unwrap().iter().any(|s| s["id"]
            .as_str()
            .unwrap()
            .starts_with("wwv")
            && s["heard_recently"] == true),
        "last_heard is filled from the detection"
    );
    // Ticks 30 dB under what the S-meter would show: the audio alone refuses.
    sim.lock().unwrap().signals = (0..5)
        .map(|i| Signal {
            hz: [2.5e6, 5e6, 10e6, 15e6, 20e6][i],
            db: 0.0,
            audio: "wwv",
        })
        .collect();
    let v = check(&mut c).unwrap();
    assert_eq!(v["verdict"], "refuse");
    sim.lock().unwrap().signals.clear();
    assert_eq!(check(&mut c).unwrap()["verdict"], "consistent_with_a_load");
    // A dead audio input, an unreadable meter and a keyed rig are refusals or errors, never a pass.
    sim.lock().unwrap().audio_dead = true;
    let v = check(&mut c).unwrap();
    assert_eq!(v["verdict"], "refuse");
    assert!(v["reasons"].to_string().contains("only zeros"), "{v}");
    sim.lock().unwrap().audio_dead = false;
    sim.lock().unwrap().meter_broken = true;
    assert!(check(&mut c)
        .unwrap_err()
        .contains("S-meter cannot be read"));
    sim.lock().unwrap().meter_broken = false;
    sim.lock().unwrap().ptt = true;
    assert!(check(&mut c).unwrap_err().contains("PTT on"));
    sim.lock().unwrap().ptt = false;
    // A baseline taken without audio does not stand for a check with audio: settings differ.
    let v = loadtools::load_check(&mut c, cfg, None, "t_chk", &json!({"frequency_hz": 7.05e6}))
        .unwrap();
    assert_eq!(v["verdict"], "refuse");
    assert!(v["reasons"]
        .to_string()
        .contains("other rig or check settings"));
}

#[test]
fn the_load_tools_exist_only_for_a_rig_with_a_transmit_attestation_and_refuse_without_one() {
    let dir = Dir::new("noattest");
    let plain = dummy_bench("t_plain", &dir, "full", "");
    let names = |b: &[Instrument]| {
        tool_defs(b)
            .iter()
            .map(|t| t.name.to_string())
            .collect::<Vec<_>>()
    };
    assert!(!names(&plain).contains(&"bench_hamlib_load_check".to_string()));
    assert!(names(&plain).contains(&"bench_hamlib_stations".to_string()));
    let e = call(
        &plain,
        "bench_hamlib_load_check",
        &json!({"frequency_hz": 7.05e6}),
    )
    .unwrap_err();
    assert!(e.contains("no transmit attestation"), "{e}");
    let (_f, _s, attested) = sim_rig("t_att", &dir, vec![]);
    assert!(names(&attested).contains(&"bench_hamlib_load_check".to_string()));
    assert!(names(&attested).contains(&"bench_hamlib_load_baseline".to_string()));
    let ro = crate::config::parse(&format!(r#"{{"instruments":{{"t_att2":{{"driver":"hamlib","tier":"read_config","rigctld_port":1{TRANSMIT}}}}}}}"#)).unwrap();
    assert!(names(&ro).contains(&"bench_hamlib_load_check".to_string()));
    assert!(
        !names(&ro).contains(&"bench_hamlib_load_baseline".to_string()),
        "recording a baseline is a full-tier tool"
    );
}

// Tests [@ANCHOR: hamlib_rx_check]
// Tests [@ANCHOR: hamlib_rx_listen]
#[test]
fn the_receiver_check_reports_signal_against_audio_and_the_tone_offset_and_restores_the_rig() {
    let dir = Dir::new("rxcheck");
    let (_f, sim, b) = sim_rig(
        "t_rx",
        &dir,
        vec![Signal {
            hz: 7.074e6,
            db: 25.0,
            audio: "carrier",
        }],
    );
    let audio = SimAudio(sim.clone());
    let cfg = &b[0].hamlib;
    let mut c = client_of(&b[0]);
    let v = rxcheck::rx_check(
        &mut c,
        cfg,
        Some(&audio),
        &json!({"frequency_hz": 7.074e6, "mode": "usb", "seconds": 4, "expect_tone_hz": 1450}),
    )
    .unwrap();
    assert!(
        v["signal"]["mean_db_relative_to_s9"].as_f64().unwrap() > -35.0,
        "{v}"
    );
    assert_eq!(v["squelch_open"], true);
    let tone = v["audio"]["dominant_tone"]["hz"].as_f64().unwrap();
    assert!((tone - 1500.0).abs() < 4.0, "{v}");
    assert!(
        (v["audio"]["tone_offset_hz"].as_f64().unwrap() - 50.0).abs() < 4.0,
        "{v}"
    );
    assert!(v["audio"]["noise_floor_dbfs"].as_f64().unwrap() < -50.0);
    assert_eq!(v["audio"]["findings"]["time_station_heard"], false);
    assert_eq!(v["rig_settings_not_restored"], json!([]));
    {
        let s = sim.lock().unwrap();
        assert_eq!((s.freq, s.mode.as_str(), s.af), (7_100_000.0, "USB", 0.3));
    }
    // Without audio it still reports the meter and says why there is no audio.
    let v = rxcheck::rx_check(&mut c, cfg, None, &json!({"frequency_hz": 7.2e6})).unwrap();
    assert!(v["audio"].as_str().unwrap().contains("none configured"));
    // Out of the receive range, keyed, bad arguments.
    assert!(
        rxcheck::rx_check(&mut c, cfg, None, &json!({"frequency_hz": 145e6}))
            .unwrap_err()
            .contains("outside the receive range")
    );
    assert!(rxcheck::rx_check(
        &mut c,
        cfg,
        None,
        &json!({"frequency_hz": 7.2e6, "seconds": 99})
    )
    .is_err());
    assert!(rxcheck::rx_check(&mut c, cfg, None, &json!({})).is_err());
    sim.lock().unwrap().ptt = true;
    assert!(
        rxcheck::rx_check(&mut c, cfg, None, &json!({"frequency_hz": 7.2e6}))
            .unwrap_err()
            .contains("PTT on")
    );
}

// Tests [@ANCHOR: hamlib_audio_resolve]
#[test]
fn a_rig_names_its_audio_input_by_the_names_of_the_audio_instrument() {
    let dir = Dir::new("audioref");
    let none = dummy_bench("t_ar", &dir, "read_config", "");
    assert!(audio_in::resolve(&none, &none[0]).unwrap().is_none());
    let b = crate::config::parse(
        r#"{"instruments":{"rig":{"driver":"hamlib","tier":"read_config","rigctld_port":1,"audio":{"instrument":"nosuch","input":"rig_rx"}},"sound":{"driver":"audio","tier":"read_only"}}}"#,
    )
    .unwrap();
    let rig = b.iter().find(|i| i.name == "rig").unwrap();
    let e = audio_in::resolve(&b, rig).err().unwrap();
    assert!(e.contains("not in the bench config"), "{e}");
    assert!(crate::config::parse(r#"{"instruments":{"rig":{"driver":"hamlib","rigctld_port":1,"audio":{"instrument":"sound"}}}}"#).unwrap_err().contains("audio needs input"));
}

#[test]
fn every_tool_schema_is_valid_json() {
    for spec in SPECS {
        serde_json::from_str::<Value>(spec.props).unwrap_or_else(|e| panic!("{}: {e}", spec.name));
    }
}

// ---- transmitting into the dummy load ----

/// The test rig's attestation: 20 W cap, 7.000 to 7.100 MHz, short intervals and probe so that the tests are quick.
const TX_CFG: &str = r#","transmit":{"dummy_load":true,"attested_by":"K6BP","attested_on":"2026-10-29","cabling":"50 ohm load on the antenna port","max_power_w":20,"max_seconds":2,"frequency_ranges":[[7000000,7100000]],"allowed_modes":["USB","CW"],"swr_probe_s":0.2,"allow_lowering_power":true,"load_check":{"readings":3,"dwell_ms":50,"references":3}}"#;

/// A rig that can transmit, a recorded baseline, and everything to call the transmit tools with.
#[cfg(test)]
struct TxBench {
    _fake: FakeRigctld,
    sim: Arc<Mutex<Sim>>,
    insts: Vec<Instrument>,
    dir: Dir,
    audio: SimAudio,
    spawner: ThreadSpawner,
}

#[cfg(test)]
impl TxBench {
    fn new(name: &str, with_baseline: bool) -> TxBench {
        let dir = Dir::new(name);
        let mut sim = Sim::new();
        sim.rfpower = 0.1;
        let (fake, shared) = serve_sim(sim);
        let insts = crate::config::parse(&format!(
            r#"{{"instruments":{{"{name}":{{"driver":"hamlib","tier":"full","rigctld_port":{},"managed_dir":"{}"{TX_CFG}}}}}}}"#,
            fake.port,
            dir.0.display()
        ))
        .unwrap();
        let b = TxBench {
            audio: SimAudio(shared.clone()),
            sim: shared,
            _fake: fake,
            insts,
            dir,
            spawner: ThreadSpawner {
                events: Arc::new(Mutex::new(vec![])),
            },
        };
        if with_baseline {
            let mut c = open(&b.insts[0]).unwrap();
            loadtools::load_baseline(
                &mut c,
                &b.insts[0].hamlib,
                Some(&b.audio),
                name,
                &json!({"frequency_hz": 7.05e6, "confirm_dummy_load": true}),
            )
            .unwrap();
        }
        // The first contact with a rig that can transmit asks it to stop transmitting; that is not what the tests count.
        drop(open(&b.insts[0]).unwrap());
        {
            let mut s = b.sim.lock().unwrap();
            s.ptt_commands.clear();
            s.log.clear();
        }
        b
    }

    fn deps<'a>(&'a self, alert: Option<&'a dyn txtools::Alerter>) -> txtools::Deps<'a> {
        txtools::Deps {
            spawner: &self.spawner,
            alert,
            audio: Some(&self.audio),
        }
    }

    fn key(&self, args: Value) -> Result<Value, String> {
        let mut c = open(&self.insts[0]).unwrap();
        txtools::key_test(&mut c, &self.insts[0], &self.deps(None), &args)
    }

    fn ptt_log(&self) -> Vec<u8> {
        self.sim.lock().unwrap().ptt_commands.clone()
    }

    fn log_lines(&self) -> Vec<Value> {
        std::fs::read_to_string(tx::log_file(&self.dir.0, &self.insts[0].name))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
}

/// (PTT, power setting) of a simulated rig, read under one lock: a test that takes the lock twice in one expression deadlocks.
#[cfg(test)]
fn snap(sim: &Arc<Mutex<Sim>>) -> (bool, f64) {
    let s = sim.lock().unwrap();
    (s.ptt, s.rfpower)
}

#[cfg(test)]
fn good_args() -> Value {
    json!({"frequency_hz": 7.05e6, "mode": "usb", "power_w": 10, "seconds": 0.5, "sample_every_ms": 100, "reason": "K6BP: check the PA into the load", "confirm_dummy_load": true})
}

// Tests [@ANCHOR: hamlib_tx_args]
#[test]
fn a_test_is_refused_above_the_power_cap_outside_the_ranges_in_a_mode_not_allowed_or_too_long() {
    let b = TxBench::new("tx_args", false);
    let tx_cfg = b.insts[0].hamlib.transmit.as_ref().unwrap();
    let with = |k: &str, v: Value| {
        let mut a = good_args();
        a[k] = v;
        txtools::check_args(&a, tx_cfg)
    };
    let ok = txtools::check_args(&good_args(), tx_cfg).unwrap();
    assert_eq!(
        (ok.mode.as_str(), ok.power_w, ok.seconds),
        ("USB", 10.0, 0.5)
    );
    assert!(
        with("power_w", json!(25))
            .unwrap_err()
            .contains("max_power_w"),
        "above the power cap"
    );
    assert!(
        with("frequency_hz", json!(7.2e6))
            .unwrap_err()
            .contains("frequency_ranges"),
        "outside the owner's ranges"
    );
    assert!(with("frequency_hz", json!(14.1e6))
        .unwrap_err()
        .contains("There is no default"));
    assert!(
        with("mode", json!("fm"))
            .unwrap_err()
            .contains("allowed_modes"),
        "a mode the owner did not allow"
    );
    assert!(with("seconds", json!(5))
        .unwrap_err()
        .contains("max_seconds"));
    assert!(with("seconds", json!(0)).is_err() && with("seconds", json!(-1)).is_err());
    assert!(with("power_w", json!(0)).is_err() && with("power_w", json!("x")).is_err());
    assert!(with("reason", json!("x".repeat(121))).is_err());
    assert!(with("confirm_dummy_load", json!("yes")).is_err());
    let mut none = good_args();
    none.as_object_mut().unwrap().remove("frequency_hz");
    assert!(txtools::check_args(&none, tx_cfg)
        .unwrap_err()
        .contains("frequency_hz is required"));
    // Nothing reaches the rig for any of these.
    let before = b.sim.lock().unwrap().log.len();
    for (k, v) in [
        ("power_w", json!(25)),
        ("frequency_hz", json!(7.2e6)),
        ("mode", json!("fm")),
        ("seconds", json!(5)),
    ] {
        let mut a = good_args();
        a[k] = v;
        assert!(b.key(a).is_err());
    }
    assert_eq!(b.sim.lock().unwrap().ptt_commands.len(), 0);
    assert_eq!(
        b.sim.lock().unwrap().log.len(),
        before,
        "arguments are checked before the rig is asked anything"
    );
    assert!(b
        .sim
        .lock()
        .unwrap()
        .log
        .iter()
        .all(|c| !c.starts_with("set_ptt")));
}

// Tests [@ANCHOR: hamlib_dump_state]
// Tests [@ANCHOR: hamlib_chk_vfo]
#[test]
fn the_level_ranges_of_dump_state_need_a_chk_vfo_first_and_the_client_sends_it() {
    let dir = Dir::new("dumpstate");
    // The real daemon: a plain dump_state has no level ranges; the client's dump_state does.
    let b = dummy_bench("t_dump", &dir, "read_config", "");
    let mut c = open(&b[0]).unwrap();
    let full = c.dump_state().unwrap();
    let min = txtools::rfpower_min(&full.lines);
    assert_eq!(min, Some(0.05), "the dummy rig's lowest RFPOWER setting");
    // The simulation behaves the same way and the client copes with its one-line chk_vfo reply.
    let (fake, _sim) = serve_sim(Sim::new());
    let mut c = Client::connect("127.0.0.1", fake.port, Duration::from_secs(2)).unwrap();
    let short = c.read("dump_state").unwrap();
    assert_eq!(txtools::rfpower_min(&short.lines), None);
    assert_eq!(
        txtools::rfpower_min(&c.dump_state().unwrap().lines),
        Some(0.05)
    );
    assert!(fake.received().contains(&"chk_vfo".to_string()));
    // A daemon that answers chk_vfo in the extended form is also understood; garbage is an error.
    let ext = FakeRigctld::serve(|c| {
        if c == "chk_vfo" {
            ok(&["ChkVFO: 0"])
        } else {
            ok(&["x"])
        }
    });
    assert!(
        Client::connect("127.0.0.1", ext.port, Duration::from_secs(1))
            .unwrap()
            .dump_state()
            .is_ok()
    );
    let junk = FakeRigctld::serve(|_| Reply::Raw("what\n".into()));
    assert!(
        Client::connect("127.0.0.1", junk.port, Duration::from_secs(1))
            .unwrap()
            .dump_state()
            .is_err()
    );
}

// Tests [@ANCHOR: hamlib_tx_min_power]
// Tests [@ANCHOR: hamlib_tx_power_convert]
// Tests [@ANCHOR: hamlib_tx_preflight]
#[test]
fn the_rig_is_read_first_and_a_split_xit_keyed_or_over_cap_rig_is_refused_without_a_byte_sent() {
    let b = TxBench::new("tx_pre", false);
    let mut c = open(&b.insts[0]).unwrap();
    let tx_cfg = b.insts[0].hamlib.transmit.as_ref().unwrap();
    let a = txtools::check_args(&good_args(), tx_cfg).unwrap();
    let rig = txtools::preflight(&mut c, tx_cfg, &a).unwrap();
    assert_eq!(
        (rig.f_min, rig.f_req, rig.f_prev, rig.w_full),
        (0.05, 0.1, 0.1, 100.0)
    );
    assert_eq!(
        txtools::rfpower_min(&["level_gran=0=0,0,0;12=0.05,1,0.002;".into()]),
        Some(0.05)
    );
    assert_eq!(
        txtools::rfpower_min(&["level_gran=12=0,0,0;".into()]),
        None,
        "an unsupported level has no minimum"
    );
    assert_eq!(txtools::rfpower_min(&["nothing".into()]), None);
    let expect_refusal = |c: &mut Client, change: &dyn Fn(&mut Sim), args: Value, text: &str| {
        let old = {
            let s = b.sim.lock().unwrap();
            (s.split, s.xit, s.ptt, s.rfpower)
        };
        change(&mut b.sim.lock().unwrap());
        let a = txtools::check_args(&args, tx_cfg).unwrap();
        let e = txtools::preflight(c, tx_cfg, &a).unwrap_err();
        assert!(
            e.contains(text) && e.contains("Nothing was sent"),
            "{text}: {e}"
        );
        let mut s = b.sim.lock().unwrap();
        (s.split, s.xit, s.ptt, s.rfpower) = old;
    };
    expect_refusal(&mut c, &|s| s.split = true, good_args(), "split is on");
    expect_refusal(&mut c, &|s| s.xit = 100.0, good_args(), "XIT");
    expect_refusal(
        &mut c,
        &|s| s.ptt = true,
        good_args(),
        "already reports PTT on",
    );
    // A power setting above the cap needs the owner's allow_lowering_power; the test config has it, so build one without.
    let mut strict = tx_cfg.clone();
    strict.allow_lowering_power = false;
    b.sim.lock().unwrap().rfpower = 0.5;
    let e = txtools::preflight(&mut c, &strict, &a).unwrap_err();
    assert!(
        e.contains("above the cap") && e.contains("allow_lowering_power"),
        "{e}"
    );
    assert!(
        txtools::preflight(&mut c, tx_cfg, &a).is_ok(),
        "with allow_lowering_power the tool may lower it"
    );
    b.sim.lock().unwrap().rfpower = 0.1;
    // Below the rig's lowest power setting, and outside the rig's own transmit range.
    let low = txtools::check_args(
        &json!({"frequency_hz": 7.05e6, "power_w": 2, "seconds": 0.5}),
        tx_cfg,
    )
    .unwrap();
    assert!(txtools::preflight(&mut c, tx_cfg, &low)
        .unwrap_err()
        .contains("lowest power setting"));
    let mut wide = tx_cfg.clone();
    wide.frequency_ranges = vec![(7.0e6, 7.5e6)];
    let out = txtools::check_args(
        &json!({"frequency_hz": 7.4e6, "power_w": 10, "seconds": 0.5}),
        &wide,
    )
    .unwrap();
    assert!(txtools::preflight(&mut c, &wide, &out)
        .unwrap_err()
        .contains("rig's own transmit range"));
    assert!(
        b.sim
            .lock()
            .unwrap()
            .log
            .iter()
            .all(|c| !c.starts_with("set_")),
        "a refusal changes nothing on the rig: {:?}",
        b.sim.lock().unwrap().log
    );
}

// Tests [@ANCHOR: hamlib_tx_key_test]
// Tests [@ANCHOR: hamlib_tx_gate]
// Tests [@ANCHOR: hamlib_tx_probe]
// Tests [@ANCHOR: hamlib_tx_key]
// Tests [@ANCHOR: hamlib_tx_release]
// Tests [@ANCHOR: hamlib_tx_log]
// Tests [@ANCHOR: hamlib_tx_read_meters]
// Tests [@ANCHOR: hamlib_tx_keyed]
// Tests [@ANCHOR: hamlib_tx_restore]
// Tests [@ANCHOR: hamlib_tx_set_verified]
// Tests [@ANCHOR: hamlib_tx_summary]
#[test]
fn a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval() {
    let b = TxBench::new("tx_ok", true);
    let alert = RecordingAlert {
        sent: Mutex::new(vec![]),
        audible: true,
    };
    let mut c = open(&b.insts[0]).unwrap();
    // The owner asked for an operator alert.
    let mut inst = b.insts[0].clone();
    inst.hamlib.transmit.as_mut().unwrap().operator_alert = true;
    let v = txtools::key_test(&mut c, &inst, &b.deps(Some(&alert)), &good_args()).unwrap();
    assert_eq!(v["load_check"]["verdict"], "consistent_with_a_load", "{v}");
    assert_eq!(v["swr_probe"]["done"], true);
    assert!(
        v["keyed"]["aborted"].is_null() && v["keyed"]["ptt_read_back_off"] == true,
        "{v}"
    );
    assert!(
        v["keyed"]["meters"]["power_w"]["max"].as_f64().unwrap() > 9.0,
        "{v}"
    );
    assert!(v["keyed"]["meters"]["swr"]["max"].as_f64().unwrap() < 2.0);
    assert_eq!(v["rig_settings_not_restored"], json!([]));
    // PTT: the probe on and off, the test on and off; and the rig is not left keyed.
    assert_eq!(b.ptt_log(), [1, 0, 1, 0]);
    {
        let s = b.sim.lock().unwrap();
        assert!(!s.ptt);
        assert_eq!(
            (s.freq, s.mode.as_str(), s.rfpower),
            (7_100_000.0, "USB", 0.1),
            "frequency, mode and power put back"
        );
    }
    // The watchdog was armed for each interval and stood down after the verified release.
    let ev = b.spawner.events.lock().unwrap().clone();
    assert_eq!(ev.iter().filter(|e| *e == "armed").count(), 2, "{ev:?}");
    assert_eq!(
        ev.iter()
            .filter(|e| e.starts_with("stood down (0)"))
            .count(),
        2,
        "{ev:?}"
    );
    assert!(
        !tx::state_file(&b.dir.0, "tx_ok").exists(),
        "no state file once released"
    );
    assert!(!tx::is_active("tx_ok"));
    // The operator alert gave the frequency, the power and the reason.
    let msg = alert.sent.lock().unwrap()[0].clone();
    assert!(
        msg.contains("7050000") && msg.contains("10 watts") && msg.contains("K6BP"),
        "{msg}"
    );
    // The log: a begin and an end line for each interval, with time, frequency, power, duration and reason.
    let log = b.log_lines();
    let begins: Vec<&Value> = log.iter().filter(|l| l["event"] == "begin").collect();
    let ends: Vec<&Value> = log.iter().filter(|l| l["event"] == "end").collect();
    assert_eq!((begins.len(), ends.len()), (2, 2), "{log:?}");
    assert_eq!(begins[0]["kind"], "probe");
    assert_eq!(begins[1]["kind"], "test");
    assert_eq!(begins[1]["frequency_hz"], 7.05e6);
    assert_eq!(begins[1]["power_w"], 10.0);
    assert!(
        begins[1]["time"].as_str().unwrap().len() == 15
            && begins[1]["reason"]
                .as_str()
                .unwrap()
                .contains("check the PA")
    );
    assert_eq!(begins[1]["by"], "K6BP");
    assert_eq!(ends[1]["outcome"], "completed");
    assert!(ends[1]["seconds_keyed"].as_f64().unwrap() >= 0.5);
    // dry_run sends nothing at all and shows the plan.
    let n = b.sim.lock().unwrap().log.len();
    let mut dry = good_args();
    dry["dry_run"] = json!(true);
    dry["confirm_dummy_load"] = json!(false);
    let v = txtools::key_test(&mut c, &b.insts[0], &b.deps(None), &dry).unwrap();
    assert_eq!(v["dry_run"], true);
    assert!(
        v["would"].as_array().unwrap().len() >= 5
            && v["note"].as_str().unwrap().contains("cannot know")
    );
    assert!(
        b.sim.lock().unwrap().log[n..]
            .iter()
            .all(|c| c.starts_with("get_")
                || c.starts_with("chk_vfo")
                || c.starts_with("dump_state")
                || c.starts_with("mW2power")
                || c.starts_with("power2mW")),
        "{:?}",
        &b.sim.lock().unwrap().log[n..]
    );
    assert_eq!(b.ptt_log().len(), 4);
}

// Tests [@ANCHOR: hamlib_tx_operator_alert]
#[test]
fn a_key_test_is_refused_without_confirmation_without_a_reason_without_a_baseline_and_when_the_alert_cannot_be_heard(
) {
    let b = TxBench::new("tx_gates", false);
    let mut args = good_args();
    // No confirmation.
    args["confirm_dummy_load"] = json!(false);
    let e = b.key(args).unwrap_err();
    assert!(
        e.contains("confirm_dummy_load: true") && e.contains("cannot know what is connected"),
        "{e}"
    );
    // No reason.
    let mut args = good_args();
    args["reason"] = json!("");
    assert!(b.key(args).unwrap_err().contains("reason is required"));
    // No baseline: the check reports and the test is refused, saying what the owner does.
    let e = b.key(good_args()).unwrap_err();
    assert!(
        e.contains("no_baseline") && e.contains("bench_hamlib_load_baseline"),
        "{e}"
    );
    assert_eq!(b.ptt_log().len(), 0, "never keyed");
    // With a baseline, an alert that cannot be heard refuses when the owner asked for one.
    let mut c = open(&b.insts[0]).unwrap();
    loadtools::load_baseline(
        &mut c,
        &b.insts[0].hamlib,
        Some(&b.audio),
        "tx_gates",
        &json!({"frequency_hz": 7.05e6, "confirm_dummy_load": true}),
    )
    .unwrap();
    let mut with_alert = b.insts[0].hamlib.clone();
    with_alert.transmit.as_mut().unwrap().operator_alert = true;
    let mut inst = b.insts[0].clone();
    inst.hamlib = with_alert;
    let quiet = RecordingAlert {
        sent: Mutex::new(vec![]),
        audible: false,
    };
    let e = txtools::key_test(&mut c, &inst, &b.deps(Some(&quiet)), &good_args()).unwrap_err();
    assert!(
        e.contains("not audible") && e.contains("nothing was sent") && e.contains("Text to send"),
        "{e}"
    );
    assert!(
        txtools::key_test(&mut c, &inst, &b.deps(None), &good_args())
            .unwrap_err()
            .contains("no audio instrument")
    );
    assert_eq!(b.ptt_log().len(), 0);
}

#[test]
fn a_path_that_hears_a_time_station_is_refused_even_with_a_baseline() {
    let b = TxBench::new("tx_wwv", true);
    b.sim.lock().unwrap().signals = vec![
        Signal {
            hz: 10e6,
            db: 25.0,
            audio: "wwv",
        },
        Signal {
            hz: 5e6,
            db: 25.0,
            audio: "wwv",
        },
        Signal {
            hz: 15e6,
            db: 25.0,
            audio: "wwv",
        },
        Signal {
            hz: 2.5e6,
            db: 25.0,
            audio: "wwv",
        },
        Signal {
            hz: 20e6,
            db: 25.0,
            audio: "wwv",
        },
    ];
    let e = b.key(good_args()).unwrap_err();
    assert!(
        e.contains("not transmitting") && e.contains("refuse") && e.contains("time station"),
        "{e}"
    );
    assert_eq!(b.ptt_log().len(), 0);
    let refused: Vec<Value> = b
        .log_lines()
        .into_iter()
        .filter(|l| l["event"] == "refused")
        .collect();
    assert_eq!(refused.len(), 1, "a refusal is logged too");
}

#[test]
fn an_swr_over_the_limit_or_not_reported_at_the_probe_stops_the_test_after_the_probe_and_the_rig_is_released(
) {
    let b = TxBench::new("tx_swr", true);
    b.sim.lock().unwrap().swr = 4.5;
    let e = b.key(good_args()).unwrap_err();
    assert!(
        e.contains("probe") && (e.contains("over the limit") || e.contains("SWR 4.50")),
        "{e}"
    );
    assert_eq!(
        b.ptt_log(),
        [1, 0],
        "only the low-power probe keyed the rig"
    );
    assert!(!b.sim.lock().unwrap().ptt);
    assert_eq!(
        b.sim.lock().unwrap().rfpower,
        0.1,
        "the power setting is back"
    );
    // A rig that reports 0 (as the dummy rig does) or nothing: an open connection cannot be ruled out.
    b.sim.lock().unwrap().swr = 0.0;
    let e = b.key(good_args()).unwrap_err();
    assert!(
        e.contains("no usable SWR") && e.contains("swr_probe: false"),
        "{e}"
    );
    assert_eq!(b.ptt_log(), [1, 0, 1, 0]);
    assert!(!b.sim.lock().unwrap().ptt);
    assert!(
        tx::believable_swr(1.0).is_some()
            && tx::believable_swr(0.0).is_none()
            && tx::believable_swr(f64::NAN).is_none()
    );
}

// Tests [@ANCHOR: hamlib_tx_swr_valid]
#[test]
fn an_swr_reading_is_believed_only_from_one_up() {
    assert_eq!(tx::believable_swr(1.5), Some(1.5));
    assert_eq!(tx::believable_swr(0.99), None);
    assert_eq!(tx::believable_swr(250.0), None);
}

#[test]
fn the_rig_is_released_when_a_meter_read_fails_while_keyed() {
    let b = TxBench::new("tx_err", true);
    // The probe passes; then every level read fails with an I/O error while keyed.
    let sim = b.sim.clone();
    let _watch = std::thread::spawn(move || {
        // Turn the fault on only for the second keyed interval.
        loop {
            std::thread::sleep(Duration::from_millis(10));
            let mut s = sim.lock().unwrap();
            if s.ptt_commands.len() >= 2 {
                s.error_when_keyed = Some(-6);
                return;
            }
        }
    });
    let v = b.key(good_args());
    let s = b.sim.lock().unwrap();
    assert!(!s.ptt, "released after the error: {:?}", s.ptt_commands);
    let e = v.unwrap();
    assert!(
        e["keyed"]["aborted"]
            .as_str()
            .unwrap()
            .contains("meter while keyed failed"),
        "{e}"
    );
    assert_eq!(e["keyed"]["ptt_read_back_off"], true);
    assert!(b
        .log_lines()
        .iter()
        .any(|l| l["event"] == "end" && l["outcome"] == "aborted"));
}

#[test]
fn a_connection_dropped_while_keyed_is_retried_and_the_rig_is_released_at_the_end() {
    let b = TxBench::new("tx_hup", true);
    let sim = b.sim.clone();
    let _watch = std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(10));
        let mut s = sim.lock().unwrap();
        if s.ptt_commands.len() >= 3 {
            s.hangup_when_keyed = true;
            return;
        }
    });
    let v = b.key(good_args());
    let s = b.sim.lock().unwrap();
    assert!(!s.ptt, "released: {:?} {:?}", s.ptt_commands, v);
    let e = v.unwrap();
    assert!(
        e["keyed"]["aborted"].is_null(),
        "a read is asked again over a new connection: {e}"
    );
    assert_eq!(e["keyed"]["ptt_read_back_off"], true);
}

#[test]
fn the_release_goes_over_a_new_connection_when_the_one_in_use_is_lost_at_the_moment_of_release() {
    let b = TxBench::new("tx_hup2", true);
    let sim = b.sim.clone();
    let _watch = std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(5));
        let mut s = sim.lock().unwrap();
        if s.ptt_commands.len() >= 3 {
            s.hangup_on_unkey = true;
            return;
        }
    });
    let v = b.key(good_args()).unwrap();
    let s = b.sim.lock().unwrap();
    assert!(
        !s.ptt,
        "released over a new connection: {:?}",
        s.ptt_commands
    );
    assert_eq!(v["keyed"]["ptt_read_back_off"], true);
    assert!(
        !s.hangup_on_unkey,
        "the fault was used: the first set_ptt 0 was dropped"
    );
}

#[test]
fn a_rig_that_will_not_unkey_is_reported_loudly_and_left_to_the_watchdog() {
    let b = TxBench::new("tx_stuck", true);
    let sim = b.sim.clone();
    let _watch = std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(10));
        let mut s = sim.lock().unwrap();
        if s.ptt_commands.len() >= 3 {
            s.ptt_sticks = true;
            return;
        }
    });
    let e = b.key(good_args()).unwrap_err();
    assert!(
        e.contains("RELEASE NOT CONFIRMED") && e.contains("Switch it off at the rig"),
        "{e}"
    );
    let ev = b.spawner.events.lock().unwrap().clone();
    assert!(ev.iter().any(|x| x == "left to guard"), "{ev:?}");
    assert!(
        ev.iter().any(|x| x.contains("FAILED")),
        "the watchdog also tried and says so: {ev:?}"
    );
    assert!(
        tx::state_file(&b.dir.0, "tx_stuck").exists(),
        "the record stays so that the next start looks again"
    );
    assert!(b
        .log_lines()
        .iter()
        .any(|l| l["outcome"] == "release_not_confirmed"));
    b.sim.lock().unwrap().ptt_sticks = false;
    b.sim.lock().unwrap().ptt = false;
    let _ = tx::release_prefix("tx_stuck");
}

// Tests [@ANCHOR: hamlib_guard_run]
// Tests [@ANCHOR: hamlib_guard_unkey]
#[test]
fn the_watchdog_unkeys_on_its_deadline_on_a_closed_pipe_and_on_abort_and_stands_down_on_done() {
    let run = |send: Option<&str>,
               close: bool,
               deadline_ms: u64|
     -> (Arc<Mutex<Sim>>, i32, f64, Vec<String>) {
        let mut sim = Sim::new();
        sim.ptt = true;
        sim.rfpower = 0.4;
        let (fake, shared) = serve_sim(sim);
        let (tx_w, rx_w) = std::sync::mpsc::channel();
        if let Some(w) = send {
            tx_w.send(guard::Word::Line(w.into())).unwrap();
        }
        if close {
            tx_w.send(guard::Word::Closed).unwrap();
        }
        let mut armed = Vec::new();
        let mut report = Vec::new();
        let t = std::time::Instant::now();
        let code = guard::run(
            "127.0.0.1",
            fake.port,
            Duration::from_millis(deadline_ms),
            Some(0.05),
            &rx_w,
            &mut armed,
            &mut report,
        );
        assert!(String::from_utf8_lossy(&armed).contains("armed"));
        let report: Vec<String> = String::from_utf8_lossy(&report)
            .lines()
            .map(String::from)
            .collect();
        drop(tx_w);
        (shared, code, t.elapsed().as_secs_f64(), report)
    };
    // The deadline passes with nothing said: un-key, set the minimum power.
    let (sim, code, took, report) = run(None, false, 700);
    assert_eq!(code, 0);
    assert!((0.6..2.0).contains(&took), "{took}");
    assert!(snap(&sim) == (false, 0.05), "on timeout");
    assert!(
        report.iter().any(|l| l.contains("deadline passed")),
        "{report:?}"
    );
    // The pipe closes without done (the server died): at once, long before the deadline.
    let (sim, _, took, report) = run(None, true, 30_000);
    assert!(
        took < 2.0 && snap(&sim) == (false, 0.05),
        "on a closed pipe: {took}"
    );
    assert!(report.iter().any(|l| l.contains("pipe closed")));
    // abort.
    let (sim, _, took, _) = run(Some("abort"), false, 30_000);
    assert!(took < 2.0 && !sim.lock().unwrap().ptt, "on abort");
    // done: the tool has released the rig; the guard leaves it alone (a keyed rig stays as the test left it).
    let (sim, code, _, _) = run(Some("done"), false, 30_000);
    assert_eq!(code, 0);
    let untouched = sim.lock().unwrap().ptt_commands.is_empty();
    assert!(
        untouched && snap(&sim).1 == 0.4,
        "done: the guard does nothing"
    );
    // A guard that cannot reach the rig does not arm, so nothing could be keyed under it.
    let (_t, rx_w) = std::sync::mpsc::channel::<guard::Word>();
    let mut armed = Vec::new();
    let mut report = Vec::new();
    assert_eq!(
        guard::run(
            "127.0.0.1",
            1,
            Duration::from_secs(1),
            None,
            &rx_w,
            &mut armed,
            &mut report
        ),
        3
    );
    assert!(armed.is_empty());
}

// Tests [@ANCHOR: hamlib_tx_release_all]
// Tests [@ANCHOR: hamlib_shutdown_all]
#[test]
fn stopping_the_server_releases_a_rig_that_is_keyed_before_the_daemons_are_stopped() {
    let b = TxBench::new("tx_stop", true);
    let mut args = good_args();
    args["seconds"] = json!(2);
    let sim = b.sim.clone();
    let key = {
        let (insts, audio) = (b.insts.clone(), SimAudio(sim.clone()));
        let events = b.spawner.events.clone();
        std::thread::spawn(move || {
            let mut c = open(&insts[0]).unwrap();
            let spawner = ThreadSpawner { events };
            txtools::key_test(
                &mut c,
                &insts[0],
                &txtools::Deps {
                    spawner: &spawner,
                    alert: None,
                    audio: Some(&audio),
                },
                &args,
            )
        })
    };
    // Wait for the main interval to be keyed.
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while sim.lock().unwrap().ptt_commands.len() < 3 && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(tx::is_active("tx_stop") && sim.lock().unwrap().ptt);
    let failed = tx::release_prefix("tx_stop");
    assert!(failed.is_empty(), "{failed:?}");
    assert!(!sim.lock().unwrap().ptt, "released by the shutdown path");
    assert_eq!(
        sim.lock().unwrap().rfpower,
        0.05,
        "and the power is at the minimum"
    );
    let r = key.join().unwrap().unwrap();
    assert!(
        r["keyed"]["aborted"]
            .as_str()
            .unwrap()
            .contains("un-keyed by something else"),
        "{r}"
    );
    assert!(sim.lock().unwrap().ptt_commands.len() >= 4);
}

// Tests [@ANCHOR: hamlib_tx_safe_state]
#[test]
fn the_safe_state_unkeys_reads_it_back_lowers_the_power_and_works_while_another_call_runs() {
    let b = TxBench::new("tx_safe", false);
    {
        let mut s = b.sim.lock().unwrap();
        s.ptt = true;
        s.rfpower = 0.5;
    }
    let mut c = open(&b.insts[0]).unwrap();
    let v = txtools::safe_state(&mut c, &b.insts[0], &json!({})).unwrap();
    assert_eq!(
        (
            v["ptt"].clone(),
            v["verified"].clone(),
            v["ptt_was"].clone()
        ),
        (json!("receive"), json!(true), json!("on"))
    );
    assert_eq!(
        (
            v["power_setting_before"].clone(),
            v["power_set_to_minimum"].clone()
        ),
        (json!(0.5), json!(0.05))
    );
    assert_eq!(snap(&b.sim), (false, 0.05));
    // Without lowering the power the setting stays.
    b.sim.lock().unwrap().rfpower = 0.3;
    let v = txtools::safe_state(&mut c, &b.insts[0], &json!({"lower_power": false})).unwrap();
    assert!(v["power_set_to_minimum"].is_null() && b.sim.lock().unwrap().rfpower == 0.3);
    assert!(txtools::safe_state(&mut c, &b.insts[0], &json!({"lower_power": "yes"})).is_err());
    // Through the server's dispatch it takes no instrument lock, so it runs while the lock is held.
    let held = crate::instrument_lock("tx_safe");
    let _g = held.lock().unwrap();
    assert!(
        call(&b.insts, "bench_hamlib_safe_state", &json!({})).is_ok(),
        "the safe state does not wait for another call"
    );
    // A rig that sticks is reported as not confirmed, with what to do by hand.
    b.sim.lock().unwrap().ptt = true;
    b.sim.lock().unwrap().ptt_sticks = true;
    let e = call(&b.insts, "bench_hamlib_safe_state", &json!({})).unwrap_err();
    assert!(
        e.contains("SAFE STATE NOT CONFIRMED") && e.contains("front panel"),
        "{e}"
    );
}

// Tests [@ANCHOR: hamlib_tx_startup_check]
// Tests [@ANCHOR: hamlib_tx_take_note]
// Tests [@ANCHOR: hamlib_startup_note]
#[test]
fn the_first_contact_releases_a_rig_left_keyed_by_a_crash_and_says_so_in_the_result() {
    let b = TxBench::new("tx_start", false);
    // The server before this one died with the rig keyed: its record is on disk and the rig is transmitting.
    std::fs::write(
        tx::state_file(&b.dir.0, "tx_start"),
        json!({"pid": 4_000_000, "host": "127.0.0.1", "port": 1, "min_power": 0.05}).to_string(),
    )
    .unwrap();
    {
        let mut s = b.sim.lock().unwrap();
        s.ptt = true;
        s.rfpower = 0.5;
    }
    tx::forget_checked("tx_start");
    let v = call(&b.insts, "bench_hamlib_get", &json!({"item": "frequency"})).unwrap();
    assert_eq!(v["startup_release"]["stuck_ptt_found"], true, "{v}");
    assert_eq!(v["startup_release"]["released"], true);
    assert!(
        snap(&b.sim) == (false, 0.05),
        "released, and the power at the minimum"
    );
    assert!(!tx::state_file(&b.dir.0, "tx_start").exists());
    assert!(b
        .log_lines()
        .iter()
        .any(|l| l["event"] == "startup_release"));
    // The note is given once.
    assert!(
        call(&b.insts, "bench_hamlib_get", &json!({"item": "frequency"}))
            .unwrap()
            .get("startup_release")
            .is_none()
    );
    // A rig that was not transmitting gets only a precaution, and no note in the result.
    tx::forget_checked("tx_start");
    let v = call(&b.insts, "bench_hamlib_get", &json!({"item": "frequency"})).unwrap();
    assert!(v.get("startup_release").is_none());
    assert!(
        b.log_lines()
            .iter()
            .filter(|l| l["event"] == "startup_release")
            .count()
            == 3
    );
}

// Tests [@ANCHOR: hamlib_tx_sweep]
#[test]
fn a_power_sweep_keys_each_step_separately_stops_at_an_abort_and_obeys_the_same_gates() {
    let b = TxBench::new("tx_sweep", true);
    let mut c = open(&b.insts[0]).unwrap();
    let args = json!({"frequency_hz": 7.05e6, "powers_w": [5, 10, 15], "dwell_s": 0.3, "sample_every_ms": 100, "reason": "K6BP: PA linearity", "confirm_dummy_load": true});
    let v = txtools::power_sweep(&mut c, &b.insts[0], &b.deps(None), &args).unwrap();
    let steps = v["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 3, "{v}");
    assert!(steps
        .iter()
        .all(|s| s["ptt_read_back_off"] == true && s["aborted"].is_null()));
    assert!(
        steps[2]["meters"]["power_w"]["max"].as_f64().unwrap()
            > steps[0]["meters"]["power_w"]["max"].as_f64().unwrap()
    );
    assert_eq!(
        b.ptt_log(),
        [1, 0, 1, 0, 1, 0, 1, 0],
        "the probe and three steps, each released before the next"
    );
    assert_eq!(snap(&b.sim), (false, 0.1));
    let ends = b
        .log_lines()
        .into_iter()
        .filter(|l| l["event"] == "end")
        .count();
    assert_eq!(ends, 4);
    // The rules of the single test apply: a power above the cap, falling steps, no confirmation, too much keyed time.
    let mut run = |a: Value| txtools::power_sweep(&mut c, &b.insts[0], &b.deps(None), &a);
    let with = |k: &str, v: Value| {
        let mut a = args.clone();
        a[k] = v;
        a
    };
    assert!(run(with("powers_w", json!([5, 25])))
        .unwrap_err()
        .contains("max_power_w"));
    assert!(run(with("powers_w", json!([10, 5])))
        .unwrap_err()
        .contains("rise"));
    assert!(run(with("powers_w", json!([]))).is_err());
    assert!(run(with("powers_w", json!([1, 2, 3, 4, 5, 6, 7, 8, 9]))).is_err());
    assert!(run(with("confirm_dummy_load", json!(false)))
        .unwrap_err()
        .contains("confirm_dummy_load: true"));
    assert!(run(with("dwell_s", json!(3))).is_err());
    let before = b.ptt_log().len();
    let mut dry = with("dry_run", json!(true));
    dry["confirm_dummy_load"] = json!(false);
    assert_eq!(run(dry).unwrap()["dry_run"], true);
    assert_eq!(b.ptt_log().len(), before);
    // An SWR that climbs past the limit ends the sweep at that step.
    b.sim.lock().unwrap().swr = 1.2;
    let sim = b.sim.clone();
    let _t = std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(10));
        let mut s = sim.lock().unwrap();
        if s.ptt_commands.len() >= before + 4 {
            s.swr = 6.0;
            return;
        }
    });
    let v = run(args.clone()).unwrap();
    let steps = v["steps"].as_array().unwrap();
    assert!(
        steps.len() < 3
            && steps.last().unwrap()["aborted"]
                .as_str()
                .unwrap()
                .contains("SWR"),
        "{v}"
    );
    assert!(!b.sim.lock().unwrap().ptt);
}

#[test]
fn the_transmit_tools_exist_only_with_the_attestation_at_the_full_tier_and_the_safe_state_always() {
    let dir = Dir::new("tx_list");
    let names = |b: &[Instrument]| {
        tool_defs(b)
            .iter()
            .map(|t| t.name.to_string())
            .collect::<Vec<_>>()
    };
    let plain = dummy_bench("t_nolist", &dir, "full", "");
    assert!(names(&plain).contains(&"bench_hamlib_safe_state".to_string()));
    assert!(!names(&plain).contains(&"bench_hamlib_tx_key_test".to_string()));
    let e = call(&plain, "bench_hamlib_tx_key_test", &json!(good_args())).unwrap_err();
    assert!(e.contains("no transmit attestation"), "{e}");
    let e = call(
        &plain,
        "bench_hamlib_tx_power_sweep",
        &json!({"frequency_hz": 7.05e6, "powers_w": [1]}),
    )
    .unwrap_err();
    assert!(e.contains("no transmit attestation"), "{e}");
    let b = TxBench::new("tx_list", false);
    assert!(
        names(&b.insts).contains(&"bench_hamlib_tx_key_test".to_string())
            && names(&b.insts).contains(&"bench_hamlib_tx_power_sweep".to_string())
    );
    let read_config = crate::config::parse(&format!(r#"{{"instruments":{{"t_rc":{{"driver":"hamlib","tier":"read_config","rigctld_port":1{TX_CFG}}}}}}}"#)).unwrap();
    assert!(
        !names(&read_config).contains(&"bench_hamlib_tx_key_test".to_string()),
        "transmitting is a full-tier act"
    );
    assert!(names(&read_config).contains(&"bench_hamlib_safe_state".to_string()));
    let e = call(
        &read_config,
        "bench_hamlib_tx_key_test",
        &json!(good_args()),
    )
    .unwrap_err();
    assert!(e.contains("needs tier"), "{e}");
}

// Tests [@ANCHOR: hamlib_tx_alert]
#[test]
fn the_operator_alert_goes_to_the_audio_instrument_and_an_unknown_one_is_an_error_without_sound() {
    let none: Vec<Instrument> = vec![];
    let a = txtools::AudioAlert {
        insts: &none,
        instrument: "nosuch".into(),
    };
    // No audio instrument is configured, so the sound tools refuse before they could make any sound.
    let e = txtools::Alerter::alert(&a, "test").unwrap_err();
    assert!(e.contains("no audio instrument"), "{e}");
}

// Tests [@ANCHOR: hamlib_guard_main]
#[test]
fn the_watchdog_entry_point_refuses_bad_arguments_before_it_reads_anything() {
    let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(guard::main(&args(&["127.0.0.1"])), 64);
    assert_eq!(guard::main(&args(&["127.0.0.1", "x", "5000", "none"])), 64);
    assert_eq!(
        guard::main(&args(&["127.0.0.1", "4532", "100", "none"])),
        64,
        "a deadline under half a second"
    );
    assert_eq!(
        guard::main(&args(&["127.0.0.1", "4532", "5000", "2"])),
        64,
        "a power setting above 1"
    );
}

// Tests [@ANCHOR: hamlib_guard_spawn]
#[test]
fn a_watchdog_that_does_not_report_armed_stops_the_keying_before_it_starts() {
    // In a test binary the program that is started again is the test harness, which does not know the flag and never says armed;
    // the real process is tried in tests/hamlib_guard.rs and tests/hamlib_e2e.rs.
    let dir = Dir::new("spawn");
    let log = dir.0.join("guard.log");
    let e = guard::Spawner::spawn(
        &guard::ProcessSpawner,
        "127.0.0.1",
        1,
        Duration::from_secs(5),
        None,
        &log,
    )
    .err()
    .unwrap();
    assert!(
        e.contains("did not report armed") && e.contains("not keying"),
        "{e}"
    );
}

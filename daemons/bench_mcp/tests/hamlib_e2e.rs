// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Hamlib tools through the real server, over its MCP stdio protocol, against Hamlib's dummy rig: the production path from the
//! tool call to the managed `rigctld` and the watchdog process. The dummy rig's PTT is a flag; no radio exists and no sound is made.
//!
//! The case that no unit test can show: the server is killed with SIGKILL while the rig is keyed. The watchdog process, which has the
//! other end of the server's pipe, un-keys the rig; the next server finds the left-over record and says so.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

struct Bench {
    dir: PathBuf,
    config: PathBuf,
}

impl Bench {
    fn new(tag: &str) -> Bench {
        let dir = std::env::temp_dir().join(format!("bench_mcp-e2e-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.json");
        let cfg = json!({"instruments": {"dr": {
            "driver": "hamlib", "tier": "full", "model": 1, "managed_dir": dir.join("hamlib"),
            // The dummy rig cannot read its PTT back; Hamlib answers from its cache for this long after a set, which is what lets
            // this test see the PTT from outside.
            "set_conf": {"cache_timeout": 5000},
            "transmit": {"dummy_load": true, "attested_by": "K6BP", "attested_on": "2026-10-29", "cabling": "the dummy rig has no antenna port",
                "max_power_w": 100, "max_seconds": 30, "frequency_ranges": [[7000000, 7100000]], "allowed_modes": ["USB"],
                "swr_probe": false, "load_check": {"readings": 3, "dwell_ms": 50, "references": 3, "max_spread_db": 30, "margin_db": 30}}
        }}});
        std::fs::write(&config, cfg.to_string()).unwrap();
        Bench { dir, config }
    }

    fn managed(&self) -> PathBuf {
        self.dir.join("hamlib")
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        // Stop any rigctld a test left behind (its record names it), then remove the directory.
        if let Ok(t) = std::fs::read_to_string(self.managed().join("dr.rigctld.json")) {
            if let Some(pid) = serde_json::from_str::<Value>(&t)
                .ok()
                .and_then(|v| v["rigctld_pid"].as_i64())
            {
                let _ = Command::new("kill").arg(pid.to_string()).status();
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
    next_id: u64,
}

impl Server {
    fn start(b: &Bench) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_bench_mcp"))
            .env("BENCH_MCP_CONFIG", &b.config)
            .env("BENCH_MCP_OUT", b.dir.join("out"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let mut s = Server {
            child,
            stdin,
            out,
            next_id: 1,
        };
        writeln!(s.stdin, r#"{{"jsonrpc":"2.0","id":0,"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}},"clientInfo":{{"name":"e2e","version":"1"}}}}}}"#).unwrap();
        let mut line = String::new();
        s.out.read_line(&mut line).unwrap();
        assert!(line.contains("bench_mcp"), "{line}");
        writeln!(
            s.stdin,
            r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
        )
        .unwrap();
        s
    }

    fn send(&mut self, tool: &str, args: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        writeln!(self.stdin, "{}", json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": tool, "arguments": args}})).unwrap();
        id
    }

    fn receive(&mut self, id: u64) -> Result<Value, String> {
        loop {
            let mut line = String::new();
            assert!(
                self.out.read_line(&mut line).unwrap() > 0,
                "the server closed its output"
            );
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == id {
                let text = v["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                return if v["result"]["isError"] == true {
                    Err(text)
                } else {
                    Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
                };
            }
        }
    }

    fn call(&mut self, tool: &str, args: Value) -> Result<Value, String> {
        let id = self.send(tool, args);
        self.receive(id)
    }
}

fn ptt_of(port: u16) -> Option<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    s.write_all(b"+\\get_ptt\n").ok()?;
    let mut r = BufReader::new(s);
    for _ in 0..4 {
        let mut l = String::new();
        r.read_line(&mut l).ok()?;
        if let Some(v) = l.trim().strip_prefix("PTT: ") {
            return Some(v.to_string());
        }
        if l.starts_with("RPRT") {
            break;
        }
    }
    None
}

fn daemon_port(dir: &Path) -> u16 {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(t) = std::fs::read_to_string(dir.join("dr.rigctld.json")) {
            if let Some(p) = serde_json::from_str::<Value>(&t)
                .ok()
                .and_then(|v| v["port"].as_u64())
            {
                return p as u16;
            }
        }
        assert!(Instant::now() < until, "no rigctld record");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_until(secs: u64, what: &str, f: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn key_args(seconds: f64) -> Value {
    json!({"frequency_hz": 7.05e6, "mode": "USB", "power_w": 10, "seconds": seconds, "sample_every_ms": 100, "reason": "e2e: dummy rig", "confirm_dummy_load": true})
}

// Tests [@ANCHOR: hamlib_guard_stand_down]
// Tests [@ANCHOR: hamlib_guard_stand_down]
#[test]
fn a_whole_transmit_test_through_the_server_with_the_watchdog_process() {
    let b = Bench::new("whole");
    let mut s = Server::start(&b);
    // The tools that transmit are listed for this rig, and the dry run sends nothing.
    let listed = {
        let id = 90;
        writeln!(
            s.stdin,
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list"}}"#
        )
        .unwrap();
        s.receive_raw(id)
    };
    assert!(
        listed.contains("bench_hamlib_tx_key_test") && listed.contains("bench_hamlib_safe_state")
    );
    // No baseline yet: refused, and the rig is never keyed.
    let e = s
        .call("bench_hamlib_tx_key_test", key_args(0.5))
        .unwrap_err();
    assert!(e.contains("no_baseline"), "{e}");
    s.call(
        "bench_hamlib_load_baseline",
        json!({"frequency_hz": 7.05e6, "confirm_dummy_load": true}),
    )
    .unwrap();
    let check = s
        .call("bench_hamlib_load_check", json!({"frequency_hz": 7.05e6}))
        .unwrap();
    assert_eq!(check["verdict"], "consistent_with_a_load", "{check}");
    let r = s.call("bench_hamlib_tx_key_test", key_args(0.6)).unwrap();
    assert_eq!(r["keyed"]["ptt_read_back_off"], true, "{r}");
    assert!(r["keyed"]["aborted"].is_null(), "{r}");
    let log = std::fs::read_to_string(b.managed().join("dr.keyed.log")).unwrap();
    assert!(
        log.lines()
            .any(|l| l.contains(r#""event":"begin""#) && l.contains("e2e: dummy rig")),
        "{log}"
    );
    assert!(log.lines().any(|l| l.contains(r#""outcome":"completed""#)));
    assert!(
        !b.managed().join("dr.keyed.json").exists(),
        "no record of a keyed rig once it is released"
    );
    let safe = s.call("bench_hamlib_safe_state", json!({})).unwrap();
    assert_eq!(safe["ptt"], "receive");
    drop(s.stdin);
    let _ = s.child.wait();
}

#[test]
fn a_server_killed_while_the_rig_is_keyed_is_covered_by_the_watchdog_and_the_next_server_says_so() {
    let b = Bench::new("sigkill");
    let mut s = Server::start(&b);
    s.call(
        "bench_hamlib_load_baseline",
        json!({"frequency_hz": 7.05e6, "confirm_dummy_load": true}),
    )
    .unwrap();
    let port = daemon_port(&b.managed());
    // An interval far longer than anything a loaded machine could take between keying and the kill (this test failed once under
    // load with 4 s, when the interval could run out on its own); the server is killed in the middle of it.
    let id = s.send("bench_hamlib_tx_key_test", key_args(20.0));
    wait_until(20, "the rig to be keyed", || {
        ptt_of(port).as_deref() == Some("1")
    });
    assert!(
        b.managed().join("dr.keyed.json").exists(),
        "the record exists while the rig may be keyed"
    );
    s.child.kill().unwrap();
    let _ = s.child.wait();
    let _ = id;
    // The watchdog process had the other end of the server's pipe: it un-keys the rig within moments, long before the 20 s are up.
    // The wait is an event (the PTT reads 0); the bound only has to be well under the interval, so a slow machine cannot fail it.
    let killed = Instant::now();
    wait_until(12, "the watchdog to un-key the rig", || {
        ptt_of(port).as_deref() == Some("0")
    });
    assert!(killed.elapsed() < Duration::from_secs(12));
    assert!(
        b.managed().join("dr.keyed.json").exists(),
        "the dead server could not remove its record"
    );
    let guard_log = std::fs::read_to_string(b.managed().join("dr.guard.log")).unwrap_or_default();
    assert!(guard_log.contains("pipe closed"), "{guard_log}");
    // The next server finds the leftover daemon and the record, stops the first, and reports it.
    let mut s2 = Server::start(&b);
    let r = s2
        .call("bench_hamlib_get", json!({"item": "frequency"}))
        .unwrap();
    assert_eq!(r["startup_release"]["stuck_ptt_found"], true, "{r}");
    assert!(!b.managed().join("dr.keyed.json").exists());
    drop(s2.stdin);
    let _ = s2.child.wait();
}

impl Server {
    fn receive_raw(&mut self, id: u64) -> String {
        loop {
            let mut line = String::new();
            assert!(self.out.read_line(&mut line).unwrap() > 0);
            if serde_json::from_str::<Value>(&line).is_ok_and(|v| v["id"] == id) {
                return line;
            }
        }
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! The procedure runner through the real server over its MCP stdio protocol: the cases no unit test can show. A server that is killed
//! with SIGKILL while a run is active leaves a run that the next server marks lost, after putting the instruments it touched in their
//! safe state; the program dies with its server; a SIGTERM runs the cleanup handlers and the safe state before the server exits.
//! The instrument is a fake device on a local socket that speaks the example dialect (`src/example`).

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Fake {
    port: u16,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    fn start() -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(sock) = conn else { break };
                let log = log.clone();
                std::thread::spawn(move || {
                    let mut out = sock.try_clone().unwrap();
                    let mut on = false;
                    for line in BufReader::new(sock).lines() {
                        let Ok(line) = line else { break };
                        log.lock().unwrap().push(line.clone());
                        let reply = match line.as_str() {
                            "*IDN?" => Some("ACME,EXAMPLE-1000,SN-0000,1.0.0".to_string()),
                            "OUTP?" => Some(if on { "ON" } else { "OFF" }.to_string()),
                            "OUTP ON" => {
                                on = true;
                                None
                            }
                            "OUTP OFF" => {
                                on = false;
                                None
                            }
                            "LEV?" => Some("0.1".to_string()),
                            "MEAS:VOLT?" => Some("0.1".to_string()),
                            _ => None,
                        };
                        if let Some(r) = reply {
                            if writeln!(out, "{r}").is_err() {
                                break;
                            }
                        }
                    }
                });
            }
        });
        Fake { port, seen }
    }

    fn count(&self, command: &str) -> usize {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.as_str() == command)
            .count()
    }
}

struct Bench {
    dir: PathBuf,
    config: PathBuf,
}

impl Bench {
    fn new(tag: &str, port: u16) -> Bench {
        let dir =
            std::env::temp_dir().join(format!("bench_mcp-proc-e2e-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.json");
        let cfg = json!({"instruments": {
            "gen": {"driver": "example", "tier": "full", "host": "127.0.0.1", "port": port},
            "procs": {"driver": "procedure", "tier": "full", "dir": dir.join("procedures"), "grace_s": 2.0}
        }});
        std::fs::write(&config, cfg.to_string()).unwrap();
        Bench { dir, config }
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
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

    fn call(&mut self, tool: &str, args: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        writeln!(self.stdin, "{}", json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": tool, "arguments": args}})).unwrap();
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
}

fn wait_until(secs: u64, what: &str, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

const SOURCE: &str = r#"
def leave():
    bench.call("bench_example_safe_state", instrument="gen")
    bench.emit("cleanup", "ran")
bench.cleanup(leave)
bench.call("bench_example_pulse", instrument="gen", level_v=0.1, pulse_s=0.1, confirm=True)
bench.emit("pulsed", True)
while True:
    bench.sleep(0.05)
"#;

fn start_run(s: &mut Server) -> (String, i64) {
    let r = s
        .call(
            "bench_proc_run",
            json!({"source": SOURCE, "tier": "full", "confirm_full_tier": true, "label": "e2e"}),
        )
        .unwrap();
    let id = r["run"].as_str().unwrap().to_string();
    wait_until(30, "the program's first pulse", || {
        s.call("bench_proc_output", json!({"run": id})).unwrap()["emitted"]["pulsed"] == json!(true)
    });
    let pid = s.call("bench_proc_status", json!({"run": id})).unwrap()["pid"]
        .as_i64()
        .unwrap();
    (id, pid)
}

fn alive(pid: i64) -> bool {
    // SAFETY: signal 0 only tests whether the process exists.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[test]
fn a_server_killed_during_a_run_leaves_a_lost_run_and_the_next_server_makes_the_instrument_safe() {
    let fake = Fake::start();
    let b = Bench::new("kill", fake.port);
    let mut a = Server::start(&b);
    let (id, pid) = start_run(&mut a);
    let offs = fake.count("OUTP OFF");
    assert!(alive(pid));
    a.child.kill().unwrap();
    a.child.wait().unwrap();
    wait_until(10, "the program to die with its server", || !alive(pid));
    let mut next = Server::start(&b);
    wait_until(60, "the run to be marked lost", || {
        next.call("bench_proc_status", json!({"run": id})).unwrap()["state"] == "lost"
    });
    let r = next.call("bench_proc_result", json!({"run": id})).unwrap();
    assert_eq!(r["failure"]["kind"], "lost", "{r}");
    assert_eq!(r["safe_state_ok"], json!(true), "{r}");
    assert_eq!(r["safe_state"][0]["tool"], "bench_example_safe_state");
    assert_eq!(
        r["emitted"]["pulsed"],
        json!(true),
        "what the run had emitted survives the restart"
    );
    assert!(
        fake.count("OUTP OFF") > offs,
        "the new server switched the output off"
    );
    let list = next.call("bench_proc_list", json!({})).unwrap();
    assert_eq!(list["runs"][0]["state"], "lost");
    assert_eq!(list["active_here"], json!(0));
    next.child.kill().unwrap();
    next.child.wait().unwrap();
}

#[test]
fn a_terminated_server_runs_the_cleanup_handlers_and_the_safe_state_before_it_exits() {
    let fake = Fake::start();
    let b = Bench::new("term", fake.port);
    let mut a = Server::start(&b);
    let (id, pid) = start_run(&mut a);
    // SAFETY: a plain SIGTERM to the server's own pid.
    unsafe { libc::kill(a.child.id() as i32, libc::SIGTERM) };
    let status = a.child.wait().unwrap();
    assert_eq!(
        status.code(),
        Some(143),
        "the server exits after stopping its runs"
    );
    assert!(!alive(pid));
    let mut next = Server::start(&b);
    let r = next.call("bench_proc_result", json!({"run": id})).unwrap();
    assert_eq!(r["state"], "cancelled", "{r}");
    assert_eq!(r["failure"]["reason"], "shutdown");
    assert_eq!(
        r["emitted"]["cleanup"], "ran",
        "the cleanup handler made its tool call"
    );
    assert!(
        r["safe_state"]
            .as_array()
            .is_some_and(|s| !s.is_empty() && s.iter().all(|x| x["ok"] == json!(true))),
        "{r}"
    );
    next.child.kill().unwrap();
    next.child.wait().unwrap();
}

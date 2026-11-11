// SPDX-License-Identifier: AGPL-3.0-or-later
//! The transmit watchdog as the real process: `bench_mcp --radioctl-guard`, against a virtual radioctl rig through a `radioctld` started here
//! on a free localhost port. "The server" is this test; closing the pipe is the server dying. Nothing is keyed that is not the virtual
//! rig's PTT flag, and no sound is made.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[path = "support/radioctld_binary.rs"]
mod radioctld_binary;

struct Daemon {
    child: Child,
    port: u16,
    dir: std::path::PathBuf,
    /// The connection that keyed the rig. radioctld unkeys a rig when the client that keyed it disconnects, so the keying
    /// connection stays open for the test (the guard process has a connection of its own).
    keyer: Option<TcpStream>,
}

impl Daemon {
    fn start() -> Daemon {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let dir = std::env::temp_dir().join(format!("bench_mcp-guard-test-{port}"));
        std::fs::create_dir_all(&dir).unwrap();
        let conf = dir.join("radioctld.toml");
        // A virtual KX3 with its transmit gate open: the guard's unkey needs a rig that can be keyed.
        std::fs::write(&conf, format!("[server]\ncache_max_age_ms = 0\n\n[[rig]]\nname = \"guard\"\ndescription = \"{}/elecraft-kx3.toml\"\nlisten = \"127.0.0.1:{port}\"\ntransmit = true\n\n[rig.transport]\nkind = \"virtual\"\n", radioctld_binary::rigs_dir().display())).unwrap();
        let child = Command::new(radioctld_binary::radioctld())
            .arg("--config")
            .arg(&conf)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("radioctld is needed for this test");
        let until = Instant::now() + Duration::from_secs(10);
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < until, "radioctld did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
        Daemon { child, port, dir, keyer: None }
    }

    /// One command over a new connection, tried again a few times: a connection can be dropped under a burst of short ones, as it was
    /// with the earlier daemon (the module's own client asks reads again for the same reason).
    fn ask(&self, cmd: &str) -> Vec<String> {
        for attempt in 0..5 {
            if let Some(out) = self.try_ask(cmd) {
                return out;
            }
            std::thread::sleep(Duration::from_millis(100 * (attempt + 1)));
        }
        panic!("radioctld did not answer {cmd}");
    }

    fn try_ask(&self, cmd: &str) -> Option<Vec<String>> {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
        s.write_all(format!("+\\{cmd}\n").as_bytes()).ok()?;
        let mut out = vec![];
        for line in BufReader::new(s).lines() {
            let line = line.ok()?;
            let done = line.starts_with("RPRT");
            out.push(line);
            if done {
                return Some(out);
            }
        }
        None
    }

    /// Keys the rig over a connection that stays open.
    fn key(&mut self) {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        s.write_all(b"+\\set_ptt 1\n").unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut line = String::new();
        while !line.starts_with("RPRT") {
            line.clear();
            r.read_line(&mut line).unwrap();
        }
        assert!(line.starts_with("RPRT 0"), "keyed: {line}");
        self.keyer = Some(s);
    }

    fn ptt(&self) -> String {
        self.ask("get_ptt")
            .iter()
            .find_map(|l| l.strip_prefix("PTT: ").map(String::from))
            .unwrap_or_else(|| "?".into())
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn guard(port: u16, deadline_ms: u64) -> (Child, BufReader<std::process::ChildStdout>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bench_mcp"))
        .args([
            "--radioctl-guard",
            "127.0.0.1",
            &port.to_string(),
            &deadline_ms.to_string(),
            "0.05",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    assert_eq!(
        line.trim(),
        "armed",
        "the guard reports armed once it has reached the rig"
    );
    (child, out)
}

fn wait_exit(child: &mut Child, secs: u64) -> Option<i32> {
    let until = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < until {
        if let Some(s) = child.try_wait().unwrap() {
            return s.code();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn the_guard_process_unkeys_the_rig_when_the_servers_pipe_closes() {
    let mut d = Daemon::start();
    d.key();
    assert_eq!(d.ptt(), "1");
    let (mut g, _out) = guard(d.port, 60_000);
    // The server dies: its end of the pipe closes without "done".
    drop(g.stdin.take());
    assert_eq!(wait_exit(&mut g, 10), Some(0), "the guard acted and exited");
    assert_eq!(d.ptt(), "0", "PTT is off");
    let rfpower: Vec<String> = d.ask("get_level RFPOWER");
    let power: f64 = rfpower.iter().find_map(|l| l.parse().ok()).unwrap_or(1.0);
    assert!(
        power <= 0.05,
        "and the power is at the minimum the guard was given (a virtual KX3 starts at a third of full power): {rfpower:?}"
    );
}

#[test]
fn the_guard_process_unkeys_the_rig_when_its_deadline_passes() {
    let mut d = Daemon::start();
    d.key();
    let (mut g, _out) = guard(d.port, 1_000);
    let began = Instant::now();
    assert_eq!(wait_exit(&mut g, 10), Some(0));
    assert!(
        began.elapsed() >= Duration::from_millis(900),
        "not before the deadline"
    );
    assert_eq!(d.ptt(), "0");
}

#[test]
fn the_guard_process_does_nothing_when_told_done() {
    let mut d = Daemon::start();
    d.key();
    let (mut g, _out) = guard(d.port, 60_000);
    g.stdin.as_mut().unwrap().write_all(b"done\n").unwrap();
    assert_eq!(wait_exit(&mut g, 5), Some(0));
    assert_eq!(
        d.ptt(),
        "1",
        "the tool had released the rig itself; the guard leaves it as it is"
    );
    d.ask("set_ptt 0");
}

#[test]
fn the_guard_does_not_arm_when_it_cannot_reach_the_rig() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_bench_mcp"))
        .args([
            "--radioctl-guard",
            "127.0.0.1",
            &port.to_string(),
            "5000",
            "none",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    assert_eq!(
        wait_exit(&mut child, 10),
        Some(3),
        "no way to the rig: exit without saying armed"
    );
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.is_empty());
    // And bad arguments are refused.
    let bad = Command::new(env!("CARGO_BIN_EXE_bench_mcp"))
        .args(["--radioctl-guard", "127.0.0.1", "x", "5000", "none"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(64));
}

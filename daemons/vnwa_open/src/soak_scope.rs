// SPDX-License-Identifier: AGPL-3.0-or-later
//! The soak harness's scope connection: checked with `*IDN?` and tolerant of a stale single-client SCPI session (to-do 5c1e7a42). Documented in `README.md`.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// A connected scope: a buffered reader and a writer on the same socket.
pub type ScopeStream = (BufReader<TcpStream>, TcpStream);

/// Connects to `addr` (`host:port`) and asks `*IDN?`; on a connect error, a reset or no reply it drops the socket, sleeps the next of `waits` and tries
/// again. After the last attempt the error starts with `NO-GO:` and carries each attempt's error, `diagnose(addr)` and the recommended action.
pub fn open_checked(
    addr: &str,
    waits: &[Duration],
    diagnose: &dyn Fn(&str) -> String,
) -> Result<ScopeStream, String> {
    // [@ANCHOR: vnwa_open_soak_scope_open_checked]
    let mut errors = Vec::new();
    for attempt in 0..=waits.len() {
        match try_once(addr) {
            Ok(s) => return Ok(s),
            Err(e) => errors.push(format!("attempt {}: {e}", attempt + 1)),
        }
        if let Some(w) = waits.get(attempt) {
            std::thread::sleep(*w);
        }
    }
    Err(format!(
        "NO-GO: scope {addr} did not answer *IDN? in {} attempts over {:.0} s ({}). State: {}. Recommended action: power-cycle the scope (its single-client SCPI server probably still holds a dead session from before a host power loss); if that is not the cause, note it in the to-do.",
        errors.len(),
        waits.iter().sum::<Duration>().as_secs_f64(),
        errors.join("; "),
        diagnose(addr)
    ))
}

/// One attempt: connect (5 s limit), send `*IDN?`, read one line (5 s limit); an empty line (the scope closed) is an error.
fn try_once(addr: &str) -> Result<ScopeStream, String> {
    // [@ANCHOR: vnwa_open_soak_scope_try_once]
    let sock = addr
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    let mut w = TcpStream::connect_timeout(&sock, Duration::from_secs(5))
        .map_err(|e| format!("connect: {e}"))?;
    w.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut r = BufReader::new(w.try_clone().map_err(|e| e.to_string())?);
    w.write_all(b"*IDN?\n").map_err(|e| format!("send: {e}"))?;
    let mut l = String::new();
    r.read_line(&mut l).map_err(|e| format!("read: {e}"))?;
    if l.trim().is_empty() {
        return Err("empty reply to *IDN? (the scope closed the connection)".into());
    }
    Ok((r, w))
}

/// The scope's state for a failure report: a one-packet ping, then whether TCP connects to the port of `addr` and to port 80 (2 s limit each). Reads the network only.
pub fn state(addr: &str) -> String {
    // [@ANCHOR: vnwa_open_soak_scope_state]
    let (host, port) = addr.rsplit_once(':').unwrap_or((addr, "5025"));
    let ping = match std::process::Command::new("ping")
        .args(["-c", "1", "-W", "1", host])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(s) if s.success() => "ping answers",
        Ok(_) => "ping does not answer",
        Err(_) => "ping unavailable",
    };
    let mut out = vec![ping.to_string()];
    for p in [port.to_string(), "80".to_string()] {
        out.push(match connects(host, &p) {
            Ok(()) => format!("port {p} accepts"),
            Err(e) => format!("port {p}: {e}"),
        });
    }
    out.join(", ")
}

fn connects(host: &str, port: &str) -> Result<(), String> {
    // [@ANCHOR: vnwa_open_soak_scope_connects]
    let a = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    TcpStream::connect_timeout(&a, Duration::from_secs(2))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    /// A scope that, for the first `resets` connections, lets the query arrive and closes with it unread (which TCP answers with a reset); the next answers.
    fn stale(resets: usize, total: usize) -> (String, std::thread::JoinHandle<usize>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let h = std::thread::spawn(move || {
            for n in 1..=total {
                let (mut s, _) = l.accept().unwrap();
                std::thread::sleep(Duration::from_millis(150));
                if n <= resets {
                    drop(s);
                } else {
                    let mut b = [0u8; 6];
                    s.read_exact(&mut b).unwrap();
                    assert_eq!(&b, b"*IDN?\n");
                    s.write_all(b"SIGLENT,SDS1202X-E,x,y\n").unwrap();
                    let mut rest = Vec::new();
                    let _ = s.read_to_end(&mut rest);
                }
            }
            total
        });
        (addr, h)
    }

    // Tests [@ANCHOR: vnwa_open_soak_scope_open_checked]
    // Tests [@ANCHOR: vnwa_open_soak_scope_try_once]
    #[test]
    fn a_scope_that_resets_the_first_connections_is_waited_out_and_then_answers() {
        let (addr, server) = stale(2, 3);
        let waits = [Duration::from_millis(10); 4];
        let s = open_checked(&addr, &waits, &|_| "unused".into()).unwrap();
        drop(s);
        assert_eq!(
            server.join().unwrap(),
            3,
            "two resets, then the third connection answered"
        );
    }

    // Tests [@ANCHOR: vnwa_open_soak_scope_open_checked]
    // Tests [@ANCHOR: vnwa_open_soak_scope_state]
    #[test]
    fn a_scope_that_never_answers_is_a_no_go_with_the_evidence_and_the_action() {
        let (addr, server) = stale(99, 3);
        let waits = [Duration::from_millis(10); 2];
        let e = open_checked(&addr, &waits, &state).err().unwrap();
        server.join().unwrap();
        assert!(e.starts_with("NO-GO: scope"), "{e}");
        assert!(e.contains("3 attempts"), "{e}");
        assert!(e.contains("attempt 1:") && e.contains("attempt 3:"), "{e}");
        // The report names port 80's state whichever way it goes: a host with a web server on loopback port 80 (this test's scope is 127.0.0.1) says
        // "accepts", one with nothing there says "port 80: <error>".
        assert!(
            e.contains("ping ") && (e.contains("port 80: ") || e.contains("port 80 accepts")),
            "{e}"
        );
        assert!(e.contains("power-cycle the scope"), "{e}");
    }

    // Tests [@ANCHOR: vnwa_open_soak_scope_state]
    // Tests [@ANCHOR: vnwa_open_soak_scope_connects]
    #[test]
    fn the_state_names_an_accepting_port_and_a_refused_one() {
        let live = TcpListener::bind("127.0.0.1:0").unwrap();
        let s = state(&live.local_addr().unwrap().to_string());
        let port = live.local_addr().unwrap().port();
        assert!(s.contains(&format!("port {port} accepts")), "{s}");
        assert!(s.contains("port 80"), "{s}");
    }
}

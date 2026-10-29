// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client of `rigctld`'s documented TCP protocol (the "extended response" form: a command written as `+\get_freq`, answered
//! by an echo line, `Key: value` lines and a final `RPRT <n>`). Hamlib is LGPL; this module talks to its daemon over a socket
//! and links nothing of it.
//!
//! Every exchange is bounded by a timeout. A reply that does not end in `RPRT` is an error, never a value; a connection that
//! failed once is dropped and reopened for the single retry that a read gets. Commands that change the rig are not repeated.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// Why a command did not give a value.
#[derive(Clone, Debug, PartialEq)]
pub enum RigError {
    /// The socket failed, timed out or closed, or the reply was not in the protocol's form: nothing is known about the rig.
    Link(String),
    /// rigctld answered `RPRT <code>` with a negative code: the rig or the backend refused or could not do it.
    Rig { code: i32, cmd: String },
}

impl RigError {
    /// True when the rig or backend does not offer the function (not an outage): the value does not exist, a fact worth reporting.
    pub fn unsupported(&self) -> bool {
        matches!(self, RigError::Rig { code, .. } if matches!(code, -4 | -11 | -18 | -1 | -15 | -17))
    }
}

impl std::fmt::Display for RigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // [@ANCHOR: hamlib_error_display]
        match self {
            RigError::Link(m) => write!(f, "{m}"),
            RigError::Rig { code, cmd } => write!(
                f,
                "rigctld refused {cmd}: RPRT {code} ({}). {}",
                code_name(*code),
                code_next_step(*code)
            ),
        }
    }
}

/// The name of a Hamlib `RIG_E*` return code.
pub fn code_name(code: i32) -> &'static str {
    // [@ANCHOR: hamlib_error_codes]
    match code {
        0 => "OK",
        -1 => "invalid parameter",
        -2 => "invalid configuration",
        -3 => "out of memory",
        -4 => "not implemented by this backend",
        -5 => "the rig did not answer (timeout)",
        -6 => "I/O error talking to the rig",
        -7 => "internal Hamlib error",
        -8 => "protocol error",
        -9 => "command rejected by the rig",
        -10 => "reply truncated",
        -11 => "not available on this rig",
        -12 => "VFO not targetable",
        -13 => "bus error",
        -14 => "bus busy",
        -15 => "invalid argument",
        -16 => "invalid VFO",
        -17 => "argument out of domain",
        -18 => "function deprecated",
        -19 => "security error",
        -20 => "rig not powered on",
        -21 => "limit exceeded",
        -22 => "access denied",
        _ => "unknown Hamlib code",
    }
}

fn code_next_step(code: i32) -> &'static str {
    // [@ANCHOR: hamlib_code_next_step]
    match code {
        -5 | -6 | -8 | -10 | -13 | -14 => "Next: check the cable and that the rig is powered and on the baud rate in the config, run bench_hamlib_diagnose, power-cycle the rig's USB port with the hub tools if it hangs off a switchable port, then bench_hamlib_rigctld restart.",
        -20 => "Next: the rig reports it is off; switch it on at the rig.",
        -4 | -11 | -18 => "The rig or backend does not offer this function; bench_hamlib_models with this model shows what it offers.",
        -2 => "Next: check the model number and the device settings in the config (bench_hamlib_diagnose).",
        _ => "Next: check the argument against bench_hamlib_models for this rig.",
    }
}

/// The text of a reply: the lines between the echo of the command and `RPRT`, and the return code.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub lines: Vec<String>,
}

impl Reply {
    /// The value of a `Key: value` line, the key compared without case.
    pub fn kv(&self, key: &str) -> Option<&str> {
        self.lines.iter().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(key).then(|| v.trim())
        })
    }

    /// The first line that is a bare value (get_level, get_func answer with only the value).
    pub fn bare(&self) -> Option<&str> {
        self.lines.first().map(|s| s.trim())
    }

    /// A finite number from `key`, or from the bare line when `key` is `None`.
    pub fn number(&self, key: Option<&str>) -> Result<f64, RigError> {
        // [@ANCHOR: hamlib_reply_number]
        let text = match key {
            Some(k) => self.kv(k),
            None => self.bare(),
        };
        let text = text.ok_or_else(|| {
            RigError::Link(format!(
                "rigctld reply lacks {}: {:?}",
                key.unwrap_or("a value"),
                self.lines
            ))
        })?;
        text.split_whitespace()
            .next()
            .and_then(|t| t.parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .ok_or_else(|| RigError::Link(format!("rigctld gave {text:?}, which is not a finite number. Next: bench_hamlib_rigctld restart; if it repeats the rig is misreporting.")))
    }
}

/// A command word or argument that may be sent: printable, no spaces inside an argument, no control characters, so that no
/// text from a caller can become a second command.
pub fn check_arg(s: &str) -> Result<&str, String> {
    // [@ANCHOR: hamlib_arg_check]
    if s.is_empty()
        || s.len() > 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.+-/:".contains(&b))
    {
        return Err(format!("{s:?} is not a usable rigctld argument (letters, digits and _ . + - / : only, up to 64 characters)"));
    }
    Ok(s)
}

pub struct Client {
    host: String,
    port: u16,
    timeout: Duration,
    conn: Option<(TcpStream, BufReader<TcpStream>)>,
}

impl Client {
    pub fn connect(host: &str, port: u16, timeout: Duration) -> Result<Client, RigError> {
        // [@ANCHOR: hamlib_connect]
        let mut c = Client {
            host: host.to_string(),
            port,
            timeout,
            conn: None,
        };
        c.reopen()?;
        Ok(c)
    }

    fn describe(&self) -> String {
        format!("rigctld at {}:{}", self.host, self.port)
    }

    fn reopen(&mut self) -> Result<(), RigError> {
        // [@ANCHOR: hamlib_reopen]
        self.conn = None;
        let addr = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|e| RigError::Link(format!("{}: {e}", self.describe())))?
            .next()
            .ok_or_else(|| RigError::Link(format!("{}: no address", self.describe())))?;
        let s = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| {
            RigError::Link(format!("cannot connect to {} ({e}). Next: bench_hamlib_rigctld status, then start or restart; for an existing rigctld check that it runs and the port in the config.", self.describe()))
        })?;
        s.set_nodelay(true).ok();
        let r = s
            .try_clone()
            .map_err(|e| RigError::Link(format!("{}: {e}", self.describe())))?;
        self.conn = Some((s, BufReader::new(r)));
        Ok(())
    }

    /// One command, one reply, bounded by the timeout. No retry; the connection is dropped on any failure so the next command starts clean.
    pub fn command(&mut self, cmd: &str) -> Result<Reply, RigError> {
        // [@ANCHOR: hamlib_command]
        if cmd.bytes().any(|b| b.is_ascii_control()) {
            return Err(RigError::Link(format!(
                "refusing a command with control characters: {cmd:?}"
            )));
        }
        if self.conn.is_none() {
            self.reopen()?;
        }
        let result = self.exchange(cmd);
        if matches!(result, Err(RigError::Link(_))) {
            self.conn = None;
        }
        result
    }

    fn exchange(&mut self, cmd: &str) -> Result<Reply, RigError> {
        // [@ANCHOR: hamlib_exchange]
        let name = cmd.split_whitespace().next().unwrap_or(cmd).to_string();
        let who = self.describe();
        let timeout = self.timeout;
        let (w, r) = self.conn.as_mut().expect("connected");
        let fail = |what: String| {
            RigError::Link(format!("{who}: {what} after {:.0} s (command {name}). Next: bench_hamlib_diagnose; then bench_hamlib_rigctld restart; check the cable and power of the rig.", timeout.as_secs_f64()))
        };
        r.get_ref().set_read_timeout(Some(timeout)).ok();
        w.set_write_timeout(Some(timeout)).ok();
        w.write_all(format!("+\\{cmd}\n").as_bytes())
            .map_err(|e| fail(format!("write failed ({e})")))?;
        let deadline = Instant::now() + timeout;
        let mut lines: Vec<String> = Vec::new();
        let mut first = true;
        loop {
            if Instant::now() > deadline {
                return Err(fail("no complete reply".into()));
            }
            let mut line = String::new();
            let n = r
                .read_line(&mut line)
                .map_err(|e| fail(format!("read failed ({e})")))?;
            if n == 0 {
                return Err(fail("the connection was closed".into()));
            }
            let t = line.trim_end_matches(['\r', '\n']);
            if first {
                first = false;
                // The echo line "get_freq:" names the command. Anything else first is not this protocol.
                if !t.starts_with(&format!("{name}:")) {
                    return Err(fail(format!("an unexpected first line {t:?}")));
                }
                continue;
            }
            if let Some(code) = t.strip_prefix("RPRT ") {
                let code: i32 = code
                    .trim()
                    .parse()
                    .map_err(|_| fail(format!("bad RPRT line {t:?}")))?;
                if code < 0 {
                    return Err(RigError::Rig {
                        code,
                        cmd: cmd.to_string(),
                    });
                }
                return Ok(Reply { lines });
            }
            if lines.len() >= 4000 {
                return Err(fail("a reply of more than 4000 lines".into()));
            }
            lines.push(t.to_string());
        }
    }

    /// A read: tried again once over a fresh connection when the link failed or the rig timed out. Never for commands that change the rig.
    pub fn read(&mut self, cmd: &str) -> Result<Reply, RigError> {
        // [@ANCHOR: hamlib_read_retry]
        match self.command(cmd) {
            Err(RigError::Link(_)) | Err(RigError::Rig { code: -5 | -6, .. }) => {
                std::thread::sleep(Duration::from_millis(100));
                self.conn = None;
                self.command(cmd)
            }
            other => other,
        }
    }

    /// The same, for a number in `key` (or the bare line).
    pub fn read_number(&mut self, cmd: &str, key: Option<&str>) -> Result<f64, RigError> {
        self.read(cmd)?.number(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: hamlib_error_codes]
    // Tests [@ANCHOR: hamlib_error_display]
    // Tests [@ANCHOR: hamlib_code_next_step]
    #[test]
    fn return_codes_have_names_and_a_next_step() {
        assert_eq!(code_name(-5), "the rig did not answer (timeout)");
        assert_eq!(code_name(-11), "not available on this rig");
        assert_eq!(code_name(-999), "unknown Hamlib code");
        let e = RigError::Rig {
            code: -5,
            cmd: "get_freq".into(),
        }
        .to_string();
        assert!(
            e.contains("Next:") && e.contains("bench_hamlib_diagnose"),
            "{e}"
        );
        assert!(RigError::Rig {
            code: -11,
            cmd: "x".into()
        }
        .unsupported());
        assert!(!RigError::Rig {
            code: -5,
            cmd: "x".into()
        }
        .unsupported());
        assert!(!RigError::Link("x".into()).unsupported());
    }

    // Tests [@ANCHOR: hamlib_arg_check]
    #[test]
    fn arguments_cannot_carry_a_second_command() {
        assert!(check_arg("STRENGTH").is_ok());
        assert!(check_arg("7100000").is_ok());
        for bad in [
            "",
            "a b",
            "a\nb",
            "x;y",
            "T 1",
            "\\set_ptt",
            "a\"b",
            &"x".repeat(65),
        ] {
            assert!(check_arg(bad).is_err(), "{bad:?}");
        }
    }

    // Tests [@ANCHOR: hamlib_reply_number]
    #[test]
    fn reply_helpers_read_keys_and_bare_values_and_refuse_garbage() {
        let r = Reply {
            lines: vec!["Frequency: 145000000".into()],
        };
        assert_eq!(r.number(Some("frequency")).unwrap(), 145e6);
        assert_eq!(r.kv("Mode"), None);
        let b = Reply {
            lines: vec!["-27".into()],
        };
        assert_eq!(b.number(None).unwrap(), -27.0);
        let bad = Reply {
            lines: vec!["nan".into()],
        };
        assert!(bad.number(None).is_err());
        let none = Reply { lines: vec![] };
        assert!(none.number(None).is_err());
    }
}

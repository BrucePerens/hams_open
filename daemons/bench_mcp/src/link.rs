// SPDX-License-Identifier: AGPL-3.0-or-later
//! A SCPI link over a TCP socket (port 5025) or a serial port (a USB-serial adapter, 8N1). Reply
//! handling copes with both header settings (`C1:VDIV 1.00E+00V`, or just `1.00E+00`), with
//! length-prefixed `#9` blocks and with headerless streams (a screenshot).

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;

/// A byte stream with a settable read timeout.
pub trait Port: Read + Write + Send {
    fn set_timeout(&mut self, d: Duration);
}

impl Port for TcpStream {
    fn set_timeout(&mut self, d: Duration) {
        self.set_read_timeout(Some(d)).ok();
    }
}

// [@ANCHOR: bench_mcp_link_serial]
/// A serial device at a fixed baud rate, 8 data bits, no parity, 1 stop bit, no flow control.
pub struct Serial {
    f: File,
    timeout: Duration,
}

impl Serial {
    pub fn open(path: &str, baud: u32) -> Result<Serial, String> {
        // [@ANCHOR: bench_mcp_link_serial_open]
        // A device on the config's `reserved_serial` list is never opened (nothing is opened before this check).
        crate::serial_guard::check_reserved_open(path)?;
        let speed = match baud {
            9600 => libc::B9600,
            19200 => libc::B19200,
            38400 => libc::B38400,
            57600 => libc::B57600,
            115200 => libc::B115200,
            _ => return Err(format!("unsupported baud rate {baud}")),
        };
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
            .open(path)
            .map_err(|e| format!("open {path}: {e}"))?;
        // One conversation at a time across processes too: wait up to 10 s for another server
        // process (or test) to release the port. The lock goes with the fd, so it is released on close.
        let start = std::time::Instant::now();
        // SAFETY: fd is valid for the life of `f`.
        while unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            if start.elapsed() > Duration::from_secs(10) {
                return Err(format!("{path} is in use by another process"));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // SAFETY: fd is valid for the life of `f`; termios is plain data filled by tcgetattr.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(f.as_raw_fd(), &mut t) != 0 {
                return Err(format!(
                    "{path}: not a serial port: {}",
                    std::io::Error::last_os_error()
                ));
            }
            libc::cfmakeraw(&mut t);
            t.c_cflag |= libc::CLOCAL | libc::CREAD;
            t.c_cflag &= !(libc::CRTSCTS | libc::CSTOPB | libc::PARENB);
            libc::cfsetispeed(&mut t, speed);
            libc::cfsetospeed(&mut t, speed);
            if libc::tcsetattr(f.as_raw_fd(), libc::TCSANOW, &t) != 0 {
                return Err(format!(
                    "{path}: tcsetattr: {}",
                    std::io::Error::last_os_error()
                ));
            }
            libc::tcflush(f.as_raw_fd(), libc::TCIOFLUSH);
        }
        Ok(Serial {
            f,
            timeout: Duration::from_secs(5),
        })
    }
}

impl Read for Serial {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // [@ANCHOR: bench_mcp_link_serial_read]
        let mut p = libc::pollfd {
            fd: self.f.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        let n = unsafe {
            libc::poll(
                &mut p,
                1,
                self.timeout.as_millis().min(i32::MAX as u128) as i32,
            )
        };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if n == 0 {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.f.read(buf)
    }
}

impl Write for Serial {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.f.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.f.flush()
    }
}

impl Port for Serial {
    fn set_timeout(&mut self, d: Duration) {
        self.timeout = d;
    }
}

// [@ANCHOR: bench_mcp_link_link]
pub struct Link {
    p: Box<dyn Port>,
    /// Command terminator: `\n` for the scope, `\r\n` for the serial supply.
    eol: &'static str,
    default_timeout: Duration,
}

impl Link {
    pub fn tcp(host: &str, port: u16) -> Result<Link, String> {
        // [@ANCHOR: bench_mcp_link_tcp]
        let addr = (host, port)
            .to_socket_addrs()
            .map_err(|e| format!("{host}:{port}: {e}"))?
            .next()
            .ok_or("no address")?;
        let s = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
            .map_err(|e| format!("connect {host}:{port}: {e}"))?;
        s.set_nodelay(true).ok();
        Ok(Link::new(Box::new(s), "\n"))
    }

    pub fn serial(path: &str, baud: u32) -> Result<Link, String> {
        // A supply is opened by a stable name: see `serial_guard::check_supply`.
        crate::serial_guard::check_supply_open(path)?;
        Ok(Link::new(Box::new(Serial::open(path, baud)?), "\r\n"))
    }

    /// A scope link that is checked with `*IDN?` and that waits out a stale session. The Siglent scope serves one SCPI client at a time: after a host
    /// power loss it kept the dead session, accepted TCP on 5025 and reset every connection on its first query (soak 2026-10-29, unconfirmed cause).
    /// Each failed attempt (connect error, reset, no reply) drops the socket, sleeps the next of `waits` and tries again; after the last the error carries
    /// every attempt's text, the state `diagnose(host, port)` reports, and the recommended action.
    pub fn tcp_checked(
        host: &str,
        port: u16,
        waits: &[Duration],
        diagnose: &dyn Fn(&str, u16) -> String,
    ) -> Result<Link, String> {
        // [@ANCHOR: bench_mcp_link_tcp_checked]
        let mut errors = Vec::new();
        for attempt in 0..=waits.len() {
            let tried = Link::tcp(host, port).and_then(|mut l| {
                let id = l.query("*IDN?")?;
                if id.is_empty() {
                    Err("empty reply to *IDN?".to_string())
                } else {
                    Ok(l)
                }
            });
            match tried {
                Ok(l) => return Ok(l),
                Err(e) => errors.push(format!("attempt {}: {e}", attempt + 1)),
            }
            if let Some(w) = waits.get(attempt) {
                std::thread::sleep(*w);
            }
        }
        let total: Duration = waits.iter().sum();
        Err(format!(
            "NO-GO: {host}:{port} did not answer *IDN? in {} attempts over {:.0} s ({}). State: {}. Recommended action: power-cycle the scope (its single-client SCPI server probably still holds a dead session from before a host power loss); if that is not the cause, note it in the to-do.",
            errors.len(),
            total.as_secs_f64(),
            errors.join("; "),
            diagnose(host, port)
        ))
    }

    /// The scope's state for a failure report: a one-packet ping, and whether TCP connects to each of `ports` (2 s limit each). Reads the network only.
    pub fn scope_state(host: &str, ports: &[u16]) -> String {
        // [@ANCHOR: bench_mcp_link_scope_state]
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
        for p in ports {
            let r = (host, *p)
                .to_socket_addrs()
                .ok()
                .and_then(|mut a| a.next())
                .map(|a| TcpStream::connect_timeout(&a, Duration::from_secs(2)));
            out.push(match r {
                Some(Ok(_)) => format!("port {p} accepts"),
                Some(Err(e)) => format!("port {p}: {e}"),
                None => format!("port {p}: no address"),
            });
        }
        out.join(", ")
    }

    fn new(mut p: Box<dyn Port>, eol: &'static str) -> Link {
        // [@ANCHOR: bench_mcp_link_new]
        let default_timeout = Duration::from_secs(10);
        p.set_timeout(default_timeout);
        Link {
            p,
            eol,
            default_timeout,
        }
    }

    pub fn set_timeout_secs(&mut self, secs: u64) {
        self.p.set_timeout(Duration::from_secs(secs));
    }

    fn reset_timeout(&mut self) {
        self.p.set_timeout(self.default_timeout);
    }

    pub fn send(&mut self, cmd: &str) -> Result<(), String> {
        self.p
            .write_all(format!("{cmd}{}", self.eol).as_bytes())
            .map_err(|e| format!("send: {e}"))
    }

    fn byte(&mut self) -> Result<u8, String> {
        // [@ANCHOR: bench_mcp_link_byte]
        let mut b = [0u8; 1];
        self.p
            .read_exact(&mut b)
            .map_err(|e| format!("read: {e}"))?;
        Ok(b[0])
    }

    pub fn line(&mut self) -> Result<String, String> {
        // [@ANCHOR: bench_mcp_link_line]
        let mut v = Vec::new();
        loop {
            let b = self.byte()?;
            if b == b'\n' {
                break;
            }
            v.push(b);
        }
        Ok(String::from_utf8_lossy(&v).trim().to_string())
    }

    pub fn query(&mut self, cmd: &str) -> Result<String, String> {
        self.send(cmd)?;
        self.line()
    }

    /// The value part of a reply: after the last space or comma, so both header settings parse.
    pub fn value(reply: &str) -> &str {
        reply.rsplit([' ', ',']).next().unwrap_or(reply)
    }

    pub fn number(&mut self, cmd: &str) -> Result<f64, String> {
        // [@ANCHOR: bench_mcp_link_number]
        let r = self.query(cmd)?;
        let v = Link::value(&r);
        // Units follow the number (V, Hz, Sa/s, MHz): drop everything after the last digit or decimal point.
        let v = v.trim_end_matches(|c: char| !(c.is_ascii_digit() || c == '.'));
        v.parse::<f64>()
            .map_err(|_| format!("{cmd}: not a number: {r:?}"))
    }

    /// Sends `head`, then `data` as a `#9nnnnnnnnn` block (nine digits of length), then the terminator, for a command whose argument is binary
    /// (the scope's `PNSU` setup restore). The data may hold any byte, newlines included.
    pub fn send_block(&mut self, head: &str, data: &[u8]) -> Result<(), String> {
        // [@ANCHOR: bench_mcp_link_send_block]
        let mut msg = format!("{head}#9{:09}", data.len()).into_bytes();
        msg.extend_from_slice(data);
        msg.extend_from_slice(self.eol.as_bytes());
        self.p.write_all(&msg).map_err(|e| format!("send: {e}"))
    }

    /// A `#9nnnnnnnnn` block, skipping any text before the `#`. Returns the payload.
    pub fn block(&mut self, cmd: &str) -> Result<Vec<u8>, String> {
        // [@ANCHOR: bench_mcp_link_block]
        self.send(cmd)?;
        while self.byte()? != b'#' {}
        let nd = (self.byte()? as char)
            .to_digit(10)
            .ok_or("bad block header")? as usize;
        let mut n = String::new();
        for _ in 0..nd {
            n.push(self.byte()? as char);
        }
        let n: usize = n.parse().map_err(|_| "bad block length")?;
        let mut data = vec![0u8; n];
        self.p
            .read_exact(&mut data)
            .map_err(|e| format!("block: {e}"))?;
        // Trailing terminators, if the instrument sent them.
        self.p.set_timeout(Duration::from_millis(200));
        let mut junk = [0u8; 2];
        let _ = self.p.read(&mut junk);
        self.reset_timeout();
        Ok(data)
    }

    /// Raw stream with no header (the `SCDP` screenshot): read until the instrument goes idle.
    pub fn raw_until_idle(
        &mut self,
        cmd: &str,
        idle_ms: u64,
        overall_s: u64,
    ) -> Result<Vec<u8>, String> {
        // [@ANCHOR: bench_mcp_link_raw_until_idle]
        self.send(cmd)?;
        let start = std::time::Instant::now();
        let mut out = Vec::new();
        let mut buf = [0u8; 65536];
        self.p.set_timeout(Duration::from_millis(idle_ms));
        while start.elapsed().as_secs() < overall_s {
            match self.p.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    if !out.is_empty() {
                        break;
                    }
                }
                Err(e) => return Err(format!("raw read: {e}")),
            }
        }
        self.reset_timeout();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{Link, Serial};
    use std::fs::File;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::os::fd::FromRawFd;
    use std::thread::JoinHandle;
    use std::time::Duration;

    // Tests [@ANCHOR: bench_mcp_link_link]
    #[test]
    fn value_handles_both_header_settings() {
        assert_eq!(Link::value("C1:VDIV 1.00E+00V"), "1.00E+00V");
        assert_eq!(Link::value("1.00E+00"), "1.00E+00");
        assert_eq!(Link::value("PKPK,4.040000E+00"), "4.040000E+00");
        assert_eq!(Link::value("FREQ,****"), "****");
    }

    /// An instrument on a loopback socket: for each `(command, reply)` it reads one line, which must be the command, and
    /// sends the reply bytes. It then waits for the client to hang up and returns the commands it saw.
    fn instrument(script: Vec<(&'static str, Vec<u8>)>) -> (u16, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            for (cmd, reply) in script {
                let mut line = Vec::new();
                let mut b = [0u8; 1];
                while s.read_exact(&mut b).is_ok() && b[0] != b'\n' {
                    line.push(b[0]);
                }
                let line = String::from_utf8_lossy(&line).trim().to_string();
                assert_eq!(line, cmd);
                seen.push(line);
                s.write_all(&reply).unwrap();
            }
            let mut rest = Vec::new();
            let _ = s.read_to_end(&mut rest);
            seen
        });
        (port, h)
    }

    // Tests [@ANCHOR: bench_mcp_link_tcp]
    // Tests [@ANCHOR: bench_mcp_link_new]
    // Tests [@ANCHOR: bench_mcp_link_byte]
    // Tests [@ANCHOR: bench_mcp_link_line]
    // Tests [@ANCHOR: bench_mcp_link_number]
    #[test]
    fn a_tcp_link_sends_commands_and_reads_lines_and_numbers_under_both_header_settings() {
        let (port, server) = instrument(vec![
            ("*IDN?", b"ACME,SCOPE,1,2\r\n".to_vec()),
            ("VDIV?", b"C1:VDIV 1.00E+00V\n".to_vec()),
            ("FREQ?", b"FREQ,1.5E+03Hz\n".to_vec()),
            ("BAD?", b"hello\n".to_vec()),
        ]);
        let mut l = Link::tcp("127.0.0.1", port).unwrap();
        assert_eq!(l.query("*IDN?").unwrap(), "ACME,SCOPE,1,2");
        assert_eq!(l.number("VDIV?").unwrap(), 1.0);
        assert_eq!(l.number("FREQ?").unwrap(), 1500.0);
        let e = l.number("BAD?").unwrap_err();
        assert!(e.contains("not a number"), "{e}");
        drop(l);
        assert_eq!(server.join().unwrap(), ["*IDN?", "VDIV?", "FREQ?", "BAD?"]);
    }

    // Tests [@ANCHOR: bench_mcp_link_tcp]
    // Tests [@ANCHOR: bench_mcp_link_byte]
    // Tests [@ANCHOR: bench_mcp_link_line]
    #[test]
    fn a_tcp_link_to_nothing_or_to_a_peer_that_hangs_up_is_an_error() {
        // A port that was just free and has no listener: the connection is refused.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let e = Link::tcp("127.0.0.1", port).err().unwrap();
        assert!(e.contains("connect"), "{e}");
        // The instrument closes in the middle of a reply: the half line is not returned.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(b"half a rep").unwrap();
        });
        let mut l = Link::tcp("127.0.0.1", port).unwrap();
        server.join().unwrap();
        let e = l.line().unwrap_err();
        assert!(e.starts_with("read:"), "{e}");
    }

    /// A scope that resets the first `resets` connections on their first query (linger 0 makes the close a TCP reset) and answers on the next.
    fn stale_scope(resets: usize, total: usize) -> (u16, JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let mut n = 0;
            while n < total {
                let (mut s, _) = listener.accept().unwrap();
                n += 1;
                let mut b = [0u8; 1];
                while s.read_exact(&mut b).is_ok() && b[0] != b'\n' {}
                if n <= resets {
                    let l = libc::linger {
                        l_onoff: 1,
                        l_linger: 0,
                    };
                    // SAFETY: a valid socket descriptor and a linger struct of the stated size.
                    unsafe {
                        libc::setsockopt(
                            std::os::fd::AsRawFd::as_raw_fd(&s),
                            libc::SOL_SOCKET,
                            libc::SO_LINGER,
                            &l as *const _ as *const libc::c_void,
                            std::mem::size_of::<libc::linger>() as u32,
                        );
                    }
                    drop(s);
                } else {
                    s.write_all(b"ACME,SCOPE,1,2\n").unwrap();
                    let mut rest = Vec::new();
                    let _ = s.read_to_end(&mut rest);
                }
            }
            n
        });
        (port, h)
    }

    // Tests [@ANCHOR: bench_mcp_link_tcp_checked]
    #[test]
    fn a_scope_that_resets_the_first_connections_is_waited_out_and_then_answers() {
        let (port, server) = stale_scope(2, 3);
        let waits = [Duration::from_millis(20); 4];
        let l = Link::tcp_checked("127.0.0.1", port, &waits, &|_, _| "unused".into()).unwrap();
        drop(l);
        assert_eq!(server.join().unwrap(), 3);
    }

    // Tests [@ANCHOR: bench_mcp_link_tcp_checked]
    // Tests [@ANCHOR: bench_mcp_link_scope_state]
    #[test]
    fn a_scope_that_never_answers_gives_a_no_go_with_the_evidence_and_the_action() {
        let (port, server) = stale_scope(99, 3);
        let waits = [Duration::from_millis(10); 2];
        // Something that accepts, standing in for port 80 being closed and 5025 open.
        let live = TcpListener::bind("127.0.0.1:0").unwrap();
        let live_port = live.local_addr().unwrap().port();
        let e = Link::tcp_checked("127.0.0.1", port, &waits, &|h, _| {
            Link::scope_state(h, &[live_port])
        })
        .err()
        .unwrap();
        assert_eq!(server.join().unwrap(), 3, "one connection per attempt");
        assert!(e.starts_with("NO-GO:"), "{e}");
        assert!(e.contains("3 attempts"), "{e}");
        assert!(e.contains("attempt 1:") && e.contains("attempt 3:"), "{e}");
        assert!(e.contains(&format!("port {live_port} accepts")), "{e}");
        assert!(e.contains("power-cycle the scope"), "{e}");
    }

    // Tests [@ANCHOR: bench_mcp_link_scope_state]
    #[test]
    fn the_scope_state_reports_an_accepting_and_a_refused_port() {
        let live = TcpListener::bind("127.0.0.1:0").unwrap();
        let live_port = live.local_addr().unwrap().port();
        let dead_port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let s = Link::scope_state("127.0.0.1", &[live_port, dead_port]);
        assert!(s.contains(&format!("port {live_port} accepts")), "{s}");
        assert!(s.contains(&format!("port {dead_port}: ")), "{s}");
        assert!(!s.contains(&format!("port {dead_port} accepts")), "{s}");
    }

    // Tests [@ANCHOR: bench_mcp_link_block]
    // Tests [@ANCHOR: bench_mcp_link_byte]
    // Tests [@ANCHOR: bench_mcp_link_line]
    #[test]
    fn a_length_prefixed_block_is_read_after_any_text_and_the_next_reply_is_not_lost() {
        let mut reply = b"WF ALL:#9000000005hello".to_vec();
        reply.extend_from_slice(b"\n\n");
        let (port, server) = instrument(vec![
            ("WAV?", reply),
            ("NEXT?", b"after\n".to_vec()),
            ("BAD?", b"#x\n".to_vec()),
        ]);
        let mut l = Link::tcp("127.0.0.1", port).unwrap();
        assert_eq!(l.block("WAV?").unwrap(), b"hello");
        assert_eq!(l.query("NEXT?").unwrap(), "after");
        assert_eq!(l.block("BAD?").unwrap_err(), "bad block header");
        drop(l);
        server.join().unwrap();
    }

    // Tests [@ANCHOR: bench_mcp_link_send_block]
    #[test]
    fn a_binary_block_goes_out_with_its_length_and_every_byte_intact() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            s.read_to_end(&mut got).unwrap();
            got
        });
        let mut l = Link::tcp("127.0.0.1", port).unwrap();
        l.send_block("PNSU ", &[0, 10, 13, 255, b'x']).unwrap();
        drop(l);
        assert_eq!(server.join().unwrap(), b"PNSU #9000000005\0\n\r\xffx\n");
    }

    // Tests [@ANCHOR: bench_mcp_link_raw_until_idle]
    #[test]
    fn a_headerless_stream_is_read_until_the_instrument_goes_idle() {
        let big: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let (port, server) = instrument(vec![("SCDP", big.clone())]);
        let mut l = Link::tcp("127.0.0.1", port).unwrap();
        let got = l.raw_until_idle("SCDP", 300, 10).unwrap();
        assert_eq!(got, big);
        drop(l);
        server.join().unwrap();
    }

    /// A pseudo-terminal: the file for the instrument side and the path of the side the link opens.
    fn pty() -> (File, String) {
        // SAFETY: plain libc calls on the descriptor posix_openpt returns, which the File then owns.
        unsafe {
            let m = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            assert!(m >= 0);
            assert_eq!(libc::grantpt(m), 0);
            assert_eq!(libc::unlockpt(m), 0);
            let mut buf = [0 as libc::c_char; 128];
            assert_eq!(libc::ptsname_r(m, buf.as_mut_ptr(), buf.len()), 0);
            let path = std::ffi::CStr::from_ptr(buf.as_ptr())
                .to_string_lossy()
                .into_owned();
            (File::from_raw_fd(m), path)
        }
    }

    // Tests [@ANCHOR: bench_mcp_link_serial_open]
    // Tests [@ANCHOR: bench_mcp_link_serial_read]
    // Tests [@ANCHOR: bench_mcp_link_new]
    // Tests [@ANCHOR: bench_mcp_link_line]
    #[test]
    fn a_serial_link_talks_to_a_device_with_crlf_and_times_out_when_it_is_silent() {
        let (mut device, path) = pty();
        let mut l = Link::serial(&path, 115200).unwrap();
        l.send("MEAS:VOLT?").unwrap();
        let mut got = [0u8; 12];
        device.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"MEAS:VOLT?\r\n");
        device.write_all(b"3.30\r\n").unwrap();
        assert_eq!(l.line().unwrap(), "3.30");
        // Nothing more is coming: the read gives up at the timeout instead of waiting for ever.
        l.set_timeout_secs(1);
        let t = std::time::Instant::now();
        let e = l.line().unwrap_err();
        assert!(e.starts_with("read:"), "{e}");
        assert!(t.elapsed().as_millis() >= 900 && t.elapsed().as_secs() < 5);
    }

    // Tests [@ANCHOR: bench_mcp_link_serial_open]
    // Tests [@ANCHOR: bench_mcp_link_serial]
    #[test]
    fn a_serial_port_that_is_not_one_or_has_an_unsupported_baud_rate_is_refused() {
        let (_device, path) = pty();
        assert_eq!(
            Serial::open(&path, 1234).err().unwrap(),
            "unsupported baud rate 1234"
        );
        let e = Serial::open("/nonexistent/ttyUSB9", 9600).err().unwrap();
        assert!(e.starts_with("open /nonexistent/ttyUSB9"), "{e}");
        // A regular file opens but is not a terminal.
        let dir = crate::testdir::TestDir::new("bench_link");
        let file = dir.join("plain");
        std::fs::write(&file, b"x").unwrap();
        let e = Serial::open(file.to_str().unwrap(), 9600).err().unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert!(e.contains("not a serial port"), "{e}");
    }
}

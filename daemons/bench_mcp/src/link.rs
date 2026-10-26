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

/// A serial device at a fixed baud rate, 8 data bits, no parity, 1 stop bit, no flow control.
pub struct Serial {
    f: File,
    timeout: Duration,
}

impl Serial {
    pub fn open(path: &str, baud: u32) -> Result<Serial, String> {
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

pub struct Link {
    p: Box<dyn Port>,
    /// Command terminator: `\n` for the scope, `\r\n` for the serial supply.
    eol: &'static str,
    default_timeout: Duration,
}

impl Link {
    pub fn tcp(host: &str, port: u16) -> Result<Link, String> {
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
        Ok(Link::new(Box::new(Serial::open(path, baud)?), "\r\n"))
    }

    fn new(mut p: Box<dyn Port>, eol: &'static str) -> Link {
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
        let mut b = [0u8; 1];
        self.p
            .read_exact(&mut b)
            .map_err(|e| format!("read: {e}"))?;
        Ok(b[0])
    }

    pub fn line(&mut self) -> Result<String, String> {
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
        let r = self.query(cmd)?;
        let v = Link::value(&r);
        let v = v.trim_end_matches(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E');
        v.parse::<f64>()
            .map_err(|_| format!("{cmd}: not a number: {r:?}"))
    }

    /// A `#9nnnnnnnnn` block, skipping any text before the `#`. Returns the payload.
    pub fn block(&mut self, cmd: &str) -> Result<Vec<u8>, String> {
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
    use super::Link;

    #[test]
    fn value_handles_both_header_settings() {
        assert_eq!(Link::value("C1:VDIV 1.00E+00V"), "1.00E+00V");
        assert_eq!(Link::value("1.00E+00"), "1.00E+00");
        assert_eq!(Link::value("PKPK,4.040000E+00"), "4.040000E+00");
        assert_eq!(Link::value("FREQ,****"), "****");
    }
}

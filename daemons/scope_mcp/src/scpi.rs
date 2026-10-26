// SPDX-License-Identifier: AGPL-3.0-or-later
//! SCPI over TCP (port 5025) for a Siglent SDS1000X-E style oscilloscope. Reply handling copes with
//! the header setting (`C1:VDIV 1.00E+00V`, or just `1.00E+00`) and with length-prefixed blocks.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub struct Scope {
    s: TcpStream,
}

impl Scope {
    pub fn connect(host: &str, port: u16) -> Result<Scope, String> {
        let addr = (host, port)
            .to_socket_addrs()
            .map_err(|e| format!("{host}:{port}: {e}"))?
            .next()
            .ok_or("no address")?;
        let s = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
            .map_err(|e| format!("connect {host}:{port}: {e}"))?;
        s.set_read_timeout(Some(Duration::from_secs(10))).ok();
        s.set_nodelay(true).ok();
        Ok(Scope { s })
    }

    pub fn set_timeout(&mut self, secs: u64) {
        self.s
            .set_read_timeout(Some(Duration::from_secs(secs)))
            .ok();
    }

    pub fn send(&mut self, cmd: &str) -> Result<(), String> {
        self.s
            .write_all(format!("{cmd}\n").as_bytes())
            .map_err(|e| format!("send: {e}"))
    }

    fn byte(&mut self) -> Result<u8, String> {
        let mut b = [0u8; 1];
        self.s
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
        let v = Scope::value(&r);
        let v = v.trim_end_matches(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E');
        v.parse::<f64>()
            .map_err(|_| format!("{cmd}: not a number: {r:?}"))
    }

    /// A `#9nnnnnnnnn` block, skipping any text before the `#`. Returns the payload.
    pub fn block(&mut self, cmd: &str) -> Result<Vec<u8>, String> {
        self.send(cmd)?;
        loop {
            if self.byte()? == b'#' {
                break;
            }
        }
        let nd = (self.byte()? as char)
            .to_digit(10)
            .ok_or("bad block header")? as usize;
        let mut n = String::new();
        for _ in 0..nd {
            n.push(self.byte()? as char);
        }
        let n: usize = n.parse().map_err(|_| "bad block length")?;
        let mut data = vec![0u8; n];
        self.s
            .read_exact(&mut data)
            .map_err(|e| format!("block: {e}"))?;
        // Trailing terminators, if the scope sent them.
        self.s
            .set_read_timeout(Some(Duration::from_millis(200)))
            .ok();
        let mut junk = [0u8; 2];
        let _ = self.s.read(&mut junk);
        self.s.set_read_timeout(Some(Duration::from_secs(10))).ok();
        Ok(data)
    }

    /// Raw stream with no header (the `SCDP` screenshot): read until the scope goes idle.
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
        self.s
            .set_read_timeout(Some(Duration::from_millis(idle_ms)))
            .ok();
        while start.elapsed().as_secs() < overall_s {
            match self.s.read(&mut buf) {
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
        self.s.set_read_timeout(Some(Duration::from_secs(10))).ok();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::Scope;

    #[test]
    fn value_handles_both_header_settings() {
        assert_eq!(Scope::value("C1:VDIV 1.00E+00V"), "1.00E+00V");
        assert_eq!(Scope::value("1.00E+00"), "1.00E+00");
        assert_eq!(Scope::value("PKPK,4.040000E+00"), "4.040000E+00");
        assert_eq!(Scope::value("FREQ,****"), "****");
    }
}

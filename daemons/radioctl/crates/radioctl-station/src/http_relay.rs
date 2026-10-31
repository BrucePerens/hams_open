// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shelly Gen2 and Tasmota relays as channel drivers, re-expressing the semantics of the relay's
//! `antenna_drivers/http_relay.rs` (written there from the makers' public documents):
//! - Shelly Gen2+ Switch component: `GET /rpc/Switch.Set?id=N&on=true|false` and
//!   `GET /rpc/Switch.GetStatus?id=N` (the reply has `"output":bool`); ids are 0-based.
//! - Tasmota: `GET /cm?cmnd=Power<x>%20On|Off` and `GET /cm?cmnd=Power<x>` (reply `POWER<x>` or
//!   `POWER` for relay 1); x is 1-based.
//!
//! The address must be an IP literal in a private, link-local or loopback range: never the public
//! internet and never a name that could resolve elsewhere. A device with a password answers 401
//! and is reported; authentication is not supported. A reply that is not understood is an error.

use crate::channel::{BoxFuture, ChannelDriver};
use crate::error::{StationError, StationErrorKind};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const IO_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_REPLY: u64 = 16 * 1024;

/// The device family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Shelly Gen2 RPC.
    Shelly,
    /// Tasmota HTTP commands.
    Tasmota,
}

/// A bank of relays on one device; line n is relay n (0-based).
pub struct HttpRelay {
    family: Family,
    addr: SocketAddr,
    relays: u32,
}

fn driver_err(m: impl Into<String>) -> StationError {
    StationError::new(StationErrorKind::Driver, m)
}

// [@ANCHOR: radioctl_station_http_is_private]
/// Whether an address is loopback, private or link-local (IPv4 or IPv6, including IPv4-mapped).
pub fn is_private_or_loopback(ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    };
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback() || first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
        }
    }
}

// [@ANCHOR: radioctl_station_http_parse_address]
/// The address the owner typed: an IP literal, optionally `:port` (default 80), private only.
pub fn parse_private_address(text: &str) -> Result<SocketAddr, StationError> {
    let text = text.trim();
    let addr = if let Ok(sa) = text.parse::<SocketAddr>() {
        sa
    } else if let Ok(ip) = text.parse::<IpAddr>() {
        SocketAddr::new(ip, 80)
    } else {
        return Err(StationError::new(
            StationErrorKind::Config,
            "the device address must be an IP address on your own network, such as 192.168.1.50",
        ));
    };
    if !is_private_or_loopback(addr.ip()) {
        return Err(StationError::new(
            StationErrorKind::Config,
            "a network relay must be on a private network address, never the public internet",
        ));
    }
    Ok(addr)
}

impl HttpRelay {
    // [@ANCHOR: radioctl_station_http_new]
    /// A driver for `relays` relays (1 to 32) at `address`. Nothing is sent.
    pub fn new(family: Family, address: &str, relays: u32) -> Result<HttpRelay, StationError> {
        if !(1..=32).contains(&relays) {
            return Err(StationError::new(
                StationErrorKind::Config,
                "a relay bank has 1 to 32 relays",
            ));
        }
        Ok(HttpRelay {
            family,
            addr: parse_private_address(address)?,
            relays,
        })
    }

    // [@ANCHOR: radioctl_station_http_get]
    /// One HTTP/1.0 GET; the body of a 200 reply.
    async fn get(&self, path_and_query: &str) -> Result<String, StationError> {
        let exchange = async {
            let mut s = TcpStream::connect(self.addr)
                .await
                .map_err(|e| driver_err(format!("cannot reach the relay: {e}")))?;
            let req = format!(
                "GET {path_and_query} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n",
                self.addr
            );
            s.write_all(req.as_bytes())
                .await
                .map_err(|e| driver_err(format!("cannot send to the relay: {e}")))?;
            let mut buf = Vec::new();
            s.take(MAX_REPLY)
                .read_to_end(&mut buf)
                .await
                .map_err(|e| driver_err(format!("no answer from the relay: {e}")))?;
            Ok::<_, StationError>(buf)
        };
        let buf = timeout(IO_TIMEOUT, exchange).await.map_err(|_| {
            StationError::new(StationErrorKind::Timeout, "the relay did not answer")
        })??;
        let text = String::from_utf8_lossy(&buf).to_string();
        let (head, body) = text
            .split_once("\r\n\r\n")
            .ok_or_else(|| driver_err("the relay sent no HTTP answer"))?;
        let status = head.lines().next().unwrap_or("");
        if !status.contains(" 200") {
            return Err(driver_err(format!(
                "the relay answered \"{}\" (a device with a password is not supported)",
                status.trim()
            )));
        }
        Ok(body.to_string())
    }

    // [@ANCHOR: radioctl_station_http_check]
    /// Refuses a relay number past the bank.
    fn check(&self, line: u32) -> Result<(), StationError> {
        if line >= self.relays {
            return Err(StationError::new(
                StationErrorKind::Config,
                format!("this relay bank has relays 0 to {}", self.relays - 1),
            ));
        }
        Ok(())
    }
}

impl ChannelDriver for HttpRelay {
    // [@ANCHOR: radioctl_station_http_write_line]
    /// Switches one relay.
    fn write_line(&self, line: u32, on: bool) -> BoxFuture<'_, Result<(), StationError>> {
        Box::pin(async move {
            self.check(line)?;
            let path = match self.family {
                Family::Shelly => format!("/rpc/Switch.Set?id={line}&on={on}"),
                Family::Tasmota => format!(
                    "/cm?cmnd=Power{}%20{}",
                    line + 1,
                    if on { "On" } else { "Off" }
                ),
            };
            self.get(&path).await.map(|_| ())
        })
    }

    // [@ANCHOR: radioctl_station_http_read_line]
    /// Reads one relay's state from the device.
    fn read_line(&self, line: u32) -> BoxFuture<'_, Result<bool, StationError>> {
        Box::pin(async move {
            self.check(line)?;
            let not_json = || driver_err("the relay's status was not JSON");
            match self.family {
                Family::Shelly => {
                    let body = self
                        .get(&format!("/rpc/Switch.GetStatus?id={line}"))
                        .await?;
                    let v: serde_json::Value =
                        serde_json::from_str(&body).map_err(|_| not_json())?;
                    v.get("output")
                        .and_then(|o| o.as_bool())
                        .ok_or_else(|| driver_err("the relay's status had no output"))
                }
                Family::Tasmota => {
                    let n = line + 1;
                    let body = self.get(&format!("/cm?cmnd=Power{n}")).await?;
                    let v: serde_json::Value =
                        serde_json::from_str(&body).map_err(|_| not_json())?;
                    let state = v
                        .get(format!("POWER{n}"))
                        .or_else(|| v.get("POWER"))
                        .and_then(|s| s.as_str())
                        .ok_or_else(|| driver_err("the relay's status had no power state"))?;
                    Ok(state.eq_ignore_ascii_case("ON"))
                }
            }
        })
    }

    // [@ANCHOR: radioctl_station_http_can_read_back]
    /// Both families report their relay state.
    fn can_read_back(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    /// A fake device on loopback: keeps relay states and the paths requested.
    async fn fake(
        tasmota: bool,
        relays: usize,
        status: &'static str,
    ) -> (SocketAddr, Arc<Mutex<(Vec<bool>, Vec<String>)>>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let st = Arc::new(Mutex::new((vec![false; relays], Vec::new())));
        let st2 = Arc::clone(&st);
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let mut buf = vec![0u8; 2048];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                let body = {
                    let mut g = st2.lock().unwrap();
                    g.1.push(path.clone());
                    if tasmota {
                        let cmd = path.split("cmnd=").nth(1).unwrap_or("").replace("%20", " ");
                        let mut parts = cmd.split(' ');
                        let name = parts.next().unwrap_or("");
                        let n: usize = name.trim_start_matches("Power").parse().unwrap_or(1);
                        match parts.next() {
                            Some("On") => g.0[n - 1] = true,
                            Some("Off") => g.0[n - 1] = false,
                            _ => {}
                        }
                        format!(
                            "{{\"POWER{n}\":\"{}\"}}",
                            if g.0[n - 1] { "ON" } else { "OFF" }
                        )
                    } else {
                        let id: usize = path
                            .split("id=")
                            .nth(1)
                            .and_then(|r| r.split('&').next())
                            .and_then(|r| r.parse().ok())
                            .unwrap_or(0);
                        if path.starts_with("/rpc/Switch.Set") {
                            let was = g.0[id];
                            g.0[id] = path.contains("on=true");
                            format!("{{\"was_on\":{was}}}")
                        } else {
                            format!("{{\"id\":{id},\"output\":{}}}", g.0[id])
                        }
                    }
                };
                let reply = format!(
                    "HTTP/1.0 {status}\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(reply.as_bytes()).await;
            }
        });
        (addr, st)
    }

    // Tests [@ANCHOR: radioctl_station_http_is_private]
    // Tests [@ANCHOR: radioctl_station_http_parse_address]
    // Tests [@ANCHOR: radioctl_station_http_new]
    #[test]
    fn only_private_ip_literals_are_accepted() {
        for ok in [
            "192.168.1.50",
            "10.0.0.2:8080",
            "127.0.0.1",
            "169.254.1.1",
            "[::1]:80",
            "[fd00::5]:80",
            "::ffff:192.168.1.5",
        ] {
            assert!(parse_private_address(ok).is_ok(), "{ok}");
        }
        for bad in [
            "8.8.8.8",
            "shelly.local",
            "example.com",
            "",
            "[2001:db8::1]:80",
            "::ffff:8.8.8.8",
            "172.32.0.1",
        ] {
            assert!(parse_private_address(bad).is_err(), "{bad}");
        }
        assert_eq!(parse_private_address("10.0.0.2").unwrap().port(), 80);
        assert!(HttpRelay::new(Family::Shelly, "10.0.0.2", 0).is_err());
        assert!(HttpRelay::new(Family::Shelly, "8.8.8.8", 1).is_err());
    }

    // Tests [@ANCHOR: radioctl_station_http_write_line]
    // Tests [@ANCHOR: radioctl_station_http_read_line]
    // Tests [@ANCHOR: radioctl_station_http_can_read_back]
    // Tests [@ANCHOR: radioctl_station_http_get]
    // Tests [@ANCHOR: radioctl_station_http_check]
    #[tokio::test]
    async fn shelly_and_tasmota_write_and_read_back() {
        for tasmota in [false, true] {
            let (addr, st) = fake(tasmota, 2, "200 OK").await;
            let relay = HttpRelay::new(
                if tasmota {
                    Family::Tasmota
                } else {
                    Family::Shelly
                },
                &addr.to_string(),
                2,
            )
            .unwrap();
            assert!(relay.can_read_back());
            relay.write_line(1, true).await.unwrap();
            assert!(relay.read_line(1).await.unwrap());
            assert!(!relay.read_line(0).await.unwrap());
            relay.write_line(1, false).await.unwrap();
            assert!(!relay.read_line(1).await.unwrap());
            assert!(relay.write_line(2, true).await.is_err());
            let paths = st.lock().unwrap().1.clone();
            let want = if tasmota {
                "/cm?cmnd=Power2%20On"
            } else {
                "/rpc/Switch.Set?id=1&on=true"
            };
            assert_eq!(paths[0], want);
        }
    }

    #[tokio::test]
    async fn a_selector_over_http_is_break_before_make_with_read_back() {
        use crate::channel::Switchboard;
        use crate::model::{ChannelDef, ChannelId, ChannelKind, Value};
        let (addr, st) = fake(false, 3, "200 OK").await;
        let relay = Arc::new(HttpRelay::new(Family::Shelly, &addr.to_string(), 3).unwrap());
        let mut b = Switchboard::new();
        let def = ChannelDef {
            id: ChannelId("ant".into()),
            kind: ChannelKind::Selector {
                ways: 3,
                settle_ms: 1,
            },
            driver: "shelly".into(),
            line: 0,
            safe: None,
            description: String::new(),
        };
        b.add_channel(def, relay).unwrap();
        let id = ChannelId("ant".into());
        b.set(&id, Value::Port(1)).await.unwrap();
        let before = st.lock().unwrap().1.len();
        let out = b.set(&id, Value::Port(3)).await.unwrap();
        assert_eq!(out.confirmed, Some(Value::Port(3)));
        let paths = st.lock().unwrap().1[before..].to_vec();
        let off1 = paths
            .iter()
            .position(|p| p == "/rpc/Switch.Set?id=0&on=false")
            .unwrap();
        let on3 = paths
            .iter()
            .position(|p| p == "/rpc/Switch.Set?id=2&on=true")
            .unwrap();
        assert!(off1 < on3);
    }

    #[tokio::test]
    async fn an_unreachable_password_protected_or_odd_device_is_a_clean_error() {
        let relay = HttpRelay::new(Family::Shelly, "127.0.0.1:1", 1).unwrap();
        assert!(relay.write_line(0, true).await.is_err());
        let (addr, _) = fake(false, 1, "401 Unauthorized").await;
        let relay = HttpRelay::new(Family::Shelly, &addr.to_string(), 1).unwrap();
        let e = relay.read_line(0).await.unwrap_err();
        assert!(e.detail.contains("401"), "{e}");
        // a 200 that is not JSON, or has no output, is not believed
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let mut b = [0u8; 512];
                let _ = s.read(&mut b).await;
                let _ = s.write_all(b"HTTP/1.0 200 OK\r\n\r\n<html>hi</html>").await;
            }
        });
        let relay = HttpRelay::new(Family::Tasmota, &addr.to_string(), 1).unwrap();
        assert!(relay.read_line(0).await.is_err());
    }
}

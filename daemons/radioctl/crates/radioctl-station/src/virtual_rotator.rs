// SPDX-License-Identifier: AGPL-3.0-or-later
//! A virtual rotator: the far end of the rotctld and GS-232 protocols with a slewing model, so the
//! engines, the command handle and the transports can be tested without a rotator. It speaks the
//! protocol a real controller or a `rotctld` speaks, moves toward its target at the description's
//! slew rate, refuses targets outside its travel, and can be made stuck (accepts commands, never
//! moves), silent (never answers) or slow.
//!
//! [`VirtualRotator`] is sans-IO (time is passed in); [`serve_virtual_rotator`] serves one on a
//! TCP listener.

use crate::rotator::{RotatorDescription, RotatorProtocol};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::time::Instant;

/// What the virtual rotator is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotatorState {
    /// Where it is now, degrees.
    pub az: f32,
    /// Where it is now, degrees.
    pub el: f32,
    /// Where it is going.
    pub target_az: f32,
    /// Where it is going.
    pub target_el: f32,
}

/// The sans-IO virtual rotator.
pub struct VirtualRotator {
    desc: RotatorDescription,
    state: RotatorState,
    last: Duration,
    /// Accepts commands and never moves.
    pub stuck: bool,
    /// Never answers.
    pub silent: bool,
    received: Vec<String>,
    buffer: Vec<u8>,
}

// [@ANCHOR: radioctl_station_vrot_step]
/// Moves `from` toward `to` by at most `amount`.
fn step(from: f32, to: f32, amount: f32) -> f32 {
    if (to - from).abs() <= amount {
        to
    } else if to > from {
        from + amount
    } else {
        from - amount
    }
}

impl VirtualRotator {
    // [@ANCHOR: radioctl_station_vrot_new]
    /// A rotator at its minimum travel, at rest.
    pub fn new(desc: RotatorDescription) -> VirtualRotator {
        let state = RotatorState {
            az: desc.az_min,
            el: desc.el_min,
            target_az: desc.az_min,
            target_el: desc.el_min,
        };
        VirtualRotator {
            desc,
            state,
            last: Duration::ZERO,
            stuck: false,
            silent: false,
            received: Vec::new(),
            buffer: Vec::new(),
        }
    }

    // [@ANCHOR: radioctl_station_vrot_advance]
    /// Lets time pass: moves toward the target at the slew rate unless stuck.
    pub fn advance(&mut self, now: Duration) {
        let dt = now.saturating_sub(self.last).as_secs_f32();
        self.last = now.max(self.last);
        if self.stuck {
            return;
        }
        let amount = dt * self.desc.slew_deg_per_s;
        self.state.az = step(self.state.az, self.state.target_az, amount);
        self.state.el = step(self.state.el, self.state.target_el, amount);
    }

    // [@ANCHOR: radioctl_station_vrot_state]
    /// The state now.
    pub fn state(&mut self, now: Duration) -> RotatorState {
        self.advance(now);
        self.state
    }

    // [@ANCHOR: radioctl_station_vrot_received]
    /// Every command line received, in order.
    pub fn received(&self) -> &[String] {
        &self.received
    }

    // [@ANCHOR: radioctl_station_vrot_set_target]
    /// Accepts a target if it is inside the travel.
    fn set_target(&mut self, az: f32, el: f32) -> bool {
        let el = if self.desc.has_elevation { el } else { 0.0 };
        if !self.desc.contains(az, el) {
            return false;
        }
        self.state.target_az = az;
        self.state.target_el = el;
        true
    }

    // [@ANCHOR: radioctl_station_vrot_feed]
    /// Bytes from the client; returns the reply bytes (none when silent).
    pub fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.advance(now);
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > 4096 {
            self.buffer.clear();
        }
        let mut out = Vec::new();
        let end = if self.desc.protocol == RotatorProtocol::Rotctld {
            b'\n'
        } else {
            b'\r'
        };
        while let Some(at) = self.buffer.iter().position(|b| *b == end) {
            let raw: Vec<u8> = self.buffer.drain(..=at).collect();
            let text = String::from_utf8_lossy(&raw).trim().to_string();
            if text.is_empty() {
                continue;
            }
            self.received.push(text.clone());
            let reply = match self.desc.protocol {
                RotatorProtocol::Rotctld => self.rotctld(&text),
                RotatorProtocol::Gs232 => self.gs232(&text),
            };
            if !self.silent {
                out.extend_from_slice(&reply);
            }
        }
        out
    }

    // [@ANCHOR: radioctl_station_vrot_rotctld]
    /// One rotctld command line.
    fn rotctld(&mut self, text: &str) -> Vec<u8> {
        let mut words = text.split_whitespace();
        match words.next() {
            Some("P") => {
                let az = words.next().and_then(|w| w.parse::<f32>().ok());
                let el = words.next().and_then(|w| w.parse::<f32>().ok());
                match (az, el) {
                    (Some(az), Some(el)) if self.set_target(az, el) => b"RPRT 0\n".to_vec(),
                    (Some(_), Some(_)) => b"RPRT -1\n".to_vec(),
                    _ => b"RPRT -8\n".to_vec(),
                }
            }
            Some("p") => format!("{:.6}\n{:.6}\n", self.state.az, self.state.el).into_bytes(),
            Some("S") => {
                self.state.target_az = self.state.az;
                self.state.target_el = self.state.el;
                b"RPRT 0\n".to_vec()
            }
            Some("K") => {
                let park = self.desc.park;
                match park {
                    Some(p) if self.set_target(p.az, p.el) => b"RPRT 0\n".to_vec(),
                    _ => b"RPRT -11\n".to_vec(),
                }
            }
            _ => b"RPRT -4\n".to_vec(),
        }
    }

    // [@ANCHOR: radioctl_station_vrot_gs232]
    /// One GS-232 command (CR-terminated, already trimmed): `Waaa eee`, `C2`, `S`. A command it
    /// does not know gets `?>`, as the real controller answers.
    fn gs232(&mut self, text: &str) -> Vec<u8> {
        if let Some(args) = text.strip_prefix('W') {
            let mut words = args.split_whitespace();
            let az = words.next().and_then(|w| w.parse::<f32>().ok());
            let el = words.next().and_then(|w| w.parse::<f32>().ok());
            return match (az, el) {
                (Some(az), Some(el)) if self.set_target(az, el) => Vec::new(),
                _ => b"?>\r\n".to_vec(),
            };
        }
        match text {
            "C2" => format!(
                "+0{:03}+0{:03}\r\n",
                self.state.az.round() as i32,
                self.state.el.round() as i32
            )
            .into_bytes(),
            "S" => {
                self.state.target_az = self.state.az;
                self.state.target_el = self.state.el;
                Vec::new()
            }
            _ => b"?>\r\n".to_vec(),
        }
    }
}

// [@ANCHOR: radioctl_station_vrot_lock]
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Control of a served virtual rotator.
#[derive(Clone)]
pub struct VirtualRotatorControl {
    rotator: Arc<Mutex<VirtualRotator>>,
    epoch: Instant,
}

impl VirtualRotatorControl {
    // [@ANCHOR: radioctl_station_vrot_control_state]
    /// Where the rotator is and is going.
    pub fn state(&self) -> RotatorState {
        lock(&self.rotator).state(self.epoch.elapsed())
    }

    // [@ANCHOR: radioctl_station_vrot_control_received]
    /// Every command line received.
    pub fn received(&self) -> Vec<String> {
        lock(&self.rotator).received().to_vec()
    }

    // [@ANCHOR: radioctl_station_vrot_control_set_stuck]
    /// Makes the rotator stuck (or frees it).
    pub fn set_stuck(&self, stuck: bool) {
        let mut g = lock(&self.rotator);
        g.advance(self.epoch.elapsed());
        g.stuck = stuck;
    }

    // [@ANCHOR: radioctl_station_vrot_control_set_silent]
    /// Makes the rotator silent (or lets it answer).
    pub fn set_silent(&self, silent: bool) {
        lock(&self.rotator).silent = silent;
    }
}

// [@ANCHOR: radioctl_station_vrot_serve]
/// Serves `rotator` on `listener` from a task on `runtime`; one client at a time, a new
/// connection replaces the old one.
pub fn serve_virtual_rotator(
    runtime: &Handle,
    listener: TcpListener,
    rotator: VirtualRotator,
) -> VirtualRotatorControl {
    let control = VirtualRotatorControl {
        rotator: Arc::new(Mutex::new(rotator)),
        epoch: Instant::now(),
    };
    let shared = control.clone();
    runtime.spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let _ = stream.set_nodelay(true);
            let mut buffer = [0u8; 256];
            while let Ok(n) = stream.read(&mut buffer).await {
                if n == 0 {
                    break;
                }
                let reply = lock(&shared.rotator)
                    .feed(buffer.get(..n).unwrap_or(&[]), shared.epoch.elapsed());
                if !reply.is_empty() && stream.write_all(&reply).await.is_err() {
                    break;
                }
            }
        }
    });
    control
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rot(id: &str) -> VirtualRotator {
        VirtualRotator::new(
            RotatorDescription::builtin()
                .into_iter()
                .find(|d| d.id == id)
                .unwrap(),
        )
    }

    // Tests [@ANCHOR: radioctl_station_vrot_new]
    // Tests [@ANCHOR: radioctl_station_vrot_advance]
    // Tests [@ANCHOR: radioctl_station_vrot_step]
    // Tests [@ANCHOR: radioctl_station_vrot_state]
    // Tests [@ANCHOR: radioctl_station_vrot_set_target]
    // Tests [@ANCHOR: radioctl_station_vrot_feed]
    // Tests [@ANCHOR: radioctl_station_vrot_received]
    // Tests [@ANCHOR: radioctl_station_vrot_rotctld]
    #[test]
    fn it_slews_at_the_description_rate_and_refuses_what_is_out_of_travel() {
        let mut r = rot("generic-rotctld"); // 3 degrees per second
        let t = Duration::from_secs;
        assert_eq!(r.feed(b"P 90 30\n", t(0)), b"RPRT 0\n");
        assert_eq!(r.state(t(10)).az, 30.0);
        assert_eq!(r.state(t(10)).el, 30.0);
        assert_eq!(r.state(t(30)).az, 90.0);
        assert_eq!(r.feed(b"p\n", t(30)), b"90.000000\n30.000000\n");
        assert_eq!(r.feed(b"P 400 0\n", t(30)), b"RPRT -1\n");
        assert_eq!(r.feed(b"P x y\n", t(30)), b"RPRT -8\n");
        assert_eq!(r.feed(b"Z\n", t(30)), b"RPRT -4\n");
        assert_eq!(r.feed(b"K\n", t(30)), b"RPRT -11\n"); // no park position
        assert_eq!(r.feed(b"P 10 0\nS\n", t(30)), b"RPRT 0\nRPRT 0\n");
        assert_eq!(r.state(t(60)).az, 90.0, "stop froze it where it was");
        assert_eq!(r.received().len(), 8);
        // Time never runs backwards.
        assert_eq!(r.state(t(1)).az, 90.0);
    }

    // Tests [@ANCHOR: radioctl_station_vrot_gs232]
    #[test]
    fn it_speaks_gs232() {
        let mut r = rot("yaesu-g-5500-gs232b");
        let t = Duration::from_secs;
        assert!(r.feed(b"W090 045\r", t(0)).is_empty());
        assert_eq!(r.feed(b"C2\r", t(0)), b"+0000+0000\r\n");
        assert_eq!(r.feed(b"C2\r", t(10)), b"+0020+0020\r\n");
        assert_eq!(r.feed(b"W500 000\r", t(10)), b"?>\r\n");
        assert_eq!(r.feed(b"Q\r", t(10)), b"?>\r\n");
        assert!(r.feed(b"S\r", t(10)).is_empty());
        assert_eq!(r.feed(b"C2\r", t(60)), b"+0020+0020\r\n");
        // Split across reads
        assert!(r.feed(b"C", t(60)).is_empty());
        assert_eq!(r.feed(b"2\r", t(60)), b"+0020+0020\r\n");
        // Stuck and silent
        r.stuck = true;
        r.feed(b"W100 000\r", t(60));
        assert_eq!(r.state(t(120)).az, 20.0);
        r.silent = true;
        assert!(r.feed(b"C2\r", t(120)).is_empty());
    }
}

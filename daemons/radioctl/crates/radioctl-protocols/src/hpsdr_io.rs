// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bridge between the `hpsdr-network` engine's line protocol and an `hpsdr` crate session
//! (Protocol 1 [`Radio`] or Protocol 2 [`Radio2`]), behind an in-memory duplex stream the rig actor
//! uses as its transport. It also builds the virtual ANAN: a session against the loopback Protocol 1 or
//! Protocol 2 emulator, for tests.
//!
//! Transmit: the bridge owns an `hpsdr` [`TxGate`] that is closed. It opens it, for [`HpsdrOptions::ptt_lease`]
//! only, when a `P 1` line arrives (which the rig actor sends only with its own transmit gate open), closes
//! it on `P 0`, and unkeys by itself when the lease runs out. The drive level is
//! [`HpsdrOptions::drive`], zero by default. Everything is `verified=unverified`.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hpsdr::gate::TxGate;
use hpsdr::p1::P1Config;
use hpsdr::session::{Radio, SessionOptions};
use hpsdr::session_p2::{P2Config, Radio2};
use hpsdr::types::HardwareStatus;
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::runtime::Handle;

/// A running session of either protocol.
#[derive(Debug)]
pub enum HpsdrLink {
    /// A Protocol 1 session.
    P1(Radio),
    /// A Protocol 2 session.
    P2(Radio2),
}

/// Bridge settings.
#[derive(Debug, Clone, Copy)]
pub struct HpsdrOptions {
    /// How long one `P 1` keys the radio before the bridge unkeys by itself.
    pub ptt_lease: Duration,
    /// Drive level 0..=255 sent while keyed.
    pub drive: u8,
    /// Interval of the unsolicited power and SWR lines.
    pub status_every: Duration,
    /// Power constant 2 for SWR (0.095 ANAN-100, 0.09 ANAN-10).
    pub power_constant: f64,
}

impl Default for HpsdrOptions {
    // [@ANCHOR: radioctl_hpsdr_options_default]
    /// A 3 second lease, drive zero, status every 250 ms, the ANAN-100 constant.
    fn default() -> Self {
        Self {
            ptt_lease: Duration::from_secs(3),
            drive: 0,
            status_every: Duration::from_millis(250),
            power_constant: 0.095,
        }
    }
}

struct Shared {
    link: Mutex<Option<HpsdrLink>>,
    gate: TxGate,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Handle on the served bridge: the attenuator (not an `Op` in the core vocabulary yet) and the status.
#[derive(Clone)]
pub struct HpsdrControl {
    shared: Arc<Shared>,
}

impl HpsdrControl {
    // [@ANCHOR: radioctl_hpsdr_control_set_attenuator]
    /// Sets the step attenuator in dB (0..=31).
    pub fn set_attenuator(&self, db: u8) -> Result<(), hpsdr::error::HpsdrError> {
        match lock(&self.shared.link).as_ref() {
            Some(HpsdrLink::P1(r)) => r.update(|c| {
                c.att_db = db;
                c.att_enable = true;
            }),
            Some(HpsdrLink::P2(r)) => r.update(|c| c.att_adc0 = db),
            None => Err(hpsdr::error::HpsdrError::Stopped),
        }
    }

    // [@ANCHOR: radioctl_hpsdr_control_status]
    /// The latest hardware status of the radio.
    pub fn status(&self) -> HardwareStatus {
        match lock(&self.shared.link).as_ref() {
            Some(HpsdrLink::P1(r)) => r.status(),
            Some(HpsdrLink::P2(r)) => r.status(),
            None => HardwareStatus::default(),
        }
    }

    // [@ANCHOR: radioctl_hpsdr_control_take_streams]
    /// Hands over the receive I/Q streams of the session (the data plane), once.
    pub fn take_streams(&self) -> Vec<hpsdr::iq::ChannelStream> {
        match lock(&self.shared.link).as_mut() {
            Some(HpsdrLink::P1(r)) => r.take_streams(),
            Some(HpsdrLink::P2(r)) => r.take_streams(),
            None => Vec::new(),
        }
    }
}

// [@ANCHOR: radioctl_hpsdr_set_frequency]
/// Retunes DDC0 (and the transmit frequency, which follows it).
fn set_frequency(link: &HpsdrLink, hz: u32) -> Result<(), hpsdr::error::HpsdrError> {
    match link {
        HpsdrLink::P1(r) => r.update(|c| {
            c.rx_hz[0] = hz;
            c.tx_hz = hz;
        }),
        HpsdrLink::P2(r) => r.update(|c| {
            c.rx_hz[0] = hz;
            c.tx_hz = hz;
        }),
    }
}

// [@ANCHOR: radioctl_hpsdr_request_ptt]
/// Keys or unkeys through the `hpsdr` session, which re-checks its gate before every packet.
fn request_ptt(link: &HpsdrLink, on: bool, drive: u8) -> Result<(), hpsdr::error::HpsdrError> {
    match link {
        HpsdrLink::P1(r) => r.request_ptt(on, drive),
        HpsdrLink::P2(r) => r.request_ptt(on, drive),
    }
}

/// Mutable state of the bridge task.
struct State {
    freq: u32,
    ptt: bool,
}

// [@ANCHOR: radioctl_hpsdr_handle_line]
/// Answers one request line.
fn handle_line(line: &str, shared: &Shared, st: &mut State, opts: &HpsdrOptions) -> String {
    let guard = lock(&shared.link);
    let Some(link) = guard.as_ref() else {
        return "ERR stopped\n".into();
    };
    let mut parts = line.split_whitespace();
    match (
        parts.next(),
        parts.next().and_then(|v| v.parse::<u64>().ok()),
    ) {
        (Some("GF"), _) => format!("F {}\n", st.freq),
        (Some("GP"), _) => format!("P {}\n", u8::from(st.ptt)),
        (Some("GW"), _) => format!("W {}\n", status_of(link).forward_raw),
        (Some("GR"), _) => format!("R {}\n", swr_tenths(&status_of(link), opts)),
        (Some("F"), Some(hz)) => match u32::try_from(hz).map(|h| set_frequency(link, h)) {
            Ok(Ok(())) => {
                st.freq = hz as u32;
                "OK\n".into()
            }
            _ => "ERR frequency\n".into(),
        },
        (Some("P"), Some(1)) => {
            shared.gate.open_until(Instant::now() + opts.ptt_lease);
            match request_ptt(link, true, opts.drive) {
                Ok(()) => {
                    st.ptt = true;
                    "OK\n".into()
                }
                Err(e) => {
                    shared.gate.close();
                    format!("ERR {e}\n")
                }
            }
        }
        (Some("P"), Some(0)) => {
            let r = request_ptt(link, false, 0);
            shared.gate.close();
            st.ptt = false;
            if r.is_ok() {
                "OK\n".into()
            } else {
                "ERR unkey\n".into()
            }
        }
        _ => "ERR command\n".into(),
    }
}

fn status_of(link: &HpsdrLink) -> HardwareStatus {
    match link {
        HpsdrLink::P1(r) => r.status(),
        HpsdrLink::P2(r) => r.status(),
    }
}

// [@ANCHOR: radioctl_hpsdr_swr_tenths]
/// SWR in tenths from forward and reverse counts; 10 (1.0:1) with no forward power.
fn swr_tenths(st: &HardwareStatus, opts: &HpsdrOptions) -> u16 {
    st.swr(opts.power_constant)
        .map_or(10, |s| (s * 10.0).round().clamp(10.0, 999.0) as u16)
}

// [@ANCHOR: radioctl_hpsdr_serve]
/// Serves `link` behind an in-memory stream for the rig actor, and returns the control handle. The task
/// unkeys and closes the gate if the actor goes away.
pub fn serve_hpsdr(
    runtime: &Handle,
    link: HpsdrLink,
    gate: TxGate,
    freq: u32,
    opts: HpsdrOptions,
) -> (DuplexStream, HpsdrControl) {
    let (near, mut far) = duplex(4096);
    let shared = Arc::new(Shared {
        link: Mutex::new(Some(link)),
        gate,
    });
    let task = Arc::clone(&shared);
    runtime.spawn(async move {
        let mut st = State { freq, ptt: false };
        let (mut buf, mut line) = ([0u8; 256], Vec::<u8>::new());
        let mut tick = tokio::time::interval(opts.status_every);
        loop {
            tokio::select! {
                read = far.read(&mut buf) => {
                    let Ok(n) = read else { break };
                    if n == 0 { break; }
                    for &b in buf.get(..n).unwrap_or(&[]) {
                        if b == b'\n' {
                            let text = String::from_utf8_lossy(&line).into_owned();
                            line.clear();
                            let reply = handle_line(text.trim(), &task, &mut st, &opts);
                            if far.write_all(reply.as_bytes()).await.is_err() { break; }
                        } else if line.len() < 128 {
                            line.push(b);
                        }
                    }
                }
                _ = tick.tick() => {
                    let out = {
                        let g = lock(&task.link);
                        if st.ptt && !task.gate.is_open(Instant::now()) {
                            if let Some(l) = g.as_ref() { let _ = request_ptt(l, false, 0); }
                            st.ptt = false;
                        }
                        g.as_ref().map(|l| {
                            let s = status_of(l);
                            format!("W {}\nR {}\nP {}\n", s.forward_raw, swr_tenths(&s, &opts), u8::from(st.ptt))
                        })
                    };
                    if let Some(o) = out {
                        if far.write_all(o.as_bytes()).await.is_err() { break; }
                    }
                }
            }
        }
        // The actor is gone: unkey, close the gate, stop the session.
        task.gate.close();
        let link = lock(&task.link).take();
        match link {
            Some(HpsdrLink::P1(r)) => { let _ = r.stop().await; }
            Some(HpsdrLink::P2(r)) => { let _ = r.stop().await; }
            None => {}
        }
    });
    (near, HpsdrControl { shared })
}

// [@ANCHOR: radioctl_hpsdr_virtual_anan]
/// A virtual ANAN: a Protocol 1 or 2 session on a loopback emulator that the caller started. The gate
/// is a closed [`TxGate`]; the session and the bridge share it.
pub async fn start_session(
    runtime: &Handle,
    radio: &hpsdr::discovery::Discovered,
    protocol2_ports: Option<hpsdr::p2::RadioPorts>,
    freq: u32,
) -> Result<(HpsdrLink, TxGate), hpsdr::error::HpsdrError> {
    let gate = TxGate::new();
    let opts = SessionOptions::default();
    let link = match protocol2_ports {
        None => HpsdrLink::P1(
            Radio::start(
                runtime,
                radio,
                P1Config {
                    rx_hz: [freq; 7],
                    tx_hz: freq,
                    ..P1Config::default()
                },
                gate.clone(),
                opts,
            )
            .await?,
        ),
        Some(ports) => HpsdrLink::P2(
            Radio2::start(
                runtime,
                radio,
                P2Config {
                    rx_hz: vec![freq],
                    tx_hz: freq,
                    ports,
                    ..P2Config::default()
                },
                gate.clone(),
                opts,
            )
            .await?,
        ),
    };
    Ok((link, gate))
}

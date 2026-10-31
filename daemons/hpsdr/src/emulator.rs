// SPDX-License-Identifier: AGPL-3.0-or-later
//! An HPSDR emulator: a UDP server on the loopback address that speaks the Protocol 1 subset this
//! crate implements and produces synthetic I/Q tones. It lets the library's tests run with no radio.
//!
//! It binds `127.0.0.1` only (the constructor takes a port, never an address), and it never produces
//! RF: it only counts what a transmit request would have been (`mox_frames`, `drive_nonzero_frames`,
//! `tx_iq_packets`), which the tests assert stay zero while the transmit gate is closed.
//! Everything here is `verified=unverified`: it models the published documents, not a real radio.

use crate::error::HpsdrError;
use crate::p1::{self, P1Config, P1Reply};
use crate::types::{Board, HardwareStatus};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

/// A synthetic carrier at an absolute radio frequency.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tone {
    /// Absolute frequency in hertz.
    pub freq_hz: f64,
    /// Amplitude, 1.0 = ADC full scale.
    pub amplitude: f64,
}

/// What the emulated radio is and how it behaves.
#[derive(Debug, Clone)]
pub struct EmulatorConfig {
    /// Protocol 1 board identifier to report (1 = Hermes).
    pub board_id: u8,
    /// MAC address to report.
    pub mac: [u8; 6],
    /// Gateware code version to report.
    pub code_version: u8,
    /// Carriers present at the antenna.
    pub tones: Vec<Tone>,
    /// Status the radio reports (overload, power, supply).
    pub status: HardwareStatus,
    /// Time between EP6 datagrams (real time at 48 ksps one receiver is about 10.5 ms).
    pub pace: Duration,
    /// When non-zero, every Nth EP6 datagram is numbered but not sent (to exercise loss accounting).
    pub skip_every: u32,
}

impl Default for EmulatorConfig {
    // [@ANCHOR: hpsdr_emulator_config_default]
    /// A Hermes with one -6 dBFS tone at 7.1005 MHz, fast pacing.
    fn default() -> Self {
        Self {
            board_id: 1,
            mac: [0x02, 0x48, 0x50, 0x53, 0x44, 0x52],
            code_version: 73,
            tones: vec![Tone {
                freq_hz: 7_100_500.0,
                amplitude: 0.5,
            }],
            status: HardwareStatus {
                firmware: 73,
                supply_raw: 3500,
                ..Default::default()
            },
            pace: Duration::from_millis(2),
            skip_every: 0,
        }
    }
}

/// What the emulator has seen from the client.
#[derive(Debug, Default)]
pub struct EmulatorCounters {
    /// Discovery requests answered.
    pub discoveries: AtomicU64,
    /// Start commands with the I/Q bit set.
    pub starts: AtomicU64,
    /// Stop commands (or start commands with the I/Q bit clear).
    pub stops: AtomicU64,
    /// EP2 datagrams parsed.
    pub ep2_packets: AtomicU64,
    /// EP2 frames whose MOX bit was set.
    pub mox_frames: AtomicU64,
    /// Drive (address 9) frames with a non-zero level.
    pub drive_nonzero_frames: AtomicU64,
    /// EP2 datagrams carrying non-zero transmit I/Q.
    pub tx_iq_packets: AtomicU64,
    /// EP6 datagrams sent.
    pub ep6_sent: AtomicU64,
}

/// A running emulator.
#[derive(Debug)]
pub struct Emulator {
    addr: SocketAddr,
    counters: Arc<EmulatorCounters>,
    task: JoinHandle<()>,
}

impl Emulator {
    // [@ANCHOR: hpsdr_emulator_start_p1]
    /// Bind `127.0.0.1:port` (0 picks a free port) and serve Protocol 1 on `handle`.
    pub fn start_p1(handle: &Handle, port: u16, cfg: EmulatorConfig) -> Result<Self, HpsdrError> {
        let sock =
            crate::discovery::udp_socket(handle, SocketAddr::from((Ipv4Addr::LOCALHOST, port)))?;
        let addr = sock.local_addr()?;
        let counters = Arc::new(EmulatorCounters::default());
        let task = handle.spawn(run_p1(sock, cfg, counters.clone()));
        Ok(Self {
            addr,
            counters,
            task,
        })
    }

    // [@ANCHOR: hpsdr_emulator_addr]
    /// The loopback address it listens on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    // [@ANCHOR: hpsdr_emulator_counters]
    /// What it has seen so far.
    pub fn counters(&self) -> &EmulatorCounters {
        &self.counters
    }

    // [@ANCHOR: hpsdr_emulator_stop]
    /// Stop serving and release the socket.
    pub fn stop(self) {
        self.task.abort();
    }
}

impl EmulatorCounters {
    // [@ANCHOR: hpsdr_emulator_counters_get]
    /// Read one counter.
    pub fn get(c: &AtomicU64) -> u64 {
        c.load(Ordering::Relaxed)
    }
}

// [@ANCHOR: hpsdr_emulator_synth]
/// Samples of one receiver: `count` 24-bit (I, Q) pairs starting at sample index `start`, the sum of
/// every tone whose offset from `centre_hz` lies inside the Nyquist band.
pub fn synth(
    tones: &[Tone],
    centre_hz: u32,
    rate_hz: u32,
    start: u64,
    count: usize,
) -> Vec<(i32, i32)> {
    let fs = f64::from(rate_hz);
    (0..count)
        .map(|k| {
            let n = start + k as u64;
            let (mut i, mut q) = (0.0f64, 0.0f64);
            for t in tones {
                let f = t.freq_hz - f64::from(centre_hz);
                if f.abs() < fs / 2.0 {
                    let cycles = (f * (n as f64) / fs).fract();
                    let ph = std::f64::consts::TAU * cycles;
                    i += t.amplitude * ph.cos();
                    q += t.amplitude * ph.sin();
                }
            }
            let sc = |v: f64| (v * 8_388_607.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
            (sc(i), sc(q))
        })
        .collect()
}

// [@ANCHOR: hpsdr_emulator_handle_datagram]
/// Act on one datagram from a client; returns the reply to send to `from`, if any.
fn handle_datagram(
    d: &[u8],
    cfg: &EmulatorConfig,
    rf: &mut P1Config,
    dest: &mut Option<SocketAddr>,
    from: SocketAddr,
    c: &EmulatorCounters,
    seq: &mut u32,
) -> Option<Vec<u8>> {
    if d.len() >= 3 && d[..3] == [0xEF, 0xFE, 0x02] {
        c.discoveries.fetch_add(1, Ordering::Relaxed);
        let r = P1Reply {
            running: dest.is_some(),
            mac: cfg.mac,
            code_version: cfg.code_version,
            board_id: cfg.board_id,
            board: Board::from_p1_id(cfg.board_id),
        };
        return Some(p1::encode_reply(&r).to_vec());
    }
    if d.len() >= 4 && d[..3] == [0xEF, 0xFE, 0x04] {
        if d[3] & 1 == 1 {
            c.starts.fetch_add(1, Ordering::Relaxed);
            *dest = Some(from);
            *seq = 0;
        } else {
            c.stops.fetch_add(1, Ordering::Relaxed);
            *dest = None;
            *seq = 0;
        }
        return None;
    }
    if let Ok(ep2) = p1::parse_ep2(d) {
        c.ep2_packets.fetch_add(1, Ordering::Relaxed);
        if ep2.tx_iq_nonzero {
            c.tx_iq_packets.fetch_add(1, Ordering::Relaxed);
        }
        for cc in &ep2.cc {
            let (mox, drive) = rf.apply_cc(cc);
            if mox {
                c.mox_frames.fetch_add(1, Ordering::Relaxed);
            }
            if drive.is_some_and(|v| v > 0) {
                c.drive_nonzero_frames.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    None
}

// [@ANCHOR: hpsdr_emulator_next_ep6]
/// Build the next EP6 datagram from the current settings; `n` is the sample index of its first sample.
fn next_ep6(cfg: &EmulatorConfig, rf: &P1Config, seq: u32, n: u64, cursor: u8) -> Option<Vec<u8>> {
    let per = p1::samples_per_frame(rf.num_rx);
    let rx: Vec<Vec<(i32, i32)>> = (0..usize::from(rf.num_rx))
        .map(|r| synth(&cfg.tones, rf.rx_hz[r], rf.rate.hz(), n, 2 * per))
        .collect();
    let mic = vec![0i16; 2 * per];
    let cc = [
        p1::encode_status_cc(&cfg.status, cursor % 5),
        p1::encode_status_cc(&cfg.status, (cursor + 1) % 5),
    ];
    p1::encode_ep6(seq, &cc, &rx, &mic).ok()
}

// [@ANCHOR: hpsdr_emulator_run_p1]
/// The emulator task: answer datagrams and, after a Start, send EP6 datagrams at `cfg.pace`.
async fn run_p1(sock: UdpSocket, cfg: EmulatorConfig, c: Arc<EmulatorCounters>) {
    let mut rf = P1Config::default();
    let mut dest: Option<SocketAddr> = None;
    let (mut seq, mut n, mut cursor) = (0u32, 0u64, 0u8);
    let mut tick = tokio::time::interval(cfg.pace);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut buf = vec![0u8; 2048];
    loop {
        tokio::select! {
            r = sock.recv_from(&mut buf) => {
                let Ok((len, from)) = r else { continue };
                if let Some(reply) = handle_datagram(&buf[..len], &cfg, &mut rf, &mut dest, from, &c, &mut seq) {
                    let _ = sock.send_to(&reply, from).await;
                }
            }
            _ = tick.tick(), if dest.is_some() => {
                let Some(to) = dest else { continue };
                let skip = cfg.skip_every > 0 && (seq + 1) % cfg.skip_every == 0;
                if !skip {
                    if let Some(d) = next_ep6(&cfg, &rf, seq, n, cursor) {
                        if sock.send_to(&d, to).await.is_ok() {
                            c.ep6_sent.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                n += 2 * p1::samples_per_frame(rf.num_rx) as u64;
                seq = seq.wrapping_add(1);
                cursor = (cursor + 2) % 5;
            }
        }
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! A Protocol 2 emulator: UDP servers on the loopback address that speak the Protocol 2 subset this
//! crate implements ("openHPSDR Ethernet Protocol" v4.4) and produce synthetic I/Q tones. Like
//! [`crate::emulator::Emulator`] it binds `127.0.0.1` only (the constructor takes a command port, never an
//! address), never produces RF, and only counts what a transmit request would have been
//! (`ptt_frames`, `drive_nonzero_frames`, `tx_freq_nonzero_frames`, `duc_packets`), which the tests
//! assert stay zero while the transmit gate is closed.
//!
//! The radio side listens on the command port (discovery and General), the DDC-specific port, the High
//! Priority port and the DUC0 I/Q port, and sends the High Priority status from `ports.hp_to_pc` and DDC
//! `n` from `ports.ddc0 + n`, all to the host socket that sent the General packet. The port map comes
//! from [`P2EmulatorConfig::ports`]; [`loopback_ports`] finds a free block for tests.
//! Everything here is `verified=unverified`: it models the published document, not a real radio.

use crate::emulator::{synth, Tone};
use crate::error::HpsdrError;
use crate::p2::{self, P2Reply, RadioPorts};
use crate::types::{Board, HardwareStatus, Rate};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

/// DDC samples per packet: `(1444 - 16) / 6`.
pub const DDC_SAMPLES: usize = 238;
/// Protocol 2 board type reported by default (1 = Hermes).
pub const HERMES_P2_TYPE: u8 = 1;

/// What the emulated radio is and how it behaves.
#[derive(Debug, Clone)]
pub struct P2EmulatorConfig {
    /// Protocol 2 board type byte to report (1 = Hermes).
    pub board_type: u8,
    /// MAC address to report.
    pub mac: [u8; 6],
    /// Gateware code version to report.
    pub firmware: u8,
    /// DDCs the radio claims (and serves).
    pub num_ddcs: u8,
    /// Report and expect phase words rather than hertz.
    pub phase_word: bool,
    /// Carriers present at the antenna.
    pub tones: Vec<Tone>,
    /// Status the radio reports.
    pub status: HardwareStatus,
    /// Time between packets (real time at 48 ksps is about 5 ms for 238 samples).
    pub pace: Duration,
    /// When non-zero, every Nth DDC packet is numbered but not sent (to exercise loss accounting).
    pub skip_every: u32,
    /// The port map the emulator serves (see [`loopback_ports`]).
    pub ports: RadioPorts,
}

impl Default for P2EmulatorConfig {
    // [@ANCHOR: hpsdr_emulator_p2_config_default]
    /// A Hermes with one -6 dBFS tone at 7.1005 MHz, phase words, fast pacing, default ports.
    fn default() -> Self {
        Self {
            board_type: HERMES_P2_TYPE,
            mac: [0x02, 0x48, 0x50, 0x53, 0x44, 0x32],
            firmware: 39,
            num_ddcs: 8,
            phase_word: true,
            tones: vec![Tone {
                freq_hz: 7_100_500.0,
                amplitude: 0.5,
            }],
            status: HardwareStatus {
                firmware: 0,
                supply_raw: 3500,
                ..Default::default()
            },
            pace: Duration::from_millis(2),
            skip_every: 0,
            ports: RadioPorts::default(),
        }
    }
}

/// What the emulator has seen from the client.
#[derive(Debug, Default)]
pub struct P2Counters {
    /// Discovery requests answered.
    pub discoveries: AtomicU64,
    /// General packets received.
    pub generals: AtomicU64,
    /// DDC-specific packets received.
    pub ddc_specific: AtomicU64,
    /// High Priority packets received.
    pub hp_packets: AtomicU64,
    /// Transitions of the run bit from clear to set.
    pub run_starts: AtomicU64,
    /// Transitions of the run bit from set to clear.
    pub run_stops: AtomicU64,
    /// DDC packets sent.
    pub ddc_sent: AtomicU64,
    /// High Priority packets whose PTT bit was set.
    pub ptt_frames: AtomicU64,
    /// High Priority packets with a non-zero drive level.
    pub drive_nonzero_frames: AtomicU64,
    /// High Priority packets with a non-zero transmit frequency.
    pub tx_freq_nonzero_frames: AtomicU64,
    /// Packets received on the DUC0 I/Q port (any content).
    pub duc_packets: AtomicU64,
    /// 1 while the latest High Priority packet has PTT set.
    pub ptt_now: AtomicU64,
    /// Latest drive level.
    pub last_drive: AtomicU64,
    /// Latest ADC0 attenuator setting.
    pub last_atten_adc0: AtomicU64,
    /// Latest DDC0 rate in ksps.
    pub last_rate_ksps: AtomicU64,
}

impl P2Counters {
    // [@ANCHOR: hpsdr_emulator_p2_counters_get]
    /// Read one counter.
    pub fn get(c: &AtomicU64) -> u64 {
        c.load(Ordering::Relaxed)
    }
}

static NEXT_BLOCK: AtomicU16 = AtomicU16::new(0);

// [@ANCHOR: hpsdr_emulator_p2_loopback_ports]
/// A port map with every port free on `127.0.0.1` right now: a block of 40 consecutive ports is probed
/// and released, and successive calls in one process get different blocks. DDC `n` source ports are
/// `ddc0 + n`, so `ddcs` consecutive ports are reserved for them.
pub fn loopback_ports(ddcs: u8) -> Result<RadioPorts, HpsdrError> {
    for _ in 0..200 {
        let k = NEXT_BLOCK.fetch_add(1, Ordering::Relaxed);
        let base = 30_000 + (k % 600) * 40;
        let ok =
            (0..30u16).all(|i| std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, base + i)).is_ok());
        if ok && u16::from(ddcs) <= 16 {
            return Ok(RadioPorts {
                ddc_specific: base,
                duc_specific: base + 1,
                hp_from_pc: base + 2,
                hp_to_pc: base + 3,
                ddc_audio: base + 4,
                duc0_iq: base + 5,
                ddc0: base + 8,
                mic: base + 24,
                wideband0: base + 25,
            });
        }
    }
    Err(HpsdrError::Io("no free loopback port block".into()))
}

/// A running Protocol 2 emulator.
#[derive(Debug)]
pub struct EmulatorP2 {
    addr: SocketAddr,
    counters: Arc<P2Counters>,
    task: JoinHandle<()>,
}

fn bind(handle: &Handle, port: u16) -> Result<UdpSocket, HpsdrError> {
    crate::discovery::udp_socket(handle, SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
}

impl EmulatorP2 {
    // [@ANCHOR: hpsdr_emulator_p2_start]
    /// Bind `127.0.0.1:port` (0 picks a free port) for discovery and General, plus the ports of
    /// `cfg.ports`, and serve Protocol 2 on `handle`.
    pub fn start(handle: &Handle, port: u16, cfg: P2EmulatorConfig) -> Result<Self, HpsdrError> {
        let cmd = bind(handle, port)?;
        let addr = cmd.local_addr()?;
        let ddc_spec = Arc::new(bind(handle, cfg.ports.ddc_specific)?);
        let hp_in = bind(handle, cfg.ports.hp_from_pc)?;
        let duc_in = bind(handle, cfg.ports.duc0_iq)?;
        let status_out = if cfg.ports.hp_to_pc == cfg.ports.ddc_specific {
            ddc_spec.clone()
        } else {
            Arc::new(bind(handle, cfg.ports.hp_to_pc)?)
        };
        let mut ddc_out = Vec::new();
        for n in 0..u16::from(cfg.num_ddcs) {
            ddc_out.push(bind(handle, cfg.ports.ddc0 + n)?);
        }
        let counters = Arc::new(P2Counters::default());
        let io = Io {
            cmd,
            ddc_spec,
            hp_in,
            duc_in,
            status_out,
            ddc_out,
        };
        let task = handle.spawn(run(io, cfg, counters.clone()));
        Ok(Self {
            addr,
            counters,
            task,
        })
    }

    // [@ANCHOR: hpsdr_emulator_p2_addr]
    /// The loopback command address (discovery and General go here).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    // [@ANCHOR: hpsdr_emulator_p2_counters]
    /// What it has seen so far.
    pub fn counters(&self) -> &P2Counters {
        &self.counters
    }

    // [@ANCHOR: hpsdr_emulator_p2_stop]
    /// Stop serving and release the sockets.
    pub fn stop(self) {
        self.task.abort();
    }
}

struct Io {
    cmd: UdpSocket,
    ddc_spec: Arc<UdpSocket>,
    hp_in: UdpSocket,
    duc_in: UdpSocket,
    status_out: Arc<UdpSocket>,
    ddc_out: Vec<UdpSocket>,
}

/// What the client has told the radio.
#[derive(Debug)]
struct RfState {
    host: Option<SocketAddr>,
    phase_word: bool,
    enabled: Vec<bool>,
    rate: Vec<Rate>,
    hz: Vec<u32>,
    atten0: u8,
    run: bool,
}

// [@ANCHOR: hpsdr_emulator_p2_apply_ddc_specific]
/// Fold a DDC-specific packet into the settings: enable bits from byte 7, rate from byte 18 of each
/// 6-byte group starting at byte 17.
fn apply_ddc_specific(d: &[u8], rf: &mut RfState, c: &P2Counters) {
    if d.len() < p2::BIG_LEN {
        return;
    }
    for n in 0..rf.enabled.len() {
        rf.enabled[n] = d[7 + n / 8] & (1 << (n % 8)) != 0;
        let at = 17 + 6 * n;
        if let Ok(r) = Rate::from_ksps(u16::from_be_bytes([d[at + 1], d[at + 2]])) {
            rf.rate[n] = r;
        }
    }
    if let Some(r) = rf.rate.first() {
        c.last_rate_ksps
            .store(u64::from(r.hz() / 1000), Ordering::Relaxed);
    }
}

// [@ANCHOR: hpsdr_emulator_p2_apply_hp]
/// Fold a High Priority packet into the settings and the counters.
fn apply_hp(d: &[u8], cfg: &P2EmulatorConfig, rf: &mut RfState, c: &P2Counters) {
    if d.len() < p2::BIG_LEN {
        return;
    }
    c.hp_packets.fetch_add(1, Ordering::Relaxed);
    let run = d[4] & 1 != 0;
    if run && !rf.run {
        c.run_starts.fetch_add(1, Ordering::Relaxed);
    }
    if !run && rf.run {
        c.run_stops.fetch_add(1, Ordering::Relaxed);
    }
    rf.run = run;
    let ptt = d[4] & 2 != 0;
    c.ptt_now.store(u64::from(ptt), Ordering::Relaxed);
    if ptt {
        c.ptt_frames.fetch_add(1, Ordering::Relaxed);
    }
    if d[345] > 0 {
        c.drive_nonzero_frames.fetch_add(1, Ordering::Relaxed);
    }
    c.last_drive.store(u64::from(d[345]), Ordering::Relaxed);
    if d[329..333] != [0; 4] {
        c.tx_freq_nonzero_frames.fetch_add(1, Ordering::Relaxed);
    }
    rf.atten0 = d[1443];
    c.last_atten_adc0
        .store(u64::from(d[1443]), Ordering::Relaxed);
    let clock = Board::from_p2_type(cfg.board_type).dsp_clock_hz();
    for (n, hz) in rf.hz.iter_mut().enumerate() {
        let w = u32::from_be_bytes([d[9 + 4 * n], d[10 + 4 * n], d[11 + 4 * n], d[12 + 4 * n]]);
        *hz = match (rf.phase_word, clock) {
            (true, Some(ck)) => (f64::from(w) * ck / 4_294_967_296.0).round() as u32,
            _ => w,
        };
    }
}

// [@ANCHOR: hpsdr_emulator_p2_handle_cmd]
/// Act on one datagram on the command port; returns the reply to send to `from`, if any.
fn handle_cmd(
    d: &[u8],
    cfg: &P2EmulatorConfig,
    rf: &mut RfState,
    from: SocketAddr,
    c: &P2Counters,
) -> Option<Vec<u8>> {
    if d.len() >= 5 && d[..4] == [0; 4] && d[4] == 2 {
        c.discoveries.fetch_add(1, Ordering::Relaxed);
        let r = P2Reply {
            running: rf.run,
            mac: cfg.mac,
            board: Board::from_p2_type(cfg.board_type),
            protocol_version: 39,
            firmware: cfg.firmware,
            num_ddcs: cfg.num_ddcs,
            phase_word: cfg.phase_word,
            data_modes: 0,
        };
        return Some(p2::encode_reply(&r, cfg.board_type).to_vec());
    }
    if d.len() >= p2::SMALL_LEN && d[4] == 0 {
        c.generals.fetch_add(1, Ordering::Relaxed);
        rf.host = Some(from);
        rf.phase_word = d[37] & 8 != 0;
    }
    None
}

// [@ANCHOR: hpsdr_emulator_p2_ddc_packet]
/// The next DDC packet of receiver `n`: `DDC_SAMPLES` samples from sample index `at` at the receiver's
/// current centre and rate, the tones scaled by the ADC0 attenuator.
fn ddc_packet(cfg: &P2EmulatorConfig, rf: &RfState, n: usize, seq: u32, at: u64) -> Vec<u8> {
    let gain = 10f64.powf(-f64::from(rf.atten0) / 20.0);
    let tones: Vec<Tone> = cfg
        .tones
        .iter()
        .map(|t| Tone {
            amplitude: t.amplitude * gain,
            ..*t
        })
        .collect();
    let iq = synth(&tones, rf.hz[n], rf.rate[n].hz(), at, DDC_SAMPLES);
    p2::encode_ddc_packet(seq, at, &iq)
}

// [@ANCHOR: hpsdr_emulator_p2_run]
/// The emulator task: answer datagrams on its ports and, while the run bit is set, send one DDC packet
/// per enabled receiver and a status packet each `cfg.pace`.
async fn run(io: Io, cfg: P2EmulatorConfig, c: Arc<P2Counters>) {
    let k = usize::from(cfg.num_ddcs);
    let mut rf = RfState {
        host: None,
        phase_word: false,
        enabled: vec![false; k],
        rate: vec![Rate::K48; k],
        hz: vec![0; k],
        atten0: 0,
        run: false,
    };
    let (mut seq, mut at) = (vec![0u32; k], vec![0u64; k]);
    let mut status_seq = 0u32;
    let mut tick = tokio::time::interval(cfg.pace);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let (mut b0, mut b1, mut b2, mut b3) = (
        vec![0u8; 2048],
        vec![0u8; 2048],
        vec![0u8; 2048],
        vec![0u8; 2048],
    );
    loop {
        tokio::select! {
            r = io.cmd.recv_from(&mut b0) => {
                let Ok((len, from)) = r else { continue };
                if let Some(reply) = handle_cmd(&b0[..len], &cfg, &mut rf, from, &c) {
                    let _ = io.cmd.send_to(&reply, from).await;
                }
            }
            r = io.ddc_spec.recv_from(&mut b1) => {
                let Ok((len, _)) = r else { continue };
                c.ddc_specific.fetch_add(1, Ordering::Relaxed);
                apply_ddc_specific(&b1[..len], &mut rf, &c);
            }
            r = io.hp_in.recv_from(&mut b2) => {
                let Ok((len, _)) = r else { continue };
                apply_hp(&b2[..len], &cfg, &mut rf, &c);
                if !rf.run {
                    seq.iter_mut().for_each(|s| *s = 0);
                    at.iter_mut().for_each(|a| *a = 0);
                }
            }
            r = io.duc_in.recv_from(&mut b3) => {
                if r.is_ok() {
                    c.duc_packets.fetch_add(1, Ordering::Relaxed);
                }
            }
            _ = tick.tick(), if rf.host.is_some() => {
                let Some(to) = rf.host else { continue };
                let st = p2::encode_hp_status(status_seq, &cfg.status);
                status_seq = status_seq.wrapping_add(1);
                let _ = io.status_out.send_to(&st, to).await;
                if !rf.run {
                    continue;
                }
                for n in 0..k {
                    if !rf.enabled[n] {
                        continue;
                    }
                    let skip = cfg.skip_every > 0 && (seq[n] + 1) % cfg.skip_every == 0;
                    if !skip {
                        let d = ddc_packet(&cfg, &rf, n, seq[n], at[n]);
                        if io.ddc_out[n].send_to(&d, to).await.is_ok() {
                            c.ddc_sent.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    at[n] += DDC_SAMPLES as u64;
                    seq[n] = seq[n].wrapping_add(1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn listens_on_loopback_only_and_answers_discovery() {
        // Tests [@ANCHOR: hpsdr_emulator_p2_start]
        // Tests [@ANCHOR: hpsdr_emulator_p2_addr]
        // Tests [@ANCHOR: hpsdr_emulator_p2_stop]
        // Tests [@ANCHOR: hpsdr_emulator_p2_loopback_ports]
        // Tests [@ANCHOR: hpsdr_emulator_p2_handle_cmd]
        // Tests [@ANCHOR: hpsdr_emulator_p2_config_default]
        // Tests [@ANCHOR: hpsdr_emulator_p2_counters]
        // Tests [@ANCHOR: hpsdr_emulator_p2_counters_get]
        // Tests [@ANCHOR: hpsdr_emulator_p2_run]
        let ports = loopback_ports(2).expect("ports");
        let other = loopback_ports(2).expect("ports");
        assert_ne!(ports.ddc_specific, other.ddc_specific);
        let emu = EmulatorP2::start(
            &Handle::current(),
            0,
            P2EmulatorConfig {
                ports,
                ..Default::default()
            },
        )
        .expect("binds");
        assert!(emu.addr().ip().is_loopback());
        let c = bind(&Handle::current(), 0).expect("client");
        c.send_to(&p2::discovery_packet(), emu.addr())
            .await
            .expect("send");
        let mut b = [0u8; 128];
        let (n, _) = tokio::time::timeout(Duration::from_secs(2), c.recv_from(&mut b))
            .await
            .expect("in time")
            .expect("reply");
        let r = p2::parse_reply(&b[..n]).expect("parses");
        assert_eq!(r.board, Board::Hermes);
        assert!(r.phase_word && !r.running);
        // Junk and short datagrams on every port change nothing.
        for p in [ports.ddc_specific, ports.hp_from_pc] {
            c.send_to(&[1, 2, 3], (Ipv4Addr::LOCALHOST, p))
                .await
                .expect("send");
        }
        c.send_to(&[9; 10], emu.addr()).await.expect("send");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(P2Counters::get(&emu.counters().discoveries), 1);
        assert_eq!(P2Counters::get(&emu.counters().hp_packets), 0);
        assert_eq!(P2Counters::get(&emu.counters().ddc_sent), 0);
        emu.stop();
    }

    #[test]
    fn ddc_packets_follow_the_settings() {
        // Tests [@ANCHOR: hpsdr_emulator_p2_apply_hp]
        // Tests [@ANCHOR: hpsdr_emulator_p2_apply_ddc_specific]
        // Tests [@ANCHOR: hpsdr_emulator_p2_ddc_packet]
        let cfg = P2EmulatorConfig::default();
        let c = P2Counters::default();
        let mut rf = RfState {
            host: None,
            phase_word: true,
            enabled: vec![false; 2],
            rate: vec![Rate::K48; 2],
            hz: vec![0; 2],
            atten0: 0,
            run: false,
        };
        let ddcs = [
            p2::DdcSetting {
                enabled: true,
                adc: 0,
                rate: Rate::K384,
            },
            p2::DdcSetting {
                enabled: false,
                adc: 0,
                rate: Rate::K96,
            },
        ];
        let d = p2::encode_ddc_specific(0, 1, 0, 0, &ddcs).expect("encodes");
        apply_ddc_specific(&d, &mut rf, &c);
        assert_eq!(rf.enabled, [true, false]);
        assert_eq!(rf.rate, [Rate::K384, Rate::K96]);
        let hp = p2::HpRx {
            run: true,
            ddc_hz: vec![7_100_000, 14_200_000],
            atten_adc0: 10,
            atten_adc1: 0,
            phase_clock_hz: Some(122_880_000.0),
        };
        let d = p2::encode_high_priority_rx(1, &hp).expect("encodes");
        apply_hp(&d, &cfg, &mut rf, &c);
        assert_eq!(rf.hz, [7_100_000, 14_200_000]);
        assert!(rf.run && rf.atten0 == 10);
        assert_eq!(P2Counters::get(&c.run_starts), 1);
        assert_eq!(P2Counters::get(&c.ptt_frames), 0);
        let pkt = ddc_packet(&cfg, &rf, 0, 7, 100);
        let parsed = p2::parse_ddc_packet(&pkt).expect("parses");
        assert_eq!((parsed.seq, parsed.timestamp), (7, 100));
        assert_eq!(parsed.samples.len(), DDC_SAMPLES);
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! A Protocol 2 receive session with the same shape as the Protocol 1 [`crate::session::Radio`]:
//! configure, start, stream several DDC receivers as [`IqBlock`]s with sequence numbers and drop
//! counters, change frequency, rate and attenuator while running, report hardware status, stop.
//! The transmit path exists but stays closed unless the caller opens the [`TxGate`].
//!
//! Wire facts come from "openHPSDR Ethernet Protocol" v4.4 (P2), see [`crate::p2`]. The session sends
//! every command packet from one host socket and receives every radio-to-host packet on that same
//! socket, telling the DDC streams apart by the radio's source port (`ports.ddc0 + n`, P2 "General
//! Packet"), which is how this crate reads the specification. Whether a real radio does that, or sends
//! to per-stream destination ports, is not settled by the text (`verified=unverified`).
//! Pacing values are `verified=unverified`.

use crate::discovery::{udp_socket, Discovered};
use crate::error::HpsdrError;
use crate::gate::TxGate;
use crate::iq::{ChannelStream, IqBlock, SeqTracker, StreamCounters};
use crate::p2::{self, DdcSetting, HpRx, RadioPorts, TxState as P2Tx};
use crate::session::SessionOptions;
use crate::types::{HardwareStatus, Protocol, Rate};
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Largest queue of transmit I/Q samples held for the sender task (about 85 ms at 48 ksps).
pub const TX_QUEUE_MAX: usize = 4096;
/// The DDC-specific packet is repeated at least this often so a lost datagram cannot leave the radio
/// with stale settings (the High Priority packet is sent every tick).
const DDC_REFRESH: Duration = Duration::from_secs(1);

/// What the session asks the radio to do. Settings the protocol cannot express are rejected.
#[derive(Debug, Clone, PartialEq)]
pub struct P2Config {
    /// DDC sample rate (48 to 1536 ksps), the same for every receiver.
    pub rate: Rate,
    /// NCO frequency of each receiver in hertz; its length is the receiver count (1..=8).
    pub rx_hz: Vec<u32>,
    /// ADCs the radio has (1 for a Hermes).
    pub num_adc: u8,
    /// Step attenuator before ADC0, 0..=31 dB.
    pub att_adc0: u8,
    /// Step attenuator before ADC1, 0..=31 dB.
    pub att_adc1: u8,
    /// ADC dither.
    pub dither: bool,
    /// ADC random.
    pub random: bool,
    /// Transmit NCO frequency in hertz.
    pub tx_hz: u32,
    /// The radio's UDP port map (sent in the General packet).
    pub ports: RadioPorts,
}

impl Default for P2Config {
    // [@ANCHOR: hpsdr_session_p2_config_default]
    /// One receiver at 48 ksps on 7.1 MHz, no attenuation, the specification's default ports.
    fn default() -> Self {
        Self {
            rate: Rate::K48,
            rx_hz: vec![7_100_000],
            num_adc: 1,
            att_adc0: 0,
            att_adc1: 0,
            dither: false,
            random: false,
            tx_hz: 7_100_000,
            ports: RadioPorts::default(),
        }
    }
}

impl P2Config {
    // [@ANCHOR: hpsdr_session_p2_config_validate]
    /// Reject settings the protocol cannot express.
    pub fn validate(&self) -> Result<(), HpsdrError> {
        if self.rx_hz.is_empty() || self.rx_hz.len() > 8 {
            return Err(HpsdrError::Config("1 to 8 receivers".into()));
        }
        if self.att_adc0 > 31 || self.att_adc1 > 31 {
            return Err(HpsdrError::Config("attenuator 0..=31 dB".into()));
        }
        if self.num_adc == 0 || self.num_adc > 8 {
            return Err(HpsdrError::Config("1 to 8 ADCs".into()));
        }
        Ok(())
    }

    // [@ANCHOR: hpsdr_session_p2_config_ddcs]
    /// The DDC-specific settings: every receiver enabled on ADC0 at the common rate.
    fn ddcs(&self) -> Vec<DdcSetting> {
        self.rx_hz
            .iter()
            .map(|_| DdcSetting {
                enabled: true,
                adc: 0,
                rate: self.rate,
            })
            .collect()
    }
}

#[derive(Debug, Default)]
struct TxQueue {
    ptt: bool,
    drive: u8,
    iq: VecDeque<(i16, i16)>,
}

#[derive(Debug)]
struct Shared {
    cfg: Mutex<(P2Config, u64)>,
    status: Mutex<HardwareStatus>,
    tx: Mutex<TxQueue>,
    stray: AtomicU64,
}

// [@ANCHOR: hpsdr_session_p2_lock]
/// Lock a mutex, taking the data even if a panic poisoned it (the gate still decides on transmit).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// Sequence numbers of the four packet kinds the session sends.
#[derive(Debug, Default)]
struct Seqs {
    general: u32,
    ddc: u32,
    hp: u32,
    duc: u32,
}

/// Where the session sends and what it computes with.
#[derive(Debug, Clone)]
struct Target {
    ip: IpAddr,
    cmd_port: u16,
    clock_hz: Option<f64>,
}

// [@ANCHOR: hpsdr_session_p2_hp_rx]
/// The receive High Priority settings from the configuration; `run` is the run bit.
fn hp_rx(cfg: &P2Config, t: &Target, run: bool) -> HpRx {
    HpRx {
        run,
        ddc_hz: cfg.rx_hz.clone(),
        atten_adc0: cfg.att_adc0,
        atten_adc1: cfg.att_adc1,
        phase_clock_hz: t.clock_hz,
    }
}

// [@ANCHOR: hpsdr_session_p2_next_packets]
/// The next High Priority packet and, only while keyed, a DUC I/Q packet. They are transmit packets
/// only while the caller has asked to key AND the gate is open at `now`; otherwise the High Priority
/// packet is the receive packet (PTT bit, drive and transmit frequency structurally absent) and no
/// DUC packet is built. A stale key request is cleared.
fn next_packets(
    shared: &Shared,
    gate: &TxGate,
    t: &Target,
    seqs: &mut Seqs,
    now: Instant,
) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let cfg = lock(&shared.cfg).0.clone();
    let hp = hp_rx(&cfg, t, true);
    let mut tx = lock(&shared.tx);
    if tx.ptt && gate.is_open(now) {
        let n = tx.iq.len().min(p2::DUC_SAMPLES);
        let iq: Vec<(i32, i32)> = tx
            .iq
            .drain(..n)
            .map(|(i, q)| (i32::from(i) << 8, i32::from(q) << 8))
            .collect();
        let state = P2Tx {
            ptt: true,
            duc_hz: cfg.tx_hz,
            drive: tx.drive,
        };
        let hpp = p2::encode_high_priority_tx(seqs.hp, &hp, gate, now, &state).ok();
        let duc = if iq.is_empty() {
            None
        } else {
            p2::encode_duc_iq(seqs.duc, &iq, gate, now).ok()
        };
        if hpp.is_some() {
            seqs.hp = seqs.hp.wrapping_add(1);
        }
        if duc.is_some() {
            seqs.duc = seqs.duc.wrapping_add(1);
        }
        return (hpp, duc);
    }
    tx.ptt = false;
    tx.drive = 0;
    tx.iq.clear();
    let hpp = p2::encode_high_priority_rx(seqs.hp, &hp).ok();
    if hpp.is_some() {
        seqs.hp = seqs.hp.wrapping_add(1);
    }
    (hpp, None)
}

// [@ANCHOR: hpsdr_session_p2_handle_datagram]
/// Decode one received datagram by the radio's source port: a DDC packet becomes a block on its
/// stream (a gap in its sequence numbers is counted as loss), a status packet updates the status,
/// anything else is counted as stray.
fn handle_datagram(
    d: &[u8],
    from_port: u16,
    shared: &Shared,
    senders: &[mpsc::Sender<IqBlock>],
    counters: &[Arc<StreamCounters>],
    trackers: &mut [SeqTracker],
) {
    let (cfg, _) = lock(&shared.cfg).clone();
    let ports = cfg.ports;
    if from_port == ports.hp_to_pc {
        if let Ok((_, st)) = p2::parse_hp_status(d) {
            let mut cur = lock(&shared.status);
            let fw = cur.firmware;
            *cur = st;
            cur.firmware = fw;
            return;
        }
    }
    let n = usize::from(from_port.wrapping_sub(ports.ddc0));
    if from_port < ports.ddc0 || n >= senders.len() {
        shared.stray.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let Ok(p) = p2::parse_ddc_packet(d) else {
        counters[n].malformed();
        return;
    };
    let lost = trackers[n].observe(p.seq);
    counters[n].lost(lost);
    let count = p.samples.len();
    let block = IqBlock {
        stream: n as u8,
        seq: p.seq,
        lost_before: lost,
        sample_rate_hz: cfg.rate.hz(),
        centre_hz: cfg.rx_hz.get(n).copied().unwrap_or(0),
        samples: p.samples,
    };
    match senders[n].try_send(block) {
        Ok(()) => counters[n].delivered(count),
        Err(_) => counters[n].dropped(),
    }
}

/// The task's fixed inputs.
struct Task {
    sock: Arc<UdpSocket>,
    shared: Arc<Shared>,
    gate: TxGate,
    target: Target,
    senders: Vec<mpsc::Sender<IqBlock>>,
    counters: Vec<Arc<StreamCounters>>,
    tick: Duration,
    seqs: Seqs,
}

// [@ANCHOR: hpsdr_session_p2_run]
/// The session task: send High Priority packets (and DDC-specific packets when the settings changed or
/// a second has passed) on a timer, and decode whatever arrives.
async fn run(mut t: Task) {
    let mut trackers = vec![SeqTracker::default(); t.senders.len()];
    let mut iv = tokio::time::interval(t.tick);
    iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut buf = vec![0u8; 2048];
    let (mut sent_ver, mut sent_at) = (u64::MAX, Instant::now());
    loop {
        tokio::select! {
            _ = iv.tick() => {
                let (cfg, ver) = lock(&t.shared.cfg).clone();
                let ip = t.target.ip;
                if ver != sent_ver || sent_at.elapsed() >= DDC_REFRESH {
                    let ddcs = cfg.ddcs();
                    if let Ok(d) = p2::encode_ddc_specific(
                        t.seqs.ddc, cfg.num_adc, u8::from(cfg.dither), u8::from(cfg.random), &ddcs,
                    ) {
                        t.seqs.ddc = t.seqs.ddc.wrapping_add(1);
                        let _ = t.sock.send_to(&d, (ip, cfg.ports.ddc_specific)).await;
                    }
                    sent_ver = ver;
                    sent_at = Instant::now();
                }
                let (hp, duc) = next_packets(&t.shared, &t.gate, &t.target, &mut t.seqs, Instant::now());
                if let Some(d) = hp {
                    let _ = t.sock.send_to(&d, (ip, cfg.ports.hp_from_pc)).await;
                }
                if let Some(d) = duc {
                    let _ = t.sock.send_to(&d, (ip, cfg.ports.duc0_iq)).await;
                }
            }
            r = t.sock.recv_from(&mut buf) => {
                if let Ok((len, from)) = r {
                    handle_datagram(&buf[..len], from.port(), &t.shared, &t.senders, &t.counters, &mut trackers);
                }
            }
        }
    }
}

/// A running Protocol 2 receive session.
#[derive(Debug)]
pub struct Radio2 {
    shared: Arc<Shared>,
    sock: Arc<UdpSocket>,
    task: JoinHandle<()>,
    streams: Vec<ChannelStream>,
    gate: TxGate,
    target: Target,
    num_rx: usize,
}

impl Radio2 {
    // [@ANCHOR: hpsdr_session_p2_start]
    /// Configure the radio (General, DDC-specific, then High Priority with the run bit) and begin
    /// streaming. `radio` must be a Protocol 2 radio from discovery. `gate` is the transmit gate; pass
    /// `TxGate::new()` (closed) for a receive-only session. `opts.tick` paces the command packets.
    pub async fn start(
        handle: &Handle,
        radio: &Discovered,
        cfg: P2Config,
        gate: TxGate,
        opts: SessionOptions,
    ) -> Result<Self, HpsdrError> {
        if radio.protocol != Protocol::P2 {
            return Err(HpsdrError::Unsupported(
                "Protocol 1 radio in a Protocol 2 session".into(),
            ));
        }
        cfg.validate()?;
        let phase = radio.phase_word;
        let clock_hz = if phase {
            Some(radio.board.dsp_clock_hz().ok_or_else(|| {
                HpsdrError::Unsupported("phase words on a board with no known clock".into())
            })?)
        } else {
            None
        };
        let target = Target {
            ip: radio.addr.ip(),
            cmd_port: radio.addr.port(),
            clock_hz,
        };
        let sock = Arc::new(udp_socket(
            handle,
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
        )?);
        let mut seqs = Seqs::default();
        let general = p2::encode_general(seqs.general, &cfg.ports, phase);
        seqs.general = seqs.general.wrapping_add(1);
        sock.send_to(&general, (target.ip, target.cmd_port)).await?;
        // Give the radio the port map before the packets that use it.
        tokio::time::sleep(Duration::from_millis(5)).await;
        let num_rx = cfg.rx_hz.len();
        let shared = Arc::new(Shared {
            cfg: Mutex::new((cfg, 0)),
            status: Mutex::new(HardwareStatus::default()),
            tx: Mutex::new(TxQueue::default()),
            stray: AtomicU64::new(0),
        });
        let (mut senders, mut counters, mut streams) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..num_rx {
            let c = Arc::new(StreamCounters::default());
            let (tx, s) = ChannelStream::new(opts.stream_depth, c.clone());
            senders.push(tx);
            counters.push(c);
            streams.push(s);
        }
        let task = handle.spawn(run(Task {
            sock: sock.clone(),
            shared: shared.clone(),
            gate: gate.clone(),
            target: target.clone(),
            senders,
            counters,
            tick: opts.tick,
            seqs,
        }));
        Ok(Self {
            shared,
            sock,
            task,
            streams,
            gate,
            target,
            num_rx,
        })
    }

    // [@ANCHOR: hpsdr_session_p2_take_streams]
    /// Hand over the per-receiver streams (index = DDC number). Returns an empty list the second time.
    pub fn take_streams(&mut self) -> Vec<ChannelStream> {
        std::mem::take(&mut self.streams)
    }

    // [@ANCHOR: hpsdr_session_p2_update]
    /// Change settings (frequency, sample rate, attenuator...). The receiver count and the port map are
    /// fixed for the life of the session. The change reaches the radio on the next tick.
    pub fn update(&self, f: impl FnOnce(&mut P2Config)) -> Result<(), HpsdrError> {
        let mut g = lock(&self.shared.cfg);
        let mut next = g.0.clone();
        f(&mut next);
        next.validate()?;
        if next.rx_hz.len() != self.num_rx {
            return Err(HpsdrError::Config(
                "the receiver count cannot change while running".into(),
            ));
        }
        if next.ports != g.0.ports {
            return Err(HpsdrError::Config(
                "the port map cannot change while running".into(),
            ));
        }
        g.0 = next;
        g.1 += 1;
        Ok(())
    }

    // [@ANCHOR: hpsdr_session_p2_status]
    /// The latest hardware status (ADC overload, power counts, supply).
    pub fn status(&self) -> HardwareStatus {
        *lock(&self.shared.status)
    }

    // [@ANCHOR: hpsdr_session_p2_stray]
    /// Datagrams that arrived from a source port that belongs to no stream or status.
    pub fn stray_datagrams(&self) -> u64 {
        self.shared.stray.load(Ordering::Relaxed)
    }

    // [@ANCHOR: hpsdr_session_p2_request_ptt]
    /// Ask to key (or unkey) the transmitter with `drive` 0..=255. Refused with
    /// [`HpsdrError::GateClosed`] while the gate is closed; unkeying is always accepted. The sender
    /// task re-checks the gate before every packet and unkeys when the lease expires.
    pub fn request_ptt(&self, ptt: bool, drive: u8) -> Result<(), HpsdrError> {
        let mut tx = lock(&self.shared.tx);
        if !ptt {
            tx.ptt = false;
            tx.drive = 0;
            tx.iq.clear();
            return Ok(());
        }
        if !self.gate.is_open(Instant::now()) {
            return Err(HpsdrError::GateClosed);
        }
        tx.ptt = true;
        tx.drive = drive;
        Ok(())
    }

    // [@ANCHOR: hpsdr_session_p2_send_tx_iq]
    /// Queue transmit I/Q (16-bit pairs). Refused while the gate is closed or the queue would exceed
    /// [`TX_QUEUE_MAX`] (nothing is dropped silently or repeated).
    pub fn send_tx_iq(&self, iq: &[(i16, i16)]) -> Result<(), HpsdrError> {
        if !self.gate.is_open(Instant::now()) {
            return Err(HpsdrError::GateClosed);
        }
        let mut tx = lock(&self.shared.tx);
        if tx.iq.len() + iq.len() > TX_QUEUE_MAX {
            return Err(HpsdrError::Config("transmit queue full".into()));
        }
        tx.iq.extend(iq.iter().copied());
        Ok(())
    }

    // [@ANCHOR: hpsdr_session_p2_stop]
    /// Unkey, send a High Priority packet with the run bit clear, and end the session task.
    pub async fn stop(self) -> Result<(), HpsdrError> {
        {
            let mut tx = lock(&self.shared.tx);
            tx.ptt = false;
            tx.drive = 0;
            tx.iq.clear();
        }
        self.task.abort();
        let cfg = lock(&self.shared.cfg).0.clone();
        let d = p2::encode_high_priority_rx(u32::MAX, &hp_rx(&cfg, &self.target, false))?;
        self.sock
            .send_to(&d, (self.target.ip, cfg.ports.hp_from_pc))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::discover;
    use crate::emulator::Tone;
    use crate::emulator_p2::{loopback_ports, EmulatorP2, P2Counters, P2EmulatorConfig};
    use crate::iq::{Complex, IqStream};

    fn tone_offset_hz(s: &[Complex], fs: f64) -> f64 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for w in s.windows(2) {
            let (a, b) = (w[0], w[1]);
            re += f64::from(a.re * b.re + a.im * b.im);
            im += f64::from(a.re * b.im - a.im * b.re);
        }
        im.atan2(re) * fs / std::f64::consts::TAU
    }

    fn rms(s: &[Complex]) -> f64 {
        let e: f64 = s.iter().map(|c| f64::from(c.re * c.re + c.im * c.im)).sum();
        (e / s.len() as f64).sqrt()
    }

    async fn setup(mut ecfg: P2EmulatorConfig, ddcs: u8) -> (EmulatorP2, Discovered, RadioPorts) {
        let ports = loopback_ports(ddcs).expect("free ports");
        ecfg.ports = ports;
        let emu = EmulatorP2::start(&Handle::current(), 0, ecfg).expect("emulator binds loopback");
        let found = discover(
            &Handle::current(),
            &[emu.addr()],
            Duration::from_millis(300),
        )
        .await
        .expect("discover");
        assert_eq!(found.len(), 1);
        (emu, found[0].clone(), ports)
    }

    async fn wait_until(mut f: impl FnMut() -> bool) -> bool {
        for _ in 0..300 {
            if f() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }

    async fn take_block(s: &mut impl IqStream) -> IqBlock {
        tokio::time::timeout(Duration::from_secs(5), s.next_block())
            .await
            .expect("block in time")
            .expect("stream open")
    }

    fn cfg_with(ports: RadioPorts, rx_hz: Vec<u32>, rate: Rate) -> P2Config {
        P2Config {
            rx_hz,
            rate,
            ports,
            ..P2Config::default()
        }
    }

    #[tokio::test]
    async fn two_receivers_stream_their_own_tones_without_loss() {
        // Tests [@ANCHOR: hpsdr_session_p2_start]
        // Tests [@ANCHOR: hpsdr_session_p2_run]
        // Tests [@ANCHOR: hpsdr_session_p2_handle_datagram]
        // Tests [@ANCHOR: hpsdr_session_p2_take_streams]
        // Tests [@ANCHOR: hpsdr_session_p2_next_packets]
        // Tests [@ANCHOR: hpsdr_session_p2_lock]
        // Tests [@ANCHOR: hpsdr_session_p2_hp_rx]
        // Tests [@ANCHOR: hpsdr_session_p2_config_default]
        // Tests [@ANCHOR: hpsdr_session_p2_config_ddcs]
        // Tests [@ANCHOR: hpsdr_session_p2_stray]
        // Tests [@ANCHOR: hpsdr_session_p2_stop]
        let ecfg = P2EmulatorConfig {
            tones: vec![
                Tone {
                    freq_hz: 7_100_500.0,
                    amplitude: 0.4,
                },
                Tone {
                    freq_hz: 14_198_500.0,
                    amplitude: 0.4,
                },
            ],
            ..Default::default()
        };
        let (emu, d, ports) = setup(ecfg, 2).await;
        assert!(d.phase_word);
        let cfg = cfg_with(ports, vec![7_100_000, 14_200_000], Rate::K96);
        let mut radio = Radio2::start(
            &Handle::current(),
            &d,
            cfg,
            TxGate::new(),
            SessionOptions::default(),
        )
        .await
        .expect("start");
        let mut s = radio.take_streams();
        assert_eq!(s.len(), 2);
        assert!(radio.take_streams().is_empty());
        let mut last_seq = [None::<u32>; 2];
        for r in 0..2 {
            let mut all = Vec::new();
            for _ in 0..8 {
                let b = take_block(&mut s[r]).await;
                assert_eq!(b.stream, r as u8);
                assert_eq!(b.lost_before, 0);
                if let Some(prev) = last_seq[r] {
                    assert_eq!(b.seq, prev + 1, "sequence numbers are consecutive");
                }
                last_seq[r] = Some(b.seq);
                assert_eq!(b.sample_rate_hz, 96_000);
                all.extend(b.samples);
            }
            let off = tone_offset_hz(&all, 96_000.0);
            let want = if r == 0 { 500.0 } else { -1500.0 };
            assert!(
                (off - want).abs() < 20.0,
                "rx{r} tone at {off}, want {want}"
            );
        }
        assert_eq!(s[0].stats().lost_packets, 0);
        assert_eq!(radio.stray_datagrams(), 0);
        assert!(P2Counters::get(&emu.counters().generals) >= 1);
        assert!(P2Counters::get(&emu.counters().ddc_specific) >= 1);
        radio.stop().await.expect("stop");
        assert!(wait_until(|| P2Counters::get(&emu.counters().run_stops) >= 1).await);
        emu.stop();
    }

    #[tokio::test]
    async fn frequency_rate_and_attenuator_changes_reach_the_radio() {
        // Tests [@ANCHOR: hpsdr_session_p2_update]
        // Tests [@ANCHOR: hpsdr_session_p2_config_validate]
        let (emu, d, ports) = setup(P2EmulatorConfig::default(), 1).await;
        let mut radio = Radio2::start(
            &Handle::current(),
            &d,
            cfg_with(ports, vec![7_100_000], Rate::K48),
            TxGate::new(),
            SessionOptions::default(),
        )
        .await
        .expect("start");
        let mut s = radio.take_streams().remove(0);
        let b = take_block(&mut s).await;
        assert_eq!(b.sample_rate_hz, 48_000);
        let mut all = Vec::new();
        for _ in 0..6 {
            all.extend(take_block(&mut s).await.samples);
        }
        let full = rms(&all);
        assert!((tone_offset_hz(&all, 48_000.0) - 500.0).abs() < 20.0);
        // Retune so the tone moves to -2500 Hz, widen the rate and add 20 dB of attenuation.
        radio
            .update(|c| {
                c.rx_hz = vec![7_103_000];
                c.rate = Rate::K192;
                c.att_adc0 = 20;
            })
            .expect("update");
        assert!(wait_until(|| P2Counters::get(&emu.counters().last_atten_adc0) == 20).await);
        // Blocks buffered before the change are still in the stream, and the radio applies the
        // rate and the frequency in separate packets: accept three consecutive blocks that show
        // the new tone.
        let mut good: Vec<Complex> = Vec::new();
        let mut run_len = 0;
        for _ in 0..400 {
            let b = take_block(&mut s).await;
            let ok = b.sample_rate_hz == 192_000
                && b.centre_hz == 7_103_000
                && (tone_offset_hz(&b.samples, 192_000.0) + 2500.0).abs() < 60.0;
            if ok {
                run_len += 1;
                good.extend(b.samples);
            } else {
                run_len = 0;
                good.clear();
            }
            if run_len == 3 {
                break;
            }
        }
        assert_eq!(run_len, 3, "the new tone never appeared");
        let ratio = rms(&good) / full;
        assert!((0.07..0.14).contains(&ratio), "20 dB down, ratio {ratio}");
        assert!(wait_until(|| P2Counters::get(&emu.counters().last_rate_ksps) == 192).await);
        // Bad updates are refused and change nothing.
        assert!(radio.update(|c| c.att_adc0 = 32).is_err());
        assert!(radio.update(|c| c.rx_hz.push(1)).is_err());
        assert!(radio.update(|c| c.ports.ddc0 += 1).is_err());
        assert!(P2Config {
            rx_hz: vec![],
            ..P2Config::default()
        }
        .validate()
        .is_err());
        radio.stop().await.expect("stop");
        emu.stop();
    }

    #[tokio::test]
    async fn overload_power_and_supply_status_are_reported() {
        // Tests [@ANCHOR: hpsdr_session_p2_status]
        let mut ecfg = P2EmulatorConfig::default();
        ecfg.status.adc_overload[0] = true;
        ecfg.status.forward_raw = 1234;
        ecfg.status.reverse_raw = 77;
        ecfg.status.supply_raw = 3500;
        let (emu, d, ports) = setup(ecfg, 1).await;
        let radio = Radio2::start(
            &Handle::current(),
            &d,
            cfg_with(ports, vec![7_100_000], Rate::K48),
            TxGate::new(),
            SessionOptions::default(),
        )
        .await
        .expect("start");
        assert!(wait_until(|| radio.status().forward_raw == 1234).await);
        let st = radio.status();
        assert!(st.adc_overload[0] && !st.adc_overload[1]);
        assert_eq!(st.reverse_raw, 77);
        assert!((st.supply_volts() - 2.82).abs() < 0.05);
        radio.stop().await.expect("stop");
        emu.stop();
    }

    #[tokio::test]
    async fn a_whole_receive_session_with_a_closed_gate_never_transmits() {
        // Tests [@ANCHOR: hpsdr_session_p2_request_ptt]
        // Tests [@ANCHOR: hpsdr_session_p2_send_tx_iq]
        let (emu, d, ports) = setup(P2EmulatorConfig::default(), 1).await;
        let radio = Radio2::start(
            &Handle::current(),
            &d,
            cfg_with(ports, vec![7_100_000], Rate::K48),
            TxGate::new(),
            SessionOptions::default(),
        )
        .await
        .expect("start");
        assert_eq!(radio.request_ptt(true, 200), Err(HpsdrError::GateClosed));
        assert_eq!(
            radio.send_tx_iq(&[(1000, 1000)]),
            Err(HpsdrError::GateClosed)
        );
        assert!(radio.request_ptt(false, 0).is_ok());
        let c = emu.counters();
        assert!(wait_until(|| P2Counters::get(&c.hp_packets) >= 30).await);
        assert!(P2Counters::get(&c.ddc_sent) > 0);
        assert_eq!(P2Counters::get(&c.ptt_frames), 0);
        assert_eq!(P2Counters::get(&c.drive_nonzero_frames), 0);
        assert_eq!(P2Counters::get(&c.duc_packets), 0);
        assert_eq!(P2Counters::get(&c.tx_freq_nonzero_frames), 0);
        radio.stop().await.expect("stop");
        assert_eq!(P2Counters::get(&c.ptt_frames), 0);
        assert_eq!(P2Counters::get(&c.duc_packets), 0);
        emu.stop();
    }

    #[tokio::test]
    async fn an_open_gate_keys_and_the_lease_expiry_unkeys() {
        let (emu, d, ports) = setup(P2EmulatorConfig::default(), 1).await;
        let gate = TxGate::new();
        let radio = Radio2::start(
            &Handle::current(),
            &d,
            cfg_with(ports, vec![7_100_000], Rate::K48),
            gate.clone(),
            SessionOptions::default(),
        )
        .await
        .expect("start");
        let c = emu.counters();
        assert!(gate.open_until(Instant::now() + Duration::from_millis(400)));
        radio.request_ptt(true, 50).expect("gate open");
        radio.send_tx_iq(&[(1000, -1000); 480]).expect("queued");
        assert!(wait_until(|| P2Counters::get(&c.ptt_frames) > 0).await);
        assert!(wait_until(|| P2Counters::get(&c.duc_packets) >= 2).await);
        assert_eq!(P2Counters::get(&c.last_drive), 50);
        // The lease runs out by itself: the next command packets are receive packets.
        assert!(wait_until(|| !gate.is_open(Instant::now())).await);
        tokio::time::sleep(Duration::from_millis(60)).await;
        let frames = P2Counters::get(&c.ptt_frames);
        let duc = P2Counters::get(&c.duc_packets);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            P2Counters::get(&c.ptt_frames),
            frames,
            "no PTT after expiry"
        );
        assert_eq!(P2Counters::get(&c.duc_packets), duc, "no I/Q after expiry");
        assert_eq!(P2Counters::get(&c.ptt_now), 0);
        assert_eq!(radio.send_tx_iq(&[(1, 1)]), Err(HpsdrError::GateClosed));
        // A Protocol 1 radio does not belong in this session.
        let mut p1 = d.clone();
        p1.protocol = Protocol::P1;
        assert!(Radio2::start(
            &Handle::current(),
            &p1,
            P2Config::default(),
            TxGate::new(),
            SessionOptions::default()
        )
        .await
        .is_err());
        radio.stop().await.expect("stop");
        emu.stop();
    }

    #[tokio::test]
    async fn lost_datagrams_and_a_slow_consumer_are_counted() {
        let ecfg = P2EmulatorConfig {
            skip_every: 5,
            ..Default::default()
        };
        let (emu, d, ports) = setup(ecfg, 1).await;
        let mut radio = Radio2::start(
            &Handle::current(),
            &d,
            cfg_with(ports, vec![7_100_000], Rate::K48),
            TxGate::new(),
            SessionOptions::default(),
        )
        .await
        .expect("start");
        let mut s = radio.take_streams().remove(0);
        let mut lost_seen = 0u32;
        for _ in 0..30 {
            lost_seen += take_block(&mut s).await.lost_before;
        }
        assert!(lost_seen >= 4, "loss injection shows in lost_before");
        assert_eq!(u64::from(lost_seen), s.stats().lost_packets);
        radio.stop().await.expect("stop");
        emu.stop();
        // A consumer that does not read: the bounded buffer drops new blocks instead of growing.
        let (emu, d, ports) = setup(P2EmulatorConfig::default(), 1).await;
        let opts = SessionOptions {
            stream_depth: 2,
            ..SessionOptions::default()
        };
        let mut slow = Radio2::start(
            &Handle::current(),
            &d,
            cfg_with(ports, vec![7_100_000], Rate::K48),
            TxGate::new(),
            opts,
        )
        .await
        .expect("start");
        let ss = slow.take_streams().remove(0);
        assert!(wait_until(|| ss.stats().dropped_blocks > 0).await);
        slow.stop().await.expect("stop");
        emu.stop();
    }
}

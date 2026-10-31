// SPDX-License-Identifier: AGPL-3.0-or-later
//! A Protocol 1 receive session: configure, start, stream I/Q for several DDC receivers, report
//! hardware status, stop. The transmit path exists but stays closed unless the caller opens the
//! [`TxGate`].
//!
//! The session spawns its task on the runtime [`Handle`] the caller passes and never creates a
//! runtime. All timing and pacing values are `verified=unverified`.

use crate::discovery::{udp_socket, Discovered};
use crate::error::HpsdrError;
use crate::gate::TxGate;
use crate::iq::{ChannelStream, IqBlock, SeqTracker, StreamCounters};
use crate::p1::{self, Ep2Builder, P1Config, TxRequest};
use crate::types::{HardwareStatus, Protocol};
use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Largest queue of transmit I/Q samples held for the sender task (about 85 ms at 48 ksps).
pub const TX_QUEUE_MAX: usize = 4096;

/// Tunables of a session.
#[derive(Debug, Clone, Copy)]
pub struct SessionOptions {
    /// Interval between EP2 datagrams (C&C refresh and transmit I/Q pacing). Unverified: a real
    /// radio may want datagrams paced to the 48 ksps transmit sample clock (126 samples, 2.6 ms).
    pub tick: Duration,
    /// Blocks buffered per receiver before the oldest-unread policy drops new ones.
    pub stream_depth: usize,
}

impl Default for SessionOptions {
    // [@ANCHOR: hpsdr_session_options_default]
    /// 10 ms tick, 64 blocks per stream.
    fn default() -> Self {
        Self {
            tick: Duration::from_millis(10),
            stream_depth: 64,
        }
    }
}

#[derive(Debug, Default)]
struct TxState {
    ptt: bool,
    drive: u8,
    iq: VecDeque<(i16, i16)>,
}

#[derive(Debug)]
struct Shared {
    cfg: Mutex<P1Config>,
    status: Mutex<HardwareStatus>,
    tx: Mutex<TxState>,
}

// [@ANCHOR: hpsdr_session_lock]
/// Lock a mutex, taking the data even if a panic poisoned it (the gate still decides on transmit).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// A running Protocol 1 receive session.
#[derive(Debug)]
pub struct Radio {
    shared: Arc<Shared>,
    sock: Arc<UdpSocket>,
    task: JoinHandle<()>,
    streams: Vec<ChannelStream>,
    gate: TxGate,
    num_rx: u8,
}

// [@ANCHOR: hpsdr_session_next_datagram]
/// The next EP2 datagram. It is a transmit datagram only while the caller has asked to key AND the
/// gate is open at `now`; otherwise it is a receive datagram, and a stale key request is cleared.
fn next_datagram(
    shared: &Shared,
    gate: &TxGate,
    b: &mut Ep2Builder,
    now: Instant,
) -> Option<Vec<u8>> {
    let cfg = lock(&shared.cfg).clone();
    let mut tx = lock(&shared.tx);
    if tx.ptt && gate.is_open(now) {
        let n = tx.iq.len().min(p1::TX_SAMPLES_PER_DATAGRAM);
        let iq: Vec<(i16, i16)> = tx.iq.drain(..n).collect();
        let req = TxRequest {
            ptt: true,
            drive: tx.drive,
            iq: &iq,
        };
        return b.build_tx(&cfg, gate, now, &req).ok();
    }
    tx.ptt = false;
    tx.iq.clear();
    b.build_rx(&cfg).ok()
}

// [@ANCHOR: hpsdr_session_handle_ep6]
/// Decode one received datagram, update status and push a block per receiver.
fn handle_ep6(
    d: &[u8],
    shared: &Shared,
    senders: &[mpsc::Sender<IqBlock>],
    counters: &[Arc<StreamCounters>],
    trackers: &mut [SeqTracker],
) {
    let cfg = lock(&shared.cfg).clone();
    let Ok(p) = p1::parse_ep6(d, cfg.num_rx) else {
        counters.iter().for_each(|c| c.malformed());
        return;
    };
    {
        let mut st = lock(&shared.status);
        for cc in &p.cc {
            p1::apply_status_cc(&mut st, cc);
        }
    }
    for (r, samples) in p.rx.into_iter().enumerate() {
        let lost = trackers[r].observe(p.seq);
        counters[r].lost(lost);
        let n = samples.len();
        let block = IqBlock {
            stream: r as u8,
            seq: p.seq,
            lost_before: lost,
            sample_rate_hz: cfg.rate.hz(),
            centre_hz: cfg.rx_hz[r],
            samples,
        };
        match senders[r].try_send(block) {
            Ok(()) => counters[r].delivered(n),
            Err(_) => counters[r].dropped(),
        }
    }
}

// [@ANCHOR: hpsdr_session_run]
/// The session task: send EP2 datagrams on a timer and decode whatever arrives.
async fn run(
    sock: Arc<UdpSocket>,
    shared: Arc<Shared>,
    gate: TxGate,
    senders: Vec<mpsc::Sender<IqBlock>>,
    counters: Vec<Arc<StreamCounters>>,
    tick: Duration,
) {
    let mut builder = Ep2Builder::new();
    let mut trackers = vec![SeqTracker::default(); senders.len()];
    let mut iv = tokio::time::interval(tick);
    iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut buf = vec![0u8; 2048];
    loop {
        tokio::select! {
            _ = iv.tick() => {
                if let Some(d) = next_datagram(&shared, &gate, &mut builder, Instant::now()) {
                    let _ = sock.send(&d).await;
                }
            }
            r = sock.recv(&mut buf) => {
                if let Ok(len) = r {
                    handle_ep6(&buf[..len], &shared, &senders, &counters, &mut trackers);
                }
            }
        }
    }
}

impl Radio {
    // [@ANCHOR: hpsdr_session_start]
    /// Configure the radio, send Start, and begin streaming. `radio` must be a Protocol 1 radio from
    /// discovery. `gate` is the transmit gate; pass `TxGate::new()` (closed) for a receive-only session.
    pub async fn start(
        handle: &Handle,
        radio: &Discovered,
        cfg: P1Config,
        gate: TxGate,
        opts: SessionOptions,
    ) -> Result<Self, HpsdrError> {
        if radio.protocol != Protocol::P1 {
            return Err(HpsdrError::Unsupported("Protocol 2 sessions".into()));
        }
        cfg.validate()?;
        let sock = udp_socket(handle, SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))?;
        sock.connect(radio.addr).await?;
        let sock = Arc::new(sock);
        let mut pre = Ep2Builder::new();
        for _ in 0..cfg.addresses().len().div_ceil(2) + 1 {
            sock.send(&pre.build_rx(&cfg)?).await?;
        }
        sock.send(&p1::start_packet(true, false)).await?;
        let num_rx = cfg.num_rx;
        let shared = Arc::new(Shared {
            cfg: Mutex::new(cfg),
            status: Mutex::new(HardwareStatus::default()),
            tx: Mutex::new(TxState::default()),
        });
        let (mut senders, mut counters, mut streams) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..num_rx {
            let c = Arc::new(StreamCounters::default());
            let (tx, s) = ChannelStream::new(opts.stream_depth, c.clone());
            senders.push(tx);
            counters.push(c);
            streams.push(s);
        }
        let task = handle.spawn(run(
            sock.clone(),
            shared.clone(),
            gate.clone(),
            senders,
            counters,
            opts.tick,
        ));
        Ok(Self {
            shared,
            sock,
            task,
            streams,
            gate,
            num_rx,
        })
    }

    // [@ANCHOR: hpsdr_session_take_streams]
    /// Hand over the per-receiver streams (index = DDC number). Returns an empty list the second time.
    pub fn take_streams(&mut self) -> Vec<ChannelStream> {
        std::mem::take(&mut self.streams)
    }

    // [@ANCHOR: hpsdr_session_update]
    /// Change settings (frequency, sample rate, attenuator, pre-amp...). The receiver count is fixed
    /// for the life of the session. The change reaches the radio in the next datagrams.
    pub fn update(&self, f: impl FnOnce(&mut P1Config)) -> Result<(), HpsdrError> {
        let mut cfg = lock(&self.shared.cfg);
        let mut next = cfg.clone();
        f(&mut next);
        next.validate()?;
        if next.num_rx != self.num_rx {
            return Err(HpsdrError::Config(
                "the receiver count cannot change while running".into(),
            ));
        }
        *cfg = next;
        Ok(())
    }

    // [@ANCHOR: hpsdr_session_status]
    /// The latest hardware status (ADC overload, power counts, supply, firmware).
    pub fn status(&self) -> HardwareStatus {
        *lock(&self.shared.status)
    }

    // [@ANCHOR: hpsdr_session_request_ptt]
    /// Ask to key (or unkey) the transmitter with `drive` 0..=255. Refused with
    /// [`HpsdrError::GateClosed`] while the gate is closed; unkeying is always accepted. The sender
    /// task re-checks the gate before every datagram and unkeys when the lease expires.
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

    // [@ANCHOR: hpsdr_session_send_tx_iq]
    /// Queue transmit I/Q (16-bit pairs at 48 ksps). Refused while the gate is closed or the queue
    /// would exceed [`TX_QUEUE_MAX`] (nothing is dropped silently or repeated).
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

    // [@ANCHOR: hpsdr_session_stop]
    /// Unkey, send Stop, and end the session task.
    pub async fn stop(self) -> Result<(), HpsdrError> {
        {
            let mut tx = lock(&self.shared.tx);
            tx.ptt = false;
            tx.drive = 0;
            tx.iq.clear();
        }
        self.task.abort();
        self.sock.send(&p1::stop_packet()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::discover;
    use crate::emulator::{Emulator, EmulatorConfig, EmulatorCounters, Tone};
    use crate::iq::{Complex, IqStream};
    use crate::types::Rate;

    const WAIT: Duration = Duration::from_secs(5);

    fn tone_offset_hz(s: &[Complex], fs: f64) -> f64 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for w in s.windows(2) {
            let (a, b) = (w[0], w[1]);
            re += f64::from(a.re * b.re + a.im * b.im);
            im += f64::from(a.re * b.im - a.im * b.re);
        }
        im.atan2(re) * fs / std::f64::consts::TAU
    }

    async fn setup(cfg: EmulatorConfig) -> (Emulator, Discovered) {
        let emu = Emulator::start_p1(&Handle::current(), 0, cfg).expect("emulator binds loopback");
        let found = discover(
            &Handle::current(),
            &[emu.addr()],
            Duration::from_millis(300),
        )
        .await
        .expect("discover");
        assert_eq!(found.len(), 1);
        let d = found[0].clone();
        (emu, d)
    }

    async fn wait_until(mut f: impl FnMut() -> bool) -> bool {
        for _ in 0..200 {
            if f() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }

    #[tokio::test]
    async fn discovery_finds_the_emulator_with_board_and_version() {
        // Tests [@ANCHOR: hpsdr_discovery_parse_reply]
        // Tests [@ANCHOR: hpsdr_discovery_discover]
        // Tests [@ANCHOR: hpsdr_discovery_udp_socket]
        // Tests [@ANCHOR: hpsdr_discovery_broadcast_target]
        // Tests [@ANCHOR: hpsdr_emulator_start_p1]
        // Tests [@ANCHOR: hpsdr_emulator_addr]
        // Tests [@ANCHOR: hpsdr_emulator_config_default]
        // Tests [@ANCHOR: hpsdr_emulator_handle_datagram]
        // Tests [@ANCHOR: hpsdr_emulator_stop]
        let (emu, d) = setup(EmulatorConfig::default()).await;
        assert_eq!(d.protocol, Protocol::P1);
        assert_eq!(d.board, crate::types::Board::Hermes);
        assert_eq!(d.code_version, 73);
        assert!(!d.busy);
        assert!(
            emu.addr().ip().is_loopback(),
            "the emulator listens on loopback only"
        );
        assert_eq!(EmulatorCounters::get(&emu.counters().discoveries), 1);
        assert_eq!(crate::discovery::broadcast_target().port(), 1024);
        assert!(crate::discovery::parse_reply(emu.addr(), &[0u8; 20]).is_err());
        emu.stop();
    }

    #[tokio::test]
    async fn two_receivers_stream_their_own_tones_without_loss() {
        // Tests [@ANCHOR: hpsdr_session_start]
        // Tests [@ANCHOR: hpsdr_session_run]
        // Tests [@ANCHOR: hpsdr_session_handle_ep6]
        // Tests [@ANCHOR: hpsdr_session_take_streams]
        // Tests [@ANCHOR: hpsdr_session_next_datagram]
        // Tests [@ANCHOR: hpsdr_session_lock]
        // Tests [@ANCHOR: hpsdr_session_options_default]
        // Tests [@ANCHOR: hpsdr_emulator_synth]
        // Tests [@ANCHOR: hpsdr_emulator_run_p1]
        // Tests [@ANCHOR: hpsdr_emulator_next_ep6]
        // Tests [@ANCHOR: hpsdr_emulator_counters_get]
        // Tests [@ANCHOR: hpsdr_emulator_counters]
        let ecfg = EmulatorConfig {
            tones: vec![
                Tone {
                    freq_hz: 7_100_500.0,
                    amplitude: 0.4,
                },
                Tone {
                    freq_hz: 14_200_000.0 - 1_500.0,
                    amplitude: 0.4,
                },
            ],
            ..Default::default()
        };
        let (emu, d) = setup(ecfg).await;
        let cfg = P1Config {
            rate: Rate::K96,
            num_rx: 2,
            rx_hz: [7_100_000, 14_200_000, 0, 0, 0, 0, 0],
            ..P1Config::default()
        };
        let mut radio = Radio::start(
            &Handle::current(),
            &d,
            cfg,
            TxGate::new(),
            SessionOptions::default(),
        )
        .await
        .expect("start");
        let mut streams = radio.take_streams();
        assert_eq!(streams.len(), 2);
        assert!(radio.take_streams().is_empty());
        let mut got = Vec::new();
        for s in streams.iter_mut() {
            let mut all = Vec::new();
            let mut last_seq = None;
            for _ in 0..6 {
                let b = tokio::time::timeout(WAIT, s.next_block())
                    .await
                    .expect("block in time")
                    .expect("stream open");
                assert_eq!(b.sample_rate_hz, 96_000);
                assert_eq!(b.lost_before, 0);
                if let Some(l) = last_seq {
                    assert_eq!(b.seq, u32::wrapping_add(l, 1), "consecutive datagrams");
                }
                last_seq = Some(b.seq);
                all.push(b);
            }
            got.push(all);
        }
        assert_eq!(got[0][0].stream, 0);
        assert_eq!(got[1][0].stream, 1);
        assert_eq!(got[0][0].centre_hz, 7_100_000);
        assert_eq!(got[1][0].centre_hz, 14_200_000);
        let off0 = tone_offset_hz(&got[0][1].samples, 96_000.0);
        let off1 = tone_offset_hz(&got[1][1].samples, 96_000.0);
        assert!(
            (off0 - 500.0).abs() < 2.0,
            "receiver 0 sees +500 Hz, got {off0}"
        );
        assert!(
            (off1 + 1500.0).abs() < 2.0,
            "receiver 1 sees -1500 Hz, got {off1}"
        );
        assert_eq!(streams[0].stats().lost_packets, 0);
        assert!(streams[0].stats().blocks >= 6);
        assert_eq!(EmulatorCounters::get(&emu.counters().starts), 1);
        radio.stop().await.expect("stop");
        assert!(wait_until(|| EmulatorCounters::get(&emu.counters().stops) >= 1).await);
        emu.stop();
    }

    #[tokio::test]
    async fn sample_rate_and_frequency_changes_reach_the_radio() {
        // Tests [@ANCHOR: hpsdr_session_update]
        let (emu, d) = setup(EmulatorConfig::default()).await;
        let mut radio = Radio::start(
            &Handle::current(),
            &d,
            P1Config {
                rx_hz: [7_100_000; 7],
                ..P1Config::default()
            },
            TxGate::new(),
            SessionOptions::default(),
        )
        .await
        .expect("start");
        let mut s = radio.take_streams().remove(0);
        let b = tokio::time::timeout(WAIT, s.next_block())
            .await
            .expect("t")
            .expect("open");
        assert_eq!(b.sample_rate_hz, 48_000);
        radio
            .update(|c| {
                c.rate = Rate::K192;
                c.rx_hz[0] = 7_100_000 - 2_000;
            })
            .expect("update");
        let mut seen = None;
        for _ in 0..200 {
            let b = tokio::time::timeout(WAIT, s.next_block())
                .await
                .expect("t")
                .expect("open");
            if b.sample_rate_hz == 192_000 && b.centre_hz == 7_098_000 {
                let off = tone_offset_hz(&b.samples, 192_000.0);
                if (off - 2_500.0).abs() < 5.0 {
                    seen = Some(off);
                    break;
                }
            }
        }
        assert!(
            seen.is_some(),
            "the emulator applied the new rate and frequency and the tone moved to +2500 Hz"
        );
        assert!(
            radio.update(|c| c.num_rx = 2).is_err(),
            "receiver count is fixed"
        );
        assert!(radio.update(|c| c.att_db = 40).is_err());
        radio.stop().await.expect("stop");
        emu.stop();
    }

    #[tokio::test]
    async fn overload_power_and_supply_status_are_reported() {
        // Tests [@ANCHOR: hpsdr_session_status]
        let mut ecfg = EmulatorConfig::default();
        ecfg.status.adc_overload[0] = true;
        ecfg.status.forward_raw = 3000;
        ecfg.status.reverse_raw = 500;
        ecfg.status.supply_raw = 4095;
        let (emu, d) = setup(ecfg).await;
        let radio = Radio::start(
            &Handle::current(),
            &d,
            P1Config::default(),
            TxGate::new(),
            SessionOptions::default(),
        )
        .await
        .expect("start");
        assert!(
            wait_until(|| radio.status().supply_raw == 4095
                && radio.status().forward_raw == 3000
                && radio.status().reverse_raw == 500)
            .await
        );
        let st = radio.status();
        assert!(st.adc_overload[0] && !st.adc_overload[1]);
        assert_eq!(st.firmware, 73);
        assert!(st.swr(0.095).is_some_and(|s| s > 1.0));
        radio.stop().await.expect("stop");
        emu.stop();
    }

    #[tokio::test]
    async fn a_whole_receive_session_with_a_closed_gate_never_transmits() {
        // Tests [@ANCHOR: hpsdr_session_request_ptt]
        // Tests [@ANCHOR: hpsdr_session_send_tx_iq]
        let (emu, d) = setup(EmulatorConfig::default()).await;
        let gate = TxGate::new();
        let mut radio = Radio::start(
            &Handle::current(),
            &d,
            P1Config {
                num_rx: 3,
                ..P1Config::default()
            },
            gate,
            SessionOptions::default(),
        )
        .await
        .expect("start");
        assert_eq!(radio.request_ptt(true, 255), Err(HpsdrError::GateClosed));
        assert_eq!(
            radio.send_tx_iq(&[(1000, 1000); 100]),
            Err(HpsdrError::GateClosed)
        );
        assert_eq!(
            radio.request_ptt(false, 0),
            Ok(()),
            "unkeying is always accepted"
        );
        let mut s = radio.take_streams().remove(0);
        for _ in 0..30 {
            let _ = tokio::time::timeout(WAIT, s.next_block()).await.expect("t");
        }
        let c = emu.counters();
        assert!(
            EmulatorCounters::get(&c.ep2_packets) > 10,
            "the client really sent C&C datagrams"
        );
        assert_eq!(EmulatorCounters::get(&c.mox_frames), 0);
        assert_eq!(EmulatorCounters::get(&c.drive_nonzero_frames), 0);
        assert_eq!(EmulatorCounters::get(&c.tx_iq_packets), 0);
        radio.stop().await.expect("stop");
        emu.stop();
    }

    #[tokio::test]
    async fn an_open_gate_keys_and_the_lease_expiry_unkeys() {
        // Tests [@ANCHOR: hpsdr_session_stop]
        let (emu, d) = setup(EmulatorConfig::default()).await;
        let gate = TxGate::new();
        let radio = Radio::start(
            &Handle::current(),
            &d,
            P1Config::default(),
            gate.clone(),
            SessionOptions::default(),
        )
        .await
        .expect("start");
        assert!(gate.open_until(Instant::now() + Duration::from_millis(300)));
        assert_eq!(radio.request_ptt(true, 100), Ok(()));
        assert_eq!(radio.send_tx_iq(&[(500, -500); 126]), Ok(()));
        assert_eq!(
            radio.send_tx_iq(&[(0, 0); crate::session::TX_QUEUE_MAX]),
            Err(HpsdrError::Config("transmit queue full".into()))
        );
        let c = emu.counters();
        assert!(
            wait_until(|| EmulatorCounters::get(&c.mox_frames) > 0
                && EmulatorCounters::get(&c.tx_iq_packets) > 0)
            .await
        );
        assert!(wait_until(|| EmulatorCounters::get(&c.drive_nonzero_frames) > 0).await);
        tokio::time::sleep(Duration::from_millis(500)).await;
        let frozen = EmulatorCounters::get(&c.mox_frames);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            EmulatorCounters::get(&c.mox_frames),
            frozen,
            "no MOX after the lease expired"
        );
        assert_eq!(radio.request_ptt(true, 100), Err(HpsdrError::GateClosed));
        radio.stop().await.expect("stop");
        emu.stop();
    }

    #[tokio::test]
    async fn lost_datagrams_and_a_slow_consumer_are_counted() {
        // Tests [@ANCHOR: hpsdr_emulator_start_p1]
        let ecfg = EmulatorConfig {
            skip_every: 4,
            pace: Duration::from_millis(1),
            ..Default::default()
        };
        let (emu, d) = setup(ecfg).await;
        let opts = SessionOptions {
            stream_depth: 2,
            ..Default::default()
        };
        let mut radio = Radio::start(
            &Handle::current(),
            &d,
            P1Config::default(),
            TxGate::new(),
            opts,
        )
        .await
        .expect("start");
        let mut s = radio.take_streams().remove(0);
        tokio::time::sleep(Duration::from_millis(400)).await;
        let st = s.stats();
        assert!(
            st.dropped_blocks > 0,
            "an unread stream drops, never blocks: {st:?}"
        );
        assert!(st.lost_packets > 0, "sequence gaps are counted: {st:?}");
        let mut lost_marked = false;
        for _ in 0..40 {
            let b = tokio::time::timeout(WAIT, s.next_block())
                .await
                .expect("t")
                .expect("open");
            lost_marked |= b.lost_before > 0;
        }
        assert!(lost_marked, "a block reports the datagrams lost before it");
        radio.stop().await.expect("stop");
        emu.stop();
    }
}

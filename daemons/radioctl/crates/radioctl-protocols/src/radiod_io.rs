// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bridge between the `radiod-channel` engine's line protocol ([`crate::radiod_net`]) and one
//! channel of a ka9q-radio `radiod`, behind an in-memory duplex stream the rig actor uses as its
//! transport. It owns the `radiod_client` control socket and the channel, keeps the channel alive,
//! and receives its RTP audio for [`RadiodControl::take_audio`].
//!
//! **The channel.** The bridge chooses nothing on the wire beyond `radiod_client`'s closed schema:
//! every command carries a finite lifetime ([`RadiodLink::lifetime_frames`]), and the channel is kept
//! by re-asserting its whole state (the `Audio` command, then the `Filter` command if a width was set)
//! every [`RadiodLink::refresh`]. Re-asserting rather than polling means a `radiod` that restarted gets
//! the channel back at the right frequency and mode, where a bare poll would have made one at 0 Hz.
//! When the actor goes away the channel is removed (lifetime 1); if the bridge itself dies, the
//! channel expires by its lifetime.
//!
//! **Receive only.** Nothing here can key anything: `radiod` has no transmit path and the line
//! protocol has no transmit request.
//!
//! **Audio.** The channel is asked for 16-bit PCM (12 kHz for USB, LSB, CW and AM; 24 kHz for FM).
//! A second task reads the data group, keeps the packets of this channel's SSRC whose payload type is
//! the one its status announced, decodes them to mono `f32` frames and offers them on a bounded queue;
//! a full queue drops the frame and counts it ([`RadiodStats::audio_dropped`]).
//!
//! **Front end.** At every refresh the bridge also reads the front end's status (SSRC 0), so the
//! overrange count and RF gain are at hand ([`RadiodControl::front_end`]).

use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::units::Mode;
use radiod_client::command::{AudioChannel, Command, Encoding, Mode as RadiodMode, Policy, Ssrc};
use radiod_client::net::{Control, RtpSocket, RTP_PORT, STATUS_PORT};
use radiod_client::rtp::{AudioDecoder, Encoding as RtpEncoding, RtpPacket, SeqTracker};
use radiod_client::status::ChannelStatus;
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};

use crate::radiod_net::{mode_from_name, signal_raw, ModeSpec};

/// Longest request line accepted from the engine.
const MAX_LINE: usize = 128;
/// Largest RTP datagram read.
const MAX_RTP: usize = 9000;

/// Where the channel lives and how it is kept. Build with [`RadiodLink::new`] and adjust fields.
#[derive(Debug, Clone, PartialEq)]
pub struct RadiodLink {
    /// `radiod`'s status (control) multicast group.
    pub status_group: Ipv4Addr,
    /// Its port (5006 by default).
    pub status_port: u16,
    /// The data (RTP) multicast group the channel's audio goes to (`radiod`'s `data =`).
    pub data_group: Ipv4Addr,
    /// Its port (5004 by default).
    pub data_port: u16,
    /// Interface address to join on (`127.0.0.1` for a loopback `radiod`).
    pub iface: Ipv4Addr,
    /// The channel's SSRC, chosen by the configuration; 1 to 0xfffffffe, and not used by any other
    /// client of the same `radiod` (two clients on one SSRC share one channel).
    pub ssrc: u32,
    /// Lifetime given with every command, 20 ms frames (default 500, 10 s).
    pub lifetime_frames: u32,
    /// How often the channel's state is re-asserted (default 2 s; must be well inside the lifetime).
    pub refresh: Duration,
    /// How long one command waits for `radiod`'s acknowledgement (default 1 s).
    pub ack_wait: Duration,
    /// Site limits on frequency and lifetime ([`Policy`]); frequencies outside are refused.
    pub policy: Policy,
    /// Starting dial frequency, Hz.
    pub frequency_hz: u64,
    /// Starting mode (USB, LSB, CW, AM or FM).
    pub mode: Mode,
    /// Audio frames queued for [`RadiodControl::take_audio`] before new ones are dropped (default 50,
    /// one second).
    pub audio_queue: usize,
}

impl RadiodLink {
    // [@ANCHOR: radioctl_radiod_link_new]
    /// A link with the default ports (5006 status, 5004 data), loopback interface, 10 s lifetime,
    /// 2 s refresh, 1 s acknowledgement wait, `Policy::default()` (10 kHz to 30 MHz), 10 MHz AM, and
    /// a one-second audio queue.
    pub fn new(status_group: Ipv4Addr, data_group: Ipv4Addr, ssrc: u32) -> RadiodLink {
        RadiodLink {
            status_group,
            status_port: STATUS_PORT,
            data_group,
            data_port: RTP_PORT,
            iface: Ipv4Addr::LOCALHOST,
            ssrc,
            lifetime_frames: 500,
            refresh: Duration::from_secs(2),
            ack_wait: Duration::from_secs(1),
            policy: Policy::default(),
            frequency_hz: 10_000_000,
            mode: Mode::Am,
            audio_queue: 50,
        }
    }

    // [@ANCHOR: radioctl_radiod_link_validate]
    /// Checks the link before anything is opened: a usable SSRC, a supported starting mode, a
    /// lifetime the policy allows, a refresh at most a third of the lifetime (so one lost refresh
    /// does not let the channel lapse), a non-zero acknowledgement wait and audio queue.
    pub fn validate(&self) -> Result<(), RigError> {
        let bad = |why: String| Err(RigError::new(ErrorKind::Unsupported, why));
        if Ssrc::new(self.ssrc).is_err() {
            return bad(format!(
                "radiod ssrc {} is reserved (0 and 0xffffffff)",
                self.ssrc
            ));
        }
        if crate::radiod_net::mode_name(self.mode).is_none() {
            return bad(format!(
                "radiod starting mode {:?} is not USB, LSB, CW, AM or FM",
                self.mode
            ));
        }
        if self.lifetime_frames == 0 || self.lifetime_frames > self.policy.max_lifetime_frames {
            return bad(format!(
                "radiod lifetime {} frames is outside 1..={}",
                self.lifetime_frames, self.policy.max_lifetime_frames
            ));
        }
        let lifetime = Duration::from_millis(u64::from(self.lifetime_frames) * 20);
        if self.refresh.is_zero() || self.refresh * 3 > lifetime {
            return bad(format!(
                "radiod refresh {:?} must be non-zero and at most a third of the {:?} lifetime",
                self.refresh, lifetime
            ));
        }
        if self.ack_wait.is_zero() || self.audio_queue == 0 {
            return bad("radiod ack_wait and audio_queue must be non-zero".to_string());
        }
        Ok(())
    }
}

/// One decoded RTP packet of the channel's audio.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFrame {
    /// Mono samples in -1.0..=1.0 (20 ms: 240 at 12 kHz, 480 at 24 kHz).
    pub samples: Vec<f32>,
    /// Sample rate, Hz, from the channel's status.
    pub rate_hz: u32,
    /// RTP sequence number.
    pub seq: u16,
    /// RTP timestamp, in samples.
    pub timestamp: u32,
}

/// Counters and the latest statuses, for diagnostics and the front-end overrange display.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RadiodStats {
    /// Commands `radiod` acknowledged.
    pub acks: u64,
    /// Commands that got no acknowledgement within `ack_wait`.
    pub ack_timeouts: u64,
    /// Commands refused by the client's schema before sending (out-of-policy values).
    pub refused: u64,
    /// Audio frames queued.
    pub audio_frames: u64,
    /// Audio frames dropped because the queue was full or nobody had taken it.
    pub audio_dropped: u64,
    /// RTP packets missing in sequence.
    pub rtp_lost: u64,
    /// RTP packets of this SSRC that did not decode (wrong payload type or size).
    pub rtp_bad: u64,
    /// The channel's latest status.
    pub channel: Option<ChannelStatus>,
    /// The front end's latest status (SSRC 0).
    pub front_end: Option<ChannelStatus>,
}

/// What the stream format of the channel is, as its status announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Format {
    pt: u8,
    encoding: RtpEncoding,
    channels: usize,
    rate_hz: u32,
}

struct Shared {
    stats: Mutex<RadiodStats>,
    format: Mutex<Option<Format>>,
    audio: Mutex<Option<mpsc::Receiver<AudioFrame>>>,
}

// [@ANCHOR: radioctl_radiod_lock]
/// Locks a mutex, taking the data of a poisoned one (a panicked holder leaves counters only).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Handle on a served bridge: the audio queue, counters and the latest statuses.
#[derive(Clone)]
pub struct RadiodControl {
    shared: Arc<Shared>,
}

impl RadiodControl {
    // [@ANCHOR: radioctl_radiod_control_take_audio]
    /// The channel's audio frames, once; `None` on later calls. Until it is taken, and whenever the
    /// receiver is not read fast enough, frames are dropped and counted, never buffered without end.
    pub fn take_audio(&self) -> Option<mpsc::Receiver<AudioFrame>> {
        lock(&self.shared.audio).take()
    }

    // [@ANCHOR: radioctl_radiod_control_stats]
    /// A copy of the counters and the latest channel and front-end statuses.
    pub fn stats(&self) -> RadiodStats {
        lock(&self.shared.stats).clone()
    }

    // [@ANCHOR: radioctl_radiod_control_front_end]
    /// The front end's latest status (SSRC 0: `ad_over`, `rf_gain_db`, `rf_atten_db`, `input_samprate`,
    /// ...), read at every refresh; `None` until `radiod` has answered once.
    pub fn front_end(&self) -> Option<ChannelStatus> {
        lock(&self.shared.stats).front_end.clone()
    }
}

// [@ANCHOR: radioctl_radiod_radiod_mode]
/// The client crate's mode for a radioctl mode the channel can demodulate.
fn radiod_mode(mode: Mode) -> Option<RadiodMode> {
    Some(match mode {
        Mode::Usb => RadiodMode::Usb,
        Mode::Lsb => RadiodMode::Lsb,
        Mode::Cw => RadiodMode::Cw,
        Mode::Am => RadiodMode::Am,
        Mode::Fm => RadiodMode::Fm,
        _ => return None,
    })
}

/// The channel as the engine last asked for it.
struct State {
    ssrc: Ssrc,
    freq: u64,
    mode: Mode,
    width: Option<u64>,
}

impl State {
    // [@ANCHOR: radioctl_radiod_state_commands]
    /// The commands that make `radiod`'s channel match this state: `Audio`, then `Filter` when a
    /// width was set. Every one carries `lifetime`.
    fn commands(&self, lifetime: u32) -> Vec<Command> {
        let mode = radiod_mode(self.mode).unwrap_or(RadiodMode::Usb);
        let mut out = vec![Command::Audio(AudioChannel {
            ssrc: self.ssrc,
            mode,
            frequency_hz: self.freq as f64,
            encoding: Encoding::Pcm16Be,
            lifetime_frames: lifetime,
        })];
        if let Some(w) = self.width {
            let (low_hz, high_hz) = ModeSpec::of(self.mode).edges(w);
            out.push(Command::Filter {
                ssrc: self.ssrc,
                low_hz,
                high_hz,
                lifetime_frames: lifetime,
            });
        }
        out
    }
}

/// The control side of the bridge: one task owns this.
struct Bridge {
    control: Control,
    link: RadiodLink,
    state: State,
    shared: Arc<Shared>,
}

impl Bridge {
    // [@ANCHOR: radioctl_radiod_bridge_send]
    /// Sends one command and waits for its acknowledgement; records the status and the stream
    /// format. `Err` carries the line-protocol error text.
    async fn send(&mut self, cmd: &Command) -> Result<ChannelStatus, String> {
        match self.control.request(cmd, self.link.ack_wait).await {
            Ok(Some(status)) => {
                let format = format_of(&status);
                let mut stats = lock(&self.shared.stats);
                stats.acks += 1;
                stats.channel = Some(status.clone());
                drop(stats);
                if format.is_some() {
                    *lock(&self.shared.format) = format;
                }
                Ok(status)
            }
            Ok(None) => {
                lock(&self.shared.stats).ack_timeouts += 1;
                Err("radiod did not answer".to_string())
            }
            Err(radiod_client::net::NetError::Command(e)) => {
                lock(&self.shared.stats).refused += 1;
                Err(format!("refused: {e}"))
            }
            Err(e) => Err(format!("{e}")),
        }
    }

    // [@ANCHOR: radioctl_radiod_bridge_assert]
    /// Makes `radiod`'s channel match the state (creating it if `radiod` has none).
    async fn assert(&mut self) -> Result<ChannelStatus, String> {
        let mut last = Err("nothing sent".to_string());
        for cmd in self.state.commands(self.link.lifetime_frames) {
            last = self.send(&cmd).await;
            last.as_ref().map_err(Clone::clone)?;
        }
        last
    }

    // [@ANCHOR: radioctl_radiod_bridge_refresh]
    /// The periodic keep-alive: re-assert the channel, then read the front end.
    async fn refresh(&mut self) {
        let _ = self.assert().await;
        if let Ok(Some(fe)) = self.control.front_end(self.link.ack_wait).await {
            lock(&self.shared.stats).front_end = Some(fe);
        }
    }

    // [@ANCHOR: radioctl_radiod_bridge_handle_line]
    /// Answers one request line of the engine's protocol (module note of [`crate::radiod_net`]).
    async fn handle_line(&mut self, line: &str) -> String {
        let mut parts = line.split_whitespace();
        let head = parts.next();
        let arg = parts.next();
        let number = arg.and_then(|v| v.parse::<u64>().ok());
        let last = lock(&self.shared.stats).channel.clone();
        match (head, number) {
            (Some("GF"), _) => {
                let hz = last
                    .and_then(|s| s.frequency_hz)
                    .filter(|f| f.is_finite() && *f >= 0.0)
                    .map_or(self.state.freq, |f| f.round() as u64);
                format!("F {hz}\n")
            }
            (Some("GM"), _) => {
                format!("M {}\n", ModeSpec::of(self.state.mode).name)
            }
            (Some("GW"), _) => {
                let width = last
                    .and_then(|s| Some(s.high_edge_hz? - s.low_edge_hz?))
                    .filter(|w| w.is_finite() && *w > 0.0)
                    .map_or_else(
                        || {
                            self.state
                                .width
                                .unwrap_or(ModeSpec::of(self.state.mode).default_width_hz)
                        },
                        |w| w.round() as u64,
                    );
                format!("W {width}\n")
            }
            (Some("GS"), _) => {
                let poll = Command::Poll {
                    ssrc: self.state.ssrc,
                    lifetime_frames: self.link.lifetime_frames,
                };
                match self.send(&poll).await {
                    Ok(s) => match s.baseband_power_db {
                        Some(db) => format!("S {}\n", signal_raw(db)),
                        None => "ERR no baseband power in the status\n".to_string(),
                    },
                    Err(e) => format!("ERR {e}\n"),
                }
            }
            (Some("F"), Some(hz)) => {
                let before = self.state.freq;
                self.state.freq = hz;
                match self.assert().await {
                    Ok(_) => "OK\n".to_string(),
                    Err(e) => {
                        self.state.freq = before;
                        format!("ERR {e}\n")
                    }
                }
            }
            (Some("M"), _) => match arg.and_then(mode_from_name) {
                Some(mode) => {
                    let before = (self.state.mode, self.state.width);
                    self.state.mode = mode;
                    self.state.width = None;
                    match self.assert().await {
                        Ok(_) => "OK\n".to_string(),
                        Err(e) => {
                            (self.state.mode, self.state.width) = before;
                            format!("ERR {e}\n")
                        }
                    }
                }
                None => "ERR mode\n".to_string(),
            },
            (Some("W"), Some(hz)) if (1..=24_000).contains(&hz) => {
                let before = self.state.width;
                self.state.width = Some(hz);
                match self.assert().await {
                    Ok(_) => "OK\n".to_string(),
                    Err(e) => {
                        self.state.width = before;
                        format!("ERR {e}\n")
                    }
                }
            }
            _ => "ERR command\n".to_string(),
        }
    }
}

// [@ANCHOR: radioctl_radiod_format_of]
/// The stream format a channel status announces, if it is complete and decodable as PCM.
fn format_of(s: &ChannelStatus) -> Option<Format> {
    let encoding = RtpEncoding::from_radiod(s.encoding?)?;
    if encoding == RtpEncoding::Opus {
        return None;
    }
    Some(Format {
        pt: u8::try_from(s.rtp_pt?).ok()?,
        encoding,
        channels: usize::try_from(s.channels?)
            .ok()
            .filter(|c| (1..=2).contains(c))?,
        rate_hz: u32::try_from(s.samprate?).ok().filter(|r| *r > 0)?,
    })
}

// [@ANCHOR: radioctl_radiod_serve]
/// Opens the sockets for `link`, serves the channel behind an in-memory stream for the rig actor,
/// and returns the stream and the control handle. Two tasks are spawned on `runtime`: the control
/// task (line protocol, keep-alive, front-end reads) and the audio task. When the actor drops its
/// end of the stream, the channel is removed and both tasks end.
pub fn serve_radiod(
    runtime: &Handle,
    link: RadiodLink,
) -> Result<(DuplexStream, RadiodControl), RigError> {
    link.validate()?;
    let net = |e: radiod_client::net::NetError| {
        RigError::new(ErrorKind::LinkDown, format!("radiod sockets: {e}"))
    };
    let control = Control::open_on_port(
        runtime,
        link.status_group,
        link.status_port,
        link.iface,
        link.policy.clone(),
    )
    .map_err(net)?;
    let rtp = RtpSocket::open_on_port(runtime, link.data_group, link.data_port, link.iface)
        .map_err(net)?;
    let ssrc = Ssrc::new(link.ssrc)
        .map_err(|e| RigError::new(ErrorKind::Unsupported, format!("radiod ssrc: {e}")))?;
    let (tx, rx) = mpsc::channel(link.audio_queue);
    let shared = Arc::new(Shared {
        stats: Mutex::new(RadiodStats::default()),
        format: Mutex::new(None),
        audio: Mutex::new(Some(rx)),
    });
    let (near, far) = duplex(4096);
    let (stop_tx, stop_rx) = watch::channel(false);
    let bridge = Bridge {
        control,
        state: State {
            ssrc,
            freq: link.frequency_hz,
            mode: link.mode,
            width: None,
        },
        link,
        shared: Arc::clone(&shared),
    };
    runtime.spawn(run_control(bridge, far, stop_tx));
    runtime.spawn(run_audio(rtp, ssrc.get(), Arc::clone(&shared), tx, stop_rx));
    Ok((near, RadiodControl { shared }))
}

// [@ANCHOR: radioctl_radiod_run_control]
/// The control task: create the channel, then answer lines and refresh until the actor goes away;
/// then remove the channel and stop the audio task.
async fn run_control(mut bridge: Bridge, mut far: DuplexStream, stop: watch::Sender<bool>) {
    let _ = bridge.assert().await;
    let mut tick = tokio::time::interval(bridge.link.refresh);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick.tick().await;
    let (mut buf, mut line, mut discarding) = ([0u8; 256], Vec::<u8>::new(), false);
    'serve: loop {
        tokio::select! {
            read = far.read(&mut buf) => {
                let n = match read {
                    Ok(0) | Err(_) => break 'serve,
                    Ok(n) => n,
                };
                for &b in buf.get(..n).unwrap_or(&[]) {
                    if b == b'\n' {
                        if !discarding {
                            let text = String::from_utf8_lossy(&line).into_owned();
                            let reply = bridge.handle_line(text.trim()).await;
                            if far.write_all(reply.as_bytes()).await.is_err() {
                                break 'serve;
                            }
                        }
                        line.clear();
                        discarding = false;
                    } else if line.len() >= MAX_LINE {
                        line.clear();
                        discarding = true;
                    } else if !discarding {
                        line.push(b);
                    }
                }
            }
            _ = tick.tick() => bridge.refresh().await,
        }
    }
    let _ = stop.send(true);
    let remove = Command::Remove(bridge.state.ssrc);
    let _ = bridge.control.request(&remove, bridge.link.ack_wait).await;
}

// [@ANCHOR: radioctl_radiod_run_audio]
/// The audio task: decode this channel's RTP packets into the queue until told to stop.
async fn run_audio(
    rtp: RtpSocket,
    ssrc: u32,
    shared: Arc<Shared>,
    tx: mpsc::Sender<AudioFrame>,
    mut stop: watch::Receiver<bool>,
) {
    let mut buf = vec![0u8; MAX_RTP];
    let mut seq = SeqTracker::default();
    let mut decoder: Option<(Format, AudioDecoder)> = None;
    loop {
        let received = tokio::select! {
            _ = stop.changed() => break,
            r = rtp.recv(&mut buf, Duration::from_millis(500)) => r,
        };
        let n = match received {
            Ok(Some(n)) => n,
            Ok(None) => continue,
            Err(_) => break,
        };
        let Ok(pkt) = RtpPacket::parse(buf.get(..n).unwrap_or(&[])) else {
            continue;
        };
        if pkt.ssrc != ssrc {
            continue;
        }
        let Some(format) = *lock(&shared.format) else {
            continue;
        };
        let bad = |shared: &Shared| lock(&shared.stats).rtp_bad += 1;
        if pkt.pt != format.pt {
            bad(&shared);
            continue;
        }
        if decoder.as_ref().is_none_or(|(f, _)| *f != format) {
            match AudioDecoder::new(format.encoding, format.channels) {
                Ok(d) => decoder = Some((format, d)),
                Err(_) => {
                    bad(&shared);
                    continue;
                }
            }
        }
        let Some((_, dec)) = decoder.as_mut() else {
            continue;
        };
        let Ok(samples) = dec.decode(pkt.payload) else {
            bad(&shared);
            continue;
        };
        seq.push(pkt.seq);
        let frame = AudioFrame {
            samples,
            rate_hz: format.rate_hz,
            seq: pkt.seq,
            timestamp: pkt.timestamp,
        };
        let queued = tx.try_send(frame).is_ok();
        let mut stats = lock(&shared.stats);
        stats.rtp_lost = seq.lost;
        if queued {
            stats.audio_frames += 1;
        } else {
            stats.audio_dropped += 1;
        }
    }
}

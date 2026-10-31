// SPDX-License-Identifier: AGPL-3.0-or-later
//! Network rig links: a TCP or UDP connection that keeps itself alive, behind the same
//! [`crate::transport::Transport`] the rig actor uses for a serial port.
//!
//! A radio on Ethernet (a network serial server, a FlexRadio on port 4992, a Kenwood or Icom
//! radio with a LAN port, `rigctld`) is reached through a [`NetLink`]. A supervisor task owns the
//! real socket and hands the actor an in-memory byte stream that stays open across reconnects, just
//! as the serial link does, and publishes a [`LinkStatus`] watch for
//! [`crate::rig::RigOptions::link_watch`]. That is the whole reconnect story: when the socket is
//! lost the status becomes `Reconnecting` (the actor refuses commands with `LinkDown`), the
//! supervisor reconnects with exponential backoff and, once the session is ready again, publishes
//! `Connected` with a higher generation. The actor then unkeys the rig before serving any other
//! command, because the rig may have been keyed when the network dropped. Nothing here keys a rig.
//!
//! Every wait is bounded: connect, handshake, each write, and (when configured) silence on the
//! wire. Bytes the actor writes while the link is down or still handshaking are dropped and
//! counted, never queued: a stale command must not be replayed into a rig after a reconnect.
//!
//! Radios that wrap CAT in their own framing or want a login (Kenwood TS-890S LAN, Icom network
//! control) are served by the [`Framing`] trait: sans-IO hooks for a greeting sent at every
//! connect, wire framing in both directions, a handshake that must complete before the link is
//! declared up, and a keep-alive. [`RawFraming`] is the identity and [`LengthFraming`] the
//! common length-prefixed form. No manufacturer protocol is implemented here: the vendors'
//! network manuals are not all public, so each radio's framing is added as its own engine once its
//! documentation is, and until then only these hooks exist.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{duplex, split, AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::net::{lookup_host, TcpStream, UdpSocket};
use tokio::runtime::Handle;
use tokio::sync::{oneshot, watch};
use tokio::time::{sleep, timeout, Instant};

use crate::backoff::Backoff;
use crate::error::{ErrorKind, RigError};
use crate::serial::{LinkStatus, SerialStats};

const STREAM_BYTES: usize = 4096;
const TCP_READ: usize = 4096;
const UDP_READ: usize = 65_536;
/// The largest frame a [`LengthFraming`] accepts; a longer one is a protocol error, not a reason
/// to allocate.
pub const MAX_FRAME: usize = 1 << 20;

/// Counters of a network link (the same shape as a serial link's).
pub type NetStats = SerialStats;

/// Which transport protocol the link uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetProtocol {
    /// A TCP connection, reconnected when it is lost.
    Tcp,
    /// A connected UDP socket: one datagram per write and per read. UDP has no connection to
    /// lose, so loss is detected by an ICMP error from the stack or by silence
    /// ([`NetConfig::idle_timeout`]), and "reconnect" means opening a fresh socket and
    /// repeating the greeting.
    Udp,
}

/// How to reach a rig over the network and keep the link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetConfig {
    /// `host:port`. A name is resolved again at every connect.
    pub address: String,
    /// TCP or UDP.
    pub protocol: NetProtocol,
    /// Longest wait for one connection attempt.
    pub connect_timeout: Duration,
    /// Longest wait for the framing's handshake to finish after the socket is up.
    pub handshake_timeout: Duration,
    /// Longest wait for one write to the socket.
    pub write_timeout: Duration,
    /// Silence on the wire after which the link is declared lost (`None`: never). Set it with a
    /// keep-alive, or a quiet rig is reconnected for nothing. Required for UDP loss detection.
    pub idle_timeout: Option<Duration>,
    /// Waits between reconnect attempts.
    pub backoff: Backoff,
    /// Attempts for the first connection (at least one); a failure after that is an error to the
    /// caller. Later losses are retried without limit.
    pub first_attempts: u32,
}

impl NetConfig {
    // [@ANCHOR: radioctl_core_net_config_new]
    /// TCP to `address`, three-second connect, handshake and write limits, no idle limit, the
    /// default backoff and one first attempt.
    pub fn new(address: impl Into<String>) -> NetConfig {
        NetConfig {
            address: address.into(),
            protocol: NetProtocol::Tcp,
            connect_timeout: Duration::from_secs(3),
            handshake_timeout: Duration::from_secs(3),
            write_timeout: Duration::from_secs(3),
            idle_timeout: None,
            backoff: Backoff::default(),
            first_attempts: 1,
        }
    }
}

/// What a framing produced from bytes read off the wire.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Decoded {
    /// Bytes for the rig actor (the CAT stream).
    pub payload: Vec<u8>,
    /// Bytes to send straight back to the radio (a login answer, a pong).
    pub reply: Vec<u8>,
    /// The handshake is complete: the link may be declared up.
    pub ready: bool,
    /// The handshake failed or the stream is corrupt: drop the connection and reconnect.
    pub failure: Option<String>,
}

/// Sans-IO hooks for a radio that wraps its CAT stream in a framing or a login.
pub trait Framing: Send + Sync + 'static {
    /// Forgets all state; called at the start of every connection.
    fn reset(&mut self);
    /// Bytes sent first at every connection (a greeting or login request), empty for none.
    fn greeting(&mut self) -> Vec<u8>;
    /// Whether the link is up only after the radio answers (a login). When false the link is up
    /// as soon as the greeting is sent.
    fn needs_handshake(&self) -> bool;
    /// Wraps bytes from the actor for the wire.
    fn encode(&mut self, payload: &[u8]) -> Vec<u8>;
    /// Unwraps bytes read from the wire; may be called with a partial frame.
    fn decode(&mut self, wire: &[u8]) -> Decoded;
    /// A keep-alive: how often to send it and what. `None` for no keep-alive.
    fn keepalive(&self) -> Option<(Duration, Vec<u8>)>;
}

/// No framing: the CAT stream is the wire stream, nothing is greeted, nothing is kept alive.
#[derive(Debug, Clone, Copy, Default)]
pub struct RawFraming;

impl Framing for RawFraming {
    // [@ANCHOR: radioctl_core_net_raw_reset]
    fn reset(&mut self) {}

    // [@ANCHOR: radioctl_core_net_raw_greeting]
    fn greeting(&mut self) -> Vec<u8> {
        Vec::new()
    }

    // [@ANCHOR: radioctl_core_net_raw_needs_handshake]
    fn needs_handshake(&self) -> bool {
        false
    }

    // [@ANCHOR: radioctl_core_net_raw_encode]
    fn encode(&mut self, payload: &[u8]) -> Vec<u8> {
        payload.to_vec()
    }

    // [@ANCHOR: radioctl_core_net_raw_decode]
    fn decode(&mut self, wire: &[u8]) -> Decoded {
        Decoded {
            payload: wire.to_vec(),
            ..Decoded::default()
        }
    }

    // [@ANCHOR: radioctl_core_net_raw_keepalive]
    fn keepalive(&self) -> Option<(Duration, Vec<u8>)> {
        None
    }
}

/// Frames of a 2 or 4 byte big-endian length followed by that many bytes. Each write from the
/// actor becomes one frame; the payload of every complete frame read is passed up. A length over
/// [`MAX_FRAME`] is a failure (the stream is out of step), never an allocation.
#[derive(Debug, Clone)]
pub struct LengthFraming {
    width: usize,
    pending: Vec<u8>,
}

impl LengthFraming {
    // [@ANCHOR: radioctl_core_net_length_new]
    /// A framing with a length field of `width` bytes: 2 or 4 (anything else is made 4).
    pub fn new(width: usize) -> LengthFraming {
        LengthFraming {
            width: if width == 2 { 2 } else { 4 },
            pending: Vec::new(),
        }
    }
}

impl Framing for LengthFraming {
    // [@ANCHOR: radioctl_core_net_length_reset]
    fn reset(&mut self) {
        self.pending.clear();
    }

    // [@ANCHOR: radioctl_core_net_length_greeting]
    fn greeting(&mut self) -> Vec<u8> {
        Vec::new()
    }

    // [@ANCHOR: radioctl_core_net_length_needs_handshake]
    fn needs_handshake(&self) -> bool {
        false
    }

    // [@ANCHOR: radioctl_core_net_length_encode]
    fn encode(&mut self, payload: &[u8]) -> Vec<u8> {
        let len = payload.len();
        let mut out = Vec::with_capacity(self.width + len);
        if self.width == 2 {
            out.extend_from_slice(&u16::try_from(len).unwrap_or(u16::MAX).to_be_bytes());
        } else {
            out.extend_from_slice(&u32::try_from(len).unwrap_or(u32::MAX).to_be_bytes());
        }
        out.extend_from_slice(payload);
        out
    }

    // [@ANCHOR: radioctl_core_net_length_decode]
    fn decode(&mut self, wire: &[u8]) -> Decoded {
        self.pending.extend_from_slice(wire);
        let mut out = Decoded::default();
        while let Some(head) = self.pending.get(..self.width) {
            let len = if self.width == 2 {
                usize::from(u16::from_be_bytes([
                    head.first().copied().unwrap_or(0),
                    head.get(1).copied().unwrap_or(0),
                ]))
            } else {
                u32::from_be_bytes([
                    head.first().copied().unwrap_or(0),
                    head.get(1).copied().unwrap_or(0),
                    head.get(2).copied().unwrap_or(0),
                    head.get(3).copied().unwrap_or(0),
                ]) as usize
            };
            if len > MAX_FRAME {
                out.failure = Some(format!("frame of {len} bytes is over the limit"));
                self.pending.clear();
                break;
            }
            let end = self.width + len;
            let Some(body) = self.pending.get(self.width..end) else {
                break;
            };
            out.payload.extend_from_slice(body);
            self.pending.drain(..end);
        }
        out
    }

    // [@ANCHOR: radioctl_core_net_length_keepalive]
    fn keepalive(&self) -> Option<(Duration, Vec<u8>)> {
        None
    }
}

#[derive(Default)]
struct Counters {
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
    bytes_dropped: AtomicU64,
    reconnects: AtomicU64,
}

// [@ANCHOR: radioctl_core_net_snapshot]
fn snapshot(counters: &Counters) -> NetStats {
    NetStats {
        bytes_in: counters.bytes_in.load(Ordering::Relaxed),
        bytes_out: counters.bytes_out.load(Ordering::Relaxed),
        bytes_dropped: counters.bytes_dropped.load(Ordering::Relaxed),
        reconnects: counters.reconnects.load(Ordering::Relaxed),
    }
}

/// A running network link: the byte stream for the actor and the link status.
pub struct NetLink {
    /// The byte stream to hand to [`crate::rig::spawn`]. Dropping it closes the socket.
    pub transport: DuplexStream,
    status: watch::Receiver<LinkStatus>,
    counters: Arc<Counters>,
}

impl NetLink {
    // [@ANCHOR: radioctl_core_net_link_status]
    /// The current link status.
    pub fn status(&self) -> LinkStatus {
        self.status.borrow().clone()
    }

    // [@ANCHOR: radioctl_core_net_link_watch]
    /// A receiver that wakes on every status change; pass it as `RigOptions::link_watch`.
    pub fn watch(&self) -> watch::Receiver<LinkStatus> {
        self.status.clone()
    }

    // [@ANCHOR: radioctl_core_net_link_stats]
    /// A snapshot of the counters.
    pub fn stats(&self) -> NetStats {
        snapshot(&self.counters)
    }

    // [@ANCHOR: radioctl_core_net_link_into_parts]
    /// Splits the link into the byte stream (for [`crate::rig::spawn`]) and the monitor the
    /// owner keeps. Dropping the monitor does not close the socket; dropping the stream does.
    pub fn into_parts(self) -> (DuplexStream, NetMonitor) {
        (
            self.transport,
            NetMonitor {
                status: self.status,
                counters: self.counters,
            },
        )
    }
}

/// The part of a [`NetLink`] that stays with the owner: the link status and the counters.
pub struct NetMonitor {
    status: watch::Receiver<LinkStatus>,
    counters: Arc<Counters>,
}

impl NetMonitor {
    // [@ANCHOR: radioctl_core_net_monitor_status]
    /// The current link status.
    pub fn status(&self) -> LinkStatus {
        self.status.borrow().clone()
    }

    // [@ANCHOR: radioctl_core_net_monitor_watch]
    /// A receiver that wakes on every status change.
    pub fn watch(&self) -> watch::Receiver<LinkStatus> {
        self.status.clone()
    }

    // [@ANCHOR: radioctl_core_net_monitor_stats]
    /// A snapshot of the counters.
    pub fn stats(&self) -> NetStats {
        snapshot(&self.counters)
    }
}

// [@ANCHOR: radioctl_core_net_error]
fn net_error(detail: impl Into<String>) -> RigError {
    RigError::new(ErrorKind::LinkDown, detail)
}

/// The open socket of one session.
enum Wire {
    Tcp(TcpStream),
    Udp(UdpSocket),
}

impl Wire {
    // [@ANCHOR: radioctl_core_net_wire_open]
    /// Resolves the address and opens a socket, within `limit`.
    async fn open(config: &NetConfig) -> Result<Wire, String> {
        let opening = async {
            let addr: SocketAddr = lookup_host(config.address.as_str())
                .await
                .map_err(|e| format!("resolve {}: {e}", config.address))?
                .next()
                .ok_or_else(|| format!("{} has no address", config.address))?;
            match config.protocol {
                NetProtocol::Tcp => {
                    let stream = TcpStream::connect(addr)
                        .await
                        .map_err(|e| format!("connect {addr}: {e}"))?;
                    stream
                        .set_nodelay(true)
                        .map_err(|e| format!("set_nodelay: {e}"))?;
                    Ok(Wire::Tcp(stream))
                }
                NetProtocol::Udp => {
                    let local: SocketAddr = if addr.is_ipv4() {
                        SocketAddr::from(([0, 0, 0, 0], 0))
                    } else {
                        SocketAddr::from((std::net::Ipv6Addr::UNSPECIFIED, 0))
                    };
                    let socket = UdpSocket::bind(local)
                        .await
                        .map_err(|e| format!("bind udp: {e}"))?;
                    socket
                        .connect(addr)
                        .await
                        .map_err(|e| format!("connect udp {addr}: {e}"))?;
                    Ok(Wire::Udp(socket))
                }
            }
        };
        match timeout(config.connect_timeout, opening).await {
            Ok(result) => result,
            Err(_) => Err(format!("no answer within {:?}", config.connect_timeout)),
        }
    }

    // [@ANCHOR: radioctl_core_net_wire_recv]
    /// Reads what is available: a chunk of a TCP stream, or one datagram. `Ok(0)` is a closed
    /// TCP connection.
    async fn recv(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Wire::Tcp(stream) => stream.read(buffer).await,
            Wire::Udp(socket) => socket.recv(buffer).await,
        }
    }

    // [@ANCHOR: radioctl_core_net_wire_send]
    /// Writes all of `bytes` (TCP) or sends them as one datagram (UDP).
    async fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            Wire::Tcp(stream) => stream.write_all(bytes).await,
            Wire::Udp(socket) => socket.send(bytes).await.map(|_| ()),
        }
    }
}

/// Why a session ended.
enum SessionEnd {
    /// The actor dropped its end: stop for good.
    Shutdown,
    /// The wire failed: reconnect.
    Lost(String),
}

struct Supervisor {
    config: NetConfig,
    framing: Box<dyn Framing>,
    status: watch::Sender<LinkStatus>,
    counters: Arc<Counters>,
}

// [@ANCHOR: radioctl_core_net_sleep_until]
/// Resolves at `at`, or never when there is no deadline.
async fn sleep_until_opt(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending::<()>().await,
    }
}

impl Supervisor {
    // [@ANCHOR: radioctl_core_net_supervisor_run]
    /// The supervisor task: first connection, then sessions and reconnects until the actor's end
    /// of the stream is dropped.
    async fn run(mut self, ours: DuplexStream, first: oneshot::Sender<Result<(), RigError>>) {
        let (mut from_actor, mut to_actor) = split(ours);
        let mut first = Some(first);
        let attempts = self.config.first_attempts.max(1);
        let mut failures = 0u32;
        let mut generation = 0u32;
        loop {
            // One attempt: open, greet, handshake. Bytes the actor writes meanwhile are dropped.
            let attempt = self.attempt(&mut from_actor);
            let last = match attempt.await {
                Ok((wire, leftover)) => {
                    failures = 0;
                    generation += 1;
                    if generation > 1 {
                        self.counters.reconnects.fetch_add(1, Ordering::Relaxed);
                    }
                    let _ = self.status.send(LinkStatus::Connected {
                        port: format!(
                            "{}://{}",
                            proto_name(self.config.protocol),
                            self.config.address
                        )
                        .into(),
                        generation,
                    });
                    if let Some(first) = first.take() {
                        let _ = first.send(Ok(()));
                    }
                    match self
                        .session(wire, leftover, &mut from_actor, &mut to_actor)
                        .await
                    {
                        SessionEnd::Shutdown => break,
                        SessionEnd::Lost(why) => why,
                    }
                }
                Err(AttemptEnd::Shutdown) => break,
                Err(AttemptEnd::Failed(why)) => why,
            };
            failures = failures.saturating_add(1);
            if first.is_some() && failures >= attempts {
                if let Some(first) = first.take() {
                    let _ = first.send(Err(net_error(format!(
                        "{} connect failed after {failures} attempts: {last}",
                        proto_name(self.config.protocol)
                    ))));
                }
                break;
            }
            let _ = self.status.send(LinkStatus::Reconnecting {
                attempt: failures,
                last_error: last.clone(),
            });
            if !self
                .sleep_dropping(&mut from_actor, self.config.backoff.delay(failures))
                .await
            {
                break;
            }
        }
        let _ = self.status.send(LinkStatus::Closed);
    }

    // [@ANCHOR: radioctl_core_net_sleep_dropping]
    /// Waits `delay`, dropping (and counting) bytes the actor writes meanwhile. Returns false if
    /// the actor dropped its end of the stream.
    async fn sleep_dropping(
        &self,
        from_actor: &mut tokio::io::ReadHalf<DuplexStream>,
        delay: Duration,
    ) -> bool {
        let until = Instant::now() + delay;
        let mut buffer = [0u8; 512];
        loop {
            tokio::select! {
                _ = sleep(until.saturating_duration_since(Instant::now())) => return true,
                read = from_actor.read(&mut buffer) => match read {
                    Ok(0) | Err(_) => return false,
                    Ok(n) => {
                        self.counters.bytes_dropped.fetch_add(n as u64, Ordering::Relaxed);
                    }
                },
            }
        }
    }

    // [@ANCHOR: radioctl_core_net_attempt]
    /// Opens the socket, sends the greeting and, when the framing needs it, waits for the
    /// handshake. Returns the wire and any payload that arrived with the handshake's last bytes.
    /// While this runs, actor writes are dropped and counted.
    async fn attempt(
        &mut self,
        from_actor: &mut tokio::io::ReadHalf<DuplexStream>,
    ) -> Result<(Wire, Vec<u8>), AttemptEnd> {
        let mut dropped = 0u64;
        let result = {
            let work = self.open_and_handshake();
            tokio::pin!(work);
            let mut buffer = [0u8; 512];
            loop {
                tokio::select! {
                    done = &mut work => break done,
                    read = from_actor.read(&mut buffer) => match read {
                        Ok(0) | Err(_) => return Err(AttemptEnd::Shutdown),
                        Ok(n) => dropped += n as u64,
                    },
                }
            }
        };
        self.counters
            .bytes_dropped
            .fetch_add(dropped, Ordering::Relaxed);
        result.map_err(AttemptEnd::Failed)
    }

    // [@ANCHOR: radioctl_core_net_open_and_handshake]
    /// The cancel-safe part of an attempt: socket, greeting, handshake.
    async fn open_and_handshake(&mut self) -> Result<(Wire, Vec<u8>), String> {
        let mut wire = Wire::open(&self.config).await?;
        self.framing.reset();
        let greeting = self.framing.greeting();
        if !greeting.is_empty() {
            self.send(&mut wire, &greeting).await?;
        }
        if !self.framing.needs_handshake() {
            return Ok((wire, Vec::new()));
        }
        let deadline = Instant::now() + self.config.handshake_timeout;
        let mut buffer = vec![0u8; read_size(self.config.protocol)];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let n = match timeout(left, wire.recv(&mut buffer)).await {
                Err(_) => return Err("handshake timed out".into()),
                Ok(Err(e)) => return Err(format!("handshake read: {e}")),
                Ok(Ok(0)) => return Err("closed during handshake".into()),
                Ok(Ok(n)) => n,
            };
            let decoded = self.framing.decode(buffer.get(..n).unwrap_or(&[]));
            if let Some(why) = decoded.failure {
                return Err(format!("handshake failed: {why}"));
            }
            if !decoded.reply.is_empty() {
                self.send(&mut wire, &decoded.reply).await?;
            }
            if decoded.ready {
                return Ok((wire, decoded.payload));
            }
        }
    }

    // [@ANCHOR: radioctl_core_net_send]
    /// One bounded write to the socket.
    async fn send(&self, wire: &mut Wire, bytes: &[u8]) -> Result<(), String> {
        match timeout(self.config.write_timeout, wire.send(bytes)).await {
            Err(_) => Err(format!(
                "write timed out after {:?}",
                self.config.write_timeout
            )),
            Ok(Err(e)) => Err(format!("write: {e}")),
            Ok(Ok(())) => {
                self.counters
                    .bytes_out
                    .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                Ok(())
            }
        }
    }

    // [@ANCHOR: radioctl_core_net_session]
    /// One ready connection: wire to actor, actor to wire, keep-alive and silence limit.
    async fn session(
        &mut self,
        mut wire: Wire,
        leftover: Vec<u8>,
        from_actor: &mut tokio::io::ReadHalf<DuplexStream>,
        to_actor: &mut tokio::io::WriteHalf<DuplexStream>,
    ) -> SessionEnd {
        if !leftover.is_empty() && !self.deliver(to_actor, &leftover).await {
            return SessionEnd::Shutdown;
        }
        let keepalive = self.framing.keepalive();
        let mut next_keepalive = keepalive.as_ref().map(|(every, _)| Instant::now() + *every);
        let mut silent_until = self.config.idle_timeout.map(|t| Instant::now() + t);
        let mut wire_buffer = vec![0u8; read_size(self.config.protocol)];
        let mut actor_buffer = [0u8; TCP_READ];
        loop {
            tokio::select! {
                read = wire.recv(&mut wire_buffer) => {
                    let n = match read {
                        Ok(0) => return SessionEnd::Lost("connection closed by the radio".into()),
                        Ok(n) => n,
                        Err(e) => return SessionEnd::Lost(format!("read: {e}")),
                    };
                    self.counters.bytes_in.fetch_add(n as u64, Ordering::Relaxed);
                    silent_until = self.config.idle_timeout.map(|t| Instant::now() + t);
                    let decoded = self.framing.decode(wire_buffer.get(..n).unwrap_or(&[]));
                    if let Some(why) = decoded.failure {
                        return SessionEnd::Lost(why);
                    }
                    if !decoded.reply.is_empty() {
                        if let Err(why) = self.send(&mut wire, &decoded.reply).await {
                            return SessionEnd::Lost(why);
                        }
                    }
                    if !decoded.payload.is_empty() && !self.deliver(to_actor, &decoded.payload).await {
                        return SessionEnd::Shutdown;
                    }
                }
                read = from_actor.read(&mut actor_buffer) => {
                    let n = match read {
                        Ok(0) | Err(_) => return SessionEnd::Shutdown,
                        Ok(n) => n,
                    };
                    let framed = self.framing.encode(actor_buffer.get(..n).unwrap_or(&[]));
                    if let Err(why) = self.send(&mut wire, &framed).await {
                        // The write that found the link dead is lost with it.
                        self.counters.bytes_dropped.fetch_add(n as u64, Ordering::Relaxed);
                        return SessionEnd::Lost(why);
                    }
                }
                () = sleep_until_opt(next_keepalive) => {
                    if let Some((every, probe)) = keepalive.as_ref() {
                        if let Err(why) = self.send(&mut wire, probe).await {
                            return SessionEnd::Lost(why);
                        }
                        next_keepalive = Some(Instant::now() + *every);
                    }
                }
                () = sleep_until_opt(silent_until) => {
                    return SessionEnd::Lost(format!(
                        "nothing heard for {:?}",
                        self.config.idle_timeout.unwrap_or_default()
                    ));
                }
            }
        }
    }

    // [@ANCHOR: radioctl_core_net_deliver]
    /// Writes bytes to the actor's stream, bounded. A stream nobody reads is dropped from (and
    /// counted) rather than waited on; `false` means the actor's end is gone.
    async fn deliver(
        &self,
        to_actor: &mut tokio::io::WriteHalf<DuplexStream>,
        bytes: &[u8],
    ) -> bool {
        match timeout(self.config.write_timeout, to_actor.write_all(bytes)).await {
            Ok(Ok(())) => true,
            Ok(Err(_)) => false,
            Err(_) => {
                self.counters
                    .bytes_dropped
                    .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                true
            }
        }
    }
}

/// How an attempt ended without a wire.
enum AttemptEnd {
    /// The actor dropped its end: stop for good.
    Shutdown,
    /// The attempt failed; the text says why.
    Failed(String),
}

// [@ANCHOR: radioctl_core_net_proto_name]
fn proto_name(protocol: NetProtocol) -> &'static str {
    match protocol {
        NetProtocol::Tcp => "tcp",
        NetProtocol::Udp => "udp",
    }
}

// [@ANCHOR: radioctl_core_net_read_size]
fn read_size(protocol: NetProtocol) -> usize {
    match protocol {
        NetProtocol::Tcp => TCP_READ,
        NetProtocol::Udp => UDP_READ,
    }
}

// [@ANCHOR: radioctl_core_net_open_link]
/// Opens a network link on `runtime` and returns once the first connection is ready, or with
/// the last attempt's error after `config.first_attempts` ([`ErrorKind::LinkDown`]). After that
/// the supervisor task reconnects without limit until the stream is dropped. Pass
/// `Box::new(RawFraming)` for a plain CAT stream.
pub async fn open_link(
    runtime: &Handle,
    config: NetConfig,
    framing: Box<dyn Framing>,
) -> Result<NetLink, RigError> {
    let (ours, theirs) = duplex(STREAM_BYTES);
    let (status_tx, status_rx) = watch::channel(LinkStatus::Reconnecting {
        attempt: 0,
        last_error: "not connected yet".into(),
    });
    let counters = Arc::new(Counters::default());
    let supervisor = Supervisor {
        config,
        framing,
        status: status_tx,
        counters: Arc::clone(&counters),
    };
    let (first_tx, first_rx) = oneshot::channel();
    runtime.spawn(supervisor.run(ours, first_tx));
    match first_rx.await {
        Ok(Ok(())) => Ok(NetLink {
            transport: theirs,
            status: status_rx,
            counters,
        }),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(net_error("network supervisor stopped before connecting")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_core_net_config_new]
    #[test]
    fn config_defaults_are_bounded() {
        let config = NetConfig::new("192.168.1.9:4992");
        assert_eq!(config.protocol, NetProtocol::Tcp);
        assert!(config.connect_timeout > Duration::ZERO);
        assert!(config.write_timeout > Duration::ZERO);
        assert_eq!(config.first_attempts, 1);
        assert_eq!(config.idle_timeout, None);
    }

    // Tests [@ANCHOR: radioctl_core_net_raw_reset]
    // Tests [@ANCHOR: radioctl_core_net_raw_greeting]
    // Tests [@ANCHOR: radioctl_core_net_raw_needs_handshake]
    // Tests [@ANCHOR: radioctl_core_net_raw_encode]
    // Tests [@ANCHOR: radioctl_core_net_raw_decode]
    // Tests [@ANCHOR: radioctl_core_net_raw_keepalive]
    #[test]
    fn raw_framing_is_the_identity() {
        let mut raw = RawFraming;
        raw.reset();
        assert!(raw.greeting().is_empty());
        assert!(!raw.needs_handshake());
        assert_eq!(raw.encode(b"FA;"), b"FA;");
        let decoded = raw.decode(b"FA00014074000;");
        assert_eq!(decoded.payload, b"FA00014074000;");
        assert!(decoded.reply.is_empty() && decoded.failure.is_none());
        assert!(raw.keepalive().is_none());
    }

    // Tests [@ANCHOR: radioctl_core_net_length_new]
    // Tests [@ANCHOR: radioctl_core_net_length_reset]
    // Tests [@ANCHOR: radioctl_core_net_length_greeting]
    // Tests [@ANCHOR: radioctl_core_net_length_needs_handshake]
    // Tests [@ANCHOR: radioctl_core_net_length_encode]
    // Tests [@ANCHOR: radioctl_core_net_length_decode]
    // Tests [@ANCHOR: radioctl_core_net_length_keepalive]
    #[test]
    fn length_framing_round_trips_and_reassembles() {
        for width in [2usize, 4] {
            let mut framing = LengthFraming::new(width);
            framing.reset();
            assert!(framing.greeting().is_empty());
            assert!(!framing.needs_handshake());
            assert!(framing.keepalive().is_none());
            let mut wire = framing.encode(b"IF;");
            wire.extend(framing.encode(b"FA;"));
            assert_eq!(wire.len(), 2 * (width + 3));
            // Fed a byte at a time, the frames still come out whole and in order.
            let mut payload = Vec::new();
            for byte in &wire {
                let decoded = framing.decode(std::slice::from_ref(byte));
                assert!(decoded.failure.is_none());
                payload.extend(decoded.payload);
            }
            assert_eq!(payload, b"IF;FA;");
        }
        assert_eq!(LengthFraming::new(7).encode(b"x"), [0, 0, 0, 1, b'x']);
    }

    // Tests [@ANCHOR: radioctl_core_net_length_decode]
    #[test]
    fn length_framing_refuses_an_absurd_length() {
        let mut framing = LengthFraming::new(4);
        let decoded = framing.decode(&[0xff, 0xff, 0xff, 0xff, 1, 2]);
        assert!(decoded.failure.is_some());
        assert!(decoded.payload.is_empty());
        // Reset forgets the corrupt stream.
        framing.reset();
        assert_eq!(framing.decode(&[0, 0, 0, 1, 9]).payload, [9]);
    }

    // Tests [@ANCHOR: radioctl_core_net_snapshot]
    // Tests [@ANCHOR: radioctl_core_net_proto_name]
    // Tests [@ANCHOR: radioctl_core_net_read_size]
    // Tests [@ANCHOR: radioctl_core_net_error]
    #[test]
    fn small_helpers() {
        let counters = Counters::default();
        counters.bytes_in.store(3, Ordering::Relaxed);
        counters.reconnects.store(1, Ordering::Relaxed);
        let stats = snapshot(&counters);
        assert_eq!((stats.bytes_in, stats.reconnects), (3, 1));
        assert_eq!(proto_name(NetProtocol::Tcp), "tcp");
        assert_eq!(proto_name(NetProtocol::Udp), "udp");
        assert!(read_size(NetProtocol::Udp) > read_size(NetProtocol::Tcp));
        assert_eq!(net_error("x").kind, ErrorKind::LinkDown);
    }
}

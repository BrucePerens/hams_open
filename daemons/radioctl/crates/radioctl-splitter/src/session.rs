// SPDX-License-Identifier: AGPL-3.0-or-later
//! The splitter proper: any number of clients in front of one rig actor.
//!
//! Each client is a task with its own [`Personality`]. It reads the client's bytes, asks the rig
//! (through the actor's single queue, or its cache for reads fresh enough), writes the reply, and
//! pushes the rig's changes to the client as that protocol's unsolicited frames (a field first
//! read by a request is a reading, not a change, and is not pushed; one the rig reported on its
//! own always is). The first client
//! to key PTT owns the transmitter until it unkeys or goes away; going away unkeys the rig.
//! Requests that transmit are subject to the rig's own transmit gate as usual.
//!
//! The splitter creates no runtime: every spawn takes the caller's `Handle`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use radioctl_core::event::Source;
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::rig::RigHandle;
use radioctl_core::transport::Transport;
use radioctl_core::units::{Hz, Vfo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::arbiter::{ClientId, PttOwner};
use crate::civ_personality::CivPersonality;
use crate::personality::{Action, Outcome, Personality, Request};
use crate::ts2000_personality::Ts2000Personality;

const CHANNEL: usize = 64;
const READ_BUFFER: usize = 256;

/// The protocol a splitter endpoint speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Icom CI-V as an IC-7300 at `address` (0x94), with the bus echo on or off.
    Civ {
        /// The CI-V address to answer at.
        address: u8,
        /// Echo each frame back to the client that sent it.
        echo: bool,
    },
    /// Kenwood TS-2000 CAT.
    Ts2000,
}

/// Splitter settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitterConfig {
    /// The protocol clients speak.
    pub protocol: Protocol,
    /// A read is answered from the rig's cache if the cached value is no older than this.
    pub max_age: Duration,
    /// Deadline for a request that has to go to the rig.
    pub request_timeout: Duration,
    /// While the rig is keyed in split operation, a plain read of the current frequency (CI-V
    /// `03`, TS-2000 `FA;`) answers with the transmit frequency, as OmniRig-style clients
    /// assume (Hamlib issue 138). Reads that name the other VFO (`25 01`, `FB;`) are unchanged.
    /// Turn it off for a client that wants the receive frequency while keyed.
    pub report_tx_frequency_while_keyed: bool,
}

impl SplitterConfig {
    // [@ANCHOR: radioctl_splitter_config_new]
    /// Defaults: cached reads up to 5 seconds old, 2 second deadline for the rig.
    pub fn new(protocol: Protocol) -> SplitterConfig {
        SplitterConfig {
            protocol,
            max_age: Duration::from_secs(5),
            request_timeout: Duration::from_secs(2),
            report_tx_frequency_while_keyed: true,
        }
    }
}

/// What a client delivers to its task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientInput {
    /// Bytes the client sent.
    Bytes(Vec<u8>),
    /// The client went away (a stream ended, a pty was closed by the program using it).
    Disconnected,
}

/// Counters of a splitter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SplitterStats {
    /// Requests that reached the rig side (served from cache or sent to the rig).
    pub requests: u64,
    /// Reads answered from the cache.
    pub cache_hits: u64,
    /// Requests refused.
    pub refused: u64,
    /// Keying attempts refused because another client owns the transmitter.
    pub ptt_conflicts: u64,
    /// Clients attached so far.
    pub clients: u64,
}

#[derive(Default)]
struct Counters {
    requests: AtomicU64,
    cache_hits: AtomicU64,
    refused: AtomicU64,
    ptt_conflicts: AtomicU64,
    clients: AtomicU64,
}

struct Inner {
    rig: RigHandle,
    config: SplitterConfig,
    owner: Mutex<PttOwner>,
    next_id: AtomicU64,
    counters: Counters,
    epoch: Instant,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The splitter in front of one rig. Clone it freely.
#[derive(Clone)]
pub struct Splitter {
    inner: Arc<Inner>,
}

/// A running client.
pub struct ClientHandle {
    /// The client's identifier (the PTT owner id).
    pub id: ClientId,
    task: JoinHandle<()>,
}

impl ClientHandle {
    // [@ANCHOR: radioctl_splitter_client_wait]
    /// Waits until the client's task has ended.
    pub async fn wait(self) {
        let _ = self.task.await;
    }

    // [@ANCHOR: radioctl_splitter_client_abort]
    /// Stops the client's task. Its PTT ownership is not released by this; drop the stream to
    /// disconnect cleanly.
    pub fn abort(&self) {
        self.task.abort();
    }
}

impl Splitter {
    // [@ANCHOR: radioctl_splitter_new]
    /// A splitter for `rig`.
    pub fn new(rig: RigHandle, config: SplitterConfig) -> Splitter {
        Splitter {
            inner: Arc::new(Inner {
                rig,
                config,
                owner: Mutex::new(PttOwner::default()),
                next_id: AtomicU64::new(1),
                counters: Counters::default(),
                epoch: Instant::now(),
            }),
        }
    }

    // [@ANCHOR: radioctl_splitter_stats]
    /// A snapshot of the counters.
    pub fn stats(&self) -> SplitterStats {
        let c = &self.inner.counters;
        SplitterStats {
            requests: c.requests.load(Ordering::Relaxed),
            cache_hits: c.cache_hits.load(Ordering::Relaxed),
            refused: c.refused.load(Ordering::Relaxed),
            ptt_conflicts: c.ptt_conflicts.load(Ordering::Relaxed),
            clients: c.clients.load(Ordering::Relaxed),
        }
    }

    // [@ANCHOR: radioctl_splitter_ptt_owner]
    /// The client that owns the transmitter, if any.
    pub fn ptt_owner(&self) -> Option<ClientId> {
        locked(&self.inner.owner).owner()
    }

    // [@ANCHOR: radioctl_splitter_personality]
    /// A new personality for the configured protocol.
    fn personality(&self) -> Box<dyn Personality> {
        match self.inner.config.protocol {
            Protocol::Civ { address, echo } => Box::new(CivPersonality::new(address, echo)),
            Protocol::Ts2000 => Box::new(Ts2000Personality::new()),
        }
    }

    // [@ANCHOR: radioctl_splitter_attach_channels]
    /// Attaches a client given as channels. With `persistent` a [`ClientInput::Disconnected`]
    /// resets the client (and gives up its PTT) but keeps the task, for a pty that programs open
    /// and close; otherwise it ends the client.
    pub fn attach_channels(
        &self,
        runtime: &Handle,
        input: mpsc::Receiver<ClientInput>,
        output: mpsc::Sender<Vec<u8>>,
        persistent: bool,
    ) -> ClientHandle {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner.counters.clients.fetch_add(1, Ordering::Relaxed);
        let task = runtime.spawn(run_client(
            Arc::clone(&self.inner),
            id,
            self.personality(),
            input,
            output,
            persistent,
        ));
        ClientHandle { id, task }
    }

    // [@ANCHOR: radioctl_splitter_attach_stream]
    /// Attaches a client that is any byte stream (a TCP connection, an in-memory duplex). The end
    /// of the stream disconnects the client.
    pub fn attach_stream<S: Transport>(&self, runtime: &Handle, stream: S) -> ClientHandle {
        let (mut reader, mut writer) = tokio::io::split(stream);
        let (input_tx, input_rx) = mpsc::channel(CHANNEL);
        let (output_tx, mut output_rx) = mpsc::channel::<Vec<u8>>(CHANNEL);
        runtime.spawn(async move {
            let mut buffer = [0u8; READ_BUFFER];
            loop {
                match reader.read(&mut buffer).await {
                    Ok(n) if n > 0 => {
                        let chunk = buffer.get(..n).unwrap_or(&[]).to_vec();
                        if input_tx.send(ClientInput::Bytes(chunk)).await.is_err() {
                            return;
                        }
                    }
                    _ => {
                        let _ = input_tx.send(ClientInput::Disconnected).await;
                        return;
                    }
                }
            }
        });
        runtime.spawn(async move {
            while let Some(bytes) = output_rx.recv().await {
                if writer.write_all(&bytes).await.is_err() {
                    break;
                }
            }
        });
        self.attach_channels(runtime, input_rx, output_tx, false)
    }

    // [@ANCHOR: radioctl_splitter_serve_tcp]
    /// Accepts connections on `listener` until the returned task is aborted; each connection is a
    /// client. Bind the listener to loopback unless the network is meant to reach the radio.
    pub fn serve_tcp(&self, runtime: &Handle, listener: TcpListener) -> JoinHandle<()> {
        let splitter = self.clone();
        let handle = runtime.clone();
        runtime.spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let _ = socket.set_nodelay(true);
                splitter.attach_stream(&handle, socket);
            }
        })
    }

    // [@ANCHOR: radioctl_splitter_cleanup]
    /// Gives up the client's PTT ownership, unkeying the rig if the client held it.
    async fn cleanup(&self, id: ClientId) {
        let held = locked(&self.inner.owner).release(id);
        if held {
            let _ = self
                .inner
                .rig
                .execute_from(
                    Op::SetPtt(false),
                    self.inner.config.request_timeout,
                    Source::Splitter,
                )
                .await;
        }
    }

    fn count(&self, counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    // [@ANCHOR: radioctl_splitter_keyed_split_frequency]
    /// The transmit frequency to answer a plain read of the current frequency with: only while
    /// keyed in split operation, only for VFO A (the current VFO of both personalities), and only
    /// when the configuration allows it. `None` means answer as usual.
    fn keyed_split_frequency(&self, vfo: Vfo) -> Option<Hz> {
        if vfo != Vfo::A || !self.inner.config.report_tx_frequency_while_keyed {
            return None;
        }
        let cached = |field| self.inner.rig.cached(field).map(|c| c.value);
        match (
            cached(Field::Ptt),
            cached(Field::Split),
            cached(Field::ActiveTxFrequency),
        ) {
            (
                Some(Observation::Ptt(true)),
                Some(Observation::Split(true)),
                Some(Observation::ActiveTxFrequency(hz)),
            ) => Some(hz),
            _ => None,
        }
    }

    // [@ANCHOR: radioctl_splitter_read]
    /// A read: the cached value if fresh enough, else asks the rig.
    async fn read(&self, field: Field, op: Op) -> Outcome {
        if let Some(cached) = self.inner.rig.cached(field) {
            if cached.age <= self.inner.config.max_age {
                self.count(&self.inner.counters.cache_hits);
                return Outcome::Value(cached.value);
            }
        }
        match self
            .inner
            .rig
            .execute_from(op, self.inner.config.request_timeout, Source::Splitter)
            .await
        {
            Ok(Some(value)) => Outcome::Value(value),
            _ => Outcome::Refused,
        }
    }

    // [@ANCHOR: radioctl_splitter_set]
    /// A set: asks the rig and reports done or refused.
    async fn set(&self, op: Op) -> Outcome {
        match self
            .inner
            .rig
            .execute_from(op, self.inner.config.request_timeout, Source::Splitter)
            .await
        {
            Ok(_) => Outcome::Done,
            Err(_) => Outcome::Refused,
        }
    }

    // [@ANCHOR: radioctl_splitter_execute]
    /// Carries out one request for `client`, with PTT arbitration.
    async fn execute(&self, client: ClientId, request: &Request) -> Outcome {
        self.count(&self.inner.counters.requests);
        let outcome = match request {
            Request::ReadFrequency(vfo) => match self.keyed_split_frequency(*vfo) {
                Some(hz) => Outcome::Value(Observation::Frequency(*vfo, hz)),
                None => {
                    self.read(Field::Frequency(*vfo), Op::GetFrequency(*vfo))
                        .await
                }
            },
            Request::ReadMode => self.read(Field::Mode, Op::GetMode).await,
            Request::ReadPtt => self.read(Field::Ptt, Op::GetPtt).await,
            Request::ReadSignal => self.read(Field::Signal, Op::GetSignal).await,
            Request::ReadPower => self.read(Field::Power, Op::GetPower).await,
            Request::SetFrequency(vfo, hz) => self.set(Op::SetFrequency(*vfo, *hz)).await,
            Request::SetMode(mode) => self.set(Op::SetMode(*mode)).await,
            Request::SetPtt(true) => self.key(client).await,
            Request::SetPtt(false) => {
                let outcome = self.set(Op::SetPtt(false)).await;
                if outcome == Outcome::Done {
                    locked(&self.inner.owner).clear();
                }
                outcome
            }
            Request::SendCw(text) => self.send_cw(client, text).await,
        };
        if outcome == Outcome::Refused {
            self.count(&self.inner.counters.refused);
        }
        outcome
    }

    // [@ANCHOR: radioctl_splitter_send_cw]
    /// Sends Morse for `client`. Morse puts RF on the air, so the sender claims the transmitter
    /// like a key does: the claim is given back if the rig refuses, and a sender that goes away
    /// unkeys the rig.
    async fn send_cw(&self, client: ClientId, text: &str) -> Outcome {
        let claim = locked(&self.inner.owner).try_key(client);
        if claim.is_err() {
            self.count(&self.inner.counters.ptt_conflicts);
            return Outcome::Refused;
        }
        let outcome = self.set(Op::SendCw(text.to_string())).await;
        if outcome != Outcome::Done {
            locked(&self.inner.owner).release(client);
        }
        outcome
    }

    // [@ANCHOR: radioctl_splitter_key]
    /// Keys for `client` if no other client owns the transmitter; the claim is given back if the
    /// rig refuses (the gate is closed, the link is down).
    async fn key(&self, client: ClientId) -> Outcome {
        let claim = locked(&self.inner.owner).try_key(client);
        if claim.is_err() {
            self.count(&self.inner.counters.ptt_conflicts);
            return Outcome::Refused;
        }
        let outcome = self.set(Op::SetPtt(true)).await;
        if outcome != Outcome::Done {
            locked(&self.inner.owner).release(client);
        }
        outcome
    }
}

// [@ANCHOR: radioctl_splitter_run_client]
/// The task of one client: input, rig changes, until the client ends.
async fn run_client(
    inner: Arc<Inner>,
    id: ClientId,
    mut personality: Box<dyn Personality>,
    mut input: mpsc::Receiver<ClientInput>,
    output: mpsc::Sender<Vec<u8>>,
    persistent: bool,
) {
    let splitter = Splitter { inner };
    let mut events = splitter.inner.rig.events();
    'client: loop {
        tokio::select! {
            item = input.recv() => match item {
                Some(ClientInput::Bytes(bytes)) => {
                    let now = Instant::now().saturating_duration_since(splitter.inner.epoch);
                    for pending in personality.parse(&bytes, now) {
                        if !pending.echo.is_empty() && output.send(pending.echo).await.is_err() {
                            break 'client;
                        }
                        let reply = match pending.action {
                            Action::Ignore => Vec::new(),
                            Action::Local(bytes) => bytes,
                            Action::Rig { request, context } => {
                                let outcome = splitter.execute(id, &request).await;
                                personality.reply(&context, &request, &outcome)
                            }
                        };
                        if !reply.is_empty() && output.send(reply).await.is_err() {
                            break 'client;
                        }
                    }
                }
                Some(ClientInput::Disconnected) => {
                    splitter.cleanup(id).await;
                    personality.reset();
                    if !persistent {
                        break;
                    }
                }
                None => break,
            },
            event = events.next() => match event {
                Some(event) => {
                    let from_rig = event.source == Source::Transceive;
                    for change in event.changes.iter().filter(|c| from_rig || c.old.is_some()) {
                        let bytes = personality.unsolicited(change);
                        if !bytes.is_empty() && output.send(bytes).await.is_err() {
                            break 'client;
                        }
                    }
                }
                None => break,
            },
        }
    }
    splitter.cleanup(id).await;
}

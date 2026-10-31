// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rig actor and its handle.
//!
//! One actor task per rig owns the transport and the protocol engine and runs commands one at a
//! time. Rigs share nothing: no global lock, no global state. The actor is spawned on a tokio
//! runtime the caller supplies; the library never creates a runtime unless the `owned-runtime`
//! feature is on and its dedicated constructor is used. The runtime needs the time driver.
//!
//! A [`RigHandle`] is `Clone + Send + Sync`. Its methods only wait on channels (the actor enforces
//! every deadline and always answers), so a handle can be used from any thread, with or without a
//! tokio runtime of its own, and from a runtime other than the actor's.
//!
//! Priorities: unkeying the transmitter goes through a separate emergency channel, is taken before
//! anything else, and abandons a command that is waiting for a reply (that command ends with
//! [`ErrorKind::Cancelled`]); ordinary commands are served in order; polling runs only when no
//! command is waiting. Every command has a deadline and ends in a typed error, never a hang.
//!
//! Transmit watchdog: from the moment the rig is keyed (by command, or seen keyed by polling or
//! transceive) or Morse is sent, the actor runs a dead-man timer of its own, independent of the
//! caller. Transmitting past [`RigOptions::max_transmit`], or past a lapse of the caller's
//! [`RigHandle::heartbeat`] when [`RigOptions::heartbeat`] is set, forces an unkey through the
//! emergency path and reports it as an [`Alarm`] event. An unkey is retried with a short backoff;
//! one that fails after every attempt is reported as [`Alarm::UnkeyFailed`] and counted, never
//! swallowed. An unkey is never refused for want of a queue slot: when the emergency queue is
//! full the request rides on the unkey already queued and gets its result.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Handle;
use tokio::sync::{broadcast, mpsc, oneshot, watch, Notify};
use tokio::time::{sleep_until, timeout_at, Instant};

use crate::capability::{Capability, CapabilitySet};
use crate::dispatcher::{Callback, Dispatcher, DispatcherCounters};
use crate::engine::{Inbound, ProtocolEngine, ReplyKind};
use crate::error::{ErrorKind, RigError};
use crate::event::{Change, RigEvent, Source};
use crate::op::{Alarm, Field, Observation, Op};
use crate::poll::{PollConfig, PollSchedule};
use crate::scope::ScopeFrame;
use crate::serial::LinkStatus;
use crate::transport::Transport;
use crate::txstate::TxState;
use crate::units::{Hz, Mode, Vfo};

const TRANSCRIPT_LINES: usize = 16;
const READ_BUFFER: usize = 256;
const EMERGENCY_QUEUE: usize = 16;
const UNKEY_ATTEMPTS: u32 = 3;
const UNKEY_BACKOFF: Duration = Duration::from_millis(100);

/// The transmit time limit a rig gets when the owner sets none: three minutes.
pub const DEFAULT_MAX_TRANSMIT: Duration = Duration::from_secs(180);

/// Options for a rig.
#[derive(Clone)]
pub struct RigOptions {
    /// How many ordinary commands may wait in the queue.
    pub queue_capacity: usize,
    /// Deadline applied when a call does not give one.
    pub default_timeout: Duration,
    /// Polling.
    pub poll: PollConfig,
    /// Events held for the callback before they are coalesced.
    pub dispatcher_capacity: usize,
    /// Events held per async-stream subscriber before it lags.
    pub stream_capacity: usize,
    /// Scope frames held per scope-stream subscriber before it lags.
    pub scope_capacity: usize,
    /// The status-change callback, if any.
    pub callback: Option<Callback>,
    /// The transport's link status, when the transport can report it
    /// ([`crate::serial::SerialLink::watch`]). The serial supervisor keeps the byte stream alive
    /// across a port loss (writes are dropped and counted, not failed), so without this the actor
    /// would never learn that the port went away or came back. With it, the actor marks the link
    /// down (and refuses commands with `LinkDown`) while the port is `Reconnecting`, and on every
    /// reopen after the first it marks the link up and unkeys the rig before serving any other
    /// command: the rig may have been keyed when the link dropped.
    pub link_watch: Option<watch::Receiver<LinkStatus>>,
    /// The longest the rig may transmit in one go before the actor unkeys it on its own
    /// ([`DEFAULT_MAX_TRANSMIT`] when the owner sets none). Measured from the moment the rig is
    /// keyed or Morse is sent; `Duration::MAX` disables the limit, which the owner must decide.
    pub max_transmit: Duration,
    /// A dead-man period: while the rig transmits, the caller must call
    /// [`RigHandle::heartbeat`] at least this often or the actor unkeys the rig. `None` (the
    /// default) relies on `max_transmit` alone. Not enforced while receiving.
    pub heartbeat: Option<Duration>,
}

impl Default for RigOptions {
    fn default() -> RigOptions {
        RigOptions {
            queue_capacity: 64,
            default_timeout: Duration::from_secs(2),
            poll: PollConfig::default(),
            dispatcher_capacity: 64,
            stream_capacity: 64,
            scope_capacity: 16,
            callback: None,
            link_watch: None,
            max_transmit: DEFAULT_MAX_TRANSMIT,
            heartbeat: None,
        }
    }
}

impl RigOptions {
    // [@ANCHOR: radioctl_core_rig_options_with_callback]
    /// Sets the status-change callback. It runs on a dedicated thread, never on the actor.
    pub fn with_callback<F: Fn(RigEvent) + Send + Sync + 'static>(
        mut self,
        callback: F,
    ) -> RigOptions {
        self.callback = Some(Arc::new(callback));
        self
    }
}

/// Proof that someone decided transmitting is allowed on one rig. There is no bool shortcut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransmitPermit {
    granted_by: String,
}

impl TransmitPermit {
    // [@ANCHOR: radioctl_core_transmit_permit_new]
    /// A permit recording who granted it (an operator, an attestation, a test name).
    pub fn new(granted_by: impl Into<String>) -> TransmitPermit {
        TransmitPermit {
            granted_by: granted_by.into(),
        }
    }

    // [@ANCHOR: radioctl_core_transmit_permit_granted_by]
    /// Who granted the permit.
    pub fn granted_by(&self) -> &str {
        &self.granted_by
    }
}

/// How fresh a read must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Any cached value, however old; asks the rig only if nothing is cached.
    Cached,
    /// A cached value no older than the duration.
    MaxAge(Duration),
    /// Always asks the rig.
    Fresh,
}

/// A value with the age of the information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cached<T> {
    /// The value.
    pub value: T,
    /// Time since the rig last reported it.
    pub age: Duration,
}

/// Counters of one rig.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RigStats {
    /// Commands that succeeded.
    pub commands_ok: u64,
    /// Commands that failed.
    pub commands_failed: u64,
    /// Commands abandoned for an emergency command.
    pub preempted: u64,
    /// Polls issued.
    pub polls: u64,
    /// Observations the rig sent on its own.
    pub unsolicited: u64,
    /// Replies that arrived after their command had been given up (a rig without the
    /// `Transceive` capability answering late); they never slow polling.
    pub late_replies: u64,
    /// Received bytes no frame could be made of.
    pub unrecognised: u64,
    /// Events produced.
    pub events: u64,
    /// Events a stream subscriber missed because it lagged.
    pub stream_lagged: u64,
    /// Scope frames published.
    pub scope_frames: u64,
    /// Scope frames a stream subscriber missed because it lagged.
    pub scope_lagged: u64,
    /// Unkeys the transmit watchdog forced (time limit or heartbeat lapse).
    pub watchdog_unkeys: u64,
    /// Unkeys the actor tried on its own and could not complete: the rig may be on the air.
    pub unkey_failures: u64,
    /// Unkeys that found the emergency queue full and rode on the one already queued.
    pub unkeys_coalesced: u64,
    /// The callback dispatcher's counters.
    pub dispatcher: DispatcherCounters,
}

#[derive(Default)]
struct Counters {
    commands_ok: AtomicU64,
    commands_failed: AtomicU64,
    preempted: AtomicU64,
    polls: AtomicU64,
    unsolicited: AtomicU64,
    late_replies: AtomicU64,
    unrecognised: AtomicU64,
    events: AtomicU64,
    stream_lagged: AtomicU64,
    scope_frames: AtomicU64,
    scope_lagged: AtomicU64,
    watchdog_unkeys: AtomicU64,
    unkey_failures: AtomicU64,
    unkeys_coalesced: AtomicU64,
}

struct CacheEntry {
    value: Observation,
    at: Instant,
}

struct Shared {
    id: String,
    capabilities: CapabilitySet,
    cache: Mutex<HashMap<Field, CacheEntry>>,
    gate: AtomicBool,
    permit: Mutex<Option<TransmitPermit>>,
    link_up: AtomicBool,
    emergency_pending: AtomicUsize,
    emergency_wake: Notify,
    /// Unkeys that found the emergency queue full; answered with the next unkey's result.
    unkey_waiters: Mutex<Vec<oneshot::Sender<Reply>>>,
    /// Milliseconds since `epoch` of the last heartbeat.
    last_heartbeat: AtomicU64,
    /// Woken by every heartbeat so the actor recomputes its watchdog deadline.
    heartbeat_wake: Notify,
    epoch: Instant,
    events: broadcast::Sender<RigEvent>,
    scope: broadcast::Sender<ScopeFrame>,
    dispatcher: Option<Dispatcher>,
    counters: Counters,
    default_timeout: Duration,
}

fn guard<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

type Reply = Result<Option<Observation>, RigError>;

struct Command {
    op: Op,
    deadline: Instant,
    source: Source,
    reply: oneshot::Sender<Reply>,
}

struct Senders {
    normal: mpsc::Sender<Command>,
    emergency: mpsc::Sender<Command>,
}

/// A handle to a running rig. Clone it freely; every clone talks to the same actor.
#[derive(Clone)]
pub struct RigHandle {
    shared: Arc<Shared>,
    senders: Arc<Senders>,
}

// [@ANCHOR: radioctl_core_rig_spawn]
/// Spawns the actor for `engine` over `transport` on `runtime`.
///
/// The runtime must have its time driver enabled. The actor ends when every handle is dropped.
pub fn spawn<E: ProtocolEngine, T: Transport>(
    runtime: &Handle,
    engine: E,
    transport: T,
    options: RigOptions,
) -> Result<RigHandle, RigError> {
    let description = engine.description();
    let id = description.id.clone();
    let capabilities = description.capabilities.clone();
    let dispatcher = match &options.callback {
        Some(callback) => Some(Dispatcher::start(
            &id,
            options.dispatcher_capacity,
            Arc::clone(callback),
        )?),
        None => None,
    };
    let (events, _) = broadcast::channel(options.stream_capacity.max(1));
    let (scope, _) = broadcast::channel(options.scope_capacity.max(1));
    let shared = Arc::new(Shared {
        id,
        capabilities,
        cache: Mutex::new(HashMap::new()),
        gate: AtomicBool::new(false),
        permit: Mutex::new(None),
        link_up: AtomicBool::new(true),
        emergency_pending: AtomicUsize::new(0),
        emergency_wake: Notify::new(),
        unkey_waiters: Mutex::new(Vec::new()),
        last_heartbeat: AtomicU64::new(0),
        heartbeat_wake: Notify::new(),
        epoch: Instant::now(),
        events,
        scope,
        dispatcher,
        counters: Counters::default(),
        default_timeout: options.default_timeout,
    });
    let (normal_tx, normal_rx) = mpsc::channel(options.queue_capacity.max(1));
    let (emergency_tx, emergency_rx) = mpsc::channel(EMERGENCY_QUEUE);
    let poll = PollSchedule::for_capabilities(&shared.capabilities, &options.poll, Instant::now());
    let actor = Actor {
        engine,
        transport,
        shared: Arc::clone(&shared),
        normal: normal_rx,
        emergency: emergency_rx,
        poll,
        transcript: VecDeque::new(),
        epoch: shared.epoch,
        buffer: [0u8; READ_BUFFER],
        last_write: None,
        keyed_since: None,
        max_transmit: options.max_transmit,
        heartbeat: options.heartbeat,
        link_watch: options.link_watch,
        link_generation: 0,
    };
    runtime.spawn(actor.run());
    Ok(RigHandle {
        shared,
        senders: Arc::new(Senders {
            normal: normal_tx,
            emergency: emergency_tx,
        }),
    })
}

// [@ANCHOR: radioctl_core_rig_spawn_current]
/// Like [`spawn`], on the runtime of the calling task. Fails if there is none.
pub fn spawn_current<E: ProtocolEngine, T: Transport>(
    engine: E,
    transport: T,
    options: RigOptions,
) -> Result<RigHandle, RigError> {
    let runtime = Handle::try_current()
        .map_err(|e| RigError::new(ErrorKind::Unsupported, format!("no tokio runtime: {e}")))?;
    spawn(&runtime, engine, transport, options)
}

/// A rig together with the small runtime that runs it.
#[cfg(feature = "owned-runtime")]
pub struct OwnedRig {
    /// The rig's handle.
    pub handle: RigHandle,
    runtime: tokio::runtime::Runtime,
}

#[cfg(feature = "owned-runtime")]
impl OwnedRig {
    // [@ANCHOR: radioctl_core_rig_owned_shutdown]
    /// Stops the runtime without waiting for tasks. Call from outside any async context.
    pub fn shutdown(self) {
        self.runtime.shutdown_background();
    }
}

#[cfg(feature = "owned-runtime")]
// [@ANCHOR: radioctl_core_rig_spawn_with_owned_runtime]
/// Builds a one-worker runtime of its own and spawns the rig on it. For `radioctld` and the C
/// ABI; library users should pass their runtime to [`spawn`] instead.
pub fn spawn_with_owned_runtime<E: ProtocolEngine, T: Transport>(
    make_transport: impl FnOnce() -> T,
    engine: E,
    options: RigOptions,
) -> Result<OwnedRig, RigError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .thread_name("radioctl-rig")
        .build()
        .map_err(|e| RigError::new(ErrorKind::LinkDown, format!("runtime: {e}")))?;
    let transport = {
        let _entered = runtime.enter();
        make_transport()
    };
    let handle = spawn(runtime.handle(), engine, transport, options)?;
    Ok(OwnedRig { handle, runtime })
}

impl RigHandle {
    // [@ANCHOR: radioctl_core_handle_id]
    /// The rig's identifier.
    pub fn id(&self) -> &str {
        &self.shared.id
    }

    // [@ANCHOR: radioctl_core_handle_capabilities]
    /// What the rig can do.
    pub fn capabilities(&self) -> &CapabilitySet {
        &self.shared.capabilities
    }

    // [@ANCHOR: radioctl_core_handle_open_gate]
    /// Opens the transmit gate for this rig only. It starts closed.
    pub fn open_transmit_gate(&self, permit: TransmitPermit) {
        *guard(&self.shared.permit) = Some(permit);
        self.shared.gate.store(true, Ordering::Release);
    }

    // [@ANCHOR: radioctl_core_handle_close_gate]
    /// Closes the transmit gate. Transmit commands still queued are refused when they run.
    pub fn close_transmit_gate(&self) {
        self.shared.gate.store(false, Ordering::Release);
        *guard(&self.shared.permit) = None;
    }

    // [@ANCHOR: radioctl_core_handle_gate_state]
    /// Who opened the gate, or `None` while it is closed.
    pub fn transmit_permit(&self) -> Option<TransmitPermit> {
        if self.shared.gate.load(Ordering::Acquire) {
            guard(&self.shared.permit).clone()
        } else {
            None
        }
    }

    // [@ANCHOR: radioctl_core_handle_stats]
    /// A snapshot of the counters.
    pub fn stats(&self) -> RigStats {
        let c = &self.shared.counters;
        RigStats {
            commands_ok: c.commands_ok.load(Ordering::Relaxed),
            commands_failed: c.commands_failed.load(Ordering::Relaxed),
            preempted: c.preempted.load(Ordering::Relaxed),
            polls: c.polls.load(Ordering::Relaxed),
            unsolicited: c.unsolicited.load(Ordering::Relaxed),
            late_replies: c.late_replies.load(Ordering::Relaxed),
            unrecognised: c.unrecognised.load(Ordering::Relaxed),
            events: c.events.load(Ordering::Relaxed),
            stream_lagged: c.stream_lagged.load(Ordering::Relaxed),
            scope_frames: c.scope_frames.load(Ordering::Relaxed),
            scope_lagged: c.scope_lagged.load(Ordering::Relaxed),
            watchdog_unkeys: c.watchdog_unkeys.load(Ordering::Relaxed),
            unkey_failures: c.unkey_failures.load(Ordering::Relaxed),
            unkeys_coalesced: c.unkeys_coalesced.load(Ordering::Relaxed),
            dispatcher: self
                .shared
                .dispatcher
                .as_ref()
                .map(Dispatcher::counters)
                .unwrap_or_default(),
        }
    }

    // [@ANCHOR: radioctl_core_handle_heartbeat]
    /// Proves the caller is alive. With [`RigOptions::heartbeat`] set, a keyed rig whose caller
    /// stops calling this is unkeyed by the actor once the period lapses. Cheap: one atomic store.
    pub fn heartbeat(&self) {
        let millis = Instant::now()
            .saturating_duration_since(self.shared.epoch)
            .as_millis();
        self.shared
            .last_heartbeat
            .store(u64::try_from(millis).unwrap_or(u64::MAX), Ordering::Release);
        self.shared.heartbeat_wake.notify_one();
    }

    // [@ANCHOR: radioctl_core_handle_link_up]
    /// False once the transport has failed.
    pub fn link_up(&self) -> bool {
        self.shared.link_up.load(Ordering::Acquire)
    }

    // [@ANCHOR: radioctl_core_handle_events]
    /// An async stream of status changes, starting now.
    pub fn events(&self) -> EventStream {
        EventStream {
            rx: self.shared.events.subscribe(),
            shared: Arc::clone(&self.shared),
        }
    }

    // [@ANCHOR: radioctl_core_handle_scope_frames]
    /// A stream of the rig's scope sweeps, starting now. Frames have a stream of their own
    /// (not the status-change events): a subscriber that falls behind skips frames and the miss
    /// is counted in `scope_lagged`. The rig sends sweeps once `Op::SetScopeOutput(true)` has run.
    pub fn scope_frames(&self) -> ScopeStream {
        ScopeStream {
            rx: self.shared.scope.subscribe(),
            shared: Arc::clone(&self.shared),
        }
    }

    // [@ANCHOR: radioctl_core_handle_cached]
    /// The cached value of a field with its age, without touching the rig.
    pub fn cached(&self, field: Field) -> Option<Cached<Observation>> {
        let cache = guard(&self.shared.cache);
        cache.get(&field).map(|e| Cached {
            value: e.value,
            age: Instant::now().saturating_duration_since(e.at),
        })
    }

    // [@ANCHOR: radioctl_core_handle_execute]
    /// Runs a command with the default deadline.
    pub async fn execute(&self, op: Op) -> Reply {
        self.execute_within(op, self.shared.default_timeout).await
    }

    // [@ANCHOR: radioctl_core_handle_execute_within]
    /// Runs a command that must finish within `timeout`. Unkeying the transmitter takes the
    /// emergency path. The call waits only on a channel; the actor enforces the deadline.
    pub async fn execute_within(&self, op: Op, timeout: Duration) -> Reply {
        let receiver = self.submit(op, timeout, Source::Api)?;
        receiver
            .await
            .unwrap_or_else(|_| Err(RigError::new(ErrorKind::LinkDown, "rig actor stopped")))
    }

    // [@ANCHOR: radioctl_core_handle_execute_from]
    /// Like [`RigHandle::execute_within`], recording `source` on the events the command causes
    /// (the protocol splitter passes [`Source::Splitter`]). Same queue, same gates, same priority.
    pub async fn execute_from(&self, op: Op, timeout: Duration, source: Source) -> Reply {
        let receiver = self.submit(op, timeout, source)?;
        receiver
            .await
            .unwrap_or_else(|_| Err(RigError::new(ErrorKind::LinkDown, "rig actor stopped")))
    }

    // [@ANCHOR: radioctl_core_handle_submit]
    fn submit(
        &self,
        op: Op,
        timeout: Duration,
        source: Source,
    ) -> Result<oneshot::Receiver<Reply>, RigError> {
        if !self.shared.capabilities.has(op.required_capability()) {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                format!("{op:?} is not in the rig description"),
            ));
        }
        if op.transmits() && !self.shared.gate.load(Ordering::Acquire) {
            return Err(RigError::new(
                ErrorKind::TransmitGateClosed,
                "transmit gate is closed for this rig",
            ));
        }
        let (reply, receiver) = oneshot::channel();
        let command = Command {
            op,
            deadline: deadline_after(Instant::now(), timeout),
            source,
            reply,
        };
        let sent = if command.op.is_emergency() {
            self.shared.emergency_pending.fetch_add(1, Ordering::AcqRel);
            match self.senders.emergency.try_send(command) {
                Ok(()) => {
                    self.shared.emergency_wake.notify_one();
                    Ok(())
                }
                Err(mpsc::error::TrySendError::Full(command)) if command.op.is_unkey() => {
                    // An unkey is never refused: one pending unkey is enough for everybody, so
                    // this request rides on it and is answered with its result.
                    self.shared.emergency_pending.fetch_sub(1, Ordering::AcqRel);
                    guard(&self.shared.unkey_waiters).push(command.reply);
                    self.shared
                        .counters
                        .unkeys_coalesced
                        .fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }
                Err(other) => {
                    // A stop-Morse that finds the queue full is refused: the queue holds unkeys
                    // only, and each of those ends the transmission it would have stopped.
                    self.shared.emergency_pending.fetch_sub(1, Ordering::AcqRel);
                    Err(other)
                }
            }
        } else {
            self.senders.normal.try_send(command)
        };
        match sent {
            Ok(()) => Ok(receiver),
            Err(mpsc::error::TrySendError::Full(_)) => {
                Err(RigError::new(ErrorKind::QueueFull, "command queue full"))
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err(RigError::new(ErrorKind::LinkDown, "rig actor stopped"))
            }
        }
    }

    // [@ANCHOR: radioctl_core_handle_read]
    async fn read(
        &self,
        field: Field,
        op: Op,
        freshness: Freshness,
    ) -> Result<Cached<Observation>, RigError> {
        if let Some(cached) = self.cached(field) {
            let usable = match freshness {
                Freshness::Cached => true,
                Freshness::MaxAge(limit) => cached.age <= limit,
                Freshness::Fresh => false,
            };
            if usable {
                return Ok(cached);
            }
        }
        match self.execute(op).await? {
            Some(value) => Ok(Cached {
                value,
                age: Duration::ZERO,
            }),
            None => Err(RigError::new(
                ErrorKind::Malformed,
                "read returned no value",
            )),
        }
    }

    // [@ANCHOR: radioctl_core_handle_frequency]
    /// A VFO's frequency.
    pub async fn frequency(&self, vfo: Vfo, freshness: Freshness) -> Result<Cached<Hz>, RigError> {
        let read = self
            .read(Field::Frequency(vfo), Op::GetFrequency(vfo), freshness)
            .await?;
        match read.value {
            Observation::Frequency(_, hz) => Ok(Cached {
                value: hz,
                age: read.age,
            }),
            other => Err(RigError::new(
                ErrorKind::Malformed,
                format!("expected frequency, got {other:?}"),
            )),
        }
    }

    // [@ANCHOR: radioctl_core_handle_split]
    /// Whether the rig is in split operation.
    pub async fn split(&self, freshness: Freshness) -> Result<Cached<bool>, RigError> {
        let read = self.read(Field::Split, Op::GetSplit, freshness).await?;
        match read.value {
            Observation::Split(on) => Ok(Cached {
                value: on,
                age: read.age,
            }),
            other => Err(RigError::new(
                ErrorKind::Malformed,
                format!("expected split, got {other:?}"),
            )),
        }
    }

    // [@ANCHOR: radioctl_core_handle_tx_state]
    /// Brings the inputs of the transmit-frequency derivation up to date: split state and both
    /// VFO frequencies (the second only when the rig has one), asking the rig for what the
    /// cache lacks or, with [`Freshness::Fresh`], for everything.
    async fn tx_state(&self, freshness: Freshness) -> Result<TxState, RigError> {
        if self.capabilities().has(Capability::Split) {
            self.split(freshness).await?;
            if self.capabilities().has(Capability::SecondVfo) {
                self.frequency(Vfo::B, freshness).await?;
            }
        }
        self.frequency(Vfo::A, freshness).await?;
        Ok(TxState::from_cache(|f| self.cached(f).map(|c| c.value)))
    }

    // [@ANCHOR: radioctl_core_handle_tx_vfo]
    /// The VFO the rig transmits on: VFO B (the unselected VFO on CI-V) in split operation on
    /// most rigs, VFO A otherwise; the rig's own report where its protocol gives one.
    pub async fn tx_vfo(&self, freshness: Freshness) -> Result<Vfo, RigError> {
        Ok(self.tx_state(freshness).await?.tx_vfo())
    }

    // [@ANCHOR: radioctl_core_handle_rx_frequency]
    /// The frequency the rig receives on: VFO A, or the receive VFO the rig reports (Kenwood).
    pub async fn rx_frequency(&self, freshness: Freshness) -> Result<Hz, RigError> {
        self.tx_state(freshness)
            .await?
            .rx_frequency()
            .ok_or_else(|| RigError::new(ErrorKind::Malformed, "receive frequency unknown"))
    }

    // [@ANCHOR: radioctl_core_handle_tx_frequency]
    /// The frequency of the transmit VFO, which is the receive frequency when not split. Unlike
    /// [`RigHandle::frequency`] it never depends on which VFO the caller names.
    pub async fn tx_frequency(&self, freshness: Freshness) -> Result<Hz, RigError> {
        self.tx_state(freshness)
            .await?
            .tx_frequency()
            .ok_or_else(|| RigError::new(ErrorKind::Malformed, "transmit frequency unknown"))
    }

    // [@ANCHOR: radioctl_core_handle_active_tx_frequency]
    /// The frequency the rig transmits on now, or will when keyed, correct in split operation
    /// and while keyed. Amplifiers, tuners, antenna selectors and band decoders follow this; the
    /// status-change callback reports `Field::ActiveTxFrequency` when it changes.
    pub async fn active_tx_frequency(&self, freshness: Freshness) -> Result<Hz, RigError> {
        self.tx_state(freshness)
            .await?
            .active_tx_frequency()
            .ok_or_else(|| RigError::new(ErrorKind::Malformed, "transmit frequency unknown"))
    }

    // [@ANCHOR: radioctl_core_handle_set_frequency]
    /// Sets a VFO's frequency.
    pub async fn set_frequency(&self, vfo: Vfo, hz: Hz) -> Result<(), RigError> {
        self.execute(Op::SetFrequency(vfo, hz)).await.map(|_| ())
    }

    // [@ANCHOR: radioctl_core_handle_mode]
    /// The mode.
    pub async fn mode(&self, freshness: Freshness) -> Result<Cached<Mode>, RigError> {
        let read = self.read(Field::Mode, Op::GetMode, freshness).await?;
        match read.value {
            Observation::Mode(mode) => Ok(Cached {
                value: mode,
                age: read.age,
            }),
            other => Err(RigError::new(
                ErrorKind::Malformed,
                format!("expected mode, got {other:?}"),
            )),
        }
    }

    // [@ANCHOR: radioctl_core_handle_set_mode]
    /// Sets the mode.
    pub async fn set_mode(&self, mode: Mode) -> Result<(), RigError> {
        self.execute(Op::SetMode(mode)).await.map(|_| ())
    }

    // [@ANCHOR: radioctl_core_handle_ptt]
    /// The transmit state.
    pub async fn ptt(&self, freshness: Freshness) -> Result<Cached<bool>, RigError> {
        let read = self.read(Field::Ptt, Op::GetPtt, freshness).await?;
        match read.value {
            Observation::Ptt(on) => Ok(Cached {
                value: on,
                age: read.age,
            }),
            other => Err(RigError::new(
                ErrorKind::Malformed,
                format!("expected ptt, got {other:?}"),
            )),
        }
    }

    // [@ANCHOR: radioctl_core_handle_set_ptt]
    /// Keys (needs the transmit gate open) or unkeys (always allowed, jumps the queue).
    pub async fn set_ptt(&self, on: bool) -> Result<(), RigError> {
        self.execute(Op::SetPtt(on)).await.map(|_| ())
    }

    // [@ANCHOR: radioctl_core_handle_signal]
    /// The S-meter in raw units.
    pub async fn signal(&self, freshness: Freshness) -> Result<Cached<u16>, RigError> {
        let read = self.read(Field::Signal, Op::GetSignal, freshness).await?;
        match read.value {
            Observation::Signal(level) => Ok(Cached {
                value: level,
                age: read.age,
            }),
            other => Err(RigError::new(
                ErrorKind::Malformed,
                format!("expected signal, got {other:?}"),
            )),
        }
    }
}

/// An async stream of scope sweeps.
pub struct ScopeStream {
    rx: broadcast::Receiver<ScopeFrame>,
    shared: Arc<Shared>,
}

impl ScopeStream {
    // [@ANCHOR: radioctl_core_scope_stream_next]
    /// The next sweep, or `None` once the rig has stopped. Frames missed while lagging are
    /// skipped and counted.
    pub async fn next(&mut self) -> Option<ScopeFrame> {
        loop {
            match self.rx.recv().await {
                Ok(frame) => return Some(frame),
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    self.shared
                        .counters
                        .scope_lagged
                        .fetch_add(missed, Ordering::Relaxed);
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

/// An async stream of status changes.
pub struct EventStream {
    rx: broadcast::Receiver<RigEvent>,
    shared: Arc<Shared>,
}

impl EventStream {
    // [@ANCHOR: radioctl_core_event_stream_next]
    /// The next event, or `None` once the rig has stopped. A subscriber that falls behind skips
    /// the events it missed and the miss is counted in the rig's `stream_lagged`.
    pub async fn next(&mut self) -> Option<RigEvent> {
        loop {
            match self.rx.recv().await {
                Ok(event) => return Some(event),
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    self.shared
                        .counters
                        .stream_lagged
                        .fetch_add(missed, Ordering::Relaxed);
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

enum Wait {
    Done(Reply),
    TimedOut,
    Preempted,
    LinkDown(String),
}

struct Actor<E, T> {
    engine: E,
    transport: T,
    shared: Arc<Shared>,
    normal: mpsc::Receiver<Command>,
    emergency: mpsc::Receiver<Command>,
    poll: PollSchedule,
    transcript: VecDeque<String>,
    epoch: Instant,
    buffer: [u8; READ_BUFFER],
    last_write: Option<Instant>,
    /// When the rig was last seen going from receive to transmit; `None` while receiving.
    keyed_since: Option<Instant>,
    max_transmit: Duration,
    heartbeat: Option<Duration>,
    /// The transport's link status, when it has one ([`RigOptions::link_watch`]).
    link_watch: Option<watch::Receiver<LinkStatus>>,
    /// The `generation` of the last `Connected` status seen; 0 before the first.
    link_generation: u32,
}

impl<E: ProtocolEngine, T: Transport> Actor<E, T> {
    // [@ANCHOR: radioctl_core_actor_run]
    async fn run(mut self) {
        // A status that changed before the actor started (the port was lost during spawn) is
        // not an edge the watch would deliver: look once.
        self.link_status_changed().await;
        loop {
            let due = self.poll.next_due();
            let link = self.shared.link_up.load(Ordering::Acquire);
            let watchdog = self.watchdog_due();
            let shared = Arc::clone(&self.shared);
            let has_watch = self.link_watch.is_some();
            tokio::select! {
                biased;
                command = self.emergency.recv() => match command {
                    Some(command) => {
                        self.shared.emergency_pending.fetch_sub(1, Ordering::AcqRel);
                        self.serve(command, false).await;
                    }
                    None => break,
                },
                changed = link_changed(self.link_watch.as_mut()), if has_watch => {
                    if changed {
                        self.link_status_changed().await;
                    } else {
                        // The supervisor is gone: the link will not come back.
                        self.link_watch = None;
                        self.link_down("link status watch closed");
                    }
                }
                _ = wait_until(watchdog) => self.watchdog_fire().await,
                _ = shared.heartbeat_wake.notified(), if watchdog.is_some() => {}
                command = self.normal.recv() => match command {
                    Some(command) => self.serve(command, true).await,
                    None => break,
                },
                read = self.transport.read(&mut self.buffer), if link => match read {
                    Ok(n) if n > 0 => {
                        let data = self.buffer.get(..n).unwrap_or(&[]).to_vec();
                        self.log("RX", &data);
                        self.ingest(&data);
                    }
                    Ok(_) => self.link_down("end of stream"),
                    Err(error) => self.link_down(&error.to_string()),
                },
                _ = wait_until(due), if link => self.poll_once().await,
            }
        }
        self.unkey_on_stop().await;
    }

    // [@ANCHOR: radioctl_core_actor_link_status_changed]
    /// Applies the transport's current link status. `Reconnecting` and `Closed` take the link
    /// down (every command is refused with `LinkDown`, reported once as [`Alarm::LinkDown`]).
    /// `Connected` with a generation the actor has not seen brings the link up; when it is a
    /// reopen (generation above the first seen), the rig is unkeyed through the fail-closed path
    /// before any queued command is served, because the rig may have been keyed when the port
    /// was lost and nothing could unkey it meanwhile. The unkey comes first, then the event
    /// [`Alarm::LinkRestored`], so a subscriber that reacts to the event finds the rig unkeyed.
    async fn link_status_changed(&mut self) {
        let status = match self.link_watch.as_mut() {
            Some(watch) => watch.borrow_and_update().clone(),
            None => return,
        };
        match status {
            LinkStatus::Connected { generation, .. } => {
                let first = self.link_generation == 0;
                if generation == self.link_generation {
                    return;
                }
                self.link_generation = generation;
                let was_up = self.shared.link_up.swap(true, Ordering::AcqRel);
                if first && was_up {
                    return;
                }
                self.transcript
                    .push_back(format!("LINK UP generation {generation}"));
                self.unkey_fail_closed("link restored", Source::Api).await;
                self.alarm(Alarm::LinkRestored, Source::Api);
            }
            LinkStatus::Reconnecting { last_error, .. } => {
                self.link_down(&format!("port lost: {last_error}"));
            }
            LinkStatus::Closed => self.link_down("serial supervisor closed"),
        }
    }

    // [@ANCHOR: radioctl_core_actor_unkey_on_stop]
    /// The last handle is gone: a rig that may be transmitting (its cache says keyed, or its
    /// gate was open) is unkeyed before the actor ends, so a dropped handle never leaves RF on
    /// the air. Best effort: the result goes to the transcript, there is nobody left to tell.
    async fn unkey_on_stop(&mut self) {
        let keyed = matches!(
            guard(&self.shared.cache).get(&Field::Ptt).map(|e| e.value),
            Some(Observation::Ptt(true))
        );
        if keyed || self.shared.gate.load(Ordering::Acquire) {
            self.unkey_fail_closed("actor stopping", Source::Api).await;
        }
    }

    // [@ANCHOR: radioctl_core_actor_watchdog_due]
    /// When the transmit watchdog fires, while the rig transmits: the earlier of the transmit
    /// time limit and the lapse of the caller's heartbeat (counted from the later of the last
    /// heartbeat and the moment of keying, so a caller that keys and then beats is not tripped
    /// by the time before it keyed).
    fn watchdog_due(&self) -> Option<Instant> {
        let keyed = self.keyed_since?;
        let limit = deadline_after(keyed, self.max_transmit);
        let Some(period) = self.heartbeat else {
            return Some(limit);
        };
        let beat = deadline_after(
            self.epoch,
            Duration::from_millis(self.shared.last_heartbeat.load(Ordering::Acquire)),
        );
        Some(limit.min(deadline_after(beat.max(keyed), period)))
    }

    // [@ANCHOR: radioctl_core_actor_watchdog_fire]
    /// The watchdog lapsed: report why, unkey through the fail-closed path, and disarm. A rig
    /// still seen keyed afterwards (the unkey failed, or the operator keyed again) re-arms it.
    async fn watchdog_fire(&mut self) {
        let Some(keyed) = self.keyed_since.take() else {
            return;
        };
        let alarm = if Instant::now() >= deadline_after(keyed, self.max_transmit) {
            Alarm::TransmitTimeout
        } else {
            Alarm::HeartbeatLapsed
        };
        self.shared
            .counters
            .watchdog_unkeys
            .fetch_add(1, Ordering::Relaxed);
        self.alarm(alarm, Source::Watchdog);
        let why = match alarm {
            Alarm::TransmitTimeout => "transmit time limit reached",
            Alarm::HeartbeatLapsed => "caller heartbeat lapsed",
            Alarm::UnkeyFailed => "unkey failed",
            Alarm::LinkDown => "link down",
            Alarm::LinkRestored => "link restored",
        };
        self.unkey_fail_closed(why, Source::Watchdog).await;
    }

    // [@ANCHOR: radioctl_core_actor_alarm]
    /// Emits an alarm event attributed to `source` (the watchdog, or the caller whose unkey
    /// failed). Alarms are never deduplicated and never cached.
    fn alarm(&mut self, alarm: Alarm, source: Source) {
        self.transcript.push_back(format!("ALARM {alarm:?}"));
        self.emit(RigEvent {
            rig: self.shared.id.clone(),
            source,
            changes: vec![Change {
                field: Field::Alarm,
                old: None,
                new: Observation::Alarm(alarm),
            }],
        });
    }

    // [@ANCHOR: radioctl_core_actor_unkey_fail_closed]
    /// Writes an unkey to the rig and waits for its answer within the engine's timeout, not
    /// preemptible, up to `UNKEY_ATTEMPTS` times with a short backoff. Used when the outcome of
    /// a transmit command is unknown, when the watchdog fires and when the actor stops. An
    /// unkey that cannot be done (every attempt unanswered or refused, link down, no command
    /// for it) is counted and reported as [`Alarm::UnkeyFailed`], never dropped silently.
    async fn unkey_fail_closed(&mut self, why: &str, source: Source) {
        self.transcript
            .push_back(format!("FAIL CLOSED unkey: {why}"));
        let request = if self.shared.capabilities.has(Capability::SetPtt) {
            self.engine.encode(&Op::SetPtt(false)).ok()
        } else {
            None
        };
        let timeout = self.engine.description().timing.timeout;
        let mut done = false;
        if let Some(request) = request {
            for attempt in 0..UNKEY_ATTEMPTS {
                if !self.shared.link_up.load(Ordering::Acquire) {
                    break;
                }
                if attempt > 0 {
                    sleep_until(deadline_after(Instant::now(), UNKEY_BACKOFF * attempt)).await;
                }
                let until = deadline_after(Instant::now(), timeout);
                match timeout_at(until, self.transport.write_all(&request.bytes)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        self.link_down(&error.to_string());
                        break;
                    }
                    Err(_) => continue,
                }
                self.last_write = Some(Instant::now());
                self.log("TX", &request.bytes);
                done = request.reply == ReplyKind::None
                    || matches!(
                        self.await_reply(request.reply, until, source, false).await,
                        Wait::Done(Ok(_))
                    );
                if done {
                    break;
                }
            }
        }
        if done {
            self.apply_set(&Op::SetPtt(false), source);
        } else {
            self.shared
                .counters
                .unkey_failures
                .fetch_add(1, Ordering::Relaxed);
            self.alarm(Alarm::UnkeyFailed, source);
        }
    }

    fn now(&self) -> Duration {
        Instant::now().saturating_duration_since(self.epoch)
    }

    // [@ANCHOR: radioctl_core_actor_link_down]
    /// The transport failed: every later command is refused with `LinkDown`, and the loss is
    /// reported once as [`Alarm::LinkDown`] (a keyed rig whose link drops is on the air with no
    /// way to unkey it by command, which the operator must hear about at once).
    fn link_down(&mut self, why: &str) {
        let was_up = self.shared.link_up.swap(false, Ordering::AcqRel);
        self.transcript.push_back(format!("LINK DOWN {why}"));
        if was_up {
            self.alarm(Alarm::LinkDown, Source::Api);
        }
    }

    // [@ANCHOR: radioctl_core_actor_log]
    fn log(&mut self, direction: &str, bytes: &[u8]) {
        let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
        self.transcript
            .push_back(format!("{direction} {}", hex.join(" ")));
        while self.transcript.len() > TRANSCRIPT_LINES {
            self.transcript.pop_front();
        }
    }

    fn tail(&self) -> Vec<String> {
        self.transcript.iter().cloned().collect()
    }

    fn error(&self, kind: ErrorKind, detail: &str) -> RigError {
        RigError::new(kind, detail.to_string()).with_transcript(self.tail())
    }

    // [@ANCHOR: radioctl_core_actor_serve]
    async fn serve(&mut self, command: Command, may_preempt: bool) {
        let result = self
            .run_command(&command.op, command.deadline, command.source, may_preempt)
            .await;
        let counter = if result.is_ok() {
            &self.shared.counters.commands_ok
        } else {
            &self.shared.counters.commands_failed
        };
        counter.fetch_add(1, Ordering::Relaxed);
        if let Err(error) = &result {
            if error.kind == ErrorKind::Cancelled {
                self.shared
                    .counters
                    .preempted
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        if command.op.is_unkey() {
            // An unkey a caller asked for and the rig did not take (timed out, refused, link
            // down) leaves the rig possibly on the air: count and report it, whatever the
            // caller does with its own copy of the result. A failed stop-Morse is the caller's
            // alone: it is not an unkey and nobody rides on it.
            if result.is_err() {
                self.shared
                    .counters
                    .unkey_failures
                    .fetch_add(1, Ordering::Relaxed);
                self.alarm(Alarm::UnkeyFailed, command.source);
            }
            let waiters: Vec<_> = guard(&self.shared.unkey_waiters).drain(..).collect();
            for waiter in waiters {
                let _ = waiter.send(result.clone());
            }
        }
        let _ = command.reply.send(result);
    }

    // [@ANCHOR: radioctl_core_actor_run_command]
    async fn run_command(
        &mut self,
        op: &Op,
        deadline: Instant,
        source: Source,
        may_preempt: bool,
    ) -> Reply {
        if !self.shared.link_up.load(Ordering::Acquire) {
            return Err(self.error(ErrorKind::LinkDown, "transport is down"));
        }
        if Instant::now() >= deadline {
            return Err(self.error(ErrorKind::Timeout, "deadline passed before the command ran"));
        }
        if op.transmits() && !self.shared.gate.load(Ordering::Acquire) {
            return Err(self.error(
                ErrorKind::TransmitGateClosed,
                "transmit gate is closed for this rig",
            ));
        }
        let request = self.engine.encode(op)?;
        let timing = self.engine.description().timing;
        let attempts = if op.transmits() {
            1
        } else {
            1 + u32::from(timing.retries)
        };
        for _ in 0..attempts {
            // Inter-command spacing is a courtesy to the rig's parser; an emergency unkey does
            // not wait for it, so a generous `inter_command_ms` never delays getting off the air.
            if !op.is_emergency() {
                if let Some(ready) = self.spacing(timing.inter_command, deadline) {
                    sleep_until(ready).await;
                }
            }
            match timeout_at(deadline, self.transport.write_all(&request.bytes)).await {
                Err(_) => {
                    // Part of a transmit command may already be on the wire.
                    if op.transmits() {
                        self.unkey_fail_closed("transmit command write timed out", source)
                            .await;
                    }
                    return Err(self.error(
                        ErrorKind::Timeout,
                        "write did not finish before the deadline",
                    ));
                }
                Ok(Err(error)) => {
                    self.link_down(&error.to_string());
                    return Err(self.error(ErrorKind::LinkDown, &error.to_string()));
                }
                Ok(Ok(())) => {}
            }
            self.last_write = Some(Instant::now());
            self.log("TX", &request.bytes);
            if request.reply == ReplyKind::None {
                self.apply_set(op, source);
                return Ok(None);
            }
            let until = deadline.min(deadline_after(Instant::now(), timing.timeout));
            match self
                .await_reply(request.reply, until, source, may_preempt)
                .await
            {
                Wait::Done(result) => {
                    if result.is_ok() {
                        self.apply_set(op, source);
                    }
                    return result;
                }
                Wait::Preempted => {
                    return Err(
                        self.error(ErrorKind::Cancelled, "abandoned for an emergency command")
                    )
                }
                Wait::LinkDown(why) => {
                    self.link_down(&why);
                    return Err(self.error(ErrorKind::LinkDown, &why));
                }
                Wait::TimedOut if Instant::now() >= deadline => break,
                Wait::TimedOut => {}
            }
        }
        // A transmit command was written and never answered: the rig may be keyed. Fail closed
        // and unkey it rather than report a timeout and leave RF on the air.
        if op.transmits() {
            self.unkey_fail_closed("transmit command not answered", source)
                .await;
        }
        Err(self.error(ErrorKind::Timeout, "no reply from the rig"))
    }

    // [@ANCHOR: radioctl_core_actor_spacing]
    /// When the next command may be written, if that is in the future.
    fn spacing(&self, gap: Duration, deadline: Instant) -> Option<Instant> {
        let last = self.last_write?;
        let ready = deadline_after(last, gap).min(deadline);
        (ready > Instant::now()).then_some(ready)
    }

    // [@ANCHOR: radioctl_core_actor_await_reply]
    async fn await_reply(
        &mut self,
        expect: ReplyKind,
        until: Instant,
        source: Source,
        may_preempt: bool,
    ) -> Wait {
        let shared = Arc::clone(&self.shared);
        // The watchdog also preempts a command that waits for a reply, so a slow radio cannot
        // hold the actor past the transmit limit.
        loop {
            let watchdog = self.watchdog_due();
            let woken = shared.emergency_wake.notified();
            let beat = shared.heartbeat_wake.notified();
            tokio::select! {
                biased;
                _ = woken, if may_preempt => {
                    if shared.emergency_pending.load(Ordering::Acquire) > 0 {
                        return Wait::Preempted;
                    }
                }
                _ = wait_until(watchdog), if may_preempt => return Wait::Preempted,
                _ = beat, if may_preempt && watchdog.is_some() => {}
                read = timeout_at(until, self.transport.read(&mut self.buffer)) => match read {
                    Err(_) => return Wait::TimedOut,
                    Ok(Err(error)) => return Wait::LinkDown(error.to_string()),
                    Ok(Ok(0)) => return Wait::LinkDown("end of stream".to_string()),
                    Ok(Ok(n)) => {
                        let data = self.buffer.get(..n).unwrap_or(&[]).to_vec();
                        self.log("RX", &data);
                        if let Some(done) = self.absorb(&data, expect, source) {
                            return Wait::Done(done);
                        }
                    }
                },
            }
        }
    }

    // [@ANCHOR: radioctl_core_actor_absorb]
    /// Feeds received bytes to the engine; returns the command's result once it is decided.
    fn absorb(&mut self, data: &[u8], expect: ReplyKind, source: Source) -> Option<Reply> {
        let mut done: Option<Reply> = None;
        for item in self.engine.feed(data, self.now()) {
            match item {
                Inbound::Observation(value) => {
                    let wanted = done.is_none() && expect == ReplyKind::Observation(value.field());
                    self.observe(value, if wanted { source } else { Source::Transceive });
                    if wanted {
                        done = Some(Ok(Some(value)));
                    } else {
                        self.note_unsolicited();
                    }
                }
                Inbound::ScopeFrame(mut frame) => {
                    let seq = self
                        .shared
                        .counters
                        .scope_frames
                        .fetch_add(1, Ordering::Relaxed)
                        + 1;
                    frame.seq = seq;
                    let _ = self.shared.scope.send(frame);
                }
                Inbound::Ack if done.is_none() && expect == ReplyKind::Ack => done = Some(Ok(None)),
                Inbound::Nak if done.is_none() && expect != ReplyKind::None => {
                    done = Some(Err(
                        self.error(ErrorKind::Rejected, "the rig refused the command")
                    ));
                }
                Inbound::Unrecognised(bytes) => {
                    self.shared
                        .counters
                        .unrecognised
                        .fetch_add(1, Ordering::Relaxed);
                    self.log("??", &bytes);
                }
                Inbound::Ack | Inbound::Nak => {}
            }
        }
        done
    }

    // [@ANCHOR: radioctl_core_actor_note_unsolicited]
    /// A frame arrived that no command was waiting for. On a rig that reports on its own
    /// (`Capability::Transceive`) that is transceive traffic and polling slows to the maximum
    /// interval; on any other rig it is a late reply to a command already given up, counted as
    /// such and never allowed to pin polling for the rig's lifetime.
    fn note_unsolicited(&mut self) {
        if self.shared.capabilities.has(Capability::Transceive) {
            self.shared
                .counters
                .unsolicited
                .fetch_add(1, Ordering::Relaxed);
            self.poll.note_transceive();
        } else {
            self.shared
                .counters
                .late_replies
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Handles bytes that arrive while no command waits (transceive, late replies).
    fn ingest(&mut self, data: &[u8]) {
        self.absorb(data, ReplyKind::None, Source::Transceive);
    }

    // [@ANCHOR: radioctl_core_actor_apply_set]
    /// Records a successful set in the cache; polling reconciles it with the rig later.
    fn apply_set(&mut self, op: &Op, source: Source) {
        match op {
            Op::SetFrequency(vfo, hz) => self.observe(Observation::Frequency(*vfo, *hz), source),
            Op::SetMode(mode) => self.observe(Observation::Mode(*mode), source),
            Op::SetPtt(on) => self.observe(Observation::Ptt(*on), source),
            Op::SetSplit(on) => self.observe(Observation::Split(*on), source),
            Op::SetRit(on) => self.observe(Observation::Rit(*on), source),
            Op::SetXit(on) => self.observe(Observation::Xit(*on), source),
            Op::SetRitOffset(hz) => self.observe(Observation::RitOffset(*hz), source),
            Op::SetFilterWidth(hz) => self.observe(Observation::FilterWidth(*hz), source),
            Op::SetLevel(level, value) => self.observe(Observation::Level(*level, *value), source),
            Op::SetScopeOutput(on) => self.observe(Observation::ScopeOutput(*on), source),
            Op::SetScopeAxis(rx, axis) => self.observe(Observation::ScopeAxis(*rx, *axis), source),
            Op::SetScopeSpan(rx, span) => self.observe(Observation::ScopeSpan(*rx, *span), source),
            Op::SetScopeReference(rx, centi_db) => {
                self.observe(Observation::ScopeReference(*rx, *centi_db), source)
            }
            Op::SendCw(_) => self.arm_watchdog(Instant::now()),
            // A stop clears the CW buffer but says nothing about PTT: the rig may still be keyed
            // by hand or by a `SetPtt(true)`, so the watchdog stays armed until a poll or a
            // transceive report shows the rig receiving.
            Op::StopCw => {}
            _ => {}
        }
    }

    // [@ANCHOR: radioctl_core_actor_arm_watchdog]
    /// Starts the transmit watchdog if it is not running.
    fn arm_watchdog(&mut self, now: Instant) {
        if self.keyed_since.is_none() {
            self.keyed_since = Some(now);
            self.transcript.push_back("WATCHDOG armed".to_string());
        }
    }

    // [@ANCHOR: radioctl_core_actor_observe]
    fn observe(&mut self, value: Observation, source: Source) {
        let field = value.field();
        let now = Instant::now();
        match value {
            Observation::Ptt(true) => self.arm_watchdog(now),
            Observation::Ptt(false) => self.keyed_since = None,
            _ => {}
        }
        let mut changes = Vec::new();
        {
            let mut cache = guard(&self.shared.cache);
            let old = cache.get(&field).map(|e| e.value);
            cache.insert(field, CacheEntry { value, at: now });
            if old != Some(value) {
                changes.push(Change {
                    field,
                    old,
                    new: value,
                });
            }
            // The transmit frequency is derived here, so every engine gets it and the callback
            // fires when it changes, whichever field moved it.
            let state = TxState::from_cache(|f| cache.get(&f).map(|e| e.value));
            if let Some(hz) = state.active_tx_frequency() {
                let derived = Observation::ActiveTxFrequency(hz);
                let before = cache.get(&Field::ActiveTxFrequency).map(|e| e.value);
                if before != Some(derived) {
                    cache.insert(
                        Field::ActiveTxFrequency,
                        CacheEntry {
                            value: derived,
                            at: now,
                        },
                    );
                    changes.push(Change {
                        field: Field::ActiveTxFrequency,
                        old: before,
                        new: derived,
                    });
                }
            }
        }
        if changes.is_empty() {
            return;
        }
        self.emit(RigEvent {
            rig: self.shared.id.clone(),
            source,
            changes,
        });
    }

    // [@ANCHOR: radioctl_core_actor_emit]
    /// Sends an event to the stream subscribers and the callback dispatcher.
    fn emit(&self, event: RigEvent) {
        self.shared.counters.events.fetch_add(1, Ordering::Relaxed);
        let _ = self.shared.events.send(event.clone());
        if let Some(dispatcher) = &self.shared.dispatcher {
            dispatcher.submit(event);
        }
    }

    // [@ANCHOR: radioctl_core_actor_poll_once]
    async fn poll_once(&mut self) {
        let now = Instant::now();
        let transmitting = matches!(
            guard(&self.shared.cache).get(&Field::Ptt).map(|e| e.value),
            Some(Observation::Ptt(true))
        );
        let mut entry = None;
        while self.poll.next_due().is_some_and(|due| due <= now) {
            entry = self.poll.take_due(now, transmitting);
            if entry.is_some() {
                break;
            }
        }
        let Some(entry) = entry else {
            return;
        };
        let before = guard(&self.shared.cache).get(&entry.field).map(|e| e.value);
        let timeout = self.engine.description().timing.timeout;
        self.shared.counters.polls.fetch_add(1, Ordering::Relaxed);
        let result = self
            .run_command(&entry.op, deadline_after(now, timeout), Source::Poll, true)
            .await;
        let after = guard(&self.shared.cache).get(&entry.field).map(|e| e.value);
        self.poll.report(
            entry.field,
            before != after,
            result.is_err(),
            Instant::now(),
        );
    }
}

// [@ANCHOR: radioctl_core_deadline_after]
/// `from + wait`, saturating: a duration too large for the clock (`Duration::MAX`, an absurd
/// timing value from a rig file) gives the farthest representable instant instead of a panic
/// that would kill the actor or the caller's task.
fn deadline_after(from: Instant, wait: Duration) -> Instant {
    from.checked_add(wait).unwrap_or_else(|| {
        let mut wait = wait;
        loop {
            wait /= 2;
            if let Some(at) = from.checked_add(wait) {
                return at;
            }
        }
    })
}

// [@ANCHOR: radioctl_core_link_changed]
/// Waits for the next link status change; `false` when the watch's sender is gone. Pending
/// forever without a watch (the branch that calls it is disabled then).
async fn link_changed(watch: Option<&mut watch::Receiver<LinkStatus>>) -> bool {
    match watch {
        Some(watch) => watch.changed().await.is_ok(),
        None => std::future::pending::<bool>().await,
    }
}

// [@ANCHOR: radioctl_core_wait_until]
async fn wait_until(due: Option<Instant>) {
    match due {
        Some(at) => sleep_until(at).await,
        None => std::future::pending::<()>().await,
    }
}

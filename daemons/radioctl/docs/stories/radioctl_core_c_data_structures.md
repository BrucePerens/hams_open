# Data structures

### `RigOptions` [@ANCHOR: radioctl:radioctl_core_rig_rig_options]

**Purpose and location.** Options for a rig. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:68` as a pub `struct`. It derives Clone.

| Field | Type | Meaning |
|---|---|---|
| `queue_capacity` | `usize` | How many ordinary commands may wait in the queue. |
| `default_timeout` | `Duration` | Deadline applied when a call does not give one. |
| `poll` | `PollConfig` | Polling. |
| `stream_capacity` | `usize` | Events held per async-stream subscriber before it lags. |
| `scope_capacity` | `usize` | Scope frames held per scope-stream subscriber before it lags. |
| `link_watch` | `Option<watch::Receiver<LinkStatus>>` | The transport's link status, when the transport can report it ([`crate::serial::SerialLink::watch`]). The serial supervisor keeps the byte stream alive across a port loss (writes are dropped and counted, not failed), so without this the actor would never learn that the port went away or came back. With it, the actor marks the link down (and refuses commands with `LinkDown`) while the port is `Reconnecting`, and on every reopen after the first it marks the link up and unkeys the rig before serving any other command: the rig may have been keyed when the link dropped. |
| `max_transmit` | `Duration` | The longest the rig may transmit in one go before the actor unkeys it on its own ([`DEFAULT_MAX_TRANSMIT`] when the owner sets none). Measured from the moment the rig is keyed or Morse is sent; `Duration::MAX` disables the limit, which the owner must decide. |
| `heartbeat` | `Option<Duration>` | A dead-man period: while the rig transmits, the caller must call [`RigHandle::heartbeat`] at least this often or the actor unkeys the rig. `None` (the default) relies on `max_transmit` alone. Not enforced while receiving. |
| `control_lines` | `Option<Arc<dyn ControlLines>>` | The transport's control lines, for a rig whose commands are control-line sequences ([`ProtocolEngine::encode_lines`]). `None` (the default) refuses such a command with `Unsupported`. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `default` (src/rig.rs:102), `quiet` (tests/actor.rs:31), `civ` (tests/actor.rs:41), `kenwood` (tests/actor.rs:52), `quiet` (tests/easy.rs:17).

**See also:** test `the_transmit_state_is_still_polled_under_a_flood_of_client_commands (tests/actor.rs)`.

### `TransmitPermit` [@ANCHOR: radioctl:radioctl_core_rig_transmit_permit]

**Purpose and location.** Proof that someone decided transmitting is allowed on one rig. There is no bool shortcut. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:119` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `granted_by` | `String` | Who or what decided that transmitting is allowed (recorded text). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/rig.rs:126), `open_transmit_gate` (src/rig.rs:421), `transmit_permit` (src/rig.rs:435), `permit` (tests/actor.rs:61), `permit` (tests/easy.rs:42).

**See also:** test `a_cut_connection_is_reconnected_and_the_rig_is_unkeyed_first (tests/net.rs)`.

### `Freshness` [@ANCHOR: radioctl:radioctl_core_rig_freshness]

**Purpose and location.** How fresh a read must be. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:141` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Cached` | `Cached` | Any cached value, however old; asks the rig only if nothing is cached. |
| `MaxAge` | `MaxAge(Duration)` | A cached value no older than the duration. |
| `Fresh` | `Fresh` | Always asks the rig. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `frequency` (src/rig.rs:670), `split` (src/rig.rs:688), `tx_state` (src/rig.rs:706), `tx_vfo` (src/rig.rs:720), `rx_frequency` (src/rig.rs:726), `tx_frequency` (src/rig.rs:736), `active_tx_frequency` (src/rig.rs:747), `mode` (src/rig.rs:762).

**See also:** test `commands_round_trip_and_the_cache_reports_age (tests/actor.rs)`.

### `Cached` [@ANCHOR: radioctl:radioctl_core_rig_cached]

**Purpose and location.** A value with the age of the information. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:152` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `value` | `T` | The value. |
| `age` | `Duration` | Time since the rig last reported it. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `cached` (src/rig.rs:526), `frequency` (src/rig.rs:670), `split` (src/rig.rs:688), `mode` (src/rig.rs:762), `ptt` (src/rig.rs:784), `signal` (src/rig.rs:806), `level` (src/rig/easy.rs:25), `meter` (src/rig/easy.rs:49).

**See also:** test `the_inputs_come_from_the_cached_observations (src/txstate.rs)`.

### `RigStats` [@ANCHOR: radioctl:radioctl_core_rig_rig_stats]

**Purpose and location.** Counters of one rig. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:161` as a pub `struct`. It derives Debug, Clone, Copy, Default, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `commands_ok` | `u64` | Commands that succeeded. |
| `commands_failed` | `u64` | Commands that failed. |
| `preempted` | `u64` | Commands abandoned for an emergency command. |
| `polls` | `u64` | Polls issued. |
| `reserved_polls` | `u64` | Polls of the transmit state run ahead of a queued command because none had been done for `PollConfig::ptt_reserve` (counted in `polls` too). |
| `unsolicited` | `u64` | Observations the rig sent on its own. |
| `late_replies` | `u64` | Replies that arrived after their command had been given up: matched to that command by order (`absorb`), or, when no request is outstanding, counted as such on a rig without the `Transceive` capability. They never slow polling. |
| `stale_replies` | `u64` | Late replies that fitted the command then waiting and would have been taken for its answer without the order rule (`absorb`): matched to the abandoned command instead. |
| `stale_drains` | `u64` | Times a normal command waited for an abandoned command's late reply before being written. |
| `unrecognised` | `u64` | Received bytes no frame could be made of. |
| `events` | `u64` | Events produced. |
| `stream_lagged` | `u64` | Events a stream subscriber missed because it lagged. |
| `scope_frames` | `u64` | Scope frames published. |
| `scope_lagged` | `u64` | Scope frames a stream subscriber missed because it lagged. |
| `filter_scope_frames` | `u64` | Filter-scope frames published. |
| `filter_scope_lagged` | `u64` | Filter-scope frames a stream subscriber missed because it lagged. |
| `watchdog_unkeys` | `u64` | Unkeys the transmit watchdog forced (time limit or heartbeat lapse). |
| `unkey_failures` | `u64` | Unkeys the actor tried on its own and could not complete: the rig may be on the air. |
| `unkeys_coalesced` | `u64` | Unkeys that found the emergency queue full and rode on the one already queued. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `stats` (src/rig.rs:445).

**See also:** test `a_late_reply_on_a_polled_rig_is_not_taken_for_transceive (tests/actor.rs)`.

### `Counters` [@ANCHOR: radioctl:radioctl_core_rig_counters]

**Purpose and location.** The atomic counters of one rig, behind `RigStats`; each field has the meaning of the `RigStats` field of the same name. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:207` as a private `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `commands_ok` | `AtomicU64` | Commands that succeeded. |
| `commands_failed` | `AtomicU64` | Commands that failed. |
| `preempted` | `AtomicU64` | Commands abandoned for an emergency command. |
| `polls` | `AtomicU64` | Polls issued. |
| `reserved_polls` | `AtomicU64` | Transmit-state polls run ahead of a queued command (also counted in `polls`). |
| `unsolicited` | `AtomicU64` | Observations the rig sent on its own. |
| `late_replies` | `AtomicU64` | Replies that arrived after their command had been given up. |
| `stale_replies` | `AtomicU64` | Late replies that fitted the command then waiting and were matched to the abandoned command instead. |
| `stale_drains` | `AtomicU64` | Times a normal command waited for an abandoned command's late reply. |
| `unrecognised` | `AtomicU64` | Received bytes no frame could be made of. |
| `events` | `AtomicU64` | Events produced. |
| `stream_lagged` | `AtomicU64` | Events a stream subscriber missed because it lagged. |
| `scope_frames` | `AtomicU64` | Scope frames published. |
| `scope_lagged` | `AtomicU64` | Scope frames a subscriber missed. |
| `filter_scope_frames` | `AtomicU64` | Filter-scope frames published. |
| `filter_scope_lagged` | `AtomicU64` | Filter-scope frames a subscriber missed. |
| `watchdog_unkeys` | `AtomicU64` | Unkeys the transmit watchdog forced (time limit or heartbeat lapse). |
| `unkey_failures` | `AtomicU64` | Unkeys the actor tried and could not complete: the rig may be on the air. |
| `unkeys_coalesced` | `AtomicU64` | Unkeys that found the emergency queue full and rode on the one already queued. |

**Invariants, ownership and concurrency.** Relaxed atomics, written by the actor and read by any handle without a lock.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `snapshot` (src/net.rs:280), `snapshot` (src/serial.rs:432), `snapshot` (src/usb_link.rs:88).

**See also:** test `small_helpers (src/net.rs)`.

### `CacheEntry` [@ANCHOR: radioctl:radioctl_core_rig_cache_entry]

**Purpose and location.** One cached observation and when it arrived, so a read can be answered with its age. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:229` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `value` | `Observation` | The observation. |
| `at` | `Instant` | When it was received. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `two_rigs_on_two_runtimes_work_and_a_handle_works_with_no_runtime_at_all (tests/actor.rs)`.

### `Shared` [@ANCHOR: radioctl:radioctl_core_rig_shared]

**Purpose and location.** The state shared by every `RigHandle` of a rig and its actor task. It holds the transmit gate and permit, the link flag, the observation cache and the broadcast channels, so a handle can answer or refuse without asking the actor. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:234` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `id` | `String` | The rig's id from its description. |
| `capabilities` | `CapabilitySet` | What the rig can do (from the description). |
| `cache` | `Mutex<HashMap<Field, CacheEntry>>` | The latest observation of each field, behind a mutex. |
| `gate` | `AtomicBool` | The transmit gate (atomic); closed unless a permit holder opened it. Keying checks it before anything is queued. |
| `permit` | `Mutex<Option<TransmitPermit>>` | The transmit permit, behind a mutex; the gate cannot be opened without one. |
| `link_up` | `AtomicBool` | Whether the transport reports a usable link (atomic). |
| `emergency_pending` | `AtomicUsize` | How many emergency commands (unkey, stop Morse) are queued. |
| `emergency_wake` | `Notify` | Notified so the actor serves an emergency command at once. |
| `unkey_waiters` | `Mutex<Vec<oneshot::Sender<Reply>>>` | Unkeys that found the emergency queue full; answered with the next unkey's result. |
| `last_heartbeat` | `AtomicU64` | Milliseconds since `epoch` of the last heartbeat. |
| `heartbeat_wake` | `Notify` | Woken by every heartbeat so the actor recomputes its watchdog deadline. |
| `epoch` | `Instant` | The start instant, the reference for the heartbeat time. |
| `events` | `broadcast::Sender<RigEvent>` | Broadcast of `RigEvent`s. |
| `scope` | `broadcast::Sender<ScopeFrame>` | Broadcast of scope frames. |
| `filter_scope` | `broadcast::Sender<FilterScopeFrame>` | Broadcast of filter-scope frames. |
| `counters` | `Counters` | The counters. |
| `default_timeout` | `Duration` | The default time a command waits for its reply. |

**Invariants, ownership and concurrency.** Held in an `Arc` by the handles and the actor. Fields are atomics, mutexes (taken through `guard`, which tolerates poisoning), or tokio notifiers; no lock is held across an await. Transmit is fail-closed: gate and permit are both required, and unkey paths never need either.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `two_rigs_on_two_runtimes_work_and_a_handle_works_with_no_runtime_at_all (tests/actor.rs)`.

### `Reply` [@ANCHOR: radioctl:radioctl_core_rig_reply]

**Purpose and location.** The result of one command: an optional observation, or an error. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:263` as a private `type`. It derives no traits.

**Definition.** `type Reply = Result<Option<Observation>, RigError>;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `execute` (src/rig.rs:536), `execute_within` (src/rig.rs:543), `execute_from` (src/rig.rs:554).

**See also:** test `two_rigs_on_two_runtimes_work_and_a_handle_works_with_no_runtime_at_all (tests/actor.rs)`.

### `Command` [@ANCHOR: radioctl:radioctl_core_rig_command]

**Purpose and location.** One queued command: the operation, its deadline, who sent it and where to send the reply. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:265` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `op` | `Op` | The operation. |
| `deadline` | `Instant` | When the caller stops waiting. |
| `source` | `Source` | Whether it came from a client or from the poller. |
| `reply` | `oneshot::Sender<Reply>` | The one-shot channel the reply goes to. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `serve` (src/rig.rs:1391).

**See also:** test `the_c_program_passes_against_the_shared_library (../radioctl-capi/tests/c_program.rs)`.

### `Senders` [@ANCHOR: radioctl:radioctl_core_rig_senders]

**Purpose and location.** The two queues into the actor. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:272` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `normal` | `mpsc::Sender<Command>` | The normal queue. |
| `emergency` | `mpsc::Sender<Command>` | The emergency queue (unkey, stop Morse), served before the normal one. |

**Invariants, ownership and concurrency.** Plain data: its fields hold mpsc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `two_rigs_on_two_runtimes_work_and_a_handle_works_with_no_runtime_at_all (tests/actor.rs)`.

### `RigHandle` [@ANCHOR: radioctl:radioctl_core_rig_rig_handle]

**Purpose and location.** A handle to a running rig. Clone it freely; every clone talks to the same actor. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:279` as a pub `struct`. It derives Clone.

| Field | Type | Meaning |
|---|---|---|
| `shared` | `Arc<Shared>` | The shared state. |
| `senders` | `Arc<Senders>` | The queues into the actor. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `key` (src/keyer.rs:79), `civ` (tests/actor.rs:41), `kenwood` (tests/actor.rs:52), `civ_with_timing` (tests/actor.rs:1102), `ic7300` (tests/easy.rs:27).

**See also:** test `dropping_the_last_handle_unkeys_a_keyed_rig (tests/actor.rs)`.

### `OwnedRig` [@ANCHOR: radioctl:radioctl_core_rig_owned_rig]

**Purpose and location.** A rig together with the small runtime that runs it. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:368` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `handle` | `RigHandle` | The rig's handle. |
| `runtime` | `tokio::runtime::Runtime` | The small runtime that runs the actor; dropped with the rig. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `two_rigs_on_two_runtimes_work_and_a_handle_works_with_no_runtime_at_all (tests/actor.rs)`.

### `ScopeStream` [@ANCHOR: radioctl:radioctl_core_rig_scope_stream]

**Purpose and location.** An async stream of scope sweeps. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:822` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `broadcast::Receiver<ScopeFrame>` | The broadcast receiver of scope frames. |
| `shared` | `Weak<Shared>` | A weak reference to the rig, so the stream ends when the rig is dropped. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `scope_frames` (src/rig.rs:506), `start_scope` (src/rig/easy.rs:68).

**See also:** test `a_scope_stream_starts_subscribed_and_stops (tests/easy.rs)`.

### `FilterScopeStream` [@ANCHOR: radioctl:radioctl_core_rig_filter_scope_stream]

**Purpose and location.** An async stream of filter-scope sweeps. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:850` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `broadcast::Receiver<FilterScopeFrame>` | The broadcast receiver of filter-scope frames. |
| `shared` | `Weak<Shared>` | A weak reference to the rig. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `filter_scope_frames` (src/rig.rs:517).

**See also:** test `a_scope_stream_starts_subscribed_and_stops (tests/easy.rs)`.

### `EventRecv` [@ANCHOR: radioctl:radioctl_core_rig_event_recv]

**Purpose and location.** The boxed future that receives the next event from the broadcast channel. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:877` as a private `type`. It derives no traits.

**Definition.** `type EventRecv = Pin<` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `stream_trait_delivers_events_in_order_with_old_and_new (tests/actor.rs)`.

### `EventStream` [@ANCHOR: radioctl:radioctl_core_rig_event_stream]

**Purpose and location.** The status changes of one rig: a `futures_core::Stream<Item = RigEvent>` backed by a tokio broadcast channel. Each subscriber owns its stream and drives it from its own task, so a slow or panicking subscriber never touches the actor, the unkey path or other subscribers. A subscriber that falls more than `RigOptions::stream_capacity` events behind skips the oldest events; the skipped count is added to `RigStats::stream_lagged` and to this stream's [`EventStream::lagged`], and the stream goes on with the next event it still has. The stream ends (`None`) when the rig stops. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:895` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `Option<broadcast::Receiver<RigEvent>>` | The broadcast receiver, `None` while a receive is pending. |
| `pending` | `Option<EventRecv>` | The pending receive, if any. |
| `shared` | `Weak<Shared>` | A weak reference to the rig, so the stream ends when the rig is dropped. |
| `lagged` | `u64` | How many events were missed because the subscriber lagged. |
| `done` | `bool` | Set when the stream ended. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `events` (src/rig.rs:492), `drain` (tests/actor.rs:526), `stream_next` (tests/actor.rs:649).

**See also:** test `stream_trait_delivers_events_in_order_with_old_and_new (tests/actor.rs)`.

### `Wait` [@ANCHOR: radioctl:radioctl_core_rig_wait]

**Purpose and location.** How waiting for a command's reply ended. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:975` as a private `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Done` | `Done(Reply)` | The reply arrived. |
| `TimedOut` | `TimedOut` | The deadline passed. |
| `Preempted` | `Preempted` | An emergency command took over. |
| `LinkDown` | `LinkDown(String)` | The link went down; the text says why. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `drain_stale` (src/rig.rs:1713).

**See also:** test `two_rigs_on_two_runtimes_work_and_a_handle_works_with_no_runtime_at_all (tests/actor.rs)`.

### `Actor` [@ANCHOR: radioctl:radioctl_core_rig_actor]

**Purpose and location.** The rig actor: the task state of one rig. It owns the engine and the transport, serves the normal and emergency queues, runs the poll schedule, keeps the transcript, enforces the transmit time limit and heartbeat, and drives control lines. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:1006` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `engine` | `E` | The protocol engine. |
| `transport` | `T` | The byte transport. |
| `shared` | `Arc<Shared>` | The state shared with handles. |
| `normal` | `mpsc::Receiver<Command>` | The normal command queue. |
| `emergency` | `mpsc::Receiver<Command>` | The emergency command queue. |
| `poll` | `PollSchedule` | The poll schedule. |
| `transcript` | `VecDeque<String>` | The last exchanges as text, for diagnostics. |
| `epoch` | `Instant` | The start instant. |
| `buffer` | `[u8; READ_BUFFER]` | The read buffer. |
| `last_write` | `Option<Instant>` | When the last bytes were written, for the inter-command delay. |
| `keyed_since` | `Option<Instant>` | When the rig was last seen going from receive to transmit; `None` while receiving. |
| `max_transmit` | `Duration` | The longest the rig may stay keyed before the watchdog unkeys it. |
| `heartbeat` | `Option<Duration>` | The heartbeat period; a lapse unkeys the rig. `None` when no heartbeat is required. |
| `ptt_reserve` | `Duration` | [`PollConfig::ptt_reserve`]. |
| `link_watch` | `Option<watch::Receiver<LinkStatus>>` | The transport's link status, when it has one ([`RigOptions::link_watch`]). |
| `control_lines` | `Option<Arc<dyn ControlLines>>` | The control lines for engines whose commands are line sequences. |
| `link_generation` | `u32` | The `generation` of the last `Connected` status seen; 0 before the first. |
| `session_opened` | `bool` | The open or restore requests have been written (once per actor start). |
| `outstanding` | `VecDeque<Outstanding>` | Requests written whose reply has not been seen: the current command's and those of commands abandoned (timed out, preempted) whose answer may still arrive. Oldest first, the order the rig answers in. |
| `command_seq` | `u64` | Identifies one `run_command` or fail-closed unkey (its retries share it). |

**Invariants, ownership and concurrency.** Owned by a single tokio task (`run`); only that task touches it. The emergency queue is always served first, and the keyed time limit applies whatever the callers do.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_rig_actor_reads_a_virtual_kx3_through_a_pty (tests/transports.rs)`.

### `Outstanding` [@ANCHOR: radioctl:radioctl_core_rig_outstanding]

**Purpose and location.** A request on the wire whose reply has not been seen. Defined in `daemons/radioctl/crates/radioctl-core/src/rig.rs:1040` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `kind` | `ReplyKind` | The kind of reply expected. |
| `seq` | `u64` | The sequence number of the request. |
| `expires` | `Instant` | After this a reply is no longer expected: the entry is dropped so that an answer the rig never gives cannot eat a later command's reply for ever. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `two_rigs_on_two_runtimes_work_and_a_handle_works_with_no_runtime_at_all (tests/actor.rs)`.
### `ProvenanceKind` [@ANCHOR: radioctl:radioctl_core_rigfile_provenance_kind]

**Purpose and location.** Where a datum came from. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:45` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Manual` | `Manual` | A vendor manual; `reference` names the document and section. |
| `Capture` | `Capture` | A recorded capture of a real radio; `reference` names the capture. |
| `Owner` | `Owner` | Run by the owner on a radio; `reference` says who and when. |
| `RepoCode` | `RepoCode` | Existing code in this repository; `reference` names the file and symbol. |
| `ProgramOutput` | `ProgramOutput` | The output of a program run; `reference` names the command. |
| `HamlibSource` | `HamlibSource` | A fact read from Hamlib's source (LGPL; facts only, no code or table copied): used only where no vendor manual could be used. `reference` names the Hamlib release, file and function; `note` says why a manual was not the source. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_complete_description_loads_with_its_facts (src/rigfile.rs)`.

### `Provenance` [@ANCHOR: radioctl:radioctl_core_rigfile_provenance]

**Purpose and location.** A provenance record. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:65` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `kind` | `ProvenanceKind` | What kind of source. |
| `reference` | `String` | The precise source. |
| `note` | `String` | Anything a reader should know about the source. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_complete_description_loads_with_its_facts (src/rigfile.rs)`.

### `Verified` [@ANCHOR: radioctl:radioctl_core_rigfile_verified]

**Purpose and location.** What has been checked about a datum. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:77` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Unverified` | `Unverified` | Nothing has been checked. |
| `ManualChecked` | `ManualChecked { reference: String, }` | Compared with a vendor manual. The document and section. |
| `TranscriptChecked` | `TranscriptChecked { reference: String, }` | Replayed against a recorded transcript. The transcript. |
| `HardwareVerified` | `HardwareVerified { radio: String, firmware: String, date: String, tested: String, }` | Tested on a real radio. The radio. Its firmware version. The date, `YYYY-MM-DD`. Exactly what was tested. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `build_verified` (src/rigfile.rs:1249).

**See also:** test `an_exact_fact_wins_and_the_summary_counts_states (src/rigfile.rs)`.

### `Fact` [@ANCHOR: radioctl:radioctl_core_rigfile_fact]

**Purpose and location.** One provenance and verification record for a key or a key prefix. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:105` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `key` | `String` | An exact key, or a prefix ending in `.*`. |
| `provenance` | `Provenance` | Where the datum came from. |
| `verified` | `Verified` | What has been checked. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `build_facts` (src/rigfile.rs:1226), `check_coverage` (src/rigfile.rs:1464), `fact_for` (src/rigfile.rs:1485).

**See also:** test `a_complete_description_loads_with_its_facts (src/rigfile.rs)`.

### `LoadedDescription` [@ANCHOR: radioctl:radioctl_core_rigfile_loaded_description]

**Purpose and location.** A parsed description together with the record of where its data came from. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:116` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `description` | `RigDescription` | The description. |
| `facts` | `Vec<Fact>` | Provenance and verification records. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `load` (src/rigfile.rs:493), `parse` (src/rigfile.rs:507).

**See also:** test `a_complete_description_loads_with_its_facts (src/rigfile.rs)`.

### `FileDoc` [@ANCHOR: radioctl:radioctl_core_rigfile_file_doc]

**Purpose and location.** The whole rig description file as the TOML parser reads it (schema 1). Unknown keys are an error (`deny_unknown_fields`), and every datum needs a `[[facts]]` entry. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:125` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `schema` | `u32` | The schema version; only 1 is accepted. |
| `rig` | `RigSection` | The `[rig]` table. |
| `civ` | `Option<CivSection>` | The `[civ]` table for Icom CI-V rigs. |
| `kenwood` | `Option<KenwoodSection>` | The `[kenwood]` table. |
| `elecraft` | `Option<ElecraftSection>` | The `[elecraft]` table. |
| `yaesu` | `Option<YaesuSection>` | The `[yaesu]` table (newcat). |
| `yaesu_legacy` | `Option<YaesuLegacySection>` | The `[yaesu_legacy]` table (five-byte CAT). |
| `kenwood_hh` | `Option<KenwoodHhSection>` | The `[kenwood_hh]` table (handhelds and mobiles). |
| `flex` | `Option<FlexSection>` | The `[flex]` table. |
| `hpsdr` | `Option<HpsdrSection>` | The `[hpsdr]` table. |
| `vendor` | `Option<VendorSection>` | The `[vendor]` table for a serial vendor protocol. |
| `receiver` | `Option<ReceiverSection>` | The `[receiver]` table for a receiver family. |
| `usb` | `Option<UsbSection>` | The `[usb]` table for a USB device. |
| `scope` | `Option<ScopeSection>` | The optional `[scope]` table. |
| `levels` | `Option<LevelsSection>` | The optional `[levels]` table. |
| `power_meter` | `Option<PowerMeterSection>` | The optional `[power_meter]` table. |
| `timing` | `TimingSection` | The `[timing]` table. |
| `defaults` | `DefaultsSection` | The `[defaults]` table. |
| `capabilities` | `Vec<String>` | The capability names the rig has. |
| `modes` | `Vec<ModeEntry>` | The `[[modes]]` entries (mode name and code). |
| `facts` | `Vec<FactEntry>` | The `[[facts]]` provenance and verification records. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `build_description` (src/rigfile.rs:557), `build_facts` (src/rigfile.rs:1226).

**See also:** test `a_complete_description_loads_with_its_facts (src/rigfile.rs)`.

### `RigSection` [@ANCHOR: radioctl:radioctl_core_rigfile_rig_section]

**Purpose and location.** The `[rig]` table. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:154` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `id` | `String` | The description id (also the file stem). |
| `name` | `String` | The display name. |
| `family` | `String` | The protocol family name. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_complete_description_loads_with_its_facts (src/rigfile.rs)`.

### `CivSection` [@ANCHOR: radioctl:radioctl_core_rigfile_civ_section]

**Purpose and location.** The `[civ]` table: Icom CI-V parameters. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:162` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `address` | `u8` | The rig's CI-V address. |
| `controller` | `u8` | The controller's address. |
| `model_id` | `u8` | The model id byte. |
| `echo` | `bool` | Whether the bus echoes each frame back. |
| `frequency_bytes` | `u8` | Bytes in a frequency (4, 5 or 6); a non-default value needs its fact. |
| `command_set` | `CivCommandSetName` | The command set name. |
| `main_sub` | `CivMainSubName` | `"select"` or `"full"` (see `CivMainSub`); absent means none. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `civ_frequency_bytes_is_four_five_or_six_and_needs_its_fact (src/rigfile.rs)`.

### `CivMainSubName` [@ANCHOR: radioctl:radioctl_core_rigfile_civ_main_sub_name]

**Purpose and location.** The `main_sub` setting of a CI-V description. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:178` as a private `enum`. It derives Deserialize, Default, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `None` | `None` | No Main/Sub band commands. |
| `Select` | `Select` | Select the band with the Main/Sub commands. |
| `Full` | `Full` | The full Main/Sub command set. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `civ_frequency_bytes_is_four_five_or_six_and_needs_its_fact (src/rigfile.rs)`.

### `CivCommandSetName` [@ANCHOR: radioctl:radioctl_core_rigfile_civ_command_set_name]

**Purpose and location.** The CI-V command set name in the file. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:187` as a private `enum`. It derives Deserialize, Default, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Standard` | `Standard` | The standard command numbers. |
| `F8101` | `F8101` | The command set of the Icom IC-F8101 style rigs. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `civ_frequency_bytes_is_four_five_or_six_and_needs_its_fact (src/rigfile.rs)`.

### `KenwoodSection` [@ANCHOR: radioctl:radioctl_core_rigfile_kenwood_section]

**Purpose and location.** The `[kenwood]` table. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:199` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `model_id` | `u16` | The model id. |
| `dialect` | `Option<String>` | The dialect name (only when not `classic`); needs its fact. |
| `ky` | `Option<KySection>` | The `KY` (send CW) block, if the model has one. |
| `scope_link` | `Option<String>` | `lan` (the default) or the other link a scope uses. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_kenwood_dialect_is_named_known_and_needs_its_fact (src/rigfile.rs)`.

### `KySection` [@ANCHOR: radioctl:radioctl_core_rigfile_ky_section]

**Purpose and location.** The `KY` block of a Kenwood description. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:211` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `form` | `String` | How the model takes `KY` text. |
| `width` | `Option<u8>` | Fixed width of the text field, if the form has one. |
| `lowercase` | `bool` | Whether lower case is accepted. |
| `at_sign` | `bool` | Whether the `@` character is accepted. |
| `stop` | `Option<String>` | How a message in flight is stopped. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `build_ky` (src/rigfile.rs:524).

**See also:** test `a_kenwood_dialect_is_named_known_and_needs_its_fact (src/rigfile.rs)`.

### `ElecraftSection` [@ANCHOR: radioctl:radioctl_core_rigfile_elecraft_section]

**Purpose and location.** The `[elecraft]` table. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:225` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `model_id` | `u16` | The model id. |
| `levels` | `BTreeMap<String, (u16, u16)>` | Per-model level ranges, `level-name = [min, max]` (see `ElecraftParams::level_ranges`). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `build_elecraft` (src/rigfile.rs:942).

**See also:** test `the_elecraft_family_needs_its_own_block_and_fact (src/rigfile.rs)`.

### `YaesuSection` [@ANCHOR: radioctl:radioctl_core_rigfile_yaesu_section]

**Purpose and location.** The `[yaesu]` table (newcat). Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:234` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `model_id` | `u16` | The model id. |
| `frequency_digits` | `Option<u8>` | Digits in a frequency (8 or 9); a non-default value needs its fact. |
| `levels` | `BTreeMap<String, (u16, u16)>` | Per-model level ranges, `level-name = [min, max]` (see `YaesuParams::level_ranges`). |
| `clarifier` | `Option<String>` | `"cf-receive"`: the clarifier is the `CF` command (see `YaesuClarifier`); `"rt-xt"` is the default. |
| `split` | `Option<String>` | `"st"`: split is the `ST` command (see `YaesuParams::split_st`); `"ft"` is the default. |
| `tx_band` | `bool` | `true`: `FT` picks the transmitting band, Main or Sub (see `YaesuParams::tx_band`). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_yaesu_frequency_width_is_8_or_9_and_a_non_default_needs_its_fact (src/rigfile.rs)`.

### `YaesuLegacySection` [@ANCHOR: radioctl:radioctl_core_rigfile_yaesu_legacy_section]

**Purpose and location.** The `[yaesu_legacy]` table: the five-byte CAT commands and status reads of a legacy Yaesu rig. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:254` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `stop_bits` | `u8` | Stop bits on the serial line. |
| `high_band_flag` | `bool` | Whether the status carries a high-band flag. |
| `echo` | `bool` | Whether the rig echoes commands. |
| `set_frequency` | `Option<LegacyCommandEntry>` | The set-frequency command. |
| `set_mode` | `Option<LegacyCommandEntry>` | The set-mode command. |
| `ptt_on` | `Option<[u8; 5]>` | The five bytes that key the rig. |
| `ptt_off` | `Option<[u8; 5]>` | The five bytes that unkey the rig. |
| `split_on` | `Option<[u8; 5]>` | The five bytes that switch split on. |
| `split_off` | `Option<[u8; 5]>` | The five bytes that switch split off. |
| `reads` | `Vec<LegacyReadEntry>` | The status reads (request, reply length and field positions). |
| `status_modes` | `Vec<ModeEntry>` | The mode codes as they appear in a status reply (the command codes are used when empty). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `build_legacy_block` (src/rigfile.rs:1104).

**See also:** test `yaesu_legacy_in_split_refuses_a_key_on_because_vfo_b_is_unreadable (../radioctl-protocols/tests/tx_frequency.rs)`.

### `LegacyCommandEntry` [@ANCHOR: radioctl:radioctl_core_rigfile_legacy_command_entry]

**Purpose and location.** One five-byte command with a slot for a parameter. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:274` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `template` | `[u8; 5]` | The five bytes. |
| `at` | `u8` | Where in the command the parameter goes. |
| `form` | `Option<String>` | How the parameter is written. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `yaesu_legacy_in_split_refuses_a_key_on_because_vfo_b_is_unreadable (../radioctl-protocols/tests/tx_frequency.rs)`.

### `LegacyFreqEntry` [@ANCHOR: radioctl:radioctl_core_rigfile_legacy_freq_entry]

**Purpose and location.** Where a frequency lies in a status reply. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:282` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `offset` | `u16` | The byte offset. |
| `form` | `String` | How the frequency is written. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `yaesu_legacy_in_split_refuses_a_key_on_because_vfo_b_is_unreadable (../radioctl-protocols/tests/tx_frequency.rs)`.

### `LegacyModeEntry` [@ANCHOR: radioctl:radioctl_core_rigfile_legacy_mode_entry]

**Purpose and location.** Where the mode lies in a status reply. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:289` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `offset` | `u16` | The byte offset. |
| `mask` | `u8` | The bits that hold the mode. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `yaesu_legacy_in_split_refuses_a_key_on_because_vfo_b_is_unreadable (../radioctl-protocols/tests/tx_frequency.rs)`.

### `LegacyFlagEntry` [@ANCHOR: radioctl:radioctl_core_rigfile_legacy_flag_entry]

**Purpose and location.** A flag in a status reply. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:296` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `offset` | `u16` | The byte offset. |
| `mask` | `u8` | The bit mask. |
| `on_when_set` | `bool` | Whether the feature is on when the bit is set. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `yaesu_legacy_in_split_refuses_a_key_on_because_vfo_b_is_unreadable (../radioctl-protocols/tests/tx_frequency.rs)`.

### `LegacyReadEntry` [@ANCHOR: radioctl:radioctl_core_rigfile_legacy_read_entry]

**Purpose and location.** One status read: the request, the reply length and where the fields are. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:304` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `request` | `[u8; 5]` | The five request bytes. |
| `length` | `u16` | The reply length in bytes. |
| `frequency` | `Option<LegacyFreqEntry>` | Where the frequency is. |
| `mode` | `Option<LegacyModeEntry>` | Where the mode is. |
| `ptt` | `Option<LegacyFlagEntry>` | Where the transmit flag is. |
| `split` | `Option<LegacyFlagEntry>` | Where the split flag is. |
| `vfo_mode` | `Option<LegacyFlagEntry>` | Where the VFO-mode flag is. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `yaesu_legacy_in_split_refuses_a_key_on_because_vfo_b_is_unreadable (../radioctl-protocols/tests/tx_frequency.rs)`.

### `KenwoodHhSection` [@ANCHOR: radioctl:radioctl_core_rigfile_kenwood_hh_section]

**Purpose and location.** The `[kenwood_hh]` table. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:316` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `dialect` | `String` | The handheld or mobile dialect name. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_kenwood_dialect_is_named_known_and_needs_its_fact (src/rigfile.rs)`.

### `FlexSection` [@ANCHOR: radioctl:radioctl_core_rigfile_flex_section]

**Purpose and location.** The `[flex]` table. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:322` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `model` | `String` | The model name. |
| `scus` | `u8` | The number of SCUs (receivers). |
| `slice` | `u8` | The slice used. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `only_a_flex_description_builds_an_engine (../radioctl-protocols/src/flex.rs)`.

### `ReceiverSection` [@ANCHOR: radioctl:radioctl_core_rigfile_receiver_section]

**Purpose and location.** The `[receiver]` table: a receiver family. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:331` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `protocol` | `String` | The receiver protocol name. |
| `dialect` | `String` | The dialect name. |
| `address` | `Option<u8>` | The bus address, if the protocol uses one. |
| `baud` | `u32` | The baud rate. |
| `data_bits` | `u8` | Data bits. |
| `parity` | `String` | Parity name. |
| `stop_bits` | `u8` | Stop bits. |
| `flow` | `String` | Flow control name. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `build_receiver` (src/rigfile.rs:867).

**See also:** test `receiver_protocol_names_round_trip (src/description.rs)`.

### `HpsdrSection` [@ANCHOR: radioctl:radioctl_core_rigfile_hpsdr_section]

**Purpose and location.** The `[hpsdr]` table. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:345` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `model` | `String` | The model name. |
| `protocol` | `u8` | The OpenHPSDR protocol number (1 or 2). |
| `anan10` | `bool` | Whether the radio is an ANAN-10 (which has its own behaviour). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `protocol_1_virtual_anan_tunes_reports_power_and_keeps_the_gate_closed (../radioctl-protocols/tests/hpsdr_actor.rs)`.

### `VendorSection` [@ANCHOR: radioctl:radioctl_core_rigfile_vendor_section]

**Purpose and location.** The `[vendor]` table: a serial vendor protocol. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:353` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `protocol` | `String` | The vendor protocol name. |
| `dialect` | `String` | The dialect name. |
| `data_bits` | `u8` | Data bits. |
| `parity` | `String` | Parity name. |
| `stop_bits` | `u8` | Stop bits. |
| `flow` | `String` | Flow control name. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `vendor_protocol_names_round_trip (src/description.rs)`.

### `UsbSection` [@ANCHOR: radioctl:radioctl_core_rigfile_usb_section]

**Purpose and location.** The `[usb]` table: a USB device. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:384` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `protocol` | `String` | The USB protocol name. |
| `vendor_id` | `u16` | The USB vendor id. |
| `product_id` | `u16` | The USB product id. |
| `manufacturer` | `String` | The manufacturer string the device reports. |
| `product` | `String` | The product string. |
| `multiplier` | `u8` | A frequency multiplier the device needs (for example for an up-converter). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `build_usb` (src/rigfile.rs:850).

**See also:** test `usb_protocol_names_round_trip (src/description.rs)`.

### `ScopeSection` [@ANCHOR: radioctl:radioctl_core_rigfile_scope_section]

**Purpose and location.** The `[scope]` table: the spectrum scope. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:395` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `receivers` | `u8` | Number of receivers with a scope. |
| `line_length` | `u16` | Points in one scope line. |
| `samples_per_frame` | `u16` | Samples per frame. |
| `amplitude_max` | `u16` | The largest amplitude value. |
| `spans_hz` | `Vec<u64>` | The spans the scope offers, in hertz. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_scope_block_is_icom_only_sane_and_needs_its_facts (src/rigfile.rs)`.

### `LevelsSection` [@ANCHOR: radioctl:radioctl_core_rigfile_levels_section]

**Purpose and location.** The `[levels]` table. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:405` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `attenuator_db` | `Vec<u8>` | The attenuator steps in dB. |
| `attenuator_any_up_to_db` | `u8` | An attenuator that accepts any value up to this many dB. |
| `preamp_steps` | `u8` | Number of preamp steps. |
| `antenna_ports` | `u8` | Number of antenna ports. |
| `absent` | `Vec<String>` | Levels the rig does not have. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `build_levels` (src/rigfile.rs:1003).

**See also:** test `a_levels_block_is_sane_and_needs_its_facts (src/rigfile.rs)`.

### `PowerMeterSection` [@ANCHOR: radioctl:radioctl_core_rigfile_power_meter_section]

**Purpose and location.** The `[power_meter]` table. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:418` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `raw_to_milliwatts` | `Vec<(u16, u32)>` | Calibration points as (raw, milliwatts), ascending. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_power_meter_block_needs_a_calibration_and_its_fact (src/rigfile.rs)`.

### `TimingSection` [@ANCHOR: radioctl:radioctl_core_rigfile_timing_section]

**Purpose and location.** The `[timing]` table. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:424` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `timeout_ms` | `u64` | Reply timeout in milliseconds. |
| `retries` | `u8` | How many times a read is retried. |
| `inter_command_ms` | `u64` | Pause between commands in milliseconds; values that would delay or defeat an unkey are rejected. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `check_timing` (src/rigfile.rs:909).

**See also:** test `timing_that_would_delay_or_defeat_an_unkey_is_rejected (src/rigfile.rs)`.

### `DefaultsSection` [@ANCHOR: radioctl:radioctl_core_rigfile_defaults_section]

**Purpose and location.** The `[defaults]` table: where a virtual rig starts. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:432` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `frequency_hz` | `u64` | The default frequency. |
| `mode` | `String` | The default mode name. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_complete_description_loads_with_its_facts (src/rigfile.rs)`.

### `CodeValue` [@ANCHOR: radioctl:radioctl_core_rigfile_code_value]

**Purpose and location.** A mode code as written in the file: a byte or a character string. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:439` as a private `enum`. It derives Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Byte` | `Byte(u8)` | A numeric code. |
| `Char` | `Char(String)` | A text code. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_complete_description_loads_with_its_facts (src/rigfile.rs)`.

### `ModeEntry` [@ANCHOR: radioctl:radioctl_core_rigfile_mode_entry]

**Purpose and location.** One `[[modes]]` entry. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:446` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `mode` | `String` | The mode name. |
| `code` | `CodeValue` | The protocol's code for it. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_complete_description_loads_with_its_facts (src/rigfile.rs)`.

### `VerifiedEntry` [@ANCHOR: radioctl:radioctl_core_rigfile_verified_entry]

**Purpose and location.** The `verified` inline table of a fact. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:453` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `state` | `String` | `unverified`, `manual-checked`, `transcript-checked` or `hardware-verified`. |
| `reference` | `Option<String>` | The manual or transcript reference (needed for the checked states). |
| `radio` | `Option<String>` | The radio tested (needed for hardware verification). |
| `firmware` | `Option<String>` | The firmware tested. |
| `date` | `Option<String>` | The date of the test. |
| `tested` | `Option<String>` | What exactly was tested. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `build_verified` (src/rigfile.rs:1249).

**See also:** test `a_complete_description_loads_with_its_facts (src/rigfile.rs)`.

### `FactEntry` [@ANCHOR: radioctl:radioctl_core_rigfile_fact_entry]

**Purpose and location.** One `[[facts]]` entry: a key and where its data came from. Defined in `daemons/radioctl/crates/radioctl-core/src/rigfile.rs:464` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `key` | `String` | The exact key (`civ.address`) or a prefix wildcard (`capability.*`). |
| `provenance` | `Provenance` | Where the datum came from. |
| `verified` | `VerifiedEntry` | What has been checked. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_complete_description_loads_with_its_facts (src/rigfile.rs)`.

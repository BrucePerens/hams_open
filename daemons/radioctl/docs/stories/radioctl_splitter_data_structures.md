# Data structures

### `ClientId` [@ANCHOR: radioctl:radioctl_splitter_arbiter_client_id]

**Purpose and location.** A client identifier assigned by the splitter (a plain number). Defined in `daemons/radioctl/crates/radioctl-splitter/src/arbiter.rs:5` as a pub `type`. It derives no traits.

**Definition.** `pub type ClientId = u64;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `try_key` (src/arbiter.rs:18), `release` (src/arbiter.rs:31), `owner` (src/arbiter.rs:48), `ptt_owner` (src/session.rs:220), `cleanup` (src/session.rs:341), `execute` (src/session.rs:486), `send_cw` (src/session.rs:520), `key` (src/session.rs:536).

**See also:** test `the_first_client_to_key_owns_the_transmitter (src/arbiter.rs)`.

### `PttOwner` [@ANCHOR: radioctl:radioctl_splitter_arbiter_ptt_owner]

**Purpose and location.** Who holds the transmitter: at most one client at a time. A client that asks to key while another owns it is refused, and the owner is cleared when it unkeys or disconnects. Defined in `daemons/radioctl/crates/radioctl-splitter/src/arbiter.rs:9` as a pub `struct`. It derives Debug, Default, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `owner` | `Option<ClientId>` | The owning client, `None` when nobody holds the transmitter. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_first_client_to_key_owns_the_transmitter (src/arbiter.rs)`.

### `CivPersonality` [@ANCHOR: radioctl:radioctl_splitter_civ_personality_civ_personality]

**Purpose and location.** The IC-7300 emulated by a CI-V endpoint: it parses CI-V frames from a client and answers as the rig would. Defined in `daemons/radioctl/crates/radioctl-splitter/src/civ_personality.rs:31` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `address` | `u8` | The CI-V address to answer at. |
| `echo` | `bool` | Whether each frame is echoed back to the client that sent it (the CI-V bus echo). |
| `modes` | `Vec<(Mode, u8)>` | The rig's mode codes as (mode, CI-V code) pairs, from the rig description. |
| `parser` | `FrameParser` | The frame parser holding bytes received so far. |

**Invariants, ownership and concurrency.** One per client, owned by that client's task; not shared. It never talks to the rig itself: it turns frames into `Request`s and renders `Outcome`s.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/civ_personality.rs:42), `ask` (src/civ_personality.rs:218).

**See also:** test `requests_become_rig_actions_and_the_rest_is_refused_or_ignored (src/civ_personality.rs)`.

### `Request` [@ANCHOR: radioctl:radioctl_splitter_personality_request]

**Purpose and location.** What a client asks of the rig, in protocol-independent form. Defined in `daemons/radioctl/crates/radioctl-splitter/src/personality.rs:16` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `ReadFrequency` | `ReadFrequency(Vfo)` | Read a VFO's frequency. |
| `SetFrequency` | `SetFrequency(Vfo, Hz)` | Set a VFO's frequency. |
| `ReadMode` | `ReadMode` | Read the mode. |
| `SetMode` | `SetMode(Mode)` | Set the mode. |
| `ReadPtt` | `ReadPtt` | Read the transmit state. |
| `SetPtt` | `SetPtt(bool)` | Key (`true`) or unkey (`false`); the splitter checks the owner and the transmit gate before the rig is asked. |
| `ReadSignal` | `ReadSignal` | Read the S-meter. |
| `ReadPower` | `ReadPower` | Read the power meter. |
| `ReadStatus` | `ReadStatus` | Read the composite status (frequency, mode, transmit, RIT, XIT and its offset, split). |
| `SendCw` | `SendCw(String)` | Send Morse text. |
| `StopCw` | `StopCw` | Stop Morse in progress (always allowed, with no owner check: it is a stop path). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `reply` (src/civ_personality.rs:155), `context_of` (src/civ_personality.rs:224), `reply` (src/personality.rs:108), `execute` (src/session.rs:486), `rig` (src/ts2000_personality.rs:61), `reply` (src/ts2000_personality.rs:158), `rig` (src/ts2000_personality.rs:219).

**See also:** test `requests_become_rig_actions_and_the_rest_is_refused_or_ignored (src/civ_personality.rs)`.

### `Status` [@ANCHOR: radioctl:radioctl_splitter_personality_status]

**Purpose and location.** The rig state a composite status answer (the Kenwood `IF`) is built from. Defined in `daemons/radioctl/crates/radioctl-splitter/src/personality.rs:43` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `rx_vfo` | `Vfo` | The VFO the rig receives on (VFO A when the rig does not say). |
| `frequency` | `Hz` | Frequency of the receive VFO. |
| `mode` | `Mode` | Operating mode. |
| `ptt` | `bool` | Transmitting. |
| `rit` | `bool` | RIT (receiver incremental tuning) on (off when the rig cannot say). |
| `xit` | `bool` | XIT (transmitter incremental tuning) on (off when the rig cannot say). |
| `rit_offset` | `i16` | RIT/XIT offset in hertz (0 when the rig cannot say). |
| `split` | `bool` | Split operation on (off when the rig cannot say). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `status_answer` (src/ts2000_personality.rs:78).

**See also:** test `the_composite_status_has_the_ts2000_layout (src/ts2000_personality.rs)`.

### `Outcome` [@ANCHOR: radioctl:radioctl_splitter_personality_outcome]

**Purpose and location.** What the splitter did with a `Request`. Defined in `daemons/radioctl/crates/radioctl-splitter/src/personality.rs:64` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Value` | `Value(Observation)` | A value (the answer to a read). |
| `Status` | `Status(Status)` | The composite status (the answer to `Request::ReadStatus`). |
| `Done` | `Done` | A set or key was done. |
| `Refused` | `Refused` | The request was refused (unsupported, gate closed, PTT owned by another client, or failed). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `reply` (src/civ_personality.rs:155), `reply` (src/personality.rs:108), `read` (src/session.rs:385), `read_frequency` (src/session.rs:406), `read_status` (src/session.rs:422), `set` (src/session.rs:472), `execute` (src/session.rs:486), `send_cw` (src/session.rs:520).

**See also:** test `replies_carry_the_addresses_prefix_and_value (src/civ_personality.rs)`.

### `Action` [@ANCHOR: radioctl:radioctl_splitter_personality_action]

**Purpose and location.** What the splitter must do for one parsed item. Defined in `daemons/radioctl/crates/radioctl-splitter/src/personality.rs:77` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Rig` | `Rig { request: Request, context: Vec<u8>, }` | Ask the rig with `request`; the personality's `reply` renders the outcome, and `context` (opaque to the splitter) is handed back to it. |
| `Local` | `Local(Vec<u8>)` | Answer locally with these bytes (identity, per-client flags, refusals of what the personality does not speak). |
| `Ignore` | `Ignore` | Nothing to do. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `classify` (src/civ_personality.rs:84), `rig` (src/ts2000_personality.rs:61), `classify` (src/ts2000_personality.rs:99), `actions` (src/ts2000_personality.rs:212), `rig` (src/ts2000_personality.rs:219).

**See also:** test `requests_become_rig_actions_and_the_rest_is_refused_or_ignored (src/civ_personality.rs)`.

### `Pending` [@ANCHOR: radioctl:radioctl_splitter_personality_pending]

**Purpose and location.** One parsed item from a client: an echo to send at once and the action. Defined in `daemons/radioctl/crates/radioctl-splitter/src/personality.rs:95` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `echo` | `Vec<u8>` | Bytes to send to the client at once (the CI-V bus echo of its own frame). |
| `action` | `Action` | What to do. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse` (src/civ_personality.rs:132), `ask` (src/civ_personality.rs:218), `context_of` (src/civ_personality.rs:224), `parse` (src/personality.rs:105), `parse` (src/ts2000_personality.rs:143).

**See also:** test `requests_become_rig_actions_and_the_rest_is_refused_or_ignored (src/civ_personality.rs)`.

### `PtyEndpoint` [@ANCHOR: radioctl:radioctl_splitter_pty_pty_endpoint]

**Purpose and location.** A pseudo-terminal endpoint of the splitter. Hand `PtyEndpoint::path` to the outside program as its serial port. Defined in `daemons/radioctl/crates/radioctl-splitter/src/pty.rs:29` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `slave` | `PathBuf` | The path of the pty's slave side. |
| `link` | `Option<PathBuf>` | A symbolic link made at a path the caller chose, if any. |
| `stop` | `Arc<AtomicBool>` | Flag that ends the endpoint's reader and writer tasks. |

**Invariants, ownership and concurrency.** Owns a pty and its tasks; closing it removes the link. Reads from the pty are polled in `SLICE` steps, and input goes to the client task through a bounded channel.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `two_programs_on_two_virtual_serial_ports_share_the_radio_and_the_transmitter (tests/splitter.rs)`.

### `Protocol` [@ANCHOR: radioctl:radioctl_splitter_session_protocol]

**Purpose and location.** The protocol a splitter endpoint speaks. Defined in `daemons/radioctl/crates/radioctl-splitter/src/session.rs:40` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Civ` | `Civ { address: u8, echo: bool, }` | Icom CI-V as an IC-7300 at `address` (0x94), with the bus echo on or off. |
| `Ts2000` | `Ts2000` | Kenwood TS-2000 CAT. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/session.rs:84), `splitter` (tests/splitter.rs:45), `civ` (tests/splitter.rs:49).

**See also:** test `front_panel_changes_reach_every_client_as_that_clients_unsolicited_frames (tests/splitter.rs)`.

### `SplitterConfig` [@ANCHOR: radioctl:radioctl_splitter_session_splitter_config]

**Purpose and location.** Splitter settings, a plain copyable value. Defined in `daemons/radioctl/crates/radioctl-splitter/src/session.rs:54` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `protocol` | `Protocol` | The protocol clients speak. |
| `max_age` | `Duration` | A read is answered from the rig's cache if the cached value is no older than this. |
| `request_timeout` | `Duration` | Deadline for a request that has to go to the rig. |
| `report_tx_frequency_while_keyed` | `bool` | While the rig is keyed in split operation, a plain read of the current frequency (CI-V `03`, TS-2000 `FA;`) answers with the transmit frequency, as OmniRig-style clients assume (Hamlib issue 138). Reads that name the other VFO (`25 01`, `FB;`) are unchanged. Turn it off for a client that wants the receive frequency while keyed. |
| `max_clients` | `usize` | Most simultaneous TCP clients (`serve_tcp`); a connection past the limit is closed before a byte is read. Clients attached directly (`attach_stream`, `attach_channels`, a pty) are not counted: the caller made each of those on purpose. |
| `idle_timeout` | `Duration` | A stream client silent for this long is disconnected (its PTT given up, its tasks ended), so an abandoned connection cannot hold a slot, or the transmitter, for ever. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/session.rs:84), `new` (src/session.rs:190).

**See also:** test `a_ts2000_client_reads_the_composite_status_from_the_rigs_state (tests/splitter.rs)`.

### `ClientInput` [@ANCHOR: radioctl:radioctl_splitter_session_client_input]

**Purpose and location.** What a client delivers to its task. Defined in `daemons/radioctl/crates/radioctl-splitter/src/session.rs:98` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Bytes` | `Bytes(Vec<u8>)` | Bytes the client sent. |
| `Disconnected` | `Disconnected` | The client went away (a stream ended, or a pty was closed by the program using it); its PTT (push to talk) is given up. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `read_master` (src/pty.rs:115).

**See also:** test `a_client_ends_when_its_stream_ends_or_when_aborted (tests/splitter.rs)`.

### `SplitterStats` [@ANCHOR: radioctl:radioctl_splitter_session_splitter_stats]

**Purpose and location.** Counters of a splitter, a snapshot copied out of `Counters`. Defined in `daemons/radioctl/crates/radioctl-splitter/src/session.rs:107` as a pub `struct`. It derives Debug, Clone, Copy, Default, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `requests` | `u64` | Requests that reached the rig side (served from cache or sent to the rig). |
| `cache_hits` | `u64` | Reads answered from the cache. |
| `refused` | `u64` | Requests refused. |
| `ptt_conflicts` | `u64` | Keying attempts refused because another client owns the transmitter. |
| `clients` | `u64` | Clients attached so far. |
| `connections_refused` | `u64` | TCP connections closed at once because `max_clients` were already connected. |
| `idle_disconnects` | `u64` | Stream clients disconnected for being silent longer than `idle_timeout`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `stats` (src/session.rs:205).

**See also:** test `tcp_connections_past_the_client_limit_are_closed_and_a_freed_slot_is_reused (tests/splitter.rs)`.

### `Counters` [@ANCHOR: radioctl:radioctl_splitter_session_counters]

**Purpose and location.** The atomic counters behind `SplitterStats`. Defined in `daemons/radioctl/crates/radioctl-splitter/src/session.rs:125` as a private `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `requests` | `AtomicU64` | Requests that reached the rig side. |
| `cache_hits` | `AtomicU64` | Reads answered from the cache. |
| `refused` | `AtomicU64` | Requests refused. |
| `ptt_conflicts` | `AtomicU64` | Keying attempts refused because another client owns the transmitter. |
| `clients` | `AtomicU64` | Clients attached so far. |
| `connections_refused` | `AtomicU64` | TCP connections closed at once because `max_clients` were connected. |
| `idle_disconnects` | `AtomicU64` | Stream clients disconnected for being silent past `idle_timeout`. |

**Invariants, ownership and concurrency.** Relaxed atomics updated from many tasks without a lock.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `tcp_connections_past_the_client_limit_are_closed_and_a_freed_slot_is_reused (tests/splitter.rs)`.

### `Inner` [@ANCHOR: radioctl:radioctl_splitter_session_inner]

**Purpose and location.** The state shared by every clone of a `Splitter`. Defined in `daemons/radioctl/crates/radioctl-splitter/src/session.rs:135` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `rig` | `RigHandle` | The handle of the rig behind the splitter. |
| `config` | `SplitterConfig` | The settings. |
| `owner` | `Mutex<PttOwner>` | The transmitter owner, behind a mutex that is held only to check or change the owner. |
| `next_id` | `AtomicU64` | The next client id (atomic). |
| `counters` | `Counters` | The counters. |
| `epoch` | `Instant` | The start instant, the reference for cache ages. |

**Invariants, ownership and concurrency.** Plain data: its fields hold AtomicU64, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `one_client_owns_the_transmitter_and_its_departure_unkeys_the_rig (tests/splitter.rs)`.

### `Splitter` [@ANCHOR: radioctl:radioctl_splitter_session_splitter]

**Purpose and location.** The splitter in front of one rig: it multiplexes several clients (CI-V or TS-2000 flavoured) onto one rig, arbitrates who may key, and caches reads. Clone it freely; clones share one state. Defined in `daemons/radioctl/crates/radioctl-splitter/src/session.rs:152` as a pub `struct`. It derives Clone.

| Field | Type | Meaning |
|---|---|---|
| `inner` | `Arc<Inner>` | The shared state in an `Arc`. |

**Invariants, ownership and concurrency.** Safe to use from many tasks. Keying is gated twice: the owner arbiter (one client at a time) and the rig handle's own transmit checks (the relay lock, band check and transmit gate of the rig layer still apply behind it).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/session.rs:190), `splitter` (tests/splitter.rs:45), `client` (tests/splitter.rs:57).

**See also:** test `a_ts2000_client_reads_the_composite_status_from_the_rigs_state (tests/splitter.rs)`.

### `ClientHandle` [@ANCHOR: radioctl:radioctl_splitter_session_client_handle]

**Purpose and location.** A running client of the splitter. Defined in `daemons/radioctl/crates/radioctl-splitter/src/session.rs:157` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `id` | `ClientId` | The client's identifier (the PTT owner id). |
| `task` | `JoinHandle<()>` | The client's task; ended by `Disconnected`, by an idle timeout or when the handle is closed. |
| `io` | `Vec<JoinHandle<()>>` | The stream's reader and writer tasks (`attach_stream`); none for channel clients. |

**Invariants, ownership and concurrency.** Plain data: its fields hold JoinHandle (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `attach_stream` (src/session.rs:265).

**See also:** test `a_client_ends_when_its_stream_ends_or_when_aborted (tests/splitter.rs)`.

### `Ts2000Personality` [@ANCHOR: radioctl:radioctl_splitter_ts2000_personality_ts2000_personality]

**Purpose and location.** The TS-2000 emulated by a CAT (computer-aided transceiver control) endpoint: it tokenizes `;`-terminated commands and answers as the rig would. Defined in `daemons/radioctl/crates/radioctl-splitter/src/ts2000_personality.rs:29` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `modes` | `Vec<(Mode, u8)>` | The rig's mode codes as (mode, code) pairs from the rig description. |
| `tokenizer` | `Tokenizer` | Splits the received bytes into commands. |
| `auto_info` | `bool` | The client's auto-information setting (`AI`), off at start; it is answered locally and nothing is pushed unasked. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/ts2000_personality.rs:38), `default` (src/ts2000_personality.rs:136), `actions` (src/ts2000_personality.rs:212).

**See also:** test `commands_become_rig_actions_local_answers_or_refusals (src/ts2000_personality.rs)`.

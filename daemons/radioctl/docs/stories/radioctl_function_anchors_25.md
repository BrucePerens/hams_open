# radioctl function anchors, batch 25

Function anchors for the `radioctl-core` crate (the rig handle's read and submit helpers, the actor's reply matching, session open and restore, fail-closed unkey, time helpers, the boxed engine and capability names) and for the `radioctl-protocols` crate (the link-restored hooks of the Kenwood, Elecraft, TS-890S and Flex engines, and the virtual rigs' session and byte counters). Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
For anything that can key a transmitter, the entry says how the gate applies: `submit` refuses a keying op with the gate closed, the actor refuses it again, and `unkey_fail_closed` and `unkey_on_stop` unkey without needing the gate. The session hooks never key; the virtual rigs transmit nothing.

## `crates/radioctl-protocols/src/io.rs`

### `pub fn bytes_received(&self) -> usize` [@ANCHOR: radioctl:radioctl_virtual_io_bytes_received]

Returns the total number of bytes the controller (the actor under test) has written to the virtual rig since it was spawned. The counter is incremented by the reader task for every chunk read from the stream, before echo, fault injection or the rig sees them, so it counts exactly what was sent on the wire. Used by tests to prove that a command was or was not written, for example that a refused keying command put nothing on the wire. Relaxed atomic read; a snapshot. Cannot fail.

## `crates/radioctl-core/src/rig.rs`

### `fn absorb( &mut self, data: &[u8], expect: ReplyKind, seq: u64, source: Source, ) -> Option<Reply>` [@ANCHOR: radioctl:radioctl_core_actor_absorb]

Feeds received bytes to the protocol engine and decides what they mean for the command with identity `seq`, which expects `expect`. For each decoded item it first claims the outstanding request the item answers. An item owed to an earlier, abandoned command is stale: counted in `late_replies`, noted in the transcript, counted in `stale_replies` too if it would otherwise have passed for this command's answer, and never taken as this command's result; an observation among them still reaches the cache as a real older reading. A fitting observation becomes the command's result `Ok(Some(value))`, an Ack becomes `Ok(None)`, and a Nak becomes a `Rejected` error. Observations are recorded with `observe` (source of the command if wanted, else `Transceive`), unsolicited ones go through `note_unsolicited`, scope frames get a sequence number and are published on their streams, unrecognised bytes are counted and logged. Returns `None` while the command is still undecided.

### `fn next_seq(&mut self) -> u64` [@ANCHOR: radioctl:radioctl_core_actor_next_seq]

Returns a fresh identity for one command: the actor's counter is incremented with wrapping and kept at least 1, so the value 0 is never issued (0 is used for traffic that belongs to no command). The retries of one command and the retries of one fail-closed unkey share the identity, which lets `claim_reply` tell which command a reply belongs to. Pure state change on the actor, cannot fail.

### `fn note_written(&mut self, kind: ReplyKind, seq: u64, timeout: Duration)` [@ANCHOR: radioctl:radioctl_core_actor_note_written]

Records that a request which expects a reply has been written: the kind of reply, the command identity `seq` and an expiry of two reply timeouts from now (the one the command waits and one more for a late answer). It lets the answer be matched to its request even if it arrives after the command was abandoned, so a late reply cannot be taken for a later command's answer. A request that expects no reply (`ReplyKind::None`) is not recorded. Expired entries are pruned first, so an answer the rig never gives cannot eat a later reply for ever.

### `fn claim_reply(&mut self, inbound: &Inbound) -> Option<u64>` [@ANCHOR: radioctl:radioctl_core_actor_claim_reply]

Finds which written request a decoded item answers and removes it from the outstanding list, returning that command's identity. The rig answers in the order it was asked, so it takes the oldest outstanding entry the item fits: an Ack fits an Ack request, an observation fits a request for that field, and a Nak fits any request. Scope frames, filter-scope frames and unrecognised bytes never fit. Returns `None` when nothing waited for it (transceive traffic, or a reply to a request already forgotten as expired). Expired entries are pruned first.

### `async fn drain_stale(&mut self, deadline: Instant, may_preempt: bool) -> Option<Wait>` [@ANCHOR: radioctl:radioctl_core_actor_drain_stale]

Called before a normal command is written. While an abandoned command's reply may still arrive (an outstanding entry has not expired), it reads and absorbs input (`ingest`) so that reply is matched to its own request and not taken for the new command's answer. It waits at most until the last such expectation expires or `deadline`, counting each wait in `stale_drains`. It returns `None` when it is done, `Some(Wait::Preempted)` if an emergency command arrived (only when `may_preempt` is true), and `Some(Wait::LinkDown(reason))` for a read error or end of stream. Emergency commands never call it; they go out at once.

### `fn apply_set(&mut self, op: &Op, source: Source)` [@ANCHOR: radioctl:radioctl_core_actor_apply_set]

Records a successful set command in the cache, as an observation with `source`, so a reader sees the new value at once and polling reconciles it later: frequency, mode, PTT (push to talk), split, RIT (receiver incremental tuning) and XIT (transmitter incremental tuning), offset, filter width, levels, scope settings and the other set ops each become the matching observation. Setting split also moves the transmit VFO in the same step (entering split first sets the transmit VFO to the other one; leaving first clears the split flag) so the derived transmit frequency changes once and never names the wrong VFO. A keying result arms the watchdog through `observe`, `SendCw` arms it directly, and `StopCw` changes nothing because the rig may still be keyed. Called only after the rig accepted the command or none was expected.

### `async fn await_reply( &mut self, expect: ReplyKind, seq: u64, until: Instant, source: Source, may_preempt: bool, ) -> Wait` [@ANCHOR: radioctl:radioctl_core_actor_await_reply]

Waits for the reply to the command `seq` until `until`. It reads from the transport and passes the bytes to `absorb` until the command is decided. It returns `Wait::Done(result)` for the rig's answer, `Wait::TimedOut` when `until` passes, `Wait::LinkDown(reason)` for a read error or end of stream, and `Wait::Preempted` when, with `may_preempt` true, an emergency command is pending or the transmit watchdog is due (so a slow radio cannot hold the actor past the transmit limit). Heartbeat wakeups only recompute the watchdog. A fail-closed unkey and the session requests pass false, so they cannot be preempted. Async; `expect` is the kind of reply the request named.

### `fn log(&mut self, direction: &str, bytes: &[u8])` [@ANCHOR: radioctl:radioctl_core_actor_log]

Appends one line to the actor's transcript: the direction tag (`TX`, `RX`, `??` for unrecognised bytes) followed by the bytes in lowercase hex separated by spaces. The transcript keeps only the latest 16 lines (older ones are dropped), and its tail is attached to errors (`with_transcript`) so a failure carries the recent wire traffic for diagnosis. It does no I/O, takes no lock, and cannot fail.

### `fn spacing(&self, gap: Duration, deadline: Instant) -> Option<Instant>` [@ANCHOR: radioctl:radioctl_core_actor_spacing]

Works out when the next command may be written so that the radio's parser gets its inter-command gap (`gap`, from the engine's timing). It returns `None` when nothing has been written yet or the gap has already passed, and otherwise the instant `last_write + gap`, never later than `deadline`. The caller sleeps until it. Emergency commands do not use it. Pure apart from reading the clock.

### `pub(crate) async fn read( &self, field: Field, op: Op, freshness: Freshness, ) -> Result<Cached<Observation>, RigError>` [@ANCHOR: radioctl:radioctl_core_handle_read]

The common read used by the typed getters (`frequency`, `mode`, `ptt`, `signal`, `level`, `meter`). If the cache holds `field` and the freshness allows it (`Cached`: any age; `MaxAge(limit)`: age at most `limit`; `Fresh`: never), it returns the cached value with its age and does not touch the rig. Otherwise it runs `op` through `execute` with the default deadline and returns the value with age zero. A reply with no value gives a `Malformed` error ("read returned no value"); other errors are those of `execute`. Crate-private. Never transmits.

### `fn submit( &self, op: Op, timeout: Duration, source: Source, ) -> Result<oneshot::Receiver<Reply>, RigError>` [@ANCHOR: radioctl:radioctl_core_handle_submit]

Queues a command for the actor without waiting; the caller then awaits the returned receiver. It first refuses an op whose required capability is not in the rig description (`Unsupported`) and a transmitting op while the gate is closed (`TransmitGateClosed`). It then builds the command with a deadline of `timeout` from now. Emergency ops (unkey, stop-Morse) go to the emergency queue (16 entries) and wake the actor; if that queue is full, an unkey rides on the pending one (its caller is answered with that result and `unkeys_coalesced` is counted), so an unkey is never refused for want of a slot, while a stop-Morse that finds it full is refused with `QueueFull`. Ordinary ops use `try_send` on the normal queue (capacity from the options, default 64) and fail with `QueueFull` when full. A stopped actor gives `LinkDown`. Never blocks.

### `async fn prime_tx_state(&self, op: &Op)` [@ANCHOR: radioctl:radioctl_core_handle_prime_tx_state]

Run before submitting a transmit-capable op with the gate open: it brings the transmit-frequency derivation up to date by reading what the cache lacks (split state, VFO B if the rig has one, VFO A), never re-reading a value already cached (`Freshness::Cached`). Errors are ignored here, because the actor's own fail-closed check is the one that refuses (`TransmitFrequencyUnknown`), so the command is refused only when the rig could not be read. For any non-transmitting op, or with the gate closed, it does nothing. The recursion through `execute` is broken by boxing the future. Does not key.

### `async fn unkey_fail_closed(&mut self, why: &str, source: Source)` [@ANCHOR: radioctl:radioctl_core_actor_unkey_fail_closed]

Unkeys the rig when the actor cannot trust its state: when the outcome of a transmit command is unknown, when the watchdog fires, when the link is restored and when the actor stops. `why` (borrowed text) goes to the transcript. It encodes `Op::SetPtt(false)` (if the rig has the capability), writes it, and waits for the reply within the engine's timeout; it is not preemptible and makes up to three attempts with a 100 ms times attempt-number backoff, stopping if the link is down. A write failure takes the link down. Success updates the cache (`apply_set`). When it cannot be done (no PTT command, every attempt unanswered or refused, link down) it counts an `unkey_failures` and emits `Alarm::UnkeyFailed` with `source`, never silently. No gate is needed.

### `async fn unkey_on_stop(&mut self)` [@ANCHOR: radioctl:radioctl_core_actor_unkey_on_stop]

Runs when the last handle is gone and the actor is about to end. If the cached PTT is on, or the transmit gate is open (so the rig could have been keyed without the cache knowing), it unkeys through `unkey_fail_closed` before the actor ends, so a dropped handle never leaves RF on the air. Best effort: a failure is counted and alarmed there but nobody is left to hear it. When the rig is receiving and the gate is closed it does nothing.

### `async fn restore_session(&mut self)` [@ANCHOR: radioctl:radioctl_core_actor_restore_session]

After a reconnect, writes the requests the engine returns from `link_restored` (session set-up the radio forgot with the old link, such as Kenwood or Elecraft auto-information or Flex subscriptions), once each and in order, through `write_session_requests`. They are not retried and never transmit; a request that fails is noted in the transcript and the rest still go. It runs inside the link-restored step after the fail-closed unkey and before the `LinkRestored` alarm, so no queued command is served in front of it.

### `async fn open_session(&mut self)` [@ANCHOR: radioctl:radioctl_core_actor_open_session]

Once, when the actor starts: writes the requests the engine returns from `open_requests` (an Icom PCR is powered on and told to stop its automatic updates), before any queued command is served. It does nothing when a session was already opened or restored (`session_opened`) or when the link was down at the start. It marks the session opened first, then writes the requests with `write_session_requests`; a write that fails takes the link down, so every command then answers `LinkDown` and a failed open does not hang. These requests never key the transmitter.

### `async fn write_session_requests(&mut self, requests: Vec<Request>)` [@ANCHOR: radioctl:radioctl_core_actor_write_session_requests]

Writes session requests once each, in order, waiting for the reply each one names (with one reply timeout for the write and the wait). `requests` is owned. It stops if the link is down; a write error takes the link down and ends the loop; a write timeout is noted in the transcript as `SESSION RESTORE write timed out` and the loop continues with the next request. The reply wait cannot be preempted and its result is ignored. Each request records its expected reply (`note_written`) so a late answer is matched correctly. Never retries.

## `crates/radioctl-core/src/engine.rs`

### `fn link_restored(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_core_engine_box_link_restored]

The `ProtocolEngine` implementation for `Box<dyn ProtocolEngine>`: forwards the link-restored hook to the boxed engine, so one `spawn` serves every protocol family. It returns whatever the inner engine returns, which is the list of requests that set the session up again after a link loss, or an empty list for engines that set nothing up. No logic of its own and no error.

### `fn encode_lines(&mut self, op: &Op) -> Result<Option<Vec<LineStep>>, RigError>` [@ANCHOR: radioctl:radioctl_core_engine_box_encode_lines]

Forwards the control-line hook to the boxed engine. `op` is borrowed. It returns the inner engine's answer: `Ok(Some(steps))` when the command is a sequence of control-line changes (the actor then drives the steps and does not call `encode`), `Ok(None)` for an ordinary byte request, or an error. The inner engine never returns steps for a transmitting op; the actor refuses that. No logic of its own.

### `fn open_requests(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_core_engine_box_open_requests]

Forwards the open hook to the boxed engine: the requests that make a freshly opened rig ready to talk (for an Icom PCR the power-on and update-stop commands), or an empty list when the rig needs nothing. The actor writes them once when it starts with the link up. They never key the transmitter. No logic of its own.

## `crates/radioctl-protocols/src/elecraft.rs`

### `fn link_restored(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_elecraft_engine_link_restored]

Called by the actor after the link to an Elecraft rig came back. It replaces the tokenizer with a fresh one, so a half-received frame from the old link cannot be joined to the first bytes of the new one. If the engine had transceive (auto-information) switched on, it returns one request that sets it on again, because the radio forgot it with the link; otherwise (or if encoding that fails) it returns an empty list. The request is only the auto-information setting; it never keys the transmitter.

## `crates/radioctl-protocols/src/flex.rs`

### `fn link_restored(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_flex_engine_link_restored]

Called after the link to a FlexRadio came back. A new connection means the half line and the replies still owed belong to the old one and the radio has forgotten this client's subscriptions, so it clears the partial line, the discard flag and the pending and awaiting reply lists. If the engine had subscribed, it builds one request containing the subscription lines again (`sub slice all`, `sub tx all`, `sub pan all`, with `info` inserted after the first two), each with a fresh sequence number, the last one carrying the awaited reply, and returns it with the Ack reply kind. If it had not subscribed it returns an empty list. The `subscribed` flag stays set so the next command does not repeat the lines. Never keys.

## `crates/radioctl-protocols/src/kenwood.rs`

### `fn link_restored(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_kenwood_engine_link_restored]

Called after the link to a Kenwood rig came back. It replaces the tokenizer with a fresh one so a half frame from the old link is not joined to the new link's first bytes. If the engine had transceive (auto-information, `AI2;`) on, it returns the one request that turns it on again, since the radio forgot it with the link; if not, or if the request cannot be encoded, it returns an empty list. It only restores a listening setting and never keys the transmitter.

## `crates/radioctl-protocols/src/kenwood_ts890.rs`

### `fn link_restored(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_ts890_engine_link_restored]

Called after the link to a Kenwood TS-890S came back. It replaces the tokenizer with a fresh one (with the engine's maximum frame length) so a half frame from the old link is dropped. It then returns, in order, the requests that restore what the radio forgot: auto-information if transceive was on, and the scope output if it was on. A request that cannot be encoded is left out. With neither enabled the list is empty. These requests only restore listening settings; none keys the transmitter.

## `crates/radioctl-protocols/src/flex_io.rs`

### `pub fn forget_session(&self)` [@ANCHOR: radioctl:radioctl_virtual_flex_control_forget_session]

For the virtual Flex radio behind a stream: tells it to forget this client's subscriptions, as a real radio does when the client reconnects. It locks the rig briefly and calls the model's `forget_session`, which clears the slice, tx and pan subscription flags. Tests use it to simulate a link loss and then check that the engine subscribes again after `link_restored`. Cannot fail.

### `pub fn subscribed(&self) -> (bool, bool, bool)` [@ANCHOR: radioctl:radioctl_virtual_flex_control_subscribed]

Reports whether the client of the virtual Flex radio is currently subscribed to slice, tx and pan status, as a tuple of three booleans in that order. It locks the rig briefly and reads the model's flags. A snapshot for test assertions, for example that a restored session subscribed to all three. Cannot fail.

## `crates/radioctl-protocols/src/virtual_flex.rs`

### `pub fn forget_session(&mut self)` [@ANCHOR: radioctl:radioctl_virtual_flex_forget_session]

The virtual Flex radio's reaction to a new client connection: it clears the slice, tx and pan subscription flags, because a real radio keeps no subscriptions of an old connection. The radio's other state (frequency, mode, key state) is left as it is. After this the model sends no status lines until the client subscribes again. Cannot fail.

### `pub fn subscribed(&self) -> (bool, bool, bool)` [@ANCHOR: radioctl:radioctl_virtual_flex_subscribed]

Returns whether the virtual Flex radio's client is subscribed to slice, tx and pan status, in that order, as the current values of the three flags (all false at the start and after `forget_session`). Read-only, borrowed, cannot fail. The model uses the slice flag to decide whether to send slice status, so the flags also control what the radio emits.

## `crates/radioctl-protocols/src/virtual_rig.rs`

### `pub fn forget_session(&mut self)` [@ANCHOR: radioctl:radioctl_virtual_forget_session]

Makes the virtual radio lose what the controller set up on the old link, as a power cycle or a dropped connection would: transceive goes off and the auto-information level goes back to 0. The dial, mode and key state stay as they were, since a real transmitter does not unkey itself because a cable was pulled. So after it the radio sends no auto-information until the controller asks again, which is what the restore tests check. Cannot fail.

## `crates/radioctl-protocols/src/io.rs`

### `pub fn forget_session(&self)` [@ANCHOR: radioctl:radioctl_virtual_io_forget_session]

Control handle for the stream-wrapped virtual rig: tells the rig to forget the session set up on the old link (see `VirtualRig::forget_session`), so auto-information is off again while frequency, mode and key state are kept. It locks the rig mutex briefly (poisoning tolerated). Used by tests that simulate a reconnect. It sends no bytes by itself. Cannot fail.

## `crates/radioctl-core/src/rig.rs`

### `fn deadline_after(from: Instant, wait: Duration) -> Instant` [@ANCHOR: radioctl:radioctl_core_deadline_after]

Adds `wait` to `from` with saturation: if the sum is not representable by the clock (`Duration::MAX`, or an absurd timing value from a rig description file) it halves `wait` repeatedly until it fits, giving the farthest representable instant instead of a panic. A panic here would kill the actor or the caller's task, and so leave a keyed rig without its watchdog. Used for every deadline in the actor. Pure, cannot fail.

### `async fn wait_until(due: Option<Instant>)` [@ANCHOR: radioctl:radioctl_core_wait_until]

Sleeps until `due`. With `Some(instant)` it completes when that time is reached (immediately if already past); with `None` it never completes, so a branch of the actor's `select` that depends on an optional deadline (the next poll, the watchdog) is simply never taken when there is no deadline. Cancel-safe. Needs the runtime's time driver.

## `crates/radioctl-core/src/capability.rs`

### `pub fn name(self) -> &'static str` [@ANCHOR: radioctl:radioctl_core_capability_name]

Returns the text name of a capability as written in rig description files, such as `read-power`, `send-cw`, `split`, `rit-xit` and `scope`. Looks it up in the table of names. A capability missing from the table returns `unknown`, which should not happen because the table lists every variant; a test in the crate checks the round trip with `from_name`. Pure.

### `pub fn from_name(name: &str) -> Option<Capability>` [@ANCHOR: radioctl:radioctl_core_capability_from_name]

Parses a capability name from a rig description file (`read-power`, `send-cw`, and so on) into the `Capability` value, the inverse of `name`. `name` is borrowed and matched exactly (case-sensitive, no trimming). Returns `None` for an unknown name (the test checks that `fly` is refused); how a caller reports it, for example the rig file loader, is `unknown` here, check the call site. Pure.

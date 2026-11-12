# radioctl function anchors, batch 24

Function anchors for the `radioctl-core` crate (scope and meter helpers, the transmit lease, the event stream, the rig actor's main loop, command execution and polling) and for the virtual rig's stream wrapper in `radioctl-protocols`. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
For anything that can key a transmitter, the entry says how the gate applies. The transmit lease is the only helper that opens the gate, and only with a caller-supplied permit; the actor's `run_command` refuses a keying op when the gate is closed or the transmit frequency is unknown, tries a keying write once, and unkeys fail-closed if the outcome is unknown. The virtual rig is a software test double and transmits nothing.

## `crates/radioctl-core/src/rig/easy.rs`

### `pub async fn meter(&self, meter: Meter, freshness: Freshness) -> Result<Cached<u16>, RigError>` [@ANCHOR: radioctl:radioctl_core_easy_meter]

Reads a raw meter (SWR, ALC, COMP, Vd or Id, chosen with the `Meter` enum) in the rig's own scale as a `u16`; there is no conversion to physical units, so the scale per rig and per meter is `unknown` here (check the rig's description or manual). `freshness` is `Cached` (any cached value), `MaxAge(duration)` or `Fresh` (always ask the radio). The result carries the value and its age. If the cached value is usable it is returned without touching the radio. Errors are those of `execute` (unsupported capability, link down, timeout, queue full), and `Malformed` if the answer is a different meter or empty. Read-only: it never transmits and needs no gate.

### `pub async fn start_scope(&self) -> Result<ScopeStream, RigError>` [@ANCHOR: radioctl:radioctl_core_easy_start_scope]

Starts the rig's scope (spectrum) output and returns the stream of sweeps. It subscribes to the scope frames first and only then sends `Op::SetScopeOutput(true)`, in that order, so the first sweep cannot be missed. The call returns the error of the command if the rig refuses it (for example `Unsupported` when the model has no scope, `LinkDown`, `Timeout`), in which case no stream is returned. It is receive-only and never keys. The sweeps come from the actor into a separate broadcast channel of `scope_capacity` frames (default 16); a subscriber that falls behind skips frames and the miss is counted in `scope_lagged`. `stop_scope` ends the output.

### `pub async fn stop_scope(&self) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_easy_stop_scope]

Tells the rig to stop sending scope sweeps by running `Op::SetScopeOutput(false)` with the default deadline. Returns `Ok(())` once the rig accepted it, or the error (`Unsupported`, `LinkDown`, `Timeout`, `QueueFull`). Streams already obtained from `start_scope` stay subscribed and simply see no more frames until output is started again. Idempotent as far as the rig is concerned, never transmits.

### `pub fn transmit_lease( &self, permit: TransmitPermit, max: Duration, ) -> Result<TransmitLease, RigError>` [@ANCHOR: radioctl:radioctl_core_easy_transmit_lease]

Opens this rig's transmit gate for a bounded time and returns the lease that holds it. `permit` (owned) is the explicit decision to allow transmitting, and no other helper opens the gate. `max` is the lifetime (a `Duration`, must be greater than zero). Refusals: a zero `max` gives `Rejected`; calling outside a Tokio runtime gives `Unsupported` (a timer task is needed); a gate that is already open (someone else decided that) gives `Rejected`. On success it opens the gate, spawns a timer that, after `max`, closes the gate and fires an unkey, and returns the `TransmitLease`. Ending the lease (release, drop, or the timer) always closes the gate and unkeys; the unkey takes the emergency path and is never refused for lack of a queue slot. Not async; it does not key the radio.

### `pub async fn next_change(&mut self, field: Field) -> Option<(Change, Source)>` [@ANCHOR: radioctl:radioctl_core_easy_next_change]

On an `EventStream`, waits for the next event that contains a change to `field` and returns that `Change` (old and new values) together with the event's `Source` (API, poll, transceive, splitter or watchdog). Events that do not change `field` are skipped, and events missed because the subscriber lagged are skipped by the stream and counted. Returns `None` once the rig has stopped. It waits without limit, so wrap it in `tokio::time::timeout` to bound the wait. Cancel-safe because the underlying `next` is. Takes `&mut self`, so one task owns the stream.

### `pub fn remaining(&self) -> Duration` [@ANCHOR: radioctl:radioctl_core_easy_lease_remaining]

Returns how much of a transmit lease is left: the time until its deadline, or `Duration::ZERO` once the lease has ended (released, or the timer task has finished). Uses saturating arithmetic, so a value never goes negative. Read-only and cheap; `transmit_for` uses it to cut a transmission short. It is a snapshot, as the lease can end right after the call.

### `pub async fn key(&self) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_easy_lease_key]

Keys the transmitter through the rig handle (`set_ptt(true)`). The lease holds the gate open, so while it lasts the key is accepted subject to the actor's other checks: a known transmit frequency, link up, and the deadline. After the lease has ended the gate is closed and the call is refused with `TransmitGateClosed`, like any transmit command with the gate closed. The actor's transmit time limit (`max_transmit`, default 180 s) and heartbeat still apply, so a keyed rig is unkeyed even if the lease is longer. Returns `Ok(())` or the error.

### `pub async fn unkey(&self) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_easy_lease_unkey]

Unkeys the transmitter through the rig handle (`set_ptt(false)`). Always allowed: it needs no gate and takes the emergency path ahead of other commands. It does not end the lease and does not close the gate; the lease stays as it is until released, dropped or timed out. Returns `Ok(())` when the rig confirmed the unkey, or the error (`LinkDown`, `Timeout`); a failed unkey is also counted and reported as `Alarm::UnkeyFailed`.

### `pub async fn transmit_for(&self, duration: Duration) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_easy_lease_transmit_for]

Keys the rig, holds the key for `duration`, and unkeys. The hold is the smaller of `duration` and the lease's remaining time, so a transmission is never longer than the lease. If the key fails the error is returned and nothing is held. If the future is dropped or cancelled while keyed, the lease's `Drop` unkeys, and the actor's watchdog is the further backstop. Returns the result of the final unkey, so a failure to unkey is visible to the caller. Async; the sleep uses Tokio time, so it needs the runtime's time driver.

### `pub async fn release(mut self) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_easy_lease_release]

Ends the lease now. It consumes the lease, marks it ended, aborts the timer, closes the transmit gate, and then unkeys with `set_ptt(false)`, returning that result so the caller learns whether the unkey succeeded. Because the lease is marked ended, its `Drop` does nothing further. The gate is closed before the unkey is sent; an unkey needs no gate, so it is not refused. Returns `Ok(())` or the unkey error (`LinkDown`, `Timeout`).

### `fn drop(&mut self)` [@ANCHOR: radioctl:radioctl_core_easy_lease_drop]

Ends a transmit lease that was simply dropped: it aborts the timer and, if the lease had not ended and the timer had not already finished, closes the transmit gate and sends an unkey without awaiting it (`end_lease`, synchronous because the unkey takes the emergency path and is never refused). If the timer already ended the lease, or `release` ran, nothing more is done. So a lease that is dropped on an error path, a panic that unwinds, or a cancelled future cannot leave the gate open or the rig keyed. No return and no error.

## `crates/radioctl-core/src/rig.rs`

### `fn poll_event(&mut self, cx: &mut Context<'_>) -> Poll<Option<RigEvent>>` [@ANCHOR: radioctl:radioctl_core_event_stream_poll]

The shared core of the event stream's `next` and `poll_next`. It keeps one pending receive future (a boxed `recv` on the broadcast receiver, which owns the receiver while it runs) so the stream is cancel-safe. A received event returns `Ready(Some)`. If the subscriber lagged, the number of missed events is added to this stream's `lagged` (saturating) and to the rig's `stream_lagged` counter (through a weak reference to the shared state, so the stream does not keep the rig alive), and the loop continues with the next event it still has. A closed channel (the rig stopped) sets `done` and returns `Ready(None)`, and stays `None`. `cx` is the task context for wakeups.

### `pub async fn next(&mut self) -> Option<RigEvent>` [@ANCHOR: radioctl:radioctl_core_event_stream_next]

Waits for the next status-change event and returns it, or `None` once the rig has stopped. Cancel-safe: dropping the future loses no event, because the pending receive is kept in the stream. Events that this subscriber missed because it fell behind are skipped and counted (see `lagged`). Built on `poll_event` through `poll_fn`. Takes `&mut self`, so one task consumes a given stream.

### `pub fn lagged(&self) -> u64` [@ANCHOR: radioctl:radioctl_core_event_stream_lagged]

Returns the total number of events this subscriber has missed because it fell more than `stream_capacity` events behind (default 64), summed over the life of this stream. It is per stream; the rig-wide total across all subscribers is `RigStats::stream_lagged`. A value above zero means the subscriber should treat its view as possibly incomplete and re-read state (`cached` or `Fresh` reads). Read-only.

### `fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<RigEvent>>` [@ANCHOR: radioctl:radioctl_core_event_stream_poll_next]

The `futures_core::Stream` implementation for `EventStream`, so it can be used with stream combinators. It simply calls `poll_event` on the unpinned stream (the type is `Unpin`), so it behaves exactly like `next`: yields each `RigEvent`, skips and counts missed events, and yields `None` after the rig has stopped. `cx` is the waker context.

### `async fn run(mut self)` [@ANCHOR: radioctl:radioctl_core_actor_run]

The rig actor's main loop; it runs as one task and owns the engine and the transport, so all rig I/O is serialized here. At start it applies any link status that changed before the actor existed, and writes the engine's session-opening requests. Then it loops on a biased `select` in this priority: emergency commands (unkey, stop-Morse; the pending count is decremented and the command served without preemption); link status changes (a closed watch drops the watch and marks the link down); the transmit watchdog deadline; heartbeat wakeups; normal commands (after the reserved PTT poll); bytes arriving from the rig when the link is up (fed to the engine as unsolicited traffic; an end of stream or error takes the link down); and finally the next due poll. It ends when either command channel closes (every handle dropped), after which `unkey_on_stop` unkeys a keyed rig or one whose gate was open.

### `async fn serve(&mut self, command: Command, may_preempt: bool)` [@ANCHOR: radioctl:radioctl_core_actor_serve]

Executes one queued command and answers its caller. `may_preempt` is true for normal commands (they may be abandoned for an emergency command) and false for emergency ones. It runs `run_command`, then counts `commands_ok` or `commands_failed`, and counts `preempted` when the error was `Cancelled`. For an unkey command (`SetPtt(false)`) that failed, it counts an `unkey_failures` and emits `Alarm::UnkeyFailed` with the command's source, because the rig may still be on the air; a failed stop-Morse is not an unkey and is reported to its caller only. Any unkey requests that arrived when the emergency queue was full ride on this result and are answered with a clone of it. Finally the result is sent to the caller's reply channel (a dropped receiver is ignored).

### `async fn run_command( &mut self, op: &Op, deadline: Instant, source: Source, may_preempt: bool, ) -> Reply` [@ANCHOR: radioctl:radioctl_core_actor_run_command]

Runs one command against the radio with all the safety checks, and is the center of the actor. Refusals in order: link down (`LinkDown`), deadline already passed (`Timeout`), a transmitting op with the gate closed (`TransmitGateClosed`), a transmitting op while the transmit frequency is unknown (`TransmitFrequencyUnknown`), and an antenna change while the rig is known keyed (`Unsupported`). A control-line op goes to `run_line_steps`. Otherwise the op is encoded by the engine, and written with the inter-command spacing and after draining late replies of abandoned commands (emergency commands skip both). Non-transmit ops are retried `retries` extra times; a transmitting op is tried once. If the write times out, or a transmit command is not answered, the actor unkeys fail-closed and returns `Timeout`. A reply decides the result; on success the cache and the watchdog are updated (`apply_set`). A command waiting for a reply can be preempted (`Cancelled`) by an emergency command or the watchdog.

### `fn require_known_tx_frequency(&self) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_actor_require_known_tx_frequency]

Refuses a transmit-capable command while the frequency the rig would transmit on is not known from the rig's own reports. It builds the transmit state from the cache and requires that the split state (on a rig with the `Split` capability) and the transmit VFO's frequency are both present; otherwise it returns `TransmitFrequencyUnknown` with the transcript tail. It fails closed. The handle reads those values before submitting such a command (`prime_tx_state`), so through the handle a refusal happens only when the rig could not be read. Read-only on the cache; briefly takes its mutex.

### `async fn run_line_steps( &mut self, op: &Op, steps: &[LineStep], deadline: Instant, source: Source, ) -> Reply` [@ANCHOR: radioctl:radioctl_core_actor_run_line_steps]

Runs a control-line command: a sequence of DTR/RTS (or similar) line changes with holds, for rigs whose commands are line patterns rather than bytes. No bytes are written and no reply is expected, so the command is complete after the last step. It is refused for a transmitting op (`Unsupported`, a line sequence never carries a transmit command), when the transport has no control lines (`Unsupported`) and when the link is down (`LinkDown`). The sequence runs on a blocking thread (`spawn_blocking`) because each line call is a system call; the remaining time to `deadline` is the limit between steps. A failing step stops the sequence there (a failure of kind `LinkDown` also takes the link down) and its error is returned; a join failure is `LinkDown`. On success the step count is noted in the transcript, the op's effect is applied to the cache, and `Ok(None)` is returned.

### `fn drive_lines( lines: &dyn ControlLines, steps: &[LineStep], limit: Duration, ) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_rig_drive_lines]

Executes a line sequence synchronously on the calling thread (the actor calls it from `spawn_blocking`). For each step in order it checks the elapsed time since the start against `limit` and returns a `Timeout` error ("control-line sequence did not finish before the deadline") if it has passed, sets the step's line to asserted or not with `lines.set_control` (an error from the driver is returned at once), and then sleeps for the step's hold with `thread::sleep` if it is non-zero. Blocking; never call it on the async runtime directly. A limit that lapses mid-sequence leaves the lines as the last step set them; the next sequence begins by putting them idle.

### `fn observe(&mut self, value: Observation, source: Source)` [@ANCHOR: radioctl:radioctl_core_actor_observe]

Records something the rig reported. For a PTT observation it keeps the transmit watchdog in step: `Ptt(true)` arms it (`arm_watchdog`) and `Ptt(false)` disarms it. The value is stored in the cache with the current time, and if it differs from the old value a `Change` (old and new) is created. It then derives the active transmit frequency from the cache (split state, VFOs, keyed state) and, when that derived value changed, stores it and adds a `Change` for `Field::ActiveTxFrequency`, so amplifier and band-decoder subscribers follow the transmit frequency whichever field moved it. If there are changes, one `RigEvent` is emitted with `source`. An unchanged value updates the age only and emits nothing.

### `async fn poll_once(&mut self)` [@ANCHOR: radioctl:radioctl_core_actor_poll_once]

Runs at most one scheduled poll. It decides whether the rig is transmitting from the cached PTT value, then asks the schedule for the next due entry (skipping entries that `take_due` pushed back because the rig is transmitting, as only transmit state and power are polled while keyed). If an entry was found it is polled with `poll_entry`; if nothing is due it returns without I/O. Called by the actor loop only when no command is pending and the link is up, so polling never delays a command beyond the current poll, whose deadline is one reply timeout.

### `async fn poll_entry(&mut self, entry: PollEntry)` [@ANCHOR: radioctl:radioctl_core_actor_poll_entry]

Reads one polled field and adapts its schedule. It counts a `polls`, notes the cached value before, runs the entry's read op through `run_command` with source `Poll`, a deadline of one reply timeout (from the engine's description) and preemption allowed, then compares the cached value after. It reports to the schedule whether the value changed and whether the command failed: a change resets the interval to its minimum, no change doubles it up to the maximum, a failure goes straight to the maximum. A new value reaches subscribers through `observe` (events with source `Poll`). Never transmits.

### `async fn poll_reserved(&mut self)` [@ANCHOR: radioctl:radioctl_core_actor_poll_reserved]

The transmit state's reserved poll share, run before a queued normal command is served. If the link is up and the PTT field has not been polled for `PollConfig::ptt_reserve` (default 2 s), it counts a `reserved_polls` and polls PTT now. Without it a client issuing back-to-back commands would keep polling from ever running, and a radio that unkeyed or keyed on its own would go unnoticed, so the watchdog and the cache would stay wrong for as long as the traffic lasted. Does nothing when the link is down or the PTT poll is not starved.

## `crates/radioctl-protocols/src/io.rs`

### `pub fn spawn_virtual_rig( runtime: &Handle, rig: VirtualRig, reply_delay: Duration, ) -> (DuplexStream, VirtualRigControl)` [@ANCHOR: radioctl:radioctl_virtual_io_spawn]

Runs a `VirtualRig` (a software model of a radio, sans-IO) behind an in-memory duplex stream (4096 bytes) on `runtime`, so an actor can talk to it like to a serial port. `rig` is moved in; `reply_delay` models a slow radio: the command is applied at once, but its answer reaches the stream only after the delay, in order. It returns the stream end for the actor and a `VirtualRigControl` with fault-injection knobs. One task reads the controller's bytes (counting them, optionally echoing them, feeding the rig, and applying the drop, truncate and jitter knobs to the reply) and one task writes the due replies, optionally in small chunks. The tasks end when the stream closes. No hardware or RF is involved.

### `pub fn state(&self) -> RigState` [@ANCHOR: radioctl:radioctl_virtual_io_state]

Returns a copy of the virtual rig's current state (frequencies, mode, PTT, levels and so on) for tests to check what a command did. It locks the rig's mutex briefly (poisoning tolerated) and clones the state. Read-only; a snapshot taken at the call.

### `pub fn front_panel(&self, change: PanelChange)` [@ANCHOR: radioctl:radioctl_virtual_io_front_panel]

Simulates a change made at the radio itself, not over the wire: `PanelChange` is one of a frequency (`Hz`, VFO A), a mode, a PTT press or release, a signal level (`u16`), split on or off, or a RIT/XIT offset (`i16`, hertz). The rig updates its state, and any unsolicited bytes it produces (transceive reports) are sent to the controller at once with no reply delay and without being subject to the drop or truncate knobs. Nothing is sent when the model produces no unsolicited bytes. Used to test the actor's handling of a radio that keys or changes frequency by hand.

### `pub fn set_reply_delay(&self, delay: Duration)` [@ANCHOR: radioctl:radioctl_virtual_io_set_reply_delay]

Changes how long answers take, from the next command on. `delay` is a `Duration` added to each reply's due time (together with any jitter). Replies already queued keep the delay they were given. Takes effect through a mutex, so it is safe from any thread. Cannot fail. A large delay is how tests make a command time out.

### `pub fn drop_replies(&self, count: usize)` [@ANCHOR: radioctl:radioctl_virtual_io_drop_replies]

Loses the next `count` answers on the way back, as a corrupted frame would be, while the commands still take effect on the rig. The counter is stored (it replaces, not adds to, any earlier count) and each answer that is non-empty consumes one. Replies that are empty (the command had no answer) do not consume the count. Fault injection for the timing and retry policy of the actor. Cannot fail.

### `fn next_jitter(shared: &Shared) -> Duration` [@ANCHOR: radioctl:radioctl_virtual_io_next_jitter]

Returns a pseudo-random share of the configured jitter, from zero up to the whole jitter, to add to a reply delay. If the jitter is zero it returns zero without touching the generator. Otherwise it advances a xorshift generator with a fixed seed (so test runs are repeatable), turns the state into a fraction between 0 and 1 using a modulus of 1,000,001, and returns the jitter multiplied by it. Locks the jitter and generator mutexes (poisoning tolerated). Not cryptographic and not meant to be.

### `pub fn set_reply_jitter(&self, jitter: Duration)` [@ANCHOR: radioctl:radioctl_virtual_io_set_reply_jitter]

Adds a random delay from zero to `jitter` to every answer from now on, on top of the fixed reply delay, to model an uneven radio. The sequence is repeatable because the generator has a fixed seed. A zero `jitter` turns it off. Because later replies can get a shorter delay than earlier ones, but the writer sends them in order, order is kept. Cannot fail.

### `pub fn set_echo(&self, on: bool)` [@ANCHOR: radioctl:radioctl_virtual_io_set_echo]

With `on` true, the controller's own bytes come back on the stream immediately, before the answer, as on a single-wire CI-V bus; with `false` it stops that. The echo is of each chunk of bytes read by the virtual rig, sent with no delay. The flag is read for every received chunk, so it takes effect from the next write. Cannot fail.

### `pub fn set_reply_chunk(&self, bytes: usize)` [@ANCHOR: radioctl:radioctl_virtual_io_set_reply_chunk]

Writes every answer in pieces of at most `bytes` bytes, with a 1 ms gap between pieces, so a frame arrives across several reads of the controller. A value of 0 (the default) writes answers whole. Used to test that the engine's parser reassembles split frames. Takes effect for the next answer written. Cannot fail.

### `pub fn truncate_replies(&self, count: usize)` [@ANCHOR: radioctl:radioctl_virtual_io_truncate_replies]

Cuts the last byte off the next `count` non-empty answers (a frame that never ends, for CI-V the closing terminator), while the commands still take effect on the rig. The count replaces any earlier value. Fault injection to test that the actor and parser recover from a frame cut short. Cannot fail.

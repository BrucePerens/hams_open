# radioctl function anchors, batch 23

Function anchors for the `radioctl-core` crate: the rig actor's spawn helpers, the `RigHandle` accessors and typed commands, the transmit watchdog and link-loss handling inside the actor, the rig description file loader, and the level helpers. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
For anything that can key a transmitter, the entry says how the gate applies: the per-rig transmit gate starts closed and is opened only with a `TransmitPermit`; a keying command is refused with `TransmitGateClosed` at submission and again when the actor runs it; unkeying never needs the gate and takes the emergency queue.

## `crates/radioctl-core/src/rig.rs`

### `pub fn spawn_current<E: ProtocolEngine, T: Transport>( engine: E, transport: T, options: RigOptions, ) -> Result<RigHandle, RigError>` [@ANCHOR: radioctl:radioctl_core_rig_spawn_current]

Like `spawn`, but takes the runtime from the calling task instead of an argument. `engine` (the protocol engine) and `transport` (the byte stream to the radio) are moved into the actor, and `options` is consumed. It looks up `Handle::try_current()`; if the call is made outside any Tokio runtime it returns an `Unsupported` error whose text says there is no tokio runtime. Otherwise it returns whatever `spawn` returns, a `RigHandle`. The gate starts closed, so nothing can transmit until the caller opens it. Not async, does not block, and sends nothing to the radio itself.

### `pub fn shutdown(self)` [@ANCHOR: radioctl:radioctl_core_rig_owned_shutdown]

Stops the small runtime owned by an `OwnedRig` without waiting for its tasks (`shutdown_background`), which is how `radioctld` and the C ABI end a rig. It consumes the `OwnedRig`. It must be called from outside any async context (the doc says so). Because tasks are not awaited, whether the actor's stop path (which unkeys a keyed rig or one whose gate is open) completes is not guaranteed by this call, so a caller should unkey and close the gate through the handle first and wait for that to finish. Only with the `owned-runtime` feature. Cannot return an error.

### `pub fn spawn_with_owned_runtime<E: ProtocolEngine, T: Transport>( make_transport: impl FnOnce() -> T, engine: E, options: RigOptions, ) -> Result<OwnedRig, RigError>` [@ANCHOR: radioctl:radioctl_core_rig_spawn_with_owned_runtime]

Builds a one-worker multi-thread Tokio runtime of its own (thread name `radioctl-rig`, time driver on) and spawns the rig on it, for `radioctld` and the C ABI; library users should pass their own runtime to `spawn`. `make_transport` is a closure called once with the new runtime entered, because some transports (a serial or TCP stream) need a runtime context to be created. `engine` and `options` are moved. Returns an `OwnedRig` holding the handle and the runtime, or a `LinkDown` error if the runtime cannot be built, or whatever `spawn` fails with. Blocks only to build the runtime. Gate starts closed.

### `pub fn id(&self) -> &str` [@ANCHOR: radioctl:radioctl_core_handle_id]

Returns the rig's identifier string, taken from the engine's description when the rig was spawned (for example `icom-ic-7300`), borrowed from the shared state. It is the value placed in each `RigEvent` and used by daemons to name the rig. Read-only, never blocks, cannot fail.

### `pub fn capabilities(&self) -> &CapabilitySet` [@ANCHOR: radioctl:radioctl_core_handle_capabilities]

Returns what the rig can do (read frequency, set PTT, second VFO, split, scope and so on) as a borrowed `CapabilitySet` copied from the description at spawn. Commands whose required capability is missing are refused with `Unsupported` at submission, so callers can check here first. The set never changes during the life of the rig. Read-only, cannot fail.

### `pub fn open_transmit_gate(&self, permit: TransmitPermit)` [@ANCHOR: radioctl:radioctl_core_handle_open_gate]

Opens the transmit gate for this rig only; it starts closed. `permit` (owned) records who decided transmitting is allowed and is stored so `transmit_permit` can report it. The gate flag is set with release ordering after the permit is stored. While open, keying (`SetPtt(true)`) and Morse (`SendCw`) can be queued; they still go through the actor's other checks (a known transmit frequency, link up, lock). It does not key anything. Idempotent: calling again replaces the permit. The decision to call it belongs to the owner of the station; this function does no validation.

### `pub fn close_transmit_gate(&self)` [@ANCHOR: radioctl:radioctl_core_handle_close_gate]

Closes the transmit gate and forgets the permit. Transmit commands already queued are refused when the actor runs them (`TransmitGateClosed`), and new ones are refused at submission. It does not unkey a rig that is already transmitting: use `set_ptt(false)`, which needs no gate. Idempotent, takes no arguments, cannot fail, and is safe to call from any thread.

### `pub fn transmit_permit(&self) -> Option<TransmitPermit>` [@ANCHOR: radioctl:radioctl_core_handle_gate_state]

Reports who opened the gate: a clone of the stored `TransmitPermit` while the gate is open, `None` while it is closed (even if a stale permit is still stored). Used by status displays and tests to show the gate state. Briefly takes the permit mutex (poisoning is tolerated). A snapshot: the gate may change right after the call.

### `pub fn stats(&self) -> RigStats` [@ANCHOR: radioctl:radioctl_core_handle_stats]

Returns a snapshot of the rig's counters as a `RigStats` struct of 19 `u64` fields: commands that succeeded and failed, commands preempted, polls and reserved polls, unsolicited observations, late and stale replies, stale drains, unrecognised bytes, events produced, lag counts for the event, scope and filter-scope streams, scope frames, `watchdog_unkeys`, `unkey_failures` (an unkey the actor tried and could not complete, so the rig may be on the air) and `unkeys_coalesced`. Each counter is read separately with relaxed ordering, so the set is not one atomic instant. Read-only, never blocks.

### `pub fn link_up(&self) -> bool` [@ANCHOR: radioctl:radioctl_core_handle_link_up]

Returns whether the actor currently believes the transport is up: false once a write failed or the link status watch reported `Reconnecting` or `Closed`, true again after a reconnect (a restored link first unkeys the rig). While false every command is refused with `LinkDown`. Starts true. Read with acquire ordering; a snapshot.

### `pub fn heartbeat(&self)` [@ANCHOR: radioctl:radioctl_core_handle_heartbeat]

Tells the actor the caller is alive. With `RigOptions::heartbeat` set, a keyed rig whose caller stops calling this for the configured period is unkeyed by the actor (`Alarm::HeartbeatLapsed`). The call stores the milliseconds since the rig's start in an atomic (saturating at `u64::MAX`) and wakes the actor so it recomputes its watchdog deadline. Cheap, never blocks, no error. It has no effect when no heartbeat period is configured, and the heartbeat is not enforced while receiving. A caller that keys and then beats is not tripped by the time before it keyed.

### `fn watchdog_due(&self) -> Option<Instant>` [@ANCHOR: radioctl:radioctl_core_actor_watchdog_due]

Computes when the transmit watchdog next fires. `None` when the rig is not known to be transmitting (`keyed_since` is unset). Otherwise the time limit is `keyed_since` plus `max_transmit` (default 180 s; `Duration::MAX` disables it). If a heartbeat period is configured the deadline is the earlier of that limit and the heartbeat deadline, counted from the later of the last heartbeat and the moment of keying. Uses saturating deadline arithmetic, so huge periods do not overflow. Pure read of actor state; the main loop sleeps until this instant.

### `async fn watchdog_fire(&mut self)` [@ANCHOR: radioctl:radioctl_core_actor_watchdog_fire]

Runs when the transmit watchdog lapses, so a rig cannot transmit for ever. It takes `keyed_since` (disarming the watchdog; returns at once if it was already unset), decides the reason (`Alarm::TransmitTimeout` if `max_transmit` has passed, else `Alarm::HeartbeatLapsed`), counts a `watchdog_unkeys`, emits the alarm event with source `Watchdog`, and unkeys through `unkey_fail_closed`. If the rig is still seen keyed afterwards (the unkey failed, or the operator keyed again) the watchdog is re-armed by the next observation. The unkey does not need the gate. Async; the unkey can take up to three attempts of one reply timeout each.

### `fn arm_watchdog(&mut self, now: Instant)` [@ANCHOR: radioctl:radioctl_core_actor_arm_watchdog]

Starts the transmit watchdog if it is not already running: when `keyed_since` is `None` it is set to `now` and `WATCHDOG armed` is added to the transcript. If already armed it does nothing, so the time limit is measured from the first moment the rig was keyed (or Morse sent), not from each report. Called when a key or Morse command succeeds and when an observation shows PTT on. `now` is a monotonic Tokio instant. No return, cannot fail.

### `fn alarm(&mut self, alarm: Alarm, source: Source)` [@ANCHOR: radioctl:radioctl_core_actor_alarm]

Emits an alarm event: a `RigEvent` for this rig with `source` and one `Change` on `Field::Alarm` whose new value is `Observation::Alarm(alarm)` and old is `None`. It also adds `ALARM <name>` to the transcript. Alarms (`TransmitTimeout`, `HeartbeatLapsed`, `UnkeyFailed`, `LinkDown`, `LinkRestored`) are never deduplicated and never written to the cache, so each occurrence reaches every subscriber. `source` names the watchdog or the caller whose unkey failed. No return value; broadcast sends never wait for a subscriber.

### `async fn link_status_changed(&mut self)` [@ANCHOR: radioctl:radioctl_core_actor_link_status_changed]

Applies the transport's current link status (from `RigOptions::link_watch`; with none it returns at once). `Reconnecting` and `Closed` call `link_down`, so every command is refused with `LinkDown` and one `Alarm::LinkDown` is reported. `Connected { generation }` with a generation not yet seen brings the link up; the very first connection is silent, but a later reopen first adds `LINK UP` to the transcript, unkeys the rig through the fail-closed path before any queued command is served (the rig may have been keyed when the port was lost), runs the engine's session restore, and only then emits `Alarm::LinkRestored`, so a subscriber that reacts finds the rig unkeyed. Async.

### `async fn link_changed(watch: Option<&mut watch::Receiver<LinkStatus>>) -> bool` [@ANCHOR: radioctl:radioctl_core_link_changed]

Waits for the next change of the link status. `watch` is the optional receiver from `RigOptions::link_watch`, borrowed mutably. Returns true when a change arrived and false when the watch's sender is gone (the serial supervisor ended). On a false result the actor's main loop treats the link as gone for good: it drops the watch (so this branch is never selected again) and marks the link down with the reason "link status watch closed". With `None` it never completes, which disables that branch of the actor's `select` for transports that cannot report status. Cancel-safe.

### `fn link_down(&mut self, why: &str)` [@ANCHOR: radioctl:radioctl_core_actor_link_down]

Marks the transport as failed. `why` (borrowed text such as the I/O error or `port lost: ...`) is added to the transcript as `LINK DOWN`. The link-up flag is swapped to false; only if it was up is `Alarm::LinkDown` emitted once with source `Api`, so repeated failures are not repeated alarms. After this every command is refused with `LinkDown`. A keyed rig whose link drops is on the air with no way to unkey it by command, which is why the alarm must reach the operator at once; the unkey is retried when the link returns.

### `fn note_unsolicited(&mut self)` [@ANCHOR: radioctl:radioctl_core_actor_note_unsolicited]

Handles a frame that arrived when no command was waiting for it. On a rig with `Capability::Transceive` it counts an `unsolicited` observation and tells the poll schedule that transceive is active, so polling slows to its maximum interval. On any other rig the frame is a late reply to a command already given up: it is counted in `late_replies` and does not change polling, so one stray reply cannot pin the poll interval for the life of the rig. No arguments or return value.

### `fn emit(&self, event: RigEvent)` [@ANCHOR: radioctl:radioctl_core_actor_emit]

Publishes an event to the stream subscribers. It counts one `events` and sends a clone through the broadcast channel (capacity `stream_capacity`, default 64) without waiting: a subscriber that falls behind loses its oldest events and counts a lag, and a send with no subscribers is ignored. So the actor never blocks on a slow consumer. `event` is owned. No error return.

### `pub fn events(&self) -> EventStream` [@ANCHOR: radioctl:radioctl_core_handle_events]

Subscribes to the rig's status-change stream, starting now: events caused before the call are not replayed. Returns an `EventStream`, a `futures_core::Stream<Item = RigEvent>` that ends when the rig stops. The caller spawns a task on its own runtime to consume it. Each subscriber has its own buffer of `stream_capacity` events; one that lags is told and counted (`stream_lagged`) rather than blocking the actor. It holds only a weak reference to the shared state. Cannot fail.

### `pub fn cached(&self, field: Field) -> Option<Cached<Observation>>` [@ANCHOR: radioctl:radioctl_core_handle_cached]

Returns the cached value of `field` with its age (time since the rig last reported it), without touching the rig; `None` if the field has never been seen. Takes the cache mutex briefly. The age is measured at the call and uses saturating arithmetic. Used by the splitter and daemons for fast reads and by the transmit-frequency checks. A cached value can be stale; it says so in `age`.

### `pub async fn execute(&self, op: Op) -> Reply` [@ANCHOR: radioctl:radioctl_core_handle_execute]

Runs one command with the rig's default deadline (`RigOptions::default_timeout`, 2 s unless set). It is `execute_within` with that timeout, and records the source as the API. The reply is `Ok(Some(observation))` for a read, `Ok(None)` for a set, or the error: `Unsupported`, `TransmitGateClosed`, `QueueFull`, `Timeout`, `LinkDown`, `TransmitFrequencyUnknown` and engine errors. A keying command needs the gate open; unkeying never does.

### `pub async fn execute_within(&self, op: Op, timeout: Duration) -> Reply` [@ANCHOR: radioctl:radioctl_core_handle_execute_within]

Runs a command that must finish within `timeout` (counted from submission). If the op can transmit and the gate is open it first reads whatever the transmit-frequency derivation lacks (errors ignored). It then checks the capability and, for a transmitting op, the gate, and queues the command: ordinary ops on the normal queue (capacity 64, `QueueFull` if full), unkeying and stop-Morse on the emergency queue, which is taken first and preempts a command waiting for a reply. The caller waits only on a channel; the actor enforces the deadline. If the actor has stopped the result is `LinkDown`. Source is the API.

### `pub async fn frequency(&self, vfo: Vfo, freshness: Freshness) -> Result<Cached<Hz>, RigError>` [@ANCHOR: radioctl:radioctl_core_handle_frequency]

Reads the frequency of `vfo` in hertz. `freshness` says how old a cached value may be: `Cached` (any cached value, asking the rig only if nothing is cached), `MaxAge(d)` (not older than `d`) or `Fresh` (always ask). Returns the value with its age (zero when freshly read). Errors are those of `execute` (unsupported, link down, timeout, queue full), and `Malformed` if the rig returned a different kind of observation or no value. Never transmits.

### `pub async fn set_frequency(&self, vfo: Vfo, hz: Hz) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_handle_set_frequency]

Sets the frequency of `vfo` to `hz` (hertz, a `Hz` newtype). Goes through `execute` with the default deadline and discards the reply value. Returns `Ok(())` when the rig confirmed (or the engine needs no reply), or the error: `Unsupported`, `LinkDown`, `Timeout`, `QueueFull`, or a rig refusal. It does not key the transmitter, so no gate is involved, but changing the frequency changes where a later transmission goes; the actor's transmit checks use the frequency at keying time. The rig may reject out-of-range values; the range is the rig's own, `unknown` here.

### `pub async fn mode(&self, freshness: Freshness) -> Result<Cached<Mode>, RigError>` [@ANCHOR: radioctl:radioctl_core_handle_mode]

Reads the operating mode (`Mode`: LSB, USB, CW, FM and so on) honouring `freshness` (`Cached`, `MaxAge`, `Fresh`). Returns the value with its age. Errors as for `execute`; `Malformed` if the answer is not a mode or there was no value. Never transmits.

### `pub async fn set_mode(&self, mode: Mode) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_handle_set_mode]

Sets the operating mode. `mode` is the library's `Mode` enum, translated to the radio's code by the engine; a mode the radio's table lacks is an error from the engine. Runs through `execute` with the default deadline and returns `Ok(())` on success or the error (unsupported, link down, timeout, queue full). Does not key the transmitter and needs no gate.

### `pub async fn ptt(&self, freshness: Freshness) -> Result<Cached<bool>, RigError>` [@ANCHOR: radioctl:radioctl_core_handle_ptt]

Reads the transmit state: true when the rig is transmitting. `freshness` follows the usual rules, but a caller that needs the truth about a transmitter should use `Fresh`, since the cached value is only as new as the last poll or report. Returns the value and its age. Errors as for `execute`; `Malformed` for a wrong observation. A rig description without the read-PTT capability gives `Unsupported`. Never transmits.

### `pub async fn set_ptt(&self, on: bool) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_handle_set_ptt]

Keys (`on` true) or unkeys (`on` false) the transmitter. Keying is refused with `TransmitGateClosed` unless the gate was opened with a permit, needs a known transmit frequency (`TransmitFrequencyUnknown` otherwise), and arms the transmit watchdog; the actor will unkey after `max_transmit` or a lapsed heartbeat. Unkeying is always allowed, goes through the emergency queue ahead of other commands, preempts a command waiting for a reply and is retried if it fails; an unkey that finally fails raises `Alarm::UnkeyFailed`. Returns `Ok(())` or the error.

### `pub async fn signal(&self, freshness: Freshness) -> Result<Cached<u16>, RigError>` [@ANCHOR: radioctl:radioctl_core_handle_signal]

Reads the S-meter in the rig's raw units (a `u16`; the scale is the radio's own, so convert before showing it as S-units; the scale per rig is `unknown` here, check the rig's description). `freshness` selects cached, bounded-age or fresh. Returns the value and age; errors as for `execute`; `Malformed` for a wrong observation. Read-only, never transmits.

## `crates/radioctl-core/src/rigfile.rs`

### `pub fn rigs_dir() -> PathBuf` [@ANCHOR: radioctl:radioctl_core_rigfile_rigs_dir]

Returns the directory of rig description files. The environment variable `RADIOCTL_RIGS` wins when set (used as given, even if it does not exist). Otherwise `./rigs` in the current directory if that is a directory (so binaries copied to a bench machine run from the directory that holds `rigs/`). Otherwise the `rigs/` directory of the checkout the library was built from (found with `CARGO_MANIFEST_DIR` at compile time). Reads the environment and checks the file system; does not verify that the chosen directory exists or holds files, which `load` reports. Cannot fail.

### `pub fn load(which: &str) -> Result<LoadedDescription, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_load]

Reads and validates a rig description. `which` is either a rig id found in `rigs_dir()` (`icom-ic-7300` reads `icom-ic-7300.toml` there), or a path to a description file when it ends in `.toml` or contains a `/`. It reads the file as text and passes it to `parse`. A read failure gives a `Malformed` error naming the path and the cause; a parse or validation failure gives a `Malformed` error prefixed with the file path. Returns the description together with its provenance facts. Blocking file read; call from setup code, not from a hot path.

## `crates/radioctl-core/src/rig/easy.rs`

### `pub async fn level(&self, level: Level, freshness: Freshness) -> Result<Cached<u16>, RigError>` [@ANCHOR: radioctl:radioctl_core_easy_level]

Reads a numeric level (such as AF gain, RF power or the antenna selector, one of the `Level` variants) in the rig's own units as a `u16`. `freshness` chooses a cached, bounded-age or fresh value. Returns the value with its age, or the error from `execute`; `Malformed` when the rig answered with a different level than the one asked, or with no value. Never transmits.

### `pub async fn set_level(&self, level: Level, value: u16) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_easy_set_level]

Sets a numeric level in the rig's own units (a `u16`; the valid range depends on the radio and the level and is `unknown` here, the engine or the rig rejects out-of-range values). Runs `Op::SetLevel` through `execute` with the default deadline and discards the reply. Setting the antenna level is refused while the rig is known to be keyed, because an antenna change moves the RF path. Returns `Ok(())` or the error. Does not key the transmitter, but RF power set here limits what a later key puts out.

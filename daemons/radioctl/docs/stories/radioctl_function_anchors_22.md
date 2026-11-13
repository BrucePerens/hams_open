# radioctl function anchors, batch 22

Function anchors for the `radioctl-splitter` crate (client handles, the splitter's session machinery, the TS-2000 personality) and the `radioctl-core` crate (rig handle, event merge, poll schedule, transmit permit, actor spawn), written to the ADR 0113 standard. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
For anything that can key a transmitter, the entry says how the gate applies: every splitter request goes through the rig handle, so the per-rig transmit gate, the transmit permit, the actor's frequency and band checks and its watchdog all still apply; the splitter's PTT (push to talk) arbiter only chooses which client may ask.

## `crates/radioctl-splitter/src/session.rs`

### `pub async fn wait(self)` [@ANCHOR: radioctl:radioctl_splitter_client_wait]

Waits until the client's main task has ended, whatever ended it (the client disconnected, the idle timeout fired, a channel closed, or the task was aborted). It consumes the `ClientHandle`. After the main task finishes it aborts the stream reader and writer tasks (the `io` list, empty for channel clients) so that the stream is dropped and the far end sees it close. A panic or cancellation of the task is ignored. `serve_tcp` uses it to hold a connection slot until the client is really gone. It does not itself unkey the rig: the client task does that in its own cleanup before it ends.

### `pub fn abort(&self)` [@ANCHOR: radioctl:radioctl_splitter_client_abort]

Stops the client's task at once and also aborts its stream reader and writer tasks, which drops the stream so the far end sees the connection close. It is a hard stop: the client task's own cleanup (`cleanup`, which gives up PTT ownership and unkeys the rig if this client held the transmitter) does not get to run, so the PTT ownership is not released. To disconnect cleanly, drop the stream instead and let the client see `Disconnected`. Takes `&self`, does not wait, and is safe to call more than once.

### `pub fn new(rig: RigHandle, config: SplitterConfig) -> Splitter` [@ANCHOR: radioctl:radioctl_splitter_new]

Creates a splitter in front of one rig. `rig` (a cloneable handle, owned) is the only way the splitter reaches the radio, so all its requests pass the rig's queue, gate and checks. `config` is copied. The shared state starts with nobody owning the transmitter, client ids numbered from 1, zeroed counters, and a start instant used as the clock for the personalities. `Splitter` is cheap to clone (an `Arc`); all clones share state. No tasks are started until a client is attached or `serve_tcp` is called. Cannot fail.

### `pub fn stats(&self) -> SplitterStats` [@ANCHOR: radioctl:radioctl_splitter_stats]

Returns a snapshot copy of the splitter's counters: `requests` (requests that reached the rig side), `cache_hits`, `refused`, `ptt_conflicts` (key or send-CW attempts refused because another client owned the transmitter), `clients` attached so far, `connections_refused` (TCP connections closed because `max_clients` were already connected) and `idle_disconnects`. Each counter is read separately with relaxed ordering, so the snapshot is not one atomic instant and may be slightly inconsistent under load. Read-only, never blocks, cannot fail.

### `pub fn ptt_owner(&self) -> Option<ClientId>` [@ANCHOR: radioctl:radioctl_splitter_ptt_owner]

Returns the id of the client that currently owns the transmitter, or `None` when no client does. It briefly takes the owner mutex (poisoning is tolerated, never panics). The value is a snapshot for status displays and tests; another client may claim or release the transmitter right after. It reports arbitration only, not whether the rig is actually keyed: a keyed rig can be unowned (keyed from the radio's own PTT) and an owner can exist while the rig refused.

### `pub fn attach_channels( &self, runtime: &Handle, input: mpsc::Receiver<ClientInput>, output: mpsc::Sender<Vec<u8>>, persistent: bool, ) -> ClientHandle` [@ANCHOR: radioctl:radioctl_splitter_attach_channels]

Attaches one client that talks to the splitter through two channels, and spawns its task (`run_client`) on `runtime`. `input` (owned) delivers `ClientInput::Bytes` or `Disconnected`; `output` (owned) receives reply and unsolicited bytes. `persistent` decides what `Disconnected` does: with true (a pty that programs open and close) it resets the personality and gives up the client's PTT but keeps the task alive for the next program; with false the client ends. The client gets the next id from an atomic counter (starting at 1) and the `clients` counter is incremented. Returns a `ClientHandle` with that id. No errors. The client's requests are subject to the same arbitration and gate checks as any other.

### `pub fn attach_stream<S: Transport>(&self, runtime: &Handle, stream: S) -> ClientHandle` [@ANCHOR: radioctl:radioctl_splitter_attach_stream]

Attaches a client that is any async byte stream (a TCP socket, an in-memory duplex) by splitting it into a reader task and a writer task joined to channels of 64 items, then calling `attach_channels` with `persistent` false. The reader reads up to 256 bytes at a time under the configured `idle_timeout` (default 600 s): silence longer than that counts an `idle_disconnects`, sends `Disconnected` and ends, so an abandoned connection cannot hold the transmitter. End of stream or a read error also sends `Disconnected`. The writer ends on a write error. The two tasks are stored in the handle so `abort` and `wait` can stop them. `stream` is owned and dropped when the tasks end. No errors returned.

### `pub fn serve_tcp(&self, runtime: &Handle, listener: TcpListener) -> JoinHandle<()>` [@ANCHOR: radioctl:radioctl_splitter_serve_tcp]

Starts a task that accepts connections on `listener` (already bound; owned) and attaches each as a stream client. A semaphore of `max_clients` slots (at least 1; default 16) limits simultaneous clients: a connection that finds no free slot is closed at once without reading a byte and counted in `connections_refused`. Accepted sockets get `TCP_NODELAY`. A permit is held by a small task until the client's task ends, whatever ended it. The accept loop ends when the listener returns an error or when the returned `JoinHandle` is aborted (the caller's way to stop serving). Bind the listener to loopback unless the network is meant to reach the radio; there is no authentication here, and a client that is accepted can ask to key the transmitter (subject to the rig gate).

### `async fn cleanup(&self, id: ClientId)` [@ANCHOR: radioctl:radioctl_splitter_cleanup]

Gives up a departing client's transmitter ownership. It releases `id` in the arbiter (under the owner mutex, not held across the await); only if that client really was the owner does it send `Op::SetPtt(false)` to the rig with source `Splitter` and the configured `request_timeout`, so a program that disconnects while keyed (or while sending Morse) cannot leave the radio transmitting. The unkey result is ignored here: the rig actor's own retry and `UnkeyFailed` alarm report a failed unkey. A client that was not the owner causes no rig traffic. Called when the client disconnects and when its task ends. Async.

### `async fn read(&self, field: Field, op: Op) -> Outcome` [@ANCHOR: radioctl:radioctl_splitter_read]

Answers a read request. If the rig handle has a cached value for `field` no older than `max_age` (default 5 s), it counts a `cache_hits` and returns `Outcome::Value` from the cache without touching the radio. Otherwise it runs `op` through `execute_from` with source `Splitter` and `request_timeout` (default 2 s) and returns the observed value. A reply with no value, or any error, becomes `Outcome::Refused`. Reads never transmit and never need the gate. Async; the cache is a snapshot of what the actor last saw.

### `async fn read_frequency(&self, vfo: Vfo) -> Outcome` [@ANCHOR: radioctl:radioctl_splitter_read_frequency]

Answers a frequency read for `vfo`. If the split-while-keyed rule applies (the configuration allows it, `vfo` is VFO A, and the cached PTT is on, split is on and the active transmit frequency is known), it answers with the transmit frequency, because OmniRig-style clients assume the plain read gives the transmit frequency while keyed in split. Otherwise it is an ordinary `read` of the VFO's frequency (cache first, then the rig). Returns `Outcome::Value(Observation::Frequency(..))` or `Refused`. Reads of the other VFO are not altered.

### `async fn read_status(&self) -> Outcome` [@ANCHOR: radioctl:radioctl_splitter_read_status]

Builds the composite status for the Kenwood `IF` answer. The receive VFO comes from the cached `RxVfo` observation (VFO A if the rig never reported one). Frequency of that VFO (through `read_frequency`), mode and transmit state must all be readable: if any is missing the whole request is `Refused`, because an `IF` that guessed the transmit state would lie about the transmitter. RIT (receiver incremental tuning) on/off, XIT (transmitter incremental tuning) on/off, the RIT offset (as `i16` hertz) and split are optional: a rig that cannot read one is reported at the neutral value (off, 0). Several reads are made one after another, each served from cache where fresh. Returns `Outcome::Status`.

### `async fn execute(&self, client: ClientId, request: &Request) -> Outcome` [@ANCHOR: radioctl:radioctl_splitter_execute]

Carries out one parsed client request and returns its outcome. It counts a `requests`, then dispatches: the reads (frequency, status, mode, PTT, signal, power) to the read helpers; frequency and mode sets to `set`; `SetPtt(true)` to `key` and `SendCw` to `send_cw`, both of which claim the transmitter for `client` first; `SetPtt(false)` sends the unkey and, if the rig confirmed, clears the arbiter's owner, whoever it was (unkeying is always allowed); `StopCw` is a stop path that any client may use, owner or not, with the gate open or closed, and it leaves ownership unchanged. A `Refused` outcome is counted in `refused`. All rig traffic uses source `Splitter`, so the rig's gate, permit, band and watchdog checks apply.

### `async fn send_cw(&self, client: ClientId, text: &str) -> Outcome` [@ANCHOR: radioctl:radioctl_splitter_send_cw]

Sends Morse `text` on behalf of `client`. Morse puts RF on the air, so it is treated like a key: the arbiter claim is taken first (`try_key`), and if another client owns the transmitter the call counts a `ptt_conflicts` and returns `Refused` without contacting the rig. Then `Op::SendCw` goes to the rig; if the rig refuses (gate closed, unsupported, link down, no frequency known) the claim is given back with `release`, so a refused send never strands ownership. If it is done, the client keeps ownership, and when that client goes away `cleanup` unkeys the rig. `text` is borrowed and copied into the op. The rig's own transmit watchdog still limits the transmission.

### `async fn key(&self, client: ClientId) -> Outcome` [@ANCHOR: radioctl:radioctl_splitter_key]

Keys the transmitter for `client` if no other client owns it. It takes the arbiter claim (`try_key`) first; a different current owner gives `Refused` and counts a `ptt_conflicts`, with no traffic to the rig. Then it sends `Op::SetPtt(true)` through the rig handle with source `Splitter`; the rig applies the transmit gate, the permit, the transmit frequency and band checks and the lock. If the rig refuses or fails the claim is released again, so a closed gate or down link leaves no phantom owner. On success the client stays owner until it unkeys, disconnects, or the rig is unkeyed by anyone (the watchdog, another client). Returns `Done` or `Refused`.

### `async fn run_client( inner: Arc<Inner>, id: ClientId, mut personality: Box<dyn Personality>, mut input: mpsc::Receiver<ClientInput>, output: mpsc::Sender<Vec<u8>>, persistent: bool, )` [@ANCHOR: radioctl:radioctl_splitter_run_client]

The task that serves one client until it ends. It subscribes to the rig's event stream and loops on `select`. On `Bytes` it asks the personality to parse them (with the time since the splitter started), sends any bus echo, then for each parsed item either ignores it, answers locally, or runs `execute` and has the personality render the reply; a closed output channel ends the client. On `Disconnected` it calls `cleanup` (unkeying if this client held the transmitter) and resets the personality, then ends unless `persistent`. A closed input channel or event stream ends it. For rig events it sends the personality's unsolicited bytes, but only for changes caused by the radio itself (`Transceive`) or changes that replace a known earlier value, so a first sighting is not announced. After the loop it runs `cleanup` once more.

## `crates/radioctl-splitter/src/ts2000_personality.rs`

### `pub fn new() -> Ts2000Personality` [@ANCHOR: radioctl:radioctl_splitter_ts2000_new]

Creates the Kenwood TS-2000 CAT personality for one client. Its mode table comes from `RigDescription::ts2000()` (a digit per mode), the tokenizer starts empty, and auto-information is off, so no unsolicited `FA`, `FB` or `MD` is sent until the client sends `AI2;`. `Default` calls it. Cannot fail.

### `fn status_answer(&self, status: &Status) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_splitter_ts2000_status_answer]

Formats the 38-byte answer to `IF;` from a `Status` (borrowed): `IF`, the receive VFO frequency as 11 digits of hertz, the step `0000`, the RIT or XIT offset as a sign and 5 digits (absolute value of an `i16`), RIT and XIT flags as 0 or 1, memory bank and channel `000`, transmit 1 or receive 0, the TS-2000 mode digit, the receive VFO (0 for A, 1 for B), scan `0`, split 0 or 1, and tone fields `0000`, ending in `;`. The layout follows the TS-2000 instruction manual and was cross-checked against Hamlib; the step, memory and tone fields are always fixed fillers. Returns `None` if the mode has no TS-2000 digit, in which case the caller answers `?;`. Pure.

### `fn classify(&mut self, text: &str) -> Action` [@ANCHOR: radioctl:radioctl_splitter_ts2000_classify]

Turns one command, without its `;`, into an `Action`. `FA`/`FB` alone read a VFO, with 11 digits set it; `MD` alone reads the mode and with one digit sets it (an unknown digit is refused); `TX` (with nothing, `0` or `1`) is `SetPtt(true)` and `RX` is `SetPtt(false)`; `SM` or `SM0` reads the S-meter; `IF` reads the composite status; `ID` is answered locally as `ID019;`, `PS` as `PS1;`; `AI` alone reports the flag and `AI0` or `AI2` set it for this client only (no reply). Anything else, bad digits or wrong lengths give a local `?;`. It mutates only the auto-information flag. A `TX` here only asks; the splitter arbiter and the rig gate decide whether the radio keys.

### `fn parse(&mut self, bytes: &[u8], now: Duration) -> Vec<Pending>` [@ANCHOR: radioctl:radioctl_splitter_ts2000_parse]

The `Personality::parse` for TS-2000 CAT. `bytes` from one client are fed to the tokenizer, which splits on `;` and drops stale partial commands using `now`. Each complete command goes to `classify`; a stretch of junk bytes becomes a local `?;` answer. There is no echo on CAT, so every `Pending` has an empty echo. Returns one `Pending` per token in order, empty when only a partial command has arrived. Mutates the tokenizer; no I/O.

### `fn reply(&mut self, _context: &[u8], request: &Request, outcome: &Outcome) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_splitter_ts2000_reply]

Renders an outcome as TS-2000 bytes. `Refused` is `?;`. `Done` is empty, because the TS-2000 gives no reply to a setting. A frequency is `FA` or `FB` plus 11 digits and `;`; a mode is `MD` plus the mode digit (`?;` if the mode has none); a signal level is `SM0` plus 4 digits; a composite status for `ReadStatus` is built by `status_answer`. Any other combination (a value that does not match the request) is `?;`. `_context` is unused (the TS-2000 needs none). Pure.

### `fn unsolicited(&mut self, change: &Change) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_splitter_ts2000_unsolicited]

Produces the auto-information bytes for a rig change. Only when this client has set `AI2` does it send anything: a frequency change of either VFO becomes `FA` or `FB` plus 11 digits and `;`, and a mode change becomes `MD` and the digit (nothing if the mode has no digit). Every other field, and every change while auto-information is off (the default), gives an empty vector. `change` is borrowed. Pure; the per-client flag is the only state read.

### `fn reset(&mut self)` [@ANCHOR: radioctl:radioctl_splitter_ts2000_reset]

Returns the personality to its connection-start state when the client disconnects: a fresh tokenizer, so a half-received command is forgotten, and auto-information off, so the next program on a persistent pty does not inherit the previous program's `AI2`. No arguments or result, cannot fail.

## `crates/radioctl-core/src/rig.rs`

### `pub async fn execute_from(&self, op: Op, timeout: Duration, source: Source) -> Reply` [@ANCHOR: radioctl:radioctl_core_handle_execute_from]

Runs one command on the rig and records `source` (`Api`, `Splitter`, and so on) on the events it causes; the protocol splitter passes `Source::Splitter`. `op` is owned, `timeout` is the deadline counted from submission (the actor enforces it). It first primes the transmit state: for a transmit-capable op with the gate open it reads, from cache or the rig, whatever the transmit frequency derivation lacks, ignoring errors. Then the op is submitted: unsupported capability gives `Unsupported`; a transmitting op with the gate closed gives `TransmitGateClosed`; a full queue gives `QueueFull`; a stopped actor gives `LinkDown`. The result is `Ok(Some(observation))` for reads, `Ok(None)` for sets, or the actor's error. Same queue, same gates and same priority as `execute_within`; an unkey takes the emergency path and needs no gate.

## `crates/radioctl-core/src/event.rs`

### `pub fn merge(&mut self, later: RigEvent) -> u64` [@ANCHOR: radioctl:radioctl_core_event_merge]

Folds a later event of the same rig into this one, so that two events become one with the net change. `later` is owned. Per field, the earliest `old` value is kept and the latest `new` value wins; fields only in `later` are appended; and the event's `source` becomes the later event's. It returns the number of intermediate values lost, one per field that appeared in both events. Within the radioctl crates checked here only its test calls it; whether another consumer uses the loss count is `unknown` (grep for `merge(` in the wider tree). The rig identifier is not checked or changed, so the caller must only merge events of one rig. Pure, no errors.

## `crates/radioctl-core/src/poll.rs`

### `pub fn for_capabilities( caps: &CapabilitySet, config: &PollConfig, now: Instant, ) -> PollSchedule` [@ANCHOR: radioctl:radioctl_core_poll_for_capabilities]

Builds the poll schedule for a rig. `caps` lists what the rig can read and `config` gives the intervals (defaults: fast fields 250 ms to 2 s, meters 1 s to 5 s, PTT reserve 2 s). It has one entry for each readable field among VFO A and B frequency, split, mode and transmit state (fast class) and signal and power (slow class), each starting at its minimum interval and first due at `now` (a tokio `Instant`). If `config.enabled` is false the schedule is empty and nothing is ever polled. Pure, no I/O.

### `pub fn next_due(&self) -> Option<Instant>` [@ANCHOR: radioctl:radioctl_core_poll_next_due]

Returns the earliest due time over all entries, which the actor sleeps until when it has nothing else to do, or `None` when the schedule is empty (polling disabled or no readable fields). Read-only; the time is in the past if an entry is already overdue.

### `pub fn take_starved(&self, field: Field, now: Instant, reserve: Duration) -> Option<PollEntry>` [@ANCHOR: radioctl:radioctl_core_poll_take_starved]

The reserved share for one field, used for the transmit state. If `field` has an entry that was last polled `reserve` or longer ago (or never, measured from schedule creation), it returns a copy of that entry even though its own interval may not be due yet; otherwise `None`. The actor runs it ahead of queued commands, so a client issuing back-to-back commands cannot stop the actor from noticing a radio that keyed or unkeyed on its own. It does not change the schedule; `report` does that after the poll.

### `pub fn take_due(&mut self, now: Instant, transmitting: bool) -> Option<PollEntry>` [@ANCHOR: radioctl:radioctl_core_poll_take_due]

Returns the poll entry to run now: the one with the earliest due time that is due at or before `now`, as a copy, or `None` if nothing is due. While the rig transmits (`transmitting`), only the transmit state and power are polled: if the chosen entry is any other field it is pushed back by its own interval and `None` is returned, which keeps the actor from sending reads that could disturb a keyed radio. The returned entry is not rescheduled; the caller reports the result with `report`.

### `pub fn report(&mut self, field: Field, changed: bool, failed: bool, now: Instant)` [@ANCHOR: radioctl:radioctl_core_poll_report]

Records the outcome of polling `field` and schedules its next poll. A failed poll, or any poll once transceive is active, sets the interval to the field's maximum; a changed value resets it to the minimum; an unchanged value doubles it, capped at the maximum. Then `due` becomes `now` plus the interval and the last-polled time becomes `now`, which also ends the field's starvation. An unknown field is ignored. Mutates the schedule only.

### `pub fn note_transceive(&mut self)` [@ANCHOR: radioctl:radioctl_core_poll_note_transceive]

Records that the rig reports changes on its own (transceive or auto-information). From then on `report` always sets the maximum interval, so polling slows to a safety net. The flag is never cleared for the life of the schedule. Idempotent, no arguments, cannot fail.

### `pub fn interval(&self, field: Field) -> Option<Duration>` [@ANCHOR: radioctl:radioctl_core_poll_interval]

Returns the current polling interval of `field`, or `None` if the field is not polled (not readable, or polling off). The value moves between the field's minimum and maximum as `report` is called. Read-only; the doc comment says it is for tests and diagnostics, and it has no effect on scheduling.

## `crates/radioctl-core/src/rig.rs`

### `pub fn new(granted_by: impl Into<String>) -> TransmitPermit` [@ANCHOR: radioctl:radioctl_core_transmit_permit_new]

Creates a transmit permit, the value that must exist before a rig may transmit; there is deliberately no boolean shortcut. `granted_by` (any string-like value, converted to an owned `String`) records who decided: an operator name, an attestation, or a test name, for the audit trail. The type is cloneable and comparable. Constructing a permit does not open the gate by itself; it is handed to the rig by `open_transmit_gate`. No validation is done, so the caller must pass a meaningful value. Cannot fail.

### `pub fn granted_by(&self) -> &str` [@ANCHOR: radioctl:radioctl_core_transmit_permit_granted_by]

Returns the text given as `granted_by` when the permit was created, borrowed from the permit. Used for logs, status output and tests to show who allowed transmitting on a rig. The permit is an audit record: the string is not validated or interpreted anywhere, so it is only as meaningful as what the granting code supplied. Read-only, never allocates, cannot fail.

### `pub fn spawn<E: ProtocolEngine, T: Transport>( runtime: &Handle, engine: E, transport: T, options: RigOptions, ) -> Result<RigHandle, RigError>` [@ANCHOR: radioctl:radioctl_core_rig_spawn]

Starts the rig actor for `engine` over `transport` on `runtime` (the runtime needs its time driver enabled) and returns a `RigHandle`. `engine` and `transport` are moved into the actor; `options` sets the queue size (default 64), default deadline (2 s), polling, subscriber capacities, link watch, the transmit time limit (default 180 s), the optional heartbeat and control lines. The shared state starts with the transmit gate closed, no permit, the link up and an empty cache; the poll schedule is built from the engine's capabilities. The actor ends when every handle is dropped. It returns `Ok` here in practice; no byte is sent to the radio by `spawn` itself, but the actor's start-up unkey pass runs once it starts. Queue and channel sizes are clamped to at least 1.

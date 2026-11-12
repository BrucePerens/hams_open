# radioctl function anchors, batch 46

Function anchors for the `radioctl-station` crate, part 3: the channel layer in `channel.rs`: the `ChannelDriver` trait defaults, the in-memory `VirtualBank` driver used by tests and the virtual station, the input `Debouncer`, and the `Switchboard`, which owns every channel, serialises operations on each one, confirms writes by read-back and publishes `StationEvent`s. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Transmit safety in this batch: the `Switchboard` operations (`set`, `pulse`, `force_off`) switch real lines through a driver, and a ptt channel is a transmit path. The switchboard performs the raw operation only; the interlock, the keyed-state check and the transmit gate are the `Station`'s job above it (`station.rs`), as the doc comment of `set` says. What this layer does enforce itself: a ptt channel may not sit on a `Mediated` driver (a system that can be down or stale), a power-on inside the channel's minimum off time is refused, a selector is break-before-make, and `force_off` (the emergency all-off) is never held back by a retry delay, a minimum time, a settle time or an operation in progress.

## Driver trait defaults (`ChannelDriver`)

### `fn assurance(&self) -> Assurance` [@ANCHOR: radioctl:radioctl_station_driver_assurance]

Default method of `ChannelDriver`: how far the station can rely on this driver to bring a device to its safe state, from least to most: `Mediated` (through another system such as Home Assistant), `Recorded` (acknowledged writes only, no trusted read-back), `HostSupervised` (direct, but nothing stops the device if the host dies) and `HardwareDeadMan` (the device releases itself when the host goes silent, as the FX2 firmware does). The default is `HostSupervised`; a driver overrides it to say otherwise. `Switchboard::add_channel` refuses a ptt channel on a `Mediated` driver. Pure, no arguments besides `self`, cannot fail.

### `fn safe_line(&self, _line: u32) -> bool` [@ANCHOR: radioctl:radioctl_station_driver_safe_line]

Default method: the level that means "safe" for `line` in the emergency all-off. The default is `false` (off) for every line; a board whose description says a line is safe when high overrides it. `_line` is the zero-based driver line and is unused by the default. Pure; no I/O; cannot fail. Read by `force_safe`.

### `fn force_safe(&self, line: u32) -> BoxFuture<'_, Result<(), StationError>>` [@ANCHOR: radioctl:radioctl_station_driver_force_safe]

Default method: drives `line` to its safe level for the emergency all-off. The default writes `safe_line(line)` with `write_line`, so a driver that has a retry delay or rate limit in its normal write path must override this to bypass it; the emergency path must never be held back. Returns the write's result (`Driver`, `Timeout` and so on on failure). Async; one write on the hardware. `Switchboard::force_off` calls it for each line of a channel.

### `fn recorded_line(&self, _line: u32) -> Option<Recorded>` [@ANCHOR: radioctl:radioctl_station_driver_recorded_line]

Default method: the driver's own record of a line, as `Recorded { value: Option<bool>, age: Option<Duration> }`: the level the device last acknowledged and how long ago. The default `None` means this driver keeps no record. A record is what was acknowledged, never a confirmation of the real state. `_line` is the zero-based line. Pure; cannot fail. Drivers with `Assurance::Recorded` override it and the board's `known` uses it.

## `VirtualBank` (the in-memory bank driver)

### `pub fn new() -> VirtualBank` [@ANCHOR: radioctl:radioctl_station_vbank_new]

A bank of virtual lines for tests and for the virtual station: every line starts off (an absent line reads as `false`), writes are applied and can be read back, no fault is injected. The bank is cheaply cloneable and shares its state through an `Arc<Mutex<..>>`, so a test keeps a handle while the switchboard holds another. No arguments, cannot fail, no I/O.

### `pub fn without_readback() -> VirtualBank` [@ANCHOR: radioctl:radioctl_station_vbank_no_readback]

A bank like `new` except that `read_line` always fails with `Driver` ("no read-back") and `can_read_back` is `false`. It stands for a relay board that cannot report its state, so tests can exercise the unconfirmed path in which `verify` records the confirmed value as unknown. No arguments; cannot fail; no I/O.

### `pub fn set_fault(&self, fault: Fault)` [@ANCHOR: radioctl:radioctl_station_vbank_set_fault]

Injects a failure mode for the writes and reads that follow: `Fault::None`, `FailWrites` (every write errors), `DropAfter(n)` (writes after the first `n` since the call error), `IgnoreWrites` (writes are logged but the line does not change), `FailReads` (reads error) and `WrongReadback` (reads return the opposite level). `fault` is stored by value and the count of writes done is reset to 0. `&self`: the state is behind the bank's mutex, so it may be called while another task holds a clone. Test-only behaviour; no I/O.

### `pub fn line(&self, line: u32) -> bool` [@ANCHOR: radioctl:radioctl_station_vbank_line]

The real (virtual) level of one line: what a meter on the relay would show, regardless of faults. `line` is the zero-based line number; a line never written reads `false`. Takes the bank's mutex briefly. Used by tests to compare what the station believes with what the bank holds.

### `pub fn set_physical(&self, line: u32, on: bool)` [@ANCHOR: radioctl:radioctl_station_vbank_set_physical]

Sets a line's real level behind the switchboard's back, as a sensor input changing or someone flipping a switch by hand would. It does not touch the write log and does not count as a write, so the station only learns of it by reading. `line` is zero-based; `on` the new level. Takes the bank's mutex briefly; cannot fail.

### `pub fn log(&self) -> Vec<(u32, bool)>` [@ANCHOR: radioctl:radioctl_station_vbank_log]

Every write the bank accepted or ignored since it was created, in order, as `(line, level)` pairs (a copy of the log). Writes that failed because of a fault are not in it. Tests use it to check the exact order of a break-before-make selector change or of an all-off. Cloning cost is proportional to the log length.

### `fn default() -> VirtualBank` [@ANCHOR: radioctl:radioctl_station_vbank_default]

`Default` for `VirtualBank`; the same as `VirtualBank::new()`: all lines off, read-back works, no fault. No arguments; cannot fail; no I/O; the new bank does not share state with any other bank. It exists so a `VirtualBank` can be created wherever a `Default` is expected.

### `fn write_line(&self, line: u32, on: bool) -> BoxFuture<'_, Result<(), StationError>>` [@ANCHOR: radioctl:radioctl_station_vbank_write_line]

The `ChannelDriver` write for the virtual bank. With `Fault::FailWrites` it fails; with `DropAfter(n)` it fails once `n` writes have been done; both give `Driver` ("virtual bank: write failed") and change nothing. Otherwise it counts the write, appends `(line, on)` to the log and, unless `Fault::IgnoreWrites` is set, stores the level. It completes immediately (no real await). Holds the bank's mutex only for the duration of the call.

### `fn read_line(&self, line: u32) -> BoxFuture<'_, Result<bool, StationError>>` [@ANCHOR: radioctl:radioctl_station_vbank_read_line]

The `ChannelDriver` read for the virtual bank. A bank made with `without_readback`, or `Fault::FailReads`, gives `Driver` ("virtual bank: no read-back"). Otherwise it returns the line's level (`false` if never written), inverted when `Fault::WrongReadback` is injected so a test can show the mismatch path. Takes the mutex briefly; no I/O.

### `fn can_read_back(&self) -> bool` [@ANCHOR: radioctl:radioctl_station_vbank_can_read_back]

Whether this bank can be read back: `true` for `new`, `false` for `without_readback`. The switchboard consults it before confirming a write: with `false` the confirmed state stays unknown instead of being treated as verified. Pure; takes the mutex briefly; cannot fail.

## `Debouncer` (input channels)

### `pub fn new(debounce: Duration) -> Debouncer` [@ANCHOR: radioctl:radioctl_station_debouncer_new]

Creates an input debouncer. `debounce` is the time a raw level must hold steady before it is believed (the channel's `debounce_ms`). Nothing is believed yet: `stable()` is `None`, and the first stable level arrives after `debounce` of unchanged samples. Pure; no clock of its own (times are passed to `sample`).

### `pub fn sample(&mut self, now: Duration, raw: bool) -> Option<bool>` [@ANCHOR: radioctl:radioctl_station_debouncer_sample]

Feeds one raw reading taken at `now`, a time since any fixed starting point (the caller supplies a monotonic duration, for example `Instant::elapsed`). If `raw` differs from the candidate the candidate and its start time are reset. When the candidate has held at least `debounce` and differs from the believed level, it becomes the believed level and is returned as `Some(level)`; otherwise `None`. A bounce shorter than the debounce time is therefore never reported, and a level is reported once per change, not once per sample. `now` going backwards saturates to zero elapsed. Mutates `self`; no I/O.

### `pub fn stable(&self) -> Option<bool>` [@ANCHOR: radioctl:radioctl_station_debouncer_stable]

The level the debouncer currently believes: `Some(true)` or `Some(false)` after the first stable change, `None` before any level has held for the debounce time or after the input was reset (the input task replaces the debouncer after a read error, so the state is unknown again). Read only; no side effect; cannot fail.

## `Switchboard`

### `fn default() -> Switchboard` [@ANCHOR: radioctl:radioctl_station_board_default]

`Default` for `Switchboard`; the same as `Switchboard::new()`: no channels and an event channel with capacity 256. No arguments; cannot fail; no I/O; allocates the event channel. Present so the switchboard can be built with struct defaults (for example inside a larger `Default` type).

### `pub fn new() -> Switchboard` [@ANCHOR: radioctl:radioctl_station_board_new]

An empty switchboard: no channels, and a `tokio::sync::broadcast` channel of capacity 256 for `StationEvent`s (a subscriber that falls more than 256 events behind loses the oldest and sees a lag error). Nothing is switched or opened. No arguments; cannot fail; no I/O. Channels are added with `add_channel` during set-up, then the board is shared (`Arc`).

### `pub fn add_channel(&mut self, def: ChannelDef, driver: Arc<dyn ChannelDriver>) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_board_add_channel]

Adds one channel to the board. `def` is taken by value and validated with `ChannelDef::validate` (`Config` on failure); `driver` is the shared driver object whose line `def.line` (or, for a selector, the run of lines from it) the channel uses. A channel id added twice gives `Config`. A ptt channel on a driver whose `assurance()` is `Mediated` gives `Config` ("nothing time-critical may" go through another system). On success the channel is stored with its own operation lock (`tokio::sync::Mutex`) and empty state. Nothing is written to the hardware. `&mut self`, so it is done before the board is shared.

### `pub fn assurance(&self, id: &ChannelId) -> Result<Assurance, StationError>` [@ANCHOR: radioctl:radioctl_station_board_assurance]

The assurance level of the driver behind channel `id`. Unknown channel gives `Unknown` ("no channel <id>"). Pure read; no I/O. It is part of the channel's status so a caller can tell a confirmed state from a weaker one.

### `pub fn recorded(&self, id: &ChannelId) -> Result<Option<Recorded>, StationError>` [@ANCHOR: radioctl:radioctl_station_board_recorded]

The driver's own record of the channel's line (`def.line`): the level the board last acknowledged and its age, or `None` when the driver keeps no record. It is recorded, not confirmed. `id` unknown gives `Unknown`. It looks at the first line only, which is right for binary channels; selectors use `status`. Pure read; no hardware access.

### `pub fn subscribe(&self) -> broadcast::Receiver<StationEvent>` [@ANCHOR: radioctl:radioctl_station_board_subscribe]

Returns a new receiver that sees every `StationEvent` (commanded, confirmed, mismatch, pulsed, input changed, driver fault, and the station's own) published after this call; earlier events are not replayed. The queue holds 256 events, and a receiver that is too slow gets a `Lagged` error from `recv`. Any number of receivers may exist. Cannot fail; no I/O.

### `pub(crate) fn emit(&self, event: StationEvent)` [@ANCHOR: radioctl:radioctl_station_board_emit]

Publishes `event` to every subscriber. The send result is ignored, so having no subscribers is not an error and an event with nobody listening is simply dropped. Crate-private; the board and the `Station` use it. Does not block, takes no lock.

### `pub fn def(&self, id: &ChannelId) -> Result<&ChannelDef, StationError>` [@ANCHOR: radioctl:radioctl_station_board_def]

The definition of channel `id` (kind, driver name, line, safe state, times), borrowed from the board. `Unknown` ("no channel <id>") when there is none. Pure; no I/O; cannot fail otherwise. The borrow lives as long as the board, so the caller cannot keep it past a change of the channel set (which is fixed once the board is shared).

### `pub fn ids(&self) -> Vec<ChannelId>` [@ANCHOR: radioctl:radioctl_station_board_ids]

Every channel id on the board, sorted by id so the order is stable (the underlying map is a `HashMap`). A new vector; empty for an empty board. Pure; no I/O; cannot fail.

### `pub fn status(&self, id: &ChannelId) -> Result<ChannelStatus, StationError>` [@ANCHOR: radioctl:radioctl_station_board_status]

A snapshot of one channel as `ChannelStatus { commanded, confirmed, can_confirm, recorded, assurance }`: the value last commanded, the value last confirmed by read-back (both `Option<Value>`, `None` unknown), whether the driver can read back, the driver's record (only filled for a `Recorded` driver, computed by `record_value`) and the assurance. `Unknown` for an unknown channel. Takes the channel's state lock only for the copy; does not wait for an operation in progress and does not touch the hardware.

### `pub fn known(&self, id: &ChannelId) -> Result<Option<Value>, StationError>` [@ANCHOR: radioctl:radioctl_station_board_known]

The state the station may act on: the confirmed value, or, for a channel on a `Recorded` driver (a relay board that cannot report its state, allowed by Bruce's decision of 2026-11-02), the driver's record of acknowledged writes. `None` means unknown. The caveat that a recorded value is not a confirmation stays visible in `ChannelStatus::recorded` and `assurance`. Errors as `status`. No hardware access.

### `fn record_value(&self, c: &Chan) -> Option<Value>` [@ANCHOR: radioctl:radioctl_station_board_record_value]

Builds a channel value from the driver's recorded lines. A binary channel gives `On` or `Off` from its one line. A selector reads each of its lines in order: none closed is `Off`, exactly one closed way n (1-based) is `Port(n)`, more than one closed is `None`. `None` is also returned when any line has no record or an unknown value. Private; pure; no I/O; used by `status`.

### `fn store(&self, c: &Chan, commanded: Option<Option<Value>>, confirmed: Option<Option<Value>>)` [@ANCHOR: radioctl:radioctl_station_board_store]

Records the commanded and/or the confirmed state of a channel under its state lock. The outer `Option` says whether to change that field; the inner is the new value (`None` unknown). The time of the last switch-off, used for the minimum off time, is stamped only on an edge from a known non-off value to `Off`, whether observed or commanded. A channel first seen off (the start-up read-back, or the first read after a driver fault) has no known turn-off time, because the minimum off time protects a device that was just running and must not delay a power-on after a restart of the software. Private; takes only the channel's std mutex briefly (never across an await).

### `async fn read_value(&self, c: &Chan) -> Result<Option<Value>, StationError>` [@ANCHOR: radioctl:radioctl_station_board_read_value]

Reads the real state of a channel from its driver: a binary channel is `On` or `Off` from one `read_line`; a selector reads every way in order and gives `Off` (none closed), `Port(n)` (one closed) or `Ok(None)` (ambiguous, more than one way closed). A driver error is returned as it is. Private; it does hardware reads, one after another, and does not record the result (callers do).

### `pub async fn read_back(&self, id: &ChannelId) -> Result<Option<Value>, StationError>` [@ANCHOR: radioctl:radioctl_station_board_read_back]

Reads a channel's real state from its driver and records it as the confirmed state, leaving the commanded state alone. It writes nothing, so the station uses it at start-up to learn the world without switching anything. Takes the channel's operation lock first, so it waits for a `set` or `pulse` in progress. On a driver error it records the confirmed state as unknown, emits a `DriverFault` event and returns the error; an unknown channel gives `Unknown`. Returns the value read (`None` for an ambiguous selector).

### `async fn verify(&self, c: &Chan, want: Value) -> Result<SetOutcome, StationError>` [@ANCHOR: radioctl:radioctl_station_board_verify]

Checks a write after it was made. A driver that cannot read back records the confirmed state as unknown and returns `SetOutcome { commanded: want, confirmed: None }`: the write is accepted but not confirmed. Otherwise it reads the channel (a read error is recorded as a fault and returned), stores the result, and if it equals `want` emits `Confirmed` and returns the outcome; if it differs it emits `Mismatch` and returns `NotConfirmed` ("channel <id>: commanded ..., read back ..."). Private; called with the channel's operation lock held.

### `pub(crate) async fn set(&self, id: &ChannelId, value: Value) -> Result<SetOutcome, StationError>` [@ANCHOR: radioctl:radioctl_station_board_set]

The raw operation that sets a power, output, operate or selector channel and confirms it by read-back. `id` is the channel; `value` is `On`, `Off` or, for a selector, `Port(n)` with n from 1 to its number of ways (a selector also accepts `Off`). A value the channel kind does not accept gives `Config`. A power-on inside the channel's `min_off_ms` of the last switch-off is refused (`Refused`). Setting a selector that is already confirmed on that port does nothing. Otherwise it records and announces the command, then writes. A selector is break-before-make: every other way is opened first, (where read-back exists) each is seen open, the `settle_ms` time passes, and only then the new way closes; if another way stays closed, nothing is closed and `NotConfirmed` is returned with a `Mismatch` event. After the write `verify` runs. Returns `SetOutcome { commanded, confirmed }`. Holds the channel's operation lock throughout (so operations on one channel never interleave) and awaits hardware. Interlocks, keyed-state checks and the transmit gate are the station's job; this function does not apply them.

### `pub(crate) async fn pulse(&self, id: &ChannelId) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_board_pulse]

Pulses a momentary channel (or an amplifier's tune channel): the line goes on, stays on for the channel's `pulse_ms`, and goes off. Any other kind gives `Config` ("not momentary"). It emits `Commanded` and, at the end, `Pulsed`. If the first off-write fails it is retried once; if the retry fails, that error is returned, and the line may be left on, which is why the driver is faulted (the write path records a `DriverFault`). Afterwards the commanded state is recorded `Off`; a pulse has no read-back. Holds the channel's operation lock for the whole pulse and sleeps for its width, so the Tokio runtime is needed. The off-write after the sleep is not guarded against the future being dropped mid-wait: cancelling the task during the pulse leaves the line on until something else (the all-off, a driver safe state) turns it off.

### `pub(crate) async fn force_off(&self, id: &ChannelId) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_board_force_off]

The emergency action for one channel. For power, output, selector and operate channels, every line of the channel is driven to its driver's safe level (`force_safe`, which is off unless the board says otherwise), at once, without taking the channel's operation lock (so it does not wait for an operation in progress), without the minimum off time and without a settle time. Other kinds (ptt, momentary, input, tune) are skipped with `Ok`. A failure of one line does not stop the others; the first error is returned after all lines were tried, and a `DriverFault` is emitted. On success the commanded state becomes `Off`, the confirmed state is cleared and, if the driver can read back, re-read. It never checks the keyed state: an emergency stop is never blocked. Async; hardware writes.

### `pub fn spawn_input(self: &Arc<Self>, runtime: &Handle, id: &ChannelId, interval: Duration) -> Result<JoinHandle<()>, StationError>` [@ANCHOR: radioctl:radioctl_station_board_spawn_input]

Starts a background task on `runtime` that watches an input channel (a sensor such as a fault line). Every `interval` it reads the line, feeds a `Debouncer` built from the channel's `debounce_ms`, and when the stable level changes records it as the confirmed state and emits `InputChanged { channel, high }`. A failed read resets the debouncer, marks the input unknown (fail closed) and emits a `DriverFault`; the task keeps polling. `id` must be an input channel (else `Config`) and known (else `Unknown`). Returns the `JoinHandle`; abort it to stop the task. It runs for as long as it is not aborted, holds an `Arc` of the board, and reads the hardware once per interval without taking the operation lock.

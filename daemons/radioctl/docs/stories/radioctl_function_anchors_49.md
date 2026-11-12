# radioctl function anchors, batch 49

Function anchors for the `radioctl-station` crate, part 6: `manager.rs`, the radio manager. It holds two `RadioLink` implementations (`RigLinks`, which watches attached rig handles, and `VirtualRadioLink`, an in-memory radio model for tests and the virtual station) and the `RadioManager`, which keeps one lifecycle state machine and one persisted record per radio, polls evidence (the power channel, the link and presence), reconnects with backoff, offers the power button (`press`) and reconciles with the real switches at start-up.

Transmit safety in this batch: nothing here keys a transmitter or opens a transmit gate, and the manager sends nothing to a radio. It can switch power, but only through the `Station` (`power_on`, `power_off`, `guard_action`), which applies the interlock and the dependency rules; the manager adds none of its own. `reconcile` only reads the power channels and never writes them. The `VirtualRadioLink` only reads a virtual bank and models a radio in memory.

## `crates/radioctl-station/src/manager.rs`

### `pub fn new() -> RigLinks` [@ANCHOR: radioctl:radioctl_station_riglinks_new]

Creates a `RigLinks` with no rigs attached; it is the `Default` value. No arguments, cannot fail, no locking and no I/O. Until a handle is attached with `set`, every radio is reported as down by `link_up` and `connect` fails.

### `pub fn set(&self, radio: DeviceId, handle: RigHandle)` [@ANCHOR: radioctl:radioctl_station_riglinks_set]

Attaches the rig handle of a radio, or replaces it after a reconnect. `radio` and `handle` are owned and stored in a map under the internal mutex, which is taken briefly (`&self`, so it may be called from any task while the manager polls). Returns nothing and cannot fail; no I/O. The old handle, if any, is dropped.

### `fn link_up(&self, radio: &DeviceId) -> BoxFuture<'_, bool>` [@ANCHOR: radioctl:radioctl_station_riglinks_link_up]

`RadioLink` method: returns the attached handle's own `link_up()`, and false for a radio with no handle. `radio` is borrowed. The value is read under the mutex immediately and the returned future just yields it, so there is no I/O and no failure.

### `fn present(&self, _radio: &DeviceId) -> BoxFuture<'_, Option<bool>>` [@ANCHOR: radioctl:radioctl_station_riglinks_present]

`RadioLink` method for presence evidence such as hotplug or a ping. A bare rig handle has none, so it always resolves to `None` (no opinion). `_radio` is ignored. No locking, no I/O, cannot fail.

### `fn connect(&self, radio: &DeviceId) -> BoxFuture<'_, Result<Option<KnownState>, StationError>>` [@ANCHOR: radioctl:radioctl_station_riglinks_connect]

`RadioLink` method: succeeds only if the attached handle's link is up, with `Ok(None)` (no state to report). Otherwise, including when no handle is attached, it fails with a `Driver` error "link is down"; the caller must respawn the actor and attach the new handle with `set`. `radio` is borrowed. The link state is sampled under the mutex at call time. No I/O, nothing is sent to the radio.

### `pub fn new(bank: VirtualBank) -> VirtualRadioLink` [@ANCHOR: radioctl:radioctl_station_vlink_new]

Creates a virtual radio link over `bank`, the (owned, cheaply cloneable) in-memory bank whose lines power the radios. It starts with no radios; add them with `add`. State is kept behind an `Arc<Mutex<..>>`. Cannot fail, no I/O.

### `pub fn add(&self, radio: DeviceId, line: u32, boot: Option<Duration>)` [@ANCHOR: radioctl:radioctl_station_vlink_add]

Adds, or replaces, a virtual radio. `radio` is the owned id; `line` is the zero-based bank line that powers it; `boot` is how long after power-on the radio takes to boot, where `None` means it never boots. The radio starts unpowered: not connected, not dropped, not gone, zero connect attempts and no reported state. Takes the mutex briefly; cannot fail; no I/O.

### `pub fn set_boot(&self, radio: &DeviceId, boot: Option<Duration>)` [@ANCHOR: radioctl:radioctl_station_vlink_set_boot]

Changes how long the virtual radio takes to boot after power-on (`None`: never). `radio` is borrowed; an unknown radio is ignored silently. It takes effect on the next evaluation of the boot clock, which keeps its start time. Takes the mutex briefly; cannot fail; no I/O. Test-only.

### `pub fn set_report(&self, radio: &DeviceId, state: KnownState)` [@ANCHOR: radioctl:radioctl_station_vlink_set_report]

Sets the state the virtual radio reports when it is connected: `state` is owned and handed out by `connect`. `radio` is borrowed; an unknown radio is ignored silently. Takes the mutex briefly; cannot fail; no I/O. Test-only.

### `pub fn drop_link(&self, radio: &DeviceId)` [@ANCHOR: radioctl:radioctl_station_vlink_drop_link]

Injects a fault: the link of the virtual radio drops (it becomes not connected) and cannot be re-established until the radio is power-cycled or `heal` is called. The device stays enumerated, so `present` still says true while powered and booted. `radio` is borrowed; an unknown radio is ignored. Takes the mutex briefly; cannot fail; no I/O. Test-only.

### `pub fn heal(&self, radio: &DeviceId)` [@ANCHOR: radioctl:radioctl_station_vlink_heal]

Clears a dropped link on the virtual radio so that the next `connect` can succeed. It does not reconnect by itself. `radio` is borrowed; an unknown radio is ignored. Takes the mutex briefly; cannot fail; no I/O. Test-only.

### `pub fn connects(&self, radio: &DeviceId) -> u32` [@ANCHOR: radioctl:radioctl_station_vlink_connects]

Returns how many connect attempts were made on the virtual radio, counting failed ones. `radio` is borrowed; an unknown radio gives 0. The counter is not reset by power cycling. Takes the mutex briefly; cannot fail; no I/O. Tests use it to check backoff.

### `fn refresh(&self, r: &mut VRadio) -> bool` [@ANCHOR: radioctl:radioctl_station_vlink_refresh]

Updates a virtual radio from its power line and returns whether it has booted. `r` is mutated and the caller holds the mutex. If the bank line is off, everything resets (the boot clock, connected and dropped) and it returns false. On the first moment the line is on, the boot clock starts. It returns true when a boot time is set and has elapsed since power-on and the radio is not marked gone. Reads the bank's line and the monotonic clock; cannot fail.

### `fn link_up(&self, radio: &DeviceId) -> BoxFuture<'_, bool>` [@ANCHOR: radioctl:radioctl_station_vlink_link_up]

`RadioLink` method for the virtual radio: up when it has booted, is connected and has not been dropped. An unknown radio is down. `radio` is borrowed. It evaluates `refresh` first, so a powered-off radio is reset as a side effect. Computed immediately under the mutex; no I/O; cannot fail.

### `fn present(&self, radio: &DeviceId) -> BoxFuture<'_, Option<bool>>` [@ANCHOR: radioctl:radioctl_station_vlink_present]

`RadioLink` method: whether the virtual radio is enumerated, true once it has booted (a dropped link still is) and false before that or when unpowered; `None` for a radio that was never added. `radio` is borrowed. Runs `refresh` first, with its reset side effect when the line is off. No I/O, cannot fail.

### `fn connect(&self, radio: &DeviceId) -> BoxFuture<'_, Result<Option<KnownState>, StationError>>` [@ANCHOR: radioctl:radioctl_station_vlink_connect]

`RadioLink` method: counts the attempt, then connects if the radio has booted and its link is not dropped, returning the state set with `set_report` (a clone, or `None` when none was set). Otherwise it fails with a `Driver` error "no answer". An unknown radio gives the `Unknown` error "no radio X". `radio` is borrowed. Evaluated at call time under the mutex; no real I/O.

### `fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T>` [@ANCHOR: radioctl:radioctl_station_manager_lock]

Locks a standard mutex and ignores poisoning (the guarded data are plain values): after a panic in another holder it takes the inner guard instead of panicking. `m` is borrowed and the returned guard releases the lock on drop. It blocks while another holder has it, so the guard must not be held across an await. Cannot fail; no I/O.

### `pub fn new(station: Arc<Station>, link: Arc<dyn RadioLink>, store: RecordStore, clock: Arc<dyn Fn() -> u64 + Send + Sync>, times: LifecycleTimes) -> RadioManager` [@ANCHOR: radioctl:radioctl_station_manager_new]

Builds the manager. It loads the record file through `store.load()` (a bad file is set aside by the store, and its problems are kept for `problems`) and builds an entry for every radio device of the station definition: the loaded record, or a bare one with unknown make and model and a boot-time connect procedure. The record's power source is refreshed from the definition, and a record boot time overrides the default one. Every radio starts `Unreachable` with a lifecycle machine until `reconcile` reads the switches. `station`, `link` and `clock` (unix seconds) are shared, `store` and `times` owned. Nothing is switched; it reads the record file. Cannot fail.

### `pub fn problems(&self) -> Vec<String>` [@ANCHOR: radioctl:radioctl_station_manager_problems]

Returns a copy of the problems the record loader and later saves had to report, as text lines in the order they happened; empty when there were none. Takes the problems mutex briefly; cannot fail; no I/O.

### `pub fn ids(&self) -> Vec<DeviceId>` [@ANCHOR: radioctl:radioctl_station_manager_ids]

Returns the ids of all radios the manager knows, in sorted order (the map is ordered). The list is fixed after construction. Takes the internal mutex briefly; cannot fail; no I/O.

### `pub fn subscribe(&self) -> broadcast::Receiver<RadioEvent>` [@ANCHOR: radioctl:radioctl_station_manager_subscribe]

Returns a new receiver of every `RadioEvent` published from now on, the station-level event stream. The channel is a broadcast channel with a buffer of 256 events, so a slow receiver misses the oldest events (it sees a lag error on `recv`). Events published before the call are not replayed. Cannot fail; no I/O.

### `pub fn spawn_callback<F: Fn(RadioEvent) + Send + 'static>(&self, runtime: &Handle, callback: F) -> JoinHandle<()>` [@ANCHOR: radioctl:radioctl_station_manager_spawn_callback]

Calls `callback` for every event, from a task spawned on `runtime`, as a plain function for callers that do not want to handle a receiver. `callback` is owned and runs on the runtime's thread, so it must not block. The task skips over lagged events and ends when the manager is dropped (the channel closes). Returns the `JoinHandle`, which can be aborted. Subscribes at the call, so earlier events are not delivered. Cannot fail.

### `pub fn state(&self, radio: &DeviceId) -> Option<RadioState>` [@ANCHOR: radioctl:radioctl_station_manager_state]

Returns the lifecycle state of `radio` (borrowed), or `None` for a radio the manager does not have. `RadioState` is `Copy`-like plain data, so the value is a snapshot that may be stale a moment later. Takes the internal mutex briefly; never blocks on I/O; cannot fail.

### `pub fn record(&self, radio: &DeviceId) -> Option<RadioRecord>` [@ANCHOR: radioctl:radioctl_station_manager_record]

Returns a copy of the radio's persisted record, or `None` for an unknown radio. `radio` is borrowed. The copy is a snapshot; later changes are not reflected. Takes the internal mutex briefly; cannot fail; no I/O.

### `pub fn cached(&self, radio: &DeviceId) -> Option<Cached>` [@ANCHOR: radioctl:radioctl_station_manager_cached]

Returns the last-known state of the radio with its age, served whether the radio is on or off. It is computed from the record against the injected clock (unix seconds). `None` for an unknown radio or when the record has no state yet. `radio` is borrowed. Takes the internal mutex briefly; cannot fail; no I/O.

### `pub fn note_state(&self, radio: &DeviceId, mut state: KnownState, capabilities: Option<Vec<String>>) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_manager_note_state]

Records a state read from the live radio. `state` is owned and stamped with the current clock time (`seen_unix`) before it replaces the record's last state; `capabilities`, when given, replaces the record's capability list. Then it saves all records with `persist`. Errors: `Unknown` for a radio that is not managed, or the save error from `persist` (which is also kept in `problems`). Writes the record file; takes the internal mutex briefly (released before the save).

### `pub fn persist(&self) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_manager_persist]

Saves every radio's record through the record store. It copies the records under the mutex and writes outside it. A failure is returned and also appended to `problems`. No arguments. Writes a file, blocking; ordering between concurrent calls is not guaranteed, but each writes a complete snapshot.

### `fn now(&self) -> Duration` [@ANCHOR: radioctl:radioctl_station_manager_now]

Returns the time since the manager was built, from the monotonic clock. The lifecycle machines work in this relative time, independent of the unix clock passed to `new`. No arguments besides `&self`; cannot fail; no locking; no I/O.

### `fn power_evidence(&self, radio: &DeviceId) -> PowerEvidence` [@ANCHOR: radioctl:radioctl_station_manager_power_evidence]

Turns the switch channel's known state into evidence for the lifecycle: for a radio powered through a channel, `On` or `Off` from the station's `device_power` (the confirmed value, or the driver's record on a board that cannot read back), and `Unknown` when that is unknown or errors. Any other power source (always on, momentary, unknown radio) is `Unknown`. `radio` is borrowed. Reads state only; no hardware action; cannot fail.

### `async fn gather(&self, radio: &DeviceId) -> Evidence` [@ANCHOR: radioctl:radioctl_station_manager_gather]

Takes one snapshot of evidence for a radio: the power evidence, the link's `link_up` and the link's `present`, as an `Evidence` value. `radio` is borrowed. Async because it awaits the link; the link implementation may do I/O, while the power evidence comes from known state. Cannot fail.

### `fn apply(&self, radio: &DeviceId, t: crate::lifecycle::Transition, reported: Option<KnownState>) -> RadioEvent` [@ANCHOR: radioctl:radioctl_station_manager_apply]

Applies a lifecycle transition. Under the mutex it stores the new state and the time in the record. Entering `Ready` sets the last-seen time and clears the consecutive failures and the connect failure count. Entering `Degraded` or `Unreachable` from any state other than `Off` or `Unreachable` counts one failure (saturating). Then it saves the records (a save error is ignored here, but is kept in `problems`) and builds a `RadioEvent` with from, to, reason, the unix time and `reported` (the state the radio reported on connecting, if any). The event is broadcast (ignoring no subscribers) and returned. `t` and `reported` are owned. Writes the record file; cannot fail.

### `pub async fn poll(&self, radio: &DeviceId) -> Result<Option<RadioEvent>, StationError>` [@ANCHOR: radioctl:radioctl_station_manager_poll]

Gathers evidence and steps the radio's state machine once, without trying to reconnect. `radio` is borrowed. Returns `Some(event)` when the state changed (the event is also published and the record saved) and `None` otherwise. Error: `Unknown` for a radio that is not managed. Async; awaits the link; no hardware action.

### `pub async fn connect_step(&self, radio: &DeviceId) -> Option<KnownState>` [@ANCHOR: radioctl:radioctl_station_manager_connect_step]

One reconnect attempt, if one is due: the state must be `Booting`, `Degraded` or `Unreachable` and the backoff time reached, the power evidence must not be off, and the link must not already be up. An `Unreachable` radio is tried only when the link says the device is present. The link's `connect` is then called. On success, the failure count and the backoff are reset, and the state the radio reports (if any) is stamped with the time, stored in the record and returned. On failure the backoff grows (250 ms doubling up to 8 s) and `None` is returned. Returns `None` for an unknown radio or when nothing was due. Async; nothing is sent to the radio by the manager itself, though the link's connect may open a transport.

### `pub async fn tick(&self, radio: &DeviceId) -> Result<Option<RadioEvent>, StationError>` [@ANCHOR: radioctl:radioctl_station_manager_tick]

One monitor cycle for a radio: `connect_step` if due, then a fresh evidence snapshot and one step of the state machine. A state reported on connecting rides on the event that follows when the new state is `Ready`, and is dropped otherwise (it is still stored in the record by `connect_step`). `radio` is borrowed. Returns the published event, if the state changed. Error: `Unknown` for an unmanaged radio. Async; the same effects as `connect_step` plus a record save on a transition.

### `pub fn spawn_monitor(self: &Arc<Self>, runtime: &Handle, interval: Duration) -> JoinHandle<()>` [@ANCHOR: radioctl:radioctl_station_manager_spawn_monitor]

Starts a task on `runtime` that loops forever: tick every radio (in sorted order, ignoring the errors), then sleep `interval`. It holds its own `Arc` of the manager, so it keeps the manager alive until the returned `JoinHandle` is aborted; there is no other stop. `interval` is the pause between rounds. Cannot fail itself. Each tick may reconnect radios and save records.

### `fn power_source(&self, radio: &DeviceId) -> Result<PowerSource, StationError>` [@ANCHOR: radioctl:radioctl_station_manager_power_source]

Returns a clone of the device's power source for the power button, after checking it is a managed radio. Errors: `Unknown` when the id is not a device, is not a radio, or is not in the manager; `Refused` when the radio is always on ("has no power source, so no power button"). `radio` is borrowed. Pure reads; no hardware action.

### `async fn press_on(&self, radio: &DeviceId, source: &PowerSource) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_manager_press_on]

The on half of the button. It tells the lifecycle that power-on was commanded, then calls `Station::power_on`, which applies the interlock and the dependency rules. If that fails, one state step is made from fresh evidence and the error is returned. A momentary-powered radio is then assumed powered. It then ticks the radio, sleeping 250 ms between ticks, until the state is `Ready`, `Unreachable` or `Off`, or the limit passes: the boot time (the larger of the record's and the default) plus the boot grace plus the power-on timeout. Reaching the limit is not an error; the caller reads the final state. Errors: those of `power_on` and `tick`. Async; switches power through the station and sleeps.

### `fn step_state(&self, radio: &DeviceId, f: impl FnOnce(&mut Lifecycle, Duration) -> Option<crate::lifecycle::Transition>)` [@ANCHOR: radioctl:radioctl_station_manager_step_state]

Runs a command-driven lifecycle transition and publishes it. `f` is called once with the radio's lifecycle (mutably, under the mutex) and the manager time, and may return a transition, which is then applied (state stored, record saved, event broadcast). An unknown radio does nothing. Returns nothing and cannot fail; the save error is ignored but kept in `problems`.

### `pub async fn press(&self, radio: &DeviceId, button: Button) -> Result<ButtonReport, StationError>` [@ANCHOR: radioctl:radioctl_station_manager_press]

The power button of a radio that has a power source: `Button::On`, `Off` or `Reboot`, behind the interlocks and the dependency graph, which the station checks. Off and On pass every keyed state unless the radio's power channel is a `shared_supply`; a reboot is classified first and refused up front rather than cutting power and then refusing the power-on. Off powers the radio and its dependents off (`Station::power_off`) and re-polls. Reboot waits the channel's minimum off time before turning on. On a radio that is already up (booting, ready, degraded or powering on) it does nothing; on a powered but `Unreachable` radio it is refused ("use reboot"). Returns a `ButtonReport` with the final state and the devices powered off, in order. Errors: `Unknown`, `Refused` (no power source, interlock, dependencies, unreachable), and switchboard errors. Async; switches real power and sleeps. Settings are never restored.

### `pub async fn reconcile(&self) -> Vec<ReconcileItem>` [@ANCHOR: radioctl:radioctl_station_manager_reconcile]

The start-up reconcile, run once. For each radio, it reads the real state of the power channel back (a read, never a write), takes the link and presence, and resolves the state: off if the switch is off; ready if the link is up; booting if the switch is on without a link; unreachable if the switch is unknown without a link. It reports a disagreement when the record believed off but the switch is on, or believed up but the switch is off, or believed ready with the switch known and no link. The radio's lifecycle machine is rebuilt in the resolved state, and when anything changed a transition is applied (event, save). A switch that cannot be read leaves the radio `Unreachable` with its cached state kept. A power channel is never changed. Returns one `ReconcileItem` per radio. Async; reads hardware, writes the record file; cannot fail.

### `fn life_times(&self, e: &Entry) -> LifecycleTimes` [@ANCHOR: radioctl:radioctl_station_manager_life_times]

Returns the lifecycle timing for one radio: the manager's default `LifecycleTimes`, with the boot time replaced by the record's boot time when the record has one above 0 ms. `e` is borrowed. Pure, cannot fail, no I/O.

### `fn unknown(radio: &DeviceId) -> StationError` [@ANCHOR: radioctl:radioctl_station_manager_unknown]

Builds the error for a radio the station does not have: kind `Unknown`, message "no radio X" with the id. `radio` is borrowed. Pure, cannot fail, no I/O. Used by the manager and the virtual link wherever an id is not found.

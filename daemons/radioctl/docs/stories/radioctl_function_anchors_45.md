# radioctl function anchors, batch 45

Function anchors for the `radioctl-station` crate, part 2: the radio lifecycle state machine (`lifecycle.rs`), the station definition and its validation (`model.rs`) and the host side of the FX2 Station I/O board (`fx2.rs`). Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Transmit safety in this batch: the lifecycle machine and the definition checks are pure and key nothing. The `fx2.rs` functions are the ones that can key a transmitter: `Board::key` asks the firmware to key a PTT (push to talk) pin, and `Board::heartbeat` keeps the board's hardware time-out alive. They are sans-IO (a `ControlTransport` carries the USB transfers) and are reached only through the station's interlock; the board itself releases every output when the heartbeat lapses, on a USB reset or after the per-line maximum on-time, which is why `require_station_io` refuses any board that does not report those three safeguards.

## `crates/radioctl-station/src/lifecycle.rs`

### `fn default() -> LifecycleTimes` [@ANCHOR: radioctl:radioctl_station_times_default]

The default timing of the lifecycle machine (`Default for LifecycleTimes`): `boot_time` 20 s (the time a radio is expected to take to boot), `boot_grace` 10 s (extra allowance on top), `power_on_timeout` 10 s (how long a switch may take to confirm a power-on) and `degraded_timeout` 60 s (how long a radio may stay Degraded before it is declared Unreachable). Takes no arguments, returns the struct by value, cannot fail and has no side effects. The manager adapts them per radio (its `life_times` helper, which uses the radio's record boot time); the defaults apply where nothing else is known.

### `pub fn new(state: RadioState, now: Duration, times: LifecycleTimes) -> Lifecycle` [@ANCHOR: radioctl:radioctl_station_lifecycle_new]

Creates the state machine of one radio. `state` is where it starts (the manager starts every radio as `Unreachable` at time zero and again when it is re-resolved); `now` is the monotonic time at which it is in that state, as a `Duration` since an arbitrary epoch the caller keeps; `times` is the timing set. The machine remembers no hotplug presence yet (`prev_present = None`). Pure; the machine has no clock of its own, so all timing comes from the `now` values the caller passes and the machine is deterministic and easily tested.

### `pub fn state(&self) -> RadioState` [@ANCHOR: radioctl:radioctl_station_lifecycle_state]

The current state: `Off`, `PoweringOn`, `Booting`, `Ready`, `Degraded` or `Unreachable`. `RadioState` is `Copy`, so the value is returned by copy. Read only, no side effect, cannot fail, takes no lock (the machine is owned by the manager's entry for the radio and mutated only through `step` and the command methods). The manager publishes this value as the radio's lifecycle in its status and event stream.

### `pub fn since(&self) -> Duration` [@ANCHOR: radioctl:radioctl_station_lifecycle_since]

When the current state began, in the same monotonic time base the caller passed as `now`. The timeouts in `step` are measured from it. Returned by copy; read only; cannot fail. The manager stores it in the record as `lifecycle_unix` (converted to wall-clock seconds) when a transition is saved, so a restart can show when the radio last changed state.

### `fn goto(&mut self, to: RadioState, now: Duration, reason: &'static str) -> Option<Transition>` [@ANCHOR: radioctl:radioctl_station_lifecycle_goto]

The one place the state changes. If the machine is already in `to` it does nothing and returns `None` (so a repeated event produces no transition and no event for observers). Otherwise it records the new state and `since = now` and returns a `Transition { from, to, reason }`, where `reason` is a short static text ("link up", "switch is off") that the manager passes to the event stream and the log. Private; mutates `self`; no I/O.

### `pub fn power_on_commanded(&mut self, now: Duration) -> Option<Transition>` [@ANCHOR: radioctl:radioctl_station_lifecycle_power_on_commanded]

Tells the machine that a power-on was commanded (by the power button or a profile). From `Off` or `Unreachable` it moves to `PoweringOn` with the reason "power-on commanded" and returns the transition. From any other state (`PoweringOn`, `Booting`, `Ready`, `Degraded`) it returns `None` and leaves the state alone, so commanding power on for a radio that is already running does not disturb it. The caller sends the real switch command separately, through the station interlock; this call only updates the model. `now` is the monotonic time.

### `pub fn assume_powered(&mut self, now: Duration) -> Option<Transition>` [@ANCHOR: radioctl:radioctl_station_lifecycle_assume_powered]

For a radio powered by a momentary (pulsed) line, which cannot be read back: after the pulse, the radio is taken to be powered. From `PoweringOn` it moves to `Booting` ("power line pulsed") and returns the transition; from any other state it returns `None`. The boot timers then decide whether the radio came up (`Ready` on link, `Unreachable` after `boot_time + boot_grace`). Mutates `self`; no I/O.

### `pub fn power_off_commanded(&mut self, now: Duration) -> Option<Transition>` [@ANCHOR: radioctl:radioctl_station_lifecycle_power_off_commanded]

Tells the machine that a power-off was commanded and the switch is going off, so the radio is no longer expected to answer. It goes to `Off` ("power-off commanded") from any state, or returns `None` if already `Off`. The switch's confirmed state, fed to `step` as `PowerEvidence::Off`, then keeps the machine in `Off` instead of reporting the silent link as a failure. Power-off is allowed in every keyed state (it can only stop a transmission), so there is no refusal path here.

### `pub fn step(&mut self, now: Duration, ev: &Evidence) -> Option<Transition>` [@ANCHOR: radioctl:radioctl_station_lifecycle_step]

Feeds one snapshot of everything known about the radio and returns the transition it caused, at most one per call, so every intermediate state is visible to the event stream. `now` is the monotonic time; `ev` carries `power` (`On`, `Off` or `Unknown`, from the switch read-back), `link_up` (from the rig actor's link watch) and `present` (hotplug or ping evidence, `Some(true)`, `Some(false)` or `None`). A device that has been present all along is not "appearing"; only a change to present counts. Rules: confirmed `Off` power always goes to `Off`. In `Off`: power on with link up goes to `Ready`, power on without link to `Booting`, link up with unknown power to `Ready`. `PoweringOn`: power confirmed on goes to `Booting`; after `power_on_timeout` it goes to `Unreachable`. `Booting`: link up goes to `Ready`; after `boot_time + boot_grace` it goes to `Unreachable`. `Ready`: link down goes to `Degraded`, or to `Unreachable` if the device is gone. `Degraded`: link back goes to `Ready`; device gone to `Unreachable`; after `degraded_timeout` to `Unreachable`. `Unreachable`: link up goes to `Ready`; a device appearing goes to `Booting`. Mutates `self` and its remembered `prev_present`; no I/O; never fails. It only models state: it commands no hardware.

## `crates/radioctl-station/src/model.rs`

### `pub fn lines(&self) -> Vec<u32>` [@ANCHOR: radioctl:radioctl_station_channel_lines]

The driver lines a channel occupies, as a new vector. A selector with `ways` ways occupies `ways` consecutive lines starting at `line` (computed with saturating addition, so it cannot overflow); every other kind occupies exactly `line`. `StationDef::validate` uses it to ensure that no two channels share a line of the same driver. Pure.

### `pub fn validate(&self) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_channel_validate]

Checks one channel by itself and returns `StationErrorKind::Config` ("channel <id>: ...") on the first problem. The id and driver name must be non-empty. A power or ptt channel needs a defined safe state; a power channel's safe state is `on` or `off` and a ptt channel's is `off`; a selector's safe state is `off` or a port from 1 to its number of ways; a safe port needs a selector. Sizes and times: a selector has 1 to 64 ways, a momentary pulse is 1 to 10000 ms, a ptt channel has a non-zero maximum on-time, a tune pulse on an amplifier is 1 to 10000 ms, and an amplifier's operate channel must have `safe = "off"` (standby is its safe state). Pure; called by `StationDef::validate` for every channel.

### `pub fn validate(&self) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_meterdef_validate]

Checks the definition of one external power meter (`MeterDef`). `id`, `for_rig` and `port` must be non-empty; `driver` must be a name `MeterDriver::from_name` knows (`kpa500` or `kpa1500`); the optional `unit` must be the unit that driver reports; `baud`, if given, must be 300 to 921600; `offset_db_tenths` within plus or minus 1000 (100 dB); `stale_ms` 100 to 60000. Each failure gives `Config` ("power meter <id>: ..."). Pure.

### `pub fn offset_db(&self) -> f64` [@ANCHOR: radioctl:radioctl_station_meterdef_offset_db]

The calibration offset of the meter in decibels: `offset_db_tenths` (tenths of a dB as stored in the file) divided by 10. For example 35 gives 3.5 dB. It is added to a reading to correct for a coupler or attenuator. Pure, no failure.

### `pub fn from_toml(text: &str) -> Result<StationDef, StationError>` [@ANCHOR: radioctl:radioctl_station_def_from_toml]

Parses the owner's station file (channels, devices, band routes, power meters, profiles) from TOML and runs `StationDef::validate`. `text` is borrowed. A TOML error gives `Config` ("station: ..."), as does any validation failure. Pure; nothing is connected or switched, in keeping with Phase 1 where nothing is ever switched at start-up.

### `pub fn channel(&self, id: &ChannelId) -> Option<&ChannelDef>` [@ANCHOR: radioctl:radioctl_station_def_channel]

Finds a channel definition by id with a linear search; `None` for an unknown id (callers turn that into an `Unknown` error). `id` is borrowed and compared by equality, so the lookup is exact and case-sensitive. The result borrows from the definition and lives as long as it. Cannot fail, does no I/O, and the definition is immutable after `from_toml`, so it may be shared between tasks.

### `pub fn band_routes_for(&self, rig: &str) -> Vec<BandRoute>` [@ANCHOR: radioctl:radioctl_station_def_band_routes_for]

The band-to-selector routes that the rig called `rig` should drive. A configured `[[band_routes]]` entry is included when its channel exists, has not opted out with `band_follow = false` and either belongs to that rig or to no rig in particular. Each is converted to a `BandRoute` with `low` and `high` as `Hz`, the channel and the port, in file order (so `route_for` honours the order). Returns a new vector; empty when the rig has no routes. Pure.

### `pub fn device(&self, id: &DeviceId) -> Option<&DeviceDef>` [@ANCHOR: radioctl:radioctl_station_def_device]

Finds a device (radio, amplifier, tuner and so on) by id; `None` if unknown. `id` is borrowed and compared exactly (case-sensitive). The result is a borrow of the definition's `DeviceDef` (kind, power source, dependencies, antenna needs) and lives as long as it. Linear search over a short list; cannot fail; no I/O; no lock.

### `pub fn validate(&self) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_def_validate]

Checks the whole definition and returns `Config` with a message naming the problem; the first failure wins. It runs `ChannelDef::validate` on each channel and requires unique channel ids; a `shared_supply` power channel must power a radio (elsewhere it would protect nothing); no driver line may be used by two channels; each band route must name a selector, with `low_hz <= high_hz` and a port from 1 to its ways; power meters validate and each rig has at most one; device ids are unique; a device's power source must be an existing channel of the right type (a `Power` channel for `Channel`, `Momentary` channels for `Momentary`) and a channel powers at most one device; `needs_antenna` channels must be selectors; `depends_on` must name existing devices; an amplifier channel's device must be an amplifier or tuner, a device has at most one operate channel, and its `fault_input` must be an input channel; the dependency graph must have no cycle (via `power_on_order`); profile names are unique and their steps must reference real devices and valid selector ports. Pure; run once at load time, which is why later code can rely on the references.

### `pub fn power_on_order(&self) -> Result<Vec<DeviceId>, StationError>` [@ANCHOR: radioctl:radioctl_station_def_power_on_order]

Every device in dependency order: a device appears after all the devices it `depends_on`, and among devices that are ready at the same time the order is by id (the maps are ordered), so the result is deterministic. The power-off order is the reverse. A cycle gives `Config` ("the device dependencies form a cycle"). Returns a new vector; pure. Used by `validate` (the cycle check) and by the station when it sequences a power-up.

### `pub fn dependents(&self, id: &DeviceId) -> Vec<DeviceId>` [@ANCHOR: radioctl:radioctl_station_def_dependents]

Every device that depends on `id`, directly or through others, in power-off order (the most dependent first), excluding `id` itself. The station uses it to turn those devices off before it cuts power to `id`. Computed from `power_on_order`; if that fails (a cycle that `validate` would have refused) the result is empty. Returns a new vector; pure.

## `crates/radioctl-station/src/fx2.rs`

### `pub fn parse(b: &[u8]) -> Result<Info, StationError>` [@ANCHOR: radioctl:radioctl_station_fx2_info_parse]

Decodes the board's `GET_INFO` reply (little endian; byte 0 is the record length). `b` is the received bytes, at least 20 long with a length byte of at least 20 and not more than the buffer; otherwise `Driver` ("GET_INFO: record shorter than the fixed fields"). Fields: `protocol` (major, minor), `device_type` (u16), `firmware` (three bytes), `board_id`, `capabilities` (u32 bit set), `safe_state_kind`, `default_max_on_ms` and `default_heartbeat_ms` (u16 each). Bytes past those are ignored, so a newer firmware that appends fields still parses. Pure; accepts any board, which is why `require_station_io` follows.

### `pub fn require_station_io(&self) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_fx2_info_require_station_io]

Refuses a board that is not safe to key a transmitter with; every refusal is `StationErrorKind::Refused`. The device type must be `STATION_IO_TYPE` (0x0001; the bench instrument image has another, and is never used for the station), the protocol major must be `PROTOCOL_MAJOR` (0), and the capability bits must include the hardware transmit time-out (`CAP_HW_TIMEOUT`), the heartbeat (`CAP_HEARTBEAT`) and release on USB reset (`CAP_SAFE_ON_RESET`). The safe state kind must be 0 or 1 and the default maximum on-time and heartbeat time-out non-zero. Pure; called by `Board::open`.

### `pub fn parse(b: &[u8]) -> Result<Status, StationError>` [@ANCHOR: radioctl:radioctl_station_fx2_status_parse]

Decodes the board's status record, which must be exactly 23 bytes in the first byte's own count (`b.len() >= 23` and `b[0] == 23`), else `Driver`. Flags in byte 1: `map_valid` (bit 0), `heartbeat_alive` (bit 1) and `out_of_sync` (bit 2); `keyed_mask` (which PTT channels are keyed) and `active_mask`; `last_release_reason`, `release_count`, `map_error`, `key_refusal` and the `last_sequence` of the last heartbeat (little endian). Pure; no I/O.

### `pub fn pin_id(name: &str) -> Result<u8, StationError>` [@ANCHOR: radioctl:radioctl_station_fx2_pin_id]

Converts a pin name `PA0` to `PA7`, `PB0` to `PB7` or `PD0` to `PD7` into the firmware's pin id: port in the high nibble (A 0, B 1, D 2) and the bit number in the low nibble. Anything else (lowercase, `PC`, a bit above 7, a wrong length) gives `Config` naming the allowed pins. Pure.

### `pub fn map_steps(map: &GpioMap, heartbeat_lapse_ms: u16) -> Result<Vec<MapStep>, StationError>` [@ANCHOR: radioctl:radioctl_station_fx2_map_steps]

Translates an owner's `GpioMap` into the firmware's map requests, in order: `MAP_BEGIN`; for each line a `MAP_PIN` (pin id and function in `value`, flags in `index`: safe-state defined, and active-high when the safe level is low so the firmware can refuse a safe level equal to its active level), and for a ptt line also a `MAP_PARAM` with the maximum on-time; then `MAP_HEARTBEAT` with `heartbeat_lapse_ms`; then `MAP_COMMIT`. Functions supported are ptt, keyline and power; sensors and spares are left alone (inputs are not configured); any other function is refused with `Config` ("the Station I/O firmware has no X function"), as is a line with no safe state, a ptt line without `max_on_ms` from 1 to 65535, an invalid pin, or `heartbeat_lapse_ms == 0`. Pure; it only builds the request list, so nothing is sent until `Board::load_map`.

### `pub fn open(mut t: T) -> Result<Board<T>, StationError>` [@ANCHOR: radioctl:radioctl_station_fx2_board_open]

Opens a Station I/O board through a control transport `t` (taken by value; the board owns it). It issues `GET_INFO` (reading up to 64 bytes), parses it and applies `require_station_io`; the board is refused (`Refused`) or the transfer fails (`Driver`) before any command is sent. The heartbeat sequence starts at 0. Nothing is switched. Blocking in the sense of the transport's own transfer, which in production goes through the broker's device access.

### `pub fn status(&mut self) -> Result<Status, StationError>` [@ANCHOR: radioctl:radioctl_station_fx2_board_status]

Reads and parses the board's status record with `GET_STATUS`. Errors are the transport's or `Status::parse`'s (`Driver`). Needs `&mut self` because the transport does. No state change on the board.

### `pub fn load_map(&mut self, map: &GpioMap, heartbeat_lapse_ms: u16) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_fx2_board_load_map]

Sends the whole map (`map_steps`) and commits it. If a step is rejected (the transport fails or the board stalls), it reads the status and returns `Driver` with the failing request code and the firmware's `map_error`; per the firmware the board has released everything, so a half-loaded map keys nothing. An invalid map fails before anything is sent. A successful commit makes the map valid, after which keying needs a live heartbeat.

### `pub fn heartbeat(&mut self, commanded: u8) -> Result<Status, StationError>` [@ANCHOR: radioctl:radioctl_station_fx2_board_heartbeat]

Sends one heartbeat, which is the host's proof of life: if the board does not receive one within the lapse set by the map it releases every output. `commanded` is a bit mask of the PTT channels the host believes are keyed. The sequence number is incremented first (wrapping) and sent; the answer must echo it (`NotConfirmed` otherwise) and the board's `keyed_mask` must equal `commanded` with `heartbeat_alive` true and `out_of_sync` false, or the call is a `NotConfirmed` error that carries the release reason; by then the board has released everything. Returns the parsed status. It must be called periodically by the owner of the board, well within the lapse; a missed call keys nothing, it releases. Needs `&mut self`.

### `pub fn key(&mut self, pin: &str, down: bool) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_fx2_board_key]

Keys (`down = true`) or releases a PTT line by pin name (`pin_id` rules, so a bad name is `Config`). The firmware refuses unless a map is committed and the heartbeat is live; the refusal is returned as `Refused` with the firmware's `key_refusal` code, read from the status. Success means the control transfer was accepted, not that the radio is transmitting: the next `heartbeat` confirms the keyed mask. This is the call that can put a transmitter on the air, so callers must reach it only through the station's interlock and transmit gate; the board's own maximum on-time releases the line if the host stops.

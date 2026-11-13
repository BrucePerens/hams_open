# radioctl function anchors, batch 48

Function anchors for the `radioctl-station` crate, part 5: `station.rs`, the `Station`, which sits above the `Switchboard` and adds the rules of a real station: the keyed-state interlock, the power dependencies of devices, amplifier and tuner preconditions, power profiles and the emergency all-off. It also holds the `KeyedSource` implementation for a `RigHandle`.

Transmit safety in this batch: this is the layer that decides whether switching hardware is allowed. Nothing here keys a transmitter or opens the transmit gate. Anything that could hot-switch an RF (radio frequency) path (an antenna selector, an amplifier going to operate, a non-radio power-on, an output) is `Action::RfPath` and goes through `guard_action`, which refuses while any keyed source reports keyed (`Refused`) or unknown (`KeyedUnknown`); it fails closed. Powering a device off, or powering on a radio whose power channel is not a shared supply, passes whatever the keyed state, because those can only stop or cannot cause a transmission. `all_off` goes straight to `Switchboard::force_off` with no interlock, no delays and a short per-channel timeout. The keyed state comes from `KeyedSource`s that the caller supplies.

## `crates/radioctl-station/src/station.rs`

### `fn name(&self) -> String` [@ANCHOR: radioctl:radioctl_station_rig_name]

`KeyedSource` method for a `RigHandle`: returns the rig's id as an owned `String` (`self.id().to_string()`). The interlock uses it in refusal messages and events to say which radio is keyed or unknown. No arguments besides `&self`, cannot fail, no locking, no I/O.

### `fn key_state(&self) -> BoxFuture<'_, KeyState>` [@ANCHOR: radioctl:radioctl_station_rig_key_state]

`KeyedSource` method for a `RigHandle`: the radio's keyed state for the interlock. If the rig's link is down it returns `Unknown`. If the rig has the `ReadPtt` capability it asks the actor for PTT (push to talk) with a freshness of at most one second: a true value gives `Keyed`, false gives `Unkeyed`, and a read error gives `Unknown`. Without that capability it uses the cached PTT observation (`Keyed`, `Unkeyed`, or `Unknown` when there is none). Async; may send a PTT read to the radio. It never opens the transmit gate or transmits, and it never fails, returning `Unknown` instead.

### `pub fn new(def: StationDef, board: Arc<Switchboard>, keyed: Vec<Arc<dyn KeyedSource>>, ready: Option<Arc<dyn Readiness>>) -> Result<Station, StationError>` [@ANCHOR: radioctl:radioctl_station_station_new]

Builds a station. `def` is the owned station definition; `board` is a shared switchboard that must have every channel of `def`; `keyed` is the list of keyed-state sources the interlock consults (owned); `ready` is an optional source that says whether a device has answered after power-on. It validates `def` (errors from `validate`), checks that every channel of `def` exists on the board, and refuses an amplifier powered through a `Mediated` channel (one that goes through another system such as Home Assistant) unless the device declares `external_deadman`, with a `Config` error. Nothing is switched, no I/O, not async.

### `pub fn power_assurance(&self, device: &DeviceId) -> Option<Assurance>` [@ANCHOR: radioctl:radioctl_station_station_power_assurance]

Returns the assurance level of the channel that powers `device` (borrowed id), as reported by the switchboard. Returns `None` when the device is unknown, is always on, is powered by a momentary button, or when the board has no such channel (the board error is dropped). Pure read of the definition and board; no I/O, cannot fail, takes no hardware action. It tells a caller how far to trust `device_power`.

### `pub fn board(&self) -> &Arc<Switchboard>` [@ANCHOR: radioctl:radioctl_station_station_board]

Returns a borrowed reference to the station's switchboard, for status and events. The doc says switching goes through the station, so that the interlock and the dependency rules apply; a caller that switches through the board directly bypasses them. No arguments, cannot fail, no locking, no I/O.

### `pub fn def(&self) -> &StationDef` [@ANCHOR: radioctl:radioctl_station_station_def]

Returns a borrowed reference to the station definition the station was built with: devices, channels and profiles. It never changes after construction. No arguments, cannot fail, no locking, no I/O, no side effects.

### `pub async fn guard(&self, what: &str) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_station_guard]

The interlock for an RF-path command: it is `guard_action(Action::RfPath, what)`. `what` is a borrowed description (for example `select ant 2`) used in the refusal text and event. Returns `Ok(())` only when every keyed source reports unkeyed, otherwise the error of `guard_action` (`Refused` when keyed, `KeyedUnknown` when unknown). A refusal is also published as a `Refused` event. Async because it queries the keyed sources; does not switch anything or touch the transmit gate.

### `pub async fn guard_action(&self, action: Action, what: &str) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_station_guard_action]

The interlock with the action's class. `Action::PowerOff` and `Action::PowerOnRadio` return `Ok(())` at once, whatever the keyed state: one can only stop a transmission, the other powers a radio that cannot be keyed while off. For `Action::RfPath`, each keyed source is asked in order: `Unkeyed` continues; `Keyed` returns a `Refused` error and `Unknown` a `KeyedUnknown` error, both reading "what: why" with the source's name, and a `StationEvent::Refused` is emitted first. `what` is borrowed text. Fails closed. Async, queries the sources (possibly the radios); touches no hardware outputs or transmit gate.

### `pub fn power_on_action(&self, id: &DeviceId) -> Result<Action, StationError>` [@ANCHOR: radioctl:radioctl_station_station_power_on_action]

Returns the interlock class that powering device `id` on would have, so that a caller that must know before it starts (a reboot, which would cut power first) can refuse up front. `id` is borrowed. Errors: the error of the device lookup when `id` is not in the definition. Otherwise `classify_on`'s answer. Pure, no I/O, no hardware action.

### `fn classify_on(&self, d: &DeviceDef) -> Action` [@ANCHOR: radioctl:radioctl_station_station_classify_on]

Classifies powering device `d` on. A device that is not a radio is `Action::RfPath`. A radio is `Action::PowerOnRadio` (never blocked by keyed state) unless its power channel is a `Power` channel marked `shared_supply`, in which case it is `RfPath` like any other device, since powering that supply might affect other equipment. A radio with no power channel (always on or momentary) counts as non-shared. Pure, cannot fail, no I/O.

### `fn classify_channel(&self, id: &ChannelId, value: Value) -> Action` [@ANCHOR: radioctl:radioctl_station_station_classify_channel]

Classifies setting channel `id` by hand to `value`. Off on a power channel, or on an amplifier's operate channel (standby), is `Action::PowerOff` since it can only reduce RF. On for a channel that is the power channel of a device follows `classify_on` for that device; on for any other channel is `RfPath`. Everything else, including selectors and ports, is `RfPath`. An unknown channel falls to `RfPath`. Pure, cannot fail, no I/O.

### `fn classify_pulse(&self, id: &ChannelId) -> Action` [@ANCHOR: radioctl:radioctl_station_station_classify_pulse]

Classifies pulsing momentary line `id`. It searches the devices powered by a momentary button: if `id` is a device's off line it is `Action::PowerOff`; if it is a device's on line it follows `classify_on` for that device; any other line is `RfPath`. Pure, cannot fail, no I/O, and it does not itself pulse anything.

### `pub fn device_power(&self, id: &DeviceId) -> Result<Option<Value>, StationError>` [@ANCHOR: radioctl:radioctl_station_station_device_power]

Returns the known power state of device `id` (borrowed). Always-on devices are `Some(On)`; a channel-powered device gives the switchboard's known value for the channel, meaning its confirmed value or, on a board that cannot read back, the driver's record of acknowledged writes; a momentary-powered device is `None` because there is no power sensor, as is any unknown state. Errors: the device is not in the definition, or the board does not know the channel. `power_assurance` says which kind of knowledge this is. No hardware action.

### `pub async fn set_channel(&self, id: &ChannelId, value: Value) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_station_set_channel]

Sets a power, output or selector channel by hand, behind the interlock. It runs `guard_action` for the class from `classify_channel` first. For `Value::On`, if the channel powers a device it applies `check_power_on` (dependencies on, antennas selected), and if the channel is an amplifier operate channel it applies `check_operate` (amplifier powered, fault line clear). Only then does it call `Switchboard::set` and discard the confirmed value. Errors: interlock refusals, `Refused` from the checks, and any switchboard error (unknown channel, driver, timeout, read-back mismatch). Async; switches real hardware, and it is the path a ptt-adjacent output takes, so it is the interlock that applies.

### `fn check_operate(&self, c: &ChannelDef) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_station_check_operate]

The refusals for putting an amplifier in operate. For anything other than an amplifier operate channel it returns `Ok(())`. Otherwise the amplifier's device must be known powered on (`check_amp_powered`), and if the channel names a fault input, that line's confirmed value must be off (clear): a tripped line or an unknown one gives a `Refused` error naming the device and the line, so it fails closed. `c` is borrowed. A board lookup error for the fault line is returned as is. Pure reads; no hardware action.

### `fn check_amp_powered(&self, device: &DeviceId) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_station_check_amp_powered]

Refuses unless the amplifier or tuner `device` (borrowed id) is known powered on: `device_power` must be `Some(On)`, which is confirmed read-back, or the record of acknowledged writes on a board that cannot be read back. Otherwise it returns a `Refused` error "is not known to be powered on" (this includes unknown state). A device lookup error is returned as is. Pure, no hardware action.

### `async fn standby(&self, device: &DeviceId)` [@ANCHOR: radioctl:radioctl_station_station_standby]

Puts the amplifier operate channel of `device` into standby (off) before its power is removed, best effort. It finds the operate channel of that device; unless its confirmed value is already off, it sets it off through the switchboard and ignores the result, because a failure is already an event on the switchboard and removing power is the safe direction. Returns nothing and cannot fail. Does nothing when the device has no operate channel. Async; may switch one line. Not interlocked, since standby can only stop RF.

### `pub async fn select(&self, id: &ChannelId, port: u32) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_station_select]

Selects way `port` (a zero-based way number as in the channel definition) on selector channel `id`, behind the interlock. It runs `guard` first, so nothing is switched while any source is keyed or unknown, since moving an antenna selector under power could hot-switch an RF path. Then it calls `Switchboard::set` with `Value::Port(port)` and discards the confirmed value. Errors: interlock refusals and any switchboard error (unknown channel, bad port, driver, timeout, break-before-make or read-back failures). Async; switches real hardware.

### `pub async fn pulse(&self, id: &ChannelId) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_station_pulse]

Pulses momentary channel `id` (a power button or a tuner tune line), behind the interlock. The class comes from `classify_pulse` (off line: always allowed; on line: per `classify_on`; other: RF path) and goes through `guard_action`. If the channel is an amplifier or tuner `Tune` function, the device must also be known powered on, else `Refused`. Then `Switchboard::pulse` runs. Errors: interlock refusals, the powered-on refusal, and any switchboard error. Async; switches real hardware for the pulse time of the channel.

### `fn check_power_on(&self, d: &DeviceDef) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_station_check_power_on]

The dependency refusals for powering device `d` on. Every device in its `depends_on` list must be known on (`device_power` is `Some(On)`), else a `Refused` error "needs X on first"; every selector in `needs_antenna` must have a known selected way (`Value::Port`), else `Refused` "needs an antenna selected". `d` is borrowed. Errors from lookups are returned as they are. Pure reads; no hardware action and no waiting.

### `pub async fn power_on(&self, id: &DeviceId) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_station_power_on]

Powers device `id` on, behind the interlock for its class: a radio passes whatever the keyed state unless its power channel is a `shared_supply`; any other device is refused while anything is keyed or unknown. It then applies `check_power_on` (dependencies and antennas). An always-on device returns `Ok` at once; a channel device has its channel set on and a momentary device has its on line pulsed. Afterwards it sleeps the power channel's inrush stagger, then the device's settle time, and then `wait_ready`. Errors: unknown device, interlock, `Refused`, switchboard errors, and `Timeout` when the device does not answer in time. Async; switches real hardware and waits (tens of ms to seconds). Not idempotent in timing: it repeats the sleeps if called again.

### `fn power_kind(&self, d: &DeviceDef) -> ChannelKind` [@ANCHOR: radioctl:radioctl_station_station_power_kind]

Returns the kind of the channel that powers device `d` (borrowed): a clone of its `ChannelKind` when the device is powered through a channel that exists in the definition, otherwise `ChannelKind::Output` (devices that are always on, momentary, or whose channel is missing). Used to read `shared_supply` and the inrush stagger of a power channel. Pure, cannot fail, no I/O.

### `async fn wait_ready(&self, d: &DeviceDef) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_station_wait_ready]

Waits for device `d` to answer after power-on. With no readiness source, or a ready timeout of 0 ms, it returns `Ok(())` at once. Otherwise it polls `is_ready` every 100 ms until the device answers or `ready_timeout_ms` has passed, and then fails with a `Timeout` error "did not answer within N ms". Async; sleeps between polls; the readiness source may do I/O. No hardware switching.

### `pub async fn power_off(&self, id: &DeviceId) -> Result<Vec<DeviceId>, StationError>` [@ANCHOR: radioctl:radioctl_station_station_power_off]

Orderly power-off of device `id`. It is never refused for the keyed state (cutting power can only stop a transmission), and needs no confirmation or password. Devices that depend on `id` go first, the most dependent first, then `id` itself. Each device that is not already known off is put in amplifier standby, waits its own `off_delay_ms`, then has its channel set off or its off line pulsed. Returns the devices actually powered off, in order. Errors: unknown device, `Refused` when the target is always on or a momentary device has no off line, and switchboard errors, which stop the sequence (earlier devices stay off). Async; switches real hardware and sleeps for the delays.

### `pub async fn run_profile(&self, name: &str) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_station_run_profile]

Runs the steps of profile `name` (borrowed) in order within the profile's timeout, emitting a `Step` event before each and another, "failed: ...", on a failure. A power step skips a device already known on, otherwise calls `power_on`; a select step calls `select`, so every interlock and dependency rule applies. The first failure stops the sequence and is returned; nothing already switched on is switched back off. Errors: unknown profile, any step error, and `Timeout` "ran past N ms" when the whole run exceeds the profile timeout. Async; switches real hardware and waits.

### `pub async fn stop_profile(&self, name: &str) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_station_stop_profile]

Powers off the devices that profile `name` (borrowed) powers on, in reverse order, via `power_off`, so the off delays and the dependents-first rule apply. A `Step` event "power off X" is emitted per device, and devices already known off are skipped. Errors: unknown profile, and the first error from `device_power` or `power_off`, which stops the rest. Select steps are not undone. Async; switches real hardware and sleeps. Never blocked by keyed state.

### `pub async fn all_off(&self) -> AllOffReport` [@ANCHOR: radioctl:radioctl_station_station_all_off]

The emergency all-off. It is never blocked: no keyed-state check, no minimum times, no delays, no confirmation and no password. The order is amplifier operate channels first (standby is what stops RF into the antenna), then the power channel of each device in reverse power-on order (dependents before what they depend on), then any remaining power channel. Each goes through `Switchboard::force_off` limited to 3 seconds. A failing or timed-out channel is recorded in the report's `failed` list (with the reason) and does not stop the rest. Selectors stay where they are, and momentary-powered radios have no off line here and are not touched. An `AllOff` event is emitted at the end. Returns an `AllOffReport` with `off` and `failed`; it does not return an error. Async; switches real hardware.

# Data structures

### `RigConnection` [@ANCHOR: radioctl:radioctl_station_actor_link_rig_connection]

**Purpose and location.** A freshly spawned rig actor and, when its transport reconnects by itself, that transport's link status (the same watch the actor was given as `RigOptions::link_watch`). Defined in `daemons/radioctl/crates/radioctl-station/src/actor_link.rs:41` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `handle` | `RigHandle` | The actor. |
| `watch` | `Option<watch::Receiver<LinkStatus>>` | The transport's status, if it has one. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `spawn` (src/actor_link.rs:53), `spawn` (src/spawner.rs:122), `spawn` (tests/actor_link.rs:133), `spawn` (tests/actor_link.rs:160), `spawn` (tests/actor_link.rs:377).

**See also:** test `a_failed_unkey_pass_is_a_failed_connection (tests/actor_link.rs)`.

### `Slot` [@ANCHOR: radioctl:radioctl_station_actor_link_slot]

**Purpose and location.** The record of one radio's actor in `ActorLinks`: its handle and the link status watch. Defined in `daemons/radioctl/crates/radioctl-station/src/actor_link.rs:56` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `handle` | `RigHandle` | The rig actor's handle. |
| `watch` | `Option<watch::Receiver<LinkStatus>>` | The transport's link status, when the transport reconnects by itself; `None` means no status, which counts as usable. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `no_status_means_the_transport_is_usable (src/actor_link.rs)`.

### `Inner` [@ANCHOR: radioctl:radioctl_station_actor_link_inner]

**Purpose and location.** The mutable state of `ActorLinks`, kept behind a mutex. Defined in `daemons/radioctl/crates/radioctl-station/src/actor_link.rs:62` as a private `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `slots` | `BTreeMap<DeviceId, Slot>` | The live slot of each radio. |
| `spawns` | `BTreeMap<DeviceId, u32>` | How many times each radio's actor has been spawned. |
| `journal` | `Vec<String>` | A log of what the links did, for the station's problem report. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `run_sessions` (src/homeassistant.rs:710), `session` (src/homeassistant.rs:790).

**See also:** test `no_status_means_the_transport_is_usable (src/actor_link.rs)`.

### `ActorLinks` [@ANCHOR: radioctl:radioctl_station_actor_link_actor_links]

**Purpose and location.** The links of real rigs: spawns, respawns and unkeys. Share it as an `Arc`. Defined in `daemons/radioctl/crates/radioctl-station/src/actor_link.rs:83` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `spawner` | `Arc<dyn RigSpawner>` | Creates a rig actor from a radio's spec. |
| `inner` | `Arc<Mutex<Inner>>` | The slots, spawn counts and journal, behind a mutex. |
| `one_connect` | `tokio::sync::Mutex<()>` | An async mutex so only one connect runs at a time. |
| `unkey_timeout` | `Duration` | How long an unkey pass may take before it counts as failed. |

**Invariants, ownership and concurrency.** Shared by `Arc`; a respawned radio is always unkeyed before its link is reported up (see the tests cited), so a crashed actor cannot leave a radio keyed.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/actor_link.rs:93), `with_unkey_timeout` (src/actor_link.rs:104).

**See also:** test `a_failed_unkey_pass_is_a_failed_connection (tests/actor_link.rs)`.

### `SlotState` [@ANCHOR: radioctl:radioctl_station_actor_link_slot_state]

**Purpose and location.** What a radio's slot says. Defined in `daemons/radioctl/crates/radioctl-station/src/actor_link.rs:197` as a private `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Absent` | `Absent` | No slot: the radio has no actor. |
| `Up` | `Up` | The actor is running and its link is usable. |
| `Retrying` | `Retrying` | The transport reports that it is reconnecting by itself. |
| `Dead` | `Dead` | The slot exists but its actor is not usable (its handle reports the link down, or its status is not reconnecting) and must be respawned. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `slot_state` (src/actor_link.rs:148).

**See also:** test `no_status_means_the_transport_is_usable (src/actor_link.rs)`.

### `LiveKey` [@ANCHOR: radioctl:radioctl_station_actor_link_live_key]

**Purpose and location.** The handle by which the keyed state of a radio's actor is read for the interlock. Defined in `daemons/radioctl/crates/radioctl-station/src/actor_link.rs:204` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `inner` | `Arc<Mutex<Inner>>` | The shared link state. |
| `radio` | `DeviceId` | The radio it speaks for. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_failed_unkey_pass_is_a_failed_connection (tests/actor_link.rs)`.

### `BandRoute` [@ANCHOR: radioctl:radioctl_station_band_follow_band_route]

**Purpose and location.** One band: transmit frequencies from `low` to `high` inclusive select `port` of `channel`. Defined in `daemons/radioctl/crates/radioctl-station/src/band_follow.rs:23` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `low` | `Hz` | Lowest frequency of the band. |
| `high` | `Hz` | Highest frequency of the band. |
| `channel` | `ChannelId` | The selector channel. |
| `port` | `u32` | The way (from 1) to select. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `route_for` (src/band_follow.rs:36), `band_routes_for` (src/model.rs:455), `routes` (tests/band_follow.rs:23).

**See also:** test `a_route_is_found_by_band_and_outside_every_band_there_is_none (tests/band_follow.rs)`.

### `BoxFuture` [@ANCHOR: radioctl:radioctl_station_channel_box_future]

**Purpose and location.** A boxed future, so drivers can be used as trait objects. Defined in `daemons/radioctl/crates/radioctl-station/src/channel.rs:23` as a pub `type`. It derives no traits.

**Definition.** `pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `spawn` (src/actor_link.rs:53), `key_state` (src/actor_link.rs:219), `link_up` (src/actor_link.rs:236), `present` (src/actor_link.rs:243), `connect` (src/actor_link.rs:252), `write_line` (src/channel.rs:54), `read_line` (src/channel.rs:56), `force_safe` (src/channel.rs:70).

**See also:** test `a_profile_runs_in_order_waits_for_the_radio_and_stops_in_reverse (src/station.rs)`.

### `Assurance` [@ANCHOR: radioctl:radioctl_station_channel_assurance]

**Purpose and location.** How far the station can rely on a driver to take a device to its safe state, and how directly. Ordered from least to most. Defined in `daemons/radioctl/crates/radioctl-station/src/channel.rs:28` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord.

| Variant | Definition | Meaning |
|---|---|---|
| `Mediated` | `Mediated` | The command reaches the device through another system that can be down, slow or stale (Home Assistant). Never a PTT (push to talk) path, and an amplifier behind it needs an independent stop. |
| `Recorded` | `Recorded` | The host talks to the device directly and the device acknowledged the write, but it cannot report its state (a relay board whose status read is not trusted): the state is the station's own record of acknowledged writes, never a confirmation. The driver sets a known safe state whenever the link opens, because nothing in the board stops it misbehaving. |
| `HostSupervised` | `HostSupervised` | The host talks to the device directly and supervises it, but nothing in the device stops it if the host dies (a relay board, a Shelly without its auto-off timer). |
| `HardwareDeadMan` | `HardwareDeadMan` | The device itself has a dead-man that acts when the host goes silent (the FX2 firmware). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `assurance` (src/channel.rs:49), `assurance` (src/channel.rs:457), `assurance` (src/homeassistant.rs:508), `assurance` (src/modbus.rs:600), `power_assurance` (src/station.rs:174).

**See also:** test `an_unreadable_board_is_recorded_and_read_coils_is_never_sent (src/modbus.rs)`.

### `Recorded` [@ANCHOR: radioctl:radioctl_station_channel_recorded]

**Purpose and location.** A driver's own record of one line: recorded, not confirmed. Defined in `daemons/radioctl/crates/radioctl-station/src/channel.rs:85` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `value` | `Option<bool>` | The level last acknowledged by the device (`None`: unknown). |
| `age` | `Option<Duration>` | How long ago it was acknowledged (`None`: unknown). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `recorded_line` (src/channel.rs:78), `recorded` (src/channel.rs:464), `record` (src/modbus.rs:584), `recorded_line` (src/modbus.rs:673).

**See also:** test `an_unreadable_board_is_recorded_and_read_coils_is_never_sent (src/modbus.rs)`.

### `StationEvent` [@ANCHOR: radioctl:radioctl_station_channel_station_event]

**Purpose and location.** Something that happened in the station. Defined in `daemons/radioctl/crates/radioctl-station/src/channel.rs:94` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Commanded` | `Commanded { channel: ChannelId, value: Value, }` | A command was issued (before it is confirmed). The channel. The commanded value. |
| `Confirmed` | `Confirmed { channel: ChannelId, value: Value, }` | The read-back agrees with the command. The channel. The confirmed value. |
| `Mismatch` | `Mismatch { channel: ChannelId, commanded: Value, confirmed: Option<Value>, }` | The read-back differs from the command: an alarm. The channel. What was commanded. What was read back (`None`: ambiguous or unreadable). |
| `DriverFault` | `DriverFault { channel: ChannelId, detail: String, }` | The driver failed. The channel. What happened. |
| `InputChanged` | `InputChanged { channel: ChannelId, high: bool, }` | A debounced input changed. The channel. The new level. |
| `Pulsed` | `Pulsed { channel: ChannelId, }` | A momentary pulse completed. The channel. |
| `Refused` | `Refused { what: String, why: String, }` | A command was refused by an interlock or a dependency. What was asked. Why not. |
| `Step` | `Step { name: String, index: usize, detail: String, }` | A profile or power sequence reached a step. The profile or operation. The step number from 0. The text. |
| `AllOff` | `AllOff { failed: Vec<ChannelId>, }` | The emergency all-off ran. Channels that did not go off. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `subscribe` (src/channel.rs:471), `emit` (src/channel.rs:477).

**See also:** test `power_is_commanded_then_confirmed_and_mismatches_alarm (src/channel.rs)`.

### `ChannelStatus` [@ANCHOR: radioctl:radioctl_station_channel_channel_status]

**Purpose and location.** What the switchboard knows about one channel. Defined in `daemons/radioctl/crates/radioctl-station/src/channel.rs:162` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `commanded` | `Option<Value>` | What was last commanded (`None`: nothing since start). |
| `confirmed` | `Option<Value>` | What was last confirmed by read-back or sensor (`None`: unknown). |
| `can_confirm` | `bool` | Whether this channel's driver can read back at all. |
| `recorded` | `Option<Value>` | What the driver's own record of acknowledged writes says, for a channel on a [`Assurance::Recorded`] driver (`None`: not such a channel, or the record is unknown). Recorded, not confirmed: the board cannot report its state, so a front end shows this with that caveat. |
| `assurance` | `Assurance` | The assurance level of this channel's driver. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `status` (src/channel.rs:503).

**See also:** test `force_off_ignores_minimum_times_and_busy_channels (src/channel.rs)`.

### `SetOutcome` [@ANCHOR: radioctl:radioctl_station_channel_set_outcome]

**Purpose and location.** The result of a successful set. Defined in `daemons/radioctl/crates/radioctl-station/src/channel.rs:180` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `commanded` | `Value` | What was commanded. |
| `confirmed` | `Option<Value>` | What was read back, when the driver can. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `verify` (src/channel.rs:627).

**See also:** test `force_off_ignores_minimum_times_and_busy_channels (src/channel.rs)`.

### `Fault` [@ANCHOR: radioctl:radioctl_station_channel_fault]

**Purpose and location.** Faults a virtual bank can inject. Defined in `daemons/radioctl/crates/radioctl-station/src/channel.rs:189` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `None` | `None` | Behaves. |
| `FailWrites` | `FailWrites` | Every write fails. |
| `IgnoreWrites` | `IgnoreWrites` | Writes are accepted and have no effect (a stuck relay). |
| `FailReads` | `FailReads` | Every read fails. |
| `WrongReadback` | `WrongReadback` | Reads report the opposite of the truth. |
| `DropAfter` | `DropAfter(usize)` | The first n writes work, then every write fails (a drop mid-sequence). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `set_fault` (src/channel.rs:251).

**See also:** test `faults_are_injected (src/channel.rs)`.

### `BankInner` [@ANCHOR: radioctl:radioctl_station_channel_bank_inner]

**Purpose and location.** The state of a `VirtualBank`: the lines, a log of writes and the fault to inject. Defined in `daemons/radioctl/crates/radioctl-station/src/channel.rs:205` as a private `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `lines` | `BTreeMap<u32, bool>` | Line number to its level. |
| `log` | `Vec<(u32, bool)>` | Every write as (line, level), in order. |
| `fault` | `Fault` | The fault to inject on the next writes. |
| `writes_done` | `usize` | How many writes have been done (used by faults that start after a count). |
| `readback` | `bool` | Whether the bank can read its lines back; `VirtualBank::without_readback` builds one that cannot, to test writes that cannot be verified. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_virtual_bank_holds_lines (src/channel.rs)`.

### `VirtualBank` [@ANCHOR: radioctl:radioctl_station_channel_virtual_bank]

**Purpose and location.** A virtual bank of lines with fault injection, for tests and for running a station without hardware. Clones share the same lines. Defined in `daemons/radioctl/crates/radioctl-station/src/channel.rs:216` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `inner` | `Arc<StdMutex<BankInner>>` | The shared state in a mutex. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/channel.rs:227), `without_readback` (src/channel.rs:233), `build` (src/channel.rs:237), `default` (src/channel.rs:279), `board` (src/channel.rs:905), `new` (src/manager.rs:113), `build` (src/manager.rs:972), `rig` (src/station.rs:727).

**See also:** test `the_default_safe_level_is_off_and_there_is_no_record (src/channel.rs)`.

### `Debouncer` [@ANCHOR: radioctl:radioctl_station_channel_debouncer]

**Purpose and location.** A sans-IO input debouncer: a new level is believed only after it has held for the debounce time. Time is passed in, so tests need no clock. Defined in `daemons/radioctl/crates/radioctl-station/src/channel.rs:340` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `debounce` | `Duration` | How long a level must hold to be believed. |
| `candidate` | `Option<bool>` | The level seen but not yet believed. |
| `since` | `Duration` | The time at which the candidate was first seen. |
| `stable` | `Option<bool>` | The believed level, `None` before the first. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/channel.rs:350).

**See also:** test `the_debouncer_believes_only_a_held_level (src/channel.rs)`.

### `Chan` [@ANCHOR: radioctl:radioctl_station_channel_chan]

**Purpose and location.** One channel of the switchboard: its definition, driver and mutable state. Defined in `daemons/radioctl/crates/radioctl-station/src/channel.rs:381` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `def` | `ChannelDef` | The channel definition (id, line, minimum times and so on). |
| `driver` | `Arc<dyn ChannelDriver>` | The driver that moves the line. |
| `op` | `Mutex<()>` | An async mutex that serializes operations on the channel. |
| `state` | `StdMutex<State>` | The commanded and confirmed values and the last time it went off, behind a mutex. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `chan` (src/channel.rs:481), `record_value` (src/channel.rs:535), `store` (src/channel.rs:559), `fault` (src/channel.rs:578), `read_value` (src/channel.rs:589), `verify` (src/channel.rs:627), `write` (src/channel.rs:668).

**See also:** test `force_off_ignores_minimum_times_and_busy_channels (src/channel.rs)`.

### `State` [@ANCHOR: radioctl:radioctl_station_channel_state]

**Purpose and location.** The mutable state of one channel. Defined in `daemons/radioctl/crates/radioctl-station/src/channel.rs:389` as a private `struct`. It derives Debug, Clone, Copy, Default.

| Field | Type | Meaning |
|---|---|---|
| `commanded` | `Option<Value>` | The value last commanded. |
| `confirmed` | `Option<Value>` | The value last confirmed by the driver or a read back. |
| `last_off` | `Option<Instant>` | When the channel last went off, for minimum-off-time rules. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_default_safe_level_is_off_and_there_is_no_record (src/channel.rs)`.

### `Switchboard` [@ANCHOR: radioctl:radioctl_station_channel_switchboard]

**Purpose and location.** All channels of a station, their drivers and their state. Defined in `daemons/radioctl/crates/radioctl-station/src/channel.rs:396` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `channels` | `HashMap<ChannelId, Chan>` | The channels by id. |
| `events` | `broadcast::Sender<StationEvent>` | The broadcast of station events. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `default` (src/channel.rs:408), `new` (src/channel.rs:416), `board` (src/channel.rs:905), `board` (src/station.rs:183), `board` (tests/homeassistant.rs:403).

**See also:** test `an_input_is_debounced_and_fails_closed (src/channel.rs)`.

### `StationErrorKind` [@ANCHOR: radioctl:radioctl_station_error_station_error_kind]

**Purpose and location.** What went wrong, coarsely, so a caller can branch without parsing text. Defined in `daemons/radioctl/crates/radioctl-station/src/error.rs:8` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Config` | `Config` | A configuration or map is invalid. |
| `Refused` | `Refused` | An interlock or dependency refused the command; nothing was switched. |
| `KeyedUnknown` | `KeyedUnknown` | The keyed state of a rig could not be determined: an RF-path command is refused (power-off and a radio's power-on pass; Bruce, 2026-10-31). |
| `Driver` | `Driver` | The driver could not reach the hardware or it answered something unknown. |
| `NotConfirmed` | `NotConfirmed` | The read-back did not match the commanded state. |
| `Timeout` | `Timeout` | A wait ran out. |
| `Unknown` | `Unknown` | An unknown channel, device, profile or radio. |
| `Store` | `Store` | The durable record could not be read or written. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/error.rs:40), `err` (src/fx2.rs:40), `rot_err` (src/rotator.rs:106).

**See also:** test `errors_carry_kind_and_text (src/error.rs)`.

### `StationError` [@ANCHOR: radioctl:radioctl_station_error_station_error]

**Purpose and location.** An error with a kind and a human-readable detail. Defined in `daemons/radioctl/crates/radioctl-station/src/error.rs:30` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `kind` | `StationErrorKind` | The class of error. |
| `detail` | `String` | What happened. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `spawn` (src/actor_link.rs:53), `connect` (src/actor_link.rs:252), `write_line` (src/channel.rs:54), `read_line` (src/channel.rs:56), `force_safe` (src/channel.rs:70), `write_line` (src/channel.rs:287), `read_line` (src/channel.rs:312), `config` (src/channel.rs:401).

**See also:** test `errors_carry_kind_and_text (src/error.rs)`.

### `Info` [@ANCHOR: radioctl:radioctl_station_fx2_info]

**Purpose and location.** What the board says about itself (`GET_INFO`). Defined in `daemons/radioctl/crates/radioctl-station/src/fx2.rs:60` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `protocol` | `(u8, u8)` | Protocol (major, minor). |
| `device_type` | `u16` | Device type (sub-ID). |
| `firmware` | `(u8, u8, u8)` | Firmware (major, minor, patch). |
| `board_id` | `u8` | Board map id. |
| `capabilities` | `u32` | Capability flags. |
| `safe_state_kind` | `u8` | 0: every output released to input; 1: per map entry. |
| `default_max_on_ms` | `u16` | Default maximum on-time, ms. |
| `default_heartbeat_ms` | `u16` | Default heartbeat lapse, ms. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse` (src/fx2.rs:82), `info` (src/fx2.rs:310).

**See also:** test `the_simulators_records_parse (src/fx2.rs)`.

### `Status` [@ANCHOR: radioctl:radioctl_station_fx2_status]

**Purpose and location.** The board's status record (23 bytes). Defined in `daemons/radioctl/crates/radioctl-station/src/fx2.rs:135` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `map_valid` | `bool` | A valid map is active. |
| `heartbeat_alive` | `bool` | The heartbeat is armed and alive. |
| `out_of_sync` | `bool` | The host claimed a channel keyed that the board had already released. |
| `keyed_mask` | `u8` | Bit per channel: keyed now. |
| `active_mask` | `u8` | Bit per channel: line at its active level. |
| `last_release_reason` | `u8` | Last release reason (1 timeout, 2 heartbeat lapse, 3 sequence, 4 mismatch, 5 USB reset, 6 suspend, 7 host, 8 map cleared). |
| `release_count` | `u8` | Releases so far, modulo 256. |
| `map_error` | `u8` | Last map error code. |
| `key_refusal` | `u8` | Last key refusal code. |
| `last_sequence` | `u16` | Sequence of the last heartbeat the board accepted. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse` (src/fx2.rs:161), `status` (src/fx2.rs:316), `heartbeat` (src/fx2.rs:342).

**See also:** test `the_simulators_records_parse (src/fx2.rs)`.

### `MapStep` [@ANCHOR: radioctl:radioctl_station_fx2_map_step]

**Purpose and location.** A control OUT of the map load. Defined in `daemons/radioctl/crates/radioctl-station/src/fx2.rs:185` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `request` | `u8` | Request number. |
| `value` | `u16` | wValue. |
| `index` | `u16` | wIndex. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `map_steps` (src/fx2.rs:217).

**See also:** test `a_map_becomes_the_firmwares_requests_in_order_with_inputs_left_alone (src/fx2.rs)`.

### `Board` [@ANCHOR: radioctl:radioctl_station_fx2_board]

**Purpose and location.** An open Station I/O board. Defined in `daemons/radioctl/crates/radioctl-station/src/fx2.rs:294` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `t` | `T` | The control transport to the board. |
| `info` | `Info` | The board's identity and capabilities read at open. |
| `seq` | `u16` | The heartbeat sequence number, echoed by the board. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `open` (src/fx2.rs:303).

**See also:** test `a_bench_board_or_one_without_timeouts_is_refused (src/fx2.rs)`.

### `Direction` [@ANCHOR: radioctl:radioctl_station_gpio_map_direction]

**Purpose and location.** The direction of a line. Defined in `daemons/radioctl/crates/radioctl-station/src/gpio_map.rs:13` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Output` | `Output` | A driven output. |
| `Input` | `Input` | A high-impedance input. |
| `InputPullUp` | `InputPullUp` | An input with a pull-up. |
| `InputPullDown` | `InputPullDown` | An input with a pull-down. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_good_map_loads_and_reports_safe_levels (src/gpio_map.rs)`.

### `OutputType` [@ANCHOR: radioctl:radioctl_station_gpio_map_output_type]

**Purpose and location.** How an output drives. Defined in `daemons/radioctl/crates/radioctl-station/src/gpio_map.rs:27` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `PushPull` | `PushPull` | Drives high and low. |
| `OpenDrain` | `OpenDrain` | Drives low only. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_good_map_loads_and_reports_safe_levels (src/gpio_map.rs)`.

### `Level` [@ANCHOR: radioctl:radioctl_station_gpio_map_level]

**Purpose and location.** A logic level. Defined in `daemons/radioctl/crates/radioctl-station/src/gpio_map.rs:38` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Low` | `Low` | Low. |
| `High` | `High` | High. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `safe_levels` (src/gpio_map.rs:293).

**See also:** test `a_good_map_loads_and_reports_safe_levels (src/gpio_map.rs)`.

### `Function` [@ANCHOR: radioctl:radioctl_station_gpio_map_function]

**Purpose and location.** What a line does. Defined in `daemons/radioctl/crates/radioctl-station/src/gpio_map.rs:48` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Power` | `Power` | A power switch channel. |
| `Ptt` | `Ptt` | A PTT line, timed in firmware. |
| `Keyline` | `Keyline` | An amplifier key line. |
| `Selector` | `Selector` | One bit or one way of an antenna or filter selector. |
| `Momentary` | `Momentary` | A momentary pulse. |
| `Band` | `Band` | A band-decoder output. |
| `Sensor` | `Sensor` | A status input (SWR trip, over-temperature, door, power good). |
| `Heartbeat` | `Heartbeat` | A heartbeat indicator. |
| `Spare` | `Spare` | Unused. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `rtu_frames_carry_a_crc (src/modbus.rs)`.

### `Line` [@ANCHOR: radioctl:radioctl_station_gpio_map_line]

**Purpose and location.** One line of a map. Defined in `daemons/radioctl/crates/radioctl-station/src/gpio_map.rs:71` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `pin` | `String` | The pin name, as the board model lists it. |
| `direction` | `Direction` | Direction. |
| `output` | `OutputType` | Output type (outputs only). |
| `safe` | `Option<Level>` | The level the line takes on release and after load; required for power, ptt and keyline. |
| `debounce_ms` | `u64` | Input debounce. |
| `function` | `Function` | The function. |
| `channel` | `Option<String>` | The switch channel this line belongs to (a selector way names its selector). |
| `bit` | `Option<u32>` | The way number of a selector line (from 1) or the bit of a band output. |
| `pulse_ms` | `Option<u64>` | The pulse width of a momentary line. |
| `max_on_ms` | `Option<u64>` | The longest key-down of a ptt line. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_good_map_loads_and_reports_safe_levels (src/gpio_map.rs)`.

### `GpioMap` [@ANCHOR: radioctl:radioctl_station_gpio_map_gpio_map]

**Purpose and location.** A station's assignment of functions to the lines of one board. Defined in `daemons/radioctl/crates/radioctl-station/src/gpio_map.rs:103` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `board` | `String` | The board model name. |
| `lines` | `Vec<Line>` | The lines. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `map_steps` (src/fx2.rs:217), `load_map` (src/fx2.rs:323), `map` (src/fx2.rs:429), `from_toml` (src/gpio_map.rs:195).

**See also:** test `a_good_map_loads_and_reports_safe_levels (src/gpio_map.rs)`.

### `BoardModel` [@ANCHOR: radioctl:radioctl_station_gpio_map_board_model]

**Purpose and location.** A board model: the pins it exposes. Defined in `daemons/radioctl/crates/radioctl-station/src/gpio_map.rs:113` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | The model name. |
| `pins` | `Vec<String>` | Exposed pins. |
| `notes` | `String` | Pins that are outputs only or inputs only, if any (pin to direction class). |
| `status_readable` | `bool` | Whether the board can report its relay or pin state back. False for a board whose status read is not trusted: the driver keeps its own record and never claims a confirmation. |
| `safe_state` | `Vec<bool>` | The level of each pin the driver applies when the link opens and in the emergency all-off (empty: all off). |
| `recommendation` | `Option<String>` | Optional listing status in `docs/RECOMMENDED_HARDWARE.md`: `recommended`, `acceptable` or `not-recommended`. Informational only; it never changes how a board is driven. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `get` (src/gpio_map.rs:187).

**See also:** test `the_builtin_catalog_lists_the_boards (src/gpio_map.rs)`.

### `BoardCatalog` [@ANCHOR: radioctl:radioctl_station_gpio_map_board_catalog]

**Purpose and location.** The board models known to a build, parsed from TOML. Defined in `daemons/radioctl/crates/radioctl-station/src/gpio_map.rs:141` as a pub `struct`. It derives Debug, Clone, Default, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `boards` | `BTreeMap<String, BoardModel>` | The board models by name. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_toml` (src/gpio_map.rs:157), `builtin` (src/gpio_map.rs:181), `from_toml` (src/gpio_map.rs:195), `validate` (src/gpio_map.rs:206).

**See also:** test `the_builtin_catalog_lists_the_boards (src/gpio_map.rs)`.

### `CatalogFile` [@ANCHOR: radioctl:radioctl_station_gpio_map_catalog_file]

**Purpose and location.** The TOML shape of a board catalog file: an array of `[[board]]` tables. Defined in `daemons/radioctl/crates/radioctl-station/src/gpio_map.rs:146` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `boards` | `Vec<BoardModel>` | The board models in the file. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_builtin_catalog_lists_the_boards (src/gpio_map.rs)`.

### `Secret` [@ANCHOR: radioctl:radioctl_station_homeassistant_secret]

**Purpose and location.** A secret value. Its `Debug` hides it and it has no `Display`; the only way to the text is [`Secret::expose`], used where it must go on the wire. Defined in `daemons/radioctl/crates/radioctl-station/src/homeassistant.rs:69` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `0` | `String` | The secret text; reachable only through `expose`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/homeassistant.rs:74), `secret` (src/homeassistant.rs:96), `secret` (src/homeassistant.rs:122), `secret` (src/homeassistant.rs:148).

**See also:** test `a_secret_never_shows_in_debug (src/homeassistant.rs)`.

### `MemorySecrets` [@ANCHOR: radioctl:radioctl_station_homeassistant_memory_secrets]

**Purpose and location.** Secrets held in memory (tests, or a front end that has unlocked a vault). Defined in `daemons/radioctl/crates/radioctl-station/src/homeassistant.rs:101` as a pub `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `values` | `Mutex<BTreeMap<String, String>>` | Secret name to value, behind a mutex. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/homeassistant.rs:108), `secrets` (tests/homeassistant.rs:325).

**See also:** test `a_wrong_token_never_syncs_and_is_never_logged (tests/homeassistant.rs)`.

### `FileSecrets` [@ANCHOR: radioctl:radioctl_station_homeassistant_file_secrets]

**Purpose and location.** Secrets in files of a directory, one file per secret, readable by the owner only. Defined in `daemons/radioctl/crates/radioctl-station/src/homeassistant.rs:131` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `dir` | `PathBuf` | The directory holding one file per secret. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/homeassistant.rs:138).

**See also:** test `start_refuses_what_it_should_before_touching_the_network (tests/homeassistant.rs)`.

### `AuditEntry` [@ANCHOR: radioctl:radioctl_station_homeassistant_audit_entry]

**Purpose and location.** One line of the audit log. It never holds a secret. Defined in `daemons/radioctl/crates/radioctl-station/src/homeassistant.rs:194` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `at_unix` | `u64` | Unix seconds. |
| `action` | `String` | What was done: `service switch.turn_on`, `websocket connect`, `discover`. |
| `entity` | `Option<String>` | The entity concerned, if any. |
| `result` | `String` | `ok`, or `error: ` and why. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `record` (src/homeassistant.rs:208), `entries` (src/homeassistant.rs:226), `record` (src/homeassistant.rs:234).

**See also:** test `a_rejected_service_call_is_an_error_and_an_audit_entry (tests/homeassistant.rs)`.

### `MemoryAudit` [@ANCHOR: radioctl:radioctl_station_homeassistant_memory_audit]

**Purpose and location.** An audit log held in memory. Defined in `daemons/radioctl/crates/radioctl-station/src/homeassistant.rs:213` as a pub `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `entries` | `Mutex<Vec<AuditEntry>>` | The entries, behind a mutex. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/homeassistant.rs:220), `driver` (tests/homeassistant.rs:362).

**See also:** test `discovery_switching_and_confirmation_by_event (tests/homeassistant.rs)`.

### `HaConfig` [@ANCHOR: radioctl:radioctl_station_homeassistant_ha_config]

**Purpose and location.** How to reach Home Assistant and which entities are lines. Defined in `daemons/radioctl/crates/radioctl-station/src/homeassistant.rs:244` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `address` | `String` | An IP literal in a private range, optionally with `:port` (default 8123). |
| `token_name` | `String` | The name of the long-lived access token in the secret store. |
| `entities` | `Vec<String>` | Line `n` is `entities[n]`, as `domain.object_id`. |
| `request_timeout` | `Duration` | Longest wait for one HTTP exchange or one connection step. |
| `confirm_wait` | `Duration` | Longest wait for the state event that confirms a command. |
| `ping_every` | `Duration` | How often the session pings Home Assistant; a missed pong ends the session. |
| `backoff` | `Backoff` | Waits between reconnect attempts of the session. |
| `tls` | `Option<HaTls>` | TLS (Transport Layer Security) to a Home Assistant that serves only HTTPS: the owner's trust, never off. `None` is plain HTTP and WebSocket. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `config` (tests/homeassistant.rs:331).

**See also:** test `start_refuses_what_it_should_before_touching_the_network (tests/homeassistant.rs)`.

### `Discovered` [@ANCHOR: radioctl:radioctl_station_homeassistant_discovered]

**Purpose and location.** An entity Home Assistant offers, for the owner to choose from. Defined in `daemons/radioctl/crates/radioctl-station/src/homeassistant.rs:288` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `entity_id` | `String` | `domain.object_id`. |
| `state` | `String` | Its state now (`on`, `off`, `unavailable`, ...). |
| `friendly_name` | `Option<String>` | The name Home Assistant shows, if it has one. |
| `writable` | `bool` | Whether it can be switched (otherwise it can only be read). |
| `mapped` | `bool` | Whether it is already a line of this driver. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `discover` (src/homeassistant.rs:443).

**See also:** test `discovery_switching_and_confirmation_by_event (tests/homeassistant.rs)`.

### `Cache` [@ANCHOR: radioctl:radioctl_station_homeassistant_cache]

**Purpose and location.** What the driver knows of Home Assistant's entities. Defined in `daemons/radioctl/crates/radioctl-station/src/homeassistant.rs:302` as a private `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `synced` | `bool` | Whether the first full state has arrived. |
| `states` | `HashMap<String, (String, Option<String>)>` | Entity id to (state, friendly name), every entity in an allowed domain. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `discovery_switching_and_confirmation_by_event (tests/homeassistant.rs)`.

### `Inner` [@ANCHOR: radioctl:radioctl_station_homeassistant_inner]

**Purpose and location.** The state shared by the Home Assistant driver and its session task. Defined in `daemons/radioctl/crates/radioctl-station/src/homeassistant.rs:308` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `addr` | `SocketAddr` | The Home Assistant address (a private IP literal). |
| `connector` | `Option<TlsConnector>` | The TLS connector, `None` for plain connections on loopback. |
| `token` | `Secret` | The access token, a `Secret`. |
| `cfg` | `HaConfig` | The driver configuration (allowed domains and so on). |
| `cache` | `Mutex<Cache>` | The entity cache, behind a mutex. |
| `changed` | `Notify` | Notified when the cache changes, to wake waiters. |
| `expect` | `Mutex<HashMap<String, bool>>` | The states the driver expects after a command, by entity, behind a mutex. |
| `audit` | `Arc<dyn AuditLog>` | Where every service call is recorded. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `run_sessions` (src/homeassistant.rs:710), `session` (src/homeassistant.rs:790).

**See also:** test `discovery_switching_and_confirmation_by_event (tests/homeassistant.rs)`.

### `HomeAssistant` [@ANCHOR: radioctl:radioctl_station_homeassistant_home_assistant]

**Purpose and location.** The driver. Dropping it ends the session task. Defined in `daemons/radioctl/crates/radioctl-station/src/homeassistant.rs:320` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `inner` | `Arc<Inner>` | The shared state. |
| `task` | `JoinHandle<()>` | The session task, ended when the driver is dropped. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, JoinHandle (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `driver` (tests/homeassistant.rs:362), `board` (tests/homeassistant.rs:403).

**See also:** test `a_wrong_token_never_syncs_and_is_never_logged (tests/homeassistant.rs)`.

### `Family` [@ANCHOR: radioctl:radioctl_station_http_relay_family]

**Purpose and location.** The device family. Defined in `daemons/radioctl/crates/radioctl-station/src/http_relay.rs:26` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Shelly` | `Shelly` | Shelly Gen2 RPC. |
| `Tasmota` | `Tasmota` | Tasmota HTTP commands. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/http_relay.rs:86).

**See also:** test `only_private_ip_literals_are_accepted (src/http_relay.rs)`.

### `HttpRelay` [@ANCHOR: radioctl:radioctl_station_http_relay_http_relay]

**Purpose and location.** A bank of relays on one device; line n is relay n (0-based). Defined in `daemons/radioctl/crates/radioctl-station/src/http_relay.rs:34` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `family` | `Family` | The relay family (request format). |
| `addr` | `SocketAddr` | The device's address. |
| `relays` | `u32` | The number of relays; line n is relay n. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/http_relay.rs:86).

**See also:** test `only_private_ip_literals_are_accepted (src/http_relay.rs)`.

### `RadioState` [@ANCHOR: radioctl:radioctl_station_lifecycle_radio_state]

**Purpose and location.** Where a radio is in its life. Defined in `daemons/radioctl/crates/radioctl-station/src/lifecycle.rs:13` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Off` | `Off` | Not powered (the switch is confirmed off). |
| `PoweringOn` | `PoweringOn` | The power-on was commanded and the switch has not confirmed it yet. |
| `Booting` | `Booting` | Powered; waiting for it to boot and for the link to come up. |
| `Ready` | `Ready` | The link is up. |
| `Degraded` | `Degraded` | Powered and still present, but the link was lost or is failing: reconnecting. |
| `Unreachable` | `Unreachable` | Powered or of unknown power, and nothing answers: it did not boot, it dropped, or it was never seen. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `new` (src/lifecycle.rs:103), `state` (src/lifecycle.rs:114), `goto` (src/lifecycle.rs:126), `state` (src/manager.rs:417), `drain` (src/manager.rs:1019).

**See also:** test `the_happy_path_goes_through_every_state_in_order (src/lifecycle.rs)`.

### `PowerEvidence` [@ANCHOR: radioctl:radioctl_station_lifecycle_power_evidence]

**Purpose and location.** What the switch channel says about the radio's power. Defined in `daemons/radioctl/crates/radioctl-station/src/lifecycle.rs:31` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `On` | `On` | Confirmed on by read-back. |
| `Off` | `Off` | Confirmed off by read-back. |
| `Unknown` | `Unknown` | No power source, no read-back, or a failed read. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `ev` (src/lifecycle.rs:248), `power_evidence` (src/manager.rs:475).

**See also:** test `the_happy_path_goes_through_every_state_in_order (src/lifecycle.rs)`.

### `Evidence` [@ANCHOR: radioctl:radioctl_station_lifecycle_evidence]

**Purpose and location.** One snapshot of everything known about a radio right now. Defined in `daemons/radioctl/crates/radioctl-station/src/lifecycle.rs:42` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `power` | `PowerEvidence` | The switch channel state. |
| `link_up` | `bool` | The rig actor's link state (`RigHandle::link_up`; the actor raised `Alarm::LinkDown` when this fell). |
| `present` | `Option<bool>` | Hotplug (USB) or ping (network) evidence: `Some(true)` the device is there, `Some(false)` it is not, `None` no evidence. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `step` (src/lifecycle.rs:172), `ev` (src/lifecycle.rs:248), `gather` (src/manager.rs:488).

**See also:** test `the_switch_going_off_wins_and_hotplug_wakes_an_unreachable_radio (src/lifecycle.rs)`.

### `LifecycleTimes` [@ANCHOR: radioctl:radioctl_station_lifecycle_lifecycle_times]

**Purpose and location.** The timing the machine uses. Defined in `daemons/radioctl/crates/radioctl-station/src/lifecycle.rs:55` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `boot_time` | `Duration` | Expected boot time; a radio still without a link this long plus `boot_grace` after power is confirmed on is Unreachable. |
| `boot_grace` | `Duration` | Extra time allowed on top of the boot time. |
| `power_on_timeout` | `Duration` | How long a switch may take to confirm a power-on before the radio is Unreachable. |
| `degraded_timeout` | `Duration` | How long a radio may stay Degraded before it is Unreachable. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `default` (src/lifecycle.rs:70), `new` (src/lifecycle.rs:103), `life_times` (src/manager.rs:882).

**See also:** test `the_happy_path_goes_through_every_state_in_order (src/lifecycle.rs)`.

### `Transition` [@ANCHOR: radioctl:radioctl_station_lifecycle_transition]

**Purpose and location.** A state change with its reason. Defined in `daemons/radioctl/crates/radioctl-station/src/lifecycle.rs:82` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `from` | `RadioState` | The state before. |
| `to` | `RadioState` | The state after. |
| `reason` | `&'static str` | Why, in words. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `goto` (src/lifecycle.rs:126), `power_on_commanded` (src/lifecycle.rs:143), `assume_powered` (src/lifecycle.rs:155), `power_off_commanded` (src/lifecycle.rs:165), `step` (src/lifecycle.rs:172).

**See also:** test `the_switch_going_off_wins_and_hotplug_wakes_an_unreachable_radio (src/lifecycle.rs)`.

### `Lifecycle` [@ANCHOR: radioctl:radioctl_station_lifecycle_lifecycle]

**Purpose and location.** The state machine of one radio. Time is a [`Duration`] since any fixed start. Defined in `daemons/radioctl/crates/radioctl-station/src/lifecycle.rs:93` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `state` | `RadioState` | The radio's current state. |
| `since` | `Duration` | The time the state was entered. |
| `times` | `LifecycleTimes` | The timing limits of the lifecycle. |
| `prev_present` | `Option<bool>` | Whether the radio was present on the last step. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/lifecycle.rs:103).

**See also:** test `the_happy_path_goes_through_every_state_in_order (src/lifecycle.rs)`.

### `RigLinks` [@ANCHOR: radioctl:radioctl_station_manager_rig_links]

**Purpose and location.** The link of real rigs: the actor's `link_up` for evidence; connecting is the caller's business (it respawns the actor), so `connect` here only reports whether the link is up. Defined in `daemons/radioctl/crates/radioctl-station/src/manager.rs:45` as a pub `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `rigs` | `Mutex<BTreeMap<DeviceId, RigHandle>>` | The handles of connected rigs by device id, behind a mutex. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/manager.rs:52).

**See also:** test `rig_links_report_a_radio_without_a_handle_as_down_and_a_live_handle_as_up (tests/station_rig.rs)`.

### `VirtualRadioLink` [@ANCHOR: radioctl:radioctl_station_manager_virtual_radio_link]

**Purpose and location.** A virtual radio for tests: it boots a set time after its power line (a [`VirtualBank`] line) goes on, can be made never to boot, to drop its link, or to vanish. Defined in `daemons/radioctl/crates/radioctl-station/src/manager.rs:94` as a pub `struct`. It derives Clone.

| Field | Type | Meaning |
|---|---|---|
| `bank` | `VirtualBank` | The virtual bank whose line is the radio's power. |
| `radios` | `Arc<Mutex<BTreeMap<DeviceId, VRadio>>>` | The simulated radios by device id, behind a mutex. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/manager.rs:113).

**See also:** test `the_power_button_walks_a_radio_up_and_reports_without_restoring (src/manager.rs)`.

### `VRadio` [@ANCHOR: radioctl:radioctl_station_manager_vradio]

**Purpose and location.** The simulated radio behind `VirtualRadioLink`. Defined in `daemons/radioctl/crates/radioctl-station/src/manager.rs:99` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `line` | `u32` | The power line number in the bank. |
| `boot` | `Option<Duration>` | How long the radio takes to boot after power on; `None` means it never boots. |
| `on_since` | `Option<Instant>` | When the power line went on. |
| `connected` | `bool` | Whether its link is up. |
| `dropped` | `bool` | Whether the link was dropped for the test. |
| `gone` | `bool` | Whether the radio vanished. |
| `connects` | `u32` | How many connects were made. |
| `report` | `Option<KnownState>` | The state the radio reports, if any. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `refresh` (src/manager.rs:180).

**See also:** test `the_power_button_walks_a_radio_up_and_reports_without_restoring (src/manager.rs)`.

### `RadioEvent` [@ANCHOR: radioctl:radioctl_station_manager_radio_event]

**Purpose and location.** A radio changed state: the station-level event. Defined in `daemons/radioctl/crates/radioctl-station/src/manager.rs:237` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `radio` | `DeviceId` | The radio. |
| `from` | `RadioState` | The state before. |
| `to` | `RadioState` | The state after. |
| `reason` | `String` | Why. |
| `at_unix` | `u64` | Unix seconds. |
| `reported` | `Option<KnownState>` | The state the radio reported on reconnecting (reported, never restored). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `subscribe` (src/manager.rs:392), `spawn_callback` (src/manager.rs:398), `poll` (src/manager.rs:543), `tick` (src/manager.rs:607), `drain` (src/manager.rs:1019).

**See also:** test `a_radio_that_drops_while_powered_degrades_then_comes_back_without_any_write (src/manager.rs)`.

### `Button` [@ANCHOR: radioctl:radioctl_station_manager_button]

**Purpose and location.** The power button. Defined in `daemons/radioctl/crates/radioctl-station/src/manager.rs:254` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `On` | `On` | Switch on and reconnect. |
| `Off` | `Off` | Orderly off (dependents first). |
| `Reboot` | `Reboot` | Off, wait the channel's minimum off time, on, reconnect. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_power_button_walks_a_radio_up_and_reports_without_restoring (src/manager.rs)`.

### `ButtonReport` [@ANCHOR: radioctl:radioctl_station_manager_button_report]

**Purpose and location.** What a button press did. Defined in `daemons/radioctl/crates/radioctl-station/src/manager.rs:265` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `state` | `RadioState` | The radio's state when the operation ended. |
| `powered_off` | `Vec<DeviceId>` | Devices switched off, in order (dependents first). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_power_button_walks_a_radio_up_and_reports_without_restoring (src/manager.rs)`.

### `ReconcileItem` [@ANCHOR: radioctl:radioctl_station_manager_reconcile_item]

**Purpose and location.** One radio's start-up reconcile result. Defined in `daemons/radioctl/crates/radioctl-station/src/manager.rs:274` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `radio` | `DeviceId` | The radio. |
| `real_power` | `Option<Value>` | The real power read back (`None`: could not be read or no power source). |
| `recorded` | `RadioState` | What the record said. |
| `resolved` | `RadioState` | The state it was resolved to. |
| `disagrees` | `bool` | Whether the record and the real state disagreed. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `reconcile` (src/manager.rs:798).

**See also:** test `reconcile_reads_without_writing_and_reports_disagreement_with_the_record (src/manager.rs)`.

### `Entry` [@ANCHOR: radioctl:radioctl_station_manager_entry]

**Purpose and location.** One radio in the manager: its record, lifecycle and connect backoff. Defined in `daemons/radioctl/crates/radioctl-station/src/manager.rs:287` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `record` | `RadioRecord` | The stored record of the radio. |
| `life` | `Lifecycle` | The lifecycle state machine. |
| `connect_failures` | `u32` | Consecutive failed connects. |
| `next_connect` | `Duration` | The time before which no new connect is tried. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `life_times` (src/manager.rs:882).

**See also:** test `the_power_button_walks_a_radio_up_and_reports_without_restoring (src/manager.rs)`.

### `RadioManager` [@ANCHOR: radioctl:radioctl_station_manager_radio_manager]

**Purpose and location.** The lifecycle, record and power button of the station's radios. Defined in `daemons/radioctl/crates/radioctl-station/src/manager.rs:298` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `station` | `Arc<Station>` | The station. |
| `link` | `Arc<dyn RadioLink>` | How radios are connected and observed. |
| `store` | `RecordStore` | The record file. |
| `clock` | `Arc<dyn Fn() -> u64 + Send + Sync>, start: Instant, times: LifecycleTimes, backoff: Backoff, inner: Mutex<BTreeMap<DeviceId, Entry>>, events: broadcast::Sender<RadioEvent>, problems: Mutex<Vec<String>>,` | The wall clock in seconds, injected for tests. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_power_button_walks_a_radio_up_and_reports_without_restoring (src/manager.rs)`.

### `Framing` [@ANCHOR: radioctl:radioctl_station_modbus_framing]

**Purpose and location.** Serial-line framing. Defined in `daemons/radioctl/crates/radioctl-station/src/modbus.rs:29` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Ascii` | `Ascii` | `:` hex-ASCII bytes, LRC, CR LF. |
| `Rtu` | `Rtu` | Binary bytes, CRC-16, frames separated by idle time. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `boards_are_description_data (src/modbus.rs)`.

### `ModbusBoard` [@ANCHOR: radioctl:radioctl_station_modbus_modbus_board]

**Purpose and location.** A coil relay board, as data. Defined in `daemons/radioctl/crates/radioctl-station/src/modbus.rs:38` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | Model name. |
| `framing` | `Framing` | ASCII or RTU. |
| `slave` | `u8` | Slave address. |
| `baud` | `u32` | Baud rate, 8 data bits, no parity, 1 stop bit unless the board says otherwise. |
| `coils` | `u16` | Number of coils (relays). |
| `coil_base` | `u16` | Coil address of line 0. |
| `reply_timeout_ms` | `u64` | How long to wait for a reply. |
| `inter_frame_ms` | `u64` | RTU: idle time that ends a frame. Also the pause kept before each request. |
| `status_readable` | `bool` | Whether the board's coil read (function 1) is trusted to report the relay states (Bruce, 2026-11-01, decision 23). False: function 1 is NEVER sent, the driver keeps its own record of acknowledged writes and the channel assurance is [`Assurance::Recorded`]. Even when true, the coil bits are the commanded states, not a measurement of the contacts. |
| `safe_state` | `Vec<bool>` | The level of each coil the driver applies, in ONE function 15 frame, when the link opens and after every reconnect, and in the emergency all-off. Empty means every coil off; otherwise one entry per coil (a board may need some channels on). |
| `verified` | `bool` | Set to true once checked on a real module. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `sainsmart_16ch` (src/modbus.rs:91), `from_toml` (src/modbus.rs:109), `new` (src/modbus.rs:350), `fast_board` (src/modbus.rs:917).

**See also:** test `boards_are_description_data (src/modbus.rs)`.

### `Reply` [@ANCHOR: radioctl:radioctl_station_modbus_reply]

**Purpose and location.** A validated reply. Defined in `daemons/radioctl/crates/radioctl-station/src/modbus.rs:272` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Written` | `Written` | A write was echoed as the protocol requires. |
| `Coils` | `Coils(Vec<bool>)` | The coils that were read, in order. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_reply` (src/modbus.rs:283), `exchange` (src/modbus.rs:446).

**See also:** test `unknown_replies_fail_closed (src/modbus.rs)`.

### `Shadow` [@ANCHOR: radioctl:radioctl_station_modbus_shadow]

**Purpose and location.** The driver's record of the board: one entry per coil plus the safe-state gate. Defined in `daemons/radioctl/crates/radioctl-station/src/modbus.rs:320` as a private `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `coils` | `Vec<Option<(bool, Instant)>>` | Per coil: the level the board last acknowledged and when (`None`: unknown). |
| `safe_done` | `bool` | Whether the safe state has been applied on the current link. |
| `failures` | `u32` | Consecutive failed safe-state writes. |
| `retry_at` | `Option<Instant>` | No safe-state attempt before this time. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `shadow` (src/modbus.rs:364).

**See also:** test `unknown_replies_fail_closed (src/modbus.rs)`.

### `ModbusRelay` [@ANCHOR: radioctl:radioctl_station_modbus_modbus_relay]

**Purpose and location.** A coil relay board on an async byte stream.  Nothing but a function 5 or 15 write is ever sent for a board with `status_readable = false`, and no command goes out on a link until the board's safe state has been written and acknowledged on it. The driver's record of the coils is updated only by acknowledged writes. Defined in `daemons/radioctl/crates/radioctl-station/src/modbus.rs:340` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `board` | `ModbusBoard` | The board definition. |
| `io` | `Mutex<T>` | The byte stream, behind an async mutex that serializes commands. |
| `shadow` | `StdMutex<Shadow>` | The record of the coils, updated only by acknowledged writes. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/modbus.rs:350).

**See also:** test `the_safe_state_is_the_first_frame_and_nothing_goes_before_it (src/modbus.rs)`.

### `ChannelId` [@ANCHOR: radioctl:radioctl_station_model_channel_id]

**Purpose and location.** The name of a switch channel. Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:12` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `0` | `pub String` | The channel name text. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `assurance` (src/channel.rs:457), `recorded` (src/channel.rs:464), `chan` (src/channel.rs:481), `def` (src/channel.rs:489), `ids` (src/channel.rs:495), `status` (src/channel.rs:503), `known` (src/channel.rs:526), `read_back` (src/channel.rs:610).

**See also:** test `the_sample_station_parses_and_orders (src/model.rs)`.

### `DeviceId` [@ANCHOR: radioctl:radioctl_station_model_device_id]

**Purpose and location.** The name of a device (a radio, an amplifier and so on). Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:17` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `0` | `pub String` | The device name text. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `spawn` (src/actor_link.rs:53), `handle` (src/actor_link.rs:111), `key_source` (src/actor_link.rs:119), `spawns` (src/actor_link.rs:128), `slot_state` (src/actor_link.rs:148), `link_up` (src/actor_link.rs:236), `present` (src/actor_link.rs:243), `connect` (src/actor_link.rs:252).

**See also:** test `the_sample_station_parses_and_orders (src/model.rs)`.

### `Value` [@ANCHOR: radioctl:radioctl_station_model_value]

**Purpose and location.** The value of a channel: a power or output channel is `Off` or `On`; a selector is `Off` (every way open) or `Port(n)` with n from 1. Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:23` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Off` | `Off` | Off, open, released. |
| `On` | `On` | On, closed, asserted. |
| `Port` | `Port(u32)` | Selector way n (from 1). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `known` (src/channel.rs:526), `record_value` (src/channel.rs:535), `store` (src/channel.rs:559), `read_value` (src/channel.rs:589), `read_back` (src/channel.rs:610), `verify` (src/channel.rs:627), `classify_channel` (src/station.rs:259), `device_power` (src/station.rs:311).

**See also:** test `power_is_commanded_then_confirmed_and_mismatches_alarm (src/channel.rs)`.

### `AmpFunction` [@ANCHOR: radioctl:radioctl_station_model_amp_function]

**Purpose and location.** What an amplifier or tuner control line does. Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:35` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Operate` | `Operate` | Operate (on) or standby (off). |
| `Tune` | `Tune` | Start the tune cycle (a pulse). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_sample_station_parses_and_orders (src/model.rs)`.

### `ChannelKind` [@ANCHOR: radioctl:radioctl_station_model_channel_kind]

**Purpose and location.** The typed kind of a switch channel. Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:45` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq, Serialize, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Power` | `Power { rating_watts: Option<u32>, inrush_stagger_ms: u64, min_off_ms: u64, shared_supply: bool, }` | On/off power. `inrush_stagger_ms` is the wait before the next power-on in a sequence; `min_off_ms` the shortest time it stays off before it may come on again. `shared_supply` is the opt-in shared-supply protection (Bruce, 2026-10-31): set it on a radio's power channel that also feeds an amplifier, antenna switch or other RF-path device, and powering that radio on is refused while any attached rig is keyed or its keyed state is unknown. Off (the default) means a radio's power-on passes every keyed state: an off radio cannot be keyed. Contact or load rating, informational. Stagger after switching on. Minimum off time. The channel also feeds an RF-path device: treat the radio's power-on as an RF-path change. Off by default. |
| `Selector` | `Selector { ways: u32, settle_ms: u64, }` | An n-way exclusive selector, break-before-make. Number of ways. Pause between opening the old way and closing the new one. |
| `Momentary` | `Momentary { pulse_ms: u64, }` | A pulse of fixed width (a power button, a grounded line). Pulse width. |
| `Output` | `Output` | A general digital output such as a band-decoder line. |
| `Input` | `Input { debounce_ms: u64, }` | A status sensor, debounced. How long a new level must hold before it is believed. |
| `Amplifier` | `Amplifier { device: DeviceId, function: AmpFunction, pulse_ms: u64, fault_input: Option<ChannelId>, }` | An amplifier or tuner control line. `function = "operate"` is a binary output (on = operate, off = standby; standby is the safe state and must be declared); `function = "tune"` is a momentary pulse that starts a tuner or amplifier tune cycle (it only starts the device's own cycle; the station never keys a transmitter for it). The amplifier or tuner this line belongs to. What the line does. Pulse width, for `tune` only. A status input (a fault or trip line, high = tripped) that must read low and known before the amplifier may be put in operate. |
| `Ptt` | `Ptt { max_on_ms: u64, min_off_ms: u64, }` | A radio or amplifier key line, timed in firmware; defined so a map can name it, but the switchboard never drives it (phase 3, the FX2 firmware). Longest key-down. Shortest time between key-downs. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `power_kind` (src/station.rs:479).

**See also:** test `invalid_stations_are_refused (src/model.rs)`.

### `ChannelDef` [@ANCHOR: radioctl:radioctl_station_model_channel_def]

**Purpose and location.** One switch channel. Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:119` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `id` | `ChannelId` | Its name. |
| `kind` | `ChannelKind` | Its type. |
| `driver` | `String` | The name of the driver bank that carries it. |
| `line` | `u32` | The first line on that bank (a selector uses `ways` consecutive lines). |
| `safe` | `Option<Value>` | What the channel does on host loss, at boot and on a release. Required for power and ptt. |
| `description` | `String` | Free text. |
| `rig` | `Option<String>` | The rig this channel belongs to (the rig's name in the daemon configuration). A selector follows a rig's transmit band only for that rig; none means every rig. |
| `band_follow` | `bool` | Per-channel opt-out of band following (`band_follow = false`): the channel keeps whatever way is set by hand or by a profile although a `[[band_routes]]` entry names it. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `def` (src/channel.rs:489), `channel` (src/model.rs:448), `check_operate` (src/station.rs:347).

**See also:** test `an_input_is_debounced_and_fails_closed (src/channel.rs)`.

### `DeviceKind` [@ANCHOR: radioctl:radioctl_station_model_device_kind]

**Purpose and location.** What kind of equipment a device is. Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:224` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Radio` | `Radio` | A transceiver or receiver. |
| `Amplifier` | `Amplifier` | A power amplifier. |
| `Tuner` | `Tuner` | An antenna tuner. |
| `Rotator` | `Rotator` | A rotator. |
| `Switch` | `Switch` | A switch or relay box. |
| `Computer` | `Computer` | The computer. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_sample_station_parses_and_orders (src/model.rs)`.

### `PowerSource` [@ANCHOR: radioctl:radioctl_station_model_power_source]

**Purpose and location.** How a device is powered. Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:242` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq, Serialize, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `AlwaysOn` | `AlwaysOn` | No switch: the device is always on. |
| `Channel` | `Channel { channel: ChannelId, }` | A power channel. The channel. |
| `Momentary` | `Momentary { on: ChannelId, off: Option<ChannelId>, }` | A momentary line pulsed to power on (a power button); an optional second one to power off. The pulse that powers on. The pulse that powers off, if the radio has one. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `power_source` (src/manager.rs:645), `press_on` (src/manager.rs:666).

**See also:** test `power_meters_are_validated_one_per_rig (src/model.rs)`.

### `DeviceDef` [@ANCHOR: radioctl:radioctl_station_model_device_def]

**Purpose and location.** One device in the station. Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:262` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `id` | `DeviceId` | Its name. |
| `kind` | `DeviceKind` | What it is. |
| `power` | `PowerSource` | How it gets power. |
| `depends_on` | `Vec<DeviceId>` | Devices that must be on before this one may be powered on, and are powered off after it. |
| `needs_antenna` | `Vec<ChannelId>` | Selector channels that must have a way selected before this one may be powered on. |
| `settle_ms` | `u64` | Wait after power-on before the next step. |
| `ready_timeout_ms` | `u64` | How long to wait for the device to answer (0: do not wait). |
| `off_delay_ms` | `u64` | A wait before removing power in an orderly power-off (a tube amplifier cooling). |
| `external_deadman` | `bool` | The owner states that this device has a stop path that does not depend on its power channel's driver: a hardware dead-man, or a plug-level auto-off timer. Required for an amplifier powered through a channel of the lowest assurance level (Home Assistant). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `device` (src/model.rs:473), `classify_on` (src/station.rs:238), `device` (src/station.rs:300), `check_power_on` (src/station.rs:431), `power_kind` (src/station.rs:479), `wait_ready` (src/station.rs:489).

**See also:** test `the_sample_station_parses_and_orders (src/model.rs)`.

### `ProfileStep` [@ANCHOR: radioctl:radioctl_station_model_profile_step]

**Purpose and location.** One step of a profile. Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:294` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq, Serialize, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Power` | `Power { device: DeviceId, }` | Power a device on (its dependencies must already be on). The device. |
| `Select` | `Select { channel: ChannelId, port: u32, }` | Select a way on a selector channel. The channel. The way, from 1. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_sample_station_parses_and_orders (src/model.rs)`.

### `ProfileDef` [@ANCHOR: radioctl:radioctl_station_model_profile_def]

**Purpose and location.** A named group, for example "HF (high frequency, 3 to 30 MHz) station on". Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:311` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | Its name. |
| `steps` | `Vec<ProfileStep>` | The steps in power-on order; power-off runs the power steps in reverse. |
| `timeout_ms` | `u64` | The longest the whole sequence may take. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `profile` (src/station.rs:622).

**See also:** test `the_sample_station_parses_and_orders (src/model.rs)`.

### `PowerMeterDef` [@ANCHOR: radioctl:radioctl_station_model_power_meter_def]

**Purpose and location.** An external RF (radio frequency) power meter: "the power meter for rig X" (`docs/DESIGN.md` decision 26). Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:327` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `id` | `String` | Its name, reported as the source of its readings. |
| `for_rig` | `String` | The rig whose transmit power it measures (the rig's name in the daemon configuration). |
| `driver` | `String` | The driver: `kpa500` or `kpa1500`. |
| `port` | `String` | The serial port. |
| `baud` | `Option<u32>` | The serial speed; the driver's default when absent. |
| `unit` | `Option<String>` | The unit the meter reports in: `w`, `dbm`, `dbw` or `dbmv50`; the driver's own when absent. |
| `offset_db_tenths` | `i32` | Loss of the coupler and attenuator in front of the meter, in tenths of a decibel (positive: the meter reads lower than the line). The amplifier meters need none. |
| `stale_ms` | `u64` | A reading older than this is no power. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `power_meters_are_validated_one_per_rig (src/model.rs)`.

### `StationDef` [@ANCHOR: radioctl:radioctl_station_model_station_def]

**Purpose and location.** The whole station. Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:404` as a pub `struct`. It derives Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `power_meters` | `Vec<PowerMeterDef>` | External RF power meters, one per rig at most. |
| `channels` | `Vec<ChannelDef>` | Switch channels. |
| `devices` | `Vec<DeviceDef>` | Devices. |
| `profiles` | `Vec<ProfileDef>` | Profiles. |
| `band_routes` | `Vec<BandRouteDef>` | Band routes: which way of a selector channel serves which band (`[[band_routes]]`). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `from_toml` (src/model.rs:439), `def` (src/station.rs:189).

**See also:** test `the_sample_station_parses_and_orders (src/model.rs)`.

### `BandRouteDef` [@ANCHOR: radioctl:radioctl_station_model_band_route_def]

**Purpose and location.** One `[[band_routes]]` entry: transmit frequencies from `low_hz` to `high_hz` inclusive select `port` of the selector `channel`. Defined in `daemons/radioctl/crates/radioctl-station/src/model.rs:425` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `channel` | `ChannelId` | The selector channel. |
| `low_hz` | `u64` | Lowest frequency of the band, in hertz. |
| `high_hz` | `u64` | Highest frequency of the band, in hertz. |
| `port` | `u32` | The way (from 1) to select. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_band_route_must_name_a_selector_way_and_a_sane_band (tests/band_follow.rs)`.

### `MeterDriver` [@ANCHOR: radioctl:radioctl_station_power_meter_meter_driver]

**Purpose and location.** Which wattmeter protocol a meter speaks. Defined in `daemons/radioctl/crates/radioctl-station/src/power_meter.rs:32` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Kpa500` | `Kpa500` | The Elecraft KPA500 amplifier's `^WS` read. |
| `Kpa1500` | `Kpa1500` | The Elecraft KPA1500 amplifier's `^PWF` and `^PWR` reads. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_name` (src/power_meter.rs:51), `new` (src/power_meter.rs:157), `serve_virtual_meter` (src/power_meter.rs:352).

**See also:** test `drivers_have_names_requests_and_units (src/power_meter.rs)`.

### `Sample` [@ANCHOR: radioctl:radioctl_station_power_meter_sample]

**Purpose and location.** What one poll learned, in the driver's unit. Defined in `daemons/radioctl/crates/radioctl-station/src/power_meter.rs:88` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Default.

| Field | Type | Meaning |
|---|---|---|
| `forward` | `f64` | Forward power. |
| `reflected` | `Option<f64>` | Reflected power, when the meter reports it. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_kpa500` (src/power_meter.rs:109), `poll` (src/power_meter.rs:199), `push` (src/power_meter.rs:266).

**See also:** test `a_stale_reading_is_no_power (src/power_meter.rs)`.

### `MeterPoller` [@ANCHOR: radioctl:radioctl_station_power_meter_meter_poller]

**Purpose and location.** An async poller for one external wattmeter on a byte stream. Defined in `daemons/radioctl/crates/radioctl-station/src/power_meter.rs:148` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `driver` | `MeterDriver` | The meter's protocol driver. |
| `io` | `Mutex<T>` | The byte stream, behind an async mutex. |
| `reply_timeout` | `Duration` | How long a reply may take. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/power_meter.rs:157).

**See also:** test `pollers_read_both_meters_from_virtual_devices (src/power_meter.rs)`.

### `Clock` [@ANCHOR: radioctl:radioctl_station_power_meter_clock]

**Purpose and location.** A source of the current time, replaceable in tests. Defined in `daemons/radioctl/crates/radioctl-station/src/power_meter.rs:220` as a pub `type`. It derives no traits.

**Definition.** `pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `manual_clock` (src/power_meter.rs:521).

**See also:** test `the_poller_task_keeps_a_feed_fresh_until_the_meter_goes_quiet (src/power_meter.rs)`.

### `FeedState` [@ANCHOR: radioctl:radioctl_station_power_meter_feed_state]

**Purpose and location.** A stored reading and when it arrived. Defined in `daemons/radioctl/crates/radioctl-station/src/power_meter.rs:222` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `at` | `Instant` | When the reading arrived. |
| `sample` | `Sample` | The reading. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_poller_task_keeps_a_feed_fresh_until_the_meter_goes_quiet (src/power_meter.rs)`.

### `PowerFeed` [@ANCHOR: radioctl:radioctl_station_power_meter_power_feed]

**Purpose and location.** The latest reading of one external meter, with the rule that makes a stale one "no power". Defined in `daemons/radioctl/crates/radioctl-station/src/power_meter.rs:228` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | The meter's name. |
| `unit` | `PowerUnit` | The unit the readings are in. |
| `offset_db` | `f64` | A calibration offset in dB applied to each reading. |
| `stale_after` | `Duration` | The age after which a reading counts as no power. |
| `clock` | `Clock` | The clock used to age readings. |
| `state` | `StdMutex<Option<FeedState>>` | The last reading, behind a mutex. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/power_meter.rs:241).

**See also:** test `a_stale_reading_is_no_power (src/power_meter.rs)`.

### `VirtualPower` [@ANCHOR: radioctl:radioctl_station_power_meter_virtual_power]

**Purpose and location.** What a virtual meter reports. Defined in `daemons/radioctl/crates/radioctl-station/src/power_meter.rs:323` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `forward_w` | `u32` | Forward watts. |
| `reflected_w` | `u32` | Reflected watts (KPA1500). |
| `swr_tenths` | `u32` | SWR in tenths (KPA500). |
| `silent` | `bool` | The device is off or silent: it answers nothing. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `set` (src/power_meter.rs:343).

**See also:** test `pollers_read_both_meters_from_virtual_devices (src/power_meter.rs)`.

### `VirtualMeterControl` [@ANCHOR: radioctl:radioctl_station_power_meter_virtual_meter_control]

**Purpose and location.** Control of a virtual meter. Defined in `daemons/radioctl/crates/radioctl-station/src/power_meter.rs:336` as a pub `struct`. It derives Clone.

| Field | Type | Meaning |
|---|---|---|
| `state` | `Arc<StdMutex<VirtualPower>>` | The simulated meter's state, shared with the meter. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `serve_virtual_meter` (src/power_meter.rs:352).

**See also:** test `pollers_read_both_meters_from_virtual_devices (src/power_meter.rs)`.

### `Identity` [@ANCHOR: radioctl:radioctl_station_record_identity]

**Purpose and location.** How to find the radio. Defined in `daemons/radioctl/crates/radioctl-station/src/record.rs:28` as a pub `struct`. It derives Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `usb_serial` | `Option<String>` | USB serial number, when it has one. |
| `network_address` | `Option<String>` | Network address (host or IP, optionally `:port`), when it has one. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `one_bad_record_is_dropped_and_the_rest_kept (src/record.rs)`.

### `ConnectMethod` [@ANCHOR: radioctl:radioctl_station_record_connect_method]

**Purpose and location.** How the relay connects after the radio is powered. Defined in `daemons/radioctl/crates/radioctl-station/src/record.rs:40` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Hotplug` | `Hotplug` | Wait for the USB hotplug event. |
| `Ping` | `Ping` | Ping the network address until it answers. |
| `BootTime` | `BootTime` | Wait the expected boot time, then try. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `one_bad_record_is_dropped_and_the_rest_kept (src/record.rs)`.

### `ConnectProcedure` [@ANCHOR: radioctl:radioctl_station_record_connect_procedure]

**Purpose and location.** The connect procedure. Defined in `daemons/radioctl/crates/radioctl-station/src/record.rs:51` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `method` | `ConnectMethod` | What to wait for before connecting. |
| `notes` | `String` | Free text for the owner (for example "set CI-V address 94h, 19200 baud"). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `new` (src/record.rs:146).

**See also:** test `one_bad_record_is_dropped_and_the_rest_kept (src/record.rs)`.

### `KnownState` [@ANCHOR: radioctl:radioctl_station_record_known_state]

**Purpose and location.** The state last read from the radio, with when. Served with its age while the radio is off. Defined in `daemons/radioctl/crates/radioctl-station/src/record.rs:61` as a pub `struct`. It derives Debug, Clone, Default, PartialEq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `frequency_hz` | `Option<u64>` | Frequency in hertz. |
| `mode` | `Option<String>` | Mode name. |
| `filter` | `Option<String>` | Filter name or width. |
| `levels` | `BTreeMap<String, f64>` | Levels by name (power, AF gain and so on). |
| `antenna` | `Option<String>` | The antenna selected. |
| `seen_unix` | `u64` | Unix seconds when this was read from the radio. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `connect` (src/actor_link.rs:252), `connect` (src/manager.rs:39), `connect` (src/manager.rs:79), `set_report` (src/manager.rs:148), `connect` (src/manager.rs:212), `connect_step` (src/manager.rs:560).

**See also:** test `the_power_button_walks_a_radio_up_and_reports_without_restoring (src/manager.rs)`.

### `Health` [@ANCHOR: radioctl:radioctl_station_record_health]

**Purpose and location.** Health counters. Defined in `daemons/radioctl/crates/radioctl-station/src/record.rs:83` as a pub `struct`. It derives Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `last_seen_unix` | `Option<u64>` | Unix seconds when the link was last up. |
| `failures` | `u32` | Failed connects and drops, all time. |
| `consecutive_failures` | `u32` | Failures since the last success. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `one_bad_record_is_dropped_and_the_rest_kept (src/record.rs)`.

### `RadioRecord` [@ANCHOR: radioctl:radioctl_station_record_radio_record]

**Purpose and location.** Everything durable about one radio. Defined in `daemons/radioctl/crates/radioctl-station/src/record.rs:97` as a pub `struct`. It derives Debug, Clone, PartialEq, Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `id` | `DeviceId` | The device id in the station definition; the key in the file. |
| `make` | `String` | Manufacturer. |
| `model` | `String` | Model. |
| `identity` | `Identity` | How to find it. |
| `description` | `String` | What the owner calls it. |
| `capabilities` | `Vec<String>` | Capability names from the last connection. |
| `last_state` | `Option<KnownState>` | The state last read, if ever. |
| `lifecycle` | `RadioState` | The lifecycle state when the record was last written. |
| `lifecycle_unix` | `u64` | Unix seconds when `lifecycle` was recorded. |
| `power` | `Option<PowerSource>` | How it is powered (a switch channel or a momentary line); `None`: no power button. |
| `boot_time_ms` | `u64` | Expected boot time in milliseconds. |
| `connect` | `ConnectProcedure` | The connect procedure. |
| `health` | `Health` | Health. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `record` (src/manager.rs:423), `new` (src/record.rs:146), `save` (src/record.rs:350), `sample` (src/record.rs:410), `all` (src/record.rs:435).

**See also:** test `a_record_round_trips_with_owner_only_permissions (src/record.rs)`.

### `Cached` [@ANCHOR: radioctl:radioctl_station_record_cached_type]

**Purpose and location.** The cached state with its age. Defined in `daemons/radioctl/crates/radioctl-station/src/record.rs:136` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `state` | `KnownState` | The state. |
| `age` | `Duration` | How old it is. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `cached` (src/manager.rs:429), `cached` (src/record.rs:202).

**See also:** test `one_bad_record_is_dropped_and_the_rest_kept (src/record.rs)`.

### `Loaded` [@ANCHOR: radioctl:radioctl_station_record_loaded]

**Purpose and location.** What a load found. Defined in `daemons/radioctl/crates/radioctl-station/src/record.rs:212` as a pub `struct`. It derives Debug, Clone, Default, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `records` | `BTreeMap<DeviceId, RadioRecord>` | The trusted records by id. |
| `problems` | `Vec<String>` | Things the loader had to do or drop, in words. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `load` (src/record.rs:249).

**See also:** test `one_bad_record_is_dropped_and_the_rest_kept (src/record.rs)`.

### `FileImage` [@ANCHOR: radioctl:radioctl_station_record_file_image]

**Purpose and location.** The JSON shape of the record file. Defined in `daemons/radioctl/crates/radioctl-station/src/record.rs:220` as a private `struct`. It derives Serialize, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `schema` | `u32` | The file schema version. |
| `radios` | `BTreeMap<String, serde_json::Value>` | Each radio's record as JSON, by id; one bad record is dropped on load without losing the rest. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_world_readable_file_is_tightened_and_a_huge_one_refused (src/record.rs)`.

### `RecordStore` [@ANCHOR: radioctl:radioctl_station_record_record_store]

**Purpose and location.** The record file. Defined in `daemons/radioctl/crates/radioctl-station/src/record.rs:227` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `path` | `PathBuf` | The record file path; the file is written owner-only. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/record.rs:234).

**See also:** test `a_record_round_trips_with_owner_only_permissions (src/record.rs)`.

### `RotatorProtocol` [@ANCHOR: radioctl:radioctl_station_rotator_rotator_protocol]

**Purpose and location.** Which wire protocol the rotator speaks. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:36` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Rotctld` | `Rotctld` | Hamlib's `rotctld` text protocol. |
| `Gs232` | `Gs232` | Yaesu GS-232A/B ASCII. |
| `Easycomm1` | `Easycomm1` | Easycomm I: set-only `AZaaa.a ELeee.e` lines. |
| `Easycomm2` | `Easycomm2` | Easycomm II: `AZ`/`EL` set and read, `SA SE` stop. |
| `Easycomm3` | `Easycomm3` | Easycomm III: the Easycomm II commands radioctl uses (the velocity, status and configuration extensions are not used). |
| `Rot2Prog` | `Rot2Prog` | SPID Rot2Prog / MD-01/02 (13-byte command frames, 12-byte replies). |
| `Dcu` | `Dcu` | Hy-Gain DCU-1/DCU-2/DCU-3 (`AP1nnn;` target, `AM1;` start); azimuth only, no read. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `easycomm_encodes_and_parses_the_specification_shapes (src/rotator.rs)`.

### `ParkPosition` [@ANCHOR: radioctl:radioctl_station_rotator_park_position]

**Purpose and location.** A park position. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:57` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `az` | `f32` | Azimuth, degrees. |
| `el` | `f32` | Elevation, degrees. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_shipped_descriptions_are_valid_and_bad_ones_are_refused (src/rotator.rs)`.

### `RotatorDescription` [@ANCHOR: radioctl:radioctl_station_rotator_rotator_description]

**Purpose and location.** What a rotator is and can do. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:66` as a pub `struct`. It derives Debug, Clone, PartialEq, Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `id` | `String` | A short unique name. |
| `name` | `String` | A display name. |
| `protocol` | `RotatorProtocol` | The wire protocol. |
| `az_min` | `f32` | Lowest azimuth, degrees. |
| `az_max` | `f32` | Highest azimuth, degrees (above 360 for an overlap). |
| `el_min` | `f32` | Lowest elevation, degrees. |
| `el_max` | `f32` | Highest elevation, degrees. |
| `has_elevation` | `bool` | Whether there is an elevation axis. |
| `slew_deg_per_s` | `f32` | Typical slew rate, degrees per second (informational; the virtual rotator uses it). |
| `park` | `Option<ParkPosition>` | Where `park` goes, if the rotator has one. |
| `reply_timeout_ms` | `u64` | Longest wait for a reply. |
| `pulses_per_degree` | `Option<u8>` | Pulses per degree the controller counts in (Rot2Prog: 1 to 10; a reply carries the controller's own value, which is used to read it). |
| `verified` | `bool` | Whether any of this has been checked on the rotator. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `parse_all` (src/rotator.rs:189), `builtin` (src/rotator.rs:207), `description` (src/rotator.rs:282), `target` (src/rotator.rs:297), `new` (src/rotator.rs:344), `description` (src/rotator.rs:368), `new` (src/rotator.rs:470), `description` (src/rotator.rs:495).

**See also:** test `the_shipped_descriptions_are_valid_and_bad_ones_are_refused (src/rotator.rs)`.

### `RotatorFile` [@ANCHOR: radioctl:radioctl_station_rotator_rotator_file]

**Purpose and location.** The TOML shape of a rotator description file. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:100` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `rotators` | `Vec<RotatorDescription>` | The rotator descriptions in the file. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_stuck_rotator_is_reported_with_where_it_was_seen (tests/rotator.rs)`.

### `RotOp` [@ANCHOR: radioctl:radioctl_station_rotator_rot_op]

**Purpose and location.** A rotator command. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:229` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `SetPosition` | `SetPosition { az: f32, el: f32, }` | Move to a position. Azimuth, degrees. Elevation, degrees. |
| `GetPosition` | `GetPosition` | Read the position. |
| `Stop` | `Stop` | Stop moving. |
| `Park` | `Park` | Go to the park position. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `encode` (src/rotator.rs:284), `target` (src/rotator.rs:297), `encode` (src/rotator.rs:373), `encode` (src/rotator.rs:500), `encode` (src/rotator.rs:617), `encode` (src/rotator.rs:754), `encode` (src/rotator.rs:834), `transact` (src/rotator.rs:982).

**See also:** test `positions_outside_the_travel_are_refused_not_clamped (src/rotator.rs)`.

### `RotExpect` [@ANCHOR: radioctl:radioctl_station_rotator_rot_expect]

**Purpose and location.** What reply a command expects. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:247` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Nothing` | `Nothing` | Nothing comes back. |
| `Status` | `Status` | A status line. |
| `Position` | `Position` | A position. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `rotctld_encodes_and_parses (src/rotator.rs)`.

### `Encoded` [@ANCHOR: radioctl:radioctl_station_rotator_encoded]

**Purpose and location.** A command encoded for the wire. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:258` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `bytes` | `Vec<u8>` | The bytes to write. |
| `expect` | `RotExpect` | The reply to wait for. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `encode` (src/rotator.rs:284), `encode` (src/rotator.rs:373), `encode` (src/rotator.rs:500), `encode` (src/rotator.rs:617), `encode` (src/rotator.rs:754), `encode` (src/rotator.rs:834).

**See also:** test `rotctld_encodes_and_parses (src/rotator.rs)`.

### `RotReply` [@ANCHOR: radioctl:radioctl_station_rotator_rot_reply]

**Purpose and location.** A parsed reply. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:267` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Done` | `Done` | The command was accepted. |
| `Position` | `Position { az: f32, el: f32, }` | The position. Azimuth, degrees. Elevation, degrees. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `easycomm_position` (src/rotator.rs:567), `transact` (src/rotator.rs:982).

**See also:** test `rotctld_encodes_and_parses (src/rotator.rs)`.

### `RotctldEngine` [@ANCHOR: radioctl:radioctl_station_rotator_rotctld_engine]

**Purpose and location.** Hamlib's `rotctld` protocol. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:337` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RotatorDescription` | The rotator description. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/rotator.rs:344).

**See also:** test `positions_outside_the_travel_are_refused_not_clamped (src/rotator.rs)`.

### `Gs232Engine` [@ANCHOR: radioctl:radioctl_station_rotator_gs232_engine]

**Purpose and location.** The Yaesu GS-232A/B ASCII protocol. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:463` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RotatorDescription` | The rotator description. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/rotator.rs:470).

**See also:** test `positions_outside_the_travel_are_refused_not_clamped (src/rotator.rs)`.

### `EasycommEngine` [@ANCHOR: radioctl:radioctl_station_rotator_easycomm_engine]

**Purpose and location.** The Easycomm I, II and III ASCII protocols (Chris Jackson G7UPN; Easycomm III by Alexander Schultze), from the specification the AMSAT WiSP programs ship and Hamlib carries: `AZaaa.a ELeee.e` moves, a bare `AZ EL` reads (the controller echoes `AZaaa.a ELeee.e`), `SA SE` stops both axes. Easycomm I has no read. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:550` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RotatorDescription` | The rotator description. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/rotator.rs:557).

**See also:** test `easycomm_encodes_and_parses_the_specification_shapes (src/rotator.rs)`.

### `Rot2ProgEngine` [@ANCHOR: radioctl:radioctl_station_rotator_rot2_prog_engine]

**Purpose and location.** The SPID Rot2Prog / MD-01/02 binary protocol, from SPID's "ROT2PROG Protocol" sheet: every command is 13 bytes `W` + 10 payload bytes + command + `0x20` (set: two groups of four ASCII digits of `(360 + angle) * pulses` each followed by the pulses-per-degree byte, command `0x2F`; status `0x1F`; stop `0x0F`), and status and stop answer 12 bytes `W` + four digit bytes + PH + four digit bytes + PV + `0x20`. A digit byte is read as ASCII (`0x30` to `0x39`) or as a plain 0 to 9, because the sheet and Hamlib disagree on which a controller sends. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:682` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RotatorDescription` | The rotator description. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/rotator.rs:689).

**See also:** test `rot2prog_encodes_and_parses_the_sheet_frames (src/rotator.rs)`.

### `DcuEngine` [@ANCHOR: radioctl:radioctl_station_rotator_dcu_engine]

**Purpose and location.** The Hy-Gain DCU-1/DCU-2/DCU-3 ASCII protocol, from Hy-Gain's DCU-3 manual ("Command Set": `AP1nnn;` sets the target bearing, three digits 000 to 359, and `AM1;` starts the rotation to it; 4800 baud 8N1; the controller sends nothing back). Both commands go out in one write. Stop is `AS1;`, which Hamlib's DCU-1 backend uses and the DCU-3 manual does not list: a bench has to confirm it. There is no position read, so `GetPosition` is refused. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:815` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RotatorDescription` | The rotator description. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/rotator.rs:822).

**See also:** test `dcu_encodes_the_manual_commands_and_reads_nothing (src/rotator.rs)`.

### `Rotator` [@ANCHOR: radioctl:radioctl_station_rotator_rotator]

**Purpose and location.** A rotator on a transport: commands are serialised, every wait is bounded, and a down link fails at once. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:874` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `engine` | `Box<dyn RotatorEngine>` | The protocol engine for the model. |
| `io` | `Mutex<Box<dyn Transport>>` | The transport, behind an async mutex that serializes commands. |
| `link` | `Option<watch::Receiver<LinkStatus>>` | The transport's link status, if it reports one; a down link fails a command at once. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `open` (tests/rotator_pty.rs:64).

**See also:** test `a_down_link_fails_at_once_without_writing_and_recovers (tests/rotator.rs)`.

### `SharedRotator` [@ANCHOR: radioctl:radioctl_station_rotator_shared_rotator]

**Purpose and location.** A rotator shared between tasks. Defined in `daemons/radioctl/crates/radioctl-station/src/rotator.rs:1066` as a pub `type`. It derives no traits.

**Definition.** `pub type SharedRotator = Arc<Rotator>;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_stuck_rotator_is_reported_with_where_it_was_seen (tests/rotator.rs)`.

### `FramingFactory` [@ANCHOR: radioctl:radioctl_station_spawner_framing_factory]

**Purpose and location.** Makes a fresh framing for each new connection. Defined in `daemons/radioctl/crates/radioctl-station/src/spawner.rs:28` as a pub `type`. It derives no traits.

**Definition.** `pub type FramingFactory = Arc<dyn Fn() -> Box<dyn Framing> + Send + Sync>;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `an_unknown_radio_and_an_unreachable_one_are_clean_errors (tests/spawner.rs)`.

### `RigEndpoint` [@ANCHOR: radioctl:radioctl_station_spawner_rig_endpoint]

**Purpose and location.** Where a radio is. Defined in `daemons/radioctl/crates/radioctl-station/src/spawner.rs:32` as a pub `enum`. It derives Clone.

| Variant | Definition | Meaning |
|---|---|---|
| `Tcp` | `Tcp { address: String, framing: Option<FramingFactory>, config: NetConfig, }` | A TCP address (`host:port`), with the framing it speaks (`None`: the raw CAT stream). `host:port`. The framing, if the radio has a session of its own (the TS-890S LAN login). The network link settings (timeouts, backoff, silence limit). |
| `Serial` | `Serial { target: PortTarget, baud: u32, }` | A serial port. The port. The speed. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_network_radio_keeps_its_actor_across_a_network_drop_and_is_unkeyed_by_it (tests/spawner.rs)`.

### `RigSpec` [@ANCHOR: radioctl:radioctl_station_spawner_rig_spec]

**Purpose and location.** One radio: its description, where it is, and how its actor is configured. Defined in `daemons/radioctl/crates/radioctl-station/src/spawner.rs:53` as a pub `struct`. It derives Clone.

| Field | Type | Meaning |
|---|---|---|
| `description` | `RigDescription` | The description (rig file) the engine is built from. |
| `endpoint` | `RigEndpoint` | Where the radio is. |
| `options` | `RigOptions` | The actor's options (the link watch is set by the spawner). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `tcp` (src/spawner.rs:65), `serial` (src/spawner.rs:80), `add` (src/spawner.rs:107).

**See also:** test `a_network_radio_keeps_its_actor_across_a_network_drop_and_is_unkeyed_by_it (tests/spawner.rs)`.

### `DescriptionSpawner` [@ANCHOR: radioctl:radioctl_station_spawner_description_spawner]

**Purpose and location.** Spawns the rigs of a station from their specs. Defined in `daemons/radioctl/crates/radioctl-station/src/spawner.rs:94` as a pub `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `specs` | `BTreeMap<DeviceId, RigSpec>` | The spec of each rig by device id. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/spawner.rs:101).

**See also:** test `a_network_radio_keeps_its_actor_across_a_network_drop_and_is_unkeyed_by_it (tests/spawner.rs)`.

### `KeyState` [@ANCHOR: radioctl:radioctl_station_station_key_state]

**Purpose and location.** Whether a transmitter is keyed. Defined in `daemons/radioctl/crates/radioctl-station/src/station.rs:41` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Unkeyed` | `Unkeyed` | Known not to be transmitting. |
| `Keyed` | `Keyed` | Transmitting, or its PTT reads on. |
| `Unknown` | `Unknown` | Could not be determined. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `key_state` (src/actor_link.rs:219), `key_state` (src/manager.rs:939), `key_state` (src/station.rs:55), `key_state` (src/station.rs:69), `key_state` (src/station.rs:702), `key_state` (tests/amplifier.rs:70).

**See also:** test `a_keyed_virtual_rig_blocks_switching (tests/station_rig.rs)`.

### `AllOffReport` [@ANCHOR: radioctl:radioctl_station_station_all_off_report]

**Purpose and location.** The result of an all-off. Defined in `daemons/radioctl/crates/radioctl-station/src/station.rs:98` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `off` | `Vec<ChannelId>` | Power channels driven off, in the order done. |
| `failed` | `Vec<(ChannelId, String)>` | Power channels that could not be driven off, with the reason. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `all_off` (src/station.rs:638).

**See also:** test `all_off_is_never_blocked (src/station.rs)`.

### `Action` [@ANCHOR: radioctl:radioctl_station_station_action]

**Purpose and location.** What a command does to the RF path, which decides how the keyed state treats it (Bruce, 2026-10-31). Defined in `daemons/radioctl/crates/radioctl-station/src/station.rs:108` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `PowerOff` | `PowerOff` | Powers a device off (an amplifier's standby included): it can only stop a transmission, so it passes every keyed state, known keyed and unknown alike. |
| `PowerOnRadio` | `PowerOnRadio` | Powers on a radio whose power channel is not a shared supply: an off radio cannot be keyed, so it passes every keyed state. |
| `RfPath` | `RfPath` | Anything that could hot-switch an RF path (antenna, amplifier power-on, band, outputs): refused while a rig is known keyed or its keyed state is unknown. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `guard_action` (src/station.rs:206), `power_on_action` (src/station.rs:231), `classify_on` (src/station.rs:238), `classify_channel` (src/station.rs:259), `classify_pulse` (src/station.rs:286).

**See also:** test `the_shared_supply_protection_is_opt_in_and_makes_radio_power_on_an_rf_path_change (src/station.rs)`.

### `Station` [@ANCHOR: radioctl:radioctl_station_station_station]

**Purpose and location.** A station: the definition, its switchboard and its interlock sources. Defined in `daemons/radioctl/crates/radioctl-station/src/station.rs:121` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `def` | `StationDef` | The station definition. |
| `board` | `Arc<Switchboard>` | The switchboard (shared). |
| `keyed` | `Vec<Arc<dyn KeyedSource>>` | The sources that say whether a radio is keyed (the interlock). |
| `ready` | `Option<Arc<dyn Readiness>>` | A readiness check for the radios, if configured. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `rig` (src/station.rs:727), `rig_modbus` (src/station.rs:739), `world` (tests/amplifier.rs:88), `powered` (tests/amplifier.rs:101), `rig_and_station` (tests/band_follow.rs:41), `rig_and_station_from` (tests/band_follow.rs:47), `way` (tests/band_follow.rs:75), `wait_for_way` (tests/band_follow.rs:82).

**See also:** test `the_shared_supply_protection_is_opt_in_and_makes_radio_power_on_an_rf_path_change (src/station.rs)`.

### `HaTls` [@ANCHOR: radioctl:radioctl_station_tls_ha_tls]

**Purpose and location.** How to trust a Home Assistant over TLS. Exactly one of the two fields is set. Defined in `daemons/radioctl/crates/radioctl-station/src/tls.rs:28` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `ca_pem` | `Option<String>` | The PEM text of the owner's certificate authority (or of the server's own self-signed certificate). |
| `pin_sha256` | `Option<String>` | The SHA-256 fingerprint of the server certificate, 64 hex digits (colons and spaces are ignored). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `connector` (src/tls.rs:126).

**See also:** test `the_connector_needs_exactly_one_way_to_trust (src/tls.rs)`.

### `PinVerifier` [@ANCHOR: radioctl:radioctl_station_tls_pin_verifier]

**Purpose and location.** Accepts exactly the pinned certificate; the handshake signature is still checked against its key, so the peer must hold the private key. Defined in `daemons/radioctl/crates/radioctl-station/src/tls.rs:64` as a private `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `pin` | `[u8; 32]` | The SHA-256 of the one certificate to accept. |
| `provider` | `Arc<rustls::crypto::CryptoProvider>` | The crypto provider used to check the handshake signature. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `only_the_pinned_certificate_is_accepted (src/tls.rs)`.

### `RotatorState` [@ANCHOR: radioctl:radioctl_station_virtual_rotator_rotator_state]

**Purpose and location.** What the virtual rotator is doing. Defined in `daemons/radioctl/crates/radioctl-station/src/virtual_rotator.rs:23` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `az` | `f32` | Where it is now, degrees. |
| `el` | `f32` | Where it is now, degrees. |
| `target_az` | `f32` | Where it is going. |
| `target_el` | `f32` | Where it is going. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `state` (src/virtual_rotator.rs:101), `state` (src/virtual_rotator.rs:377).

**See also:** test `it_speaks_rot2prog (src/virtual_rotator.rs)`.

### `VirtualRotator` [@ANCHOR: radioctl:radioctl_station_virtual_rotator_virtual_rotator]

**Purpose and location.** The sans-IO virtual rotator. Defined in `daemons/radioctl/crates/radioctl-station/src/virtual_rotator.rs:35` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RotatorDescription` | The rotator description. |
| `state` | `RotatorState` | The simulated position and motion. |
| `last` | `Duration` | The time of the last update. |
| `stuck` | `bool` | Accepts commands and never moves. |
| `silent` | `bool` | Never answers. |
| `raw_digits` | `bool` | Rot2Prog replies carry digit bytes 0 to 9 (as Hamlib reads them) instead of ASCII digits. |
| `pending` | `f32` | The bearing set by a DCU `AP1`, started by `AM1`. |
| `received` | `Vec<String>` | The commands received, as text. |
| `buffer` | `Vec<u8>` | Bytes received but not yet a whole command. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/virtual_rotator.rs:66), `rot` (src/virtual_rotator.rs:459), `pty_rotator` (tests/rotator_pty.rs:31).

**See also:** test `hamlibs_rotctl_drives_the_virtual_easycomm_rotators (tests/rotator_pty.rs)`.

### `VirtualRotatorControl` [@ANCHOR: radioctl:radioctl_station_virtual_rotator_virtual_rotator_control]

**Purpose and location.** Control of a served virtual rotator. Defined in `daemons/radioctl/crates/radioctl-station/src/virtual_rotator.rs:366` as a pub `struct`. It derives Clone.

| Field | Type | Meaning |
|---|---|---|
| `rotator` | `Arc<Mutex<VirtualRotator>>` | The simulated rotator, shared under a mutex. |
| `epoch` | `Instant` | The start instant for motion timing. |
| `task` | `Arc<Mutex<Option<JoinHandle<()>>>>` | The listener task, taken by the first `stop`. |
| `address` | `Option<SocketAddr>` | The address the listener is bound to, if served. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, JoinHandle, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `hamlibs_rotctl_drives_the_virtual_easycomm_rotators (tests/rotator_pty.rs)`.

### `Opcode` [@ANCHOR: radioctl:radioctl_station_ws_opcode]

**Purpose and location.** A WebSocket frame type. Defined in `daemons/radioctl/crates/radioctl-station/src/ws.rs:172` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Continuation` | `Continuation` | A continuation of a fragmented message. |
| `Text` | `Text` | UTF-8 text. |
| `Binary` | `Binary` | Binary data. |
| `Close` | `Close` | Close. |
| `Ping` | `Ping` | Ping. |
| `Pong` | `Pong` | Pong. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_code` (src/ws.rs:203), `encode_frame` (src/ws.rs:230).

**See also:** test `frames_round_trip_masked_unmasked_fragmented_and_byte_at_a_time (src/ws.rs)`.

### `Frame` [@ANCHOR: radioctl:radioctl_station_ws_frame]

**Purpose and location.** One frame as read: final-fragment flag, type, unmasked payload. Defined in `daemons/radioctl/crates/radioctl-station/src/ws.rs:217` as a private `type`. It derives no traits.

**Definition.** `type Frame = (bool, Opcode, Vec<u8>);` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `frame` (src/ws.rs:322).

**See also:** test `frames_round_trip_masked_unmasked_fragmented_and_byte_at_a_time (src/ws.rs)`.

### `Message` [@ANCHOR: radioctl:radioctl_station_ws_message]

**Purpose and location.** A whole message (fragments joined) or control frame. Defined in `daemons/radioctl/crates/radioctl-station/src/ws.rs:221` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `opcode` | `Opcode` | Its type; never `Continuation`. |
| `payload` | `Vec<u8>` | Its payload. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `push` (src/ws.rs:275).

**See also:** test `frames_round_trip_masked_unmasked_fragmented_and_byte_at_a_time (src/ws.rs)`.

### `Decoder` [@ANCHOR: radioctl:radioctl_station_ws_decoder]

**Purpose and location.** Reassembles frames from a byte stream. Defined in `daemons/radioctl/crates/radioctl-station/src/ws.rs:255` as a pub `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `buffer` | `Vec<u8>` | Bytes received but not yet a whole frame. |
| `partial` | `Option<(Opcode, Vec<u8>)>` | A fragmented message in progress: its opcode and the bytes so far. |
| `max_message` | `usize` | The largest message accepted, in bytes. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/ws.rs:264).

**See also:** test `frames_round_trip_masked_unmasked_fragmented_and_byte_at_a_time (src/ws.rs)`.

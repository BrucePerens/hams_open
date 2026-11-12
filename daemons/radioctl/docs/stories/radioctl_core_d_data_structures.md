# Data structures

### `Backoff` [@ANCHOR: radioctl:radioctl_core_backoff_backoff]

**Purpose and location.** An exponential backoff schedule. Defined in `daemons/radioctl/crates/radioctl-core/src/backoff.rs:10` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `initial` | `Duration` | The wait after the first failure. |
| `max` | `Duration` | The longest wait. |
| `factor` | `u32` | Multiplier per further failure (2 doubles). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `default` (src/backoff.rs:20).

**See also:** test `delays_grow_to_the_ceiling_and_stay_there (src/backoff.rs)`.

### `ErrorKind` [@ANCHOR: radioctl:radioctl_core_error_error_kind]

**Purpose and location.** What went wrong, in terms a caller can act on. Defined in `daemons/radioctl/crates/radioctl-core/src/error.rs:9` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `Timeout` | `Timeout` | The rig did not answer before the command's deadline. |
| `Rejected` | `Rejected` | The rig answered with a refusal (CI-V NG, Kenwood `?;`). |
| `Unsupported` | `Unsupported` | The rig or its description does not support the operation. |
| `LinkDown` | `LinkDown` | The link to the rig is down or the actor has stopped. |
| `Cancelled` | `Cancelled` | The command was abandoned before completion (preempted or shut down). |
| `TransmitGateClosed` | `TransmitGateClosed` | A transmit-capable command was refused because the rig's transmit gate is closed. |
| `TransmitFrequencyUnknown` | `TransmitFrequencyUnknown` | A transmit-capable command was refused because the frequency the rig would transmit on is not known (split state or the transmit VFO's frequency never read). Fail closed: reading it (`RigHandle::active_tx_frequency`, or any poll) makes a retry succeed. |
| `Malformed` | `Malformed` | The rig sent bytes the protocol engine could not make sense of. |
| `QueueFull` | `QueueFull` | The rig's command queue is full. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/error.rs:59), `error` (src/rig.rs:1406).

**See also:** test `retry_hints_follow_the_kind (src/error.rs)`.

### `RetryHint` [@ANCHOR: radioctl:radioctl_core_error_retry_hint]

**Purpose and location.** Whether and when a caller may usefully try again. Defined in `daemons/radioctl/crates/radioctl-core/src/error.rs:34` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Never` | `Never` | Retrying the same command cannot succeed. |
| `Now` | `Now` | A retry may succeed at once. |
| `After` | `After(Duration)` | A retry may succeed after the given delay. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `retry_hints_follow_the_kind (src/error.rs)`.

### `RigError` [@ANCHOR: radioctl:radioctl_core_error_rig_error]

**Purpose and location.** The error type of every radioctl operation. Defined in `daemons/radioctl/crates/radioctl-core/src/error.rs:45` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `kind` | `ErrorKind` | The class of failure. |
| `retry` | `RetryHint` | Whether to retry. |
| `detail` | `String` | A short human-readable description. |
| `transcript_tail` | `Vec<String>` | The last wire lines (hex, direction-prefixed) before the failure, oldest first. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `read_request` (src/baud_probe.rs:67), `open` (src/baud_probe.rs:192), `encode` (src/engine.rs:81), `encode_lines` (src/engine.rs:92), `encode` (src/engine.rs:126), `encode_lines` (src/engine.rs:138), `encode` (src/engine.rs:166), `new` (src/error.rs:59).

**See also:** test `retry_hints_follow_the_kind (src/error.rs)`.

### `Source` [@ANCHOR: radioctl:radioctl_core_event_source]

**Purpose and location.** Where a change came from. Defined in `daemons/radioctl/crates/radioctl-core/src/event.rs:8` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `Api` | `Api` | An operation made through the library's API. |
| `Poll` | `Poll` | Found by the actor's polling. |
| `Transceive` | `Transceive` | An unsolicited frame from the rig (transceive, auto-information), or a reply that arrived after its command had been abandoned. |
| `Splitter` | `Splitter` | An operation that arrived through the protocol splitter. |
| `Watchdog` | `Watchdog` | The actor's own transmit watchdog: a forced unkey, or an unkey that failed. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `execute_from` (src/rig.rs:567), `alarm` (src/rig.rs:1286), `unkey_fail_closed` (src/rig.rs:1305), `apply_set` (src/rig.rs:1889), `observe` (src/rig.rs:1948), `next_change` (src/rig/easy.rs:136), `alarms` (tests/actor.rs:516).

**See also:** test `merge_keeps_earliest_old_and_latest_new (src/event.rs)`.

### `Change` [@ANCHOR: radioctl:radioctl_core_event_change]

**Purpose and location.** One changed field with its old and new value. Defined in `daemons/radioctl/crates/radioctl-core/src/event.rs:24` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `field` | `Field` | The field. |
| `old` | `Option<Observation>` | The cached value before, `None` the first time the field is seen. |
| `new` | `Observation` | The value now. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `freq` (src/event.rs:69), `next_change` (src/rig/easy.rs:136).

**See also:** test `merge_keeps_earliest_old_and_latest_new (src/event.rs)`.

### `RigEvent` [@ANCHOR: radioctl:radioctl_core_event_rig_event]

**Purpose and location.** A status change of one rig. Defined in `daemons/radioctl/crates/radioctl-core/src/event.rs:35` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `rig` | `String` | The rig's identifier. |
| `source` | `Source` | What caused the change. |
| `changes` | `Vec<Change>` | The changed fields. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `merge` (src/event.rs:48), `poll_event` (src/rig.rs:923), `next` (src/rig.rs:971), `poll_next` (src/rig.rs:987), `emit` (src/rig.rs:2002), `alarms` (tests/actor.rs:516), `drain` (tests/actor.rs:528), `stream_next` (tests/actor.rs:652).

**See also:** test `merge_keeps_earliest_old_and_latest_new (src/event.rs)`.

### `Op` [@ANCHOR: radioctl:radioctl_core_op_op]

**Purpose and location.** A command to a rig. Defined in `daemons/radioctl/crates/radioctl-core/src/op.rs:11` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `GetFrequency` | `GetFrequency(Vfo)` | Read a VFO's frequency. |
| `SetFrequency` | `SetFrequency(Vfo, Hz)` | Set a VFO's frequency. |
| `GetMode` | `GetMode` | Read the mode. |
| `SetMode` | `SetMode(Mode)` | Set the mode. |
| `GetPtt` | `GetPtt` | Read the transmit state. |
| `SetPtt` | `SetPtt(bool)` | Key (`true`) or unkey (`false`) the transmitter. |
| `GetSignal` | `GetSignal` | Read the S-meter (raw units). |
| `GetPower` | `GetPower` | Read the power meter (raw units). |
| `SendCw` | `SendCw(String)` | Send Morse text. |
| `StopCw` | `StopCw` | Stop Morse in progress: clear the rig's CW text buffer and end the element being sent. It is a stop path, so it jumps the queue like an unkey and needs no transmit gate. It does not unkey a rig held keyed by PTT; polling reconciles the transmit state after it. |
| `SetTransceive` | `SetTransceive(bool)` | Turn the rig's unsolicited reporting on or off. |
| `GetSplit` | `GetSplit` | Read split operation. |
| `GetTxVfo` | `GetTxVfo` | Read which VFO the rig transmits on. |
| `SetSplit` | `SetSplit(bool)` | Enter (`true`) or leave (`false`) split operation. |
| `GetRit` | `GetRit` | Read whether RIT is on. |
| `SetRit` | `SetRit(bool)` | Turn RIT on or off. |
| `GetXit` | `GetXit` | Read whether XIT is on. |
| `SetXit` | `SetXit(bool)` | Turn XIT on or off. |
| `GetRitOffset` | `GetRitOffset` | Read the RIT/XIT offset in hertz. |
| `SetRitOffset` | `SetRitOffset(i16)` | Set the RIT/XIT offset in hertz (the rig limits the range). |
| `GetFilterWidth` | `GetFilterWidth` | Read the receive filter bandwidth. |
| `SetFilterWidth` | `SetFilterWidth(Hz)` | Set the receive filter bandwidth. |
| `GetLevel` | `GetLevel(Level)` | Read a level. |
| `SetLevel` | `SetLevel(Level, u16)` | Set a level, in the rig's units. |
| `GetSwr` | `GetSwr` | Read the SWR meter (tenths of a unit). |
| `GetMeter` | `GetMeter(Meter)` | Read a transmitter or supply meter in the rig's raw scale (see [`Meter`]). Read-only: it works with the transmit gate closed and means most while the rig is keyed by other means. |
| `GetModelId` | `GetModelId` | Read the rig's model identifier. |
| `GetFirmware` | `GetFirmware` | Read the rig's main firmware revision. |
| `GetPowerAmplifier` | `GetPowerAmplifier` | Read whether the rig has its power amplifier module (Elecraft `OM`: the K3's KPA3, the KX2's and KX3's KXPA100). A power-range question, not a transmit one: read-only. |
| `SetScopeOutput` | `SetScopeOutput(bool)` | Switch the rig's waveform output (the scope sweeps it sends on its own) on or off. |
| `GetScopeOutput` | `GetScopeOutput` | Read whether waveform output is on. |
| `SetScopeAxis` | `SetScopeAxis(u8, ScopeAxis)` | Set how a receiver's scope axis is anchored (receiver 0 is Main, 1 is Sub). |
| `GetScopeAxis` | `GetScopeAxis(u8)` | Read a receiver's scope axis mode. |
| `SetScopeSpan` | `SetScopeSpan(u8, Hz)` | Set a receiver's scope span (the full width in hertz; the rig offers fixed choices). |
| `GetScopeSpan` | `GetScopeSpan(u8)` | Read a receiver's scope span. |
| `SetScopeEdges` | `SetScopeEdges(u8, u8, Hz, Hz)` | Set the fixed-mode edges: range number, edge number (1 based), lower and upper edge. |
| `SetScopeReference` | `SetScopeReference(u8, i16)` | Set a receiver's reference level, in hundredths of a dB. |
| `GetScopeReference` | `GetScopeReference(u8)` | Read a receiver's reference level. |
| `SetScopeSetting` | `SetScopeSetting(u8, ScopeSetting, u8)` | Set a receiver's scope setting to the rig's own choice code (see [`ScopeSetting`]). |
| `GetScopeSetting` | `GetScopeSetting(u8, ScopeSetting)` | Read a receiver's scope setting. |
| `ScopeAction` | `ScopeAction(u8, ScopeAction)` | Run a one-shot scope command on a receiver (see [`ScopeAction`]). |
| `SelectReceiver` | `SelectReceiver(Receiver)` | Make a receiver (band) the one later commands act on (Icom `07 D0`/`D1`). |
| `GetReceiver` | `GetReceiver` | Read which receiver (band) later commands act on. |
| `SetTxReceiver` | `SetTxReceiver(Receiver)` | Choose the band the transmitter follows (Yaesu `FT` on the dual-band FTDX models). Setting it does not key the rig. |
| `GetTxReceiver` | `GetTxReceiver` | Read the band the transmitter follows. |
| `SetDualWatch` | `SetDualWatch(bool)` | Turn dual watch (both receivers listening) on or off (Icom `07 C0`/`C1`). |
| `GetDualWatch` | `GetDualWatch` | Read whether dual watch is on. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `encode` (src/engine.rs:81), `encode_lines` (src/engine.rs:92), `encode` (src/engine.rs:126), `encode_lines` (src/engine.rs:138), `encode` (src/engine.rs:166), `execute` (src/rig.rs:549), `execute_within` (src/rig.rs:556), `execute_from` (src/rig.rs:567).

**See also:** test `observation_maps_to_its_field (src/op.rs)`.

### `Field` [@ANCHOR: radioctl:radioctl_core_op_field]

**Purpose and location.** A kind of value the actor caches. Defined in `daemons/radioctl/crates/radioctl-core/src/op.rs:116` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `Frequency` | `Frequency(Vfo)` | A VFO's frequency. |
| `Mode` | `Mode` | The mode. |
| `Ptt` | `Ptt` | The transmit state. |
| `Signal` | `Signal` | The S-meter. |
| `Power` | `Power` | The power meter. |
| `Split` | `Split` | Split operation. |
| `RxVfo` | `RxVfo` | The VFO the rig receives on, when it can say (Kenwood `IF`/`FR`); otherwise VFO A. |
| `TxVfo` | `TxVfo` | The VFO the rig transmits on. |
| `ActiveTxFrequency` | `ActiveTxFrequency` | The frequency the rig transmits on, derived by the actor from the other fields; no engine reports it and no command reads it. |
| `Rit` | `Rit` | RIT on or off. |
| `Xit` | `Xit` | XIT on or off. |
| `RitOffset` | `RitOffset` | The RIT/XIT offset. |
| `FilterWidth` | `FilterWidth` | The receive filter bandwidth. |
| `Level` | `Level(Level)` | A numeric level. |
| `Swr` | `Swr` | The SWR meter. |
| `Meter` | `Meter(Meter)` | A raw meter. |
| `ModelId` | `ModelId` | The model identifier. |
| `Firmware` | `Firmware` | The firmware revision. |
| `PowerAmplifier` | `PowerAmplifier` | Whether the power amplifier module is installed. |
| `ScopeOutput` | `ScopeOutput` | Waveform output on or off. |
| `ScopeAxis` | `ScopeAxis(u8)` | A receiver's scope axis mode. |
| `ScopeSpan` | `ScopeSpan(u8)` | A receiver's scope span. |
| `ScopeReference` | `ScopeReference(u8)` | A receiver's scope reference level. |
| `ScopeSetting` | `ScopeSetting(u8, ScopeSetting)` | A receiver's scope setting. |
| `Receiver` | `Receiver` | The receiver (band) commands act on. |
| `TxReceiver` | `TxReceiver` | The band the transmitter follows. |
| `DualWatch` | `DualWatch` | Dual watch on or off. |
| `Alarm` | `Alarm` | A safety condition the actor reports on its own (see [`Alarm`]). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `field` (src/op.rs:259), `take_starved` (src/poll.rs:155), `report` (src/poll.rs:166), `interval` (src/poll.rs:189), `cached` (src/rig.rs:539), `from_cache` (src/txstate.rs:38), `next_change` (src/rig/easy.rs:136).

**See also:** test `observation_maps_to_its_field (src/op.rs)`.

### `Alarm` [@ANCHOR: radioctl:radioctl_core_op_alarm]

**Purpose and location.** A safety condition the actor reports on its own. It is not a value read from the rig and it is never deduplicated: every occurrence is an event. Defined in `daemons/radioctl/crates/radioctl-core/src/op.rs:179` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `TransmitTimeout` | `TransmitTimeout` | The transmitter was keyed for longer than the rig's maximum transmit duration; the actor unkeyed it. |
| `HeartbeatLapsed` | `HeartbeatLapsed` | The caller's heartbeat lapsed while the rig was transmitting; the actor unkeyed it. |
| `UnkeyFailed` | `UnkeyFailed` | An unkey failed after every attempt: the rig may still be transmitting. |
| `LinkDown` | `LinkDown` | The transport to the rig failed. A rig that was keyed cannot be unkeyed by command until the link is back; every command is refused with `LinkDown` meanwhile. |
| `LinkRestored` | `LinkRestored` | The transport came back after [`Alarm::LinkDown`]; the actor has run its unkey pass (see `unkey_failures` / [`Alarm::UnkeyFailed`] for one that did not succeed). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `alarm` (src/rig.rs:1286), `alarms` (tests/actor.rs:516).

**See also:** test `observation_maps_to_its_field (src/op.rs)`.

### `Observation` [@ANCHOR: radioctl:radioctl_core_op_observation]

**Purpose and location.** A value reported by a rig, solicited or not. Defined in `daemons/radioctl/crates/radioctl-core/src/op.rs:197` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Frequency` | `Frequency(Vfo, Hz)` | A VFO's frequency. |
| `Mode` | `Mode(Mode)` | The mode. |
| `Ptt` | `Ptt(bool)` | The transmit state. |
| `Signal` | `Signal(u16)` | The S-meter, raw units. |
| `Power` | `Power(u16)` | The power meter, raw units. |
| `Split` | `Split(bool)` | Split operation on or off. |
| `RxVfo` | `RxVfo(Vfo)` | The VFO the rig receives on. |
| `TxVfo` | `TxVfo(Vfo)` | The VFO the rig transmits on. |
| `ActiveTxFrequency` | `ActiveTxFrequency(Hz)` | The frequency the rig transmits on (derived; see [`crate::txstate`]). |
| `Rit` | `Rit(bool)` | RIT on or off. |
| `Xit` | `Xit(bool)` | XIT on or off. |
| `RitOffset` | `RitOffset(i16)` | The RIT/XIT offset in hertz. |
| `FilterWidth` | `FilterWidth(Hz)` | The receive filter bandwidth. |
| `Level` | `Level(Level, u16)` | A numeric level in the rig's units. |
| `Swr` | `Swr(u16)` | The SWR in tenths of a unit (10 is 1.0:1). |
| `Meter` | `Meter(Meter, u16)` | A meter deflection in the rig's raw scale (Icom: 0 to 255), uncalibrated. |
| `ModelId` | `ModelId(u16)` | The model identifier the rig reports. |
| `Firmware` | `Firmware(u8, u8)` | The firmware revision, major and minor (`02.37` is `(2, 37)`). |
| `PowerAmplifier` | `PowerAmplifier(bool)` | Whether the rig's power amplifier module is installed and detected. |
| `ScopeOutput` | `ScopeOutput(bool)` | Waveform output on or off. |
| `ScopeAxis` | `ScopeAxis(u8, ScopeAxis)` | A receiver's scope axis mode. |
| `ScopeSpan` | `ScopeSpan(u8, Hz)` | A receiver's scope span (full width). |
| `ScopeReference` | `ScopeReference(u8, i16)` | A receiver's scope reference level in hundredths of a dB. |
| `ScopeSetting` | `ScopeSetting(u8, ScopeSetting, u8)` | A receiver's scope setting, as the rig's choice code. |
| `Receiver` | `Receiver(Receiver)` | The receiver (band) commands act on. |
| `TxReceiver` | `TxReceiver(Receiver)` | The band the transmitter follows. |
| `DualWatch` | `DualWatch(bool)` | Dual watch on or off. |
| `Alarm` | `Alarm(Alarm)` | A safety condition reported by the actor, never by the rig. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `cached` (src/rig.rs:539), `observe` (src/rig.rs:1948), `from_cache` (src/txstate.rs:38).

**See also:** test `observation_maps_to_its_field (src/op.rs)`.

### `ScopeAxis` [@ANCHOR: radioctl:radioctl_core_scope_scope_axis]

**Purpose and location.** How the scope's frequency axis is anchored. Defined in `daemons/radioctl/crates/radioctl-core/src/scope.rs:21` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `Center` | `Center` | Centred on the receive frequency. |
| `Fixed` | `Fixed` | Between fixed edges. |
| `ScrollCenter` | `ScrollCenter` | Scrolling, centred. |
| `ScrollFixed` | `ScrollFixed` | Scrolling, fixed edges. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `span_and_centre_come_from_the_edges (src/scope.rs)`.

### `ScopeSetting` [@ANCHOR: radioctl:radioctl_core_scope_scope_setting]

**Purpose and location.** A scope setting that is a small choice code of the rig's own (a switch or a short list), as opposed to the span, axis, reference level and edges that have operations of their own. Defined in `daemons/radioctl/crates/radioctl-core/src/scope.rs:35` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `Display` | `Display` | The scope display on the rig's screen: 0 off, 1 on. |
| `Layout` | `Layout` | Which scope layout the rig shows (bandscope, waterfall, audio scope and so on). |
| `Range` | `Range` | Which of the rig's stored fixed-mode ranges is in use (0 is the rig's choice of default). |
| `Pause` | `Pause` | Display pause: 0 running, 1 paused. |
| `Marker` | `Marker` | Which frequencies get a marker: 0 receive only, 1 receive and transmit. |
| `Attenuator` | `Attenuator` | The scope's own attenuator, in steps of 10 dB from 0. |
| `MaxHold` | `MaxHold` | Peak hold: 0 off, 1 on. |
| `Averaging` | `Averaging` | Display averaging: 0 off, then levels 1 to 3. |
| `WaterfallSpeed` | `WaterfallSpeed` | Waterfall scroll speed, 1 (slowest) to 4. |
| `Expand` | `Expand` | Expanded spectrum analysis range: 0 off, 1 on. The rig then sends a wider sweep than it shows. |
| `AudioAttenuator` | `AudioAttenuator` | The audio scope's attenuator, in steps of 10 dB from 0. |
| `AudioSpan` | `AudioSpan` | The audio scope's span: 0 narrow (3 kHz), 1 wide (8 kHz). |
| `OscilloscopeLevel` | `OscilloscopeLevel` | The oscilloscope level, in steps of 10 dB down from 0. |
| `OscilloscopeSweep` | `OscilloscopeSweep` | The oscilloscope sweep time, from the shortest (0) up. |
| `ShiftPosition` | `ShiftPosition` | Where the scrolling marker sits: 0 four grids left of centre, 4 centre, 8 four grids right. |
| `AudioPause` | `AudioPause` | The audio scope's display pause: 0 running, 1 paused. |
| `Overflow` | `Overflow` | The scope's receive circuit state, read only: 0 normal, 1 overflow from too much input. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `observation_maps_to_its_field (src/op.rs)`.

### `ScopeAction` [@ANCHOR: radioctl:radioctl_core_scope_scope_action]

**Purpose and location.** A one-shot scope command that has no value to set or read. Defined in `daemons/radioctl/crates/radioctl-core/src/scope.rs:74` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `ClearWaterfall` | `ClearWaterfall` | Clear the waterfall display. |
| `MarkerShift` | `MarkerShift` | Shift the marker (auto scroll mode) or centre it (fixed mode); the rig ignores it in centre mode. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `observation_maps_to_its_field (src/op.rs)`.

### `ScopeFrame` [@ANCHOR: radioctl:radioctl_core_scope_scope_frame]

**Purpose and location.** One complete sweep of a rig's scope. Defined in `daemons/radioctl/crates/radioctl-core/src/scope.rs:83` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `seq` | `u64` | Sequence number in the order the actor published frames, from 1. |
| `receiver` | `u8` | The receiver: 0 (Main) or 1 (Sub). |
| `axis` | `ScopeAxis` | How the axis was anchored for this sweep. |
| `low` | `Hz` | Lowest frequency on the axis. |
| `high` | `Hz` | Highest frequency on the axis. |
| `out_of_range` | `bool` | True when the receive frequency is outside a fixed scope. |
| `amplitude_max` | `u16` | The largest value a sample can take (the rig's amplitude units; 0 is the display bottom). |
| `samples` | `Vec<u8>` | One amplitude per pixel, lowest frequency first. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `next` (src/rig.rs:845).

**See also:** test `span_and_centre_come_from_the_edges (src/scope.rs)`.

### `FilterScopeFrame` [@ANCHOR: radioctl:radioctl_core_scope_filter_scope_frame]

**Purpose and location.** One sweep of a rig's filter scope (the passband display, not the band): a few hundred pixels of amplitude with no frequency axis, since the rig draws it around the receive passband. Defined in `daemons/radioctl/crates/radioctl-core/src/scope.rs:105` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `seq` | `u64` | Sequence number in the order the actor published filter-scope frames, from 1. |
| `receiver` | `u8` | The receiver: 0 (Main) or 1 (Sub). |
| `amplitude_max` | `u16` | The largest value a sample can take (the rig's amplitude units; 0 is the display bottom). |
| `samples` | `Vec<u8>` | One amplitude per pixel, lowest audio frequency first. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `next` (src/rig.rs:874).

**See also:** test `filter_scope_peak_is_the_largest_sample (src/scope.rs)`.

### `TcpConfig` [@ANCHOR: radioctl:radioctl_core_tcp_tcp_config]

**Purpose and location.** How to reach a rig over TCP. Defined in `daemons/radioctl/crates/radioctl-core/src/tcp.rs:17` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `connect_timeout` | `Duration` | Longest wait for one connection attempt. |
| `attempts` | `u32` | How many attempts in all (at least one). |
| `backoff` | `Backoff` | Waits between attempts. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `default` (src/tcp.rs:27).

**See also:** test `tcp_connects_moves_bytes_and_fails_in_bounded_time (tests/transports.rs)`.

### `TxState` [@ANCHOR: radioctl:radioctl_core_txstate_tx_state]

**Purpose and location.** The cached values the derivation reads. Defined in `daemons/radioctl/crates/radioctl-core/src/txstate.rs:22` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq, Default.

| Field | Type | Meaning |
|---|---|---|
| `rx_vfo` | `Option<Vfo>` | The reported receive VFO. |
| `tx_vfo` | `Option<Vfo>` | The reported transmit VFO. |
| `split` | `Option<bool>` | Split operation, when known. |
| `freq_a` | `Option<Hz>` | VFO A's frequency. |
| `freq_b` | `Option<Hz>` | VFO B's frequency. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `tx_state` (src/rig.rs:719), `from_cache` (src/txstate.rs:38), `state` (src/txstate.rs:123).

**See also:** test `the_inputs_come_from_the_cached_observations (src/txstate.rs)`.

### `Chip` [@ANCHOR: radioctl:radioctl_core_usb_chip]

**Purpose and location.** The family of the USB-to-serial chip behind a port. Defined in `daemons/radioctl/crates/radioctl-core/src/usb.rs:27` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Ftdi` | `Ftdi` | FTDI FT232, FT2232 and relatives. |
| `Cp210x` | `Cp210x` | Silicon Labs CP210x. |
| `Ch340` | `Ch340` | WCH CH340, CH341, CH9102. |
| `CdcAcm` | `CdcAcm` | A USB CDC ACM device (native USB serial of the radio or a microcontroller). |
| `Unknown` | `Unknown` | Not recognised from the ids alone. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `classify` (src/usb.rs:70), `chip` (src/usb.rs:83).

**See also:** test `known_vendor_ids_name_their_chip (src/usb.rs)`.

### `PortInfo` [@ANCHOR: radioctl:radioctl_core_usb_port_info]

**Purpose and location.** What is known about one serial port. Defined in `daemons/radioctl/crates/radioctl-core/src/usb.rs:42` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `path` | `PathBuf` | The device path to open. |
| `vid` | `Option<u16>` | USB vendor id, if the port is USB. |
| `pid` | `Option<u16>` | USB product id, if the port is USB. |
| `serial` | `Option<String>` | USB serial number string, if the device has one. |
| `product` | `Option<String>` | Product string, if known. |
| `cdc_acm` | `bool` | True when the kernel driver for the port is `cdc_acm` (Linux). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `list_sysfs` (src/usb.rs:147), `list_ports` (src/usb.rs:191), `port` (src/usb.rs:223).

**See also:** test `sysfs_lists_usb_ports_and_skips_the_rest (tests/transports.rs)`.

### `UsbMatcher` [@ANCHOR: radioctl:radioctl_core_usb_usb_matcher]

**Purpose and location.** What to look for. Defined in `daemons/radioctl/crates/radioctl-core/src/usb.rs:59` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `vid` | `u16` | Required vendor id. |
| `pid` | `u16` | Required product id. |
| `serial` | `Option<String>` | Required serial number; `None` accepts any (then more than one match is an error). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `selection_needs_exactly_one_match (src/usb.rs)`.

### `UsbStatus` [@ANCHOR: radioctl:radioctl_core_usb_wire_usb_status]

**Purpose and location.** The status byte of a reply. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_wire.rs:33` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Ok` | `Ok` | The transfer completed. |
| `Timeout` | `Timeout` | The device did not answer in time. |
| `NoDevice` | `NoDevice` | The device is gone. |
| `Stall` | `Stall` | The device refused the request. |
| `Other` | `Other` | Any other failure (including a short write). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_code` (src/usb_wire.rs:61), `status_of` (src/usb_wire.rs:359).

**See also:** test `every_frame_round_trips_and_partial_or_bad_frames_are_told_apart (tests/usb_link.rs)`.

### `UsbTransfer` [@ANCHOR: radioctl:radioctl_core_usb_wire_usb_transfer]

**Purpose and location.** One USB transfer, as a request frame. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_wire.rs:74` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `ControlIn` | `ControlIn { request_type: u8, request: u8, value: u16, index: u16, length: u16, }` | A control transfer from the device. Request type (direction, type and recipient bits). Request number. wValue. wIndex. Bytes to read. |
| `ControlOut` | `ControlOut { request_type: u8, request: u8, value: u16, index: u16, data: Vec<u8>, }` | A control transfer to the device. Request type. Request number. wValue. wIndex. Data stage. |
| `BulkWrite` | `BulkWrite { endpoint: u8, data: Vec<u8>, }` | A bulk OUT transfer. Endpoint address. Bytes to send. |
| `BulkRead` | `BulkRead { endpoint: u8, length: u16, }` | A bulk IN transfer. Endpoint address. Bytes to read. |
| `InterruptWrite` | `InterruptWrite { endpoint: u8, data: Vec<u8>, }` | An interrupt OUT transfer. Endpoint address. Bytes to send. |
| `InterruptRead` | `InterruptRead { endpoint: u8, length: u16, }` | An interrupt IN transfer. Endpoint address. Bytes to read. |
| `InterruptExchange` | `InterruptExchange { out_endpoint: u8, in_endpoint: u8, data: Vec<u8>, read_length: u16, }` | An interrupt OUT transfer followed at once by an interrupt IN transfer. OUT endpoint address. IN endpoint address. Bytes to send. Bytes to read back. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `decode` (src/usb_wire.rs:243), `execute` (src/usb_wire.rs:371).

**See also:** test `every_frame_round_trips_and_partial_or_bad_frames_are_told_apart (tests/usb_link.rs)`.

### `FrameError` [@ANCHOR: radioctl:radioctl_core_usb_wire_frame_error]

**Purpose and location.** Why a frame could not be decoded. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_wire.rs:144` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Incomplete` | `Incomplete` | More bytes are needed. |
| `Malformed` | `Malformed` | The bytes are not a frame (unknown operation, or a data field over [`MAX_DATA`]). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `decode` (src/usb_wire.rs:243), `decode` (src/usb_wire.rs:335).

**See also:** test `every_frame_round_trips_and_partial_or_bad_frames_are_told_apart (tests/usb_link.rs)`.

### `UsbReply` [@ANCHOR: radioctl:radioctl_core_usb_wire_usb_reply]

**Purpose and location.** The reply frame to one [`UsbTransfer`]. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_wire.rs:313` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `op` | `u8` | The operation of the request this answers. |
| `status` | `UsbStatus` | How the transfer ended. |
| `data` | `Vec<u8>` | Bytes read from the device (empty for a write, or on failure). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `decode` (src/usb_wire.rs:335), `execute` (src/usb_wire.rs:371).

**See also:** test `every_frame_round_trips_and_partial_or_bad_frames_are_told_apart (tests/usb_link.rs)`.

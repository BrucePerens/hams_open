# Radioctl protocols data structures, part 2 (easy to hpsdr_net)

As a developer changing this code, I want every data structure in `src/easy.rs`, `src/elecraft.rs`, `src/elektor507.rs`, `src/flex.rs`, `src/flex_io.rs`, `src/funcube.rs`, `src/gomspace.rs`, `src/guohe.rs`, `src/hiqsdr.rs`, `src/hpsdr_io.rs` and `src/hpsdr_net.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `SpawnedVirtualRig` [@ANCHOR: radioctl:radioctl_easy_spawned_virtual_rig]

**Purpose.** A virtual rig that is up: the rig handle to drive it and the control of the stand-in behind it (front panel, state, fault injection).

**Definition.** `pub struct SpawnedVirtualRig` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/easy.rs`.

| Field | Type | Meaning |
|---|---|---|
| `rig` | `RigHandle` | The rig actor's handle. Its transmit gate is closed. |
| `control` | `VirtualRigControl` | The virtual radio behind the handle. |

**Used by.** `spawn`, `spawn_virtual` (functions whose signature names it).

## `SerialRig` [@ANCHOR: radioctl:radioctl_easy_serial_rig]

**Purpose.** A rig on a serial port: the handle, and the port's status, counters and control lines. Keep `link` for as long as the rig is in use: the actor's link watch and any line keyer depend on it.

**Definition.** `pub struct SerialRig` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/easy.rs`.

| Field | Type | Meaning |
|---|---|---|
| `rig` | `RigHandle` | The rig actor's handle. Its transmit gate is closed. |
| `link` | `SerialMonitor` | The port's status, counters and control lines. |

**Used by.** `open_serial_rig`, `open_serial_rig_with` (functions whose signature names it).

## `LevelWire` [@ANCHOR: radioctl:radioctl_elecraft_level_wire_struct]

**Purpose.** The command prefix and the allowed range of a level, from the manual.

**Definition.** `pub struct LevelWire` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/elecraft.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `prefix` | `&'static str` | The two-letter command. |
| `min` | `u16` | Lowest value. |
| `max` | `u16` | Highest value. |

**Used by.** `level_wire`, `level_wire_for` (functions whose signature names it).

## `ElecraftEngine` [@ANCHOR: radioctl:radioctl_elecraft_elecraft_engine]

**Purpose.** The Elecraft ASCII client engine for one rig.

**Definition.** `pub struct ElecraftEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/elecraft.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `tokenizer` | `Tokenizer` | Splits received bytes into `;`-terminated replies (`Tokenizer`). |
| `transceive` | `bool` | The auto-information the controller turned on (`AI2;`), which the rig loses with the link; sent again by [`ProtocolEngine::link_restored`]. |

**Used by.** `bytes`, `engine`, `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `Pll` [@ANCHOR: radioctl:radioctl_elektor507_pll]

**Purpose.** The PLL settings: total pump `p`, total counter `q` and divider 1 `div`.

**Definition.** `pub struct Pll` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/elektor507.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `p` | `u32` | Total pump (P). |
| `q` | `u32` | Total reference counter (Q). |
| `div` | `u32` | Divider 1 as the frequency formula uses it (2 to 127). |

**Used by.** `solve` (functions whose signature names it).

## `Samples` [@ANCHOR: radioctl:radioctl_elektor507_samples]

**Purpose.** Builds the pin samples of I2C write transactions.

**Definition.** `struct Samples<'a>` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/elektor507.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `port` | `&'a mut u8` | The pin byte, updated in place as bits are driven (SCL and SDA among them). |
| `out` | `Vec<u8>` | The samples produced so far, in order. |

## `Elektor507Engine` [@ANCHOR: radioctl:radioctl_elektor507_elektor507_engine]

**Purpose.** The Elektor SDR-USB client engine.

**Definition.** `pub struct Elektor507Engine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/elektor507.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `port` | `u8` | The current state of the output pins (SDA, SCL and the rest), which every sample carries on from. |
| `pll` | `Pll` | The PLL settings (`Pll`) last computed for the tuned frequency. |
| `rx` | `Vec<u8>` | Bytes received from the device so far and not yet consumed. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `Elektor507Twin` [@ANCHOR: radioctl:radioctl_elektor507_elektor507_twin]

**Purpose.** A virtual Elektor SDR-USB: a [`UsbDevice`] that decodes the pin samples written to the bulk OUT endpoint into I2C transactions on a clock chip's register file.

**Definition.** `pub struct Elektor507Twin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/elektor507.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `regs` | `[u8; 256]` | The clock chip's 256-byte register file, as written by the controller's I2C transactions. |
| `bitbang` | `bool` | Whether bit-bang mode has been switched on in the USB-to-pins chip. |
| `port` | `u8` | The state of the output pins as last written. |
| `scl` | `bool` | The I2C clock line level decoded from the pin samples. |
| `sda` | `bool` | The I2C data line level decoded from the pin samples. |
| `in_transaction` | `bool` | True between an I2C start and stop condition. |
| `bits` | `u8` | How many bits of the current byte have been clocked in (0 to 8). |
| `current` | `u8` | The byte being assembled from the clocked bits. |
| `bytes` | `Vec<u8>` | The bytes of the transaction so far (address, register and data). |
| `pll_written` | `bool` | Set once the PLL registers have been written by the controller. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Reply` [@ANCHOR: radioctl:radioctl_flex_reply]

**Purpose.** A decoded `R` line.

**Definition.** `pub struct Reply` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/flex.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `seq` | `u32` | The sequence number of the command. |
| `code` | `u32` | The hexadecimal response; zero is success. |
| `message` | `String` | The message part (may be empty, may contain `|`). |

**Used by.** `answer_reply`, `parse_reply` (functions whose signature names it).

## `RadioInfo` [@ANCHOR: radioctl:radioctl_flex_radio_info]

**Purpose.** What `info` told us about the radio ("TCPIP info").

**Definition.** `pub struct RadioInfo` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/flex.rs`; derives `Debug`, `Clone`, `Default`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `model` | `Option<String>` | The model, e.g. `FLEX-6300`. |
| `software_ver` | `Option<String>` | Firmware version text. |
| `slices` | `Option<u8>` | Slice receivers the radio has. |
| `scus` | `Option<u8>` | Spectrum capture units. |
| `name` | `Option<String>` | The user's name for the radio. |
| `callsign` | `Option<String>` | The owner's callsign. |

**Used by.** `parse_info`, `radio_info` (functions whose signature names it).

## `Discovery` [@ANCHOR: radioctl:radioctl_flex_discovery]

**Purpose.** A radio announced by the discovery broadcast.

**Definition.** `pub struct Discovery` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/flex.rs`; derives `Debug`, `Clone`, `Default`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `model` | `String` | Model name. |
| `serial` | `String` | Serial number. |
| `version` | `String` | Firmware version. |
| `name` | `String` | Name, with the underscores of the wire turned back into spaces. |
| `callsign` | `String` | Owner callsign. |
| `ip` | `String` | Address to connect to. |
| `port` | `u16` | TCP control port. |

**Used by.** `parse_discovery` (functions whose signature names it).

## `SliceState` [@ANCHOR: radioctl:radioctl_flex_slice_state]

**Purpose.** What the engine knows about one slice receiver.

**Definition.** `pub struct SliceState` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/flex.rs`; derives `Debug`, `Clone`, `Default`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `in_use` | `Option<bool>` | `in_use`. |
| `active` | `Option<bool>` | `active`. |
| `frequency` | `Option<Hz>` | `RF_frequency`. |
| `mode_text` | `Option<String>` | `mode` as the radio spelled it. |
| `mode` | `Option<Mode>` | The mode, when the core vocabulary has it. |
| `filter_lo_hz` | `Option<i64>` | `filter_lo` in hertz. |
| `filter_hi_hz` | `Option<i64>` | `filter_hi` in hertz. |
| `audio_level` | `Option<u16>` | `audio_level`, 0 to 100. |
| `tx` | `Option<bool>` | `tx`: this slice is the transmit slice. |
| `antenna` | `Option<String>` | `antenna_id`. |

**Used by.** `slice_state`, `slice_value` (functions whose signature names it).

## `PanadapterState` [@ANCHOR: radioctl:radioctl_flex_panadapter_state]

**Purpose.** What the engine knows about one panadapter.

**Definition.** `pub struct PanadapterState` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/flex.rs`; derives `Debug`, `Clone`, `Default`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `center` | `Option<Hz>` | `center`. |
| `bandwidth` | `Option<Hz>` | `bandwidth`. |
| `x_pixels` | `Option<u32>` | `x_pixels` (the page spells it both `x-pixels` and `x_pixels`). |
| `y_pixels` | `Option<u32>` | `y_pixels`. |
| `fps` | `Option<u32>` | `fps`. |

**Used by.** `panadapter` (functions whose signature names it).

## `TransmitState` [@ANCHOR: radioctl:radioctl_flex_transmit_state]

**Purpose.** What the engine knows about the transmitter and its interlock.

**Definition.** `pub struct TransmitState` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/flex.rs`; derives `Debug`, `Clone`, `Default`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `interlock` | `Option<String>` | `interlock state=`. |
| `tx_allowed` | `Option<bool>` | `tx_allowed`. |
| `rf_power` | `Option<u16>` | `rfpower`, 0 to 100 (key assumed to match the `transmit set` parameter). |
| `mic_level` | `Option<u16>` | `miclevel`, 0 to 100 (same assumption). |

**Used by.** `transmit_state` (functions whose signature names it).

## `Kind` [@ANCHOR: radioctl:radioctl_flex_kind]

**Purpose.** What a pending Flex command is, and so how its reply is handled.

**Definition.** `enum Kind` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/flex.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Quiet` | - | A subscription: the reply is not shown to the actor. |
| `Info` | - | `info`: the reply is parsed into [`RadioInfo`]. |
| `Set` | - | A set: the reply is the acknowledgement. |
| `Get` | `(Field)` | A read answered from the shadow. |
| `Version` | - | `version`. |

**Used by.** `commands` (functions whose signature names it).

## `Pending` [@ANCHOR: radioctl:radioctl_flex_pending]

**Purpose.** A command sent to the radio whose `R` reply has not arrived: its sequence number and what to do with the reply.

**Definition.** `struct Pending` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/flex.rs`.

| Field | Type | Meaning |
|---|---|---|
| `seq` | `u32` | The command's sequence number, which the radio echoes in its `R` line. |
| `kind` | `Kind` | How to treat the reply (`Kind`). |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `FlexEngine` [@ANCHOR: radioctl:radioctl_flex_flex_engine]

**Purpose.** The FlexRadio SmartSDR protocol engine.

**Definition.** `pub struct FlexEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/flex.rs`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `slice` | `u8` | The slice receiver (0-based) the engine's frequency, mode and level operations apply to. |
| `seq` | `u32` | The last command sequence number used; incremented (wrapping) for each command. |
| `line` | `Vec<u8>` | Bytes of the incoming line not terminated yet. |
| `line_at` | `Duration` | Time the last byte of `line` arrived, used to drop a stale partial line. |
| `discarding` | `bool` | True while the rest of an over-long line is being thrown away until its end. |
| `pending` | `Vec<Pending>` | Commands sent whose replies have not arrived, oldest first (`Pending`). |
| `awaiting` | `Vec<Field>` | Shadow fields a read has asked for and that the radio's next status message should fill (`Field`). |
| `subscribed` | `bool` | Whether the engine has sent its subscription commands yet (it sends them before the first operation). |
| `protocol_version` | `Option<String>` | The protocol version text from the radio's `V` line, `None` until seen. |
| `handle` | `Option<u32>` | The client handle from the radio's `H` line, `None` until seen. |
| `slices` | `BTreeMap<u8, SliceState>` | What is known about each slice receiver, by slice number (`SliceState`). |
| `pans` | `BTreeMap<u32, PanadapterState>` | What is known about each panadapter, by stream id (`PanadapterState`). |
| `tx` | `TransmitState` | What is known about the transmitter and its interlock (`TransmitState`). |
| `info` | `RadioInfo` | What the radio said in answer to `info` (`RadioInfo`). |
| `last_message` | `Option<(u32, String)>` | The last radio message (number and text), `None` before one. |
| `refused` | `u32` | How many commands the radio has refused (non-zero response code). |
| `last_refusal` | `Option<u32>` | The response code of the last refused command, `None` before one. |

**Used by.** `engine`, `new`, `with_slice` (functions whose signature names it).

**Notes.** One per connection; the shadow (`slices`, `pans`, `tx`, `info`) is filled from the radio's status messages and answers reads without a round trip. Transmit-capable operations reach it only through the actor's transmit gate.

## `Shared` [@ANCHOR: radioctl:radioctl_flex_io_shared]

**Purpose.** State shared between a served virtual Flex radio's connection task and the control handle: the virtual rig behind a mutex and a channel for pushing unsolicited lines to the client.

**Definition.** `struct Shared` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/flex_io.rs`.

| Field | Type | Meaning |
|---|---|---|
| `rig` | `Mutex<VirtualFlex>` | The virtual Flex radio (`VirtualFlex`), behind a mutex. |
| `push` | `mpsc::UnboundedSender<Vec<u8>>` | Sender of lines the radio pushes to the connected client on its own (status messages). |

## `VirtualFlexControl` [@ANCHOR: radioctl:radioctl_flex_io_virtual_flex_control]

**Purpose.** Control of a served virtual radio.

**Definition.** `pub struct VirtualFlexControl` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/flex_io.rs`; derives `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `shared` | `Arc<Shared>` | The shared state of the served virtual radio (`Arc<Shared>`); the handle is `Clone`. |

**Used by.** `serve_virtual_flex` (functions whose signature names it).

## `Pending` [@ANCHOR: radioctl:radioctl_funcube_pending]

**Purpose.** What the engine is waiting for after the last request it sent, so the next reply is read as its answer.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/funcube.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `SetHz` | - | A set of the frequency in hertz is outstanding. |
| `SetKhz` | - | A set of the frequency in kilohertz (the older dongle's command) is outstanding. |
| `ReadFrequency` | - | A frequency read is outstanding. |
| `SetGain` | `(u8)` | A gain set; the command whose answer is expected. |
| `Read` | `(u8, Reading)` | A read of the given command, decoded as given. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `Reading` [@ANCHOR: radioctl:radioctl_funcube_reading]

**Purpose.** How the answer byte of a read is turned into an observation.

**Definition.** `enum Reading` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/funcube.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `ProPreamp` | - | The Pro dongle's preamp state. |
| `ProAttenuator` | - | The Pro dongle's attenuator state. |
| `IfGain` | - | The IF gain. |
| `Preamp` | - | The original dongle's preamp (LNA) state. |
| `Attenuator` | - | The original dongle's attenuator state. |
| `Signal` | - | The signal strength reading. |

**Used by.** `gain_read` (functions whose signature names it).

## `FuncubeEngine` [@ANCHOR: radioctl:radioctl_funcube_funcube_engine]

**Purpose.** The FUNcube client engine.

**Definition.** `pub struct FuncubeEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/funcube.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `pro` | `bool` | True for the FUNcube Dongle Pro+ (different level commands from the original dongle). |
| `legacy` | `bool` | True for old firmware that takes the frequency in kilohertz. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `Vec<u8>` | Bytes received from the device so far and not yet consumed. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `FuncubeTwin` [@ANCHOR: radioctl:radioctl_funcube_funcube_twin]

**Purpose.** A virtual FUNcube dongle: a [`UsbDevice`] answering the HID reports above.

**Definition.** `pub struct FuncubeTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/funcube.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `hz` | `u32` | The tuned frequency in hertz. |
| `legacy` | `bool` | True when the twin emulates the old firmware that takes kilohertz. |
| `pro` | `bool` | True when the twin emulates the Pro+. |
| `lna` | `u8` | The LNA (preamp) setting. |
| `mixer` | `u8` | The mixer gain setting. |
| `if_gain` | `u8` | The IF gain setting. |
| `rssi` | `u8` | The signal strength the twin reports (set by tests with `set_rssi`). |
| `answer` | `Vec<u8>` | The bytes the next read of the device returns. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Pending` [@ANCHOR: radioctl:radioctl_gomspace_pending]

**Purpose.** What the engine is waiting for after the last request it sent, so the next reply is read as its answer.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/gomspace.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Get` | `(Vfo)` | A frequency read of this VFO is outstanding. |
| `Set` | - | A set command is outstanding; its answer is an acknowledgement. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `GomspaceEngine` [@ANCHOR: radioctl:radioctl_gomspace_gomspace_engine]

**Purpose.** The GS100 client engine.

**Definition.** `pub struct GomspaceEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/gomspace.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `GomspaceTwin` [@ANCHOR: radioctl:radioctl_gomspace_gomspace_twin]

**Purpose.** A virtual GS100.

**Definition.** `pub struct GomspaceTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/gomspace.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `table` | `u8` | The configuration table selected by `param mem <n>` (1 receive, 5 transmit). |
| `rx_hz` | `Hz` | Receive frequency in hertz. |
| `tx_hz` | `Hz` | Transmit frequency in hertz. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Pending` [@ANCHOR: radioctl:radioctl_guohe_pending]

**Purpose.** What the engine is waiting for after the last request it sent, so the next reply is read as its answer.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/guohe.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Status` | - | A status read is outstanding. |
| `Ack` | - | A set command is outstanding; its answer is an acknowledgement. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `GuoheEngine` [@ANCHOR: radioctl:radioctl_guohe_guohe_engine]

**Purpose.** The Guohe client engine.

**Definition.** `pub struct GuoheEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/guohe.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |
| `freq` | `[u64; 2]` | The last known frequency of each VFO in hertz (index 0 is VFO A). |
| `mode` | `[u8; 2]` | The last known mode byte of each VFO. |

**Used by.** `new`, `run` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `GuoheTwin` [@ANCHOR: radioctl:radioctl_guohe_guohe_twin]

**Purpose.** A virtual PMR-171.

**Definition.** `pub struct GuoheTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/guohe.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `freq` | `[u64; 2]` | The frequency of each VFO in hertz (index 0 is VFO A). |
| `mode` | `[u8; 2]` | Current mode, as the code the description's `[[modes]]` table uses. |
| `ptt` | `bool` | True while the twin is transmitting (keyed by the controller). |

**Used by.** `new`, `run` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `HiqsdrEngine` [@ANCHOR: radioctl:radioctl_hiqsdr_hiqsdr_engine]

**Purpose.** The HiQSDR client engine.

**Definition.** `pub struct HiqsdrEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/hiqsdr.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `frame` | `[u8; FRAME_LEN]` | The last 22-byte control frame; every command rebuilds and sends the whole frame because it carries all the state. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `HiqsdrTwin` [@ANCHOR: radioctl:radioctl_hiqsdr_hiqsdr_twin]

**Purpose.** A virtual HiQSDR: it keeps the last control frame it was sent.

**Definition.** `pub struct HiqsdrTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/hiqsdr.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `frame` | `[u8; FRAME_LEN]` | The last 22-byte control frame received. |
| `default_hz` | `Hz` | The description's default frequency, reported until the first tune. |
| `tuned` | `bool` | Whether a frame with a tuning word has been received. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `HpsdrLink` [@ANCHOR: radioctl:radioctl_hpsdr_io_hpsdr_link]

**Purpose.** A running session of either protocol.

**Definition.** `pub enum HpsdrLink` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/hpsdr_io.rs`; derives `Debug`.

| Variant | Type | Meaning |
|---|---|---|
| `P1` | `(Radio)` | A Protocol 1 session. |
| `P2` | `(Radio2)` | A Protocol 2 session. |

**Used by.** `request_ptt`, `serve_hpsdr`, `set_attenuator`, `set_frequency`, `start_session`, `status_of` (functions whose signature names it).

## `HpsdrOptions` [@ANCHOR: radioctl:radioctl_hpsdr_io_hpsdr_options]

**Purpose.** Bridge settings.

**Definition.** `pub struct HpsdrOptions` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/hpsdr_io.rs`; derives `Debug`, `Clone`, `Copy`.

| Field | Type | Meaning |
|---|---|---|
| `ptt_lease` | `Duration` | How long one `P 1` keys the radio before the bridge unkeys by itself. |
| `drive` | `u8` | Drive level 0..=255 sent while keyed. |
| `status_every` | `Duration` | Interval of the unsolicited power and SWR lines. |
| `power_constant` | `f64` | Power constant 2 for SWR (0.095 ANAN-100, 0.09 ANAN-10). |

**Used by.** `handle_line`, `serve_hpsdr`, `swr_tenths` (functions whose signature names it).

## `Shared` [@ANCHOR: radioctl:radioctl_hpsdr_io_shared]

**Purpose.** State shared by the OpenHPSDR bridge task and its control handle: the radio session and the transmit gate.

**Definition.** `struct Shared` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/hpsdr_io.rs`.

| Field | Type | Meaning |
|---|---|---|
| `link` | `Mutex<Option<HpsdrLink>>` | The running session (`HpsdrLink`), `None` while not connected. |
| `gate` | `TxGate` | The transmit gate (`TxGate`) that a `P 1` key request must pass. |

**Used by.** `handle_line` (functions whose signature names it).

## `HpsdrControl` [@ANCHOR: radioctl:radioctl_hpsdr_io_hpsdr_control]

**Purpose.** Handle on the served bridge: the attenuator (also reachable as `Level::Attenuator` through the actor) and the status.

**Definition.** `pub struct HpsdrControl` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/hpsdr_io.rs`; derives `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `shared` | `Arc<Shared>` | The bridge's shared state (`Arc<Shared>`); the handle is `Clone`. |

**Used by.** `serve_hpsdr` (functions whose signature names it).

## `State` [@ANCHOR: radioctl:radioctl_hpsdr_io_state]

**Purpose.** Mutable state of the bridge task.

**Definition.** `struct State` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/hpsdr_io.rs`.

| Field | Type | Meaning |
|---|---|---|
| `freq` | `u32` | The current frequency in hertz as last set through the bridge. |
| `ptt` | `bool` | Whether the radio is keyed. |
| `att_db` | `u8` | The attenuation last set through the bridge, in decibels. |

**Used by.** `handle_line` (functions whose signature names it).

## `HpsdrEngine` [@ANCHOR: radioctl:radioctl_hpsdr_net_hpsdr_engine]

**Purpose.** The engine for one OpenHPSDR radio.

**Definition.** `pub struct HpsdrEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/hpsdr_net.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `line` | `Vec<u8>` | Bytes of the incoming line not terminated yet. |
| `discarding` | `bool` | True while the rest of an over-long line is being thrown away until its end. |

**Used by.** `engine`, `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.


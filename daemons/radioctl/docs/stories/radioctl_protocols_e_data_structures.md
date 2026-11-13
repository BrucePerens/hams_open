# Radioctl protocols data structures, part 5 (tentec to yaesu_legacy)

As a developer changing this code, I want every data structure in `src/tentec.rs`, `src/tentec_rx320.rs`, `src/transfox.rs`, `src/uniden.rs`, `src/uniden_digital.rs`, `src/usb_family.rs`, `src/wj.rs`, `src/xg3.rs`, `src/yaesu.rs` and `src/yaesu_legacy.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `Dialect` [@ANCHOR: radioctl:radioctl_tentec_dialect]

**Purpose.** The Ten-Tec model families.

**Definition.** `pub enum Dialect` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/tentec.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Orion` | - | Orion TT-565/566 and Eagle TT-599: ASCII. |
| `Argonaut` | - | Argonaut V TT-516: binary with `G` acknowledgements. |
| `Jupiter` | - | Jupiter TT-538: binary, no acknowledgements. |
| `OmniVii` | - | Omni VII TT-588: binary, no acknowledgements, its own PTT (push to talk) frame. |

**Used by.** `from_name` (functions whose signature names it).

## `Pending` [@ANCHOR: radioctl:radioctl_tentec_pending]

**Purpose.** What the engine waits for after a read or an acknowledged write.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/tentec.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Frequency` | `(Vfo)` | A frequency read or write is outstanding; its answer is the frequency. |
| `Mode` | - | A mode read or write is outstanding; its answer is the mode. |
| `Ptt` | - | A transmit/receive switch command is outstanding; its answer is the acknowledgement. |
| `Ack` | - | A set command is outstanding; its answer is an acknowledgement. |

**Used by.** `decode_binary`, `decode_orion_line`, `frame_len` (functions whose signature names it).

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `TentecEngine` [@ANCHOR: radioctl:radioctl_tentec_tentec_engine]

**Purpose.** The Ten-Tec client engine for one rig.

**Definition.** `pub struct TentecEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/tentec.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `dialect` | `Dialect` | Which command set of the family the rig speaks. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |
| `modes` | `Option<(u8, u8)>` | The two mode characters (VFO A, VFO B) of the last `M` reply. |

**Used by.** `exchange`, `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `TentecTwin` [@ANCHOR: radioctl:radioctl_tentec_tentec_twin]

**Purpose.** A virtual Ten-Tec radio of one dialect.

**Definition.** `pub struct TentecTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/tentec.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `dialect` | `Dialect` | The command set the twin emulates. |
| `codes` | `Vec<u8>` | The mode codes the description defines; the twin accepts a mode setting only if its code is in this list. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `freq` | `[u64; 2]` | The frequency of each VFO in hertz (index 0 is VFO A). |
| `mode` | `[u8; 2]` | Current mode, as the code the description's `[[modes]]` table uses. |
| `ptt` | `bool` | True while the twin is transmitting (keyed by the controller). |

**Used by.** `exchange`, `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Variant` [@ANCHOR: radioctl:radioctl_tentec_rx320_variant]

**Purpose.** Which radio's formulas and frames.

**Definition.** `pub enum Variant` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/tentec_rx320.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Rx320` | - | RX-320 (Hamlib's formulas). |
| `Tt550` | - | Pegasus TT-550 receive side (the programmer's guide's formulas). |

**Used by.** `correction`, `frequency_of_for`, `from_name`, `tuning_words_for` (functions whose signature names it).

## `Rx320Engine` [@ANCHOR: radioctl:radioctl_tentec_rx320_rx320_engine]

**Purpose.** The RX-320 client engine.

**Definition.** `pub struct Rx320Engine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/tentec_rx320.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `variant` | `Variant` | Which radio's formulas and frames it uses (`Variant`). |
| `hz` | `Hz` | The frequency last set; the engine remembers it because a tune is re-sent whole. |
| `mode` | `Mode` | The mode last set. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `Rx320Twin` [@ANCHOR: radioctl:radioctl_tentec_rx320_rx320_twin]

**Purpose.** A virtual RX-320.

**Definition.** `pub struct Rx320Twin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/tentec_rx320.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description the twin takes its defaults and tables from. |
| `variant` | `Variant` | Which radio's formulas and frames it emulates. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `words` | `Option<[u16; 3]>` | The three tuning words (coarse, fine, BFO) of the last `N` frame, `None` before one. |
| `hz` | `Hz` | Current frequency in hertz. |
| `mode` | `Mode` | Current mode, as the code the description's `[[modes]]` table uses. |
| `filter` | `u8` | The index of the filter selected by `W<n>` in the 34-entry filter table. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `TransfoxEngine` [@ANCHOR: radioctl:radioctl_transfox_transfox_engine]

**Purpose.** The Transfox client engine.

**Definition.** `pub struct TransfoxEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/transfox.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `TransfoxTwin` [@ANCHOR: radioctl:radioctl_transfox_transfox_twin]

**Purpose.** A virtual Transfox.

**Definition.** `pub struct TransfoxTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/transfox.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `hz` | `u64` | Current frequency in hertz. |
| `keyed` | `bool` | True while the twin is transmitting. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `UnidenEngine` [@ANCHOR: radioctl:radioctl_uniden_uniden_engine]

**Purpose.** The Uniden engine.

**Definition.** `pub type UnidenEngine = LineEngine<UnidenDialect>;` (type) in `daemons/radioctl/crates/radioctl-protocols/src/uniden.rs`.

**Used by.** `engine` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `UnidenDialect` [@ANCHOR: radioctl:radioctl_uniden_uniden_dialect]

**Purpose.** The Uniden command set of one model.

**Definition.** `pub struct UnidenDialect` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/uniden.rs`; derives `Debug`, `Clone`, `Copy`.

| Field | Type | Meaning |
|---|---|---|
| `frequency_via_sg` | `bool` | The BC780XLT reads its frequency through `SG`. |

**Used by.** `from_description` (functions whose signature names it).

## `UnidenTwin` [@ANCHOR: radioctl:radioctl_uniden_uniden_twin]

**Purpose.** The virtual Uniden scanner.

**Definition.** `pub struct UnidenTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/uniden.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description the twin takes its defaults and tables from. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `UnidenDigitalEngine` [@ANCHOR: radioctl:radioctl_uniden_digital_uniden_digital_engine]

**Purpose.** The engine of the BCD396T and BCD996T.

**Definition.** `pub type UnidenDigitalEngine = LineEngine<UnidenDigitalDialect>;` (type) in `daemons/radioctl/crates/radioctl-protocols/src/uniden_digital.rs`.

**Used by.** `engine` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `UnidenDigitalDialect` [@ANCHOR: radioctl:radioctl_uniden_digital_uniden_digital_dialect]

**Purpose.** The BCD396T / BCD996T command set.

**Definition.** `pub struct UnidenDigitalDialect;` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/uniden_digital.rs`; derives `Debug`, `Clone`, `Copy`.

**Used by.** `from_description` (functions whose signature names it).

**Notes.** A unit struct with no state: the dialect implementation of the `Dialect` trait for the BCD396T and BCD996T command set. `LineEngine<UnidenDigitalDialect>` supplies the buffering and reply handling.

## `UnidenDigitalTwin` [@ANCHOR: radioctl:radioctl_uniden_digital_uniden_digital_twin]

**Purpose.** The virtual BCD396T / BCD996T.

**Definition.** `pub struct UnidenDigitalTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/uniden_digital.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description the twin takes its defaults and tables from. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `UsbFrameStandIn` [@ANCHOR: radioctl:radioctl_usb_family_usb_frame_stand_in]

**Purpose.** A virtual USB device as a byte-in, byte-out stand-in: request frames in, reply frames out. The conformance harness replays transcripts through it, which is the wire the engine produces.

**Definition.** `pub struct UsbFrameStandIn` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/usb_family.rs`.

| Field | Type | Meaning |
|---|---|---|
| `device` | `Box<dyn UsbDevice>` | The virtual USB device behind the stand-in. |
| `pending` | `Vec<u8>` | Bytes received that do not yet form a complete request frame. |

**Used by.** `new` (functions whose signature names it).

## `WjEngine` [@ANCHOR: radioctl:radioctl_wj_wj_engine]

**Purpose.** The WJ-8888 client engine.

**Definition.** `pub struct WjEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/wj.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `hz` | `Hz` | The frequency last set; the engine remembers it because each command frame carries the whole state. |
| `mode` | `u8` | The mode code last set. |
| `width` | `u8` | The IF bandwidth code last set (initially the 8 kHz code, where Hamlib starts). |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `WjTwin` [@ANCHOR: radioctl:radioctl_wj_wj_twin]

**Purpose.** A virtual WJ-8888: it decodes the command frames.

**Definition.** `pub struct WjTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/wj.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `hz` | `Hz` | Current frequency in hertz. |
| `mode` | `u8` | Current mode, as the code the description's `[[modes]]` table uses. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Xg3Engine` [@ANCHOR: radioctl:radioctl_xg3_xg3_engine]

**Purpose.** The XG3 client engine.

**Definition.** `pub struct Xg3Engine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/xg3.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `Xg3Twin` [@ANCHOR: radioctl:radioctl_xg3_xg3_twin]

**Purpose.** A virtual XG3.

**Definition.** `pub struct Xg3Twin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/xg3.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `hz` | `u64` | Current frequency in hertz. |
| `output` | `bool` | Whether the RF output is on (`O,01` / `O,00`). |
| `level` | `u8` | The output level last set with `L`. |
| `memory` | `u8` | The selected memory number. |
| `backlight` | `u8` | The backlight setting. |
| `memories` | `[u64; 12]` | The twelve stored frequencies in hertz. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `LevelWire` [@ANCHOR: radioctl:radioctl_yaesu_level_wire_struct]

**Purpose.** How a level is written on the wire.

**Definition.** `struct LevelWire` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/yaesu.rs`.

| Field | Type | Meaning |
|---|---|---|
| `name` | `&'static str` | The command letter(s) of the level. |
| `leading_zero` | `bool` | True when the command carries a leading `0` (main receiver / fixed) before the digits. |
| `min` | `u16` | Smallest value the radio accepts. |
| `max` | `u16` | Largest value the radio accepts. |

**Used by.** `level_wire` (functions whose signature names it).

## `YaesuEngine` [@ANCHOR: radioctl:radioctl_yaesu_yaesu_engine]

**Purpose.** The Yaesu newcat client engine for one rig.

**Definition.** `pub struct YaesuEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/yaesu.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `tokenizer` | `Tokenizer` | Splits received bytes into `;`-terminated replies (`Tokenizer`). |
| `clar` | `(bool, bool)` | The last known receive and transmit clarifier switches, for the `CF` form that sets both in one command (`yaesu.clarifier = "cf-rx-tx"`): switching one keeps the other. |

**Used by.** `eight_digit`, `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `Pending` [@ANCHOR: radioctl:radioctl_yaesu_legacy_pending]

**Purpose.** What the engine is waiting for after a read.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/yaesu_legacy.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `FrequencyMode` | - | A read of the frequency and mode is outstanding. |
| `RxStatus` | - | A read of the receive status is outstanding. |
| `TxStatus` | - | A read of the transmit status is outstanding. |
| `Block` | `(usize)` | A block-protocol status read; the index is into `LegacyBlock::reads`. |

**Used by.** `encode_block` (functions whose signature names it).

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `YaesuLegacyEngine` [@ANCHOR: radioctl:radioctl_yaesu_legacy_yaesu_legacy_engine]

**Purpose.** The Yaesu legacy client engine for one rig.

**Definition.** `pub struct YaesuLegacyEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/yaesu_legacy.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `buffer` | `Vec<u8>` | Bytes received so far of the reply being assembled. |
| `last_byte_at` | `Duration` | Time (caller's monotonic clock) at which the last byte arrived, used to drop a stale partial frame. |
| `echo_of` | `Option<Vec<u8>>` | The command bytes a radio that echoes (FT-767GX) is expected to send back. |

**Used by.** `new`, `send` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.


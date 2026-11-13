# Radioctl protocols data structures, part 1 (adat to dwt)

As a developer changing this code, I want every data structure in `src/adat.rs`, `src/alinco.rs`, `src/anytone.rs`, `src/aor.rs`, `src/aor_classic.rs`, `src/ar7030.rs`, `src/barrett.rs`, `src/barrett4100.rs`, `src/codan.rs`, `src/commradio.rs`, `src/dorji.rs`, `src/drake.rs`, `src/drt1.rs` and `src/dwt.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `Pending` [@ANCHOR: radioctl:radioctl_adat_pending]

**Purpose.** What the ADT-200A engine is waiting for after its last request (`Nothing`, `Frequency`, `Mode` or `Ptt`).

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/adat.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Frequency` | - | A frequency read or write is outstanding; its answer is the frequency. |
| `Mode` | - | A mode read or write is outstanding; its answer is the mode. |
| `Ptt` | - | A transmit/receive switch command is outstanding; its answer is the acknowledgement. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `AdatEngine` [@ANCHOR: radioctl:radioctl_adat_adat_engine]

**Purpose.** The ADT-200A client engine.

**Definition.** `pub struct AdatEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/adat.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `AdatTwin` [@ANCHOR: radioctl:radioctl_adat_adat_twin]

**Purpose.** A virtual ADT-200A.

**Definition.** `pub struct AdatTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/adat.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `hz` | `Hz` | Current frequency in hertz. |
| `mode` | `u8` | Current mode, as the code the description's `[[modes]]` table uses. |
| `ptt` | `bool` | True while the twin is transmitting (keyed by the controller). |
| `codes` | `Vec<u8>` | The mode codes the description defines; the twin accepts a mode setting only if its code is in this list. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Dialect` [@ANCHOR: radioctl:radioctl_alinco_dialect]

**Purpose.** The two Alinco dialects.

**Definition.** `pub enum Dialect` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/alinco.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Dx77` | - | DX-77: `AL` commands ended by CR, one 26-character status line. |
| `Dxsr8` | - | DX-SR8: `AL~` commands ended by CR LF. |

**Used by.** `from_name` (functions whose signature names it).

## `Pending` [@ANCHOR: radioctl:radioctl_alinco_pending]

**Purpose.** What the Alinco engine is waiting for after its last request.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/alinco.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Set` | - | A set command was sent; its answer is an acknowledgement. |
| `Read` | `(Field)` | A read of this field was sent; the answer is the status line from which that field is taken. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `AlincoEngine` [@ANCHOR: radioctl:radioctl_alinco_alinco_engine]

**Purpose.** The Alinco client engine.

**Definition.** `pub struct AlincoEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/alinco.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `dialect` | `Dialect` | Which command set of the family the rig speaks. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |
| `echoed` | `bool` | Set once the radio's echo of the sent command has been consumed (the DX-SR8 echoes commands), cleared when a new command is encoded. |

**Used by.** `ask`, `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `AlincoTwin` [@ANCHOR: radioctl:radioctl_alinco_alinco_twin]

**Purpose.** A virtual Alinco radio.

**Definition.** `pub struct AlincoTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/alinco.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `dialect` | `Dialect` | The command set the twin emulates. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `hz` | `u64` | Current frequency in hertz. |
| `mode` | `u8` | Current mode, as the code the description's `[[modes]]` table uses. |
| `ptt` | `bool` | True while the twin is transmitting (keyed by the controller). |
| `codes` | `Vec<u8>` | The mode codes the description defines; the twin accepts a mode setting only if its code is in this list. |

**Used by.** `ask`, `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `AnytoneEngine` [@ANCHOR: radioctl:radioctl_anytone_anytone_engine]

**Purpose.** The AnyTone D578A client engine.

**Definition.** `pub struct AnytoneEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/anytone.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |
| `asked` | `Option<Vfo>` | Which VFO the outstanding frequency read was for; taken when the answer arrives. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `AnytoneTwin` [@ANCHOR: radioctl:radioctl_anytone_anytone_twin]

**Purpose.** A virtual AnyTone D578A.

**Definition.** `pub struct AnytoneTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/anytone.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `hz` | `[Hz; 2]` | Current frequency in hertz. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `AorEngine` [@ANCHOR: radioctl:radioctl_aor_aor_engine]

**Purpose.** The AOR engine.

**Definition.** `pub type AorEngine = LineEngine<AorDialect>;` (type) in `daemons/radioctl/crates/radioctl-protocols/src/aor.rs`.

**Used by.** `engine` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `SignalStyle` [@ANCHOR: radioctl:radioctl_aor_signal_style]

**Purpose.** How a model's `LM` / `LB` signal reading is written.

**Definition.** `enum SignalStyle` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/aor.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `HexMasked` | - | `LMxx`: two hex digits, bit 7 squelch status (AR8000). |
| `StatusDecimal` | - | `LM`, a status character, decimal digits (AR8200, AR8600). |
| `StatusHex` | - | `LM`, a status character, hex digits (AR5000, AR5000A, AR2700). |
| `Lb` | - | `LB`, a space, a signed decimal in dB offset by 73 (SR2200). |

## `AorDialect` [@ANCHOR: radioctl:radioctl_aor_aor_dialect]

**Purpose.** The AOR command set of one model.

**Definition.** `pub struct AorDialect` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/aor.rs`; derives `Debug`, `Clone`, `Copy`.

| Field | Type | Meaning |
|---|---|---|
| `signal` | `SignalStyle` | How this model writes its `LM`/`LB` signal reading (`SignalStyle`). |
| `vfo_tags` | `bool` | The AR8000 may answer `RX` with `VA` or `VB` in place of `RF`. |
| `echo_frequency` | `bool` | The SR2200 sends the frequency exactly (no rounding to 50 Hz) and answers a frequency setting with `RF` and the frequency, which Hamlib's `sr2200_set_freq` requires. |

**Used by.** `from_description` (functions whose signature names it).

## `AorTwin` [@ANCHOR: radioctl:radioctl_aor_aor_twin]

**Purpose.** The virtual AOR receiver.

**Definition.** `pub struct AorTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/aor.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `dialect` | `AorDialect` | The command set the twin emulates. |
| `modes` | `Vec<u8>` | The mode codes the description defines; the twin accepts a mode only if its code is in this list. |
| `desc` | `RigDescription` | The rig description the twin takes its defaults and tables from. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `AorClassicEngine` [@ANCHOR: radioctl:radioctl_aor_classic_aor_classic_engine]

**Purpose.** The engine of the AR3030 and AR3000A.

**Definition.** `pub type AorClassicEngine = LineEngine<AorClassicDialect>;` (type) in `daemons/radioctl/crates/radioctl-protocols/src/aor_classic.rs`.

**Used by.** `engine` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `Style` [@ANCHOR: radioctl:radioctl_aor_classic_style]

**Purpose.** Which of the two command sets a model speaks.

**Definition.** `enum Style` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/aor_classic.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Ar3030` | - | The AR3030 command set. |
| `Ar3000a` | - | The AR3000A command set. |

**Used by.** `style_of` (functions whose signature names it).

## `AorClassicDialect` [@ANCHOR: radioctl:radioctl_aor_classic_aor_classic_dialect]

**Purpose.** The command set of one of the two models.

**Definition.** `pub struct AorClassicDialect` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/aor_classic.rs`; derives `Debug`, `Clone`, `Copy`.

| Field | Type | Meaning |
|---|---|---|
| `style` | `Style` | Which of the two command sets the model speaks. |

**Used by.** `from_description` (functions whose signature names it).

## `AorClassicTwin` [@ANCHOR: radioctl:radioctl_aor_classic_aor_classic_twin]

**Purpose.** The virtual AR3030 or AR3000A.

**Definition.** `pub struct AorClassicTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/aor_classic.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `style` | `Style` | Which of the two command sets the twin emulates. |
| `desc` | `RigDescription` | The rig description the twin takes its defaults and tables from. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Ar7030Engine` [@ANCHOR: radioctl:radioctl_ar7030_ar7030_engine]

**Purpose.** The engine of the AR7030 and AR7030 Plus.

**Definition.** `pub struct Ar7030Engine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/ar7030.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `expecting` | `ReplyKind` | What kind of reply the last request needs (`ReplyKind`), `None` when none is outstanding. |
| `want` | `usize` | How many reply bytes the last request still expects. |
| `buffer` | `Vec<u8>` | Bytes received so far of the reply being assembled. |
| `last_byte_at` | `Duration` | Time (caller's monotonic clock) at which the last byte arrived, used to drop a stale partial frame. |

**Used by.** `engine` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `Ar7030Twin` [@ANCHOR: radioctl:radioctl_ar7030_ar7030_twin]

**Purpose.** The virtual AR7030 or AR7030 Plus: the radio's address, H and lock registers and page 0.

**Definition.** `pub struct Ar7030Twin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/ar7030.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description the twin takes its defaults and tables from. |
| `page` | `u8` | The memory page currently selected by the controller. |
| `address` | `u16` | The current address within the page (up to 12 bits, set in two parts). |
| `h` | `u8` | The radio's H register as last written by the controller. |
| `lock` | `u8` | The radio's lock register as last written by the controller. |
| `working` | `[u8; 256]` | Page 0 of the radio: 256 working bytes, including the frequency counts and the mode byte at their fixed offsets. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Tuning` [@ANCHOR: radioctl:radioctl_barrett_tuning]

**Purpose.** How the radio is tuned.

**Definition.** `enum Tuning` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/barrett.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Direct` | - | `TR` and `TC` (the 2050). |
| `Channel` | `(u32)` | Memory channels from this base number (the 950: 0, the 4050: 9990). |

**Used by.** `from_name` (functions whose signature names it).

## `Pending` [@ANCHOR: radioctl:radioctl_barrett_pending]

**Purpose.** What the Barrett engine is waiting for after its last request.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/barrett.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Frequency` | `(Vfo)` | A frequency read of this VFO is outstanding. |
| `Mode` | - | A mode read or write is outstanding; its answer is the mode. |
| `Ptt` | - | A transmit/receive switch command is outstanding; its answer is the acknowledgement. |
| `Ack` | `(usize)` | This many `OK` answers. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `BarrettEngine` [@ANCHOR: radioctl:radioctl_barrett_barrett_engine]

**Purpose.** The Barrett client engine.

**Definition.** `pub struct BarrettEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/barrett.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `tuning` | `Tuning` | How this model is tuned (`Tuning`). |
| `written` | `HashMap<u32, u64>` | The frequency this engine last wrote to each channel, to write only on a change. |
| `writing` | `Option<u32>` | The channel of a write in flight. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new`, `run` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `BarrettTwin` [@ANCHOR: radioctl:radioctl_barrett_barrett_twin]

**Purpose.** A virtual Barrett transceiver.

**Definition.** `pub struct BarrettTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/barrett.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `tuning` | `Tuning` | How this model is tuned (`Tuning`). |
| `selected` | `Option<u32>` | The selected memory channel and what the channels hold (receive, transmit hertz). |
| `channels` | `HashMap<u32, (u64, u64)>` | What each memory channel holds, as (receive, transmit) hertz, keyed by channel number. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `rx_hz` | `u64` | Receive frequency in hertz. |
| `tx_hz` | `u64` | Transmit frequency in hertz. |
| `mode` | `u8` | Current mode, as the code the description's `[[modes]]` table uses. |
| `codes` | `Vec<u8>` | The mode codes the description defines; the twin accepts a mode setting only if its code is in this list. |
| `ptt` | `bool` | True while the twin is transmitting (keyed by the controller). |

**Used by.** `new`, `run` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Expect` [@ANCHOR: radioctl:radioctl_barrett4100_expect]

**Purpose.** What the next answer is.

**Definition.** `enum Expect` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/barrett4100.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Session` | - | The answer to the remote-mode command. |
| `Writes` | `(u8)` | This many `s ...` answers to writes, the last one ends the operation. |
| `Frequency` | - | The receive frequency. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `Barrett4100Engine` [@ANCHOR: radioctl:radioctl_barrett4100_barrett4100_engine]

**Purpose.** The Barrett 4100 client engine.

**Definition.** `pub struct Barrett4100Engine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/barrett4100.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |
| `entered` | `bool` | Whether the remote-mode command has been sent yet; it goes first, once. |
| `expect` | `VecDeque<Expect>` | The answers still expected, oldest first (`Expect`). |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `Barrett4100Twin` [@ANCHOR: radioctl:radioctl_barrett4100_barrett4100_twin]

**Purpose.** A virtual Barrett 4100.

**Definition.** `pub struct Barrett4100Twin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/barrett4100.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `rx_hz` | `u64` | Receive frequency in hertz. |
| `tx_hz` | `u64` | Transmit frequency in hertz. |
| `keyed` | `bool` | True while the twin is transmitting. |
| `remote` | `bool` | True once the controller has sent the remote-mode command. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Pending` [@ANCHOR: radioctl:radioctl_codan_pending]

**Purpose.** What the CODAN engine is waiting for after its last request.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/codan.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Frequency` | - | A frequency read or write is outstanding; its answer is the frequency. |
| `Mode` | - | A mode read or write is outstanding; its answer is the mode. |
| `Ptt` | - | A transmit/receive switch command is outstanding; its answer is the acknowledgement. |
| `Ack` | - | A command that is only acknowledged is outstanding. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `CodanEngine` [@ANCHOR: radioctl:radioctl_codan_codan_engine]

**Purpose.** The CODAN client engine.

**Definition.** `pub struct CodanEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/codan.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `drop_next` | `bool` | A second answer line (`FREQ:` or `PTT`) the radio sends after a set is to be dropped. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new`, `run` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `CodanTwin` [@ANCHOR: radioctl:radioctl_codan_codan_twin]

**Purpose.** A virtual CODAN radio.

**Definition.** `pub struct CodanTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/codan.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `rx_hz` | `u64` | Receive frequency in hertz. |
| `tx_hz` | `u64` | Transmit frequency in hertz. |
| `mode` | `u8` | Current mode, as the code the description's `[[modes]]` table uses. |
| `ptt` | `bool` | True while the twin is transmitting (keyed by the controller). |

**Used by.** `new`, `run` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Pending` [@ANCHOR: radioctl:radioctl_commradio_pending]

**Purpose.** What the CTX-10 engine is waiting for after its last request.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/commradio.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Get` | - | A frequency read is outstanding. |
| `Set` | `(Hz)` | A frequency write is outstanding; the `Hz` is the value that was sent and is reported when the radio acknowledges. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `CommradioEngine` [@ANCHOR: radioctl:radioctl_commradio_commradio_engine]

**Purpose.** The CTX-10 client engine.

**Definition.** `pub struct CommradioEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/commradio.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `CommradioTwin` [@ANCHOR: radioctl:radioctl_commradio_commradio_twin]

**Purpose.** A virtual CTX-10.

**Definition.** `pub struct CommradioTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/commradio.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `hz` | `Hz` | Current frequency in hertz. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Band` [@ANCHOR: radioctl:radioctl_dorji_band]

**Purpose.** The two modules.

**Definition.** `pub enum Band` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/dorji.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Uhf` | - | DRA818U, 400 to 480 MHz. |
| `Vhf` | - | DRA818V, 134 to 174 MHz. |

**Used by.** `from_name` (functions whose signature names it).

## `DorjiEngine` [@ANCHOR: radioctl:radioctl_dorji_dorji_engine]

**Purpose.** The DRA818 client engine.

**Definition.** `pub struct DorjiEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/dorji.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `band` | `Band` | Which module this is (`Band`, DRA818U 400 to 480 MHz or DRA818V 134 to 174 MHz). |
| `hz` | `Hz` | The frequency currently written to the module (used for both transmit and receive). |
| `wide` | `bool` | True for the 25 kHz bandwidth setting, false for 12.5 kHz. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |
| `waiting` | `bool` | True after a command was sent and until its answer is read. |

**Used by.** `new`, `run` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `DorjiTwin` [@ANCHOR: radioctl:radioctl_dorji_dorji_twin]

**Purpose.** A virtual DRA818.

**Definition.** `pub struct DorjiTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/dorji.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `band` | `Band` | Which module the twin emulates. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `rx_hz` | `Hz` | Receive frequency in hertz. |
| `tx_hz` | `Hz` | Transmit frequency in hertz. |
| `wide` | `bool` | The bandwidth setting last written (true for 25 kHz). |

**Used by.** `new`, `run` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Pending` [@ANCHOR: radioctl:radioctl_drake_pending]

**Purpose.** What the Drake engine is waiting for after its last request.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/drake.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Frequency` | - | A frequency read or write is outstanding; its answer is the frequency. |
| `Mode` | - | A mode read or write is outstanding; its answer is the mode. |
| `Ack` | - | A command that is only acknowledged is outstanding. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `DrakeEngine` [@ANCHOR: radioctl:radioctl_drake_drake_engine]

**Purpose.** The Drake client engine.

**Definition.** `pub struct DrakeEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/drake.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `DrakeTwin` [@ANCHOR: radioctl:radioctl_drake_drake_twin]

**Purpose.** A virtual Drake receiver.

**Definition.** `pub struct DrakeTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/drake.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `units` | `u64` | The tuned frequency in units of 10 Hz (the unit of the radio's `F` command and of its seven-digit field). |
| `mode` | `Mode` | Current mode, as the code the description's `[[modes]]` table uses. |
| `codes` | `Vec<(Mode, u8)>` | The mode codes the description defines; the twin accepts a mode setting only if its code is in this list. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Drt1Engine` [@ANCHOR: radioctl:radioctl_drt1_drt1_engine]

**Purpose.** The DRT1 client engine. It has no bytes: its commands are line sequences.

**Definition.** `pub struct Drt1Engine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/drt1.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `DdsState` [@ANCHOR: radioctl:radioctl_drt1_dds_state]

**Purpose.** The state of the virtual DRT1 AD9951 DDS chip: the three control lines, the data bits clocked in since DTR (data terminal ready) went up and the register values it latched.

**Definition.** `struct DdsState` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/drt1.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rts` | `bool` | Level of the RTS line, which carries the data bit (inverted: asserted for a 0 bit). |
| `dtr` | `bool` | Level of the DTR line, which frames one register write (raised before the instruction byte, dropped after the last data bit, which latches the register). |
| `brk` | `bool` | Break state of the transmit line, which is the serial clock (a bit is taken when break is set; it is then cleared). |
| `bits` | `Vec<bool>` | Bits clocked in since DTR went up, oldest first (the value is the data bit). |
| `cfr2` | `Option<u32>` | Value of control register 2 (address 1, 3 bytes) once written, `None` before. |
| `ftw0` | `Option<u32>` | The 32-bit frequency tuning word (register 4) once written, `None` before. |
| `hz` | `Hz` | The frequency the latched tuning word stands for. |

**Used by.** `locked` (functions whose signature names it).

**Notes.** Shared between the controller side and the virtual receiver through an `Arc<Mutex<..>>` in the twin; `latch` interprets the clocked-in bits as an instruction and its data.

## `Drt1Twin` [@ANCHOR: radioctl:radioctl_drt1_drt1_twin]

**Purpose.** A virtual DRT1: the AD9951 serial port, decoded from the line changes. Clone it to hand one handle to the actor as its control lines and keep another to look at the tuning.

**Definition.** `pub struct Drt1Twin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/drt1.rs`; derives `Debug`, `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `state` | `Arc<Mutex<DdsState>>` | The DDS state shared with the virtual receiver (`DdsState` behind `Arc<Mutex<..>>`). |

**Used by.** `drive`, `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `DwtEngine` [@ANCHOR: radioctl:radioctl_dwt_dwt_engine]

**Purpose.** The Digital World Traveller client engine.

**Definition.** `pub struct DwtEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/dwt.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `waiting` | `bool` | True after a request was sent and until its reply is read. |
| `rx` | `Vec<u8>` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `DwtTwin` [@ANCHOR: radioctl:radioctl_dwt_dwt_twin]

**Purpose.** A virtual Traveller: remembers the last kilohertz it was tuned to.

**Definition.** `pub struct DwtTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/dwt.rs`; derives `Debug`, `Default`.

| Field | Type | Meaning |
|---|---|---|
| `khz` | `Option<u16>` | The tuned frequency in kilohertz as last set by the controller, `None` before. |

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.


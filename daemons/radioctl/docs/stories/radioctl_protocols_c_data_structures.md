# Radioctl protocols data structures, part 3 (icom_marine to paragon)

As a developer changing this code, I want every data structure in `src/icom_marine.rs`, `src/icom_pcr.rs`, `src/io.rs`, `src/jrc.rs`, `src/kachina.rs`, `src/kenwood.rs`, `src/kenwood_hh.rs`, `src/kenwood_ts890.rs`, `src/kenwood_ts890_lan.rs`, `src/lowe.rs`, `src/mds.rs`, `src/micom.rs` and `src/paragon.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `MarineEngine` [@ANCHOR: radioctl:radioctl_icom_marine_marine_engine]

**Purpose.** The marine engine.

**Definition.** `pub type MarineEngine = LineEngine<MarineDialect>;` (type) in `daemons/radioctl/crates/radioctl-protocols/src/icom_marine.rs`.

**Used by.** `engine` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `MarineDialect` [@ANCHOR: radioctl:radioctl_icom_marine_marine_dialect]

**Purpose.** The command set of one model.

**Definition.** `pub struct MarineDialect` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/icom_marine.rs`; derives `Debug`, `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `id` | `u8` | The radio's two-digit remote id (the description's `address`), which every `$PICOA` sentence carries after the controller's id 90. |
| `rtty_name` | `&'static str` | The name this model reports for its RTTY mode, taken from the description's dialect text. |
| `waiting` | `Vec<String>` | Echoes still to arrive for the request in flight, with the command each must carry. |

**Used by.** `from_description` (functions whose signature names it).

## `Fields` [@ANCHOR: radioctl:radioctl_icom_marine_fields]

**Purpose.** The parts of a received `$PICOA` sentence.

**Definition.** `struct Fields<'a>` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/icom_marine.rs`.

| Field | Type | Meaning |
|---|---|---|
| `command_and_param` | `&'a str` | Everything after the radio ids: the command and its parameter. |
| `last` | `&'a str` | The text after the last comma before the checksum. |

**Used by.** `parse` (functions whose signature names it).

## `MarineTwin` [@ANCHOR: radioctl:radioctl_icom_marine_marine_twin]

**Purpose.** The virtual marine radio.

**Definition.** `pub struct MarineTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/icom_marine.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description the twin takes its defaults and tables from. |
| `dialect` | `MarineDialect` | The command set the twin emulates. |
| `tx` | `Hz` | The transmit frequency the controller last set. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `PcrEngine` [@ANCHOR: radioctl:radioctl_icom_pcr_pcr_engine]

**Purpose.** The PCR engine.

**Definition.** `pub type PcrEngine = LineEngine<PcrDialect>;` (type) in `daemons/radioctl/crates/radioctl-protocols/src/icom_pcr.rs`.

**Used by.** `bytes`, `engine` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `PcrDialect` [@ANCHOR: radioctl:radioctl_icom_pcr_pcr_dialect]

**Purpose.** The PCR command set: it remembers what it last set because the radio cannot say.

**Definition.** `pub struct PcrDialect` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/icom_pcr.rs`; derives `Debug`, `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `lead` | `bool` | The PCR1000 puts one extra character in front of each answer (`reply_offset` 1 in Hamlib). |
| `freq` | `Hz` | The frequency last set (the radio cannot report it). |
| `mode` | `u8` | The mode code last set (the radio cannot report it). |
| `pending` | `Option<(Hz, u8)>` | A tuning command sent and not yet accepted: the frequency and mode it carried. |

**Used by.** `from_description` (functions whose signature names it).

## `PcrTwin` [@ANCHOR: radioctl:radioctl_icom_pcr_pcr_twin]

**Purpose.** The virtual PCR receiver.

**Definition.** `pub struct PcrTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/icom_pcr.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `lead` | `bool` | True when the twin emulates the PCR1000, which puts one extra character in front of each answer. |
| `desc` | `RigDescription` | The rig description the twin takes its defaults and tables from. |
| `filter` | `u8` | The filter code last set (the engine always sends `2`, 15 kHz). |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Shared` [@ANCHOR: radioctl:radioctl_io_shared]

**Purpose.** State shared by a served virtual rig's connection task and its control handle: the virtual rig, the reply delay and jitter, the fault-injection counters and the outbox of delayed answers.

**Definition.** `struct Shared` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/io.rs`.

| Field | Type | Meaning |
|---|---|---|
| `rig` | `Mutex<VirtualRig>` | The virtual radio (`VirtualRig`), behind a mutex. |
| `delay` | `Mutex<Duration>` | How long each answer is held back, to imitate the radio's speed. |
| `jitter` | `Mutex<Duration>` | Added to the delay at random, up to this much (fault injection: an uneven radio). |
| `jitter_state` | `Mutex<u64>` | State of the jitter's own generator (xorshift, seeded; the tests are repeatable). |
| `received` | `AtomicUsize` | How many bytes the virtual rig has received from the controller. |
| `drop_replies` | `AtomicUsize` | Answers still to be lost on the way back (fault injection: a corrupted frame). |
| `truncate_replies` | `AtomicUsize` | Answers still to lose their last byte (fault injection: a frame cut short). |
| `echo` | `AtomicBool` | True when the controller's own bytes come back first (a single-wire bus echo). |
| `chunk` | `AtomicUsize` | Largest write of an answer; 0 writes each answer whole. |
| `outbox` | `mpsc::UnboundedSender<(Instant, Vec<u8>)>` | Queue of answers with the time each is to be written (`Instant`), drained by the writer task. |
| `epoch` | `Instant` | The instant the virtual rig started, from which the virtual clock is measured. |

**Used by.** `next_jitter` (functions whose signature names it).

**Notes.** Every field is behind a mutex or an atomic so the control handle (which is `Clone`) and the task can use it at once; there is no other locking order.

## `VirtualRigControl` [@ANCHOR: radioctl:radioctl_io_virtual_rig_control]

**Purpose.** Control of a running virtual rig: inspect its state, turn its knobs, change its speed.

**Definition.** `pub struct VirtualRigControl` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/io.rs`; derives `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `shared` | `Arc<Shared>` | The shared state of the served virtual rig (`Arc<Shared>`); the handle is `Clone`. |

**Used by.** `spawn_virtual_rig` (functions whose signature names it).

## `JrcEngine` [@ANCHOR: radioctl:radioctl_jrc_jrc_engine]

**Purpose.** The JRC engine.

**Definition.** `pub type JrcEngine = LineEngine<JrcDialect>;` (type) in `daemons/radioctl/crates/radioctl-protocols/src/jrc.rs`.

**Used by.** `bytes`, `engine` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `Kind` [@ANCHOR: radioctl:radioctl_jrc_kind]

**Purpose.** Which JRC command set a model speaks.

**Definition.** `enum Kind` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/jrc.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nrd` | - | NRD-535D and NRD-545 DSP: digits of hertz, state through `I`. |
| `Nrd525` | - | NRD-525: write only, units of 10 Hz, no terminator. |
| `Jst` | - | JST-145 and JST-245. |

**Used by.** `set` (functions whose signature names it).

## `JrcDialect` [@ANCHOR: radioctl:radioctl_jrc_jrc_dialect]

**Purpose.** The JRC command set of one model.

**Definition.** `pub struct JrcDialect` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/jrc.rs`; derives `Debug`, `Clone`, `Copy`.

| Field | Type | Meaning |
|---|---|---|
| `kind` | `Kind` | Which command set the model speaks (`Kind`). |

**Used by.** `from_description` (functions whose signature names it).

## `JrcTwin` [@ANCHOR: radioctl:radioctl_jrc_jrc_twin]

**Purpose.** The virtual JRC radio.

**Definition.** `pub struct JrcTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/jrc.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description the twin takes its defaults and tables from. |
| `dialect` | `JrcDialect` | The command set the twin emulates. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `KachinaEngine` [@ANCHOR: radioctl:radioctl_kachina_kachina_engine]

**Purpose.** The Kachina client engine.

**Definition.** `pub struct KachinaEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kachina.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `owed` | `usize` | How many reply bytes the last request still owes (what to read before the next command). |

**Used by.** `new`, `run` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `KachinaTwin` [@ANCHOR: radioctl:radioctl_kachina_kachina_twin]

**Purpose.** A virtual Kachina 505DSP.

**Definition.** `pub struct KachinaTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kachina.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `rx_hz` | `Hz` | Receive frequency in hertz. |
| `tx_hz` | `Hz` | Transmit frequency in hertz. |
| `mode` | `u8` | Current mode, as the code the description's `[[modes]]` table uses. |
| `codes` | `Vec<u8>` | The mode codes the description defines; the twin accepts a mode setting only if its code is in this list. |
| `ptt` | `bool` | True while the twin is transmitting (keyed by the controller). |

**Used by.** `new`, `run` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Token` [@ANCHOR: radioctl:radioctl_kenwood_token]

**Purpose.** What the tokenizer found.

**Definition.** `pub enum Token` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Command` | `(String)` | A command or reply without its terminator (`FA00014074000`). |
| `Junk` | `(Vec<u8>)` | Bytes that were not a printable-ASCII command. |

**Used by.** `feed` (functions whose signature names it).

## `Tokenizer` [@ANCHOR: radioctl:radioctl_kenwood_tokenizer]

**Purpose.** Splits a byte stream on `;`. Holds a partial command between calls.

**Definition.** `pub struct Tokenizer` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood.rs`; derives `Debug`, `Default`.

| Field | Type | Meaning |
|---|---|---|
| `pending` | `Vec<u8>` | Bytes of the command being received, without its `;` yet. |
| `last_byte_at` | `Duration` | When the last byte arrived, used to turn a stale partial command into junk. |
| `max_pending` | `usize` | Bytes buffered without a terminator before the stream is called junk; 0 means [`MAX_PENDING`]. |

**Used by.** `with_max_pending` (functions whose signature names it).

## `KenwoodEngine` [@ANCHOR: radioctl:radioctl_kenwood_kenwood_engine]

**Purpose.** The Kenwood-style client engine for one rig.

**Definition.** `pub struct KenwoodEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `tokenizer` | `Tokenizer` | Splits received bytes into `;`-terminated commands and junk (`Tokenizer`). |
| `older` | `bool` | The older dialect (`kenwood.dialect` `ts440` or `ts450`). |
| `fn_sp` | `bool` | Split through `FN` and `SP` (the TS-440S) and not through `FR` and `FT` (the TS-450S). |
| `ts990` | `bool` | The TS-990S dialect (`kenwood.dialect = "ts990"`): the mode through `OM0`, no `MD`. |
| `r5000` | `bool` | The R-5000 dialect: the `IF` answer lacks five characters after the frequency. |
| `if_frequency` | `bool` | The TRC-80 dialect: the frequency is read from `IF`. |
| `transceive` | `bool` | The auto-information the controller turned on (`AI` set to on, [`Op::SetTransceive`]) and the rig loses with the link; sent again by [`ProtocolEngine::link_restored`]. |

**Used by.** `engine`, `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `LevelWire` [@ANCHOR: radioctl:radioctl_kenwood_level_wire_struct]

**Purpose.** How a level is written on the wire: the command, whether a leading `0` (main receiver) follows it, and the largest value.

**Definition.** `struct LevelWire` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood.rs`.

| Field | Type | Meaning |
|---|---|---|
| `name` | `&'static str` | The command name of the level. |
| `main_digit` | `bool` | Whether a leading `0` (main receiver) follows the name. |
| `min` | `u16` | Smallest value the rig accepts. |
| `max` | `u16` | Largest value the rig accepts. |

**Used by.** `level_wire` (functions whose signature names it).

## `Pending` [@ANCHOR: radioctl:radioctl_kenwood_hh_pending]

**Purpose.** What the engine is waiting for.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood_hh.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Frequency` | `(usize)` | A frequency read or write is outstanding; its answer is the frequency. |
| `Mode` | - | A mode read or write is outstanding; its answer is the mode. |
| `Signal` | - | A signal-strength read is outstanding. |
| `Ack` | `(Option<(usize, String)>)` | The echo of a setting; for an `FO` set, the band and line to remember once it is accepted. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `KenwoodHhEngine` [@ANCHOR: radioctl:radioctl_kenwood_hh_kenwood_hh_engine]

**Purpose.** The Kenwood handheld/mobile client engine for one rig.

**Definition.** `pub struct KenwoodHhEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood_hh.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `dialect` | `HhDialect` | Which command set of the family the rig speaks. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `buffer` | `Vec<u8>` | Bytes received so far of the reply being assembled. |
| `last_byte_at` | `Duration` | Time (caller's monotonic clock) at which the last byte arrived, used to drop a stale partial frame. |
| `lines` | `[Option<String>; 2]` | The last `FO` line of each of the two bands, remembered once the radio has accepted the setting. |

**Used by.** `new`, `run` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `HhSim` [@ANCHOR: radioctl:radioctl_kenwood_hh_hh_sim]

**Purpose.** The virtual handheld: the radio side of a kenwood-hh description, kept apart from the engine.

**Definition.** `pub struct HhSim` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood_hh.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `dialect` | `HhDialect` | Which handheld dialect the virtual radio speaks (`HhDialect`). |
| `freq` | `[u64; 2]` | The frequency of each of the two bands in hertz. |
| `mode` | `[u8; 2]` | The mode code of each of the two bands. |
| `buffer` | `Vec<u8>` | Bytes of the command being received. |
| `last_byte_at` | `Duration` | When the last byte arrived, used to drop a stale partial command. |

**Used by.** `new` (functions whose signature names it).

## `Splits` [@ANCHOR: radioctl:radioctl_kenwood_ts890_splits]

**Purpose.** A serial scope sweep arriving in splits: the digits so far and the split expected next.

**Definition.** `struct Splits` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood_ts890.rs`; derives `Debug`, `Default`.

| Field | Type | Meaning |
|---|---|---|
| `next` | `u8` | The split (0-based) expected next. |
| `digits` | `String` | The sample digits collected so far. |
| `broken` | `bool` | A split of this sweep was bad and counted; the rest of the sweep is skipped without counting. |

## `LevelWire` [@ANCHOR: radioctl:radioctl_kenwood_ts890_level_wire]

**Purpose.** How a level is written: the command and the guide's range.

**Definition.** `struct LevelWire` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood_ts890.rs`.

| Field | Type | Meaning |
|---|---|---|
| `name` | `&'static str` | The command name of the level. |
| `min` | `u16` | Smallest value the guide allows. |
| `max` | `u16` | Largest value the guide allows. |

**Used by.** `level_wire` (functions whose signature names it).

## `Ts890Engine` [@ANCHOR: radioctl:radioctl_kenwood_ts890_ts890_engine]

**Purpose.** The TS-890S client engine for one rig.

**Definition.** `pub struct Ts890Engine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood_ts890.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `tokenizer` | `Tokenizer` | Splits received bytes into `;`-terminated commands and junk (`Tokenizer`). |
| `transceive` | `bool` | `AI2;` was sent and the rig loses it with the LAN or serial session; sent again by [`ProtocolEngine::link_restored`]. (The LAN login itself is the framing's, repeated by `net::open_link` on every reconnect, not the engine's.) |
| `scope_output` | `bool` | The scope output was asked for; sent again after the link is restored. |
| `scope` | `ScopeContext` | What the frame edges are computed from. |
| `dd2` | `Splits` | Serial `DD2` and `DD3` sweeps being assembled. |
| `dd3` | `Splits` | The serial `DD3` sweep (the right-edge samples) being assembled. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `ScopeContext` [@ANCHOR: radioctl:radioctl_kenwood_ts890_scope_context]

**Purpose.** What a `##DD2` frame (which has no frequency) is placed with.

**Definition.** `struct ScopeContext` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood_ts890.rs`; derives `Debug`, `Default`.

| Field | Type | Meaning |
|---|---|---|
| `freq_a` | `Option<Hz>` | The last known frequency of VFO A, or `None`. |
| `freq_b` | `Option<Hz>` | The last known frequency of VFO B, or `None`. |
| `rx_vfo` | `Option<Vfo>` | Which VFO is the receive VFO, or `None` if unknown. |
| `axis` | `Option<ScopeAxis>` | The scope's axis mode as last read, or `None`. |
| `span` | `Option<Hz>` | The scope's span as last read, or `None`. |
| `edges` | `Option<(Hz, Hz)>` | The lower and upper edge of the fixed range in use, from the last `BSM` read. |
| `expand` | `Option<bool>` | `BSO` (expanded analysis range) as last read or set. |
| `dropped` | `u64` | How many scope frames were dropped because the state needed to place them was unknown. |

## `ScopeWire` [@ANCHOR: radioctl:radioctl_kenwood_ts890_scope_wire]

**Purpose.** How a scope setting is written: the `BS` letter, the smallest and largest digit the guide allows, and whether the guide has a set form.

**Definition.** `struct ScopeWire` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood_ts890.rs`.

| Field | Type | Meaning |
|---|---|---|
| `letter` | `char` | The letter after `BS` that names the setting. |
| `min` | `u8` | Smallest digit the guide allows. |
| `max` | `u8` | Largest digit the guide allows. |
| `writable` | `bool` | Whether the guide has a set form of this command. |

**Used by.** `scope_setting_wire` (functions whose signature names it).

## `LanCredentials` [@ANCHOR: radioctl:radioctl_kenwood_ts890_lan_lan_credentials]

**Purpose.** A LAN account for the radio.

**Definition.** `pub struct LanCredentials` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood_ts890_lan.rs`; derives `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `administrator` | `bool` | True for the administrator account, false for a user account. |
| `account` | `String` | The account name sent in the login frame. |
| `password` | `String` | The password sent in the login frame. A secret: it is held in memory for the session, sent only to the radio, and must not be logged or put in an error text. |

**Used by.** `new` (functions whose signature names it).

## `Phase` [@ANCHOR: radioctl:radioctl_kenwood_ts890_lan_phase]

**Purpose.** How far the LAN session's login has got.

**Definition.** `enum Phase` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood_ts890_lan.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `AwaitConnect` | - | Waiting for the radio's `##CN` reply to the connect request. |
| `AwaitLogin` | - | Waiting for the radio's `##ID` reply to the login. |
| `Up` | - | The session is up; framing is transparent and keep-alive probes run. |

## `Ts890LanFraming` [@ANCHOR: radioctl:radioctl_kenwood_ts890_lan_ts890_lan_framing]

**Purpose.** The TS-890S LAN session framing.

**Definition.** `pub struct Ts890LanFraming` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood_ts890_lan.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `credentials` | `LanCredentials` | The account and password to log in with (`LanCredentials`). |
| `keepalive_every` | `Duration` | How often a keep-alive probe is sent while the session is up. |
| `phase` | `Phase` | Where the login handshake stands (`Phase`). |
| `pending` | `Vec<u8>` | Bytes received that do not yet form a complete line. |
| `probes_out` | `u32` | Keep-alive probes sent and not yet answered. |

**Used by.** `new`, `session`, `with_keepalive` (functions whose signature names it).

## `Pending` [@ANCHOR: radioctl:radioctl_lowe_pending]

**Purpose.** What the engine is waiting for after the last request it sent, so the next reply is read as its answer.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/lowe.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Frequency` | - | A frequency read or write is outstanding; its answer is the frequency. |
| `Mode` | - | A mode read or write is outstanding; its answer is the mode. |
| `Ack` | - | A set command is outstanding; its answer is an acknowledgement. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `LoweEngine` [@ANCHOR: radioctl:radioctl_lowe_lowe_engine]

**Purpose.** The Lowe client engine.

**Definition.** `pub struct LoweEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/lowe.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new`, `run` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `LoweTwin` [@ANCHOR: radioctl:radioctl_lowe_lowe_twin]

**Purpose.** A virtual HF-235.

**Definition.** `pub struct LoweTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/lowe.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `hz` | `Hz` | Current frequency in hertz. |
| `mode` | `u8` | Current mode, as the code the description's `[[modes]]` table uses. |

**Used by.** `new`, `run` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `MdsEngine` [@ANCHOR: radioctl:radioctl_mds_mds_engine]

**Purpose.** The MDS client engine.

**Definition.** `pub struct MdsEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/mds.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `waiting` | `bool` | True after a request that expects an answer was sent, until it is read. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `MdsTwin` [@ANCHOR: radioctl:radioctl_mds_mds_twin]

**Purpose.** A virtual MDS radio.

**Definition.** `pub struct MdsTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/mds.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `tx_hz` | `Hz` | Transmit frequency in hertz. |
| `rx_hz` | `Hz` | Receive frequency in hertz. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `MicomEngine` [@ANCHOR: radioctl:radioctl_micom_micom_engine]

**Purpose.** The MICOM client engine.

**Definition.** `pub struct MicomEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/micom.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `MicomTwin` [@ANCHOR: radioctl:radioctl_micom_micom_twin]

**Purpose.** A virtual MICOM.

**Definition.** `pub struct MicomTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/micom.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `hz` | `u32` | Current frequency in hertz. |
| `keyed` | `bool` | True while the twin is transmitting. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `ParagonEngine` [@ANCHOR: radioctl:radioctl_paragon_paragon_engine]

**Purpose.** The Paragon client engine.

**Definition.** `pub struct ParagonEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/paragon.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `ParagonTwin` [@ANCHOR: radioctl:radioctl_paragon_paragon_twin]

**Purpose.** A virtual Paragon.

**Definition.** `pub struct ParagonTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/paragon.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `units` | `u64` | The frequency in units of 10 Hz. |
| `mode_bit` | `u8` | The mode as the status block's bit (0x02 CW, 0x04 USB, 0x08 LSB, 0x10 AM, 0x20 FM, 0x40 RTTY). |
| `filter_bit` | `u8` | The filter as the status block's bit (0x08 250 Hz, 0x10 500 Hz, 0x20 1.8 kHz, 0x40 2.4 kHz, 0x80 6 kHz). |
| `vfo_a` | `bool` | True when VFO A is selected (bit 0x08 of byte 9 of the status block). |
| `split` | `bool` | True when split is on (bit 0x02 of byte 9). |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.


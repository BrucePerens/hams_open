# Radioctl protocols data structures, part 6 (civ, scope, conformance and the virtual rigs)

As a developer changing this code, I want every data structure in `src/civ.rs`, `src/civ_scope.rs`, `src/conformance.rs`, `src/virtual_flex.rs`, `src/virtual_rig.rs` and `src/virtual_ts890.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `Frame` [@ANCHOR: radioctl:radioctl_civ_frame]

**Purpose.** One decoded frame.

**Definition.** `pub struct Frame` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/civ.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `to` | `u8` | Destination address. |
| `from` | `u8` | Source address. |
| `cmd` | `u8` | Command byte. |
| `data` | `Vec<u8>` | Sub-command and data bytes. |

**Used by.** `decode` (functions whose signature names it).

## `Parsed` [@ANCHOR: radioctl:radioctl_civ_parsed]

**Purpose.** What the frame parser found.

**Definition.** `pub enum Parsed` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/civ.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Frame` | `(Frame)` | A well-formed frame. |
| `Junk` | `(Vec<u8>)` | Bytes that were not part of any frame. |

**Used by.** `drop_leading_junk`, `feed`, `frame_or_junk` (functions whose signature names it).

## `FrameParser` [@ANCHOR: radioctl:radioctl_civ_frame_parser]

**Purpose.** Splits a byte stream into CI-V frames. Holds partial frames between calls.

**Definition.** `pub struct FrameParser` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/civ.rs`; derives `Debug`, `Default`.

| Field | Type | Meaning |
|---|---|---|
| `pending` | `Vec<u8>` | Bytes of the frame being received (from its preamble), not complete yet. |
| `last_byte_at` | `Duration` | When the last byte arrived, used to turn a stale partial frame into junk. |

## `CivEngine` [@ANCHOR: radioctl:radioctl_civ_civ_engine]

**Purpose.** The CI-V client engine for one rig.

**Definition.** `pub struct CivEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/civ.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `params` | `CivParams` | The model's CI-V parameters from the description (`CivParams`: address, command variants, tables). |
| `parser` | `FrameParser` | Splits received bytes into CI-V frames and junk (`FrameParser`). |
| `mode` | `Option<Mode>` | The last mode the rig reported; the filter index tables depend on it. |
| `filter_number` | `u8` | The filter number (1 to 3) the rig last reported with its mode; resent with `26 00`. |
| `scope` | `Option<ScopeAssembler>` | The sweep assembler for the spectrum scope, `None` for a model without one. |
| `awaiting_ptt_ack` | `bool` | True between a PTT (push to talk) set and the next reply: some Xiegu radios answer it with `1C 00 <state>` instead of `FB` (Hamlib's `x108g_set_ptt` comment), which counts as the acknowledgement. |

**Used by.** `engine`, `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `ScopeMode` [@ANCHOR: radioctl:radioctl_civ_scope_scope_mode]

**Purpose.** How the scope's frequency axis is anchored.

**Definition.** `pub enum ScopeMode` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/civ_scope.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Center` | - | Centred on the receive frequency. |
| `Fixed` | - | Between fixed edges. |
| `ScrollCenter` | - | Scrolling, centred. |
| `ScrollFixed` | - | Scrolling, fixed edges. |

**Used by.** `from`, `from_code` (functions whose signature names it).

## `ScopeSpeed` [@ANCHOR: radioctl:radioctl_civ_scope_scope_speed]

**Purpose.** Sweep speed.

**Definition.** `pub enum ScopeSpeed` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/civ_scope.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Fast` | - | Fast sweep. |
| `Mid` | - | Medium sweep. |
| `Slow` | - | Slow sweep. |

**Used by.** `from_code` (functions whose signature names it).

## `ScopeCommand` [@ANCHOR: radioctl:radioctl_civ_scope_scope_command]

**Purpose.** A scope setting or query. `receiver` is 0 (Main) or 1 (Sub).

**Definition.** `pub enum ScopeCommand` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/civ_scope.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `SetDisplay` | `(bool)` | Scope display on or off. |
| `GetDisplay` | - | Read the display state. |
| `SetOutput` | `(bool)` | Waveform data output on or off (the rig then sends sweeps on its own). |
| `GetOutput` | - | Read the output state. |
| `SetMode` | `(u8, ScopeMode)` | Centre or fixed mode. |
| `GetMode` | `(u8)` | Read the mode. |
| `SetSpan` | `(u8, Hz)` | Full span in hertz (sent as the half-span the rig expects). |
| `GetSpan` | `(u8)` | Read the span. |
| `SetHold` | `(u8, bool)` | Hold on or off. |
| `SetReference` | `(u8, i16)` | Reference level in hundredths of a dB. |
| `GetReference` | `(u8)` | Read the reference level. |
| `SetSpeed` | `(u8, ScopeSpeed)` | Sweep speed. |
| `GetSpeed` | `(u8)` | Read the sweep speed. |
| `SetEdges` | `(u8, u8, Hz, Hz)` | Fixed-mode edges: range number, edge number (1 based), lower and upper edge. |

**Used by.** `command_data` (functions whose signature names it).

## `ScopeSegment` [@ANCHOR: radioctl:radioctl_civ_scope_scope_segment]

**Purpose.** One waveform frame, decoded.

**Definition.** `pub struct ScopeSegment` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/civ_scope.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `receiver` | `u8` | 0 (Main) or 1 (Sub). |
| `division` | `u8` | Frame number, from 1. |
| `divisions` | `u8` | Number of frames in the sweep. |
| `header` | `Option<ScopeHeader>` | Present in frame 1 only. |
| `samples` | `Vec<u8>` | The amplitude samples this frame carries. |

**Used by.** `parse_segment`, `push` (functions whose signature names it).

## `ScopeHeader` [@ANCHOR: radioctl:radioctl_civ_scope_scope_header]

**Purpose.** The first frame's description of the sweep.

**Definition.** `pub struct ScopeHeader` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/civ_scope.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `mode` | `ScopeMode` | How the axis is anchored. |
| `edge_a` | `Hz` | Lower edge, or centre frequency in the centred modes. |
| `edge_b` | `Hz` | Upper edge, or half-span in the centred modes. |
| `out_of_range` | `bool` | True when the receive frequency is outside a fixed scope. |

## `ScopeLine` [@ANCHOR: radioctl:radioctl_civ_scope_scope_line]

**Purpose.** One complete sweep.

**Definition.** `pub struct ScopeLine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/civ_scope.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `receiver` | `u8` | 0 (Main) or 1 (Sub). |
| `mode` | `ScopeMode` | How the axis was anchored. |
| `low` | `Hz` | Lowest frequency on the axis. |
| `high` | `Hz` | Highest frequency on the axis. |
| `out_of_range` | `bool` | True when the receive frequency is outside a fixed scope. |
| `samples` | `Vec<u8>` | One amplitude per pixel, 0 to the model's `amplitude_max`. |

**Used by.** `line_from`, `push` (functions whose signature names it).

## `ScopeAssembler` [@ANCHOR: radioctl:radioctl_civ_scope_scope_assembler]

**Purpose.** Rebuilds sweeps from waveform frames, one receiver at a time. A frame out of order discards the sweep in progress; a sweep with the wrong number of samples is never delivered.

**Definition.** `pub struct ScopeAssembler` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/civ_scope.rs`; derives `Debug`, `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `params` | `ScopeParams` | The model's scope parameters (`ScopeParams`: receivers, samples per sweep, amplitude range). |
| `current` | `[Option<Partial>; 2]` | The sweep in progress for each of the two receivers (`Partial`), `None` when none. |
| `dropped` | `u64` | How many sweeps or frames were discarded (out of order, wrong sample count); read with `dropped()`. |

**Used by.** `new` (functions whose signature names it).

## `Partial` [@ANCHOR: radioctl:radioctl_civ_scope_partial]

**Purpose.** A sweep that is being rebuilt from waveform frames: the first frame's header, the number of the next frame expected and the samples collected so far. Private to `civ_scope.rs`.

**Definition.** `struct Partial` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/civ_scope.rs`; derives `Debug`, `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `header` | `ScopeHeader` | The header carried by frame 1. |
| `next_division` | `u8` | The frame number expected next; a frame out of order discards the sweep. |
| `samples` | `Vec<u8>` | The amplitude samples collected so far. |

**Used by.** `line_from` (functions whose signature names it).

## `ScopeAnswer` [@ANCHOR: radioctl:radioctl_civ_scope_scope_answer]

**Purpose.** A virtual scope's answer to a command.

**Definition.** `pub enum ScopeAnswer` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/civ_scope.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Ok` | - | Accepted; the CI-V OK frame follows. |
| `Data` | `(Vec<u8>)` | A reply: send as `27 <data>`. |
| `Refused` | - | Refused; the CI-V NG frame follows. |

**Used by.** `handle` (functions whose signature names it).

## `SimScope` [@ANCHOR: radioctl:radioctl_civ_scope_sim_scope]

**Purpose.** State of one receiver's scope in a virtual rig.

**Definition.** `struct SimScope` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/civ_scope.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `mode` | `ScopeMode` | How the axis is anchored. |
| `half_span` | `Hz` | Half the span, in hertz. |
| `hold` | `bool` | Whether hold is on. |
| `reference_centi_db` | `i16` | The reference level in hundredths of a dB. |
| `speed` | `ScopeSpeed` | The sweep speed. |

## `ScopeSim` [@ANCHOR: radioctl:radioctl_civ_scope_scope_sim]

**Purpose.** The scope of a virtual rig: settings per receiver and a synthetic sweep generator.

**Definition.** `pub struct ScopeSim` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/civ_scope.rs`; derives `Debug`, `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `params` | `ScopeParams` | The model's scope parameters (`ScopeParams`). |
| `display` | `bool` | Whether the scope display is on. |
| `output` | `bool` | Whether waveform output is on (the rig then sends sweeps on its own). |
| `receivers` | `Vec<SimScope>` | The settings of each receiver's scope (`SimScope`), one per receiver. |
| `edges` | `Option<(Hz, Hz)>` | The last fixed-mode edges set (any range and edge number): the sweep of a fixed scope uses them instead of the tuned frequency's neighbourhood. |
| `sweeps` | `u32` | How many sweeps have been generated; the synthetic sweep varies with this counter. |

**Used by.** `new` (functions whose signature names it).

## `StandIn` [@ANCHOR: radioctl:radioctl_conformance_stand_in]

**Purpose.** The radio's stand-in a transcript replays against: the family's virtual radio, or, for an OpenHPSDR description, a stand-in for the `hpsdr_io` bridge the engine really talks to.

**Definition.** `enum StandIn` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/conformance.rs`.

| Variant | Type | Meaning |
|---|---|---|
| `Rig` | `(Box<VirtualRig>)` | The family's virtual rig (`VirtualRig`), for the CI-V, Kenwood, Elecraft and Yaesu families. |
| `Flex` | `(Box<VirtualFlex>)` | The virtual Flex radio (`VirtualFlex`). |
| `Vendor` | `(Box<dyn crate::serial_vendor::VendorTwin>)` | A virtual radio of a vendor serial family (`VendorTwin`). |
| `HpsdrBridge` | `(BridgeStandIn)` | The stand-in of the OpenHPSDR `hpsdr_io` bridge's line protocol. |
| `RadiodBridge` | `(RadiodStandIn)` | The stand-in of the `radiod_io` bridge's line protocol. |
| `Usb` | `(Box<crate::usb_family::UsbFrameStandIn>)` | A virtual USB device as a byte-in, byte-out stand-in (`UsbFrameStandIn`). |

**Used by.** `for_description`, `replay_solicited` (functions whose signature names it).

## `BridgeStandIn` [@ANCHOR: radioctl:radioctl_conformance_bridge_stand_in]

**Purpose.** What the `hpsdr_io` bridge answers on its line protocol, with no radio behind it: frequency, PTT and attenuator are kept; forward power reads 0 and SWR (standing wave ratio) 1.0 (`10` tenths), as the bridge reports with no forward power. Transcripts for the ANAN files are of this protocol, not of the radio's UDP wire (chapter 7 of the manual).

**Definition.** `struct BridgeStandIn` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/conformance.rs`.

| Field | Type | Meaning |
|---|---|---|
| `freq` | `u64` | The frequency in hertz. |
| `ptt` | `bool` | Whether the bridge is keyed. |
| `att_db` | `u64` | The attenuator setting in decibels. |

## `RadiodStandIn` [@ANCHOR: radioctl:radioctl_conformance_radiod_stand_in]

**Purpose.** What the `radiod_io` bridge answers on its line protocol, with no `radiod` behind it: frequency, mode and passband width are kept (a mode change brings back that mode's own width, as the bridge does); the signal reads a fixed noise level, `S 729` (-127.1 dB re full scale, the RX-888's 2.95 kHz noise with no antenna). Transcripts for `radiod-channel` files are of this protocol, not of `radiod`'s multicast wire.

**Definition.** `struct RadiodStandIn` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/conformance.rs`.

| Field | Type | Meaning |
|---|---|---|
| `freq` | `u64` | The frequency in hertz. |
| `mode` | `&'static str` | The mode name as the bridge spells it. |
| `width` | `u64` | The passband width in hertz (a mode change brings back that mode's own width). |

## `Order` [@ANCHOR: radioctl:radioctl_virtual_flex_order]

**Purpose.** Whether a status line follows or precedes the reply that caused it.

**Definition.** `pub enum Order` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/virtual_flex.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `ReplyFirst` | - | `R` first, then `S`. |
| `StatusFirst` | - | `S` first, then `R`. |

## `FlexPanel` [@ANCHOR: radioctl:radioctl_virtual_flex_flex_panel]

**Purpose.** A change made at the radio itself (the GUI or the front panel), not by this client.

**Definition.** `pub enum FlexPanel` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/virtual_flex.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Tune` | `(Hz)` | The slice was tuned. |
| `SetMode` | `(Mode)` | The slice changed mode. |
| `Mox` | `(bool)` | The operator pressed MOX. |

**Used by.** `front_panel` (functions whose signature names it).

## `FlexRigState` [@ANCHOR: radioctl:radioctl_virtual_flex_flex_rig_state]

**Purpose.** What the virtual radio holds.

**Definition.** `pub struct FlexRigState` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/virtual_flex.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `frequency` | `Hz` | Frequency of slice 0. |
| `mode` | `Mode` | Mode of slice 0. |
| `filter_lo_hz` | `i64` | Low filter edge in hertz. |
| `filter_hi_hz` | `i64` | High filter edge in hertz. |
| `audio_level` | `u16` | Slice audio level. |
| `mic_level` | `u16` | Microphone level. |
| `rf_power` | `u16` | Transmit power setting. |
| `keyed` | `bool` | True while "transmitting". |
| `cw_sent` | `Vec<String>` | Texts of `cwx send` commands, spaces restored. |
| `cw_stops` | `u32` | How many `cwx clear` commands arrived (each empties `cw_sent`). |
| `pan_center` | `Hz` | Panadapter center. |
| `pan_bandwidth` | `Hz` | Panadapter bandwidth. |

**Used by.** `state` (functions whose signature names it).

## `VirtualFlex` [@ANCHOR: radioctl:radioctl_virtual_flex_virtual_flex]

**Purpose.** The radio.

**Definition.** `pub struct VirtualFlex` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/virtual_flex.rs`.

| Field | Type | Meaning |
|---|---|---|
| `handle` | `u32` | The client handle the radio announces in its `H` line. |
| `model` | `String` | The model name the radio reports (for example `FLEX-6300`). |
| `scus` | `u8` | How many spectrum capture units it reports. |
| `slice` | `u8` | The one slice receiver the twin models (0 is slice A). |
| `state` | `FlexRigState` | What the virtual radio holds (`FlexRigState`). |
| `buffer` | `Vec<u8>` | Bytes of the command line being received. |
| `discarding` | `bool` | True while the rest of an over-long line is being thrown away until its end. |
| `received` | `Vec<String>` | Every command line received, in order; the transmit-gate tests use it to prove that no transmit command arrived. |
| `sub_slice` | `bool` | Whether the client has subscribed to slice status. |
| `sub_tx` | `bool` | Whether the client has subscribed to transmit status. |
| `sub_pan` | `bool` | Whether the client has subscribed to panadapter status. |
| `order` | `Order` | Whether status precedes or follows its reply. |

**Used by.** `from_description`, `radio`, `talk` (functions whose signature names it).

## `RigState` [@ANCHOR: radioctl:radioctl_virtual_rig_rig_state]

**Purpose.** The mutable state of a virtual rig.

**Definition.** `pub struct RigState` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/virtual_rig.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `freq_a` | `Hz` | VFO A. |
| `freq_b` | `Hz` | VFO B, modelled separately from A. |
| `mode` | `Mode` | The mode. |
| `ptt` | `bool` | True while keyed. |
| `signal` | `u16` | S-meter, raw units. |
| `cw_sent` | `Vec<String>` | Every Morse text received. |
| `cw_stops` | `u32` | How many times a stop-Morse command cleared the buffer (`cw_sent` is emptied too). |
| `transceive` | `bool` | True when the rig reports front-panel changes on its own (CI-V transceive, Kenwood AI). |
| `split` | `bool` | Split operation (Elecraft). |
| `rit` | `bool` | RIT (receiver incremental tuning) on (Elecraft). |
| `xit` | `bool` | XIT (transmitter incremental tuning) on (Elecraft). |
| `rit_offset` | `i16` | RIT/XIT offset in hertz (Elecraft). |
| `filter_width` | `Hz` | Receive filter bandwidth (Elecraft). |
| `levels` | `BTreeMap<Level, u16>` | Numeric levels (Elecraft). |
| `swr` | `u16` | SWR in tenths (Elecraft). |
| `auto_info` | `u8` | Auto-info mode 0-3 (Elecraft `AI`); 0 sends nothing on its own. |
| `k2_mode` | `u8` | K2 command mode 0-3 as set by the controller (Elecraft `K2`); stored, the extended formats are not emulated. |
| `k3_mode` | `u8` | K3 command mode 0-1 as set by the controller (Elecraft `K3`); stored, not emulated. |

**Used by.** `state` (functions whose signature names it).

## `Wire` [@ANCHOR: radioctl:radioctl_virtual_rig_wire]

**Purpose.** The wire family a virtual rig speaks and the parsing state that family needs between calls.

**Definition.** `enum Wire` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/virtual_rig.rs`; derives `Debug`.

| Variant | Type | Meaning |
|---|---|---|
| `Civ` | - | Icom CI-V: the model's parameters and a frame parser. |
| `Kenwood` | - | Kenwood: the parameters and a `;` tokenizer. |
| `Elecraft` | - | Elecraft: the parameters and a `;` tokenizer. |
| `Yaesu` | - | Yaesu ASCII: the parameters and a `;` tokenizer. |
| `KenwoodHh` | `{ sim: crate::kenwood_hh::HhSim }` | Kenwood handheld/mobile CR-ended lines, kept by the simulator in `kenwood_hh`. |
| `YaesuLegacy` | - | Five-byte blocks; `pending` holds the bytes of a block still arriving. |
| `Receiver` | `(Box<VirtualReceiver>)` | A receiver or scanner with a line protocol; see `receiver`. |

## `VirtualRig` [@ANCHOR: radioctl:radioctl_virtual_rig_virtual_rig]

**Purpose.** One virtual radio.

**Definition.** `pub struct VirtualRig` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/virtual_rig.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description the virtual rig was built from. |
| `state` | `RigState` | The mutable state of the rig (`RigState`). |
| `wire` | `Wire` | The wire family it speaks and its parsing state (`Wire`). |
| `scope` | `Option<ScopeSim>` | The spectrum-scope simulator for a model that has one, `None` otherwise. |
| `civ_b_selected` | `bool` | CI-V: true while VFO B is the selected VFO (command 07 01); `03`, `05` and `25 00` act on the selected VFO and `25 01` on the other. |
| `civ_sub_selected` | `bool` | Whether the Sub band is the selected one (`07 D1`) and whether dual watch is on (`07 C1`), on a model with `civ.main_sub`. |
| `civ_dual_watch` | `bool` | Whether dual watch is on (`07 C1`) on a model with `civ.main_sub`. |
| `kenwood_rx_vfo` | `u8` | Kenwood: the VFO `FR` selected for receive (0 A, 1 B) and `FT` for transmit; split is when they differ. |
| `yaesu_tx_band` | `u8` | The band the transmitter follows on a Yaesu with `yaesu.tx_band`: 0 Main, 1 Sub. |
| `kenwood_tx_vfo` | `u8` | The VFO `FT` selected for transmit (0 A, 1 B); split is when it differs from `kenwood_rx_vfo`. |
| `ts890_scope` | `virtual_ts890::Ts890Scope` | TS-890S: the bandscope settings and the sweep counter. |

**Used by.** `civ_ask`, `civ_rig`, `from_description`, `kenwood_rig`, `kx3_rig`, `shipped_rig` (functions whose signature names it).

## `PanelChange` [@ANCHOR: radioctl:radioctl_virtual_rig_panel_change]

**Purpose.** A change made at the radio itself rather than over the wire.

**Definition.** `pub enum PanelChange` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/virtual_rig.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Frequency` | `(Hz)` | The tuning knob moved VFO A. |
| `Mode` | `(Mode)` | A mode button was pressed. |
| `Ptt` | `(bool)` | The microphone PTT was pressed or released. |
| `Signal` | `(u16)` | The received signal strength changed. |
| `Split` | `(bool)` | Split was switched on or off. |
| `RitOffset` | `(i16)` | The RIT/XIT offset was changed. |

**Used by.** `elecraft_panel`, `front_panel` (functions whose signature names it).

## `CivReply` [@ANCHOR: radioctl:radioctl_virtual_rig_civ_reply]

**Purpose.** What a virtual rig's CI-V handler answers to one command before it is framed: the OK or NG frame, or a data frame.

**Definition.** `enum CivReply` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/virtual_rig.rs`.

| Variant | Type | Meaning |
|---|---|---|
| `Ok` | - | The command was accepted; the OK frame (`FB`) is sent. |
| `Ng` | - | The command was refused; the NG frame (`FA`) is sent. |
| `Data` | `(u8, Vec<u8>)` | A data reply: the command byte and the payload bytes. |

**Used by.** `civ_dispatch`, `civ_filter_get`, `civ_filter_set`, `civ_front_end_get`, `civ_front_end_set`, `civ_level_get`, `civ_level_set`, `civ_mode_reply` (functions whose signature names it).

## `Ts890Scope` [@ANCHOR: radioctl:radioctl_virtual_ts890_ts890_scope]

**Purpose.** The bandscope settings of a virtual TS-890S (every `BS` command the guide has up to `BSC`, kept as the digits the guide defines) and the sweep counter.

**Definition.** `pub(super) struct Ts890Scope` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/virtual_ts890.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `display` | `u8` | `BS0`: scope display on or off. |
| `kind` | `u8` | `BS1`: the scope layout. |
| `mode` | `u8` | `BS3`: center or fixed mode. |
| `span` | `u8` | `BS4`: the span choice (an index into `SPANS_HZ`; a set of 9 is the "initial value"). |
| `range` | `u8` | `BS5`: the fixed-mode range number. |
| `pause` | `u8` | `BS6`: pause on or off. |
| `marker` | `u8` | `BS7`: marker on or off. |
| `attenuator` | `u8` | `BS8`: the scope attenuator setting. |
| `max_hold` | `u8` | `BS9`: max hold on or off. |
| `averaging` | `u8` | `BSA`: the averaging setting. |
| `waterfall_speed` | `u8` | `BSB`: the waterfall speed. |
| `audio_attenuator` | `u8` | `BSG`: the audio-scope attenuator setting. |
| `audio_span` | `u8` | `BSH`: the audio-scope span. |
| `oscilloscope_level` | `u8` | `BSI`: the oscilloscope level. |
| `oscilloscope_sweep` | `u8` | `BSJ`: the oscilloscope sweep. |
| `shift_position` | `u8` | `BSK`: the shift position. |
| `audio_pause` | `u8` | `BSN`: audio-scope pause. |
| `expand` | `u8` | `BSO`: expanded analysis range on or off. |
| `centred_edges` | `Option<(u32, u32)>` | Edges set by the marker-center command in fixed mode, until the mode or range changes. |
| `reference` | `u16` | `BSC`: the reference level, three digits (initially 40). |
| `output` | `u8` | `DD0`: the scope output selection. |
| `filter_output` | `u8` | `DD1`, the filter-scope output: 0 off, 1 LAN, 2 COM/USB. |
| `sweep` | `u32` | The sweep counter; the synthetic sweeps vary with it. |
| `edges` | `[(u32, u32); 4]` | The lower and upper edge of each fixed-mode range in hertz; entry 0 is the default range. |

**Used by.** `default` (functions whose signature names it).


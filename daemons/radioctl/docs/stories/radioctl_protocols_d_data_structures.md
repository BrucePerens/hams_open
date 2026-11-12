# Radioctl protocols data structures, part 4 (perseus to skanti)

As a developer changing this code, I want every data structure in `src/perseus.rs`, `src/perseus_engine.rs`, `src/prm80.rs`, `src/racal.rs`, `src/radiod_io.rs`, `src/radiod_net.rs`, `src/receiver.rs`, `src/rft.rs`, `src/rs.rs`, `src/rs_hf.rs`, `src/rshfiq.rs`, `src/serial_vendor.rs`, `src/si570_usb.rs` and `src/skanti.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `PerseusFiles` [@ANCHOR: radioctl:radioctl_perseus_perseus_files]

**Purpose.** Where the loader looks for the user-supplied firmware.

**Definition.** `pub struct PerseusFiles` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/perseus.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `fx2_hex` | `PathBuf` | The FX2 firmware, Intel HEX. |
| `fpga_image` | `PathBuf` | The FPGA bitstream, raw. |

**Used by.** `files`, `in_dir`, `new`, `open_ready` (functions whose signature names it).

## `HexRecord` [@ANCHOR: radioctl:radioctl_perseus_hex_record]

**Purpose.** One data record of an Intel HEX file.

**Definition.** `pub struct HexRecord` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/perseus.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `address` | `u16` | Load address in the FX2 RAM. |
| `data` | `Vec<u8>` | The bytes. |

**Used by.** `load_fx2`, `parse_intel_hex` (functions whose signature names it).

## `ProductId` [@ANCHOR: radioctl:radioctl_perseus_product_id]

**Purpose.** The product record the board keeps in its EEPROM.

**Definition.** `pub struct ProductId` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/perseus.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `serial` | `u16` | Serial number. |
| `product_code` | `u16` | Microtelecom product code (production units: [`PRODUCT_CODE`]). |
| `hardware_release` | `u8` | Hardware release. |
| `hardware_version` | `u8` | Hardware version. |

**Used by.** `product`, `read_product_id` (functions whose signature names it).

## `PerseusControl` [@ANCHOR: radioctl:radioctl_perseus_perseus_control]

**Purpose.** A board with firmware running and the FPGA configured, ready for control.

**Definition.** `pub struct PerseusControl` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/perseus.rs`.

| Field | Type | Meaning |
|---|---|---|
| `dev` | `Box<dyn UsbDevice>` | The USB device (real or virtual) the control requests go through. |
| `product` | `ProductId` | The product record read from the board's EEPROM (`ProductId`). |
| `frontend` | `u8` | The front-end bits (attenuator) last written through Port E; the low four bits of each Port E write. |
| `sio_control` | `u8` | The serial-IO control byte last written (ADC dither and preamplifier bits among them). |
| `sio_word` | `u32` | The tuning word last written to the serial-IO chip. |
| `preselector` | `Option<u8>` | The preselector filter last selected, `None` before the first selection. |

**Used by.** `from_device`, `new`, `open_ready` (functions whose signature names it).

## `PerseusSession` [@ANCHOR: radioctl:radioctl_perseus_perseus_session]

**Purpose.** A running board as a [`UsbDevice`] that takes the session requests above. Dropping it sends the board's shutdown command (FPGA reset, preselector off, 30 dB attenuation), the safe state.

**Definition.** `pub struct PerseusSession` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/perseus.rs`.

| Field | Type | Meaning |
|---|---|---|
| `control` | `PerseusControl` | The board control (`PerseusControl`) the session drives. |
| `frequency_hz` | `Option<f64>` | The frequency last tuned in hertz, `None` before the first tune. |
| `preselector` | `bool` | Whether the preselector is enabled. |
| `dither` | `bool` | Whether ADC dither is on. |
| `preamp` | `bool` | Whether the ADC preamplifier ("gain high") is on. |

**Used by.** `new` (functions whose signature names it).

## `PerseusLoaderBackend` [@ANCHOR: radioctl:radioctl_perseus_perseus_loader_backend]

**Purpose.** Opens a real (or virtual) board through the inner backend, loading the user's firmware as needed, and returns a [`PerseusSession`]. The USB link calls it on every (re)open.

**Definition.** `pub struct PerseusLoaderBackend` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/perseus.rs`.

| Field | Type | Meaning |
|---|---|---|
| `inner` | `Arc<dyn UsbBackend>` | The underlying USB backend that opens the board (a real bus or a virtual one). |
| `files` | `PerseusFiles` | The user-supplied firmware and FPGA image paths (`PerseusFiles`). |
| `real_time` | `bool` | Whether the loader waits real time for the board to re-enumerate after the firmware loads (false in tests). |

**Used by.** `new` (functions whose signature names it).

## `ReadyVirtualPerseus` [@ANCHOR: radioctl:radioctl_perseus_ready_virtual_perseus]

**Purpose.** A virtual Perseus whose firmware is already running and FPGA configured: opens straight into a [`PerseusSession`], needing no files. Clone it: all clones are the same board.

**Definition.** `pub struct ReadyVirtualPerseus` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/perseus.rs`; derives `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `board` | `VirtualPerseusBackend` | The virtual board behind it (`VirtualPerseusBackend`); clones share the same board. |

**Used by.** `new` (functions whose signature names it).

## `VirtualPerseusState` [@ANCHOR: radioctl:radioctl_perseus_virtual_perseus_state]

**Purpose.** What the virtual board has been told, for tests.

**Definition.** `pub struct VirtualPerseusState` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/perseus.rs`; derives `Debug`, `Clone`, `Default`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `firmware_running` | `bool` | True once the FX2 CPU was held in reset and released after RAM writes. |
| `ram_writes` | `Vec<(u16, Vec<u8>)>` | `(address, data)` of every FX2 RAM write. |
| `fpga` | `Vec<u8>` | The FPGA bitstream received since the last reset. |
| `port_e` | `u8` | The last Port E byte. |
| `sio` | `(u8, u32)` | The last serial-IO control byte and tuning word. |
| `shut_down` | `bool` | True after the shutdown command. |
| `commands` | `Vec<Vec<u8>>` | Every bulk command frame received on the command endpoint, in order. |

**Used by.** `state` (functions whose signature names it).

## `VirtualInner` [@ANCHOR: radioctl:radioctl_perseus_virtual_inner]

**Purpose.** The shared state of one virtual Perseus board: what it has been told (`VirtualPerseusState`) plus its internal CPU-reset flag, the queued status replies and the product record. Private; every access is through the board's mutex.

**Definition.** `struct VirtualInner` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/perseus.rs`.

| Field | Type | Meaning |
|---|---|---|
| `state` | `VirtualPerseusState` | What the board has been told, for tests (`VirtualPerseusState`). |
| `cpu_held` | `bool` | True while the FX2 CPU is held in reset; releasing it after RAM writes marks the firmware as running. |
| `status` | `Vec<Vec<u8>>` | Status replies queued for the controller to read. |
| `done` | `bool` | The FPGA configuration-done flag; the board reports it, together with a non-empty image, as the answer to the FPGA check command. |
| `product` | `[u8; PRODUCT_ID_LEN]` | The 8-byte product record the board returns. |

**Used by.** `lock` (functions whose signature names it).

## `VirtualPerseusBackend` [@ANCHOR: radioctl:radioctl_perseus_virtual_perseus_backend]

**Purpose.** A virtual Perseus on a virtual USB bus: it appears as the blank board until the FX2 firmware has been loaded, then as the running board. Clone it: all clones are the same board.

**Definition.** `pub struct VirtualPerseusBackend` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/perseus.rs`; derives `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `inner` | `Arc<Mutex<VirtualInner>>` | The shared state of the virtual board (`Arc<Mutex<VirtualInner>>`); clones are the same board. |

**Used by.** `board`, `new` (functions whose signature names it).

## `VirtualPerseusHandle` [@ANCHOR: radioctl:radioctl_perseus_virtual_perseus_handle]

**Purpose.** One open handle on the virtual Perseus board, implementing the USB device interface; every handle shares the board's state. Private to `perseus.rs`.

**Definition.** `struct VirtualPerseusHandle` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/perseus.rs`.

| Field | Type | Meaning |
|---|---|---|
| `inner` | `Arc<Mutex<VirtualInner>>` | The shared board state (`Arc<Mutex<VirtualInner>>`). |

## `PerseusEngine` [@ANCHOR: radioctl:radioctl_perseus_engine_perseus_engine]

**Purpose.** The Perseus client engine.

**Definition.** `pub struct PerseusEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/perseus_engine.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `waiting` | `bool` | True after a request that expects an answer was sent, until it is read. |
| `rx` | `Vec<u8>` | Bytes received from the device so far and not yet consumed. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `State` [@ANCHOR: radioctl:radioctl_prm80_state]

**Purpose.** The decoded state line.

**Definition.** `pub struct State` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/prm80.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `ptt` | `bool` | Transmitting. |
| `rx` | `Hz` | Receive frequency. |
| `tx` | `Hz` | Transmit frequency. |
| `rssi` | `u8` | Raw signal strength byte. |

**Used by.** `decode_state` (functions whose signature names it).

## `Prm80Engine` [@ANCHOR: radioctl:radioctl_prm80_prm80_engine]

**Purpose.** The PRM8060 client engine.

**Definition.** `pub struct Prm80Engine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/prm80.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `pending` | `Option<Field>` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `Prm80Twin` [@ANCHOR: radioctl:radioctl_prm80_prm80_twin]

**Purpose.** A virtual PRM8060.

**Definition.** `pub struct Prm80Twin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/prm80.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `rx_hz` | `Hz` | Receive frequency in hertz. |
| `tx_hz` | `Hz` | Transmit frequency in hertz. |
| `ptt` | `bool` | True while the twin is transmitting (keyed by the controller). |
| `rssi` | `u8` | The raw signal strength byte the twin reports. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Dialect` [@ANCHOR: radioctl:radioctl_racal_dialect]

**Purpose.** The Racal-family command dialects.

**Definition.** `pub enum Dialect` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/racal.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Ra6790` | - | Racal RA6790/GM. |
| `Rx340` | - | Ten-Tec RX-340. |
| `Rx331` | - | Ten-Tec RX-331. |
| `Ra3702` | - | Racal RA3702: packet frames (LF, command, CR), frequency in hertz. |

**Used by.** `from_name` (functions whose signature names it).

## `Pending` [@ANCHOR: radioctl:radioctl_racal_pending]

**Purpose.** What the engine is waiting for after the last request it sent, so the next reply is read as its answer.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/racal.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Frequency` | - | A frequency read or write is outstanding; its answer is the frequency. |
| `Mode` | - | A mode read or write is outstanding; its answer is the mode. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `RacalEngine` [@ANCHOR: radioctl:radioctl_racal_racal_engine]

**Purpose.** The Racal client engine.

**Definition.** `pub struct RacalEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/racal.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `dialect` | `Dialect` | Which command set of the family the rig speaks. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `RacalTwin` [@ANCHOR: radioctl:radioctl_racal_racal_twin]

**Purpose.** A virtual Racal RA6790.

**Definition.** `pub struct RacalTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/racal.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `dialect` | `Dialect` | The command set the twin emulates. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `hz` | `Hz` | Current frequency in hertz. |
| `mode` | `u8` | Current mode, as the code the description's `[[modes]]` table uses. |
| `codes` | `Vec<u8>` | The mode codes the description defines; the twin accepts a mode setting only if its code is in this list. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `RadiodLink` [@ANCHOR: radioctl:radioctl_radiod_io_radiod_link]

**Purpose.** Where the channel lives and how it is kept. Build with [`RadiodLink::new`] and adjust fields.

**Definition.** `pub struct RadiodLink` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/radiod_io.rs`; derives `Debug`, `Clone`, `PartialEq`.

| Field | Type | Meaning |
|---|---|---|
| `status_group` | `Ipv4Addr` | `radiod`'s status (control) multicast group. |
| `status_port` | `u16` | Its port (5006 by default). |
| `data_group` | `Ipv4Addr` | The data (RTP) multicast group the channel's audio goes to (`radiod`'s `data =`). |
| `data_port` | `u16` | Its port (5004 by default). |
| `iface` | `Ipv4Addr` | Interface address to join on (`127.0.0.1` for a loopback `radiod`). |
| `ssrc` | `u32` | The channel's SSRC, chosen by the configuration; 1 to 0xfffffffe, and not used by any other client of the same `radiod` (two clients on one SSRC share one channel). |
| `lifetime_frames` | `u32` | Lifetime given with every command, 20 ms frames (default 500, 10 s). |
| `refresh` | `Duration` | How often the channel's state is re-asserted (default 2 s; must be well inside the lifetime). |
| `ack_wait` | `Duration` | How long one command waits for `radiod`'s acknowledgement (default 1 s). |
| `policy` | `Policy` | Site limits on frequency and lifetime ([`Policy`]); frequencies outside are refused. |
| `frequency_hz` | `u64` | Starting dial frequency, Hz. |
| `mode` | `Mode` | Starting mode (USB, LSB, CW, AM, FM, SAM, SAM-L or SAM-U). |
| `audio_queue` | `usize` | Audio frames queued for [`RadiodControl::take_audio`] before new ones are dropped (default 50, one second). |

**Used by.** `new`, `serve_radiod` (functions whose signature names it).

## `AudioFrame` [@ANCHOR: radioctl:radioctl_radiod_io_audio_frame]

**Purpose.** One decoded RTP packet of the channel's audio.

**Definition.** `pub struct AudioFrame` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/radiod_io.rs`; derives `Debug`, `Clone`, `PartialEq`.

| Field | Type | Meaning |
|---|---|---|
| `samples` | `Vec<f32>` | Mono samples in -1.0..=1.0 (20 ms: 240 at 12 kHz, 480 at 24 kHz). |
| `rate_hz` | `u32` | Sample rate, Hz, from the channel's status. |
| `seq` | `u16` | RTP sequence number. |
| `timestamp` | `u32` | RTP timestamp, in samples. |

**Used by.** `run_audio`, `take_audio` (functions whose signature names it).

## `RadiodStats` [@ANCHOR: radioctl:radioctl_radiod_io_radiod_stats]

**Purpose.** Counters and the latest statuses, for diagnostics and the front-end overrange display.

**Definition.** `pub struct RadiodStats` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/radiod_io.rs`; derives `Debug`, `Clone`, `Default`, `PartialEq`.

| Field | Type | Meaning |
|---|---|---|
| `acks` | `u64` | Commands `radiod` acknowledged. |
| `ack_timeouts` | `u64` | Commands that got no acknowledgement within `ack_wait`. |
| `refused` | `u64` | Commands refused by the client's schema before sending (out-of-policy values). |
| `audio_frames` | `u64` | Audio frames queued. |
| `audio_dropped` | `u64` | Audio frames dropped because the queue was full or nobody had taken it. |
| `rtp_lost` | `u64` | RTP packets missing in sequence. |
| `rtp_out_of_order` | `u64` | RTP packets that arrived late (within the reorder window) or duplicated. |
| `rtp_restarts` | `u64` | Times the channel's RTP sequence started again (a backward jump beyond the reorder window): `radiod` re-created the channel. Not counted in `rtp_lost`. |
| `rtp_stalls` | `u64` | Times this channel's audio stopped arriving for at least `STALL_GAP` (250 ms; packets come every 20 ms). A squelch-closed channel sends nothing, so its closures are counted too. |
| `rtp_stall_ms` | `u64` | Total length of those stalls, ms. |
| `rtp_bad` | `u64` | RTP packets of this SSRC that did not decode (wrong payload type or size). |
| `channel` | `Option<ChannelStatus>` | The channel's latest status. |
| `front_end` | `Option<ChannelStatus>` | The front end's latest status (SSRC 0). |

**Used by.** `stats` (functions whose signature names it).

## `Format` [@ANCHOR: radioctl:radioctl_radiod_io_format]

**Purpose.** What the stream format of the channel is, as its status announced.

**Definition.** `struct Format` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/radiod_io.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `pt` | `u8` | The RTP payload type of the channel's audio. |
| `encoding` | `RtpEncoding` | The sample encoding (`RtpEncoding`). |
| `channels` | `usize` | Number of audio channels in the stream. |
| `rate_hz` | `u32` | The sample rate in hertz. |

**Used by.** `format_of` (functions whose signature names it).

## `Shared` [@ANCHOR: radioctl:radioctl_radiod_io_shared]

**Purpose.** State shared by the radiod bridge task and its control handle: the counters and latest statuses, the last mode set, the stream format and the audio queue.

**Definition.** `struct Shared` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/radiod_io.rs`.

| Field | Type | Meaning |
|---|---|---|
| `stats` | `Mutex<RadiodStats>` | Counters and the latest statuses (`RadiodStats`). |
| `mode` | `Mutex<Mode>` | The mode the engine last set successfully (the bridge's `State::mode`), for `carrier_lock`. |
| `format` | `Mutex<Option<Format>>` | The channel's audio stream format once announced (`Format`), `None` before. |
| `audio` | `Mutex<Option<mpsc::Receiver<AudioFrame>>>` | The receiving end of the audio queue until a consumer takes it with `take_audio`. |

**Used by.** `run_audio` (functions whose signature names it).

## `RadiodControl` [@ANCHOR: radioctl:radioctl_radiod_io_radiod_control]

**Purpose.** Handle on a served bridge: the audio queue, counters and the latest statuses.

**Definition.** `pub struct RadiodControl` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/radiod_io.rs`; derives `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `shared` | `Arc<Shared>` | The bridge's shared state (`Arc<Shared>`); the handle is `Clone`. |

**Used by.** `serve_radiod` (functions whose signature names it).

## `State` [@ANCHOR: radioctl:radioctl_radiod_io_state]

**Purpose.** The channel as the engine last asked for it.

**Definition.** `struct State` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/radiod_io.rs`.

| Field | Type | Meaning |
|---|---|---|
| `ssrc` | `Ssrc` | The channel's SSRC (`Ssrc`). |
| `freq` | `u64` | The frequency last asked for, in hertz. |
| `mode` | `Mode` | The mode last asked for. |
| `width` | `Option<u64>` | The filter width last asked for in hertz, `None` for the mode's default. |

## `Bridge` [@ANCHOR: radioctl:radioctl_radiod_io_bridge]

**Purpose.** The control side of the bridge: one task owns this.

**Definition.** `struct Bridge` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/radiod_io.rs`.

| Field | Type | Meaning |
|---|---|---|
| `control` | `Control` | The multicast control client that talks to `radiod` (`Control`). |
| `link` | `RadiodLink` | Where the channel lives and how it is kept (`RadiodLink`). |
| `state` | `State` | The channel as the engine last asked for it (`State`). |
| `shared` | `Arc<Shared>` | The state shared with the control handle (`Arc<Shared>`). |

**Used by.** `run_control` (functions whose signature names it).

## `RadiodEngine` [@ANCHOR: radioctl:radioctl_radiod_net_radiod_engine]

**Purpose.** The engine for one `radiod` channel.

**Definition.** `pub struct RadiodEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/radiod_net.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `line` | `Vec<u8>` | Bytes of the incoming line not terminated yet. |
| `discarding` | `bool` | True while the rest of an over-long line is being thrown away until its end. |

**Used by.** `engine`, `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `LineEngine` [@ANCHOR: radioctl:radioctl_receiver_line_engine]

**Purpose.** A protocol engine for a line-oriented receiver.

**Definition.** `pub struct LineEngine<D: Dialect>` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/receiver.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `dialect` | `D` | Which command set of the family the rig speaks. |
| `expecting` | `ReplyKind` | What kind of reply the last request needs (`ReplyKind`); the dialect decodes the next line accordingly. |
| `buffer` | `Vec<u8>` | Bytes received so far of the reply being assembled. |
| `last_byte_at` | `Duration` | Time (caller's monotonic clock) at which the last byte arrived, used to drop a stale partial frame. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `VirtualReceiver` [@ANCHOR: radioctl:radioctl_receiver_virtual_receiver]

**Purpose.** The line side of a virtual receiver: collects bytes into command lines for a twin.

**Definition.** `pub struct VirtualReceiver` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/receiver.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `twin` | `Box<dyn ReceiverTwin>` | The receiver twin (`ReceiverTwin`) that answers the command lines. |
| `buffer` | `Vec<u8>` | Bytes of the command line being received. |
| `last_byte_at` | `Duration` | When the last byte arrived, used to drop a stale partial line. |

**Used by.** `new` (functions whose signature names it).

## `RftEngine` [@ANCHOR: radioctl:radioctl_rft_rft_engine]

**Purpose.** The EKD-500 client engine.

**Definition.** `pub struct RftEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/rft.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `RftTwin` [@ANCHOR: radioctl:radioctl_rft_rft_twin]

**Purpose.** A virtual RFT EKD-500. It accepts the frequency command and says nothing.

**Definition.** `pub struct RftTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/rft.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `hz` | `Hz` | Current frequency in hertz. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `RsEngine` [@ANCHOR: radioctl:radioctl_rs_rs_engine]

**Purpose.** The Rohde and Schwarz engine.

**Definition.** `pub type RsEngine = LineEngine<RsDialect>;` (type) in `daemons/radioctl/crates/radioctl-protocols/src/rs.rs`.

**Used by.** `engine` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `RsDialect` [@ANCHOR: radioctl:radioctl_rs_rs_dialect]

**Purpose.** The `FREQ` / `DEM` command set.

**Definition.** `pub struct RsDialect;` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/rs.rs`; derives `Debug`, `Clone`, `Copy`.

**Used by.** `from_description` (functions whose signature names it).

**Notes.** A unit struct with no state: the dialect implementation of the `Dialect` trait for the Rohde and Schwarz `FREQ` and `DEM` commands. `LineEngine<RsDialect>` (`RsEngine`) supplies the buffering and the reply handling, and `from_description` builds the dialect from a rig description of this family.

## `RsTwin` [@ANCHOR: radioctl:radioctl_rs_rs_twin]

**Purpose.** The virtual Rohde and Schwarz receiver.

**Definition.** `pub struct RsTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/rs.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description the twin takes its defaults and tables from. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `RsHfEngine` [@ANCHOR: radioctl:radioctl_rs_hf_rs_hf_engine]

**Purpose.** The engine of the Rohde and Schwarz HF dialects.

**Definition.** `pub type RsHfEngine = LineEngine<RsHfDialect>;` (type) in `daemons/radioctl/crates/radioctl-protocols/src/rs_hf.rs`.

**Used by.** `engine` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `Style` [@ANCHOR: radioctl:radioctl_rs_hf_style]

**Purpose.** Which command set a model speaks.

**Definition.** `enum Style` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/rs_hf.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Ek` | - | EK 895 and EK 896: `F<hz>`, `I<n>`, `L?`. |
| `Gp2000` | - | XK 2100: `F<hz>,<hz>`, `I<n>`. |
| `Xk852` | - | XK 852: `*F`, `*I`, `*O1`. |

**Used by.** `style_of` (functions whose signature names it).

## `RsHfDialect` [@ANCHOR: radioctl:radioctl_rs_hf_rs_hf_dialect]

**Purpose.** The command set of one Rohde and Schwarz HF model.

**Definition.** `pub struct RsHfDialect` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/rs_hf.rs`; derives `Debug`, `Clone`, `Copy`.

| Field | Type | Meaning |
|---|---|---|
| `style` | `Style` | Which command set the model speaks (`Style`). |

**Used by.** `from_description` (functions whose signature names it).

## `RsHfTwin` [@ANCHOR: radioctl:radioctl_rs_hf_rs_hf_twin]

**Purpose.** The virtual Rohde and Schwarz HF receiver.

**Definition.** `pub struct RsHfTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/rs_hf.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `style` | `Style` | Which command set the twin emulates. |
| `desc` | `RigDescription` | The rig description the twin takes its defaults and tables from. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `Pending` [@ANCHOR: radioctl:radioctl_rshfiq_pending]

**Purpose.** What the engine is waiting for after the last request it sent, so the next reply is read as its answer.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/rshfiq.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Frequency` | - | A frequency read or write is outstanding; its answer is the frequency. |
| `Power` | - | A transmit power read in percent is outstanding. |

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `RshfiqEngine` [@ANCHOR: radioctl:radioctl_rshfiq_rshfiq_engine]

**Purpose.** The RS-HFIQ client engine.

**Definition.** `pub struct RshfiqEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/rshfiq.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `RshfiqTwin` [@ANCHOR: radioctl:radioctl_rshfiq_rshfiq_twin]

**Purpose.** A virtual RS-HFIQ.

**Definition.** `pub struct RshfiqTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/rshfiq.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `RxBuffer` | Receive buffer (`RxBuffer`) holding the controller's bytes until a complete command has arrived. |
| `hz` | `Hz` | Current frequency in hertz. |
| `ptt` | `bool` | True while the twin is transmitting (keyed by the controller). |
| `percent` | `u16` | The transmit power in percent of nominal as last set. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `TwinSnapshot` [@ANCHOR: radioctl:radioctl_serial_vendor_twin_snapshot]

**Purpose.** What a vendor twin holds, for harnesses that compare it with another implementation of the same protocol (the Hamlib oracle batch).

**Definition.** `pub struct TwinSnapshot` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/serial_vendor.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `frequency` | `Hz` | The frequency of the receive VFO. |
| `mode_code` | `Option<u8>` | The mode code of the receive VFO as the description's `[[modes]]` table spells it, or `None` for a twin that keeps no mode. |
| `ptt` | `bool` | Whether the twin is transmitting. |

**Used by.** `snapshot` (functions whose signature names it).

## `RxBuffer` [@ANCHOR: radioctl:radioctl_serial_vendor_rx_buffer]

**Purpose.** Receive buffer that drops a partial frame older than [`PARTIAL_FRAME_TIMEOUT`].

**Definition.** `pub struct RxBuffer` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/serial_vendor.rs`; derives `Debug`, `Default`.

| Field | Type | Meaning |
|---|---|---|
| `bytes` | `Vec<u8>` | The bytes received and not yet taken. |
| `last_at` | `Duration` | When the last bytes arrived, used to discard a stale partial frame. |

## `Pending` [@ANCHOR: radioctl:radioctl_si570_usb_pending]

**Purpose.** What the engine is waiting for after the last request it sent, so the next reply is read as its answer.

**Definition.** `enum Pending` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/si570_usb.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Nothing` | - | No request is outstanding; a line that arrives is unsolicited. |
| `Set` | - | A set request is outstanding. |
| `ReadFrequency` | - | A frequency read is outstanding. |
| `ReadRegisters` | - | A read of the Si570 registers is outstanding. |
| `Fifi` | `(FifiRead)` | A FiFi-SDR parameter read of this kind is outstanding. |

**Used by.** `fifi_read`, `fifi_write` (functions whose signature names it).

**Notes.** Private state of the engine that remembers what the last request was, so that the next reply line is interpreted as its answer.

## `FifiRead` [@ANCHOR: radioctl:radioctl_si570_usb_fifi_read]

**Purpose.** What a pending FiFi-SDR parameter read holds.

**Definition.** `enum FifiRead` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/si570_usb.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `VcoFactor` | - | The VCO factor (the board's multiplier). |
| `Mode` | - | The mode. |
| `Width` | - | The filter width. |
| `Volume` | - | The volume. |
| `Preamp` | - | The preamplifier state. |
| `Signal` | - | The signal strength. |

**Used by.** `fifi_answer`, `fifi_read` (functions whose signature names it).

## `Si570UsbEngine` [@ANCHOR: radioctl:radioctl_si570_usb_si570_usb_engine]

**Purpose.** The Si570 USB client engine.

**Definition.** `pub struct Si570UsbEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/si570_usb.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `protocol` | `UsbProtocol` | Which USB request set the board speaks (`UsbProtocol`). |
| `multiplier` | `u8` | The board's frequency multiplier (4 for a quadrature-detector board, 2 for the KTH); the FiFi-SDR replaces it with the VCO factor it reports. |
| `pending` | `Pending` | What the last request was, so the next reply is read as its answer; `Nothing` when no request is outstanding. |
| `rx` | `Vec<u8>` | Receive buffer (`RxBuffer`) holding the bytes of a reply that is not complete yet; a partial reply older than `PARTIAL_FRAME_TIMEOUT` is dropped. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `Si570Twin` [@ANCHOR: radioctl:radioctl_si570_usb_si570_twin]

**Purpose.** A virtual Si570 USB board: a [`UsbDevice`] that answers the requests above.

**Definition.** `pub struct Si570Twin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/si570_usb.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `protocol` | `UsbProtocol` | Which USB request set the twin emulates. |
| `word` | `u32` | The 32-bit frequency word as written, returned by a read. |
| `registers` | `[u8; 6]` | The six Si570 registers. |
| `ptt` | `bool` | True while the twin is transmitting (keyed by the controller). |
| `fifi` | `FifiState` | The FiFi-SDR parameters the twin holds. |

**Used by.** `new` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.

## `FifiState` [@ANCHOR: radioctl:radioctl_si570_usb_fifi_state]

**Purpose.** The FiFi-SDR parameters the twin holds.

**Definition.** `struct FifiState` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/si570_usb.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `vco_factor` | `u32` | The VCO factor (the multiplier). |
| `mode` | `u8` | The mode code. |
| `width` | `u32` | The filter width. |
| `volume` | `i16` | The volume. |
| `preamp` | `u8` | The preamplifier state. |
| `signal` | `u32` | The signal strength it reports. |

## `Dialect` [@ANCHOR: radioctl:radioctl_skanti_dialect]

**Purpose.** The two Skanti command sets.

**Definition.** `pub enum Dialect` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/skanti.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Trp8000` | - | TRP8000: CR-ended commands, `>` prompt. |
| `Trp8255` | - | TRP 8255 S R: byte-wise acknowledgements. |

**Used by.** `from_name` (functions whose signature names it).

## `SkantiEngine` [@ANCHOR: radioctl:radioctl_skanti_skanti_engine]

**Purpose.** The Skanti client engine.

**Definition.** `pub struct SkantiEngine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/skanti.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description (`RigDescription`) the engine was built from; returned by `description()`. |
| `dialect` | `Dialect` | Which command set of the family the rig speaks. |
| `owed` | `usize` | Acknowledgements (TRP 8255: bytes sent; TRP8000: prompts) still to come. |

**Used by.** `new`, `send` (functions whose signature names it).

**Notes.** Client-side protocol engine implementing `ProtocolEngine`: `encode` turns an abstract `Op` into the bytes of a `Request`, `feed` turns received bytes into `Inbound` events. Sans-IO: no I/O and no clock other than the `now` the caller passes. Owned and driven by one rig actor, so it needs no locking. Transmit-capable ops reach it only through the actor's transmit gate; the engine only encodes them.

## `SkantiTwin` [@ANCHOR: radioctl:radioctl_skanti_skanti_twin]

**Purpose.** A virtual Skanti transceiver.

**Definition.** `pub struct SkantiTwin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/skanti.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `dialect` | `Dialect` | The command set the twin emulates. |
| `codes` | `Vec<u8>` | The mode codes the description defines; the twin accepts a mode setting only if its code is in this list. |
| `line` | `Vec<u8>` | Bytes of the command line being received. |
| `rx_units` | `u64` | The receive frequency in the radio's units (100 Hz). |
| `tx_units` | `u64` | The transmit frequency in the radio's units (100 Hz). |
| `mode` | `u8` | Current mode, as the code the description's `[[modes]]` table uses. |
| `ptt` | `bool` | True while the twin is transmitting (keyed by the controller). |

**Used by.** `new`, `send` (functions whose signature names it).

**Notes.** Virtual rig (a twin) of this protocol, implementing `VendorTwin`: it consumes the controller's bytes and returns what the real radio would send, so the conformance suite and the virtual rig can exercise the engine with no hardware. Never touches a device or a transmitter.


# hpsdr

A Rust client for OpenHPSDR (the open High Performance Software Defined Radio protocol and hardware family) radios (ANAN-10, ANAN-100, Hermes, and the other boards of the family), built
for the relay SDR plan (`hams_com/docs/proposals/SDR_RECEIVE_ENGINE_AND_TRANSMIT_PATH.md` sections 2 to 4)
and the transmit safety baseline (`RELAY_SDR_RADIOCTL_ROADMAP.md` section 2a). Prototype: **everything here is
`verified=unverified`** until it has run against a real radio. No hardware was used to write it.

## Status

| Piece | State |
|---|---|
| `p1`: Protocol 1 discovery, start/stop, EP2 builder, EP6 decoder (USB endpoint 2 carries data to the radio and endpoint 6 carries samples from it), status C&C | written, unit-tested |
| `gate`: transmit gate (closed by default, bounded lease) | written, unit-tested |
| `iq`: `IqStream` trait, blocks with sequence numbers, drop counters | written, unit-tested |
| `types`: boards, sample rates, power/supply/SWR conversion | written, unit-tested |
| `discovery`: UDP broadcast or unicast discovery (Protocol 1) | written, tested against the emulator |
| `session`: configure, start, stream several DDC (digital down-converter) receivers, status, stop; gated PTT and transmit I/Q queue | written, tested against the emulator |
| `emulator`: loopback-only Protocol 1 server with synthetic tones, status, loss injection and transmit counters | written |
| `p2`: Protocol 2 discovery, General, DDC-specific, High Priority (receive and gated transmit), gated DUC (digital up-converter) I/Q, HP status and DDC packet parsers | written, unit-tested by round trip only |
| `session_p2`: Protocol 2 session (General, DDC-specific, High Priority; DDC streams demultiplexed by source port; gated PTT and DUC I/Q) | written, tested against the Protocol 2 emulator |
| `emulator_p2`: loopback-only Protocol 2 server with synthetic tones, attenuator, status, loss injection and transmit counters | written |
| Hardware status for the ANAN-100: temperature | not defined by the published specs |

## Using it

```rust,ignore
let handle = tokio::runtime::Handle::current();          // the caller's runtime; the crate never makes one
let radios = hpsdr::discovery::discover(&handle, &[hpsdr::discovery::broadcast_target()], Duration::from_secs(1)).await?;
let mut radio = hpsdr::session::Radio::start(&handle, &radios[0], P1Config::default(), TxGate::new(), Default::default()).await?;
let mut ddc0 = radio.take_streams().remove(0);           // impl IqStream
while let Some(block) = ddc0.next_block().await { /* block.samples, block.seq, block.lost_before */ }
```

The emulator (`emulator::Emulator::start_p1`) listens on `127.0.0.1` only and is what the tests use: it needs no
hardware and produces tones at absolute frequencies, so each receiver sees its own offset.

## Design

* **Sans-IO.** `p1` builds and parses datagrams; it opens no sockets. The library never creates a tokio
  runtime and never calls `block_on`: anything that opens a socket or spawns takes a `tokio::runtime::Handle`.
* **Transmit fails closed.** `TxGate::default()` is closed. `Ep2Builder::build_rx` has no MOX (manual operate transmit, the transmit-on switch), drive or
  transmit I/Q parameter at all; `Ep2Builder::build_tx` returns `HpsdrError::GateClosed` and builds nothing unless
  the caller opened the gate. A gate opens for a fixed lease and expires by itself; a poisoned lock reads as closed.
  Tests assert that every receive datagram has MOX clear, drive zero and zero transmit I/Q in every C&C address.
* **Receive stream interface.** `iq::IqStream` yields `IqBlock`s (receiver index, datagram sequence number,
  datagrams lost before it, sample rate, centre frequency, complex f32 samples) and `StreamStats` (blocks, samples,
  lost packets, blocks dropped because the consumer lagged, malformed datagrams). radioctl can wrap the control plane
  later; the relay depends on the trait only.
* **Not reported.** Temperature is not defined for ANAN/Hermes hardware in the published Protocol 1 (USB v1.60) or
  Protocol 2 (v4.4, Appendix A) documents, so `HardwareStatus` has no temperature field. Forward and reverse power,
  supply voltage, ADC overload and (Protocol 2) ADC peak magnitude are reported.

## Sources and provenance

Facts (byte layouts, field meanings, calibration constants) were read from, and cited by name in the doc comments:

* "Metis - How it works" v1.33 and "openHPSDR Ethernet Protocol" v4.4 (c) March 2019 Phil Harman VK6PH, from the
  `TAPR/OpenHPSDR-Firmware` repository (GPL-3.0).
* "HPSDR - USB Data Protocol" v1.60, `USB_protocol_V1.60.doc` in the `TAPR/OpenHPSDR-SVN` repository.
* The Hermes-Lite 2 wiki page "Protocol" was read only to cross-check how C0 splits into an address and the MOX bit;
  none of its Hermes-Lite-specific registers are used.

No terms in these documents forbid reuse of the facts they describe. No source code of pihpsdr, Thetis, PowerSDR or
any other OpenHPSDR program was read or copied. Nothing is copied from the documents' text beyond field names and numbers.

Known open points (all `verified=unverified`): Protocol 2 sends each DDC stream from its own source port (`ddc0 + n`); the specification text does not say whether the General packet's radio-to-host port fields are the radio's source ports or the host's destination ports, nor whether a real radio delivers every stream to the one host socket that sent the General packet. `session_p2` assumes the first and demultiplexes by source port; `Radio2::stray_datagrams` counts anything else, so a radio that behaves otherwise shows up as stray traffic, not as wrong data. The emulator does not model the one-second host watchdog (the session sends High Priority packets every tick regardless); the DDC packet carries no DDC number, so demultiplexing needs the port; whether a radio streams EP6 while receiving EP2 only at the pacing used
here; whether Rx8 is addressable in Protocol 1 (v1.60 lists NCO addresses for Rx1..Rx7 only); the I/Q swap on transmit
(USB v1.60 says the PC must swap them); the Protocol 2 text says "little Endian" for hardware to host in one place but
revision 1.3 fixed the order to big-endian, which is what is implemented.

Licence: AGPL-3.0-or-later. The toolchain is pinned in `rust-toolchain.toml` (as for `daemons/radioctl`).
Build and test: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`.

Anchors (ADR 0090): a function carries `// [@ANCHOR: id]` in source, its test carries
`// Tests [@ANCHOR: id]`, and this README cites it as `[@ANCHOR: hpsdr:id]` below.

### `broadcast_target` [@ANCHOR: hpsdr:hpsdr_discovery_broadcast_target]

Returns the address a discovery request is sent to when looking for any radio on the local network: the limited broadcast address `255.255.255.255` with UDP port 1024 (`p1::PORT`). Pass it in the `targets` list of `discover`. The Metis document notes the subnet broadcast address is not implemented in the firmware, so the limited broadcast is used for both protocols.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

**Returns:** a `SocketAddr` (IPv4). **Errors:** none; it cannot fail. **Special concerns:** pure function, no I/O, no locks, safe from any thread. Transmit: it plays no part in keying; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn broadcast_target() -> SocketAddr`. **See also:** `discover`; test `discovery_finds_the_emulator_with_board_and_version` (session.rs).

### `parse_reply` [@ANCHOR: hpsdr:hpsdr_discovery_parse_reply]

Classifies one received discovery reply and parses it into a `Discovered` record. A datagram whose first two bytes are not `EF FE` is handed to `p2::parse_reply` (Protocol 2); anything else goes to `p1::parse_reply` (Protocol 1). It is called by `discover` for every datagram that arrives, and is public so a caller with its own socket can reuse it.

| Argument | Type | Meaning |
|---|---|---|
| `from` | `SocketAddr` | Where the datagram came from; stored as `Discovered::addr`, the address to use for everything else. |
| `d` | `&[u8]` | The raw datagram. |

**Returns:** `Discovered` with protocol, address, MAC, board, code version, busy flag and, for Protocol 2 only, protocol version, DDC count and phase-word flag (`None` or false for Protocol 1). **Errors:** `HpsdrError::Truncated` or `BadMagic` (or `Unsupported`) from the underlying parser. **Special concerns:** pure, no I/O or locks; it must survive any bytes without panicking (the property test feeds it arbitrary datagrams). Transmit: not involved; relay lock, band check and gate do not apply.

**Signature:** `pub fn parse_reply(from: SocketAddr, d: &[u8]) -> Result<Discovered, HpsdrError>`. **See also:** tests `hpsdr_packet_parsers_survive_any_datagram` (tests/parser_props.rs), `discovery_and_reply_round_trip` (p2.rs).

### `udp_socket` [@ANCHOR: hpsdr:hpsdr_discovery_udp_socket]

Crate-internal helper that binds a UDP socket and registers it with the tokio runtime behind a caller-supplied `Handle`. The library never creates a runtime of its own, so every component that opens a socket (discovery, both sessions, both emulators) goes through this function. It binds with `std::net::UdpSocket`, sets it non-blocking, enters the handle's context and converts it with `UdpSocket::from_std`.

| Argument | Type | Meaning |
|---|---|---|
| `handle` | `&Handle` | The caller's tokio runtime; the socket is registered with its reactor. |
| `bind` | `SocketAddr` | Local address and port to bind (port 0 picks a free one). |

**Returns:** a tokio `UdpSocket`. **Errors:** `HpsdrError::Io` when the bind, the non-blocking switch or the registration fails (for example the port is taken). **Special concerns:** it does not block, takes no lock, and only the `enter` guard is held for the conversion. Transmit: it opens a socket but sends nothing; the relay lock, band check and gate apply later, in the packet builders and sessions.

**Signature:** `pub(crate) fn udp_socket(handle: &Handle, bind: SocketAddr) -> Result<UdpSocket, HpsdrError>`. **See also:** test `discovery_finds_the_emulator_with_board_and_version`.

### `discover` [@ANCHOR: hpsdr:hpsdr_discovery_discover]

Finds radios on the network. It opens an ephemeral UDP socket with broadcast enabled, sends both the Protocol 1 (Metis) and the Protocol 2 discovery request to every target (a Protocol 1 radio answers the first and ignores the second, a Protocol 2 radio the reverse), then collects replies until `wait` elapses. It is the first call of the usage example in this README.

| Argument | Type | Meaning |
|---|---|---|
| `handle` | `&Handle` | The caller's tokio runtime. |
| `targets` | `&[SocketAddr]` | Where to send the request: `broadcast_target()` to find all radios, or one unicast address to find one. |
| `wait` | `Duration` | How long to keep collecting replies. |

**Returns:** a `Vec<Discovered>`; replies that fail to parse are skipped, and each radio appears once (same MAC and address). **Errors:** `HpsdrError::Io` if the socket cannot be opened or a send or receive fails. **Special concerns:** async; it suspends for the whole `wait` time and holds no locks. It sends only discovery packets, which carry no PTT, drive or I/Q, so the relay lock, band check and transmit gate do not apply.

**Signature:** `pub async fn discover(handle: &Handle, targets: &[SocketAddr], wait: Duration) -> Result<Vec<Discovered>, HpsdrError>`. **See also:** test `discovery_finds_the_emulator_with_board_and_version` (session.rs).

### `EmulatorConfig::default` [@ANCHOR: hpsdr:hpsdr_emulator_config_default]

The default configuration of the Protocol 1 emulator: board id 1 (Hermes), MAC `02:48:50:53:44:52`, code version 73, one tone at 7,100,500 Hz with amplitude 0.5 (about -6 dBFS), a status with firmware 73 and a raw supply reading of 3500, a 2 ms pace between EP6 datagrams, and `skip_every` 0 (no loss injection). Tests start from it and override fields with struct update syntax.

| Field | Type | Meaning |
|---|---|---|
| `board_id` | `u8` | Protocol 1 board id reported in discovery. |
| `mac`, `code_version` | `[u8; 6]`, `u8` | Identity reported in discovery. |
| `tones` | `Vec<Tone>` | Carriers at the antenna, absolute frequency in Hz and amplitude (1.0 is full scale). |
| `status` | `HardwareStatus` | What the status C&C bytes report. |
| `pace` | `Duration` | Interval between EP6 datagrams. |
| `skip_every` | `u32` | When non-zero, every Nth datagram is numbered but not sent. |

**Returns:** an `EmulatorConfig`. **Errors:** none. **Special concerns:** pure; the values model the published documents, not a real radio (`verified=unverified`). Transmit: nothing in this entry can key a transmitter or produce RF; the emulator only counts what a transmit request would have been. The relay lock and band check do not apply to it. The crate itself contains no relay lock (unknown whether the relay holds one around the gate; to-do: record that in the relay docs once the relay consumes this crate).

**Signature:** `impl Default for EmulatorConfig { fn default() -> Self }`. **See also:** test `discovery_finds_the_emulator_with_board_and_version`.

### `Emulator::start_p1` [@ANCHOR: hpsdr:hpsdr_emulator_start_p1]

Starts the Protocol 1 emulator: binds a UDP socket on `127.0.0.1` (the constructor takes a port, never an address, so it cannot listen on a real interface), then spawns the `run_p1` task on the caller's runtime and returns a handle holding the bound address, the shared counters and the task. Tests use it instead of hardware.

| Argument | Type | Meaning |
|---|---|---|
| `handle` | `&Handle` | The caller's tokio runtime; the task is spawned on it. |
| `port` | `u16` | Loopback port; 0 picks a free one (read it back with `addr`). |
| `cfg` | `EmulatorConfig` | Identity, tones, status, pace and loss injection. |

**Returns:** an `Emulator`. **Errors:** `HpsdrError::Io` if the bind fails. **Special concerns:** spawns one background task that runs until `stop` or until the runtime ends; dropping the `Emulator` does not stop it, only `stop` aborts it. Transmit: nothing in this entry can key a transmitter or produce RF; the emulator only counts what a transmit request would have been. The relay lock and band check do not apply to it. The crate itself contains no relay lock (unknown whether the relay holds one around the gate; to-do: record that in the relay docs once the relay consumes this crate).

**Signature:** `pub fn start_p1(handle: &Handle, port: u16, cfg: EmulatorConfig) -> Result<Self, HpsdrError>`. **See also:** tests `discovery_finds_the_emulator_with_board_and_version`, `lost_datagrams_and_a_slow_consumer_are_counted` (session.rs).

### `Emulator::addr` [@ANCHOR: hpsdr:hpsdr_emulator_addr]

Returns the loopback address and port the Protocol 1 emulator is listening on, as resolved at bind time. Tests use it as the unicast discovery target and as the radio address in a `Discovered` record, which matters when `start_p1` was given port 0 and the operating system chose the port.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | `&self` only. |

**Returns:** a `SocketAddr` whose IP is always `127.0.0.1`. **Errors:** none. **Special concerns:** a plain read of a field set at construction, no locks, safe from any thread. It stays valid after `stop`, but nothing listens there any more. Transmit: nothing in this entry can key a transmitter or produce RF; the emulator only counts what a transmit request would have been. The relay lock and band check do not apply to it. The crate itself contains no relay lock (unknown whether the relay holds one around the gate; to-do: record that in the relay docs once the relay consumes this crate).

**Signature:** `pub fn addr(&self) -> SocketAddr`. **See also:** `start_p1`; test `discovery_finds_the_emulator_with_board_and_version` (session.rs).

### `Emulator::counters` [@ANCHOR: hpsdr:hpsdr_emulator_counters]

Gives access to what the Protocol 1 emulator has seen from its client: discoveries answered, Start and Stop commands, EP2 datagrams parsed, frames with MOX set, frames with non-zero drive, datagrams with non-zero transmit I/Q, and EP6 datagrams sent. The tests assert that the MOX, drive and transmit-I/Q counters stay zero while the transmit gate is closed, which is how the fail-closed design is checked end to end.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | `&self` only. |

**Returns:** a reference to the shared `EmulatorCounters`, a struct of `AtomicU64` fields updated by the emulator task; read each with `EmulatorCounters::get`. **Errors:** none. **Special concerns:** lock-free atomics with relaxed ordering, so values are eventually consistent while the task runs; fine for tests that await the effect first. Transmit: this is the observation point for the gate; the emulator never produces RF itself, and the relay lock and band check are not modelled.

**Signature:** `pub fn counters(&self) -> &EmulatorCounters`. **See also:** `EmulatorCounters::get`; test `two_receivers_stream_their_own_tones_without_loss` (session.rs).

### `Emulator::stop` [@ANCHOR: hpsdr:hpsdr_emulator_stop]

Stops the Protocol 1 emulator by aborting its background task, which drops the UDP socket and releases the port. It consumes the `Emulator`, so the counters reference and address can no longer be read through it afterwards. Tests call it at the end to free the loopback port.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `Emulator` | The emulator, moved in. |

**Returns:** nothing. **Errors:** none. **Special concerns:** `JoinHandle::abort` takes effect at the task's next await point, so a datagram being handled may finish first; it does not wait for the task to end. It takes no locks. Transmit: not involved; nothing is keyed and the relay lock, band check and gate do not apply.

**Signature:** `pub fn stop(self)`. **See also:** `start_p1`; test `discovery_finds_the_emulator_with_board_and_version` (session.rs).

### `EmulatorCounters::get` [@ANCHOR: hpsdr:hpsdr_emulator_counters_get]

Reads one counter of the Protocol 1 emulator, for example `EmulatorCounters::get(&emu.counters().mox_frames)`. It exists so test code does not repeat the atomic load and its memory ordering at every assertion.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&AtomicU64` | One field of `EmulatorCounters`. |

**Returns:** the current value as `u64`. **Errors:** none. **Special concerns:** a relaxed atomic load, no locks, never blocks. It only observes; the transmit gate, relay lock and band check are not involved.

**Signature:** `pub fn get(c: &AtomicU64) -> u64`. **See also:** `Emulator::counters`; test `two_receivers_stream_their_own_tones_without_loss` (session.rs).

### `synth` [@ANCHOR: hpsdr:hpsdr_emulator_synth]

Generates synthetic receive samples for one receiver. For each of `count` sample indices starting at `start`, it sums every tone whose offset from the receiver centre lies inside the Nyquist band (`|f| < rate/2`) as a complex phasor, scales to 24-bit integers (full scale 8,388,607) with rounding and clamping, and returns `(I, Q)` pairs. Tones at absolute frequencies mean two receivers with different centres see the same carrier at different offsets. Both emulators use it.

| Argument | Type | Meaning |
|---|---|---|
| `tones` | `&[Tone]` | Carriers: absolute `freq_hz` and `amplitude` (1.0 is full scale). |
| `centre_hz` | `u32` | The receiver's centre frequency. |
| `rate_hz` | `u32` | Sample rate in hertz. |
| `start` | `u64` | Index of the first sample, so successive calls are phase continuous. |
| `count` | `usize` | Number of pairs to produce. |

**Returns:** `Vec<(i32, i32)>`. **Errors:** none. **Special concerns:** pure CPU work, linear in `count` times tones; no I/O or locks. Transmit: it fabricates receive data only; relay lock, band check and gate do not apply.

**Signature:** `pub fn synth(tones: &[Tone], centre_hz: u32, rate_hz: u32, start: u64, count: usize) -> Vec<(i32, i32)>`. **See also:** test `two_receivers_stream_their_own_tones_without_loss` (session.rs).

### `handle_datagram` [@ANCHOR: hpsdr:hpsdr_emulator_handle_datagram]

Private function that acts on one datagram a client sent to the Protocol 1 emulator. A discovery request (`EF FE 02`) is counted and answered with an encoded reply whose `running` flag shows whether a destination is set. A Start or Stop command (`EF FE 04 <cmd>`) sets or clears the streaming destination and resets the sequence. An EP2 datagram is parsed, counted, its C&C folded into the emulator's radio settings, and MOX frames, non-zero drive frames and non-zero transmit I/Q are counted. Anything else is ignored.

| Argument | Type | Meaning |
|---|---|---|
| `d` | `&[u8]` | The datagram. |
| `cfg` | `&EmulatorConfig` | Identity to report. |
| `rf` | `&mut P1Config` | Settings learned from C&C. |
| `dest` | `&mut Option<SocketAddr>` | Where EP6 goes; set by Start, cleared by Stop. |
| `from` | `SocketAddr` | Sender. |
| `c` | `&EmulatorCounters` | Counters to update. |
| `seq` | `&mut u32` | EP6 sequence, reset on Start or Stop. |

**Returns:** `Some(reply bytes)` for discovery, otherwise `None`. **Errors:** none; bad datagrams are dropped silently. **Special concerns:** synchronous, called only from the task. Transmit: nothing in this entry can key a transmitter or produce RF; the emulator only counts what a transmit request would have been. The relay lock and band check do not apply to it. The crate itself contains no relay lock (unknown whether the relay holds one around the gate; to-do: record that in the relay docs once the relay consumes this crate).

**Signature:** `fn handle_datagram(d, cfg, rf, dest, from, c, seq) -> Option<Vec<u8>>`. **See also:** test `discovery_finds_the_emulator_with_board_and_version`.

### `next_ep6` [@ANCHOR: hpsdr:hpsdr_emulator_next_ep6]

Private function that builds the next EP6 (radio to PC) datagram from the emulator's current settings. For each active receiver it calls `synth` with that receiver's centre and the current rate for two frames' worth of samples, adds a zeroed microphone track, attaches two status C&C quintuples chosen by a rotating `cursor` over the five status addresses, and encodes them with `p1::encode_ep6`.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&EmulatorConfig` | Tones and status. |
| `rf` | `&P1Config` | Receiver count, centres, rate as set by the client. |
| `seq` | `u32` | Datagram sequence number. |
| `n` | `u64` | Sample index of the first sample, for phase continuity. |
| `cursor` | `u8` | Rotating status address, modulo 5. |

**Returns:** `Some(datagram)` or `None` if encoding fails. **Errors:** none surfaced. **Special concerns:** pure CPU, no I/O. Transmit: builds receive-direction data only; relay lock, band check and gate do not apply.

**Signature:** `fn next_ep6(cfg, rf, seq, n, cursor) -> Option<Vec<u8>>`. **See also:** `run_p1`; test `two_receivers_stream_their_own_tones_without_loss`.

### `run_p1` [@ANCHOR: hpsdr:hpsdr_emulator_run_p1]

The Protocol 1 emulator's background task, spawned by `start_p1`. It loops in a `tokio::select!` over two events: a datagram received on the socket (passed to `handle_datagram`, and any reply sent back) and, only after a Start has set a destination, a timer tick every `cfg.pace`. On a tick it builds and sends one EP6 datagram (skipping the send but still numbering it when loss injection selects it), advances the sample index and sequence, and rotates the status cursor.

| Argument | Type | Meaning |
|---|---|---|
| `sock` | `UdpSocket` | The bound loopback socket. |
| `cfg` | `EmulatorConfig` | Behaviour. |
| `c` | `Arc<EmulatorCounters>` | Shared counters. |

**Returns:** never returns normally; it ends only when aborted by `stop`. **Errors:** receive and send errors are ignored. **Special concerns:** async, single task, missed ticks are delayed rather than bursted. Transmit: nothing in this entry can key a transmitter or produce RF; the emulator only counts what a transmit request would have been. The relay lock and band check do not apply to it. The crate itself contains no relay lock (unknown whether the relay holds one around the gate; to-do: record that in the relay docs once the relay consumes this crate).

**Signature:** `async fn run_p1(sock: UdpSocket, cfg: EmulatorConfig, c: Arc<EmulatorCounters>)`. **See also:** `lost_datagrams_and_a_slow_consumer_are_counted` (session.rs).

### `P2EmulatorConfig::default` [@ANCHOR: hpsdr:hpsdr_emulator_p2_config_default]

The default configuration of the Protocol 2 emulator: board type 1 (Hermes), MAC `02:48:50:53:44:32`, firmware 39, 8 DDCs, phase words on, one tone at 7,100,500 Hz at amplitude 0.5, a status with a raw supply reading of 3500, a 2 ms pace, no loss injection and the specification's default `RadioPorts`. Tests that need distinct ports override `ports` with `loopback_ports`.

| Field | Type | Meaning |
|---|---|---|
| `board_type`, `mac`, `firmware` | `u8`, `[u8; 6]`, `u8` | Identity in the discovery reply. |
| `num_ddcs` | `u8` | DDCs claimed and served. |
| `phase_word` | `bool` | Report and expect phase words rather than hertz. |
| `tones`, `status` | | Antenna carriers and reported hardware status. |
| `pace`, `skip_every` | `Duration`, `u32` | Packet interval and loss injection. |
| `ports` | `RadioPorts` | The port map served. |

**Returns:** a `P2EmulatorConfig`. **Errors:** none. **Special concerns:** pure. Transmit: nothing in this entry can key a transmitter or produce RF; the emulator only counts what a transmit request would have been. The relay lock and band check do not apply to it. The crate itself contains no relay lock (unknown whether the relay holds one around the gate; to-do: record that in the relay docs once the relay consumes this crate).

**Signature:** `impl Default for P2EmulatorConfig { fn default() -> Self }`. **See also:** test `listens_on_loopback_only_and_answers_discovery` (emulator_p2.rs).

### `P2Counters::get` [@ANCHOR: hpsdr:hpsdr_emulator_p2_counters_get]

Reads one counter of the Protocol 2 emulator, for example the PTT frame count, the drive-non-zero count or the DUC packet count, so a test can assert that nothing transmit-like happened while the gate was closed. It wraps a relaxed atomic load.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&AtomicU64` | One field of `P2Counters`. |

**Returns:** the current value as `u64`. **Errors:** none. **Special concerns:** lock-free, never blocks; values are eventually consistent while the emulator task runs. It only observes: the relay lock, band check and transmit gate are not involved.

**Signature:** `pub fn get(c: &AtomicU64) -> u64`. **See also:** `EmulatorP2::counters`; test `listens_on_loopback_only_and_answers_discovery`.

### `loopback_ports` [@ANCHOR: hpsdr:hpsdr_emulator_p2_loopback_ports]

Finds a block of free loopback UDP ports so that several Protocol 2 emulators can run in one test process without clashing. It probes a block of 30 consecutive ports starting at `30000 + (k mod 600) * 40`, where `k` is a process-wide counter, releases them, and returns a `RadioPorts` whose offsets follow the layout the emulator expects: DDC-specific at base, DUC-specific +1, High Priority from PC +2, to PC +3, audio +4, DUC0 I/Q +5, DDC0 +8 (DDC `n` source ports are `ddc0 + n`), mic +24, wideband +25. It tries up to 200 blocks.

| Argument | Type | Meaning |
|---|---|---|
| `ddcs` | `u8` | Number of DDCs; must be at most 16 or no block is accepted. |

**Returns:** `RadioPorts`. **Errors:** `HpsdrError::Io("no free loopback port block")` after 200 failures. **Special concerns:** racy by nature (ports are free when probed, not reserved), so a rare clash is possible. Transmit: nothing in this entry can key a transmitter or produce RF; the emulator only counts what a transmit request would have been. The relay lock and band check do not apply to it. The crate itself contains no relay lock (unknown whether the relay holds one around the gate; to-do: record that in the relay docs once the relay consumes this crate).

**Signature:** `pub fn loopback_ports(ddcs: u8) -> Result<RadioPorts, HpsdrError>`. **See also:** test `listens_on_loopback_only_and_answers_discovery`.

### `EmulatorP2::start` [@ANCHOR: hpsdr:hpsdr_emulator_p2_start]

Starts the Protocol 2 emulator. It binds `127.0.0.1:port` for discovery and General, then the DDC-specific port, the High Priority input port, the DUC0 I/Q port, the status output port (shared with the DDC-specific socket when the map says so) and one output socket per DDC at `ddc0 + n`, all from `cfg.ports`. It then spawns the `run` task on the caller's runtime and returns the handle. Loopback only: the constructor takes no address.

| Argument | Type | Meaning |
|---|---|---|
| `handle` | `&Handle` | The caller's tokio runtime. |
| `port` | `u16` | Command port; 0 picks a free one (read it with `addr`). |
| `cfg` | `P2EmulatorConfig` | Identity, tones, pace, loss injection and the port map. |

**Returns:** an `EmulatorP2`. **Errors:** `HpsdrError::Io` if any bind fails. **Special concerns:** spawns one task; sockets are released only by `stop`. Transmit: nothing in this entry can key a transmitter or produce RF; the emulator only counts what a transmit request would have been. The relay lock and band check do not apply to it. The crate itself contains no relay lock (unknown whether the relay holds one around the gate; to-do: record that in the relay docs once the relay consumes this crate).

**Signature:** `pub fn start(handle: &Handle, port: u16, cfg: P2EmulatorConfig) -> Result<Self, HpsdrError>`. **See also:** test `listens_on_loopback_only_and_answers_discovery`.

### `EmulatorP2::addr` [@ANCHOR: hpsdr:hpsdr_emulator_p2_addr]

Returns the loopback command address of the Protocol 2 emulator: the address that discovery and the General packet are sent to. Its port is the one resolved at bind time, so it is the way to learn the port when `start` was given 0. The other ports of the radio are in the `RadioPorts` the configuration carried.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | `&self` only. |

**Returns:** a `SocketAddr` with a loopback IP (the test asserts `is_loopback`). **Errors:** none. **Special concerns:** a plain field read, no locks, thread safe. Transmit: nothing in this entry can key a transmitter or produce RF; the emulator only counts what a transmit request would have been. The relay lock and band check do not apply to it. The crate itself contains no relay lock (unknown whether the relay holds one around the gate; to-do: record that in the relay docs once the relay consumes this crate).

**Signature:** `pub fn addr(&self) -> SocketAddr`. **See also:** `EmulatorP2::start`; test `listens_on_loopback_only_and_answers_discovery`.

### `EmulatorP2::counters` [@ANCHOR: hpsdr:hpsdr_emulator_p2_counters]

Gives access to what the Protocol 2 emulator has seen: discoveries, General, DDC-specific and High Priority packets, run-bit starts and stops, DDC packets sent, frames with PTT set, frames with non-zero drive, frames with a non-zero transmit frequency, packets on the DUC0 I/Q port, plus the latest PTT state, drive, ADC0 attenuator and DDC0 rate. Tests assert the transmit-like counters stay zero while the gate is closed.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | `&self` only. |

**Returns:** a reference to the shared `P2Counters` (atomics, relaxed). **Errors:** none. **Special concerns:** lock-free; read with `P2Counters::get`. Transmit: this is the observation point for the session's gate; the emulator never produces RF, and it does not model the relay lock or band check.

**Signature:** `pub fn counters(&self) -> &P2Counters`. **See also:** test `listens_on_loopback_only_and_answers_discovery`.

### `EmulatorP2::stop` [@ANCHOR: hpsdr:hpsdr_emulator_p2_stop]

Stops the Protocol 2 emulator by aborting its task, which drops every socket it bound (command, DDC-specific, High Priority, DUC0 I/Q, status and the per-DDC outputs) and frees the ports. It consumes the handle.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `EmulatorP2` | The emulator, moved in. |

**Returns:** nothing. **Errors:** none. **Special concerns:** abort takes effect at the next await point and does not wait for the task to finish; no locks. Transmit: not involved; nothing is keyed, and the relay lock, band check and gate do not apply.

**Signature:** `pub fn stop(self)`. **See also:** `EmulatorP2::start`; test `listens_on_loopback_only_and_answers_discovery`.

### `apply_ddc_specific` [@ANCHOR: hpsdr:hpsdr_emulator_p2_apply_ddc_specific]

Private function that folds a DDC-specific packet (Protocol 2, to the DDC-specific port) into the emulator's state. For each emulated DDC it reads the enable bit from byte 7 onward (bit `n mod 8` of byte `7 + n/8`) and the sample rate in kilohertz from bytes 18 and 19 of the 6-byte group starting at byte 17, and stores the first DDC's rate in `last_rate_ksps`. Packets shorter than `p2::BIG_LEN` are ignored.

| Argument | Type | Meaning |
|---|---|---|
| `d` | `&[u8]` | The packet. |
| `rf` | `&mut RfState` | Per-DDC enable flags and rates. |
| `c` | `&P2Counters` | Counters, for the rate gauge. |

**Returns:** nothing. **Errors:** none; unknown rates leave the previous rate. **Special concerns:** synchronous, called only from the task, no locks. Transmit: nothing in this entry can key a transmitter or produce RF; the emulator only counts what a transmit request would have been. The relay lock and band check do not apply to it. The crate itself contains no relay lock (unknown whether the relay holds one around the gate; to-do: record that in the relay docs once the relay consumes this crate).

**Signature:** `fn apply_ddc_specific(d: &[u8], rf: &mut RfState, c: &P2Counters)`. **See also:** test `ddc_packets_follow_the_settings` (emulator_p2.rs).

### `apply_hp` [@ANCHOR: hpsdr:hpsdr_emulator_p2_apply_hp]

Private function that folds a High Priority packet into the emulator's state and counters. It counts the packet, tracks run-bit transitions (`run_starts`, `run_stops`), records PTT (bit 1 of byte 4) in `ptt_now` and `ptt_frames`, counts non-zero drive (byte 345) and a non-zero transmit frequency (bytes 329 to 332), stores the ADC0 attenuator (byte 1443), and decodes each DDC frequency word from bytes 9 onward, converting phase words to hertz with the board's DSP clock when phase words are in use.

| Argument | Type | Meaning |
|---|---|---|
| `d` | `&[u8]` | The packet; shorter than `p2::BIG_LEN` is ignored. |
| `cfg` | `&P2EmulatorConfig` | Board type, for the clock. |
| `rf` | `&mut RfState` | Run, attenuator and frequencies. |
| `c` | `&P2Counters` | Counters. |

**Returns:** nothing. **Errors:** none. **Special concerns:** synchronous, no locks. Transmit: this is where transmit-like fields (PTT, drive, transmit frequency) are only counted, never acted on; no RF is produced, and the relay lock and band check are not modelled.

**Signature:** `fn apply_hp(d, cfg, rf, c)`. **See also:** test `ddc_packets_follow_the_settings`.

### `handle_cmd` [@ANCHOR: hpsdr:hpsdr_emulator_p2_handle_cmd]

Private function that acts on one datagram arriving on the Protocol 2 emulator's command port. A discovery request (zero sequence, command 2) is counted and answered with an encoded reply carrying the configured identity and run state. A General packet (command 0, long enough) is counted, its sender is remembered as the host that all later traffic goes to, and byte 37 bit 3 sets whether frequencies are phase words. Everything else is ignored.

| Argument | Type | Meaning |
|---|---|---|
| `d` | `&[u8]` | The datagram. |
| `cfg` | `&P2EmulatorConfig` | Identity to report. |
| `rf` | `&mut RfState` | Host address and phase-word flag. |
| `from` | `SocketAddr` | Sender. |
| `c` | `&P2Counters` | Counters. |

**Returns:** `Some(reply)` for discovery, otherwise `None`. **Errors:** none. **Special concerns:** synchronous, no locks. Transmit: nothing in this entry can key a transmitter or produce RF; the emulator only counts what a transmit request would have been. The relay lock and band check do not apply to it. The crate itself contains no relay lock (unknown whether the relay holds one around the gate; to-do: record that in the relay docs once the relay consumes this crate).

**Signature:** `fn handle_cmd(d, cfg, rf, from, c) -> Option<Vec<u8>>`. **See also:** test `listens_on_loopback_only_and_answers_discovery`.

### `ddc_packet` [@ANCHOR: hpsdr:hpsdr_emulator_p2_ddc_packet]

Private function that builds the next DDC packet of receiver `n` for the Protocol 2 emulator. It scales every configured tone by the ADC0 attenuator (`10^(-atten/20)`), calls `synth` for `DDC_SAMPLES` (238) samples from sample index `at` at the receiver's current centre and rate, and encodes the result with `p2::encode_ddc_packet` using the sequence number and `at` as the timestamp.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&P2EmulatorConfig` | The tones. |
| `rf` | `&RfState` | Receiver centres, rates and attenuator. |
| `n` | `usize` | Receiver index. |
| `seq` | `u32` | Packet sequence. |
| `at` | `u64` | Sample index of the first sample. |

**Returns:** the packet bytes. **Errors:** none. **Special concerns:** pure CPU, no locks. Transmit: receive data only; relay lock, band check and gate do not apply.

**Signature:** `fn ddc_packet(cfg, rf, n, seq, at) -> Vec<u8>`. **See also:** test `ddc_packets_follow_the_settings` (emulator_p2.rs).

### `run` [@ANCHOR: hpsdr:hpsdr_emulator_p2_run]

The Protocol 2 emulator's background task, spawned by `EmulatorP2::start`. It selects over five inputs: the command port (to `handle_cmd`, replying to discovery), the DDC-specific port (to `apply_ddc_specific`), the High Priority port (to `apply_hp`, resetting sequences when the run bit is clear), the DUC0 I/Q port (only counted), and a timer every `cfg.pace` that, once a General packet has named a host, sends a High Priority status packet and, while the run bit is set, one DDC packet per enabled receiver from that receiver's own source port (honouring `skip_every`).

| Argument | Type | Meaning |
|---|---|---|
| `io` | `Io` | The bound sockets. |
| `cfg` | `P2EmulatorConfig` | Behaviour. |
| `c` | `Arc<P2Counters>` | Shared counters. |

**Returns:** it runs until aborted by `stop`. **Errors:** receive and send errors are ignored. **Special concerns:** async, single task, missed ticks delayed. Transmit: nothing in this entry can key a transmitter or produce RF; the emulator only counts what a transmit request would have been. The relay lock and band check do not apply to it. The crate itself contains no relay lock (unknown whether the relay holds one around the gate; to-do: record that in the relay docs once the relay consumes this crate).

**Signature:** `async fn run(io: Io, cfg: P2EmulatorConfig, c: Arc<P2Counters>)`. **See also:** test `listens_on_loopback_only_and_answers_discovery`.

### `HpsdrError::fmt` [@ANCHOR: hpsdr:hpsdr_error_fmt]

The `Display` implementation of `HpsdrError`, the crate's one error type. It turns each variant into a fixed human-readable text: "datagram too short", "unexpected magic, sync or command byte", "unsupported: <m>", "invalid setting: <m>", "transmit gate is closed", "transmit frequency is outside the allowed bands", "i/o error: <m>", "timed out" and "session stopped". Callers log or show these texts, and tests compare the gate-closed text.

| Argument | Type | Meaning |
|---|---|---|
| `f` | `&mut fmt::Formatter` | The formatter to write into. |

**Returns:** `fmt::Result`. **Errors:** only formatter errors. **Special concerns:** pure, no I/O, no locks. The two transmit-related variants, `GateClosed` (the gate has no open lease) and `OutsideLimits` (the frequency is outside the bands narrowed onto the gate), are what refused transmit requests report; the relay lock itself has no variant here (unknown whether the relay has its own; to-do).

**Signature:** `impl fmt::Display for HpsdrError { fn fmt(&self, f) -> fmt::Result }`. **See also:** test `display_and_from_io` (error.rs).

### `HpsdrError::from(io::Error)` [@ANCHOR: hpsdr:hpsdr_error_from_io]

Converts a `std::io::Error` into `HpsdrError::Io` carrying the error's text, so the `?` operator works on socket and task calls throughout the crate (binds, sends, receives). The original error kind is not kept, only its message, which keeps `HpsdrError` `Clone`, `PartialEq` and `Eq`.

| Argument | Type | Meaning |
|---|---|---|
| `e` | `std::io::Error` | The failure to wrap. |

**Returns:** `HpsdrError::Io(String)`. **Errors:** none. **Special concerns:** pure; the loss of the error kind means callers cannot match on, for example, address-in-use; to-do if a caller needs that. Transmit: not involved.

**Signature:** `impl From<std::io::Error> for HpsdrError { fn from(e) -> Self }`. **See also:** test `display_and_from_io` (error.rs).

### `TxGate::new` [@ANCHOR: hpsdr:hpsdr_gate_new]

Creates a closed transmit gate. `TxGate` is the crate's safety interlock: every packet builder and session method that can carry PTT or MOX, a drive level or transmit I/Q takes it, and writes zeros or refuses unless the gate has an open lease. A new gate has no lease and no limits (full drive allowed, no band restriction), so nothing can be keyed until a caller opens it. It is identical to `TxGate::default()`.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

**Returns:** a closed `TxGate`. It is `Clone`; clones share the same lease and limits (two `Arc<Mutex<..>>` fields). **Errors:** none. **Special concerns:** no I/O, no blocking. Transmit: this is the transmit gate itself; creating it keys nothing. The band limits start unset because the crate has no band plan of its own; the caller (the relay) must narrow them with `narrow_limits`. Whether a relay lock sits in front of the gate is not visible in this crate (unknown; to-do: document at the relay).

**Signature:** `pub fn new() -> Self`. **See also:** test `default_gate_is_closed_and_lease_expires` (gate.rs).

### `TxGate::open_until` [@ANCHOR: hpsdr:hpsdr_gate_open_until]

Opens the transmit gate with a lease that ends at `deadline`, but never later than `MAX_LEASE` (60 seconds) from now, whatever the caller passes, so an absurd deadline cannot hold the gate open. After the deadline the gate closes by itself, so a caller that dies or stalls cannot leave it open; a caller that wants to keep transmitting must renew the lease. It replaces any earlier lease.

| Argument | Type | Meaning |
|---|---|---|
| `deadline` | `Instant` | When the lease should end. |

**Returns:** `true` when the lease was set; `false` (gate left as it was, which reads as closed) if the lease mutex is poisoned. **Errors:** none (the boolean is the error channel). **Special concerns:** takes the lease lock briefly, never blocks long. Transmit: opening the gate is necessary but not sufficient; each key request still passes `check_tx` for the drive cap and band limits, and the relay's own lock and band check are outside this crate (unknown; to-do).

**Signature:** `pub fn open_until(&self, deadline: Instant) -> bool`. **See also:** tests `default_gate_is_closed_and_lease_expires`, `poisoned_gate_reads_closed`, `a_lease_is_never_longer_than_the_maximum` (gate.rs).

### `TxGate::narrow_limits` [@ANCHOR: hpsdr:hpsdr_gate_narrow_limits]

Restricts what may be sent while the gate is open. The drive cap becomes the lower of the current cap and `max_drive`; the allowed bands become the overlap of the current bands (if any were set) with `bands`, as inclusive `(low_hz, high_hz)` pairs. Limits only ever narrow, so a later, looser call cannot undo an earlier restriction. This is where the relay's band check is expressed to this crate, since the crate has no band plan of its own.

| Argument | Type | Meaning |
|---|---|---|
| `max_drive` | `u8` | Highest drive level (0 to 255) to allow. |
| `bands` | `&[(u32, u32)]` | Allowed frequency ranges in hertz, inclusive. An empty list allows nothing. |

**Returns:** nothing. **Errors:** none. **Special concerns:** takes the limits lock; if it was poisoned the data is taken anyway, so narrowing still applies. The first call turns "no band limit" into an explicit list. Clones of the gate share the limits. Transmit: enforced at key time by `check_tx` and thus by `request_ptt` of both sessions; who calls this, and whether a relay lock guards it, is not in this crate (unknown; to-do).

**Signature:** `pub fn narrow_limits(&self, max_drive: u8, bands: &[(u32, u32)])`. **See also:** test `limits_only_narrow` (gate.rs).

### `TxGate::check_tx` [@ANCHOR: hpsdr:hpsdr_gate_check_tx]

Decides the drive to use for a key request at a frequency. If any bands are set and `tx_hz` is in none of them, it refuses; otherwise it returns `drive` capped at the gate's maximum. It does not look at the lease: the lease is checked separately by `is_open` when the datagram is built. Both sessions call it from `request_ptt`, so a request outside the bands never reaches a packet.

| Argument | Type | Meaning |
|---|---|---|
| `tx_hz` | `u32` | Transmit frequency in hertz. |
| `drive` | `u8` | Requested drive level. |

**Returns:** `Ok(drive)` capped at `max_drive`. **Errors:** `HpsdrError::OutsideLimits` when the frequency is outside the allowed bands, or when the limits lock is poisoned (fails closed). **Special concerns:** takes the limits lock briefly; no I/O. With no bands set it allows any frequency, so a caller that never calls `narrow_limits` has no band check here (the relay's check is the authority; unknown whether the relay always narrows; to-do).

**Signature:** `pub fn check_tx(&self, tx_hz: u32, drive: u8) -> Result<u8, HpsdrError>`. **See also:** tests `limits_only_narrow` (gate.rs), `the_gates_limits_refuse_a_frequency_and_cap_the_drive` (session.rs, session_p2.rs).

### `TxGate::close` [@ANCHOR: hpsdr:hpsdr_gate_close]

Closes the gate immediately by clearing its lease. Callers use it for an explicit stop or an emergency; it takes effect for the next datagram built. The limits are untouched, so a later `open_until` is still bound by the earlier narrowing.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | `&self` only. |

**Returns:** nothing. **Errors:** none. **Special concerns:** takes the lease lock; if a panic poisoned it, the data is taken anyway so that closing always works (the opposite of a refusal: fail closed). Clones share the lease, so closing one clone closes all. Transmit: this is the immediate way to stop new transmit datagrams from being built; it does not by itself send an unkey datagram, which the sessions do on `stop` and on drop.

**Signature:** `pub fn close(&self)`. **See also:** tests `default_gate_is_closed_and_lease_expires`, `poisoned_gate_reads_closed` (gate.rs).

### `TxGate::is_open` [@ANCHOR: hpsdr:hpsdr_gate_is_open]

Reports whether the gate is open at the given instant: true only while a lease granted by `open_until` has not expired, and the deadline itself counts as expired. The packet builders and sessions pass a clock reading so the result is consistent within one datagram and testable without sleeping.

| Argument | Type | Meaning |
|---|---|---|
| `now` | `Instant` | The time to test against. |

**Returns:** `bool`; false when no lease was ever granted, after `close`, after the deadline, or when the lease mutex is poisoned (fails closed). **Errors:** none. **Special concerns:** takes the lease lock briefly, no I/O. Transmit: `build_tx` and the session datagram builders call it for every datagram, so an expired lease turns the next datagram into a receive-only one with no PTT, drive or I/Q.

**Signature:** `pub fn is_open(&self, now: Instant) -> bool`. **See also:** tests `default_gate_is_closed_and_lease_expires`, `poisoned_gate_reads_closed` (gate.rs).

### Anchors not yet expanded

- [@ANCHOR: hpsdr:hpsdr_iq_counters_delivered] (iq) Record one delivered block of `n` samples.
- [@ANCHOR: hpsdr:hpsdr_iq_counters_lost] (iq) Record `n` datagrams lost to a sequence gap.
- [@ANCHOR: hpsdr:hpsdr_iq_counters_dropped] (iq) Record one block dropped because the consumer lagged.
- [@ANCHOR: hpsdr:hpsdr_iq_counters_malformed] (iq) Record one rejected datagram.
- [@ANCHOR: hpsdr:hpsdr_iq_counters_snapshot] (iq) Read all counters.
- [@ANCHOR: hpsdr:hpsdr_iq_seq_observe] (iq) Account for one datagram; returns how many datagrams were skipped before it. A sequence
- [@ANCHOR: hpsdr:hpsdr_iq_channel_new] (iq) A stream and the sender that feeds it; `depth` is the number of blocks buffered before drops.
- [@ANCHOR: hpsdr:hpsdr_iq_channel_next_block] (iq) Wait for the next block.
- [@ANCHOR: hpsdr:hpsdr_iq_channel_stats] (iq) Counters for this stream.
- [@ANCHOR: hpsdr:hpsdr_p1_discovery_packet] (p1) The discovery request: `EF FE 02` and 60 zero bytes (Metis, "Metis Discover Packet format"). Send it
- [@ANCHOR: hpsdr:hpsdr_p1_start_packet] (p1) Start command: `EF FE 04 <cmd>` and 60 zeros; bit 0 starts I/Q plus microphone data, bit 1 the
- [@ANCHOR: hpsdr:hpsdr_p1_stop_packet] (p1) Stop command: the start layout with both bits clear (Metis, "Stop Command").
- [@ANCHOR: hpsdr:hpsdr_p1_parse_reply] (p1) Parse a discovery reply: `EF FE <status> <MAC 6> <code version> <board id>` then zeros.
- [@ANCHOR: hpsdr:hpsdr_p1_encode_reply] (p1) Build a discovery reply (used by the emulator): 60 bytes.
- [@ANCHOR: hpsdr:hpsdr_p1_config_default] (p1) One receiver at 48 ksps on 7.1 MHz, no attenuation.
- [@ANCHOR: hpsdr:hpsdr_p1_config_validate] (p1) Reject settings the protocol cannot express.
- [@ANCHOR: hpsdr:hpsdr_p1_config_addresses] (p1) The C&C addresses sent in round-robin order: general (0), each receiver NCO (2..), transmit
- [@ANCHOR: hpsdr:hpsdr_p1_config_encode_cc] (p1) The five C&C bytes for `addr`. `mox` is C0 bit 0; `drive` is the C1 byte of address 9. The
- [@ANCHOR: hpsdr:hpsdr_p1_config_apply_cc] (p1) Fold one received C&C quintuple into the settings (used by the emulator, which plays the radio).
- [@ANCHOR: hpsdr:hpsdr_p1_ep2_new] (p1) A builder with sequence 0.
- [@ANCHOR: hpsdr:hpsdr_p1_ep2_reset] (p1) Restart the sequence at zero, as the PC must on every Start (Metis, "PC <> Metis UDP/IP data format").
- [@ANCHOR: hpsdr:hpsdr_p1_ep2_assemble] (p1) Common assembly: header, then two frames each with the next C&C address.
- [@ANCHOR: hpsdr:hpsdr_p1_ep2_build_rx] (p1) A receive-only datagram: MOX clear, drive zero, transmit I/Q zero, for every C&C address.
- [@ANCHOR: hpsdr:hpsdr_p1_ep2_build_tx] (p1) A datagram that may key the transmitter. Returns [`HpsdrError::GateClosed`] and builds nothing
- [@ANCHOR: hpsdr:hpsdr_p1_parse_ep2] (p1) Parse a PC-to-radio datagram.
- [@ANCHOR: hpsdr:hpsdr_p1_samples_per_frame] (p1) Samples per receiver in one 512-byte frame: `504 / (6 n + 2)` (USB, "NOTE 1" padding table agrees).
- [@ANCHOR: hpsdr:hpsdr_p1_parse_ep6] (p1) Decode an EP6 datagram carrying `num_rx` receivers. An EP4 (bandscope) datagram is reported as
- [@ANCHOR: hpsdr:hpsdr_p1_encode_ep6] (p1) Build an EP6 datagram (used by the emulator). `rx[r]` holds the 24-bit (I, Q) integers of receiver
- [@ANCHOR: hpsdr:hpsdr_p1_apply_status_cc] (p1) Fold one radio-to-PC C&C quintuple into `st`. C0 bits 7..3 are the address; bits 2..0 are the
- [@ANCHOR: hpsdr:hpsdr_p1_encode_status_cc] (p1) Build the radio-to-PC C&C quintuple for status address `addr` (0..=4) from `st` (emulator side).
- [@ANCHOR: hpsdr:hpsdr_p2_ports_default] (p2) The default port map from the specification.
- [@ANCHOR: hpsdr:hpsdr_p2_put16] (p2) Store a big-endian 16-bit value.
- [@ANCHOR: hpsdr:hpsdr_p2_put32] (p2) Store a big-endian 32-bit value.
- [@ANCHOR: hpsdr:hpsdr_p2_discovery_packet] (p2) Discovery request: sequence 0, command 0x02, zeros (P2, "Discovery Packet"). Send to port 1024.
- [@ANCHOR: hpsdr:hpsdr_p2_parse_reply] (p2) Parse a discovery reply. Replies with board type 254/255 (XML or full description) are reported
- [@ANCHOR: hpsdr:hpsdr_p2_encode_reply] (p2) Build a discovery reply (emulator side). The board type byte is the P2 number of `board_type`.
- [@ANCHOR: hpsdr:hpsdr_p2_encode_general] (p2) The General packet (command 0): the port map, the hardware reset timer enabled (byte 38 bit 0, so
- [@ANCHOR: hpsdr:hpsdr_p2_encode_ddc_specific] (p2) The DDC Specific packet (to the DDC-specific port): number of ADCs, dither, random, per-DDC enable
- [@ANCHOR: hpsdr:hpsdr_p2_phase_word] (p2) `2^32 * f / clock`, rounded (P2, "General Packet" byte 37 text).
- [@ANCHOR: hpsdr:hpsdr_p2_hp_base] (p2) The receive part of a High Priority packet, shared by both encoders.
- [@ANCHOR: hpsdr:hpsdr_p2_encode_hp_rx] (p2) A receive-only High Priority packet: run bit and DDC frequencies and attenuators. PTT (byte 4 bit
- [@ANCHOR: hpsdr:hpsdr_p2_encode_hp_tx] (p2) A High Priority packet that may key the transmitter. Returns [`HpsdrError::GateClosed`] and builds
- [@ANCHOR: hpsdr:hpsdr_p2_encode_duc_iq] (p2) A DUC I/Q packet (to the DUC0 I/Q port): 240 samples of 24-bit I then Q, big-endian (P2, "DUC I&Q
- [@ANCHOR: hpsdr:hpsdr_p2_parse_hp_status] (p2) Parse a High Priority Status packet (60 bytes, from the radio's status port): PTT/dot/dash and PLL
- [@ANCHOR: hpsdr:hpsdr_p2_encode_hp_status] (p2) Build a High Priority Status packet (emulator side).
- [@ANCHOR: hpsdr:hpsdr_p2_parse_ddc] (p2) Parse a DDC packet: seq, 64-bit timestamp, bits per sample (must be 24), sample count, then I/Q.
- [@ANCHOR: hpsdr:hpsdr_p2_encode_ddc] (p2) Build a DDC packet from 24-bit integers (emulator side).
- [@ANCHOR: hpsdr:hpsdr_session_options_default] (session) 10 ms tick, 64 blocks per stream.
- [@ANCHOR: hpsdr:hpsdr_session_lock] (session) Lock a mutex, taking the data even if a panic poisoned it (the gate still decides on transmit).
- [@ANCHOR: hpsdr:hpsdr_session_next_datagram] (session) The next EP2 datagram. It is a transmit datagram only while the caller has asked to key AND the
- [@ANCHOR: hpsdr:hpsdr_session_handle_ep6] (session) Decode one received datagram, update status and push a block per receiver.
- [@ANCHOR: hpsdr:hpsdr_session_run] (session) The session task: send EP2 datagrams on a timer and decode whatever arrives.
- [@ANCHOR: hpsdr:hpsdr_session_start] (session) Configure the radio, send Start, and begin streaming. `radio` must be a Protocol 1 radio from
- [@ANCHOR: hpsdr:hpsdr_session_take_streams] (session) Hand over the per-receiver streams (index = DDC number). Returns an empty list the second time.
- [@ANCHOR: hpsdr:hpsdr_session_update] (session) Change settings (frequency, sample rate, attenuator, pre-amp...). The receiver count is fixed
- [@ANCHOR: hpsdr:hpsdr_session_status] (session) The latest hardware status (ADC overload, power counts, supply, firmware).
- [@ANCHOR: hpsdr:hpsdr_session_request_ptt] (session) Ask to key (or unkey) the transmitter with `drive` 0..=255. Refused with
- [@ANCHOR: hpsdr:hpsdr_session_send_tx_iq] (session) Queue transmit I/Q (16-bit pairs at 48 ksps). Refused while the gate is closed or the queue
- [@ANCHOR: hpsdr:hpsdr_session_stop] (session) Unkey, send Stop, and end the session task.
- [@ANCHOR: hpsdr:hpsdr_session_drop] (session) A session dropped without `stop()` unkeys at once (the key request and queued I/Q are cleared) and the task then sends Stop and ends; it is never just aborted, which would leave the last keyed datagram standing.
- [@ANCHOR: hpsdr:hpsdr_session_p2_config_default] (session_p2) One receiver at 48 ksps on 7.1 MHz, no attenuation, the specification's default ports.
- [@ANCHOR: hpsdr:hpsdr_session_p2_config_validate] (session_p2) Reject settings the protocol cannot express.
- [@ANCHOR: hpsdr:hpsdr_session_p2_config_ddcs] (session_p2) The DDC-specific settings: every receiver enabled on ADC0 at the common rate.
- [@ANCHOR: hpsdr:hpsdr_session_p2_lock] (session_p2) Lock a mutex, taking the data even if a panic poisoned it (the gate still decides on transmit).
- [@ANCHOR: hpsdr:hpsdr_session_p2_hp_rx] (session_p2) The receive High Priority settings from the configuration; `run` is the run bit.
- [@ANCHOR: hpsdr:hpsdr_session_p2_next_packets] (session_p2) The next High Priority packet and, only while keyed, a DUC I/Q packet. They are transmit packets
- [@ANCHOR: hpsdr:hpsdr_session_p2_handle_datagram] (session_p2) Decode one received datagram by the radio's source port: a DDC packet becomes a block on its
- [@ANCHOR: hpsdr:hpsdr_session_p2_run] (session_p2) The session task: send High Priority packets (and DDC-specific packets when the settings changed or
- [@ANCHOR: hpsdr:hpsdr_session_p2_start] (session_p2) Configure the radio (General, DDC-specific, then High Priority with the run bit) and begin
- [@ANCHOR: hpsdr:hpsdr_session_p2_take_streams] (session_p2) Hand over the per-receiver streams (index = DDC number). Returns an empty list the second time.
- [@ANCHOR: hpsdr:hpsdr_session_p2_update] (session_p2) Change settings (frequency, sample rate, attenuator...). The receiver count and the port map are
- [@ANCHOR: hpsdr:hpsdr_session_p2_status] (session_p2) The latest hardware status (ADC overload, power counts, supply).
- [@ANCHOR: hpsdr:hpsdr_session_p2_stray] (session_p2) Datagrams that arrived from a source port that belongs to no stream or status.
- [@ANCHOR: hpsdr:hpsdr_session_p2_request_ptt] (session_p2) Ask to key (or unkey) the transmitter with `drive` 0..=255. Refused with
- [@ANCHOR: hpsdr:hpsdr_session_p2_send_tx_iq] (session_p2) Queue transmit I/Q (16-bit pairs). Refused while the gate is closed or the queue would exceed
- [@ANCHOR: hpsdr:hpsdr_session_p2_stop] (session_p2) Unkey, send a High Priority packet with the run bit clear, and end the session task.
- [@ANCHOR: hpsdr:hpsdr_session_p2_drop] (session_p2) Dropped without `stop()`: unkey at once, then the task sends the run-bit-clear packet and ends; never just aborted.
- [@ANCHOR: hpsdr:hpsdr_types_board_from_p1_id] (types) Protocol 1 `Board_ID` (Metis v1.33: 0 Metis, 1 Hermes, 2 Griffin, 4 Angelia, 5 Orion, 6 Hermes Lite).
- [@ANCHOR: hpsdr:hpsdr_types_board_from_p2_type] (types) Protocol 2 board type (spec v4.4 Discovery Reply byte 11).
- [@ANCHOR: hpsdr:hpsdr_types_board_dsp_clock] (types) DSP clock in hertz, needed for phase words (spec v4.4 Appendix A gives 122.88 MHz for Hermes);
- [@ANCHOR: hpsdr:hpsdr_types_board_power_constant] (types) Constant 2 of the forward/reverse power formula (spec v4.4 Appendix A, Hermes: 0.09 for the
- [@ANCHOR: hpsdr:hpsdr_types_rate_hz] (types) The rate in hertz.
- [@ANCHOR: hpsdr:hpsdr_types_rate_from_ksps] (types) Parse a rate in kilohertz (48, 96, 192, 384, 768, 1536).
- [@ANCHOR: hpsdr:hpsdr_types_rate_p1_code] (types) The Protocol 1 speed code (USB protocol v1.60, C0=0 C1[1:0]: 00 48k, 01 96k, 10 192k, 11 384k).
- [@ANCHOR: hpsdr:hpsdr_types_rate_from_p1_code] (types) Inverse of [`Rate::p1_code`] (only the low two bits are used).
- [@ANCHOR: hpsdr:hpsdr_types_status_supply_volts] (types) Supply volts: `adc / 4095 * 3.3` (spec v4.4 Appendix A, Hermes).
- [@ANCHOR: hpsdr:hpsdr_types_status_watts] (types) Convert a raw power count to watts: `(adc / 4095 * 3.3)^2 / c2` (spec v4.4 Appendix A).
- [@ANCHOR: hpsdr:hpsdr_types_status_swr] (types) Standing wave ratio from forward and reverse power counts; `None` with no forward power.

# hpsdr

A Rust client for OpenHPSDR (the open High Performance Software Defined Radio protocol and hardware family) radios (ANAN-10, ANAN-100, Hermes, and the other boards of the family), built
for the relay SDR (software-defined radio) plan (`hams_com/docs/proposals/SDR_RECEIVE_ENGINE_AND_TRANSMIT_PATH.md` sections 2 to 4)
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

Anchors (ADR 0090): a function carries an anchor comment (the word ANCHOR and its id in square brackets after an at sign) in
source, its test carries the same tag after the word Tests, and this README cites it, with the crate name and a colon in front of the id, in the
heading of its entry below.

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

**Returns:** a `Vec<Discovered>`; replies that fail to parse are skipped, and each radio appears once (same MAC and address). **Errors:** `HpsdrError::Io` if the socket cannot be opened or a send or receive fails. **Special concerns:** async; it suspends for the whole `wait` time and holds no locks. It sends only discovery packets, which carry no PTT (push-to-talk), drive or I/Q, so the relay lock, band check and transmit gate do not apply.

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

**Returns:** an `EmulatorConfig`. **Errors:** none. **Special concerns:** pure; the values model the published documents, not a real radio (`verified=unverified`). Transmit: nothing in this entry can key a transmitter or produce RF (radio frequency); the emulator only counts what a transmit request would have been. The relay lock and band check do not apply to it. The crate itself contains no relay lock (unknown whether the relay holds one around the gate; to-do: record that in the relay docs once the relay consumes this crate).

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

Private function that folds a High Priority packet into the emulator's state and counters. It counts the packet, tracks run-bit transitions (`run_starts`, `run_stops`), records PTT (bit 1 of byte 4) in `ptt_now` and `ptt_frames`, counts non-zero drive (byte 345) and a non-zero transmit frequency (bytes 329 to 332), stores the ADC0 attenuator (byte 1443), and decodes each DDC frequency word from bytes 9 onward, converting phase words to hertz with the board's DSP (digital signal processing) clock when phase words are in use.

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

### `StreamCounters::delivered` [@ANCHOR: hpsdr:hpsdr_iq_counters_delivered]

Records that one block of receive I/Q reached the consumer: it adds one to the `blocks` counter and `n` to the `samples` counter. The session calls it after a block is successfully queued on the stream channel, so `blocks` and `samples` measure what the consumer could read, not what the radio sent.

| Argument | Type | Meaning |
|---|---|---|
| `n` | `usize` | Number of complex samples in the delivered block; widened to `u64`. |

**Returns:** nothing. **Errors:** none; the add wraps silently at 2^64, which no real stream reaches. **Special concerns:** `StreamCounters` is shared between the session task (producer) and the consumer through an `Arc`; its five fields are `AtomicU64`, updated with relaxed ordering, so there is no lock and a reader sees values that are eventually consistent. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn delivered(&self, n: usize)`. **See also:** `StreamCounters::snapshot`; test `counters_accumulate` (iq.rs).

### `StreamCounters::lost` [@ANCHOR: hpsdr:hpsdr_iq_counters_lost]

Adds datagrams that the radio sent but that never arrived to the `lost_packets` counter. The count comes from a sequence gap found by `SeqTracker::observe` and is also stored in `IqBlock::lost_before` of the next block, so the consumer sees both the running total and where the hole was.

| Argument | Type | Meaning |
|---|---|---|
| `n` | `u32` | Datagrams skipped before the one just received (0 is allowed and adds nothing). |

**Returns:** nothing. **Errors:** none. **Special concerns:** `StreamCounters` is shared between the session task (producer) and the consumer through an `Arc`; its five fields are `AtomicU64`, updated with relaxed ordering, so there is no lock and a reader sees values that are eventually consistent. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn lost(&self, n: u32)`. **See also:** `SeqTracker::observe`; test `counters_accumulate` (iq.rs).

### `StreamCounters::dropped` [@ANCHOR: hpsdr:hpsdr_iq_counters_dropped]

Records one block that was decoded but discarded because the consumer was too slow and the bounded channel was full. A rising `dropped_blocks` tells the relay that the receive path is losing data on the host side (in contrast to `lost_packets`, which is loss on the network). The session never blocks on a slow consumer: it uses `try_send`, drops and counts.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

**Returns:** nothing. **Errors:** none. **Special concerns:** `StreamCounters` is shared between the session task (producer) and the consumer through an `Arc`; its five fields are `AtomicU64`, updated with relaxed ordering, so there is no lock and a reader sees values that are eventually consistent. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn dropped(&self)`. **See also:** test `counters_accumulate` (iq.rs).

### `StreamCounters::malformed` [@ANCHOR: hpsdr:hpsdr_iq_counters_malformed]

Records one received datagram that the parser rejected (wrong magic, wrong length, unsupported end point and so on). The session calls it instead of failing, so a noisy or hostile network cannot stop reception; the counter makes the noise visible through `StreamStats::malformed`.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

**Returns:** nothing. **Errors:** none. **Special concerns:** `StreamCounters` is shared between the session task (producer) and the consumer through an `Arc`; its five fields are `AtomicU64`, updated with relaxed ordering, so there is no lock and a reader sees values that are eventually consistent. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn malformed(&self)`. **See also:** test `counters_accumulate` (iq.rs).

### `StreamCounters::snapshot` [@ANCHOR: hpsdr:hpsdr_iq_counters_snapshot]

Reads the five counters into a plain `StreamStats` value (`blocks`, `samples`, `lost_packets`, `dropped_blocks`, `malformed`), each a `u64`. `IqStream::stats` and `Radio::stats` return it, so the consumer never touches the atomics.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

**Returns:** a `StreamStats` (`Copy`, `PartialEq`) holding the instantaneous values. **Errors:** none. **Special concerns:** each field is loaded separately with relaxed ordering, so the five numbers are not one atomic snapshot: a block delivered between two loads can show up in `blocks` but not yet in `samples`. Fine for monitoring, not for exact accounting. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn snapshot(&self) -> StreamStats`. **See also:** test `counters_accumulate` (iq.rs).

### `SeqTracker::observe` [@ANCHOR: hpsdr:hpsdr_iq_seq_observe]

Accounts for one received datagram sequence number and reports how many datagrams were skipped before it. The tracker remembers the next expected number; the first call returns 0, a number equal to the expected one returns 0, a number ahead returns the gap, and a number that is behind by less than half the 32-bit range is treated as a restart of the radio's counter (0 lost) and re-synchronises. The arithmetic is modulo 2^32, so the wrap from `u32::MAX` to 0 is a gap of 0.

| Argument | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Sequence number of the datagram just received (Protocol 1 EP6 header, or the Protocol 2 DDC packet sequence). |

**Returns:** `u32`, datagrams lost immediately before this one. **Errors:** none. **Special concerns:** `&mut self`, so each stream owns its own tracker inside the session task; no locks, no I/O. A duplicate or reordered datagram counts as 0 lost and moves the expectation, so reordering is not reported as loss. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn observe(&mut self, seq: u32) -> u32`. **See also:** test `seq_gaps_and_wrap` (iq.rs).

### `ChannelStream::new` [@ANCHOR: hpsdr:hpsdr_iq_channel_new]

Creates the receive-side stream the sessions hand to the consumer: a bounded `tokio::sync::mpsc` channel of `IqBlock`s plus the shared counters. It returns the sender, which the session task keeps, and the `ChannelStream` that implements `IqStream`. When the channel holds `depth` blocks the session drops the next one and counts it with `StreamCounters::dropped`.

| Argument | Type | Meaning |
|---|---|---|
| `depth` | `usize` | Blocks buffered before drops; values below 1 are raised to 1 (a zero-capacity channel is not allowed by tokio). |
| `counters` | `Arc<StreamCounters>` | The counters the producer updates; retained by the stream so `stats()` reads them. |

**Returns:** `(mpsc::Sender<IqBlock>, ChannelStream)`. **Errors:** none. **Special concerns:** creates no task and does no I/O; the sender and stream may live on different tasks or threads. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn new(depth: usize, counters: Arc<StreamCounters>) -> (mpsc::Sender<IqBlock>, Self)`. **See also:** test `channel_stream_delivers_in_order` (iq.rs).

### `ChannelStream::next_block` [@ANCHOR: hpsdr:hpsdr_iq_channel_next_block]

The `IqStream` receive call: waits until the session has produced another block and returns it, in the order produced. Returns `None` after the sender side is dropped (the session stopped or the radio object was dropped) and the queue is empty, which is the consumer's signal that the stream ended.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut ChannelStream` | Borrowed mutably: one consumer per stream. |

**Returns:** a future yielding `Option<IqBlock>`; `Some` while blocks arrive, `None` at the end of the stream. **Errors:** none; there is no timeout, the caller wraps the future in `tokio::time::timeout` if it needs one. **Special concerns:** async and cancel-safe (it is `mpsc::Receiver::recv`): dropping the future loses no block. The future is `Send`. No locks held. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `fn next_block(&mut self) -> impl Future<Output = Option<IqBlock>> + Send`. **See also:** test `channel_stream_delivers_in_order` (iq.rs).

### `ChannelStream::stats` [@ANCHOR: hpsdr:hpsdr_iq_channel_stats]

Returns the counters for this one stream as a `StreamStats`, by taking a snapshot of the shared `StreamCounters`. Use it to see loss, drops and malformed datagrams for a receiver (DDC) without reading blocks.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&ChannelStream` | Shared borrow; safe while another task awaits `next_block` only if the borrow rules allow, normally called between reads. |

**Returns:** `StreamStats`. **Errors:** none. **Special concerns:** no locks, relaxed atomic loads (see `StreamCounters::snapshot`). Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `fn stats(&self) -> StreamStats`. **See also:** test `channel_stream_delivers_in_order` (iq.rs).

### `discovery_packet` [@ANCHOR: hpsdr:hpsdr_p1_discovery_packet]

Builds the Protocol 1 discovery request: the two magic bytes `EF FE`, the command byte `02`, then 60 zero bytes (Metis, "Metis Discover Packet format"), 63 bytes in all. `discover` sends it to every target; a Protocol 1 radio answers with the reply that `parse_reply` decodes. Send it to 255.255.255.255:1024 because the Metis notes say the subnet broadcast is not implemented.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

**Returns:** a `[u8; 63]`. **Errors:** none. **Special concerns:** pure function, no I/O or locks. It is only discovery: it carries no PTT, drive or I/Q, so the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn discovery_packet() -> [u8; 63]`. **See also:** test `discovery_start_stop_bytes` (p1.rs).

### `start_packet` [@ANCHOR: hpsdr:hpsdr_p1_start_packet]

Builds the Protocol 1 Start command: `EF FE 04`, one command byte, then 60 zeros (64 bytes). Bit 0 of the command byte starts I/Q plus microphone data (EP6), bit 1 starts the wide bandscope data (EP4); both clear is Stop (Metis, "Start Command"). The session sends it once after the first configuration datagrams.

| Argument | Type | Meaning |
|---|---|---|
| `iq` | `bool` | Start the I/Q and microphone stream (command bit 0). |
| `bandscope` | `bool` | Start the wide bandscope stream (command bit 1). |

**Returns:** a `[u8; 64]`. **Errors:** none. **Special concerns:** pure, no I/O or locks. Starting the data stream does not key the transmitter: PTT is the MOX bit of the EP2 C&C bytes, built only by the gated `Ep2Builder::build_tx`.

**Signature:** `pub fn start_packet(iq: bool, bandscope: bool) -> [u8; 64]`. **See also:** `stop_packet`; test `discovery_start_stop_bytes` (p1.rs).

### `stop_packet` [@ANCHOR: hpsdr:hpsdr_p1_stop_packet]

Builds the Protocol 1 Stop command: the Start layout with both stream bits clear (Metis, "Stop Command"), so `EF FE 04 00` followed by 60 zeros. `Radio::stop` and the drop path send it after one last receive datagram that has MOX clear, so the radio is unkeyed before it stops streaming.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

**Returns:** a `[u8; 64]`. **Errors:** none. **Special concerns:** pure, no I/O or locks. It cannot key a transmitter; the relay lock, band check and gate do not apply.

**Signature:** `pub fn stop_packet() -> [u8; 64]`. **See also:** `Radio::stop`; test `discovery_start_stop_bytes` (p1.rs).

### `parse_reply` [@ANCHOR: hpsdr:hpsdr_p1_parse_reply]

Parses a Protocol 1 discovery reply: `EF FE <status> <MAC 6 bytes> <code version> <board id>` followed by zeros. Status 2 means idle and 3 means the radio is already streaming (busy); the board id is mapped to `Board` with `Board::from_p1_id`. Called by `discovery::parse_reply` for any datagram that does not start with the Protocol 2 marker.

| Argument | Type | Meaning |
|---|---|---|
| `d` | `&[u8]` | The raw datagram; at least 11 bytes. |

**Returns:** a `P1Reply` (`running`, `mac`, `code_version`, `board_id`, `board`). **Errors:** `HpsdrError::Truncated` for fewer than 11 bytes; `HpsdrError::BadMagic` when the first two bytes are not `EF FE` or the status byte is not 2 or 3. An unknown board id is not an error (it becomes `Board::Other(id)`). **Special concerns:** pure, no I/O, never panics on any input (the property test feeds arbitrary bytes). Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn parse_reply(d: &[u8]) -> Result<P1Reply, HpsdrError>`. **See also:** test `reply_round_trips_and_rejects_junk` (p1.rs).

### `encode_reply` [@ANCHOR: hpsdr:hpsdr_p1_encode_reply]

Builds the 60-byte Protocol 1 discovery reply for a `P1Reply`: magic, status 3 when `running` else 2, MAC, code version and board id, the rest zero. The loopback emulator uses it to play a radio; production code only parses replies.

| Argument | Type | Meaning |
|---|---|---|
| `r` | `&P1Reply` | The identity to report: running flag, MAC, code version, board id (the `board` field is not encoded). |

**Returns:** a `[u8; 60]`. **Errors:** none. **Special concerns:** pure, no I/O or locks. Round-trips with `parse_reply` (the test checks it). Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn encode_reply(r: &P1Reply) -> [u8; 60]`. **See also:** test `reply_round_trips_and_rejects_junk` (p1.rs).

### `P1Config::default` [@ANCHOR: hpsdr:hpsdr_p1_config_default]

The default Protocol 1 settings: sample rate `Rate::K48`, one receiver, all seven receiver frequencies and the transmit frequency at 7,100,000 Hz, pre-amp, dither and random off, attenuator 0 dB and disabled. Callers start from it and change fields (`P1Config { rate: Rate::K96, ..Default::default() }`).

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

**Returns:** a `P1Config`; field meanings: `rate` the common DDC rate, `num_rx` 1..=7 receivers, `rx_hz[i]` NCO frequency of receiver i in hertz, `preamp`, `dither`, `random` ADC options, `att_db` 0..=31 and `att_enable`, `tx_hz` the transmit NCO frequency in hertz. **Errors:** none. **Special concerns:** pure. The default cannot transmit: the transmit frequency only matters once the gated builder is used. The default values are `verified=unverified` (no real radio). Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `impl Default for P1Config { fn default() -> Self }`. **See also:** test `config_validation_and_cc_round_trip` (p1.rs).

### `P1Config::validate` [@ANCHOR: hpsdr:hpsdr_p1_config_validate]

Checks that the settings can be expressed in the Protocol 1 C&C bytes before any datagram is built: the rate must have a Protocol 1 code (48, 96, 192 or 384 ksps; 768 and 1536 do not), the receiver count must be 1 to 7, and the attenuator must be 0 to 31 dB. Both builders and `Radio::start` and `Radio::update` call it, so a bad setting is refused up front rather than sent.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&P1Config` | The settings to check. |

**Returns:** `Ok(())`. **Errors:** `HpsdrError::Unsupported` for a rate without a Protocol 1 code; `HpsdrError::Config` naming the receiver count or attenuator value. **Special concerns:** pure, no I/O or locks. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn validate(&self) -> Result<(), HpsdrError>`. **See also:** test `config_validation_and_cc_round_trip` (p1.rs).

### `P1Config::addresses` [@ANCHOR: hpsdr:hpsdr_p1_config_addresses]

Lists the C&C addresses that the EP2 builder sends in round-robin order, two per datagram: general settings (0), one NCO address per receiver (2 for receiver 0, 3 for receiver 1, and so on), the transmit NCO (1), the drive level (9) and the attenuator (10). Every setting reaches the radio within a few datagrams; the radio keeps the last value of each.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&P1Config` | Only `num_rx` matters. |

**Returns:** a `Vec<u8>` of `num_rx + 4` addresses. **Errors:** none. **Special concerns:** pure. The list includes the transmit NCO and drive addresses, but `build_rx` fills them with MOX clear and drive 0. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn addresses(&self) -> Vec<u8>`. **See also:** test `config_validation_and_cc_round_trip` (p1.rs).

### `P1Config::encode_cc` [@ANCHOR: hpsdr:hpsdr_p1_config_encode_cc]

Builds the five C&C bytes (C0..C4) for one address. C0 is `(addr << 1) | mox`. Address 0 carries the rate code in C1, the pre-amp, dither and random flags in C3 bits 2 to 4 and `num_rx - 1` in C4 bits 3 to 5; address 1 the transmit frequency as a big-endian 32-bit number; addresses 2 to 8 the receiver frequency; address 9 the drive byte in C1; address 10 the attenuator (5 bits plus enable in C4 bit 5). Other addresses give zeros.

| Argument | Type | Meaning |
|---|---|---|
| `addr` | `u8` | C&C address 0..=10. |
| `mox` | `bool` | C0 bit 0, the transmit-on switch. |
| `drive` | `u8` | Drive level, used only for address 9. |

**Returns:** `[u8; 5]`. **Errors:** none (private; `validate` has already run). **Special concerns:** pure and private. The receive builder always passes `false` and 0; only `build_tx` after the gate check passes anything else. A rate without a code encodes as 0 here but cannot reach it because `validate` refuses it first. Transmit: this is where MOX and drive enter the datagram, which is why its only transmit caller is behind `TxGate`.

**Signature:** `fn encode_cc(&self, addr: u8, mox: bool, drive: u8) -> [u8; 5]`. **See also:** test `config_validation_and_cc_round_trip` (p1.rs).

### `P1Config::apply_cc` [@ANCHOR: hpsdr:hpsdr_p1_config_apply_cc]

The inverse of `encode_cc`, used by the emulator (which plays the radio) to fold one received C&C quintuple into its settings: rate, pre-amp, dither, random and receiver count from address 0, the transmit frequency from 1, receiver frequencies from 2 to 8, the attenuator from 10. Address 9 is not stored but returned so the emulator can count non-zero drive.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut P1Config` | The emulator's idea of the radio's settings, mutated. |
| `cc` | `&[u8; 5]` | One received C&C quintuple. |

**Returns:** `(bool, Option<u8>)`: the MOX bit of C0, and `Some(drive)` when the address was 9. **Errors:** none; unknown addresses change nothing. **Special concerns:** pure on its inputs. Only the emulator calls it; the counters it feeds (MOX frames, non-zero drive) are what the tests use to prove the transmit gate stays closed. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn apply_cc(&mut self, cc: &[u8; 5]) -> (bool, Option<u8>)`. **See also:** test `config_validation_and_cc_round_trip` (p1.rs).

### `Ep2Builder::new` [@ANCHOR: hpsdr:hpsdr_p1_ep2_new]

Creates a PC-to-radio datagram builder whose sequence number and C&C address cursor are both 0. One builder belongs to one session; the sequence number then increases by one per datagram, wrapping at 2^32, as the Metis format requires.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

**Returns:** an `Ep2Builder`. **Errors:** none. **Special concerns:** pure; the builder holds no socket and no lock and is not shared between threads. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn new() -> Self`. **See also:** test `receive_datagrams_never_carry_transmit_state` (p1.rs).

### `Ep2Builder::reset` [@ANCHOR: hpsdr:hpsdr_p1_ep2_reset]

Restarts the datagram sequence number and the C&C address cursor at zero. The Metis document requires the PC sequence to restart on every Start, so a session that restarts streaming on an existing builder must call it. (`Radio::start` today makes a fresh builder, whose sequence is already 0.)

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Ep2Builder` | The builder to rewind. |

**Returns:** nothing. **Errors:** none. **Special concerns:** pure state change, no I/O. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn reset(&mut self)`. **See also:** `Ep2Builder::build_rx`; test `transmit_builder_refuses_unless_gate_open` (p1.rs).

### `Ep2Builder::assemble` [@ANCHOR: hpsdr:hpsdr_p1_ep2_assemble]

The private common step that builds one 1032-byte EP2 datagram: header `EF FE 01 02`, the 32-bit sequence number, then two 512-byte frames. Each frame has the sync bytes `7F 7F 7F`, the C&C quintuple for the next address in the round-robin list, then 63 four-field samples (left and right audio, always zero here, then the 16-bit Q and I swapped as the USB specification requires). Samples missing from `iq` are zero. It advances the sequence number and cursor.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&P1Config` | Settings to encode (validated first). |
| `mox` | `bool` | MOX bit for every C0. |
| `drive` | `u8` | Drive byte for address 9. |
| `iq` | `&[(i16, i16)]` | Transmit I/Q pairs, up to 126 per datagram; shorter is zero padded. |

**Returns:** the datagram as `Vec<u8>` of `DATAGRAM_LEN` (1032) bytes. **Errors:** `HpsdrError` from `cfg.validate()`. **Special concerns:** `&mut self`, no I/O. It is private: its two callers are `build_rx` (MOX false, drive 0, no I/Q) and `build_tx` (only after the gate checks), so every transmit-capable datagram passes the gate. The I/Q swap is as documented but unverified on a radio.

**Signature:** `fn assemble(&mut self, cfg: &P1Config, mox: bool, drive: u8, iq: &[(i16, i16)]) -> Result<Vec<u8>, HpsdrError>`. **See also:** test `receive_datagrams_never_carry_transmit_state` (p1.rs).

### `Ep2Builder::build_rx` [@ANCHOR: hpsdr:hpsdr_p1_ep2_build_rx]

Builds a receive-only EP2 datagram: MOX clear, drive zero and transmit I/Q zero in every C&C address. The function has no transmit parameters at all, so no caller can key the radio through it. The session uses it for every tick in which the gate or the caller does not ask for transmit.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&P1Config` | The settings to send. |

**Returns:** the 1032-byte datagram. **Errors:** `HpsdrError::Unsupported` or `Config` when `cfg` fails `validate`. **Special concerns:** advances the builder's sequence number and cursor; no I/O or locks. Transmit: structurally unable to key (the transmit-fail-closed design); tests assert MOX clear, drive zero and zero I/Q for every address.

**Signature:** `pub fn build_rx(&mut self, cfg: &P1Config) -> Result<Vec<u8>, HpsdrError>`. **See also:** test `receive_datagrams_never_carry_transmit_state` (p1.rs).

### `Ep2Builder::build_tx` [@ANCHOR: hpsdr:hpsdr_p1_ep2_build_tx]

Builds a datagram that may key the transmitter. It first requires `gate.is_open(now)`, otherwise it returns `GateClosed` and builds nothing. When `req.ptt` is true it asks `gate.check_tx(cfg.tx_hz, req.drive)` for the permitted drive (capped by the gate limits, refused with `OutsideLimits` for a frequency outside the allowed bands) and then sets MOX with up to 126 transmit I/Q samples. With the gate open and `ptt` false it builds the receive datagram.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&P1Config` | Settings, including `tx_hz` which is checked against the gate limits. |
| `gate` | `&TxGate` | The transmit gate; must be open. |
| `now` | `Instant` | Clock reading used for the lease test. |
| `req` | `&TxRequest` | `ptt`, `drive` 0..=255 and `iq` samples. |

**Returns:** the datagram `Vec<u8>`. **Errors:** `GateClosed`, `OutsideLimits`, or a `validate` error. **Special concerns:** takes the gate's lease and limits locks briefly (a poisoned lock reads as closed); no I/O. Transmit: this is the only Protocol 1 path that sets MOX. The gate is the whole defence inside the crate; the relay lock, band check and its own transmit gate still sit above this crate (unknown whether the relay consumes it yet; to-do: record in the relay docs).

**Signature:** `pub fn build_tx(&mut self, cfg: &P1Config, gate: &TxGate, now: Instant, req: &TxRequest<'_>) -> Result<Vec<u8>, HpsdrError>`. **See also:** test `transmit_builder_refuses_unless_gate_open` (p1.rs).

### `parse_ep2` [@ANCHOR: hpsdr:hpsdr_p1_parse_ep2]

Parses a PC-to-radio EP2 datagram, which the emulator does to learn what the client asked for. It checks the length (1032), the header `EF FE 01 02`, and the sync bytes of both frames, then returns the sequence number, the two C&C quintuples and whether any transmit I/Q sample is non-zero (the I and Q bytes of each 8-byte group).

| Argument | Type | Meaning |
|---|---|---|
| `d` | `&[u8]` | The raw datagram. |

**Returns:** an `Ep2 { seq, cc: [[u8; 5]; 2], tx_iq_nonzero }`. **Errors:** `Truncated` when shorter than 1032 bytes; `BadMagic` for a wrong header or sync bytes. **Special concerns:** pure, never panics on any input (property test). Transmit: the emulator uses `tx_iq_nonzero` and the MOX bit to count attempts, which is how the tests prove nothing is sent while the gate is closed.

**Signature:** `pub fn parse_ep2(d: &[u8]) -> Result<Ep2, HpsdrError>`. **See also:** test `receive_datagrams_never_carry_transmit_state` (p1.rs).

### `samples_per_frame` [@ANCHOR: hpsdr:hpsdr_p1_samples_per_frame]

Computes how many samples per receiver fit in one 512-byte EP6 frame: the 504 data bytes divided by the size of one sample group, `6 * num_rx + 2` (three bytes I and three bytes Q per receiver, plus two microphone bytes), rounded down. The USB specification's padding table agrees (1 receiver 63 samples, 2 receivers 36, and so on).

| Argument | Type | Meaning |
|---|---|---|
| `num_rx` | `u8` | Receivers in the stream; 1..=8 is meaningful, 0 gives 252. |

**Returns:** `usize` samples per receiver per frame; an EP6 datagram holds twice that. **Errors:** none. **Special concerns:** pure; used by `parse_ep6`, `encode_ep6` and the session. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn samples_per_frame(num_rx: u8) -> usize`. **See also:** test `ep6_round_trips_for_one_to_seven_receivers` (p1.rs).

### `parse_ep6` [@ANCHOR: hpsdr:hpsdr_p1_parse_ep6]

Decodes a radio-to-PC EP6 datagram carrying `num_rx` receivers: the sequence number, the two status C&C quintuples, for each receiver the 24-bit signed I and Q samples scaled to +/-1.0 (divided by 2^23), and the 16-bit microphone samples. An EP4 (bandscope) datagram is reported as unsupported.

| Argument | Type | Meaning |
|---|---|---|
| `d` | `&[u8]` | The raw datagram, at least 1032 bytes. |
| `num_rx` | `u8` | Receivers the radio was configured for, 1..=8 (the layout depends on it). |

**Returns:** an `Ep6 { seq, cc, rx: Vec<Vec<Complex>>, mic: Vec<i16> }`. **Errors:** `Config` for `num_rx` 0 or above 8; `Truncated`; `BadMagic` for a wrong header or sync bytes; `Unsupported` for any end point other than 6. **Special concerns:** pure, never panics on any input (property tests, also with well-formed headers and any body). The session counts a failure as a malformed datagram and carries on. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn parse_ep6(d: &[u8], num_rx: u8) -> Result<Ep6, HpsdrError>`. **See also:** tests `ep6_round_trips_for_one_to_seven_receivers` (p1.rs), `hpsdr_packet_parsers_survive_any_datagram` (parser_props.rs).

### `encode_ep6` [@ANCHOR: hpsdr:hpsdr_p1_encode_ep6]

Builds an EP6 datagram for the emulator: header `EF FE 01 06`, the sequence number, and two frames of sync, C&C and the sample groups (24-bit I and Q per receiver, then 16-bit microphone), zero padded to 512 bytes per frame. Each receiver's vector must hold exactly `2 * samples_per_frame(n)` pairs and the microphone vector the same count.

| Argument | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Datagram sequence number. |
| `cc` | `&[[u8; 5]; 2]` | Status C&C quintuples for the two frames. |
| `rx` | `&[Vec<(i32, i32)>]` | Per-receiver 24-bit (I, Q) integers; the low 24 bits are used. |
| `mic` | `&[i16]` | Microphone samples. |

**Returns:** the 1032-byte datagram. **Errors:** `Config` when the receiver count does not fit a `u8` or is 0, or a sample count is wrong. **Special concerns:** pure; only the emulator calls it. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn encode_ep6(seq: u32, cc: &[[u8; 5]; 2], rx: &[Vec<(i32, i32)>], mic: &[i16]) -> Result<Vec<u8>, HpsdrError>`. **See also:** test `ep6_round_trips_for_one_to_seven_receivers` (p1.rs).

### `apply_status_cc` [@ANCHOR: hpsdr:hpsdr_p1_apply_status_cc]

Folds one radio-to-PC status quintuple into a `HardwareStatus`. C0 bits 2..0 are the PTT, DASH and DOT inputs; C0 bits 7..3 are the address: 0 gives ADC0 overload (C1 bit 0) and the firmware version (C4), 1 the exciter and forward power counts (12 bits each), 2 the reverse power count, 3 the supply count, 4 the overload flags of the four ADCs. Counts are masked to 12 bits. Temperature is not defined by the published protocol, so none is stored.

| Argument | Type | Meaning |
|---|---|---|
| `st` | `&mut HardwareStatus` | Status record updated in place. |
| `cc` | `&[u8; 5]` | One received C&C quintuple. |

**Returns:** nothing. **Errors:** none; unknown addresses update only the three input bits. **Special concerns:** pure on its inputs; the session's `handle_ep6` calls it for both C&C quintuples of every EP6 datagram while holding the status mutex. The values are raw ADC counts; `HardwareStatus::supply_volts`, `forward_watts` and `swr` convert them. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn apply_status_cc(st: &mut HardwareStatus, cc: &[u8; 5])`. **See also:** test `status_cc_round_trips_overload_power_and_supply` (p1.rs).

### `encode_status_cc` [@ANCHOR: hpsdr:hpsdr_p1_encode_status_cc]

Builds the radio-to-PC status quintuple for status address 0 to 4 from a `HardwareStatus`, the exact inverse of `apply_status_cc`. The emulator cycles through the addresses so a client sees overload, power, supply and firmware values.

| Argument | Type | Meaning |
|---|---|---|
| `st` | `&HardwareStatus` | The status to report. |
| `addr` | `u8` | Status address 0..=4; others give a quintuple with only the C0 input bits. |

**Returns:** `[u8; 5]`. **Errors:** none. **Special concerns:** pure, emulator side only. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn encode_status_cc(st: &HardwareStatus, addr: u8) -> [u8; 5]`. **See also:** test `status_cc_round_trips_overload_power_and_supply` (p1.rs).

### `RadioPorts::default` [@ANCHOR: hpsdr:hpsdr_p2_ports_default]

Returns the Protocol 2 UDP port map from the specification, used when the caller does not choose ports: DDC-specific 1025, DUC-specific 1026, High Priority from the PC 1027, High Priority status from the radio 1025, DDC audio 1028, DUC0 I/Q 1029, DDC0 I/Q source port 1035 (DDC n uses 1035 + n), microphone 1026 and wideband ADC0 1027. The General packet announces these ports to the radio; `session_p2` demultiplexes the incoming streams by them.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

**Returns:** a `RadioPorts` with the nine `u16` fields above. **Errors:** none. **Special concerns:** pure. The port map is `verified=unverified` on real hardware (see the open points at the top of this file). Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `impl Default for RadioPorts { fn default() -> Self }`. **See also:** `encode_general`; test `general_packet_carries_ports_and_the_hardware_timer` (p2.rs).

### `put16` [@ANCHOR: hpsdr:hpsdr_p2_put16]

Private helper that stores a 16-bit value big-endian into a packet buffer at a byte offset. Every Protocol 2 encoder uses it so that the byte order (big-endian, as revision 1.3 of the specification fixed it) is stated in one place.

| Argument | Type | Meaning |
|---|---|---|
| `p` | `&mut [u8]` | Packet buffer; the two bytes at `at` must exist. |
| `at` | `usize` | Byte offset of the first of the two bytes. |
| `v` | `u16` | Value to store. |

**Returns:** nothing. **Errors:** panics if `at + 2` is past the end of `p`; every caller passes constants inside a buffer of 60 or 1444 bytes. **Special concerns:** pure on its argument, no I/O or locks. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `fn put16(p: &mut [u8], at: usize, v: u16)`. **See also:** `put32`, `encode_general`; test `receive_high_priority_never_carries_transmit_state` (p2.rs).

### `put32` [@ANCHOR: hpsdr:hpsdr_p2_put32]

Private helper that stores a 32-bit value big-endian into a packet buffer at a byte offset: used for the packet sequence number at byte 0, frequencies or phase words, and similar.

| Argument | Type | Meaning |
|---|---|---|
| `p` | `&mut [u8]` | Packet buffer; four bytes at `at` must exist. |
| `at` | `usize` | Byte offset. |
| `v` | `u32` | Value to store. |

**Returns:** nothing. **Errors:** panics if `at + 4` is past the end of `p`; callers pass constants inside fixed-size buffers. **Special concerns:** pure, no I/O or locks. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `fn put32(p: &mut [u8], at: usize, v: u32)`. **See also:** `put16`, `encode_high_priority_rx`; test `receive_high_priority_never_carries_transmit_state` (p2.rs).

### `discovery_packet` [@ANCHOR: hpsdr:hpsdr_p2_discovery_packet]

Builds the Protocol 2 discovery request: 60 bytes, sequence 0, command byte 2 at offset 4, the rest zero (P2, "Discovery Packet"). `discover` sends it to port 1024 of each target next to the Protocol 1 request; a Protocol 2 radio replies and a Protocol 1 radio ignores it.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

**Returns:** a `[u8; 60]`. **Errors:** none. **Special concerns:** pure, no I/O or locks. Discovery carries no PTT, drive or I/Q. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn discovery_packet() -> [u8; SMALL_LEN]`. **See also:** test `discovery_and_reply_round_trip` (p2.rs).

### `parse_reply` [@ANCHOR: hpsdr:hpsdr_p2_parse_reply]

Parses a Protocol 2 discovery reply (option 1, board type below 254): bytes 0..3 zero, byte 4 the status (2 idle, 3 already running), bytes 5..10 the MAC, 11 the board type, 12 the protocol version, 13 the firmware code version, 20 the number of DDCs, 21 the phase-word flag and 22 the data-format bits. Replies with board type 254 or 255 (XML or full hardware description) are reported as unsupported rather than guessed at.

| Argument | Type | Meaning |
|---|---|---|
| `d` | `&[u8]` | The raw datagram, at least 24 bytes. |

**Returns:** a `P2Reply { running, mac, board, protocol_version, firmware, num_ddcs, phase_word, data_modes }`. **Errors:** `Truncated` for fewer than 24 bytes; `BadMagic` when the first four bytes are not zero or the status is not 2 or 3; `Unsupported` for board type 254 or 255. **Special concerns:** pure, never panics on any input (the property test feeds arbitrary bytes). Called by `discovery::parse_reply` for datagrams that begin with the Protocol 2 marker. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn parse_reply(d: &[u8]) -> Result<P2Reply, HpsdrError>`. **See also:** test `discovery_and_reply_round_trip` (p2.rs).

### `encode_reply` [@ANCHOR: hpsdr:hpsdr_p2_encode_reply]

Builds the 60-byte Protocol 2 discovery reply for the emulator: status 3 when `running` else 2, MAC, the board type byte, protocol version, firmware, DDC count, phase-word flag and data-format bits; all other bytes zero. It is the inverse of `parse_reply` for board types below 254.

| Argument | Type | Meaning |
|---|---|---|
| `r` | `&P2Reply` | Identity to report. |
| `board_type` | `u8` | The Protocol 2 board number to put in byte 11 (the `board` field of `r` is not used, because `Board` does not keep the number). |

**Returns:** a `[u8; 60]`. **Errors:** none. **Special concerns:** pure, emulator side only. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn encode_reply(r: &P2Reply, board_type: u8) -> [u8; SMALL_LEN]`. **See also:** test `discovery_and_reply_round_trip` (p2.rs).

### `encode_general` [@ANCHOR: hpsdr:hpsdr_p2_encode_general]

Builds the General packet (command 0) that starts a Protocol 2 session: the sequence number, the nine ports at bytes 5 to 21, the phase-word flag in byte 37 bit 3, byte 38 = 1 (hardware reset timer enabled, so a lost host puts the radio into standby) and byte 39 = 0 (big-endian 3-byte I/Q). The PA enable (byte 58) and Alex enables (byte 59) are left zero. Send it to port 1024 after discovery.

| Argument | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Packet sequence number. |
| `ports` | `&RadioPorts` | The port map to announce. |
| `phase_words` | `bool` | True when the radio's discovery reply said it takes phase words instead of hertz. |

**Returns:** a `[u8; 60]`. **Errors:** none. **Special concerns:** pure, no I/O. It enables nothing that transmits (PA and Alex enables stay zero), so it is not a transmit packet. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn encode_general(seq: u32, ports: &RadioPorts, phase_words: bool) -> [u8; SMALL_LEN]`. **See also:** `session_p2::Radio2::start`; test `general_packet_carries_ports_and_the_hardware_timer` (p2.rs).

### `encode_ddc_specific` [@ANCHOR: hpsdr:hpsdr_p2_encode_ddc_specific]

Builds the 1444-byte DDC Specific packet that configures the receivers: the sequence number, the number of ADCs, dither and random flags (bytes 4 to 6), one enable bit per DDC from byte 7 (bit n % 8 of byte 7 + n / 8), and per DDC from byte 17 a 6-byte group: ADC index, rate in ksps as 16 bits, two reserved bytes (zero) and the sample size 24. Synchronous-DDC bytes stay zero, so every DDC is independent.

| Argument | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Packet sequence number. |
| `num_adc` | `u8` | ADCs fitted, 1..=8. |
| `dither` | `u8` | Dither bit mask, one bit per ADC. |
| `random` | `u8` | Random bit mask, one bit per ADC. |
| `ddcs` | `&[DdcSetting]` | Up to 80 settings: `enabled`, `adc` and `rate`. |

**Returns:** the packet as a `Vec<u8>` of 1444 bytes. **Errors:** `Config` when there are more than 80 DDCs or `num_adc` is 0 or above 8. **Special concerns:** pure, no I/O. Receive configuration only; it cannot key the transmitter. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn encode_ddc_specific(seq: u32, num_adc: u8, dither: u8, random: u8, ddcs: &[DdcSetting]) -> Result<Vec<u8>, HpsdrError>`. **See also:** `DdcSetting`; test `ddc_specific_sets_enables_rates_and_adcs` (p2.rs).

### `phase_word` [@ANCHOR: hpsdr:hpsdr_p2_phase_word]

Converts a frequency to the 32-bit phase word that a radio which announces the phase-word flag expects instead of hertz: `2^32 * f / clock`, rounded to the nearest integer (P2, "General Packet" byte 37 text).

| Argument | Type | Meaning |
|---|---|---|
| `freq_hz` | `u32` | Frequency in hertz. |
| `clock_hz` | `f64` | The radio's DSP clock in hertz, from `Board::dsp_clock_hz` (122.88 MHz for Hermes). |

**Returns:** the phase word as `u32`; the cast saturates, so a frequency at or above the clock gives `u32::MAX` rather than wrapping. **Errors:** none. **Special concerns:** pure arithmetic in `f64`; no I/O or locks. Used for the DDC frequencies and, in a transmit packet, the DUC frequency. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn phase_word(freq_hz: u32, clock_hz: f64) -> u32`. **See also:** `Board::dsp_clock_hz`; test `receive_high_priority_never_carries_transmit_state` (p2.rs).

### `hp_base` [@ANCHOR: hpsdr:hpsdr_p2_hp_base]

Builds the receive part of a High Priority packet shared by both encoders: 1444 bytes with the sequence number, the run bit (byte 4 bit 0), the DDC frequencies from byte 9 (four bytes each, hertz or phase words depending on `HpRx::phase_clock_hz`) and the two step attenuators at bytes 1442 (ADC1) and 1443 (ADC0). PTT, the DUC frequency and the drive level are left zero.

| Argument | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Packet sequence number. |
| `hp` | `&HpRx` | `run`, `ddc_hz` (up to 80), `atten_adc0`, `atten_adc1` (each 0..=31 dB) and `phase_clock_hz`. |

**Returns:** the packet `Vec<u8>`. **Errors:** `Config` for more than 80 DDC frequencies or an attenuator above 31. **Special concerns:** private and pure. It is the zero-transmit base: `encode_high_priority_tx` adds PTT, the DUC frequency and the drive on top of it, after the gate check. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `fn hp_base(seq: u32, hp: &HpRx) -> Result<Vec<u8>, HpsdrError>`. **See also:** `encode_high_priority_rx`, `encode_high_priority_tx`; test `receive_high_priority_never_carries_transmit_state` (p2.rs).

### `encode_high_priority_rx` [@ANCHOR: hpsdr:hpsdr_p2_encode_hp_rx]

Builds a receive-only High Priority packet: the run bit, DDC frequencies and attenuators. PTT (byte 4 bit 1), the DUC frequency and the drive level are structurally zero because the function has no transmit parameters. The session sends it on every tick when the caller has not asked to key.

| Argument | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Packet sequence number. |
| `hp` | `&HpRx` | The receive settings. |

**Returns:** the 1444-byte packet. **Errors:** `Config` from `hp_base` for out-of-range settings. **Special concerns:** pure, no I/O. Transmit: structurally unable to key; tests assert that PTT, DUC frequency and drive stay zero. The relay lock and band check do not apply.

**Signature:** `pub fn encode_high_priority_rx(seq: u32, hp: &HpRx) -> Result<Vec<u8>, HpsdrError>`. **See also:** `encode_high_priority_tx`; test `receive_high_priority_never_carries_transmit_state` (p2.rs).

### `encode_high_priority_tx` [@ANCHOR: hpsdr:hpsdr_p2_encode_hp_tx]

Builds a High Priority packet that may key the transmitter. It returns `GateClosed` and builds nothing unless `gate.is_open(now)`. With the gate open and `tx.ptt` false it equals the receive packet. With `ptt` true it asks `gate.check_tx(tx.duc_hz, tx.drive)` for the permitted drive (capped by the gate limits, `OutsideLimits` outside the allowed bands), then sets PTT (byte 4 bit 1), writes the DUC0 frequency at byte 329 (hertz or phase word) and the drive at byte 345.

| Argument | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Packet sequence number. |
| `hp` | `&HpRx` | Receive settings. |
| `gate` | `&TxGate` | Must be open. |
| `now` | `Instant` | Clock reading for the lease test. |
| `tx` | `&TxState` | `ptt`, `duc_hz` and `drive` 0..=255. |

**Returns:** the packet `Vec<u8>`. **Errors:** `GateClosed`, `OutsideLimits`, or `Config` from `hp_base`. **Special concerns:** takes the gate's locks briefly (a poisoned lock reads as closed); no I/O. Transmit: the only Protocol 2 path that sets PTT or drive, so the gate decides every keyed packet. The relay lock, band check and transmit gate above this crate are not part of it (to-do: record the relay side once the relay consumes this crate).

**Signature:** `pub fn encode_high_priority_tx(seq: u32, hp: &HpRx, gate: &TxGate, now: Instant, tx: &TxState) -> Result<Vec<u8>, HpsdrError>`. **See also:** `TxGate::check_tx`; test `transmit_packets_are_refused_while_the_gate_is_closed` (p2.rs).

### `encode_duc_iq` [@ANCHOR: hpsdr:hpsdr_p2_encode_duc_iq]

Builds a DUC I/Q packet for the DUC0 I/Q port: the sequence number and up to 240 samples, each 24-bit signed I then Q, big-endian (P2, "DUC I&Q Data Packet"). A short slice is zero padded (silence); a sample is never repeated. Samples are the low 24 bits of the `i32` values.

| Argument | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Packet sequence number. |
| `iq` | `&[(i32, i32)]` | Up to 240 (I, Q) pairs. |
| `gate` | `&TxGate` | Must be open. |
| `now` | `Instant` | Clock reading for the lease test. |

**Returns:** the 1444-byte packet. **Errors:** `GateClosed` unless the gate is open at `now`; `Config` for more than 240 samples. **Special concerns:** takes the gate's lease lock briefly; no I/O. Transmit: transmit I/Q is refused with a closed gate, so nothing reaches the radio's modulator without a lease. The caller still has to key PTT through `encode_high_priority_tx`.

**Signature:** `pub fn encode_duc_iq(seq: u32, iq: &[(i32, i32)], gate: &TxGate, now: Instant) -> Result<Vec<u8>, HpsdrError>`. **See also:** `DUC_SAMPLES`; test `transmit_packets_are_refused_while_the_gate_is_closed` (p2.rs).

### `parse_hp_status` [@ANCHOR: hpsdr:hpsdr_p2_parse_hp_status]

Parses a 60-byte High Priority Status packet from the radio's status port: the PTT, dot and dash inputs and PLL lock in byte 4, the ADC overload bits in byte 5, the exciter, forward and reverse power counts (bytes 6, 14, 22), the two ADC peak magnitudes (bytes 39 and 41, 15 bits), and the supply count (byte 49). Temperature is not defined by the published protocol and is not reported.

| Argument | Type | Meaning |
|---|---|---|
| `d` | `&[u8]` | The raw datagram, at least 60 bytes. |

**Returns:** `(u32, HardwareStatus)`: the packet sequence number and the status record (counts are raw ADC values; `pll_locked` is `Some`). **Errors:** `Truncated` for fewer than 60 bytes. **Special concerns:** pure, never panics on any input (property test). Called by the session under its status handling. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn parse_hp_status(d: &[u8]) -> Result<(u32, HardwareStatus), HpsdrError>`. **See also:** `encode_hp_status`; test `status_and_ddc_packets_round_trip` (p2.rs).

### `encode_hp_status` [@ANCHOR: hpsdr:hpsdr_p2_encode_hp_status]

Builds the 60-byte High Priority Status packet for the emulator from a `HardwareStatus`: the inverse of `parse_hp_status`. The emulator sends it so tests can check that overload, power and supply values arrive.

| Argument | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Packet sequence number. |
| `st` | `&HardwareStatus` | The status to report. |

**Returns:** a `[u8; 60]`. **Errors:** none. **Special concerns:** pure, emulator side only. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn encode_hp_status(seq: u32, st: &HardwareStatus) -> [u8; SMALL_LEN]`. **See also:** `parse_hp_status`; test `status_and_ddc_packets_round_trip` (p2.rs).

### `parse_ddc_packet` [@ANCHOR: hpsdr:hpsdr_p2_parse_ddc]

Parses a DDC data packet: the sequence number, a 64-bit timestamp, the bits per sample (only 24 is supported), the sample count, then that many 24-bit I and Q pairs scaled to +/-1.0. The packet does not carry its DDC number; the session identifies the stream by the source port of the datagram.

| Argument | Type | Meaning |
|---|---|---|
| `d` | `&[u8]` | The raw datagram, at least 16 bytes plus 6 per sample. |

**Returns:** a `DdcPacket { seq, timestamp, samples }`. **Errors:** `Truncated` when shorter than the header or than the announced sample count; `Unsupported` when bits per sample is not 24. **Special concerns:** pure, never panics on any input (property tests, also with well-formed headers and any body). Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn parse_ddc_packet(d: &[u8]) -> Result<DdcPacket, HpsdrError>`. **See also:** `session_p2::Radio2`; tests `status_and_ddc_packets_round_trip` (p2.rs), `hpsdr_packet_parsers_survive_any_datagram` (parser_props.rs).

### `encode_ddc_packet` [@ANCHOR: hpsdr:hpsdr_p2_encode_ddc]

Builds a DDC packet from 24-bit integers for the emulator: sequence number, timestamp, 24 bits per sample, sample count and the I and Q bytes. It is the inverse of `parse_ddc_packet`.

| Argument | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Packet sequence number. |
| `timestamp` | `u64` | Timestamp field. |
| `iq` | `&[(i32, i32)]` | The (I, Q) pairs; only the low 24 bits are written and the count must fit a `u16`. |

**Returns:** the packet `Vec<u8>` of `16 + 6 * n` bytes. **Errors:** none; a count above 65535 would be truncated by the cast (the emulator sends small packets). **Special concerns:** pure, emulator side only. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn encode_ddc_packet(seq: u32, timestamp: u64, iq: &[(i32, i32)]) -> Vec<u8>`. **See also:** `parse_ddc_packet`; test `status_and_ddc_packets_round_trip` (p2.rs).

### `SessionOptions::default` [@ANCHOR: hpsdr:hpsdr_session_options_default]

The default tunables of a session: a 10 ms tick between datagrams and 64 blocks buffered per receiver stream. The tick paces the EP2 datagrams (the C&C refresh and the transmit I/Q); the stream depth is the `depth` given to `ChannelStream::new`. Both values are unverified against a real radio, which may want datagrams paced to the 48 ksps transmit sample clock (126 samples, 2.6 ms).

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

**Returns:** a `SessionOptions { tick: Duration, stream_depth: usize }`. **Errors:** none. **Special concerns:** pure. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `impl Default for SessionOptions { fn default() -> Self }`. **See also:** `Radio::start`, `Radio2::start` (both take the options); test `two_receivers_stream_their_own_tones_without_loss` (session.rs).

### `lock (Protocol 1 session)` [@ANCHOR: hpsdr:hpsdr_session_lock]

Private helper that locks one of the session's mutexes and, if a panic in another thread poisoned it, takes the data anyway instead of panicking. The transmit gate, not these mutexes, decides whether anything is sent, so reading the data of a poisoned mutex does not weaken the transmit safety.

| Argument | Type | Meaning |
|---|---|---|
| `m` | `&Mutex<T>` | The mutex to lock. |

**Returns:** a `MutexGuard<T>`. **Errors:** none (poison is recovered). **Special concerns:** blocks until the mutex is free; callers hold the guard briefly and never across an `await`. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T>`. **See also:** `next_datagram`, `Radio::status`; test `two_receivers_stream_their_own_tones_without_loss` (session.rs).

### `next_datagram` [@ANCHOR: hpsdr:hpsdr_session_next_datagram]

Chooses and builds the next EP2 datagram for the session task. It is a transmit datagram only while the caller has asked to key (`tx.ptt`) and the gate is open at `now`: it then takes up to 126 queued I/Q samples and calls `Ep2Builder::build_tx`. In every other case, including a refusal by the gate or its limits while keyed, a stale key request and the I/Q queue are cleared and a receive datagram is built, so the radio is never left keyed.

| Argument | Type | Meaning |
|---|---|---|
| `shared` | `&Shared` | Session state: configuration, status and transmit request (each mutex). |
| `gate` | `&TxGate` | The transmit gate. |
| `b` | `&mut Ep2Builder` | The datagram builder (sequence and cursor). |
| `now` | `Instant` | Clock reading for the lease test. |

**Returns:** `Option<Vec<u8>>`: the datagram, or `None` only if even the receive datagram failed to build. **Errors:** none (a build failure becomes `None`). **Special concerns:** private, called by `run` each tick. The shared state sits behind `std::sync::Mutex`es (`cfg`, `status`, `tx`) taken through `lock`, which recovers a poisoned mutex; locks are held briefly and never across an await. Transmit: this is a keying path. The gate must be open (a bounded lease) and its limits (drive cap, allowed bands) are checked; the crate has no relay lock or band plan of its own, so the relay's lock and band check have to be applied by the caller (unknown whether the relay consumes this crate yet; to-do: record that in the relay docs).

**Signature:** `fn next_datagram(shared: &Shared, gate: &TxGate, b: &mut Ep2Builder, now: Instant) -> Option<Vec<u8>>`. **See also:** test `two_receivers_stream_their_own_tones_without_loss` (session.rs).

### `handle_ep6` [@ANCHOR: hpsdr:hpsdr_session_handle_ep6]

Handles one datagram received from the radio: decodes it with `parse_ep6` for the configured receiver count, applies both status C&C quintuples to the shared `HardwareStatus`, and for each receiver finds the sequence gap with its `SeqTracker`, records the loss and pushes an `IqBlock` (sample rate and centre frequency from the current settings) with `try_send`. A datagram that does not parse is counted as malformed on every stream.

| Argument | Type | Meaning |
|---|---|---|
| `d` | `&[u8]` | The received datagram. |
| `shared` | `&Shared` | Session state (configuration and status). |
| `senders` | `&[mpsc::Sender<IqBlock>]` | One sender per receiver stream. |
| `counters` | `&[Arc<StreamCounters>]` | One counter set per stream. |
| `trackers` | `&mut [SeqTracker]` | One tracker per stream, mutated. |

**Returns:** nothing. **Errors:** none; every failure is a counter (`malformed`, or `dropped` when the channel is full). **Special concerns:** synchronous and non-blocking (`try_send`), called from the session task. Takes the `cfg` mutex to copy the settings and the `status` mutex while updating it, never both at once. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `fn handle_ep6(d: &[u8], shared: &Shared, senders: &[mpsc::Sender<IqBlock>], counters: &[Arc<StreamCounters>], trackers: &mut [SeqTracker])`. **See also:** test `two_receivers_stream_their_own_tones_without_loss` (session.rs).

### `run (Protocol 1 session)` [@ANCHOR: hpsdr:hpsdr_session_run]

The session task. In a `tokio::select!` loop it does two things: on every tick of a timer (`MissedTickBehavior::Delay`) it builds the next EP2 datagram with `next_datagram` and sends it on the connected socket, and for every datagram that arrives it calls `handle_ep6`. When the `closing` flag is set (the `Radio` was dropped) it sends the Stop packet after the datagram of that tick, which is a receive datagram, and ends.

| Argument | Type | Meaning |
|---|---|---|
| `sock` | `Arc<UdpSocket>` | Socket connected to the radio. |
| `shared` | `Arc<Shared>` | Session state shared with the `Radio`. |
| `gate` | `TxGate` | The transmit gate (a clone). |
| `senders` | `Vec<mpsc::Sender<IqBlock>>` | Stream senders. |
| `counters` | `Vec<Arc<StreamCounters>>` | Stream counters. |
| `tick` | `Duration` | Datagram interval. |

**Returns:** a future that completes only after `closing` is set; otherwise it runs until the task is aborted by `Radio::stop`. **Errors:** send and receive errors are ignored (UDP is lossy by design); the effect is visible as lost-packet counters. **Special concerns:** async, one task per session, spawned on the caller's runtime. Single owner of the `Ep2Builder` and the trackers, so they need no lock. A transmit datagram can only come from `next_datagram`, so the gate is checked on every datagram. Transmit: this is a keying path. The gate must be open (a bounded lease) and its limits (drive cap, allowed bands) are checked; the crate has no relay lock or band plan of its own, so the relay's lock and band check have to be applied by the caller (unknown whether the relay consumes this crate yet; to-do: record that in the relay docs).

**Signature:** `async fn run(sock: Arc<UdpSocket>, shared: Arc<Shared>, gate: TxGate, senders: Vec<mpsc::Sender<IqBlock>>, counters: Vec<Arc<StreamCounters>>, tick: Duration)`. **See also:** test `two_receivers_stream_their_own_tones_without_loss` (session.rs).

### `Radio::start` [@ANCHOR: hpsdr:hpsdr_session_start]

Configures a Protocol 1 radio and begins streaming. It validates the configuration, binds an ephemeral UDP socket and connects it to the radio, sends enough receive datagrams to deliver every C&C address once (`addresses().len() / 2 + 1`, rounded up), sends the Start command (I/Q on, bandscope off), builds the shared state and one `ChannelStream` per receiver, and spawns the session task. The caller owns the returned `Radio`; call `stop` when finished.

| Argument | Type | Meaning |
|---|---|---|
| `handle` | `&Handle` | The caller's tokio runtime; the socket and task live on it. |
| `radio` | `&Discovered` | A Protocol 1 radio from `discover`. |
| `cfg` | `P1Config` | Initial settings (receiver count, rates, frequencies), moved in. |
| `gate` | `TxGate` | The transmit gate; pass `TxGate::new()` (closed) for a receive-only session. |
| `opts` | `SessionOptions` | Tick and stream depth. |

**Returns:** `Radio` (marked `must_use`; dropping it ends the session). **Errors:** `Unsupported` for a Protocol 2 radio; `Config` or `Unsupported` from `validate`; `Io` from socket bind, connect or send. **Special concerns:** async; the start-up datagrams are all receive datagrams (MOX clear). Transmit: this is a keying path. The gate must be open (a bounded lease) and its limits (drive cap, allowed bands) are checked; the crate has no relay lock or band plan of its own, so the relay's lock and band check have to be applied by the caller (unknown whether the relay consumes this crate yet; to-do: record that in the relay docs).

**Signature:** `pub async fn start(handle: &Handle, radio: &Discovered, cfg: P1Config, gate: TxGate, opts: SessionOptions) -> Result<Self, HpsdrError>`. **See also:** `Ep2Builder`, `Radio::stop`; test `two_receivers_stream_their_own_tones_without_loss` (session.rs).

### `Radio::take_streams` [@ANCHOR: hpsdr:hpsdr_session_take_streams]

Hands the caller the per-receiver `ChannelStream`s, index equal to the DDC number, by moving them out of the `Radio`. It can be called once: the second call returns an empty list, because the streams have already gone to the first caller (each stream has one consumer).

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Radio` | Mutated: the stream list is emptied. |

**Returns:** `Vec<ChannelStream>` of `num_rx` entries the first time, empty afterwards. **Errors:** none. **Special concerns:** no locks, no I/O. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn take_streams(&mut self) -> Vec<ChannelStream>`. **See also:** `IqStream`; test `two_receivers_stream_their_own_tones_without_loss` (session.rs).

### `Radio::update` [@ANCHOR: hpsdr:hpsdr_session_update]

Changes the running radio's settings (frequency, sample rate, attenuator, pre-amp and so on) through a closure that edits a copy of the `P1Config`. The copy is validated and, if the receiver count is unchanged, replaces the shared settings; the session task sends the new values in the next datagrams. The receiver count is fixed for the life of the session.

| Argument | Type | Meaning |
|---|---|---|
| `f` | `impl FnOnce(&mut P1Config)` | Closure that edits the copy; it runs while the configuration mutex is held, so it must not block or call back into the session. |

**Returns:** `Ok(())`. **Errors:** `Config` or `Unsupported` from `validate`; `Config` when the receiver count changed. On error nothing is changed. **Special concerns:** takes the `cfg` mutex for the duration of the closure. Changing `tx_hz` does not key anything; the gate limits are checked against it at the next key request and datagram. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn update(&self, f: impl FnOnce(&mut P1Config)) -> Result<(), HpsdrError>`. **See also:** `P1Config::validate`; test `sample_rate_and_frequency_changes_reach_the_radio` (session.rs).

### `Radio::status` [@ANCHOR: hpsdr:hpsdr_session_status]

Returns a copy of the latest `HardwareStatus` that the session assembled from the radio's status C&C bytes: ADC overload flags, exciter, forward and reverse power counts, supply count, the PTT, dot and dash inputs and the firmware version. All counts are raw ADC values; convert with `supply_volts`, `HardwareStatus::watts` and `swr`. It is the default value until the first EP6 datagram arrives.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&Radio` | Shared borrow. |

**Returns:** a `HardwareStatus` (`Copy`). **Errors:** none. **Special concerns:** takes the `status` mutex briefly. Temperature is not defined by the published protocol and is never reported. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn status(&self) -> HardwareStatus`. **See also:** `HardwareStatus`; test `overload_power_and_supply_status_are_reported` (session.rs).

### `Radio::request_ptt` [@ANCHOR: hpsdr:hpsdr_session_request_ptt]

Asks the session to key or unkey the transmitter. Unkeying (`ptt` false) is always accepted and clears the drive and the queued I/Q. Keying is refused with `GateClosed` while the gate is closed; otherwise `TxGate::check_tx` caps the drive at the gate's limit and refuses a transmit frequency outside the allowed bands, and only then is the request stored. The task re-checks the gate before every datagram and unkeys when the lease expires.

| Argument | Type | Meaning |
|---|---|---|
| `ptt` | `bool` | True to key, false to unkey. |
| `drive` | `u8` | Requested drive 0..=255; may be lowered by the gate limits. |

**Returns:** `Ok(())`. **Errors:** `GateClosed`; `OutsideLimits` when `tx_hz` is outside the allowed bands. **Special concerns:** takes the `cfg` and `tx` mutexes in that order, briefly. The request is a wish, not a transmission: nothing is sent until the next tick, and then only through `next_datagram`. Transmit: this is a keying path. The gate must be open (a bounded lease) and its limits (drive cap, allowed bands) are checked; the crate has no relay lock or band plan of its own, so the relay's lock and band check have to be applied by the caller (unknown whether the relay consumes this crate yet; to-do: record that in the relay docs).

**Signature:** `pub fn request_ptt(&self, ptt: bool, drive: u8) -> Result<(), HpsdrError>`. **See also:** `TxGate::open_until`, `TxGate::check_tx`; test `a_whole_receive_session_with_a_closed_gate_never_transmits` (session.rs).

### `Radio::send_tx_iq` [@ANCHOR: hpsdr:hpsdr_session_send_tx_iq]

Queues transmit I/Q samples (16-bit pairs at 48 ksps) for the session task, which sends up to 126 per datagram while keyed. The queue holds at most `TX_QUEUE_MAX` (4096, about 85 ms) samples; a call that would overflow it is refused as a whole, so nothing is dropped silently or repeated.

| Argument | Type | Meaning |
|---|---|---|
| `iq` | `&[(i16, i16)]` | (I, Q) samples to append. |

**Returns:** `Ok(())`. **Errors:** `GateClosed` while the gate is closed; `Config("transmit queue full")` when the queue would exceed `TX_QUEUE_MAX`. **Special concerns:** takes the `tx` mutex briefly. Samples are only sent while `request_ptt(true, ..)` has been accepted and the gate stays open; an unkey or a lease expiry clears the queue. Transmit: this is a keying path. The gate must be open (a bounded lease) and its limits (drive cap, allowed bands) are checked; the crate has no relay lock or band plan of its own, so the relay's lock and band check have to be applied by the caller (unknown whether the relay consumes this crate yet; to-do: record that in the relay docs).

**Signature:** `pub fn send_tx_iq(&self, iq: &[(i16, i16)]) -> Result<(), HpsdrError>`. **See also:** `TX_QUEUE_MAX`; test `a_whole_receive_session_with_a_closed_gate_never_transmits` (session.rs).

### `Radio::stop` [@ANCHOR: hpsdr:hpsdr_session_stop]

Ends the session cleanly and unkeys the radio. It clears the key request, drive and queued I/Q, aborts the session task and awaits it (so no transmit datagram can follow), sends one receive datagram (MOX clear, drive zero) so a radio that was keyed is told to unkey explicitly, then sends the Stop command. The Stop packet is sent even if the unkey datagram could not be.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `Radio` | The session, consumed. |

**Returns:** `Ok(())`. **Errors:** `Io` if the Stop packet cannot be sent; the error of the unkey datagram if only that failed. **Special concerns:** async; takes the `tx` and `cfg` mutexes briefly, then awaits the aborted task. Idempotent by consumption: a stopped session cannot be used again. Transmit: after it returns the last datagram sent had MOX clear.

**Signature:** `pub async fn stop(mut self) -> Result<(), HpsdrError>`. **See also:** `Radio::drop`; tests `an_open_gate_keys_and_the_lease_expiry_unkeys` (session.rs), `stop_while_keyed_sends_a_mox_clear_datagram_before_the_stop_packet` (session.rs).

### `Radio::drop` [@ANCHOR: hpsdr:hpsdr_session_drop]

The `Drop` implementation of the Protocol 1 session. A `Radio` dropped without `stop()` unkeys at once (key request, drive and queued I/Q are cleared, so the task's next datagram is a receive one) and sets the `closing` flag; the task then sends the Stop command and ends. The task is never just aborted, which would leave the last keyed datagram standing in the radio.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Radio` | The session being dropped. |

**Returns:** nothing. **Errors:** none (a drop cannot report a failure). **Special concerns:** synchronous and non-blocking: it takes the `tx` mutex briefly and stores an atomic flag; the Stop packet goes out from the task on its next tick (up to one tick later). Transmit: unkeying takes effect on the next datagram, so call `stop().await` where the exact moment matters.

**Signature:** `fn drop(&mut self)`. **See also:** `Radio::stop`; test `dropping_a_keyed_session_unkeys_and_sends_stop_and_the_task_ends` (session.rs).

### `P2Config::default` [@ANCHOR: hpsdr:hpsdr_session_p2_config_default]

The default Protocol 2 session settings: 48 ksps, one receiver at 7,100,000 Hz, one ADC, both attenuators 0 dB, dither and random off, transmit frequency 7,100,000 Hz and the specification's default port map (`RadioPorts::default`). Callers override fields with struct update syntax.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

**Returns:** a `P2Config { rate, rx_hz, num_adc, att_adc0, att_adc1, dither, random, tx_hz, ports }`; the receiver count is the length of `rx_hz` (1..=8). **Errors:** none. **Special concerns:** pure. The default does not transmit: the transmit frequency only matters once the gate is open and PTT is requested. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `impl Default for P2Config { fn default() -> Self }`. **See also:** `P2Config::validate`; test `two_receivers_stream_their_own_tones_without_loss` (session_p2.rs).

### `P2Config::validate` [@ANCHOR: hpsdr:hpsdr_session_p2_config_validate]

Rejects settings the Protocol 2 packets cannot express: the receiver count (the length of `rx_hz`) must be 1 to 8, each attenuator 0 to 31 dB, and the ADC count 1 to 8. `Radio2::start` and `Radio2::update` call it so that a bad setting is refused before it reaches a packet.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&P2Config` | The settings to check. |

**Returns:** `Ok(())`. **Errors:** `Config("1 to 8 receivers")`, `Config("attenuator 0..=31 dB")` or `Config("1 to 8 ADCs")`. **Special concerns:** pure. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn validate(&self) -> Result<(), HpsdrError>`. **See also:** test `frequency_rate_and_attenuator_changes_reach_the_radio` (session_p2.rs).

### `P2Config::ddcs` [@ANCHOR: hpsdr:hpsdr_session_p2_config_ddcs]

Builds the DDC-specific settings from the configuration: one `DdcSetting` per entry of `rx_hz`, each enabled, listening to ADC0, at the common sample rate. The session passes the list to `encode_ddc_specific`. Assigning receivers to other ADCs is not supported yet.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&P2Config` | Only `rx_hz.len()` and `rate` are used. |

**Returns:** a `Vec<DdcSetting>` with `rx_hz.len()` entries. **Errors:** none. **Special concerns:** private and pure. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `fn ddcs(&self) -> Vec<DdcSetting>`. **See also:** `encode_ddc_specific`; test `two_receivers_stream_their_own_tones_without_loss` (session_p2.rs).

### `lock (Protocol 2 session)` [@ANCHOR: hpsdr:hpsdr_session_p2_lock]

Private helper identical in purpose to the Protocol 1 one: it locks a mutex and, if a panic poisoned it, takes the data instead of panicking. The gate, not these mutexes, decides whether anything is transmitted.

| Argument | Type | Meaning |
|---|---|---|
| `m` | `&Mutex<T>` | The mutex to lock. |

**Returns:** a `MutexGuard<T>`. **Errors:** none (poison is recovered). **Special concerns:** blocks until the mutex is free; guards are held briefly and never across an await. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T>`. **See also:** `session::lock`; test `two_receivers_stream_their_own_tones_without_loss` (session_p2.rs).

### `hp_rx` [@ANCHOR: hpsdr:hpsdr_session_p2_hp_rx]

Builds the `HpRx` (receive High Priority settings) from the configuration and the target: the run bit as given, the receiver frequencies, both attenuators and, when the radio announced phase words, the DSP clock used to turn frequencies into phase words.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&P2Config` | Current settings. |
| `t` | `&Target` | Where the session sends; carries the optional `clock_hz`. |
| `run` | `bool` | The run bit: true while streaming, false in the final packet of `stop` and drop. |

**Returns:** an `HpRx`. **Errors:** none. **Special concerns:** private and pure. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `fn hp_rx(cfg: &P2Config, t: &Target, run: bool) -> HpRx`. **See also:** `p2::encode_high_priority_rx`; test `two_receivers_stream_their_own_tones_without_loss` (session_p2.rs).

### `next_packets` [@ANCHOR: hpsdr:hpsdr_session_p2_next_packets]

Chooses the packets for one tick: a High Priority packet and, only while keyed, a DUC I/Q packet. They are transmit packets only while the caller asked to key and the gate is open at `now`: the High Priority packet is then built by `encode_high_priority_tx` and up to 240 queued samples (shifted left 8 bits to 24-bit) by `encode_duc_iq`. Otherwise, or when the gate or its limits refuse, the key request and queue are cleared and the receive High Priority packet is built; no I/Q packet is built. Sequence numbers advance only for packets actually built.

| Argument | Type | Meaning |
|---|---|---|
| `shared` | `&Shared` | Session state. |
| `gate` | `&TxGate` | Transmit gate. |
| `t` | `&Target` | Target and clock. |
| `seqs` | `&mut Seqs` | Per-packet-kind sequence numbers, mutated. |
| `now` | `Instant` | Clock reading. |

**Returns:** `(Option<Vec<u8>>, Option<Vec<u8>>)`: the High Priority packet and the optional DUC packet. **Errors:** none (a build failure gives `None`). **Special concerns:** private, called each tick by `run`. The shared state sits behind `std::sync::Mutex`es (`cfg`, `status`, `tx`) taken through `lock`, which recovers a poisoned mutex; locks are held briefly and never across an await. Transmit: this is a keying path. The gate must be open (a bounded lease) and its limits (drive cap, allowed bands) are checked; the crate has no relay lock or band plan of its own, so the relay's lock and band check have to be applied by the caller (unknown whether the relay consumes this crate yet; to-do: record that in the relay docs).

**Signature:** `fn next_packets(shared: &Shared, gate: &TxGate, t: &Target, seqs: &mut Seqs, now: Instant) -> (Option<Vec<u8>>, Option<Vec<u8>>)`. **See also:** test `two_receivers_stream_their_own_tones_without_loss` (session_p2.rs).

### `handle_datagram` [@ANCHOR: hpsdr:hpsdr_session_p2_handle_datagram]

Handles one datagram by the radio's source port. A datagram from the high-priority-to-PC port that parses as a status packet updates the shared status (keeping the firmware value). Otherwise the stream number is `source port - ports.ddc0`; a port below `ddc0` or beyond the receiver count is counted as stray; a DDC packet that does not parse is counted as malformed for its stream; a good one becomes an `IqBlock` after the loss count from its `SeqTracker`, delivered with `try_send` (dropped and counted when the channel is full).

| Argument | Type | Meaning |
|---|---|---|
| `d` | `&[u8]` | The datagram. |
| `from_port` | `u16` | Source UDP port, which identifies the stream. |
| `shared` | `&Shared` | Session state. |
| `senders` | `&[mpsc::Sender<IqBlock>]` | Stream senders. |
| `counters` | `&[Arc<StreamCounters>]` | Stream counters. |
| `trackers` | `&mut [SeqTracker]` | Per-stream sequence trackers. |

**Returns:** nothing. **Errors:** none; every failure is a counter (`stray`, `malformed`, `dropped`). **Special concerns:** non-blocking and synchronous. Whether a real radio sends each stream from its own source port is `verified=unverified`, which is why strays are counted and visible through `Radio2::stray_datagrams`. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `fn handle_datagram(d: &[u8], from_port: u16, shared: &Shared, senders: &[mpsc::Sender<IqBlock>], counters: &[Arc<StreamCounters>], trackers: &mut [SeqTracker])`. **See also:** test `two_receivers_stream_their_own_tones_without_loss` (session_p2.rs).

### `run (Protocol 2 session)` [@ANCHOR: hpsdr:hpsdr_session_p2_run]

The Protocol 2 session task. On every tick it sends a DDC-specific packet when the settings changed (a version counter) or a second has passed (so a lost datagram cannot leave stale settings), then the High Priority packet and, while keyed, the DUC I/Q packet from `next_packets`, each to its own port. For every datagram that arrives it calls `handle_datagram` with the source port. When `closing` is set it sends the run-bit-clear High Priority packet and ends.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `Task` | The task's fixed inputs: socket, shared state, gate, target, senders, counters, tick and sequence numbers. |

**Returns:** a future that completes only after `closing` is set; otherwise it runs until aborted by `Radio2::stop`. **Errors:** send and receive errors are ignored (UDP); loss shows in the counters. **Special concerns:** async, one task per session, sole owner of the trackers and sequence numbers. All transmit packets come from `next_packets`, so the gate is consulted on every tick. Transmit: this is a keying path. The gate must be open (a bounded lease) and its limits (drive cap, allowed bands) are checked; the crate has no relay lock or band plan of its own, so the relay's lock and band check have to be applied by the caller (unknown whether the relay consumes this crate yet; to-do: record that in the relay docs).

**Signature:** `async fn run(mut t: Task)`. **See also:** test `two_receivers_stream_their_own_tones_without_loss` (session_p2.rs).

### `Radio2::start` [@ANCHOR: hpsdr:hpsdr_session_p2_start]

Configures a Protocol 2 radio and begins streaming. It validates the settings, takes the DSP clock from the board when the radio announced phase words (refusing a board with no known clock), binds an ephemeral socket, sends the General packet to the radio's command port, waits 5 ms so the radio has the port map, creates one stream per receiver and spawns the session task, whose first ticks send the DDC-specific and High Priority packets with the run bit set.

| Argument | Type | Meaning |
|---|---|---|
| `handle` | `&Handle` | The caller's tokio runtime. |
| `radio` | `&Discovered` | A Protocol 2 radio from discovery. |
| `cfg` | `P2Config` | Initial settings. |
| `gate` | `TxGate` | Transmit gate; pass `TxGate::new()` (closed) for receive only. |
| `opts` | `SessionOptions` | `tick` paces the command packets; `stream_depth` the stream buffers. |

**Returns:** `Radio2` (marked `must_use`). **Errors:** `Unsupported` for a Protocol 1 radio or for phase words on a board with no known clock; `Config` from `validate`; `Io` from the socket or the General packet send. **Special concerns:** async; the 5 ms pause is a fixed delay, unverified against hardware. The General packet enables no transmit path. Transmit: this is a keying path. The gate must be open (a bounded lease) and its limits (drive cap, allowed bands) are checked; the crate has no relay lock or band plan of its own, so the relay's lock and band check have to be applied by the caller (unknown whether the relay consumes this crate yet; to-do: record that in the relay docs).

**Signature:** `pub async fn start(handle: &Handle, radio: &Discovered, cfg: P2Config, gate: TxGate, opts: SessionOptions) -> Result<Self, HpsdrError>`. **See also:** `p2::encode_general`, `Radio2::stop`; test `two_receivers_stream_their_own_tones_without_loss` (session_p2.rs).

### `Radio2::take_streams` [@ANCHOR: hpsdr:hpsdr_session_p2_take_streams]

Hands the caller the per-receiver `ChannelStream`s (index equal to the DDC number) by moving them out of the `Radio2`. The second call returns an empty list.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Radio2` | Mutated: the stream list is emptied. |

**Returns:** `Vec<ChannelStream>`; empty after the first call. **Errors:** none. **Special concerns:** no locks, no I/O. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn take_streams(&mut self) -> Vec<ChannelStream>`. **See also:** `Radio::take_streams`; test `two_receivers_stream_their_own_tones_without_loss` (session_p2.rs).

### `Radio2::update` [@ANCHOR: hpsdr:hpsdr_session_p2_update]

Changes the running radio's settings through a closure editing a copy of the `P2Config`. The copy is validated; the receiver count and the port map may not change. On success it replaces the shared settings and bumps the version counter, so the task sends a fresh DDC-specific packet on its next tick.

| Argument | Type | Meaning |
|---|---|---|
| `f` | `impl FnOnce(&mut P2Config)` | Closure that edits the copy; it runs under the `cfg` mutex and must not block or call back into the session. |

**Returns:** `Ok(())`. **Errors:** `Config` from `validate`, `Config("the receiver count cannot change while running")`, `Config("the port map cannot change while running")`; nothing changes on error. **Special concerns:** takes the `cfg` mutex for the duration of the closure. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn update(&self, f: impl FnOnce(&mut P2Config)) -> Result<(), HpsdrError>`. **See also:** `P2Config::validate`; test `frequency_rate_and_attenuator_changes_reach_the_radio` (session_p2.rs).

### `Radio2::status` [@ANCHOR: hpsdr:hpsdr_session_p2_status]

Returns a copy of the latest `HardwareStatus` from the High Priority Status packets: ADC overload flags, ADC peak magnitudes, exciter, forward and reverse power counts, supply count, PTT, dot and dash inputs and PLL lock. Counts are raw ADC values. It is the default until the first status packet arrives.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&Radio2` | Shared borrow. |

**Returns:** a `HardwareStatus` (`Copy`). **Errors:** none. **Special concerns:** takes the `status` mutex briefly. Temperature is not defined by the published protocol. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn status(&self) -> HardwareStatus`. **See also:** `Radio::status`; test `overload_power_and_supply_status_are_reported` (session_p2.rs).

### `Radio2::stray_datagrams` [@ANCHOR: hpsdr:hpsdr_session_p2_stray]

Returns how many datagrams arrived from a source port that belongs to no stream or status. A non-zero count means the radio does not send its streams the way the session assumes (one source port per DDC, `ddc0 + n`), which is an open, unverified point of the specification; it signals stray traffic instead of delivering wrong data.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&Radio2` | Shared borrow. |

**Returns:** `u64`, the running count since the session started. **Errors:** none. **Special concerns:** a relaxed atomic load, no locks. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn stray_datagrams(&self) -> u64`. **See also:** `handle_datagram`; test `two_receivers_stream_their_own_tones_without_loss` (session_p2.rs).

### `Radio2::request_ptt` [@ANCHOR: hpsdr:hpsdr_session_p2_request_ptt]

Asks the Protocol 2 session to key or unkey. Unkeying is always accepted and clears the drive and queued I/Q. Keying is refused with `GateClosed` while the gate is closed, and `TxGate::check_tx` caps the drive and refuses a transmit frequency outside the allowed bands before the request is stored. The task re-checks the gate before every packet and unkeys when the lease expires.

| Argument | Type | Meaning |
|---|---|---|
| `ptt` | `bool` | True to key, false to unkey. |
| `drive` | `u8` | Requested drive 0..=255, possibly lowered by the gate limits. |

**Returns:** `Ok(())`. **Errors:** `GateClosed`; `OutsideLimits`. **Special concerns:** takes the `cfg` and `tx` mutexes briefly. The request is stored, not sent: packets go out on the next tick through `next_packets`. Transmit: this is a keying path. The gate must be open (a bounded lease) and its limits (drive cap, allowed bands) are checked; the crate has no relay lock or band plan of its own, so the relay's lock and band check have to be applied by the caller (unknown whether the relay consumes this crate yet; to-do: record that in the relay docs).

**Signature:** `pub fn request_ptt(&self, ptt: bool, drive: u8) -> Result<(), HpsdrError>`. **See also:** `TxGate::check_tx`; test `a_whole_receive_session_with_a_closed_gate_never_transmits` (session_p2.rs).

### `Radio2::send_tx_iq` [@ANCHOR: hpsdr:hpsdr_session_p2_send_tx_iq]

Queues transmit I/Q (16-bit pairs) for the session task, which sends up to 240 per DUC packet while keyed. The queue is limited to `TX_QUEUE_MAX` (4096) samples; a call that would overflow it is refused whole, so nothing is dropped silently or repeated.

| Argument | Type | Meaning |
|---|---|---|
| `iq` | `&[(i16, i16)]` | (I, Q) samples to append. |

**Returns:** `Ok(())`. **Errors:** `GateClosed`; `Config("transmit queue full")`. **Special concerns:** takes the `tx` mutex briefly. An unkey or a lease expiry clears the queue. Transmit: this is a keying path. The gate must be open (a bounded lease) and its limits (drive cap, allowed bands) are checked; the crate has no relay lock or band plan of its own, so the relay's lock and band check have to be applied by the caller (unknown whether the relay consumes this crate yet; to-do: record that in the relay docs).

**Signature:** `pub fn send_tx_iq(&self, iq: &[(i16, i16)]) -> Result<(), HpsdrError>`. **See also:** `TX_QUEUE_MAX`; test `a_whole_receive_session_with_a_closed_gate_never_transmits` (session_p2.rs).

### `Radio2::stop` [@ANCHOR: hpsdr:hpsdr_session_p2_stop]

Ends the Protocol 2 session and unkeys. It clears the key request, drive and queue, aborts the task and awaits it (so a transmit packet being built on another worker thread cannot follow and undo the unkey), then sends one High Priority packet with the run bit clear and sequence `u32::MAX` to the radio's High Priority port.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `Radio2` | The session, consumed. |

**Returns:** `Ok(())`. **Errors:** `Config` from encoding the packet; `Io` from the send. **Special concerns:** async; takes the `tx` and `cfg` mutexes briefly. After it returns the last packet sent had PTT clear and the run bit clear. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub async fn stop(mut self) -> Result<(), HpsdrError>`. **See also:** `Radio::stop`; tests `two_receivers_stream_their_own_tones_without_loss` (session_p2.rs), `stopping_or_dropping_a_keyed_session_leaves_the_radio_unkeyed` (session_p2.rs).

### `Radio2::drop` [@ANCHOR: hpsdr:hpsdr_session_p2_drop]

The `Drop` implementation of the Protocol 2 session: same contract as the Protocol 1 one. A `Radio2` dropped without `stop()` clears the key request, drive and queue at once and sets `closing`; the task's next tick then sends the unkeyed run-bit-clear packet and ends. It is never just aborted.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Radio2` | The session being dropped. |

**Returns:** nothing. **Errors:** none. **Special concerns:** synchronous and non-blocking (a mutex and an atomic flag); the final packet goes out up to one tick later. Call `stop().await` where the exact moment matters. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `fn drop(&mut self)`. **See also:** `Radio::drop`; test `stopping_or_dropping_a_keyed_session_leaves_the_radio_unkeyed` (session_p2.rs).

### `Board::from_p1_id` [@ANCHOR: hpsdr:hpsdr_types_board_from_p1_id]

Maps a Protocol 1 `Board_ID` byte from the discovery reply to the `Board` enum: 0 Metis, 1 Hermes, 4 Angelia, 5 Orion, 6 Hermes Lite; any other value, including 2 (Griffin, which the enum has no variant for), becomes `Board::Other(id)`.

| Argument | Type | Meaning |
|---|---|---|
| `id` | `u8` | The board id byte (Metis v1.33). |

**Returns:** a `Board`. **Errors:** none; unknown ids are kept in `Other`. **Special concerns:** pure. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn from_p1_id(id: u8) -> Self`. **See also:** `Board::from_p2_type`; test `board_ids_map_and_unknown_is_kept` (types.rs).

### `Board::from_p2_type` [@ANCHOR: hpsdr:hpsdr_types_board_from_p2_type]

Maps the Protocol 2 board type (byte 11 of the discovery reply, spec v4.4) to `Board`: 0 Metis, 1 Hermes, 2 Hermes B, 3 Angelia, 4 Orion, 5 Orion Mk II, 6 Hermes Lite, 10 Saturn; any other value becomes `Board::Other(t)`.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `u8` | The board type byte. |

**Returns:** a `Board`. **Errors:** none. **Special concerns:** pure. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn from_p2_type(t: u8) -> Self`. **See also:** `Board::from_p1_id`; test `board_ids_map_and_unknown_is_kept` (types.rs).

### `Board::dsp_clock_hz` [@ANCHOR: hpsdr:hpsdr_types_board_dsp_clock]

Gives the radio's DSP clock in hertz, which is needed to turn frequencies into phase words: 122,880,000 for Hermes, Hermes B, Angelia, Orion and Orion Mk II (spec v4.4 Appendix A gives 122.88 MHz for Hermes). Other boards give `None` because the crate has not read their clock from the specification.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `Board` | The board. |

**Returns:** `Option<f64>` hertz. **Errors:** none; `None` is the unknown case (a session that needs phase words refuses to start for it). **Special concerns:** pure. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn dsp_clock_hz(self) -> Option<f64>`. **See also:** `p2::phase_word`, `Radio2::start`; test `board_ids_map_and_unknown_is_kept` (types.rs).

### `Board::power_constant` [@ANCHOR: hpsdr:hpsdr_types_board_power_constant]

Gives constant 2 of the forward and reverse power formula (spec v4.4 Appendix A): for Hermes 0.09 for the ANAN-10 and 0.095 for the ANAN-100. Both models share board type 1, so the caller says which by the flag. Any other board gives `None` because the specification states no constant for it.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `Board` | The board. |
| `anan10` | `bool` | True for an ANAN-10, false for an ANAN-100. |

**Returns:** `Option<f64>`, the constant `c2` for `HardwareStatus::watts` and `swr`. **Errors:** none; `None` means power cannot be converted to watts for this board (the raw counts are still reported). **Special concerns:** pure. Power calibration is `verified=unverified` on real hardware. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn power_constant(self, anan10: bool) -> Option<f64>`. **See also:** `HardwareStatus::watts`; test `board_ids_map_and_unknown_is_kept` (types.rs).

### `Rate::hz` [@ANCHOR: hpsdr:hpsdr_types_rate_hz]

Returns a receiver sample rate in hertz: 48,000, 96,000, 192,000, 384,000, 768,000 or 1,536,000. The sessions put it in `IqBlock::sample_rate_hz`.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `Rate` | The rate. |

**Returns:** `u32` hertz. **Errors:** none. **Special concerns:** pure. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn hz(self) -> u32`. **See also:** `Rate::from_ksps`; test `rates_round_trip` (types.rs).

### `Rate::from_ksps` [@ANCHOR: hpsdr:hpsdr_types_rate_from_ksps]

Parses a sample rate given in kilohertz: 48, 96, 192, 384, 768 or 1536. Used to turn a human or configuration value into a `Rate`.

| Argument | Type | Meaning |
|---|---|---|
| `k` | `u16` | Rate in ksps. |

**Returns:** the matching `Rate`. **Errors:** `Config("sample rate N ksps")` for any other value. **Special concerns:** pure. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn from_ksps(k: u16) -> Result<Self, HpsdrError>`. **See also:** `Rate::hz`; test `rates_round_trip` (types.rs).

### `Rate::p1_code` [@ANCHOR: hpsdr:hpsdr_types_rate_p1_code]

Gives the Protocol 1 speed code for the C&C general address (USB protocol v1.60, C1 bits 1..0): 0 for 48 ksps, 1 for 96, 2 for 192, 3 for 384. Protocol 1 cannot express 768 or 1536 ksps.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `Rate` | The rate. |

**Returns:** `Ok(u8)` code 0..=3. **Errors:** `Config("Protocol 1 supports 48/96/192/384 ksps")` for 768 and 1536 ksps; `P1Config::validate` surfaces it. **Special concerns:** pure. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn p1_code(self) -> Result<u8, HpsdrError>`. **See also:** `Rate::from_p1_code`; test `rates_round_trip` (types.rs).

### `Rate::from_p1_code` [@ANCHOR: hpsdr:hpsdr_types_rate_from_p1_code]

The inverse of `Rate::p1_code`: only the low two bits of the code are used (0 48 ksps, 1 96, 2 192, 3 384), so it is total and the emulator can decode any received C1 byte.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `u8` | The code; bits above the low two are ignored. |

**Returns:** a `Rate` (never 768 or 1536). **Errors:** none. **Special concerns:** pure. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn from_p1_code(c: u8) -> Self`. **See also:** `Rate::p1_code`; test `rates_round_trip` (types.rs).

### `HardwareStatus::supply_volts` [@ANCHOR: hpsdr:hpsdr_types_status_supply_volts]

Converts the raw 12-bit supply count to volts with the Hermes formula `adc / 4095 * 3.3` (spec v4.4 Appendix A). The formula is for Hermes; other boards may scale differently (unknown; to-do: check each board when hardware is available).

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&HardwareStatus` | Uses `supply_raw`. |

**Returns:** `f64` volts, 0 to 3.3. **Errors:** none. **Special concerns:** pure. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn supply_volts(&self) -> f64`. **See also:** `HardwareStatus`; test `power_and_swr_from_counts` (types.rs).

### `HardwareStatus::watts` [@ANCHOR: hpsdr:hpsdr_types_status_watts]

Converts a raw forward or reverse power count to watts: `(adc / 4095 * 3.3)^2 / c2` (spec v4.4 Appendix A). Obtain `c2` from `Board::power_constant`. An associated function, not a method, so it works on any raw count.

| Argument | Type | Meaning |
|---|---|---|
| `raw` | `u16` | The 12-bit ADC count. |
| `c2` | `f64` | Calibration constant 2; must be non-zero, a zero gives infinity or NaN. |

**Returns:** `f64` watts. **Errors:** none (no check on `c2`). **Special concerns:** pure. It is a report of what the radio measured, never a gate input. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn watts(raw: u16, c2: f64) -> f64`. **See also:** `Board::power_constant`; test `power_and_swr_from_counts` (types.rs).

### `HardwareStatus::swr` [@ANCHOR: hpsdr:hpsdr_types_status_swr]

Computes the standing wave ratio from the forward and reverse power counts: the reflection coefficient is the square root of reverse over forward power, and the SWR (standing wave ratio) is `(1 + rho) / (1 - rho)`. Returns `None` when there is no forward power, and infinity when reverse is at least forward.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&HardwareStatus` | Uses `forward_raw` and `reverse_raw`. |
| `c2` | `f64` | Calibration constant 2 from `Board::power_constant`. |

**Returns:** `Option<f64>`. **Errors:** none; `None` means "no forward power". **Special concerns:** pure. Informational only: the crate does not trip anything on SWR. Transmit: not involved; the relay lock, band check and transmit gate do not apply.

**Signature:** `pub fn swr(&self, c2: f64) -> Option<f64>`. **See also:** `HardwareStatus::watts`; test `power_and_swr_from_counts` (types.rs).

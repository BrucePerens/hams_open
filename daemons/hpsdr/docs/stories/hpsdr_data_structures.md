# Data structures

### `Discovered` [@ANCHOR: hpsdr:hpsdr_discovery_discovered]

**Purpose and location.** A radio that answered discovery. Defined in `daemons/hpsdr/src/discovery.rs:19` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `protocol` | `Protocol` | Wire protocol it speaks. |
| `addr` | `SocketAddr` | Where it answered from (use this address for everything else). |
| `mac` | `[u8; 6]` | MAC address. |
| `board` | `Board` | Board type. |
| `code_version` | `u8` | Gateware version byte. |
| `busy` | `bool` | Already streaming to a host (status 0x03). |
| `protocol_version` | `Option<u8>` | Protocol version implemented (Protocol 2 byte 12). |
| `num_ddcs` | `Option<u8>` | Number of DDCs implemented (Protocol 2 byte 20). |
| `phase_word` | `bool` | Protocol 2 radio wants frequencies as phase words (byte 21); false for Protocol 1. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_reply` (src/discovery.rs:48), `setup` (src/session.rs:382), `setup` (src/session_p2.rs:559).

**See also:** test `stop_while_keyed_sends_a_mox_clear_datagram_before_the_stop_packet (src/session.rs)`.

### `Tone` [@ANCHOR: hpsdr:hpsdr_emulator_tone]

**Purpose and location.** A synthetic carrier at an absolute radio frequency. Defined in `daemons/hpsdr/src/emulator.rs:23` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `freq_hz` | `f64` | Absolute frequency in hertz. |
| `amplitude` | `f64` | Amplitude, 1.0 = ADC full scale. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `two_receivers_stream_their_own_tones_without_loss (src/session.rs)`.

### `EmulatorConfig` [@ANCHOR: hpsdr:hpsdr_emulator_emulator_config]

**Purpose and location.** What the emulated radio is and how it behaves. Defined in `daemons/hpsdr/src/emulator.rs:32` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `board_id` | `u8` | Protocol 1 board identifier to report (1 = Hermes). |
| `mac` | `[u8; 6]` | MAC address to report. |
| `code_version` | `u8` | Gateware code version to report. |
| `tones` | `Vec<Tone>` | Carriers present at the antenna. |
| `status` | `HardwareStatus` | Status the radio reports (overload, power, supply). |
| `pace` | `Duration` | Time between EP6 datagrams (real time at 48 ksps one receiver is about 10.5 ms). |
| `skip_every` | `u32` | When non-zero, every Nth EP6 datagram is numbered but not sent (to exercise loss accounting). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `start_p1` (src/emulator.rs:104), `next_ep6` (src/emulator.rs:228), `run_p1` (src/emulator.rs:243), `setup` (src/session.rs:382).

**See also:** test `discovery_finds_the_emulator_with_board_and_version (src/session.rs)`.

### `EmulatorCounters` [@ANCHOR: hpsdr:hpsdr_emulator_emulator_counters]

**Purpose and location.** What the emulator has seen from the client. Defined in `daemons/hpsdr/src/emulator.rs:74` as a pub `struct`. It derives Debug, Default.

| Field | Type | Meaning |
|---|---|---|
| `discoveries` | `AtomicU64` | Discovery requests answered. |
| `starts` | `AtomicU64` | Start commands with the I/Q bit set. |
| `stops` | `AtomicU64` | Stop commands (or start commands with the I/Q bit clear). |
| `ep2_packets` | `AtomicU64` | EP2 datagrams parsed. |
| `mox_frames` | `AtomicU64` | EP2 frames whose MOX bit was set. |
| `drive_nonzero_frames` | `AtomicU64` | Drive (address 9) frames with a non-zero level. |
| `tx_iq_packets` | `AtomicU64` | EP2 datagrams carrying non-zero transmit I/Q. |
| `ep6_sent` | `AtomicU64` | EP6 datagrams sent. |

**Invariants, ownership and concurrency.** Plain data: its fields hold AtomicU64 (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `counters` (src/emulator.rs:125), `run_p1` (src/emulator.rs:243).

**See also:** test `discovery_finds_the_emulator_with_board_and_version (src/session.rs)`.

### `Emulator` [@ANCHOR: hpsdr:hpsdr_emulator_emulator]

**Purpose and location.** Handle to a running Protocol 1 radio emulator that listens on loopback only; tests use it in place of hardware. Defined in `daemons/hpsdr/src/emulator.rs:95` as a pub `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `addr` | `SocketAddr` | The loopback address the UDP socket bound (read with `addr()`). |
| `counters` | `Arc<EmulatorCounters>` | Shared atomic counters the emulator task updates and tests read. |
| `task` | `JoinHandle<()>` | The tokio task running `run_p1`; `stop` aborts it, a drop does not. |

**Invariants, ownership and concurrency.** Owned by the test that started it. The counters are shared by `Arc` with the task and are atomics, so no lock is held. The emulator cannot key a transmitter; it only counts transmit requests.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `setup` (src/session.rs:382).

**See also:** test `discovery_finds_the_emulator_with_board_and_version (src/session.rs)`.

### `P2EmulatorConfig` [@ANCHOR: hpsdr:hpsdr_emulator_p2_p2_emulator_config]

**Purpose and location.** What the emulated radio is and how it behaves. Defined in `daemons/hpsdr/src/emulator_p2.rs:34` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `board_type` | `u8` | Protocol 2 board type byte to report (1 = Hermes). |
| `mac` | `[u8; 6]` | MAC address to report. |
| `firmware` | `u8` | Gateware code version to report. |
| `num_ddcs` | `u8` | DDCs the radio claims (and serves). |
| `phase_word` | `bool` | Report and expect phase words rather than hertz. |
| `tones` | `Vec<Tone>` | Carriers present at the antenna. |
| `status` | `HardwareStatus` | Status the radio reports. |
| `pace` | `Duration` | Time between packets (real time at 48 ksps is about 5 ms for 238 samples). |
| `skip_every` | `u32` | When non-zero, every Nth DDC packet is numbered but not sent (to exercise loss accounting). |
| `ports` | `RadioPorts` | The port map the emulator serves (see [`loopback_ports`]). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `start` (src/emulator_p2.rs:171), `apply_hp` (src/emulator_p2.rs:265), `ddc_packet` (src/emulator_p2.rs:337), `run` (src/emulator_p2.rs:354), `setup` (src/session_p2.rs:559).

**See also:** test `listens_on_loopback_only_and_answers_discovery (src/emulator_p2.rs)`.

### `P2Counters` [@ANCHOR: hpsdr:hpsdr_emulator_p2_p2_counters]

**Purpose and location.** What the emulator has seen from the client. Defined in `daemons/hpsdr/src/emulator_p2.rs:85` as a pub `struct`. It derives Debug, Default.

| Field | Type | Meaning |
|---|---|---|
| `discoveries` | `AtomicU64` | Discovery requests answered. |
| `generals` | `AtomicU64` | General packets received. |
| `ddc_specific` | `AtomicU64` | DDC-specific packets received. |
| `hp_packets` | `AtomicU64` | High Priority packets received. |
| `run_starts` | `AtomicU64` | Transitions of the run bit from clear to set. |
| `run_stops` | `AtomicU64` | Transitions of the run bit from set to clear. |
| `ddc_sent` | `AtomicU64` | DDC packets sent. |
| `ptt_frames` | `AtomicU64` | High Priority packets whose PTT bit was set. |
| `drive_nonzero_frames` | `AtomicU64` | High Priority packets with a non-zero drive level. |
| `tx_freq_nonzero_frames` | `AtomicU64` | High Priority packets with a non-zero transmit frequency. |
| `duc_packets` | `AtomicU64` | Packets received on the DUC0 I/Q port (any content). |
| `ptt_now` | `AtomicU64` | 1 while the latest High Priority packet has PTT set. |
| `last_drive` | `AtomicU64` | Latest drive level. |
| `last_atten_adc0` | `AtomicU64` | Latest ADC0 attenuator setting. |
| `last_rate_ksps` | `AtomicU64` | Latest DDC0 rate in ksps. |

**Invariants, ownership and concurrency.** Plain data: its fields hold AtomicU64 (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `counters` (src/emulator_p2.rs:211), `apply_ddc_specific` (src/emulator_p2.rs:246), `apply_hp` (src/emulator_p2.rs:265), `run` (src/emulator_p2.rs:354).

**See also:** test `listens_on_loopback_only_and_answers_discovery (src/emulator_p2.rs)`.

### `EmulatorP2` [@ANCHOR: hpsdr:hpsdr_emulator_p2_emulator_p2]

**Purpose and location.** A running Protocol 2 emulator. Defined in `daemons/hpsdr/src/emulator_p2.rs:157` as a pub `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `addr` | `SocketAddr` | The loopback address of the command socket (read with `addr()`). |
| `counters` | `Arc<P2Counters>` | Shared atomic counters the emulator task updates. |
| `task` | `JoinHandle<()>` | The tokio task serving Protocol 2; `stop` aborts it. |

**Invariants, ownership and concurrency.** Owned by the test that started it; counters are `Arc` shared atomics. Loopback only, no RF.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `setup` (src/session_p2.rs:559).

**See also:** test `listens_on_loopback_only_and_answers_discovery (src/emulator_p2.rs)`.

### `Io` [@ANCHOR: hpsdr:hpsdr_emulator_p2_io]

**Purpose and location.** The set of UDP sockets the Protocol 2 emulator uses, bundled so the serving task owns them together. Defined in `daemons/hpsdr/src/emulator_p2.rs:222` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `cmd` | `UdpSocket` | The command socket: receives discovery requests (answered on the same socket) and General packets. |
| `ddc_spec` | `Arc<UdpSocket>` | The DDC-specific socket: receives DDC-specific packets; shared by `Arc` because the status output reuses it when both ports are equal. |
| `hp_in` | `UdpSocket` | Receives High Priority packets from the host. |
| `duc_in` | `UdpSocket` | Receives transmit I/Q packets from the host. |
| `status_out` | `Arc<UdpSocket>` | Sends High Priority status to the host; shared by `Arc`. |
| `ddc_out` | `Vec<UdpSocket>` | One socket per DDC that sends its I/Q stream to the host. |

**Invariants, ownership and concurrency.** Private to the emulator task; sockets are bound on loopback ports by `bind` and closed when the task ends.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `run` (src/emulator_p2.rs:354).

**See also:** test `display_and_from_io (src/error.rs)`.

### `RfState` [@ANCHOR: hpsdr:hpsdr_emulator_p2_rf_state]

**Purpose and location.** What the client has told the radio. Defined in `daemons/hpsdr/src/emulator_p2.rs:233` as a private `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `host` | `Option<SocketAddr>` | Address of the host that last sent a command, where status and I/Q go; `None` before any command. |
| `phase_word` | `bool` | True when the host asked for frequencies as phase words rather than hertz. |
| `enabled` | `Vec<bool>` | Per-DDC enable flags from the DDC-specific packet. |
| `rate` | `Vec<Rate>` | Per-DDC sample rate from the DDC-specific packet. |
| `hz` | `Vec<u32>` | Per-DDC centre frequency from the High Priority packet, in hertz. |
| `atten0` | `u8` | ADC0 step attenuator in dB. |
| `run` | `bool` | The run bit from the High Priority packet. |

**Invariants, ownership and concurrency.** Owned and mutated only by the emulator task (`apply_ddc_specific`, `apply_hp`); not shared across threads.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `apply_ddc_specific` (src/emulator_p2.rs:246), `apply_hp` (src/emulator_p2.rs:265), `ddc_packet` (src/emulator_p2.rs:337).

**See also:** test `ddc_packets_follow_the_settings (src/emulator_p2.rs)`.

### `HpsdrError` [@ANCHOR: hpsdr:hpsdr_error_hpsdr_error]

**Purpose and location.** Why an operation or a datagram was rejected. Defined in `daemons/hpsdr/src/error.rs:8` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Truncated` | `Truncated` | The datagram is shorter than the format needs. |
| `BadMagic` | `BadMagic` | The datagram does not start with the expected magic, sync bytes or command. |
| `Unsupported` | `Unsupported(String)` | A field holds a value this crate does not support (message names it). |
| `Config` | `Config(String)` | A caller-supplied setting is outside the legal range (message names it). |
| `GateClosed` | `GateClosed` | A transmit-capable operation was refused because the transmit gate is closed. |
| `OutsideLimits` | `OutsideLimits` | A transmit request is outside the limits narrowed onto the gate (frequency outside the allowed bands). |
| `Io` | `Io(String)` | A socket or task failure (the I/O error text). |
| `Timeout` | `Timeout` | Nothing arrived in time. |
| `Stopped` | `Stopped` | The session has been stopped. |

**Invariants, ownership and concurrency.** Plain value type returned by every fallible function of the crate; `Io` wraps a `std::io::Error` (so it is not `Clone` or `PartialEq`).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_reply` (src/discovery.rs:48), `udp_socket` (src/discovery.rs:79), `start_p1` (src/emulator.rs:104), `loopback_ports` (src/emulator_p2.rs:132), `bind` (src/emulator_p2.rs:163), `start` (src/emulator_p2.rs:171), `check_tx` (src/gate.rs:90), `parse_reply` (src/p1.rs:79).

**See also:** test `display_and_from_io (src/error.rs)`.

### `TxGate` [@ANCHOR: hpsdr:hpsdr_gate_tx_gate]

**Purpose and location.** A shared, clonable transmit gate. `TxGate::default()` is closed. Defined in `daemons/hpsdr/src/gate.rs:21` as a pub `struct`. It derives Clone, Debug, Default.

| Field | Type | Meaning |
|---|---|---|
| `lease` | `Arc<Mutex<Option<Instant>>>` | Instant at which the current lease ends; `None` means closed. Behind a mutex. |
| `limits` | `Arc<Mutex<Limits>>` | The drive and band limits applied while open. Behind a mutex. |

**Invariants, ownership and concurrency.** The transmit safety point of the crate: closed by default, opened only by `open_until` for at most MAX_LEASE, closing itself when the lease passes. Clones share the same state (`Arc`), so a gate handed to the session task and kept by the caller are one gate. A poisoned lock reads as closed. This crate holds no relay lock and no band plan; the relay's band check stays the authority.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `default_gate_is_closed_and_lease_expires (src/gate.rs)`.

### `Limits` [@ANCHOR: hpsdr:hpsdr_gate_limits]

**Purpose and location.** What may be sent while the gate is open. Only ever narrowed ([`TxGate::narrow_limits`]). Defined in `daemons/hpsdr/src/gate.rs:28` as a private `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `max_drive` | `u8` | Highest drive level (0..=255) a key request may use; default 255 (no cap). |
| `bands` | `Option<Vec<(u32, u32)>>` | Allowed transmit ranges as (low_hz, high_hz) pairs; `None` means no band limit was set. |

**Invariants, ownership and concurrency.** Only ever narrowed by `TxGate::narrow_limits`; it lives inside the gate's mutex, so reads and writes are serialized.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `limits_only_narrow (src/gate.rs)`.

### `Complex` [@ANCHOR: hpsdr:hpsdr_iq_complex]

**Purpose and location.** One complex baseband sample, scaled so that full scale is +/-1.0. Defined in `daemons/hpsdr/src/iq.rs:14` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Default.

| Field | Type | Meaning |
|---|---|---|
| `re` | `f32` | In-phase part. |
| `im` | `f32` | Quadrature part. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `tone_offset_hz` (src/session.rs:372), `tone_offset_hz` (src/session_p2.rs:544), `rms` (src/session_p2.rs:554).

**See also:** test `channel_stream_delivers_in_order (src/iq.rs)`.

### `IqBlock` [@ANCHOR: hpsdr:hpsdr_iq_iq_block]

**Purpose and location.** A contiguous run of samples from one receiver (DDC). Defined in `daemons/hpsdr/src/iq.rs:23` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `stream` | `u8` | Receiver index (0 for the first DDC). |
| `seq` | `u32` | Sequence number of the datagram this block came from (wraps at 2^32). |
| `lost_before` | `u32` | Datagrams lost on this stream immediately before this one (from the sequence gap). |
| `sample_rate_hz` | `u32` | Sample rate in hertz. |
| `centre_hz` | `u32` | Centre frequency of the receiver in hertz when the block was produced. |
| `samples` | `Vec<Complex>` | The samples. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `next_block` (src/iq.rs:133), `new` (src/iq.rs:148), `next_block` (src/iq.rs:157), `take_block` (src/session_p2.rs:584).

**See also:** test `channel_stream_delivers_in_order (src/iq.rs)`.

### `StreamCounters` [@ANCHOR: hpsdr:hpsdr_iq_stream_counters]

**Purpose and location.** Counters shared between the producer task and the consumer. Defined in `daemons/hpsdr/src/iq.rs:40` as a pub `struct`. It derives Debug, Default.

| Field | Type | Meaning |
|---|---|---|
| `blocks` | `AtomicU64` | Blocks delivered to the consumer. |
| `samples` | `AtomicU64` | Samples delivered. |
| `lost_packets` | `AtomicU64` | Datagrams the sequence tracker found missing. |
| `dropped_blocks` | `AtomicU64` | Blocks dropped because the consumer's channel was full. |
| `malformed` | `AtomicU64` | Datagrams that failed to parse. |

**Invariants, ownership and concurrency.** All fields are atomics shared between the receive task (writer) and the consumer (reader, via `snapshot`); no lock, relaxed counting.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/iq.rs:148).

**See also:** test `counters_accumulate (src/iq.rs)`.

### `StreamStats` [@ANCHOR: hpsdr:hpsdr_iq_stream_stats]

**Purpose and location.** A snapshot of [`StreamCounters`]. Defined in `daemons/hpsdr/src/iq.rs:50` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq, Default.

| Field | Type | Meaning |
|---|---|---|
| `blocks` | `u64` | Blocks delivered to the consumer. |
| `samples` | `u64` | Samples delivered to the consumer. |
| `lost_packets` | `u64` | Datagrams the radio sent that never arrived (sequence gaps). |
| `dropped_blocks` | `u64` | Blocks discarded because the consumer was too slow (the channel was full). |
| `malformed` | `u64` | Datagrams rejected by the parser. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `snapshot` (src/iq.rs:91), `stats` (src/iq.rs:135), `stats` (src/iq.rs:163).

**See also:** test `counters_accumulate (src/iq.rs)`.

### `SeqTracker` [@ANCHOR: hpsdr:hpsdr_iq_seq_tracker]

**Purpose and location.** Tracks a 32-bit sequence number and reports gaps. Defined in `daemons/hpsdr/src/iq.rs:104` as a pub `struct`. It derives Debug, Clone, Copy, Default.

| Field | Type | Meaning |
|---|---|---|
| `next` | `Option<u32>` | The sequence number expected next; `None` before the first datagram. |

**Invariants, ownership and concurrency.** Owned by one receive task; plain `Copy` value, not shared. A backwards jump of less than half the 32-bit range counts as a restart, not a loss.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `seq_gaps_and_wrap (src/iq.rs)`.

### `ChannelStream` [@ANCHOR: hpsdr:hpsdr_iq_channel_stream]

**Purpose and location.** An [`IqStream`] fed by a bounded channel from the receive task. Defined in `daemons/hpsdr/src/iq.rs:140` as a pub `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `mpsc::Receiver<IqBlock>` | Receiving end of the bounded channel the receive task feeds. |
| `counters` | `Arc<StreamCounters>` | Shared counters for this stream. |

**Invariants, ownership and concurrency.** Owned by the `Radio` or `Radio2` that made it and handed out through its streams; the sender half goes to the task, so the task never blocks on a slow consumer (it drops and counts).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `take_streams` (src/session.rs:251), `take_streams` (src/session_p2.rs:421).

**See also:** test `channel_stream_delivers_in_order (src/iq.rs)`.

### `P1Reply` [@ANCHOR: hpsdr:hpsdr_p1_p1_reply]

**Purpose and location.** A parsed Protocol 1 discovery reply (Metis, "Metis Discovery reply format"). Defined in `daemons/hpsdr/src/p1.rs:64` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `running` | `bool` | True when the radio is already sending data (status byte 0x03). |
| `mac` | `[u8; 6]` | The radio's MAC address. |
| `code_version` | `u8` | Gateware code version. |
| `board_id` | `u8` | Raw board identifier. |
| `board` | `Board` | Decoded board. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_reply` (src/p1.rs:79), `encode_reply` (src/p1.rs:99).

**See also:** test `reply_round_trips_and_rejects_junk (src/p1.rs)`.

### `P1Config` [@ANCHOR: hpsdr:hpsdr_p1_p1_config]

**Purpose and location.** Receiver and front-end settings carried by the C&C bytes of EP2 frames. Defined in `daemons/hpsdr/src/p1.rs:111` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `rate` | `Rate` | DDC sample rate (48/96/192/384 ksps). |
| `num_rx` | `u8` | Number of receivers, 1..=7. |
| `rx_hz` | `[u32; 7]` | NCO frequency of each receiver in hertz. |
| `preamp` | `bool` | Receiver pre-amplifier (C0=0, C3 bit 2). |
| `dither` | `bool` | ADC dither (C3 bit 3). |
| `random` | `bool` | ADC random (C3 bit 4). |
| `att_db` | `u8` | ADC1 step attenuator, 0..=31 dB (C0=10, C4[4:0]). |
| `att_enable` | `bool` | Enable the step attenuator (C4 bit 5); when clear the pre-amp bit applies instead. |
| `tx_hz` | `u32` | Transmit NCO frequency in hertz (C0=1). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `next_ep6` (src/emulator.rs:228), `build_rx` (src/p1.rs:304), `cfg4` (src/p1.rs:538), `update` (src/session.rs:258).

**See also:** test `config_validation_and_cc_round_trip (src/p1.rs)`.

### `TxRequest` [@ANCHOR: hpsdr:hpsdr_p1_tx_request]

**Purpose and location.** Transmit request for [`Ep2Builder::build_tx`]. Defined in `daemons/hpsdr/src/p1.rs:237` as a pub `struct`. It derives Debug, Clone, Copy.

| Field | Type | Meaning |
|---|---|---|
| `ptt` | `bool` | Key the transmitter (MOX). |
| `drive` | `u8` | Drive level 0..=255 (C0=9, C1). |
| `iq` | `&'a [(i16, i16)]` | Transmit I/Q as 16-bit (I, Q) pairs at 48 ksps; at most [`TX_SAMPLES_PER_DATAGRAM`]; a short slice is padded with zeros (silence) and a sample is never repeated. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `transmit_builder_refuses_unless_gate_open (src/p1.rs)`.

### `Ep2Builder` [@ANCHOR: hpsdr:hpsdr_p1_ep2_builder]

**Purpose and location.** Builds EP2 datagrams (PC to radio) with the C&C round-robin and a sequence counter. Defined in `daemons/hpsdr/src/p1.rs:249` as a pub `struct`. It derives Debug, Clone, Default.

| Field | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Next sequence number for the EP2 datagram. |
| `cursor` | `usize` | Round-robin position in the rotating command-and-control address table. |

**Invariants, ownership and concurrency.** Owned by the session task and mutated on every datagram; not shared. Transmit parts of a datagram stay zero unless the caller passes an open `TxGate`.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `receive_datagrams_never_carry_transmit_state (src/p1.rs)`.

### `Ep2` [@ANCHOR: hpsdr:hpsdr_p1_ep2]

**Purpose and location.** An EP2 datagram as the radio sees it (the emulator uses this to count transmit requests). Defined in `daemons/hpsdr/src/p1.rs:335` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Datagram sequence number. |
| `cc` | `[[u8; 5]; 2]` | The two C&C quintuples. |
| `tx_iq_nonzero` | `bool` | True if any transmit I/Q byte (I or Q slots) is non-zero. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_ep2` (src/p1.rs:346).

**See also:** test `receive_datagrams_never_carry_transmit_state (src/p1.rs)`.

### `Ep6` [@ANCHOR: hpsdr:hpsdr_p1_ep6]

**Purpose and location.** One decoded EP6 datagram (radio to PC). Defined in `daemons/hpsdr/src/p1.rs:377` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Datagram sequence number (independent per end point). |
| `cc` | `[[u8; 5]; 2]` | The two C&C quintuples (radio-to-PC meaning). |
| `rx` | `Vec<Vec<Complex>>` | Samples of each receiver, in time order across both frames. |
| `mic` | `Vec<i16>` | Microphone samples (16-bit, 48 ksps). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_ep6` (src/p1.rs:397).

**See also:** test `ep6_round_trips_for_one_to_seven_receivers (src/p1.rs)`.

### `RadioPorts` [@ANCHOR: hpsdr:hpsdr_p2_radio_ports]

**Purpose and location.** The UDP ports of a session (P2, "General Packet to SDR", bytes 5..22). Zero means "use the default". Defined in `daemons/hpsdr/src/p2.rs:37` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `ddc_specific` | `u16` | Port the DDC-specific packet goes to (default 1025). |
| `duc_specific` | `u16` | DUC-specific (default 1026). |
| `hp_from_pc` | `u16` | High priority from the PC (default 1027). |
| `hp_to_pc` | `u16` | High priority status from the radio, source port (default 1025). |
| `ddc_audio` | `u16` | DDC audio (default 1028). |
| `duc0_iq` | `u16` | DUC0 I/Q (default 1029). |
| `ddc0` | `u16` | DDC0 I/Q source port; DDC n uses this plus n (default 1035). |
| `mic` | `u16` | Microphone source port (default 1026). |
| `wideband0` | `u16` | Wideband ADC0 source port (default 1027). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `loopback_ports` (src/emulator_p2.rs:132), `encode_general` (src/p2.rs:165), `setup` (src/session_p2.rs:559), `cfg_with` (src/session_p2.rs:591).

**See also:** test `general_packet_carries_ports_and_the_hardware_timer (src/p2.rs)`.

### `P2Reply` [@ANCHOR: hpsdr:hpsdr_p2_p2_reply]

**Purpose and location.** A parsed Protocol 2 discovery reply (P2, "Discovery Reply Packet", option 1: board type below 254). Defined in `daemons/hpsdr/src/p2.rs:98` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `running` | `bool` | Already connected to a host (byte 4 = 0x03). |
| `mac` | `[u8; 6]` | MAC address (bytes 5..10). |
| `board` | `Board` | Board. |
| `protocol_version` | `u8` | Protocol version implemented, decimal/10 (byte 12). |
| `firmware` | `u8` | Firmware code version (byte 13). |
| `num_ddcs` | `u8` | Independent DDCs implemented (byte 20). |
| `phase_word` | `bool` | Frequencies are sent as phase words (byte 21 = 1) rather than hertz. |
| `data_modes` | `u8` | Supported endian/data-format bits (byte 22; 0 = big-endian 3-byte only). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_reply` (src/p2.rs:120), `encode_reply` (src/p2.rs:148).

**See also:** test `discovery_and_reply_round_trip (src/p2.rs)`.

### `DdcSetting` [@ANCHOR: hpsdr:hpsdr_p2_ddc_setting]

**Purpose and location.** One DDC's settings (P2, "DDC Specific Packet"). Defined in `daemons/hpsdr/src/p2.rs:184` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `enabled` | `bool` | Send this DDC's data. |
| `adc` | `u8` | ADC index it listens to. |
| `rate` | `Rate` | Sample rate. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `ddcs` (src/session_p2.rs:96).

**See also:** test `ddc_specific_sets_enables_rates_and_adcs (src/p2.rs)`.

### `HpRx` [@ANCHOR: hpsdr:hpsdr_p2_hp_rx]

**Purpose and location.** What the receive high-priority packet carries. Defined in `daemons/hpsdr/src/p2.rs:232` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `run` | `bool` | Set the run bit (byte 4 bit 0). |
| `ddc_hz` | `Vec<u32>` | DDC frequencies in hertz (DDC 0 first). |
| `atten_adc0` | `u8` | Step attenuator before ADC0, 0..=31 dB (byte 1443). |
| `atten_adc1` | `u8` | Step attenuator before ADC1, 0..=31 dB (byte 1442). |
| `phase_clock_hz` | `Option<f64>` | `Some(clock_hz)` to send phase words (the radio's discovery reply said so), `None` for hertz. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `hp_base` (src/p2.rs:247), `encode_high_priority_rx` (src/p2.rs:270), `hp` (src/p2.rs:445), `hp_rx` (src/session_p2.rs:153).

**See also:** test `receive_high_priority_never_carries_transmit_state (src/p2.rs)`.

### `TxState` [@ANCHOR: hpsdr:hpsdr_p2_tx_state]

**Purpose and location.** Transmit part of a High Priority packet. Defined in `daemons/hpsdr/src/p2.rs:276` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `ptt` | `bool` | Key the transmitter (byte 4 bit 1, "PTT0"). |
| `duc_hz` | `u32` | DUC0 frequency in hertz. |
| `drive` | `u8` | DUC0 drive level 0..=255 (byte 345). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `transmit_packets_are_refused_while_the_gate_is_closed (src/p2.rs)`.

### `DdcPacket` [@ANCHOR: hpsdr:hpsdr_p2_ddc_packet]

**Purpose and location.** One DDC data packet (P2, "DDC Packet", single DDC, 24 bits). Defined in `daemons/hpsdr/src/p2.rs:388` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Sequence number (per DDC port). |
| `timestamp` | `u64` | 64-bit sample-count timestamp. |
| `samples` | `Vec<Complex>` | Samples. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_ddc_packet` (src/p2.rs:399).

**See also:** test `status_and_ddc_packets_round_trip (src/p2.rs)`.

### `SessionOptions` [@ANCHOR: hpsdr:hpsdr_session_session_options]

**Purpose and location.** Tunables of a session. Defined in `daemons/hpsdr/src/session.rs:30` as a pub `struct`. It derives Debug, Clone, Copy.

| Field | Type | Meaning |
|---|---|---|
| `tick` | `Duration` | Interval between EP2 datagrams (C&C refresh and transmit I/Q pacing). Unverified: a real radio may want datagrams paced to the 48 ksps transmit sample clock (126 samples, 2.6 ms). |
| `stream_depth` | `usize` | Blocks buffered per receiver before the oldest-unread policy drops new ones. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `two_receivers_stream_their_own_tones_without_loss (src/session.rs)`.

### `TxState` [@ANCHOR: hpsdr:hpsdr_session_tx_state]

**Purpose and location.** The transmit request the Protocol 1 session task reads on each tick: key flag, drive level and queued I/Q. Defined in `daemons/hpsdr/src/session.rs:50` as a private `struct`. It derives Debug, Default.

| Field | Type | Meaning |
|---|---|---|
| `ptt` | `bool` | The caller has asked to key; the task still needs an open gate to act on it, and clears a stale request. |
| `drive` | `u8` | Requested drive level 0..=255 (capped by the gate's limit). |
| `iq` | `VecDeque<(i16, i16)>` | Queue of (I, Q) 16-bit transmit samples waiting to be sent. |

**Invariants, ownership and concurrency.** Held in `Shared.tx` behind a mutex; the caller sets it and the task drains it. Locked with the poison-tolerant `lock` helper because the gate, not the lock, decides on transmit.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `transmit_packets_are_refused_while_the_gate_is_closed (src/p2.rs)`.

### `Shared` [@ANCHOR: hpsdr:hpsdr_session_shared]

**Purpose and location.** State shared between a Protocol 1 `Radio` handle and its session task. Defined in `daemons/hpsdr/src/session.rs:57` as a private `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `cfg` | `Mutex<P1Config>` | Current `P1Config` (frequencies, rate, attenuation), behind a mutex. |
| `status` | `Mutex<HardwareStatus>` | Latest `HardwareStatus` decoded from received frames, behind a mutex. |
| `tx` | `Mutex<TxState>` | The pending transmit request, behind a mutex. |
| `closing` | `AtomicBool` | Set when the Radio is dropped without `stop()`; the task sends one unkeyed datagram and the Stop packet, then ends. |

**Invariants, ownership and concurrency.** Held in an `Arc` by both sides. Locks are taken one at a time with the poison-tolerant `lock` helper; `next_datagram` clones `cfg` and then holds `tx`; no lock is held across an await.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `an_open_gate_keys_and_the_lease_expiry_unkeys (src/session.rs)`.

### `Radio` [@ANCHOR: hpsdr:hpsdr_session_radio]

**Purpose and location.** A running Protocol 1 receive session. Call [`Radio::stop`]; a plain drop unkeys and stops the radio too, but only from the task's next tick and without reporting a failure. Defined in `daemons/hpsdr/src/session.rs:78` as a pub `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `shared` | `Arc<Shared>` | State shared with the session task. |
| `sock` | `Arc<UdpSocket>` | The UDP socket to the radio. |
| `task` | `Option<JoinHandle<()>>` | The session task handle; `None` after `stop`. |
| `streams` | `Vec<ChannelStream>` | One `ChannelStream` per receiver. |
| `gate` | `TxGate` | The transmit gate (closed unless the caller opened it). |
| `num_rx` | `u8` | Number of receivers streaming. |

**Invariants, ownership and concurrency.** Owned by the caller; one per radio. Not `Clone`. `stop().await` is the orderly end; a drop makes the task unkey and stop on its next tick.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `two_receivers_stream_their_own_tones_without_loss (src/session.rs)`.

### `P2Config` [@ANCHOR: hpsdr:hpsdr_session_p2_p2_config]

**Purpose and location.** What the session asks the radio to do. Settings the protocol cannot express are rejected. Defined in `daemons/hpsdr/src/session_p2.rs:39` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `rate` | `Rate` | DDC sample rate (48 to 1536 ksps), the same for every receiver. |
| `rx_hz` | `Vec<u32>` | NCO frequency of each receiver in hertz; its length is the receiver count (1..=8). |
| `num_adc` | `u8` | ADCs the radio has (1 for a Hermes). |
| `att_adc0` | `u8` | Step attenuator before ADC0, 0..=31 dB. |
| `att_adc1` | `u8` | Step attenuator before ADC1, 0..=31 dB. |
| `dither` | `bool` | ADC dither. |
| `random` | `bool` | ADC random. |
| `tx_hz` | `u32` | Transmit NCO frequency in hertz. |
| `ports` | `RadioPorts` | The radio's UDP port map (sent in the General packet). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `hp_rx` (src/session_p2.rs:153), `update` (src/session_p2.rs:428), `cfg_with` (src/session_p2.rs:591).

**See also:** test `frequency_rate_and_attenuator_changes_reach_the_radio (src/session_p2.rs)`.

### `TxQueue` [@ANCHOR: hpsdr:hpsdr_session_p2_tx_queue]

**Purpose and location.** The transmit request the Protocol 2 session task reads on each tick. Defined in `daemons/hpsdr/src/session_p2.rs:109` as a private `struct`. It derives Debug, Default.

| Field | Type | Meaning |
|---|---|---|
| `ptt` | `bool` | The caller asked to key; ignored by the task unless the gate is open. |
| `drive` | `u8` | Requested DUC0 drive 0..=255. |
| `iq` | `VecDeque<(i16, i16)>` | Queue of (I, Q) transmit samples. |

**Invariants, ownership and concurrency.** Held in `Shared.tx` behind a mutex; same role as the Protocol 1 `TxState`.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `an_open_gate_keys_and_the_lease_expiry_unkeys (src/session_p2.rs)`.

### `Shared` [@ANCHOR: hpsdr:hpsdr_session_p2_shared]

**Purpose and location.** State shared between a Protocol 2 `Radio2` handle and its session task. Defined in `daemons/hpsdr/src/session_p2.rs:116` as a private `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `cfg` | `Mutex<(P2Config, u64)>` | The `P2Config` paired with a version number; the task resends the DDC-specific packet when the version it last sent differs. Behind a mutex. |
| `status` | `Mutex<HardwareStatus>` | Latest `HardwareStatus` decoded from received status, behind a mutex. |
| `tx` | `Mutex<TxQueue>` | The pending transmit request, behind a mutex. |
| `stray` | `AtomicU64` | Count of received datagrams whose source port is neither a DDC stream port nor the status port (read with `stray_datagrams`). |
| `closing` | `AtomicBool` | Set when the Radio2 is dropped without `stop()`; the task sends one unkeyed packet with the run bit clear and ends. |

**Invariants, ownership and concurrency.** Held in an `Arc` by both sides; atomics for counters, mutexes for the rest, taken with the poison-tolerant `lock` helper and not held across an await.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `lost_datagrams_and_a_slow_consumer_are_counted (src/session_p2.rs)`.

### `Seqs` [@ANCHOR: hpsdr:hpsdr_session_p2_seqs]

**Purpose and location.** Sequence numbers of the four packet kinds the session sends. Defined in `daemons/hpsdr/src/session_p2.rs:136` as a private `struct`. It derives Debug, Default.

| Field | Type | Meaning |
|---|---|---|
| `general` | `u32` | Sequence number of the next General packet. |
| `ddc` | `u32` | Next DDC-specific packet sequence number. |
| `hp` | `u32` | Next High Priority packet sequence number. |
| `duc` | `u32` | Next transmit I/Q (DUC) packet sequence number. |

**Invariants, ownership and concurrency.** Owned and incremented by the session task; each packet kind has its own counter, as Protocol 2 specifies.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `two_receivers_stream_their_own_tones_without_loss (src/session_p2.rs)`.

### `Target` [@ANCHOR: hpsdr:hpsdr_session_p2_target]

**Purpose and location.** Where the session sends and what it computes with. Defined in `daemons/hpsdr/src/session_p2.rs:145` as a private `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `ip` | `IpAddr` | The radio's IP address. |
| `cmd_port` | `u16` | The radio's command port. |
| `clock_hz` | `Option<f64>` | The radio's reference clock for phase words, `Some` only when the discovery reply said the radio wants phase words. |

**Invariants, ownership and concurrency.** Cloned into the task and the `Radio2` handle; read-only after creation.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `hp_rx` (src/session_p2.rs:153).

**See also:** test `frequency_rate_and_attenuator_changes_reach_the_radio (src/session_p2.rs)`.

### `Task` [@ANCHOR: hpsdr:hpsdr_session_p2_task]

**Purpose and location.** The fixed inputs of the Protocol 2 session task, bundled so `run` takes one argument. Defined in `daemons/hpsdr/src/session_p2.rs:266` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `sock` | `Arc<UdpSocket>` | The UDP socket shared with the `Radio2`. |
| `shared` | `Arc<Shared>` | State shared with the handle. |
| `gate` | `TxGate` | The transmit gate that decides whether a key request is honoured. |
| `target` | `Target` | Where to send. |
| `senders` | `Vec<mpsc::Sender<IqBlock>>` | One bounded-channel sender per receiver. |
| `counters` | `Vec<Arc<StreamCounters>>` | One counters object per receiver. |
| `tick` | `Duration` | Interval at which command packets are sent. |
| `seqs` | `Seqs` | Per-kind sequence numbers, mutated by the task. |

**Invariants, ownership and concurrency.** Moved into the spawned task by `Radio2::start` and owned by it until the task ends.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `run` (src/session_p2.rs:280).

**See also:** test `a_whole_receive_session_with_a_closed_gate_never_transmits (src/session_p2.rs)`.

### `Radio2` [@ANCHOR: hpsdr:hpsdr_session_p2_radio2]

**Purpose and location.** A running Protocol 2 receive session. Call [`Radio2::stop`]; a plain drop unkeys and stops the radio too, but only from the task's next tick and without reporting a failure. Defined in `daemons/hpsdr/src/session_p2.rs:331` as a pub `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `shared` | `Arc<Shared>` | State shared with the session task. |
| `sock` | `Arc<UdpSocket>` | The UDP socket to the radio. |
| `task` | `Option<JoinHandle<()>>` | The session task handle; `None` after `stop`. |
| `streams` | `Vec<ChannelStream>` | One `ChannelStream` per receiver. |
| `gate` | `TxGate` | The transmit gate. |
| `target` | `Target` | Where the radio is. |
| `num_rx` | `usize` | Number of receivers (DDCs) streaming. |

**Invariants, ownership and concurrency.** Owned by the caller; not `Clone`. Call `stop().await`; a drop is handled on the next tick.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `two_receivers_stream_their_own_tones_without_loss (src/session_p2.rs)`.

### `Protocol` [@ANCHOR: hpsdr:hpsdr_types_protocol]

**Purpose and location.** Which wire protocol a radio speaks. Defined in `daemons/hpsdr/src/types.rs:13` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `P1` | `P1` | Metis/USB-framed protocol over UDP port 1024 (Hermes, ANAN-10, ANAN-100 original firmware). |
| `P2` | `P2` | The newer "openHPSDR Ethernet Protocol" with one UDP port per stream. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `discovery_and_reply_round_trip (src/p2.rs)`.

### `Board` [@ANCHOR: hpsdr:hpsdr_types_board]

**Purpose and location.** A board type as reported by discovery. Protocol 1 and 2 number boards differently; both map here. Defined in `daemons/hpsdr/src/types.rs:22` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Metis` | `Metis` | Metis or Atlas based system. |
| `Hermes` | `Hermes` | Hermes: ANAN-10, ANAN-100. |
| `HermesB` | `HermesB` | Hermes variant: ANAN-10E, ANAN-100B (Protocol 2 board type 2). |
| `Angelia` | `Angelia` | Angelia: ANAN-100D. |
| `Orion` | `Orion` | Orion: ANAN-200D. |
| `OrionMk2` | `OrionMk2` | Orion Mk II: ANAN-7000DLE, ANAN-8000DLE. |
| `HermesLite` | `HermesLite` | Hermes Lite. |
| `Saturn` | `Saturn` | Saturn: ANAN-G2. |
| `Other` | `Other(u8)` | An identifier this crate does not know (kept, never rejected). |

**Invariants, ownership and concurrency.** Plain `Copy` value. `Other(u8)` keeps an unknown identifier instead of rejecting the radio.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `board_ids_map_and_unknown_is_kept (src/types.rs)`.

### `Rate` [@ANCHOR: hpsdr:hpsdr_types_rate]

**Purpose and location.** A DDC sample rate. Protocol 1 supports the first four; Protocol 2 all six (spec v4.4 DDC Specific bytes 18-19: 48/96/192/384/768/1536 ksps). Defined in `daemons/hpsdr/src/types.rs:100` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `K48` | `K48` | 48 ksps. |
| `K96` | `K96` | 96 ksps. |
| `K192` | `K192` | 192 ksps. |
| `K384` | `K384` | 384 ksps. |
| `K768` | `K768` | 768 ksps (Protocol 2 only). |
| `K1536` | `K1536` | 1536 ksps (Protocol 2 only). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `cfg_with` (src/session_p2.rs:591).

**See also:** test `rates_round_trip (src/types.rs)`.

### `HardwareStatus` [@ANCHOR: hpsdr:hpsdr_types_hardware_status]

**Purpose and location.** Hardware status gathered from Protocol 1 command-and-control bytes or the Protocol 2 high-priority status packet. Raw ADC counts are kept; the methods convert them.  Temperature is deliberately absent: neither the Protocol 1 USB spec v1.60 nor the Protocol 2 spec v4.4 (Appendix A, Hermes) defines a temperature report for ANAN/Hermes hardware. Defined in `daemons/hpsdr/src/types.rs:175` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq, Default.

| Field | Type | Meaning |
|---|---|---|
| `adc_overload` | `[bool; 4]` | ADC overload flags, ADC0 first. |
| `adc_max_magnitude` | `[u16; 2]` | Largest ADC magnitude seen in the last status period, per ADC (Protocol 2 only). |
| `ptt_input` | `bool` | Front-panel/PTT input active (not a transmit state: a request from the radio's own jack). |
| `dot_input` | `bool` | CW dot input active. |
| `dash_input` | `bool` | CW dash input active. |
| `exciter_raw` | `u16` | Exciter forward power, raw 12-bit ADC count. |
| `forward_raw` | `u16` | PA forward power, raw 12-bit ADC count. |
| `reverse_raw` | `u16` | PA reverse power, raw 12-bit ADC count. |
| `supply_raw` | `u16` | Supply voltage, raw 12-bit ADC count. |
| `firmware` | `u8` | Firmware version byte as reported. |
| `pll_locked` | `Option<bool>` | Reference PLL locked (Protocol 2 only). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `apply_status_cc` (src/p1.rs:478), `encode_status_cc` (src/p1.rs:505), `parse_hp_status` (src/p2.rs:342), `encode_hp_status` (src/p2.rs:367), `status` (src/session.rs:274), `status` (src/session_p2.rs:450).

**See also:** test `power_and_swr_from_counts (src/types.rs)`.

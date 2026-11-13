# Data structures

### `NetStats` [@ANCHOR: radioctl:radioctl_core_net_net_stats]

**Purpose and location.** Counters of a network link (the same shape as a serial link's). Defined in `daemons/radioctl/crates/radioctl-core/src/net.rs:50` as a pub `type`. It derives no traits.

**Definition.** `pub type NetStats = SerialStats;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `snapshot` (src/net.rs:273), `stats` (src/net.rs:305), `stats` (src/net.rs:344).

**See also:** test `a_virtual_flexradio_is_read_through_the_network_link (tests/net.rs)`.

### `NetProtocol` [@ANCHOR: radioctl:radioctl_core_net_net_protocol]

**Purpose and location.** Which transport protocol the link uses. Defined in `daemons/radioctl/crates/radioctl-core/src/net.rs:54` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Tcp` | `Tcp` | A TCP connection, reconnected when it is lost. |
| `Udp` | `Udp` | A connected UDP socket: one datagram per write and per read. UDP has no connection to lose, so loss is detected by an ICMP error from the stack or by silence ([`NetConfig::idle_timeout`]), and "reconnect" means opening a fresh socket and repeating the greeting. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `proto_name` (src/net.rs:720), `read_size` (src/net.rs:728).

**See also:** test `config_defaults_are_bounded (src/net.rs)`.

### `NetConfig` [@ANCHOR: radioctl:radioctl_core_net_net_config]

**Purpose and location.** How to reach a rig over the network and keep the link. Defined in `daemons/radioctl/crates/radioctl-core/src/net.rs:66` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `address` | `String` | `host:port`. A name is resolved again at every connect. |
| `protocol` | `NetProtocol` | TCP or UDP. |
| `connect_timeout` | `Duration` | Longest wait for one connection attempt. |
| `handshake_timeout` | `Duration` | Longest wait for the framing's handshake to finish after the socket is up. |
| `write_timeout` | `Duration` | Longest wait for one write to the socket. |
| `idle_timeout` | `Option<Duration>` | Silence on the wire after which the link is declared lost (`None`: never). Set it with a keep-alive, or a quiet rig is reconnected for nothing. Required for UDP loss detection. |
| `backoff` | `Backoff` | Waits between reconnect attempts. |
| `first_attempts` | `u32` | Attempts for the first connection (at least one); a failure after that is an error to the caller. Later losses are retried without limit. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/net.rs:91), `open` (src/net.rs:363), `fast` (tests/net.rs:34).

**See also:** test `config_defaults_are_bounded (src/net.rs)`.

### `Decoded` [@ANCHOR: radioctl:radioctl_core_net_decoded]

**Purpose and location.** What a framing produced from bytes read off the wire. Defined in `daemons/radioctl/crates/radioctl-core/src/net.rs:107` as a pub `struct`. It derives Debug, Clone, Default, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `payload` | `Vec<u8>` | Bytes for the rig actor (the CAT stream). |
| `reply` | `Vec<u8>` | Bytes to send straight back to the radio (a login answer, a pong). |
| `ready` | `bool` | The handshake is complete: the link may be declared up. |
| `failure` | `Option<String>` | The handshake failed or the stream is corrupt: drop the connection and reconnect. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `decode` (src/net.rs:130), `decode` (src/net.rs:163), `decode` (src/net.rs:226), `decode` (tests/net.rs:336).

**See also:** test `a_login_handshake_gates_the_link_and_a_refusal_fails_it (tests/net.rs)`.

### `RawFraming` [@ANCHOR: radioctl:radioctl_core_net_raw_framing]

**Purpose and location.** No framing: the CAT (computer-aided transceiver control) stream is the wire stream, nothing is greeted, nothing is kept alive. Defined in `daemons/radioctl/crates/radioctl-core/src/net.rs:141` as a pub `struct`. It derives Debug, Clone, Copy, Default.

**Fields.** none (unit or opaque definition).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `raw_framing_is_the_identity (src/net.rs)`.

### `LengthFraming` [@ANCHOR: radioctl:radioctl_core_net_length_framing]

**Purpose and location.** Frames of a 2 or 4 byte big-endian length followed by that many bytes. Each write from the actor becomes one frame; the payload of every complete frame read is passed up. A length over [`MAX_FRAME`] is a failure (the stream is out of step), never an allocation. Defined in `daemons/radioctl/crates/radioctl-core/src/net.rs:180` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `width` | `usize` | The width of the length prefix in bytes (2 or 4). |
| `pending` | `Vec<u8>` | Bytes received but not yet a whole frame. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/net.rs:188).

**See also:** test `length_framing_round_trips_and_reassembles (src/net.rs)`.

### `Counters` [@ANCHOR: radioctl:radioctl_core_net_counters]

**Purpose and location.** The atomic counters of a network link, shared by the supervisor and the `NetMonitor`. Defined in `daemons/radioctl/crates/radioctl-core/src/net.rs:265` as a private `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `bytes_in` | `AtomicU64` | Bytes read from the wire. |
| `bytes_out` | `AtomicU64` | Bytes written to the wire. |
| `bytes_dropped` | `AtomicU64` | Bytes that could not be delivered (for example a full channel or a lost connection). |
| `reconnects` | `AtomicU64` | Reconnections made after the first connect. |

**Invariants, ownership and concurrency.** Relaxed atomics updated by the supervisor task and read by the owner without a lock.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `snapshot` (src/net.rs:273), `snapshot` (src/serial.rs:418), `snapshot` (src/usb_link.rs:85).

**See also:** test `small_helpers (src/net.rs)`.

### `NetLink` [@ANCHOR: radioctl:radioctl_core_net_net_link]

**Purpose and location.** A running network link: the byte stream for the actor and the link status. Defined in `daemons/radioctl/crates/radioctl-core/src/net.rs:283` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `transport` | `DuplexStream` | The byte stream to hand to [`crate::rig::spawn`]. Dropping it closes the socket. |
| `status` | `watch::Receiver<LinkStatus>` | The link status watch (connecting, connected, reconnecting and so on). |
| `counters` | `Arc<Counters>` | The shared counters. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_virtual_flexradio_is_read_through_the_network_link (tests/net.rs)`.

### `NetMonitor` [@ANCHOR: radioctl:radioctl_core_net_net_monitor]

**Purpose and location.** The part of a [`NetLink`] that stays with the owner: the link status and the counters. Defined in `daemons/radioctl/crates/radioctl-core/src/net.rs:324` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `status` | `watch::Receiver<LinkStatus>` | The link status watch. |
| `counters` | `Arc<Counters>` | The shared counters. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `into_parts` (src/net.rs:312).

**See also:** test `keepalive_is_sent_and_silence_drops_the_link (tests/net.rs)`.

### `Wire` [@ANCHOR: radioctl:radioctl_core_net_wire]

**Purpose and location.** The open socket of one session. Defined in `daemons/radioctl/crates/radioctl-core/src/net.rs:355` as a private `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Tcp` | `Tcp(TcpStream)` | A TCP stream session. |
| `Udp` | `Udp(UdpSocket)` | A UDP socket session (one datagram per write). |

**Invariants, ownership and concurrency.** Plain data: its fields hold UdpSocket (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `open` (src/net.rs:363), `open_and_handshake` (src/net.rs:568), `send` (src/net.rs:603).

**See also:** test `udp_carries_one_datagram_per_write_and_detects_silence (tests/net.rs)`.

### `SessionEnd` [@ANCHOR: radioctl:radioctl_core_net_session_end]

**Purpose and location.** Why a session ended. Defined in `daemons/radioctl/crates/radioctl-core/src/net.rs:424` as a private `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Shutdown` | `Shutdown` | The actor dropped its end: stop for good. |
| `Lost` | `Lost(String)` | The wire failed: reconnect; the text says why. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `session` (src/serial.rs:616), `session` (src/usb_link.rs:277).

**See also:** test `keepalive_is_sent_and_silence_drops_the_link (tests/net.rs)`.

### `Supervisor` [@ANCHOR: radioctl:radioctl_core_net_supervisor]

**Purpose and location.** The task state of a network link: it connects, runs a session, and reconnects with backoff after a loss. Defined in `daemons/radioctl/crates/radioctl-core/src/net.rs:431` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `config` | `NetConfig` | The network configuration (address, protocol, timeouts and backoff). |
| `framing` | `Box<dyn Framing>` | The framing that wraps and unwraps the byte stream (raw, length-prefixed or a login session). |
| `status` | `watch::Sender<LinkStatus>` | The sender of the link status. |
| `counters` | `Arc<Counters>` | The shared counters. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_first_connection_gives_up_after_its_attempts (tests/net.rs)`.

### `AttemptEnd` [@ANCHOR: radioctl:radioctl_core_net_attempt_end]

**Purpose and location.** How an attempt ended without a wire. Defined in `daemons/radioctl/crates/radioctl-core/src/net.rs:712` as a private `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Shutdown` | `Shutdown` | The actor dropped its end: stop for good. |
| `Failed` | `Failed(String)` | The attempt failed; the text says why. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_first_connection_gives_up_after_its_attempts (tests/net.rs)`.
### `DataBits` [@ANCHOR: radioctl:radioctl_core_serial_data_bits]

**Purpose and location.** Data bits per character. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:54` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Seven` | `Seven` | Seven. |
| `Eight` | `Eight` | Eight. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_vendor_descriptions_seven_even_two_line_reaches_the_port (tests/transports.rs)`.

### `Parity` [@ANCHOR: radioctl:radioctl_core_serial_parity]

**Purpose and location.** Parity. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:63` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `None` | `None` | None. |
| `Odd` | `Odd` | Odd. |
| `Even` | `Even` | Even. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_vendor_descriptions_seven_even_two_line_reaches_the_port (tests/transports.rs)`.

### `StopBits` [@ANCHOR: radioctl:radioctl_core_serial_stop_bits]

**Purpose and location.** Stop bits. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:74` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `One` | `One` | One. |
| `Two` | `Two` | Two. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_vendor_descriptions_seven_even_two_line_reaches_the_port (tests/transports.rs)`.

### `Flow` [@ANCHOR: radioctl:radioctl_core_serial_flow]

**Purpose and location.** Flow control. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:83` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `None` | `None` | None. |
| `Hardware` | `Hardware` | RTS/CTS. Do not combine with RTS keying. |
| `Software` | `Software` | XON/XOFF characters in the data stream. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `rts_cannot_key_or_carry_data_on_a_port_whose_rts_is_flow_control (tests/transports.rs)`.

### `LineControl` [@ANCHOR: radioctl:radioctl_core_serial_line_control]

**Purpose and location.** Whether the port has modem control lines this library may use. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:94` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Managed` | `Managed` | DTR and RTS exist: they are made inactive (or left at the configured idle level) after every open, and a failure to do that fails the open. This is the default and the safe choice for anything that could be wired to a transmitter. |
| `Unmanaged` | `Unmanaged` | The port has no modem lines (a pseudo-terminal, a CDC ACM gadget, a network bridge): they are never touched and [`SerialControl::set_line`] reports them unsupported. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_port_without_modem_lines_cannot_be_opened_as_managed_and_is_honest_when_unmanaged (tests/transports.rs)`.

### `PortTarget` [@ANCHOR: radioctl:radioctl_core_serial_port_target]

**Purpose and location.** Where the port is. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:106` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Path` | `Path(PathBuf)` | A device path such as `/dev/ttyUSB0` (or a symlink to one). |
| `Usb` | `Usb(UsbMatcher)` | Found by USB ids at every (re)connect. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/serial.rs:144), `resolve` (src/serial.rs:438).

**See also:** test `the_serial_probe_port_reports_a_missing_device_and_closes_idempotently (tests/baud_probe.rs)`.

### `SerialConfig` [@ANCHOR: radioctl:radioctl_core_serial_serial_config]

**Purpose and location.** How to open and keep a serial port. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:115` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `target` | `PortTarget` | The port. |
| `baud` | `u32` | Baud rate. |
| `data_bits` | `DataBits` | Data bits. |
| `parity` | `Parity` | Parity. |
| `stop_bits` | `StopBits` | Stop bits. |
| `flow` | `Flow` | Flow control. |
| `lines` | `LineControl` | Whether DTR and RTS are managed. |
| `idle_dtr` | `bool` | Level DTR is left at after opening (`false` is inactive). |
| `idle_rts` | `bool` | Level RTS is left at after opening (`false` is inactive). |
| `backoff` | `Backoff` | Waits between reconnect attempts. |
| `first_open_attempts` | `u32` | Attempts for the first open (at least one); a failure after that is an error to the caller. Later disconnects are retried without limit. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/baud_probe.rs:180), `new` (src/serial.rs:144), `with_description_line` (src/serial.rs:165), `open_port` (src/serial.rs:449), `open_serial` (src/serial.rs:699), `fast_config` (tests/transports.rs:48).

**See also:** test `the_serial_probe_port_reports_a_missing_device_and_closes_idempotently (tests/baud_probe.rs)`.

### `LinkStatus` [@ANCHOR: radioctl:radioctl_core_serial_link_status_type]

**Purpose and location.** The state of the physical link. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:221` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Connected` | `Connected { port: PathBuf, generation: u32, }` | The port is open. `generation` counts opens, starting at 1. The device path in use. How many times the port has been opened. |
| `Reconnecting` | `Reconnecting { attempt: u32, last_error: String, }` | The port is gone; the supervisor is trying to reopen it. Failed attempts since the last connection. Why the last attempt failed. |
| `Closed` | `Closed` | The supervisor has stopped (the byte stream was dropped). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `status` (src/net.rs:301), `watch` (src/net.rs:307), `status` (src/net.rs:341), `watch` (src/net.rs:347), `link_changed` (src/rig.rs:2069), `status` (src/serial.rs:357), `watch` (src/serial.rs:363), `status` (src/serial.rs:401).

**See also:** test `a_port_loss_takes_the_link_down_and_a_reopen_unkeys_before_serving_the_queue (tests/actor.rs)`.

### `SerialStats` [@ANCHOR: radioctl:radioctl_core_serial_serial_stats]

**Purpose and location.** Counters of a serial link. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:242` as a pub `struct`. It derives Debug, Clone, Copy, Default, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `bytes_in` | `u64` | Bytes read from the port. |
| `bytes_out` | `u64` | Bytes written to the port. |
| `bytes_dropped` | `u64` | Bytes written while the port was down and therefore dropped. |
| `reconnects` | `u64` | Successful reconnects (not counting the first open). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `stats` (src/serial.rs:369), `stats` (src/serial.rs:413), `snapshot` (src/serial.rs:418).

**See also:** test `a_split_link_keeps_status_and_counters_on_the_monitor (tests/transports.rs)`.

### `Counters` [@ANCHOR: radioctl:radioctl_core_serial_counters]

**Purpose and location.** The atomic counters of a serial link, shared by the supervisor and the `SerialMonitor`. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:254` as a private `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `bytes_in` | `AtomicU64` | Bytes read from the port. |
| `bytes_out` | `AtomicU64` | Bytes written to the port. |
| `bytes_dropped` | `AtomicU64` | Bytes that could not be delivered. |
| `reconnects` | `AtomicU64` | Reopens made after a loss of the port. |

**Invariants, ownership and concurrency.** Relaxed atomics updated by the supervisor thread and read by the owner without a lock.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `snapshot` (src/net.rs:280), `snapshot` (src/serial.rs:418), `snapshot` (src/usb_link.rs:85).

**See also:** test `small_helpers (src/net.rs)`.

### `SharedPort` [@ANCHOR: radioctl:radioctl_core_serial_shared_port]

**Purpose and location.** The open port shared between the supervisor and the control handle: a mutex around an optional boxed serial port (`None` while closed). Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:261` as a private `type`. It derives no traits.

**Definition.** `type SharedPort = Arc<Mutex<Option<Box<dyn SerialPort>>>>;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_split_link_keeps_status_and_counters_on_the_monitor (tests/transports.rs)`.

### `SerialControl` [@ANCHOR: radioctl:radioctl_core_serial_serial_control]

**Purpose and location.** Immediate access to the control lines of the port. Clone it freely. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:271` as a pub `struct`. It derives Clone.

| Field | Type | Meaning |
|---|---|---|
| `port` | `SharedPort` | The shared port. |
| `lines` | `LineControl` | How the control lines may be used (managed or not). |
| `flow` | `Flow` | The flow control in force; RTS cannot be used as a key or data line when it is flow control. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `rts_cannot_key_or_carry_data_on_a_port_whose_rts_is_flow_control (tests/transports.rs)`.

### `SerialLink` [@ANCHOR: radioctl:radioctl_core_serial_serial_link]

**Purpose and location.** A running serial link: the byte stream for the actor, the control lines, the link status. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:345` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `transport` | `DuplexStream` | The byte stream to hand to [`crate::rig::spawn`]. Dropping it closes the port. |
| `control` | `SerialControl` | The control lines. |
| `status` | `watch::Receiver<LinkStatus>` | The link status watch. |
| `counters` | `Arc<Counters>` | The shared counters. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `open_serial` (src/serial.rs:699).

**See also:** test `a_split_link_keeps_status_and_counters_on_the_monitor (tests/transports.rs)`.

### `SerialMonitor` [@ANCHOR: radioctl:radioctl_core_serial_serial_monitor]

**Purpose and location.** The part of a [`SerialLink`] that stays with the owner after the byte stream went to the actor: the control lines, the link status and the counters. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:391` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `control` | `SerialControl` | The control lines. |
| `status` | `watch::Receiver<LinkStatus>` | The link status watch. |
| `counters` | `Arc<Counters>` | The shared counters. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `into_parts` (src/serial.rs:377).

**See also:** test `a_split_link_keeps_status_and_counters_on_the_monitor (tests/transports.rs)`.

### `Command` [@ANCHOR: radioctl:radioctl_core_serial_command]

**Purpose and location.** A command from the owner to the supervisor thread. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:427` as a private `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Write` | `Write(Vec<u8>)` | Write these bytes to the port. |
| `Shutdown` | `Shutdown` | Close the port and end the thread. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `serve` (src/rig.rs:1391).

**See also:** test `a_split_link_keeps_status_and_counters_on_the_monitor (tests/transports.rs)`.

### `Supervisor` [@ANCHOR: radioctl:radioctl_core_serial_supervisor]

**Purpose and location.** The thread state of a serial link: it opens the port, runs a session, and reopens after a loss. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:503` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `config` | `SerialConfig` | The serial configuration. |
| `commands` | `std_mpsc::Receiver<Command>` | The channel of commands from the owner. |
| `to_actor` | `mpsc::Sender<Vec<u8>>` | The channel that carries received bytes to the rig actor. |
| `status` | `watch::Sender<LinkStatus>` | The sender of the link status. |
| `shared` | `SharedPort` | The shared open port. |
| `counters` | `Arc<Counters>` | The shared counters. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, mpsc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_split_link_keeps_status_and_counters_on_the_monitor (tests/transports.rs)`.

### `SessionEnd` [@ANCHOR: radioctl:radioctl_core_serial_session_end]

**Purpose and location.** Why one serial session ended: the owner shut the link down, or the port was lost. Defined in `daemons/radioctl/crates/radioctl-core/src/serial.rs:512` as a private `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Shutdown` | `Shutdown` | The owner asked for shutdown: stop for good. |
| `Lost` | `Lost(String)` | The port failed: reopen; the text says why. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `session` (src/serial.rs:616), `session` (src/usb_link.rs:277).

**See also:** test `a_split_link_keeps_status_and_counters_on_the_monitor (tests/transports.rs)`.
### `UsbTarget` [@ANCHOR: radioctl:radioctl_core_usb_device_usb_target]

**Purpose and location.** How to find one USB device. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_device.rs:27` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `vendor_id` | `u16` | USB vendor id. |
| `product_id` | `u16` | USB product id. |
| `manufacturer` | `String` | The manufacturer string the device must report; empty matches any. |
| `product` | `String` | The product string the device must report; empty matches any. |
| `serial` | `Option<String>` | The serial number string the device must report; `None` matches any (two identical boards on one host need it). |
| `interface` | `Option<u8>` | The interface to claim after opening (FUNcube: 2), when the device has one to claim. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `open` (src/usb_device.rs:150), `open` (src/usb_device.rs:232), `open` (src/usb_libusb.rs:61), `new` (src/usb_link.rs:67), `target` (tests/usb_link.rs:28), `fast` (tests/usb_link.rs:39).

**See also:** test `an_absent_device_is_reported_as_not_plugged_in (tests/usb_libusb.rs)`.

### `UsbFailure` [@ANCHOR: radioctl:radioctl_core_usb_device_usb_failure]

**Purpose and location.** Why one transfer failed. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_device.rs:76` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Timeout` | `Timeout` | The device did not answer in time. |
| `NoDevice` | `NoDevice` | The device is gone (unplugged, reset, permission revoked): the link must reopen it. |
| `Stall` | `Stall` | The device refused the request (a stall, "pipe" error): the link stays up. |
| `Other` | `Other(String)` | Anything else, with the backend's wording. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `failure` (src/usb_libusb.rs:38), `status_of` (src/usb_wire.rs:359), `bulk_write` (tests/usb_link.rs:92), `bulk_read` (tests/usb_link.rs:95), `interrupt_write` (tests/usb_link.rs:98), `interrupt_read` (tests/usb_link.rs:101).

**See also:** test `libusb_errors_map_to_what_the_link_does_about_them (tests/usb_libusb.rs)`.

### `FakeTransfer` [@ANCHOR: radioctl:radioctl_core_usb_device_fake_transfer]

**Purpose and location.** A transfer a [`FakeUsbBackend`] device received, as the tests inspect it. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_device.rs:161` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `ControlIn` | `ControlIn(u8, u8, u16, u16, u16)` | `control_in`: request type, request, value, index, length. |
| `ControlOut` | `ControlOut(u8, u8, u16, u16, Vec<u8>)` | `control_out`: request type, request, value, index, data. |
| `BulkWrite` | `BulkWrite(u8, Vec<u8>)` | `bulk_write`: endpoint, data. |
| `BulkRead` | `BulkRead(u8, usize)` | `bulk_read`: endpoint, length. |
| `InterruptWrite` | `InterruptWrite(u8, Vec<u8>)` | `interrupt_write`: endpoint, data. |
| `InterruptRead` | `InterruptRead(u8, usize)` | `interrupt_read`: endpoint, length. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `log` (src/usb_device.rs:225).

**See also:** test `the_fake_bus_logs_transfers_and_fails_when_unplugged (tests/usb_link.rs)`.

### `FakeShared` [@ANCHOR: radioctl:radioctl_core_usb_device_fake_shared]

**Purpose and location.** The state shared by the fake USB backend and its handles. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_device.rs:176` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `present` | `AtomicBool` | Whether the device is plugged in. |
| `opens` | `AtomicU32` | How many times it was opened. |
| `log` | `Mutex<Vec<FakeTransfer>>` | Every transfer made, behind a mutex. |
| `device` | `Mutex<Box<dyn UsbDevice>>` | The simulated device, behind a mutex. |

**Invariants, ownership and concurrency.** Plain data: its fields hold AtomicBool, AtomicU32, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_fake_bus_logs_transfers_and_fails_when_unplugged (tests/usb_link.rs)`.

### `FakeUsbBackend` [@ANCHOR: radioctl:radioctl_core_usb_device_fake_usb_backend]

**Purpose and location.** A backend that serves one in-memory device (a virtual twin, or a scripted fake) that can be unplugged and plugged back in. Clone it: all clones are the same bus. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_device.rs:186` as a pub `struct`. It derives Clone.

| Field | Type | Meaning |
|---|---|---|
| `shared` | `Arc<FakeShared>` | The shared state. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/usb_device.rs:194), `softrock` (tests/usb_link.rs:50).

**See also:** test `the_fake_bus_logs_transfers_and_fails_when_unplugged (tests/usb_link.rs)`.

### `FakeHandle` [@ANCHOR: radioctl:radioctl_core_usb_device_fake_handle]

**Purpose and location.** An open handle on the fake device. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_device.rs:246` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `shared` | `Arc<FakeShared>` | The shared state. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_fake_bus_logs_transfers_and_fails_when_unplugged (tests/usb_link.rs)`.
### `LibusbBackend` [@ANCHOR: radioctl:radioctl_core_usb_libusb_libusb_backend]

**Purpose and location.** Opens devices through libusb. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_libusb.rs:21` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `context` | `Context` | The libusb context. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/usb_libusb.rs:28).

**See also:** test `an_absent_device_is_reported_as_not_plugged_in (tests/usb_libusb.rs)`.

### `LibusbDevice` [@ANCHOR: radioctl:radioctl_core_usb_libusb_libusb_device]

**Purpose and location.** An open libusb device. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_libusb.rs:122` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `handle` | `DeviceHandle<Context>` | The libusb device handle. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_read_returns_only_the_bytes_that_arrived (tests/usb_libusb.rs)`.
### `UsbStats` [@ANCHOR: radioctl:radioctl_core_usb_link_usb_stats]

**Purpose and location.** Counters of a USB link (the same shape as a serial link's: bytes are frame bytes). Defined in `daemons/radioctl/crates/radioctl-core/src/usb_link.rs:49` as a pub `type`. It derives no traits.

**Definition.** `pub type UsbStats = SerialStats;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `snapshot` (src/usb_link.rs:85), `stats` (src/usb_link.rs:117), `stats` (src/usb_link.rs:157).

**See also:** test `a_rig_tunes_over_the_usb_link_and_garbage_does_not_wedge_it (tests/usb_link.rs)`.

### `UsbConfig` [@ANCHOR: radioctl:radioctl_core_usb_link_usb_config_type]

**Purpose and location.** How to reach a USB device and keep the link. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_link.rs:53` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `target` | `UsbTarget` | Which device. |
| `transfer_timeout` | `Duration` | Longest wait for one transfer. |
| `backoff` | `Backoff` | Waits between reopen attempts. |
| `first_open_attempts` | `u32` | Attempts for the first open (at least one); later losses are retried without limit. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/usb_link.rs:67), `fast` (tests/usb_link.rs:39).

**See also:** test `a_rig_over_the_link_and_the_fake_bus_sets_up_and_tunes (../radioctl-protocols/tests/elektor507.rs)`.

### `Counters` [@ANCHOR: radioctl:radioctl_core_usb_link_counters]

**Purpose and location.** The atomic counters of a USB link (bytes are frame bytes). Defined in `daemons/radioctl/crates/radioctl-core/src/usb_link.rs:78` as a private `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `bytes_in` | `AtomicU64` | Bytes read from the device. |
| `bytes_out` | `AtomicU64` | Bytes written to the device. |
| `bytes_dropped` | `AtomicU64` | Bytes that could not be delivered. |
| `reconnects` | `AtomicU64` | Reopens after an unplug. |

**Invariants, ownership and concurrency.** Relaxed atomics updated by the supervisor and read by the owner without a lock.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `snapshot` (src/net.rs:280), `snapshot` (src/serial.rs:432), `snapshot` (src/usb_link.rs:85).

**See also:** test `small_helpers (src/net.rs)`.

### `UsbLink` [@ANCHOR: radioctl:radioctl_core_usb_link_usb_link]

**Purpose and location.** A running USB link: the byte stream for the actor and the link status. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_link.rs:95` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `transport` | `DuplexStream` | The byte stream to hand to [`crate::rig::spawn`]. Dropping it closes the device. |
| `status` | `watch::Receiver<LinkStatus>` | The link status watch. |
| `counters` | `Arc<Counters>` | The shared counters. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_rig_tunes_over_the_usb_link_and_garbage_does_not_wedge_it (tests/usb_link.rs)`.

### `UsbMonitor` [@ANCHOR: radioctl:radioctl_core_usb_link_usb_monitor]

**Purpose and location.** The part of a [`UsbLink`] that stays with the owner after the stream went to the actor. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_link.rs:137` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `status` | `watch::Receiver<LinkStatus>` | The link status watch. |
| `counters` | `Arc<Counters>` | The shared counters. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `into_parts` (src/usb_link.rs:125).

**See also:** test `the_first_open_fails_cleanly_when_the_device_is_not_there (tests/usb_link.rs)`.

### `Command` [@ANCHOR: radioctl:radioctl_core_usb_link_command_type]

**Purpose and location.** A command from the owner to the supervisor. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_link.rs:162` as a private `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Write` | `Write(Vec<u8>)` | Write these bytes as frames to the device. |
| `Shutdown` | `Shutdown` | Close the device and end the thread. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `serve` (src/rig.rs:1391).

**See also:** test `a_rig_tunes_over_the_usb_link_and_garbage_does_not_wedge_it (tests/usb_link.rs)`.

### `SessionEnd` [@ANCHOR: radioctl:radioctl_core_usb_link_session_end]

**Purpose and location.** Why one USB session ended: the owner shut the link down, or the device was lost. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_link.rs:171` as a private `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Shutdown` | `Shutdown` | The owner asked for shutdown: stop for good. |
| `Lost` | `Lost(String)` | The device failed: reopen with backoff; the text says why. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `session` (src/serial.rs:633), `session` (src/usb_link.rs:277).

**See also:** test `the_fake_bus_logs_transfers_and_fails_when_unplugged (tests/usb_link.rs)`.

### `Supervisor` [@ANCHOR: radioctl:radioctl_core_usb_link_supervisor]

**Purpose and location.** The task state of a USB link: opens the device through the backend and reopens after an unplug. Defined in `daemons/radioctl/crates/radioctl-core/src/usb_link.rs:176` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `backend` | `Arc<dyn UsbBackend>` | The USB backend (libusb or a fake). |
| `config` | `UsbConfig` | The USB configuration. |
| `commands` | `std_mpsc::Receiver<Command>` | The channel of commands from the owner. |
| `to_actor` | `mpsc::Sender<Vec<u8>>` | The channel that carries received frames to the rig actor. |
| `status` | `watch::Sender<LinkStatus>` | The sender of the link status. |
| `counters` | `Arc<Counters>` | The shared counters. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, mpsc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_first_open_fails_cleanly_when_the_device_is_not_there (tests/usb_link.rs)`.
### `TransmitLease` [@ANCHOR: radioctl:radioctl_core_rig_easy_transmit_lease]

**Purpose and location.** The transmit gate held open for a bounded time. Created by [`RigHandle::transmit_lease`]. Dropping it ends the lease: the gate closes and the rig is unkeyed. Defined in `daemons/radioctl/crates/radioctl-core/src/rig/easy.rs:148` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `rig` | `RigHandle` | The rig whose gate this lease holds open. |
| `deadline` | `Instant` | When the lease ends and the gate is closed again. |
| `timer` | `JoinHandle<()>` | The task that closes the gate at the deadline. |
| `ended` | `bool` | Set once the lease was ended, so it is not ended twice. |

**Invariants, ownership and concurrency.** Plain data: its fields hold JoinHandle (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `transmit_for_is_cut_short_by_the_lease (tests/easy.rs)`.
### `Expect` [@ANCHOR: radioctl:radioctl_core_transcript_expect]

**Purpose and location.** What the engine must report for the rig's answer. Defined in `daemons/radioctl/crates/radioctl-core/src/transcript.rs:14` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Observation` | `Observation(Observation)` | A value. |
| `Ack` | `Ack` | The rig accepted the command. |
| `Nak` | `Nak` | The rig refused the command. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `build_expect` (src/transcript.rs:245).

**See also:** test `a_transcript_parses_into_typed_steps (src/transcript.rs)`.

### `Panel` [@ANCHOR: radioctl:radioctl_core_transcript_panel]

**Purpose and location.** A knob turned at the radio itself (an unsolicited step). Defined in `daemons/radioctl/crates/radioctl-core/src/transcript.rs:25` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Frequency` | `Frequency(Hz)` | VFO A moved. |
| `Mode` | `Mode(Mode)` | The mode changed. |
| `Ptt` | `Ptt(bool)` | The PTT (push to talk) switch moved. |
| `Signal` | `Signal(u16)` | The signal strength changed. |
| `Split` | `Split(bool)` | Split was switched on or off. |
| `RitOffset` | `RitOffset(i16)` | The RIT/XIT offset was turned to this many hertz. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `build_panel` (src/transcript.rs:278).

**See also:** test `a_transcript_parses_into_typed_steps (src/transcript.rs)`.

### `Step` [@ANCHOR: radioctl:radioctl_core_transcript_step]

**Purpose and location.** One exchange. Defined in `daemons/radioctl/crates/radioctl-core/src/transcript.rs:42` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `op` | `Option<Op>` | The command, if the step starts with one. |
| `tx` | `Vec<u8>` | Bytes the controller writes; empty for an unsolicited step. |
| `rx` | `Vec<u8>` | Bytes the rig answers; empty when it does not answer. |
| `expect` | `Option<Expect>` | What the engine must decode from `rx` (the first item, if `rx` decodes to several). |
| `expect_more` | `Vec<Expect>` | Further items `rx` must decode to, after `expect`, in order (an Elecraft `IF` answer carries frequency, mode, RIT, split and more at once). |
| `panel` | `Option<Panel>` | For an unsolicited step, the knob that makes the virtual rig send `rx`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_transcript_parses_into_typed_steps (src/transcript.rs)`.

### `Transcript` [@ANCHOR: radioctl:radioctl_core_transcript_transcript]

**Purpose and location.** A transcript. Defined in `daemons/radioctl/crates/radioctl-core/src/transcript.rs:63` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `rig` | `String` | The rig description id it applies to. |
| `origin` | `String` | Where the bytes came from. |
| `steps` | `Vec<Step>` | The exchanges, in order, against one rig that starts in its default state. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse` (src/transcript.rs:292).

**See also:** test `a_transcript_parses_into_typed_steps (src/transcript.rs)`.

### `Doc` [@ANCHOR: radioctl:radioctl_core_transcript_doc]

**Purpose and location.** The TOML shape of a transcript file: a schema number, the rig, where the transcript came from, and the steps. Defined in `daemons/radioctl/crates/radioctl-core/src/transcript.rs:74` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `schema` | `u32` | The file schema version. |
| `rig` | `String` | The rig description id the transcript is for. |
| `origin` | `String` | Where the transcript was recorded or written (provenance text). |
| `step` | `Vec<StepSpec>` | The `[[step]]` tables. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `text_steps_and_the_extended_vocabulary_parse (src/transcript.rs)`.

### `StepSpec` [@ANCHOR: radioctl:radioctl_core_transcript_step_spec]

**Purpose and location.** One `[[step]]` table: an optional operation, the bytes written and answered, and what the engine must decode. Defined in `daemons/radioctl/crates/radioctl-core/src/transcript.rs:83` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `op` | `Option<OpSpec>` | The operation that starts the step, if any. |
| `tx` | `String` | The bytes the controller writes (hex or text). |
| `rx` | `String` | The bytes the rig answers (hex or text). |
| `tx_text` | `Option<String>` | The bytes written, as ASCII text (for text protocols); an alternative to `tx`. |
| `rx_text` | `Option<String>` | The bytes received, as ASCII text; an alternative to `rx`. |
| `expect` | `Option<ExpectSpec>` | What must be decoded from `rx`. |
| `expect_more` | `Vec<ExpectSpec>` | Further items decoded from the same `rx`. |
| `panel` | `Option<PanelSpec>` | A front-panel change instead of a command. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `text_steps_and_the_extended_vocabulary_parse (src/transcript.rs)`.

### `OpSpec` [@ANCHOR: radioctl:radioctl_core_transcript_op_spec]

**Purpose and location.** An operation as written in a transcript file. Defined in `daemons/radioctl/crates/radioctl-core/src/transcript.rs:101` as a private `enum`. It derives Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `GetFrequency` | `GetFrequency { vfo: String }` | Read a VFO's frequency. |
| `SetFrequency` | `SetFrequency { vfo: String, hz: u64 }` | Set a VFO's frequency in hertz. |
| `GetMode` | `GetMode` | Read the mode. |
| `SetMode` | `SetMode { mode: String }` | Set the mode by name. |
| `GetPtt` | `GetPtt` | Read the transmit state. |
| `SetPtt` | `SetPtt { on: bool }` | Key or unkey. |
| `GetSignal` | `GetSignal` | Read the signal meter. |
| `GetPower` | `GetPower` | Read the power meter. |
| `SendCw` | `SendCw { text: String }` | Send Morse text. |
| `SetTransceive` | `SetTransceive { on: bool }` | Switch the rig's transceive (unsolicited reports) on or off. |
| `GetSplit` | `GetSplit` | Read split. |
| `SetSplit` | `SetSplit { on: bool }` | Set split on or off. |
| `GetRit` | `GetRit` | Read RIT (receiver incremental tuning) on or off. |
| `SetRit` | `SetRit { on: bool }` | Set RIT on or off. |
| `GetXit` | `GetXit` | Read XIT (transmitter incremental tuning) on or off. |
| `SetXit` | `SetXit { on: bool }` | Set XIT on or off. |
| `GetRitOffset` | `GetRitOffset` | Read the RIT offset. |
| `SetRitOffset` | `SetRitOffset { hz: i16 }` | Set the RIT offset in hertz. |
| `GetFilterWidth` | `GetFilterWidth` | Read the filter width. |
| `SetFilterWidth` | `SetFilterWidth { hz: u64 }` | Set the filter width in hertz. |
| `GetLevel` | `GetLevel { level: String }` | Read a level by name. |
| `SetLevel` | `SetLevel { level: String, value: u16 }` | Set a level by name. |
| `GetSwr` | `GetSwr` | Read the standing-wave ratio. |
| `GetMeter` | `GetMeter { meter: String }` | Read a meter by name. |
| `GetModelId` | `GetModelId` | Read the model id. |
| `GetFirmware` | `GetFirmware` | Read the firmware version. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `build_op` (src/transcript.rs:213).

**See also:** test `text_steps_and_the_extended_vocabulary_parse (src/transcript.rs)`.

### `ExpectSpec` [@ANCHOR: radioctl:radioctl_core_transcript_expect_spec]

**Purpose and location.** What a transcript step expects to decode. Defined in `daemons/radioctl/crates/radioctl-core/src/transcript.rs:132` as a private `enum`. It derives Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Frequency` | `Frequency { vfo: String, hz: u64 }` | A VFO's frequency in hertz. |
| `Mode` | `Mode { mode: String }` | The mode by name. |
| `Ptt` | `Ptt { on: bool }` | The transmit state. |
| `Signal` | `Signal { value: u16 }` | A signal meter value. |
| `Power` | `Power { value: u16 }` | A power meter value. |
| `Split` | `Split { on: bool }` | Split on or off. |
| `RxVfo` | `RxVfo { vfo: String }` | The receive VFO. |
| `TxVfo` | `TxVfo { vfo: String }` | The transmit VFO. |
| `Rit` | `Rit { on: bool }` | RIT on or off. |
| `Xit` | `Xit { on: bool }` | XIT on or off. |
| `RitOffset` | `RitOffset { hz: i16 }` | The RIT offset in hertz. |
| `FilterWidth` | `FilterWidth { hz: u64 }` | The filter width in hertz. |
| `Level` | `Level { level: String, value: u16 }` | A level by name and value. |
| `Swr` | `Swr { value: u16 }` | The standing-wave ratio. |
| `Meter` | `Meter { meter: String, value: u16 }` | A meter by name and value. |
| `ModelId` | `ModelId { value: u16 }` | The model id. |
| `Firmware` | `Firmware { major: u8, minor: u8 }` | The firmware version text. |
| `Ack` | `Ack` | The rig acknowledged a set. |
| `Nak` | `Nak` | The rig refused a set. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `build_expect` (src/transcript.rs:245).

**See also:** test `text_steps_and_the_extended_vocabulary_parse (src/transcript.rs)`.

### `PanelSpec` [@ANCHOR: radioctl:radioctl_core_transcript_panel_spec]

**Purpose and location.** A front-panel change described in a transcript. Defined in `daemons/radioctl/crates/radioctl-core/src/transcript.rs:156` as a private `enum`. It derives Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Frequency` | `Frequency { hz: u64 }` | The frequency changed to, in hertz. |
| `Mode` | `Mode { mode: String }` | The mode changed to. |
| `Ptt` | `Ptt { on: bool }` | The transmit state changed to. |
| `Signal` | `Signal { value: u16 }` | A signal meter value. |
| `Split` | `Split { on: bool }` | Split changed to. |
| `RitOffset` | `RitOffset { hz: i16 }` | RIT offset changed to, in hertz. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `build_panel` (src/transcript.rs:278).

**See also:** test `text_steps_and_the_extended_vocabulary_parse (src/transcript.rs)`.

# hpsdr

A Rust client for OpenHPSDR radios (ANAN-10, ANAN-100, Hermes, and the other boards of the family), built
for the relay SDR plan (`hams_com/docs/proposals/SDR_RECEIVE_ENGINE_AND_TRANSMIT_PATH.md` sections 2 to 4)
and the transmit safety baseline (`RELAY_SDR_RADIOCTL_ROADMAP.md` section 2a). Prototype: **everything here is
`verified=unverified`** until it has run against a real radio. No hardware was used to write it.

## Status

| Piece | State |
|---|---|
| `p1`: Protocol 1 discovery, start/stop, EP2 builder, EP6 decoder, status C&C | written, unit-tested |
| `gate`: transmit gate (closed by default, bounded lease) | written, unit-tested |
| `iq`: `IqStream` trait, blocks with sequence numbers, drop counters | written, unit-tested |
| `types`: boards, sample rates, power/supply/SWR conversion | written, unit-tested |
| `discovery`: UDP broadcast or unicast discovery (Protocol 1) | written, tested against the emulator |
| `session`: configure, start, stream several DDC receivers, status, stop; gated PTT and transmit I/Q queue | written, tested against the emulator |
| `emulator`: loopback-only Protocol 1 server with synthetic tones, status, loss injection and transmit counters | written |
| `p2`: Protocol 2 discovery, General, DDC-specific, High Priority (receive and gated transmit), gated DUC I/Q, HP status and DDC packet parsers | written, unit-tested by round trip only |
| Protocol 2 session and emulator | **not written**: Protocol 2 has no network-level test yet |
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
* **Transmit fails closed.** `TxGate::default()` is closed. `Ep2Builder::build_rx` has no MOX, drive or
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

Known open points (all `verified=unverified`): Protocol 2 sends each DDC stream from its own source port to the host port of the discovery; whether a real radio delivers them all to one host socket (this crate's planned demultiplexing by source port) or to per-stream destination ports is not settled by the text; whether a radio streams EP6 while receiving EP2 only at the pacing used
here; whether Rx8 is addressable in Protocol 1 (v1.60 lists NCO addresses for Rx1..Rx7 only); the I/Q swap on transmit
(USB v1.60 says the PC must swap them); the Protocol 2 text says "little Endian" for hardware to host in one place but
revision 1.3 fixed the order to big-endian, which is what is implemented.

Licence: AGPL-3.0-or-later. The toolchain is pinned in `rust-toolchain.toml` (as for `daemons/radioctl`).
Build and test: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`.

Anchors (ADR 0090): a function carries `// [@ANCHOR: id]` in source, its test carries
`// Tests [@ANCHOR: id]`, and this README cites it as `[@ANCHOR: hpsdr:id]` below.

- [@ANCHOR: hpsdr:hpsdr_discovery_broadcast_target] (discovery) The limited broadcast address and discovery port.
- [@ANCHOR: hpsdr:hpsdr_discovery_parse_reply] (discovery) Classify and parse one reply datagram received from `from`.
- [@ANCHOR: hpsdr:hpsdr_discovery_udp_socket] (discovery) Open a non-blocking UDP socket registered with the runtime behind `handle`.
- [@ANCHOR: hpsdr:hpsdr_discovery_discover] (discovery) Send the discovery request to every address in `targets` and collect replies for `wait`. Use
- [@ANCHOR: hpsdr:hpsdr_emulator_config_default] (emulator) A Hermes with one -6 dBFS tone at 7.1005 MHz, fast pacing.
- [@ANCHOR: hpsdr:hpsdr_emulator_start_p1] (emulator) Bind `127.0.0.1:port` (0 picks a free port) and serve Protocol 1 on `handle`.
- [@ANCHOR: hpsdr:hpsdr_emulator_addr] (emulator) The loopback address it listens on.
- [@ANCHOR: hpsdr:hpsdr_emulator_counters] (emulator) What it has seen so far.
- [@ANCHOR: hpsdr:hpsdr_emulator_stop] (emulator) Stop serving and release the socket.
- [@ANCHOR: hpsdr:hpsdr_emulator_counters_get] (emulator) Read one counter.
- [@ANCHOR: hpsdr:hpsdr_emulator_synth] (emulator) Samples of one receiver: `count` 24-bit (I, Q) pairs starting at sample index `start`, the sum of
- [@ANCHOR: hpsdr:hpsdr_emulator_handle_datagram] (emulator) Act on one datagram from a client; returns the reply to send to `from`, if any.
- [@ANCHOR: hpsdr:hpsdr_emulator_next_ep6] (emulator) Build the next EP6 datagram from the current settings; `n` is the sample index of its first sample.
- [@ANCHOR: hpsdr:hpsdr_emulator_run_p1] (emulator) The emulator task: answer datagrams and, after a Start, send EP6 datagrams at `cfg.pace`.
- [@ANCHOR: hpsdr:hpsdr_error_fmt] (error) Display text of a rejection.
- [@ANCHOR: hpsdr:hpsdr_error_from_io] (error) Wraps an I/O error.
- [@ANCHOR: hpsdr:hpsdr_gate_new] (gate) A closed gate.
- [@ANCHOR: hpsdr:hpsdr_gate_open_until] (gate) Open the gate until `deadline`; returns false (and leaves it closed) if the lock is poisoned.
- [@ANCHOR: hpsdr:hpsdr_gate_close] (gate) Close the gate now.
- [@ANCHOR: hpsdr:hpsdr_gate_is_open] (gate) True only while a lease granted by `open_until` has not expired at `now`.
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
- [@ANCHOR: hpsdr:hpsdr_p2_discovery_packet] (p2) Discovery request: sequence 0, command 0x02, zeros (P2, "Discovery Packet"). Send to port 1024.
- [@ANCHOR: hpsdr:hpsdr_p2_parse_reply] (p2) Parse a discovery reply. Replies with board type 254/255 (XML or full description) are reported
- [@ANCHOR: hpsdr:hpsdr_p2_encode_reply] (p2) Build a discovery reply (emulator side). The board type byte is the P2 number of `board_type`.
- [@ANCHOR: hpsdr:hpsdr_p2_encode_general] (p2) The General packet (command 0): the port map, the hardware reset timer enabled (byte 38 bit 0, so
- [@ANCHOR: hpsdr:hpsdr_p2_encode_ddc_specific] (p2) The DDC Specific packet (to the DDC-specific port): number of ADCs, dither, random, per-DDC enable
- [@ANCHOR: hpsdr:hpsdr_p2_phase_word] (p2) `2^32 * f / clock`, rounded (P2, "General Packet" byte 37 text).
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

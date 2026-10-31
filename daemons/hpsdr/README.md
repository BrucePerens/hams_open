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

Known open points (all `verified=unverified`): whether a radio streams EP6 while receiving EP2 only at the pacing used
here; whether Rx8 is addressable in Protocol 1 (v1.60 lists NCO addresses for Rx1..Rx7 only); the I/Q swap on transmit
(USB v1.60 says the PC must swap them); the Protocol 2 text says "little Endian" for hardware to host in one place but
revision 1.3 fixed the order to big-endian, which is what is implemented.

Licence: AGPL-3.0-or-later. The toolchain is pinned in `rust-toolchain.toml` (as for `daemons/radioctl`).
Build and test: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`.

Anchors (ADR 0090): a function carries `// [@ANCHOR: id]` in source, its test carries
`// Tests [@ANCHOR: id]`, and this README cites it as `[@ANCHOR: hpsdr:id]` below.

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

# elad_usb

A receive-only Rust driver for ELAD (an Italian maker of software-defined radio receivers) USB receivers (FDM-DUO, FDM-S2, FDM-S1), written from public descriptions of the protocol
and checked against one real FDM-S2. Used by the relay (`hams_com/daemons/hams_local_relay/src/elad_source.rs`) and by
`bench_mcp` (`src/elad/`). The design is `hams_com/docs/proposals/ELAD_RECEIVER_RELAY_AND_BENCH.md`.

## Status (verified=unverified except where a row says otherwise)

| Piece | State |
|---|---|
| Discovery by USB id (1721:061a DUO, 061c S2, 0610 S1) | **verified on an FDM-S2, 2026-11-08** |
| Descriptors: configuration 1, interface 0 vendor class, bulk OUT 0x02, bulk IN 0x86 (512 bytes), high speed | **verified on an FDM-S2** |
| Control reads: firmware version (`0xFF`), EEPROM (`0xA2`, index `0x0151`): serial, hardware version, rate correction, dB offsets | **verified on an FDM-S2** (fw 3.0, hw 1.3; plausible values; fixture `tests/fixtures/fdm_s2_fw3_0_open.txt`) |
| Tune word, S2 front-end code, FIFO (first-in, first-out sample buffer) start/stop | written from public descriptions, tested against the fake only |
| I/Q format (32-bit little-endian below 6.144 Msps, 16-bit at 6.144 Msps; I then Q) | written from public descriptions; **not decoded from real samples yet** |
| FDM-S1/S2 FPGA (programmable logic chip) image | **not loaded by this software, by decision**: see below |
| FDM-DUO | no unit available; nothing verified |

**The S1/S2 FPGA.** A bus-powered FDM-S1/S2 enumerates and answers control reads with no FPGA image, but streams no samples (a real
FDM-S2: three 500 ms bulk reads, zero bytes). Public sources say the FPGA must be programmed from ELAD's own, non-redistributable image
(one per sample rate) by ELAD's loader, `elad-firmware`. This crate contains, downloads and runs none of that. `Radio::start` refuses an
S1/S2 unless the caller states the FPGA was loaded by other means (`FpgaState::LoadedExternally`). The question for the owner is
`hams_com/night_shift_questions/open/elad-fpga-image-for-fdm-s2-*.md`. An FDM-DUO loads its own FPGA and needs nothing.

## Receive only

`protocol::is_receive_request` is the whole list of what can go on the wire; `Radio` refuses anything else. No CAT (computer control of the radio) write, no PTT (push-to-talk, transmit keying), no
EEPROM write. A test runs every model through every public method and checks each request sent.

## Using it

`cargo run --example probe` (read-only discovery and descriptors of the unit), `cargo run --release --example capture -- --freq 7100000
--rate 192000 --seconds 3 --out /tmp/x.cf32 [--fpga-loaded]` (receive only; prints measured rate, timeouts, power and CPU).
Install `udev/70-elad-fdm.rules` for permanent device access (`sudo cp udev/70-elad-fdm.rules /etc/udev/rules.d/ && sudo udevadm
control --reload && sudo udevadm trigger`).

## Provenance

Protocol facts (request codes, tuning-word formula, S2 front-end codes, EEPROM map, sample widths) come from the public READMEs, issue
text and source summaries of two GPL-3.0 projects, SDRoxide (`crates/sdroxide-elad`) and SoapyELAD, which in turn credit ELAD's own
`gr-elad`; facts (numbers, formulas) were taken, no code was copied, and the code here was written independently. Descriptors, firmware
version, hardware version and EEPROM values were observed on the owner's unit. Nothing here comes from vendor source, firmware or
manual text; ELAD's `gr-elad` and loader were not downloaded or read.

## Function reference

Each entry is a semantic anchor in the source with a test citing it (`// Tests [@ANCHOR: ...]`). Nothing in this crate transmits: every request that can reach the unit is on the receive-only allow-list (see "Receive only" above), and no function here has a transmit gate to open. All device access goes through the `Transport` trait; tests use `FakeTransport`, and only `usb::find`, `UsbTransport::open` and the examples touch real hardware. Security: the USB ids and the lock directory are the only external inputs checked; there are no network calls, no secrets and no files other than the lock file. Calls into one `Radio` are serialised by `&mut self`.

### `Model::from_pid(pid: u16) -> Result<Model, Error>` (`protocol.rs`) [@ANCHOR: elad_usb:elad_model_from_pid]

Maps a USB product id to the model: `0x061a` FDM-DUO, `0x061c` FDM-S2, `0x0610` FDM-S1. Argument `pid` is the product id from the device descriptor (the vendor id `0x1721` is checked by the caller, `usb::find`). Any other id returns `Error::UnknownDevice { pid }`, so an unrelated device is never driven. Pure, no I/O. Tests: the model-table unit tests in `src/tests.rs`.

### `tuning_word(hz: f64, clock_hz: f64) -> u32` (`protocol.rs`) [@ANCHOR: elad_usb:elad_tuning_word]

Computes the 32-bit DDC (digital down-converter) tuning word for a frequency: `(hz mod clock_hz) / clock_hz * 2^32`, truncated to 32 bits. Arguments: `hz` the wanted centre frequency, `clock_hz` the model's ADC clock (`Model::adc_clock_hz`). It never fails: a negative, NaN or infinite `hz`, or a clock that is zero, negative or not finite, gives 0; a frequency above the clock aliases through the modulo. The range check against the receive range is done by `Radio::tune`, not here. Pure, no I/O. Tests: tuning-word unit tests in `src/tests.rs`, including the clamp and the alias.

### `tune_request_sampler(word: u32) -> Request` (`protocol.rs`) [@ANCHOR: elad_usb:elad_tune_request_sampler]

Builds the FDM-S1/S2 tune control request for a tuning word: `bRequest` `0xF2` (`REQ_TUNE`), `wValue` the low 16 bits of the word, `wIndex` the next 16 bits, and a two-byte data stage whose first byte is the top 8 bits (the second is 0). Argument `word` comes from `tuning_word`. Returns the `Request` value only; nothing is sent here, and `Radio::send` still checks it against the receive-only list. Pure. Tests: the request-layout unit tests in `src/tests.rs`.

### `tune_request_duo(word: u32) -> Request` (`protocol.rs`) [@ANCHOR: elad_usb:elad_tune_request_duo]

Builds the FDM-DUO tune request. The DUO takes it through its gateway request `0xE1` (`REQ_DUO_GATEWAY`): `wValue` is the low 16 bits of the word, `wIndex` carries the sub-command `0xF2` (`DUO_TUNE`) in its high byte and bits 16 to 23 of the word in its low byte, and the data stage is the top byte plus a zero. Argument `word` comes from `tuning_word`. Pure; nothing is sent. The gateway sub-command is what `is_receive_request` inspects, so a tune passes and unknown sub-commands do not. Tests: request-layout unit tests in `src/tests.rs`.

### `front_end_code(filters: bool, hz: f64, attenuator: bool) -> u8` (`protocol.rs`) [@ANCHOR: elad_usb:elad_front_end_code]

Computes the FDM-S2 front-end code written with request `0xF7`: filters bypassed gives `0x01`; filters in gives `0x03` below 61.44 MHz, `0x02` from 61.44 to below 122.88 MHz and `0x04` above; the attenuator ORs in `0x08`. Arguments: `filters` whether the band filter bank is in circuit, `hz` the tuned frequency (it picks the band), `attenuator` whether the attenuator is switched in. Total function, no errors, no I/O. `Radio::write_front_end` sends it only for a model that has the switches. Hardware note: the band edges and codes are from public descriptions and only the S2 is covered; verified=unverified for the codes themselves. Tests: front-end code unit tests in `src/tests.rs`.

### `is_receive_request(r: &Request) -> bool` (`protocol.rs`) [@ANCHOR: elad_usb:elad_is_receive_request]

The transmit-safety allow-list: true only for requests this crate may ever put on the wire. Allowed `bRequest` values are firmware version `0xFF`, EEPROM `0xA2`, tune `0xF2`, front end `0xF7` and FIFO `0xE9`, plus the DUO gateway `0xE1` when its sub-command (high byte of `wIndex`) is FIFO init, FIFO run, tune, tune read-back or status. Everything else, including any CAT write, PTT or unknown request, is false. Argument `r` is the request to test. Pure and total. This is the first of the two guards behind the receive-only claim (the second is `is_send_request`/`is_read_request` in `Radio`). Tests: a test runs every model through every public `Radio` method and checks each request sent against this list.

### `is_send_request(r: &Request) -> bool` (`protocol.rs`) [@ANCHOR: elad_usb:elad_is_send_request]

The control-OUT guard used by `Radio::send`: true when the request is on the receive-only list but is not an IN-only read. It rejects firmware version and EEPROM requests (`0xA2` is a Cypress EEPROM request whose direction decides read or write, so as an OUT it would be an EEPROM write) and the DUO tune-read and status sub-commands, which are IN-only. Argument `r` is the request. Pure and total. This is what makes an EEPROM write impossible even though `0xA2` is on the receive-only list. Tests: the guard unit tests in `src/tests.rs`, which try an EEPROM write and expect `NotReceiveOnly`.

### `is_read_request(request: u8) -> bool` (`protocol.rs`) [@ANCHOR: elad_usb:elad_is_read_request]

True for the two `bRequest` values that go out only as a control IN: firmware version `0xFF` and EEPROM `0xA2`. Argument `request` is the `bRequest` byte alone. Used by `read_request` to refuse any other control IN, and by `is_send_request` to exclude these from control OUT. The reason for the split: `0xA2` is the Cypress EEPROM request whose direction picks read or write, so it must never be sent as an OUT. Pure and total. Tests: guard unit tests in `src/tests.rs`.

### `Calibration::from_eeprom(fields: &[Vec<u8>; 6]) -> Calibration` (`protocol.rs`) [@ANCHOR: elad_usb:elad_calibration_from_eeprom]

Decodes the unit's EEPROM calibration from the six reads in `EEPROM_MAP` order: serial (32 bytes at 0x4000), hardware version (2 at 0x404C), rate correction (4 at 0x4024), and global, low-pass and attenuator dB offsets (4 each at 0x4028, 0x402C, 0x4030). The serial is cut at the first NUL, keeps only printable ASCII, is trimmed, and is absent when empty. The hardware version and the little-endian `i32` rate correction are absent when the field is short or all ones (unprogrammed flash); the three offsets are little-endian `f32`, absent when short or not finite (all ones is NaN). Never fails, never reads the device. Tests: calibration unit tests in `src/tests.rs`, including the fixture from a real FDM-S2.

### `Calibration::level_offset_db(&self, model, rate_hz, low_pass, attenuator) -> f64` (`protocol.rs`) [@ANCHOR: elad_usb:elad_level_offset_db]

The dB offset to add to the level of a normalised full-scale sample to estimate the level at the antenna input. It is the sum of 21.4 dB for an FDM-DUO, a rate term (6.0 at 384 and 768 kHz, 5.4 at 1.536 and 3.072 MHz, -0.7 at 6.144 MHz, 0 at 192 kHz or any other rate), the EEPROM global offset, the low-pass offset when `low_pass` is true, and the attenuator offset when `attenuator` is true; an absent EEPROM value counts as 0. Pure, no errors. The constants are from public descriptions and are verified=unverified against a real signal. `Radio::level_offset_db` calls it with the current front-end state. Tests: level-offset unit tests in `src/tests.rs`.

### `Format::for_rate(rate_hz: u32) -> Format` (`convert.rs`) [@ANCHOR: elad_usb:elad_format_for_rate]

Chooses the sample component width for a rate: 16-bit signed little-endian (`S16Le`) at 6,144,000 Hz or above, otherwise 32-bit signed little-endian (`S32Le`). Argument `rate_hz` is the stream rate in Hz. Pure and total; it does not check that the rate is one the model streams (`Radio::open` does that). The choice is from public descriptions and is not yet confirmed by decoding real samples. Tests: format unit tests in `src/tests.rs`.

### `Decoder::push(&mut self, bytes: &[u8], out: &mut Vec<f32>) -> usize` (`convert.rs`) [@ANCHOR: elad_usb:elad_decoder_push]

Converts wire bytes to interleaved I, Q floats scaled to plus or minus 1.0 (each component divided by the format's full scale) and appends them to `out`. Returns the number of I/Q pairs appended. A pair split across two bulk reads is held in an internal carry buffer and completed by the next call, so any chunking gives the same output; `Decoder::reset` drops the carry (done on `Radio::start`). Arguments: `bytes` the raw chunk, `out` the destination. Never fails and does no I/O. Not thread-safe by itself (needs `&mut`). Tests: decoder unit tests in `src/tests.rs` with split and odd-sized chunks.

### `FpgaState::ready_for(self, model: Model) -> bool` (`fpga.rs`) [@ANCHOR: elad_usb:elad_fpga_ready]

Says whether samples can be expected given the caller's statement about the FPGA image. `LoadedExternally` (the caller states someone else loaded the image; not checked) is ready for every model. `Unknown` is ready only for a model that loads its own FPGA, the FDM-DUO; an FDM-S1/S2 needs ELAD's vendor image, which this crate never contains, downloads or runs. `Radio::start` refuses with `FpgaImageRequired` when this is false. Pure and total. Hardware note: an S1/S2 with no image enumerates but streams zero bytes. Tests: FPGA gate unit tests in `src/tests.rs`.

### `Radio::open(t: T, model: Model, opts: OpenOptions) -> Result<Radio<T>, Error>` (`radio.rs`) [@ANCHOR: elad_usb:elad_radio_open]

Opens a unit over a `Transport` in read-only fashion. It checks that the model streams `opts.sample_rate_hz` (else `OutOfRange`), reads the 2-byte firmware version, then reads the six EEPROM fields and builds the `Calibration`. Only control INs are used (through `read_request`), so it works on an FDM-S1/S2 with no FPGA image. Arguments: `t` the transport (real, fake, locked or boxed), `model`, `opts` (the declared `FpgaState` and the rate). The returned radio starts with filters in, attenuator out, not tuned, not streaming. Errors: `OutOfRange`, or any transport error from a read. It does not start the stream. Tests: open unit tests in `src/tests.rs` and the fixture of a real FDM-S2.

### `Radio::tune(&mut self, hz: f64) -> Result<(), Error>` (`radio.rs`) [@ANCHOR: elad_usb:elad_radio_tune]

Sets the centre frequency. `hz` must be finite and inside the model's receive range (10 kHz to 54 MHz, 30 MHz for the S1); otherwise `OutOfRange` and nothing is sent. It computes the tuning word, sends the DUO gateway or sampler tune request, records `tuned_hz`, then calls `write_front_end` so an S2 gets the filter code for the new band; a failed send leaves `tuned_hz` unchanged only for the tune request itself. Receive-only: the request goes through `send` and its allow-list. Needs `&mut self`, so calls are serialised by the owner; USB control transfers have a 1 second timeout. Tests: tune unit tests in `src/tests.rs`, which check each request sent.

### `Radio::set_filters(&mut self, on: bool) -> Result<(), Error>` (`radio.rs`) [@ANCHOR: elad_usb:elad_radio_set_filters]

Switches the FDM-S2's band-filter bank in (`true`) or bypassed (`false`) while keeping the attenuator as it is. Calls `require_switches` first, so a model without the switches (DUO, S1) gets `OutOfRange` and nothing is sent, then delegates to `set_front_end`. Before the first `tune` it only records the state. Errors: `OutOfRange` or a transport error from the write. Receive-only. Tests: front-end unit tests in `src/tests.rs`.

### `Radio::set_front_end(&mut self, filters: bool, attenuator: bool) -> Result<(), Error>` (`radio.rs`) [@ANCHOR: elad_usb:elad_radio_set_front_end]

Sets the S2's band filters and attenuator together. A model without the switches is refused with `OutOfRange`. Before the first `tune` nothing is sent, because the code depends on the band: the state is recorded and `tune` writes it, so the receiver is never tuned at a gain the caller did not ask for. After a tune the code is written at once, and the new state is recorded only if the write succeeded, so a failed write leaves the old state and `level_offset_db` stays truthful. Arguments: `filters`, `attenuator` as above. Receive-only. Tests: front-end unit tests in `src/tests.rs`.

### `Radio::require_switches(&self, what: &str) -> Result<(), Error>` (`radio.rs`) [@ANCHOR: elad_usb:elad_radio_require_switches]

A private guard: Ok for a model with a switchable filter bank and attenuator (only the FDM-S2), otherwise `OutOfRange` with the text "the <model> has no switchable <what> here". Argument `what` names the feature for the message ("filters", "attenuator", "filters or attenuator"). Called first by `set_filters`, `set_attenuator` and `set_front_end` so an unsupported model never reaches the wire. Pure, no I/O. Tests: front-end unit tests for the DUO and S1 in `src/tests.rs`.

### `Radio::set_attenuator(&mut self, on: bool) -> Result<(), Error>` (`radio.rs`) [@ANCHOR: elad_usb:elad_radio_set_attenuator]

Switches the FDM-S2's attenuator in (`true`) or out while keeping the filter setting. Calls `require_switches` (so `OutOfRange` on a model without it, with nothing sent) and then `set_front_end`. Before the first `tune` the choice is only recorded; the attenuator bit is ORed into the front-end code afterwards and the level offset uses the EEPROM attenuator value. Errors: `OutOfRange` or a transport error. Receive-only. Tests: front-end unit tests in `src/tests.rs`.

### `Radio::start(&mut self) -> Result<(), Error>` (`radio.rs`) [@ANCHOR: elad_usb:elad_radio_start]

Starts the sample FIFO. First it asks `FpgaState::ready_for`; an S1/S2 with no declared FPGA image returns `FpgaImageRequired` and sends nothing. It then marks the radio as streaming before sending (so a start the unit accepted but reported failed is still followed by a stop), and sends the FIFO request: for the DUO a gateway FIFO init then FIFO run with value 1, for the S1/S2 request `0xE9` with value 1. Finally it resets the decoder's carry buffer. Errors: `FpgaImageRequired` or a transport error. Receive-only. Tests: start/stop unit tests in `src/tests.rs`.

### `Radio::stop(&mut self) -> Result<(), Error>` (`radio.rs`) [@ANCHOR: elad_usb:elad_radio_stop]

Stops the sample FIFO: the DUO gateway FIFO run with value 0, or request `0xE9` with value 0 on the S1/S2. It does nothing and returns Ok when the radio is not streaming, so it is safe to call twice. The streaming flag is cleared before the write, so a failed stop is not retried by `Drop`, which also calls `stop` and ignores its result; dropping a `Radio` therefore never leaves the FIFO running. Error: a transport error from the write. Receive-only. Tests: start/stop unit tests in `src/tests.rs`.

### `Radio::read(&mut self, out: &mut Vec<f32>, timeout: Duration) -> Result<usize, Error>` (`radio.rs`) [@ANCHOR: elad_usb:elad_radio_read]

Reads one bulk transfer (up to 4 x 12,288 bytes) with the given timeout, decodes it into `out` as interleaved I, Q floats and returns the pairs appended. Counters in `Stats` are updated: reads, bytes, samples, timeouts and empty reads. A timeout or a zero-byte read goes through `silence`: three of them in a row before any sample has ever arrived give `NoSamples` (an unloaded S1/S2 FPGA looks like this), otherwise `Timeout`. Any other transport error is returned as it is. Blocking: the call waits up to `timeout` inside libusb. Tests: read and counter unit tests in `src/tests.rs`.

### `usb::find() -> Result<Vec<Found>, Error>` (`usb.rs`, feature `usb`) [@ANCHOR: elad_usb:elad_usb_find]

Lists the ELAD receivers on the USB bus without opening any of them: it enumerates devices through libusb, keeps those with vendor id `0x1721` and a product id `Model::from_pid` knows, and returns each as a `Found` (bus, address, pid, model). Devices whose descriptor cannot be read are skipped silently. Returns `Error::Usb` if enumeration itself fails. Read-only: no interface is claimed, so it is safe while another program streams. Tests: none under `cargo test` (needs hardware); exercised by `examples/probe` and the ignored hardware-in-the-loop test `tests/hil.rs`.

### `UsbTransport::open(found: &Found) -> Result<UsbTransport, Error>` (`usb.rs`, feature `usb`) [@ANCHOR: elad_usb:elad_usb_open]

Opens and claims one listed receiver: it finds the device at the same bus and address, re-checks the descriptor ids with `is_found` (addresses are reused after an unplug), opens it, asks libusb to detach a kernel driver automatically, and claims interface 0. Errors: `Usb("the device is no longer on the bus")`, `Usb("...no longer the receiver that was found there")`, or a libusb error mapped by `err`. The claim is a kernel-level guard against a second program; the `DeviceLock` in `lock.rs` is the cooperative one and should be taken before opening. Control requests use a 1 second timeout. Tests: needs a real unit, so only `examples/probe`, `examples/capture` and `tests/hil.rs`, not `cargo test`.

### `is_found(vendor_id: u16, product_id: u16, found: &Found) -> bool` (`usb.rs`) [@ANCHOR: elad_usb:elad_usb_is_found]

True when a device's USB ids are those of the receiver that `find` listed: the vendor id must be `0x1721` and the product id must equal `found.pid`. `UsbTransport::open` calls it so that, if the receiver was unplugged and a different device took the same bus address, that other device is refused instead of opened and claimed. Arguments: the two ids read from the descriptor and the `Found` record. Pure. Tests: `only_the_receiver_that_was_found_matches` in `usb.rs`.

### `Format::component_bytes(self) -> usize` (`convert.rs`) [@ANCHOR: elad_usb:elad_format_component_bytes]

Returns the bytes per I or Q component of a sample format: 4 for `S32Le`, 2 for `S16Le`. A complete I/Q pair is twice that (8 or 4 bytes). `Decoder::push` uses it to find pair boundaries and to chunk the input. Pure, total, no I/O. Tests: format and decoder unit tests in `src/tests.rs`.

### `Format::full_scale(self) -> f32` (`convert.rs`) [@ANCHOR: elad_usb:elad_format_full_scale]

Returns the divisor that normalises a signed component to the range of plus or minus 1.0: 2,147,483,648 (2^31) for `S32Le` and 32,768 (2^15) for `S16Le`. `Decoder::push` multiplies each integer by its reciprocal, so a full-scale negative sample decodes to exactly -1.0. The level offset in `Calibration::level_offset_db` is applied to this normalised level. Pure and total. Tests: decoder unit tests in `src/tests.rs`.

### `impl Display for Error` (`lib.rs`) [@ANCHOR: elad_usb:elad_error_display]

The human-readable text of every `Error` variant, used in logs and by the relay's and bench tool's messages: an unknown USB product id (shown as hex), the FPGA-image-required message that tells the user the S1/S2 needs ELAD's own loader (which this software does not include or run) and to load it and say so or use an FDM-DUO, no samples delivered, USB read timed out, a refused non-receive-only request (shown as a hex request code), out of range, unreadable reply and USB error with their detail. Pure formatting, no I/O, cannot fail. Tests: the error-text unit tests in `src/tests.rs`.

### `Model::adc_clock_hz(self) -> f64` (`protocol.rs`) [@ANCHOR: elad_usb:elad_model_adc_clock_hz]

Returns the model's ADC clock in Hz: 61,440,000 for the FDM-S1 and 122,880,000 for the FDM-S2 and FDM-DUO. It is the denominator of `tuning_word` and the source of the band edges (61.44 and 122.88 MHz) used by `front_end_code`. Pure and total. The values are from public descriptions; verified=unverified except through the S2's plausible EEPROM readings. Tests: model-table unit tests in `src/tests.rs`.

### `Model::name(self) -> &'static str` (`protocol.rs`) [@ANCHOR: elad_usb:elad_model_name]

Returns the display name used in messages and in the relay: "FDM-DUO", "FDM-S2" or "FDM-S1". Pure, total, no I/O. The names appear in error text such as "the FDM-DUO has no switchable filters here" and in `OutOfRange` messages for rate and frequency, so tests that match error text depend on them. Tests: model-table unit tests in `src/tests.rs`.

### `Model::pid(self) -> u16` (`protocol.rs`) [@ANCHOR: elad_usb:elad_model_pid]

Returns the USB product id of the model: `0x061a` DUO, `0x061c` S2, `0x0610` S1; the inverse of `Model::from_pid`. Used to build a `Found` record in tests and by the udev rules file, which lists the same ids. Pure and total. The S2 id is verified on a real unit; the other two are from public descriptions. Tests: the round-trip test between `from_pid` and `pid` in `src/tests.rs`.

### `Model::rx_range_hz(self) -> (u64, u64)` (`protocol.rs`) [@ANCHOR: elad_usb:elad_model_rx_range_hz]

Returns the receive range in Hz as (lowest, highest): 10 kHz to 30 MHz for the FDM-S1 and 10 kHz to 54 MHz for the FDM-S2 and FDM-DUO. `Radio::tune` refuses a frequency outside it with `OutOfRange`. The bounds are inclusive. Pure and total. The numbers are from public descriptions, not measured. Tests: tune range unit tests in `src/tests.rs`.

### `Model::sample_rates_hz(self) -> &'static [u32]` (`protocol.rs`) [@ANCHOR: elad_usb:elad_model_sample_rates_hz]

Returns the I/Q sample rates in Hz the model streams: 192, 384, 768 kHz, 1.536 and 3.072 MHz for the S1 and the DUO, plus 6.144 MHz for the S2. The DUO's rate is set by the unit and cannot be changed by software; for it the list is the set it can be configured to, and `Radio::open` still requires the caller's rate to be in the list. Pure and total. Tests: open and rate unit tests in `src/tests.rs`.

### `Radio::send(&mut self, r: &Request) -> Result<(), Error>` (`radio.rs`, crate-private) [@ANCHOR: elad_usb:elad_radio_send]

The single funnel for every control OUT: it refuses with `NotReceiveOnly(bRequest)` unless `is_send_request` accepts the request, and only then calls the transport's `control_out`. No code path in the crate writes to the device without passing through it, which is what makes the driver receive-only by construction (no CAT write, no PTT, no EEPROM write). Argument `r` is the request. Errors: `NotReceiveOnly` or a transport error. Side effect: one USB control OUT. Tests: the test that runs every model through every public method and checks each request sent, in `src/tests.rs`.

### `read_request(t, request, value, index, buf) -> Result<usize, Error>` (`radio.rs`) [@ANCHOR: elad_usb:elad_radio_read_request]

The single funnel for control INs, used by `Radio::open`: it returns `NotReceiveOnly(request)` unless `is_read_request` accepts the `bRequest` (firmware version or EEPROM), then calls `control_in` and returns the byte count read. Arguments: the transport, `bRequest`, `wValue` (the EEPROM address for `0xA2`), `wIndex` and the destination buffer. A short reply is returned as the count, not an error; the callers handle it. Side effect: one USB control IN. Tests: open unit tests in `src/tests.rs` and the real-unit fixture.

### `Radio::silence(&mut self) -> Result<usize, Error>` (`radio.rs`, private) [@ANCHOR: elad_usb:elad_radio_silence]

Turns a read that produced nothing into an error: it returns `NoSamples` when no sample has ever been decoded and at least three timeouts plus empty reads have accumulated (the signature of an S1/S2 whose FPGA image is not loaded), otherwise plain `Timeout`. It always returns Err; the Ok type is only for the signature shared with `read`. It reads and does not change the counters, which `read` has already updated. Pure. Tests: no-samples and timeout unit tests in `src/tests.rs`.

### `Radio::write_front_end(&mut self, filters: bool, attenuator: bool) -> Result<(), Error>` (`radio.rs`, private) [@ANCHOR: elad_usb:elad_radio_write_front_end]

Writes the S2's front-end code for the current frequency: it returns Ok at once for a model without the switches; otherwise it builds `front_end_code(filters, tuned_hz or 0, attenuator)` and sends request `0xF7` with the code in `wValue` and index 2 through `send`. It does not change the stored filter or attenuator state, so callers (`set_front_end`) record the state only after it succeeds. Used by `tune` (with the stored state) and `set_front_end`. Error: a transport error. Receive-only. Tests: front-end unit tests in `src/tests.rs` check the code per band.

### `FakeTransport::reply(&self, request: u8, value: u16, bytes: Vec<u8>)` (`transport.rs`) [@ANCHOR: elad_usb:elad_fake_reply]

Scripts the fake unit: a later control IN for `(request, value)` is answered with `bytes`, replacing an earlier script for the same pair. This is how tests supply a firmware version or EEPROM contents, including the fixture captured from a real FDM-S2. A control IN with no script returns zero bytes. Test-support code with no hardware; the table is behind a mutex, so clones of the fake share it. If the mutex is poisoned the call does nothing. Tests: every `Radio::open` unit test.

### `FakeTransport::control_in` (`transport.rs`, `Transport` impl) [@ANCHOR: elad_usb:elad_fake_control_in]

The fake's control IN: it logs an `Op::In` (request, value, index, length asked) and answers from the table scripted with `reply`, copying at most the buffer's length; an unscripted request gives an empty answer, Ok(0). It never fails and has no timing. The log lets tests assert exactly which requests the driver issued and in what order. Test-support only; no hardware involved. Tests: all `Radio` unit tests that open a fake.

### `FakeTransport::control_out` (`transport.rs`, `Transport` impl) [@ANCHOR: elad_usb:elad_fake_control_out]

The fake's control OUT: it appends an `Op::Out` (request, value, index and a copy of the data stage) to the shared log and returns the data length as accepted. It never fails. Tests read the log with `ops()` to check that only receive-only requests were sent and that tune, front-end and FIFO requests carry the right bytes. Test-support only; no hardware is involved and nothing is transmitted. Tests: tune, front-end and start/stop unit tests.

### `FakeTransport::queue_bulk(&self, chunk: Result<Vec<u8>, Error>)` (`transport.rs`) [@ANCHOR: elad_usb:elad_fake_queue_bulk]

Queues one bulk-read outcome for the fake: either a byte chunk or an error such as `Timeout`. Outcomes are served first in, first out by `bulk_in`, so tests can script a stream, split pairs across chunks, empty reads and timeouts. The queue is shared by clones of the fake and behind a mutex; a poisoned mutex makes the call do nothing. Test-support only. Tests: read, decoder-carry and no-samples unit tests.

### `FakeTransport::bulk_in` (`transport.rs`, `Transport` impl) [@ANCHOR: elad_usb:elad_fake_bulk_in]

The fake's bulk IN: it pops the next queued outcome. With nothing queued it returns `Error::Timeout`, which models an idle or unloaded receiver; a queued error is returned as it is; a queued chunk is copied into the buffer (truncated to its length) and its byte count returned. The timeout argument is ignored, so tests never wait. Test-support only; no hardware. Tests: `Radio::read` unit tests.

### `err(e: rusb::Error) -> Error` (`usb.rs`) [@ANCHOR: elad_usb:elad_usb_err]

Maps a libusb (rusb) error to this crate's `Error`: `Timeout` stays `Error::Timeout`, so `Radio::read` can count it and decide on `NoSamples`; every other libusb error becomes `Error::Usb` carrying libusb's message text. Pure. It matters because a bulk timeout is normal on an idle unit and must not be reported as a USB failure. Tests: `a_libusb_timeout_stays_a_timeout` in `usb.rs`.

### `impl Transport for Box<dyn Transport>`: `control_in` (`transport.rs`) [@ANCHOR: elad_usb:elad_box_control_in]

A boxed transport forwards a control IN to the inner transport unchanged, so a caller can choose the real USB transport or a fake at run time (the bench tool holds a `Box<dyn Transport>`). Same arguments, return value and errors as the inner transport; no state of its own, no locking. Tests: covered wherever a `Radio` is built on a boxed transport in `src/tests.rs`.

### `impl Transport for Box<dyn Transport>`: `control_out` (`transport.rs`) [@ANCHOR: elad_usb:elad_box_control_out]

A boxed transport forwards a control OUT, with its data stage, to the inner transport unchanged and returns what it returns. The receive-only guard is not here: it lives in `Radio::send`, in front of every transport. No state, no locking. Tests: covered wherever a `Radio` is built on a boxed transport in `src/tests.rs`.

### `impl Transport for Box<dyn Transport>`: `bulk_in` (`transport.rs`) [@ANCHOR: elad_usb:elad_box_bulk_in]

A boxed transport forwards a bulk IN read, with its timeout, to the inner transport and returns the byte count or the inner error (including `Timeout`). No buffering of its own and no locking. Blocking behaviour is that of the inner transport: the real USB transport blocks up to the timeout, the fake returns at once. Tests: covered wherever a `Radio` is built on a boxed transport in `src/tests.rs`.

### `DeviceLock::acquire(bus: u8, address: u8, owner: &str) -> Result<DeviceLock, LockError>` (`lock.rs`) [@ANCHOR: elad_lock_acquire]

Takes the single-owner lock for one receiver in the directory chosen by `lock_dir`, recording `owner` (a short program name such as "relay") so a refused second taker can be told who holds it. When neither `ELAD_LOCK_DIR` nor a usable `XDG_RUNTIME_DIR` is set it first makes sure the private fallback directory exists and is ours (`ensure_private_dir`). Then it calls `acquire_in`. Errors: `LockError::Held` naming the holder, or `LockError::Io`. Non-blocking: it returns at once. The relay and the bench tool take it so they never stream from one unit together. Tests: `the_directory_comes_from_the_environment...` in `lock.rs`.

### `DeviceLock::acquire_in(dir: &Path, bus: u8, address: u8, owner: &str) -> Result<DeviceLock, LockError>` (`lock.rs`) [@ANCHOR: elad_lock_acquire_in]

The same with the directory given. It opens (creating, never truncating) `elad-<bus>-<address>.lock` and tries a non-blocking advisory `flock` on it. If another process holds it, the holder's note is read (before anything is overwritten) and `LockError::Held { holder, path }` is returned; any other failure is `LockError::Io`. On success the file is truncated and `pid <n> (<owner>)` is written. The lock lives as long as the returned value and is released on drop or process death, so a crash leaves no stale lock. Tests: the held/release test and the unwritable-directory test in `lock.rs`.

### `lock_dir() -> PathBuf` (`lock.rs`) [@ANCHOR: elad_lock_dir]

Chooses the directory for lock files: `ELAD_LOCK_DIR` if set, else `XDG_RUNTIME_DIR` if it is an existing directory (private to the user), else `elad-<uid>` in the temp directory. It never uses `/run/lock`, which any local user can write to and could pre-create or hold the file. The uid comes from the owner of `/proc/self`, 0 if that cannot be read. Pure apart from reading environment and `/proc`; it creates nothing (`ensure_private_dir` does). Tests: `the_directory_comes_from_the_environment_then_the_runtime_dir_then_a_private_temp_dir` in `lock.rs`.

### `ensure_private_dir(dir: &Path) -> Result<(), LockError>` (`lock.rs`, private) [@ANCHOR: elad_lock_ensure_private_dir]

Makes sure the default fallback lock directory is private to this user. It creates it with mode 0700 when missing (an existing one is fine), then examines it without following a symlink and refuses it, with `LockError::Io` telling the user to remove it or set `ELAD_LOCK_DIR`, if it is not a directory, is owned by another user, or has any group or other permission bit. This stops another local user pre-creating the directory to hold or spoof the lock. Used only for the default `elad-<uid>` directory, not for an explicitly configured one. Tests: `the_default_directory_is_private_and_a_foreign_or_open_one_is_refused` in `lock.rs`.

### `lock_file_name(bus: u8, address: u8) -> String` (`lock.rs`) [@ANCHOR: elad_lock_file_name]

Returns the lock file name for a receiver, `elad-<bus>-<address>.lock`, with the bus number and device address in decimal. Different receivers get different names and therefore lock independently; the same unit always maps to the same file, which is what lets the relay and the bench tool contend for it. Pure and total. Note that a unit re-enumerated at a new address gets a new name. Tests: `different_receivers_lock_independently` in `lock.rs`.

### `read_holder(file: &mut File) -> String` (`lock.rs`, private) [@ANCHOR: elad_lock_read_holder]

Reads the note a lock holder wrote into the lock file, for the refusal message. It rewinds the file and reads it as text; if the read fails, the text is empty after trimming, or it is 200 bytes or longer, it returns "another program" instead, so a damaged or hostile file cannot flood a log line. Called by `acquire_in` only after a failed lock attempt, so the note is never read from a lock we own. No side effects beyond moving the file offset. Tests: the held/release test in `lock.rs`.

### `impl Display for LockError` (`lock.rs`) [@ANCHOR: elad_lock_error_display]

The refusal text. For `Held` it says the ELAD receiver is in use by the holder (pid and program name), names the lock file, and tells the user to stop that program (for example the relay's ELAD source) or wait and retry. For `Io` it says the receiver lock could not be taken and gives the operating-system reason. Pure formatting, cannot fail. The message deliberately names the holder so the user knows which program to stop. Tests: the held/release test in `lock.rs` checks the text.

### `Locked::new(inner: T, lock: DeviceLock) -> Locked<T>` (`lock.rs`) [@ANCHOR: elad_locked_new]

Ties a held `DeviceLock` to a transport so the lock lives exactly as long as the transport: a `Radio` built on the `Locked` value keeps the receiver to itself until it is dropped, at which point the lock is released and another program may take it. Arguments: `inner` the real or fake transport, `lock` a lock already acquired by `acquire` or `acquire_in`. It cannot fail and does no I/O. The lock is never checked again on each request; ownership is the whole mechanism. Tests: `a_locked_transport_keeps_the_lock_until_it_is_dropped...` in `lock.rs`.

### `Locked::control_in` (`lock.rs`, `Transport` impl) [@ANCHOR: elad_locked_control_in]

A locked transport's control IN: passed straight through to the inner transport with the same arguments, return value and errors. The wrapper adds nothing per call; its only job is to hold the lock for its lifetime. No locking in the call path and no state. Tests: `a_locked_transport_keeps_the_lock_until_it_is_dropped_and_passes_requests_through` in `lock.rs`, which reads a firmware version through it.

### `Locked::control_out` (`lock.rs`, `Transport` impl) [@ANCHOR: elad_locked_control_out]

A locked transport's control OUT: passed straight through to the inner transport with the same request, value, index and data stage, returning its result. The receive-only check stays in `Radio::send`, ahead of this call, so locking adds no new request path. No state. Tests: `a_locked_transport_keeps_the_lock_until_it_is_dropped_and_passes_requests_through` in `lock.rs`, which sends a tune through it and checks the fake's log.

### `Locked::bulk_in` (`lock.rs`, `Transport` impl) [@ANCHOR: elad_locked_bulk_in]

A locked transport's bulk IN: passed straight through to the inner transport with the same buffer and timeout, returning the byte count or the inner error, including `Timeout`. Blocking behaviour is the inner transport's. The lock is held across the whole stream because the `Locked` value lives as long as the radio. No state. Tests: `a_locked_transport_keeps_the_lock_until_it_is_dropped_and_passes_requests_through` in `lock.rs`.

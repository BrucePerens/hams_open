# elad_usb

A receive-only Rust driver for ELAD USB receivers (FDM-DUO, FDM-S2, FDM-S1), written from public descriptions of the protocol
and checked against one real FDM-S2. Used by the relay (`hams_com/daemons/hams_local_relay/src/elad_source.rs`) and by
`bench_mcp` (`src/elad/`). The design is `hams_com/docs/proposals/ELAD_RECEIVER_RELAY_AND_BENCH.md`.

## Status (verified=unverified except where a row says otherwise)

| Piece | State |
|---|---|
| Discovery by USB id (1721:061a DUO, 061c S2, 0610 S1) | **verified on an FDM-S2, 2026-11-08** |
| Descriptors: configuration 1, interface 0 vendor class, bulk OUT 0x02, bulk IN 0x86 (512 bytes), high speed | **verified on an FDM-S2** |
| Control reads: firmware version (`0xFF`), EEPROM (`0xA2`, index `0x0151`): serial, hardware version, rate correction, dB offsets | **verified on an FDM-S2** (fw 3.0, hw 1.3; plausible values; fixture `tests/fixtures/fdm_s2_fw3_0_open.txt`) |
| Tune word, S2 front-end code, FIFO start/stop | written from public descriptions, tested against the fake only |
| I/Q format (32-bit little-endian below 6.144 Msps, 16-bit at 6.144 Msps; I then Q) | written from public descriptions; **not decoded from real samples yet** |
| FDM-S1/S2 FPGA image | **not loaded by this software, by decision**: see below |
| FDM-DUO | no unit available; nothing verified |

**The S1/S2 FPGA.** A bus-powered FDM-S1/S2 enumerates and answers control reads with no FPGA image, but streams no samples (a real
FDM-S2: three 500 ms bulk reads, zero bytes). Public sources say the FPGA must be programmed from ELAD's own, non-redistributable image
(one per sample rate) by ELAD's loader, `elad-firmware`. This crate contains, downloads and runs none of that. `Radio::start` refuses an
S1/S2 unless the caller states the FPGA was loaded by other means (`FpgaState::LoadedExternally`). The question for the owner is
`hams_com/night_shift_questions/open/elad-fpga-image-for-fdm-s2-*.md`. An FDM-DUO loads its own FPGA and needs nothing.

## Receive only

`protocol::is_receive_request` is the whole list of what can go on the wire; `Radio` refuses anything else. No CAT write, no PTT, no
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

Each entry is a semantic anchor in the source with a test citing it (`// Tests [@ANCHOR: ...]`).

- [@ANCHOR: elad_usb:elad_model_from_pid] Maps a USB product id to FDM-DUO, FDM-S2 or FDM-S1; any other id is refused.
- [@ANCHOR: elad_usb:elad_tuning_word] The 32-bit DDC tuning word: the frequency's fraction of the ADC clock times 2^32, clamped at zero, aliasing above the clock.
- [@ANCHOR: elad_usb:elad_tune_request_sampler] The FDM-S1/S2 tune request: word low half in the value, high half in the index, top byte in the data stage.
- [@ANCHOR: elad_usb:elad_tune_request_duo] The FDM-DUO tune request through its gateway request.
- [@ANCHOR: elad_usb:elad_front_end_code] The FDM-S2 filter-bank and attenuator code for a frequency.
- [@ANCHOR: elad_usb:elad_is_receive_request] The list of requests that may be sent; a CAT write or an unknown request is not on it.
- [@ANCHOR: elad_usb:elad_calibration_from_eeprom] Decodes the EEPROM calibration; unprogrammed (all-ones) and short fields are absent.
- [@ANCHOR: elad_usb:elad_level_offset_db] The dB offset from a normalised sample level to the input level for a model, rate and front-end state.
- [@ANCHOR: elad_usb:elad_format_for_rate] The component width for a sample rate.
- [@ANCHOR: elad_usb:elad_decoder_push] Converts wire bytes to I/Q floats, carrying a split pair to the next call.
- [@ANCHOR: elad_usb:elad_fpga_ready] Whether samples can be expected for a model given what the caller says about the FPGA image.
- [@ANCHOR: elad_usb:elad_radio_open] Opens a unit read-only: rate check, firmware version, EEPROM calibration.
- [@ANCHOR: elad_usb:elad_radio_tune] Sets the centre frequency (range-checked) and the S2's filter code.
- [@ANCHOR: elad_usb:elad_radio_set_filters] Switches the S2's band filters.
- [@ANCHOR: elad_usb:elad_radio_set_attenuator] Switches the S2's attenuator.
- [@ANCHOR: elad_usb:elad_radio_start] Starts the sample FIFO; refuses an S1/S2 with no declared FPGA image.
- [@ANCHOR: elad_usb:elad_radio_stop] Stops the sample FIFO.
- [@ANCHOR: elad_usb:elad_radio_read] Reads one bulk transfer, decodes it, counts it; three silent reads before any sample are `NoSamples`.
- [@ANCHOR: elad_usb:elad_usb_find] Lists ELAD receivers on the bus without opening them.
- [@ANCHOR: elad_usb:elad_usb_open] Opens and claims a found receiver (exercised by the hardware run, not by `cargo test`).
- [@ANCHOR: elad_usb:elad_format_component_bytes] Bytes per I or Q component of a sample format.
- [@ANCHOR: elad_usb:elad_format_full_scale] The full-scale divisor of a sample format.
- [@ANCHOR: elad_usb:elad_error_display] The human-readable text of every error, including the FPGA-image and refused-request messages.
- [@ANCHOR: elad_usb:elad_model_adc_clock_hz] The model's ADC clock in Hz.
- [@ANCHOR: elad_usb:elad_model_name] The model's display name.
- [@ANCHOR: elad_usb:elad_model_pid] The model's USB product id.
- [@ANCHOR: elad_usb:elad_model_rx_range_hz] The model's receive range in Hz.
- [@ANCHOR: elad_usb:elad_model_sample_rates_hz] The sample rates the model streams.
- [@ANCHOR: elad_usb:elad_radio_send] Sends a control request only if it is on the receive-only list.
- [@ANCHOR: elad_usb:elad_radio_silence] Turns repeated silent reads into the no-samples verdict.
- [@ANCHOR: elad_usb:elad_radio_write_front_end] Writes the S2's filter and attenuator code for the current frequency.
- [@ANCHOR: elad_usb:elad_fake_reply] Scripts a control reply on the fake transport.
- [@ANCHOR: elad_usb:elad_fake_control_in] The fake's control IN: records and answers from the script.
- [@ANCHOR: elad_usb:elad_fake_control_out] The fake's control OUT: records the request.
- [@ANCHOR: elad_usb:elad_fake_queue_bulk] Queues a bulk read result on the fake.
- [@ANCHOR: elad_usb:elad_fake_bulk_in] The fake's bulk IN: serves queued results, a timeout when empty.
- [@ANCHOR: elad_usb:elad_usb_err] Maps a libusb error to this crate's error, keeping timeouts apart.
- [@ANCHOR: elad_usb:elad_box_control_in] A boxed transport forwards a control IN to the inner transport.
- [@ANCHOR: elad_usb:elad_box_control_out] A boxed transport forwards a control OUT.
- [@ANCHOR: elad_usb:elad_box_bulk_in] A boxed transport forwards a bulk IN.
- [@ANCHOR: elad_lock_acquire] Takes the single-owner lock for one receiver in the lock directory; a second taker is refused naming the holder.
- [@ANCHOR: elad_lock_acquire_in] The same with the directory given; the lock is an advisory `flock` on `elad-<bus>-<address>.lock`, released on drop or process death, with the holder's pid and name written in.
- [@ANCHOR: elad_lock_dir] The lock directory: `ELAD_LOCK_DIR`, else `/run/lock`, else the temp directory.
- [@ANCHOR: elad_lock_file_name] The lock file name for a bus and address.
- [@ANCHOR: elad_lock_read_holder] Reads who holds a lock, falling back to "another program".
- [@ANCHOR: elad_lock_error_display] The refusal text: who holds the receiver and what to do.
- [@ANCHOR: elad_locked_new] Ties a lock to a transport so the lock lives as long as the transport.
- [@ANCHOR: elad_locked_control_in] A locked transport's control IN: passed through.
- [@ANCHOR: elad_locked_control_out] A locked transport's control OUT: passed through.
- [@ANCHOR: elad_locked_bulk_in] A locked transport's bulk IN: passed through.

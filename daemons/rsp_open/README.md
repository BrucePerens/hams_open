# rsp_open

An open Rust driver for the SDRplay **RSP2pro** (USB 1df7:3010), **RSP1A** (USB 1df7:3000)
and **RSP1** (USB 1df7:2500): library crate plus examples, receive only. Status
(2026-10-25): all three receivers tune and stream from power-up with no firmware and no
SDRplay software, checked with each receiver's own 24 MHz reference harmonics (1 kHz-2 GHz
bands tried from 23.8 MHz to 1.2 GHz, including 375-420 MHz, 0 counter gaps). **Limits:**
without firmware the RSP2pro's and RSP1A's board controls (antenna selection, band filters,
notch, bias-T, front-end attenuator steps) cannot be set, and a cold board comes up about 10 dB
attenuated against its normal state (`docs/PROTOCOL.md` 5.15); whether a cold receiver
receives a real signal well was not tested (nothing was ever connected to an antenna port).
The low-IF modes are incomplete (below).

Licence: AGPL-3.0-or-later (SPDX headers in every source file, `Cargo.toml` `license` field),
the repository default described in `LICENSING.md`. The crate is not published to crates.io
(`publish = false`).

## Firmware: not required for reception

Both models power up in a boot-ROM state (bcdDevice 2.00). SDRplay's software uploads a firmware
image that turns it into the running device (bcdDevice 2.07). This crate contains **no firmware
and no firmware-loading code**, and never sends the download (0x44) or reset (0x40) requests.
It drives a running receiver (`Rsp::open`), or a boot-ROM one with `Rsp::open_boot_rom_with` / `open_boot_rom_model_with` (RSP2pro, RSP1A and RSP1 all work cold; see PROTOCOL.md 5.8 for one unexplained empty-stream episode on the RSP1A):
measured on an RSP2pro, the ROM already handles register writes, start/stop and the isochronous
stream, so tuning and streaming work. The ROM does not answer requests 0x48 and 0x49, so in
that mode the board (antenna, band filters, notch, bias-T, front-end steps) is not controlled
and those calls return `Error::Unsupported` (docs/PROTOCOL.md 5). `Rsp::open` still reports
`Error::NotRunning` for a boot-ROM device. Other bcdDevice values are refused
(`Error::UnknownState`). Other SDRplay products, the RSP1 included, are never opened.

## Stress testing on a port you can power-cycle

`examples/rsp_coldcheck.rs` is a one-shot pass/fail check of a receiver (prints one JSON line, exit 0 or 1; a stream that delivers nothing is reported as `stalled`, not waited on). `tools/power_cycle_stress.py` wraps it in a loop: switch the port's power off, check the receiver leaves the bus, switch it on, check it comes back in its cold state (bcdDevice 2.00) with the expected USB ID, then run the check, optionally several open/close cycles per power cycle (`--hammer`). It logs one JSON line per iteration, stops at the first failure with a snapshot (lsusb, kernel log tail) and leaves the device powered for inspection. Power backends: `uhubctl` (a hub that really switches port power), `cmd` (relay boards, smart plugs), `sysfs` (does not cut VBUS on many machines, including the development laptop, so it is not a real power cycle), `manual`, and `none`. USB port power on a given machine must be verified first: the receiver has to disappear from the bus and come back at bcdDevice 2.00.

## Linux: the kernel `msi2500` driver and opening a receiver

On Linux the in-kernel `msi2500` driver (with `msi001`) binds to SDRplay receivers as soon as they appear (the RSP1, 1df7:2500, even in its boot-ROM state), creating `/dev/swradio*`. While it is bound, the vendor API cannot load firmware into the device, and the interface cannot be claimed without first detaching it. Decision (Bruce Perens, 2026-10-25): the relay does not use the kernel driver for these receivers, and **it makes sure the driver is out of the way just before it opens a receiver**, rather than banning the driver in advance. The driver stays available to everything else on the machine.

Before opening, the relay (on Linux):

1. Finds the receiver by enumeration (`rsp_open::usb::list_devices`, equivalent to `lsusb -d 1df7:`): vendor 1df7 with product ID 2500, 3000 or 3010. Nothing is opened or sent at this step.
2. Checks whether anyone is using the kernel driver's device: a process with `/dev/swradio*` open (look for it with `fuser /dev/swradioN`, or by scanning `/proc/*/fd`). **If it is in use, the relay does not take the receiver**: it reports who has it (process name and ID) and asks the user to close that program, or to confirm. Detaching a bound driver while another program is streaming from it removes the device from under that program (its reads fail), so libusb's detach is not safe in that case; only `rmmod` refuses on its own.
3. If a kernel driver is bound to its interface 0 (`/sys/bus/usb/devices/<bus>-<port>:1.0/driver` points at `msi2500`), tries the least harmful step first: `UsbTransport::open` asks libusb to detach the driver from this one interface (no root needed when the device node is accessible) and to reattach it on close.
4. If that fails because of permissions, and the relay can run a privileged helper: `rmmod msi2500` then `rmmod msi001` (or unbind just this device: `echo <bus>-<port>:1.0 > /sys/bus/usb/drivers/msi2500/unbind`), then open. `rmmod` refuses while another program has `/dev/swradio*` open, so it cannot pull the driver from under another user; the relay should report that case rather than force it.
5. Opens the receiver.

The crate does steps 2 and the libusb detach of step 3 itself: `UsbTransport::open` returns `Error::KernelDriverInUse` (driver, device nodes, `pid (name)` of each program) instead of detaching a driver that a program is using, and `rsp_open::kernel::inspect` exposes the same check. Programs of other users cannot be inspected without root (`KernelDriverUse::holders_complete` is false then). The `rmmod`/unbind step 4 is for the relay's privileged helper. A stream that delivers nothing is reported by `SampleIter::next_chunk` as `Error::Stalled`. The crate never issues a USB reset (a unit test checks the source).

`rmmod` does not last: plugging a receiver in (or rebooting) loads the driver again, which is why the step is done before every open and not once at install time. A permanent ban (`/etc/modprobe.d/blacklist-msi2500.conf` with `blacklist msi2500` and `install msi2500 /bin/false`) remains an option for a machine dedicated to the relay; it also stops the driver serving other Mirics dongles on that machine. The development machine has no ban applied (it was removed on request). See `docs/PROTOCOL.md` 5.14 for the observation that motivated this.

## Provenance (clean-room record)

- **Specification:** [`docs/PROTOCOL.md`](docs/PROTOCOL.md), a description of the USB
  traffic between SDRplay's software and the author's own receiver, recorded with usbmon and
  filtered to the receiver only (its section 0 records the method and the clean-room boundary;
  section numbers in the source comments refer to it). No SDRplay binary was disassembled, decompiled, traced or
  strings-mined; no SDRplay code or documentation text was used.
- **Extra wire facts:** register values in the unit tests come from decoded private
  captures (the request sequences SDRplay's API sent while one control at a time was
  changed), read with our own capture-decoding scripts. The raw captures are not part of this
  repository; the unit tests carry only the byte tables documented in `docs/PROTOCOL.md`. The r3 "12 extra PLL fraction bits"
  rule and the "largest integer strictly below the exact quotient" rounding are our own fit
  of those captured values; the 0.05 kHz tuning accuracy on the hardware confirms the fit.
- **GPL references consulted for facts only** (Linux `drivers/media/usb/msi2500/msi2500.c`,
  `drivers/media/tuners/msi001.c`, libmirisdr-4); no code was copied. Facts taken from them:

  | Fact | Source |
  |---|---|
  | request 0x41 argument packing (wValue = low byte << 8 \| reg, wIndex = value >> 8) | kernel msi2500 / libmirisdr (also matches the wire) |
  | MSi001 SPI word layout (address in bits 3:0), r0 field positions, r5 threshold layout | kernel msi001 |
  | the 14-bit sign of the 16-bit sample words | kernel msi2500 |
  | 12-bit packed layout (two 12-bit values per 3 bytes, low nibble first) | libmirisdr-4 `convert/336_s16.c` |
  | "384" format layout (6 groups x [16 runs x 10 bytes of 10-bit values + 32-bit scale word], 24 trailing bytes) | libmirisdr-4 `convert/384_s16.c` |

  Everything else (band tables, board bytes 0x12/0x13 of both models, LNA tables, ADC clock
  rules, alt and format selection, open/close sequences) comes from the wire captures.

- **Clean-room boundary:** only bus traffic of the author's own receiver was observed;
  SDRplay's software was used only as the front end of the captures. No firmware image was
  captured, stored or inspected, and this crate contains no firmware-loading code.
- **Requests sent:** only those `docs/PROTOCOL.md` lists as observed: 0x41 (register
  write), 0x43/0x45 (start/stop), 0x48 (one-byte read at open), 0x49 (writes to the board's MCP23S18 I/O expander: IODIRA 0x00,
  IODIRB 0x01, GPIOA 0x12, GPIOB 0x13), SET_INTERFACE alt 0/1/2, and the standard string-descriptor read.
  The unit tests `only_observed_requests_are_emitted` (RSP2pro) and
  `rsp1a_only_observed_requests_are_emitted` exercise every API call against a recording
  mock and fail on anything else; both also check that the bias-T bit is never set. Never sent: 0x40, 0x42, 0x44, 0x46, 0x47, bulk
  alt 3, USB resets.

## API

```rust
use rsp_open::{Rsp, Antenna, Bandwidth, IfMode, Model, Settings};
let mut rx = Rsp::open()?;                     // first running RSP2/RSP2pro or RSP1A
// or Rsp::open_model_with(Model::Rsp1a, Settings::default())?; (Rsp2 is the same type)
rx.set_frequency(7_074_000)?;                  // 1 kHz - 2 GHz
// let mut rx = Rsp::open_auto(Settings::default())?;  // running or cold receiver, whichever is present
rx.set_sample_rate(6_000_000)?;                // 2.0, 2.048 or 3..10 MHz ADC clock (zero IF)
rx.set_if_mode(IfMode::Low1620k)?;             // or low IF with its fixed ADC clock
rx.set_bandwidth(Bandwidth::K1536)?;
rx.set_gain(40, 0)?;                           // IF gain reduction dB (0..59), LNA state
rx.select_antenna(Antenna::HiZ)?;              // RSP2pro: A, B, or HiZ (< 60 MHz); RSP1A: A only
rx.set_notch(true)?; rx.set_bias_t(false)?; rx.set_ppm(0.0)?;
rx.set_ext_ref_out(false)?;                    // RSP2pro only
rx.set_dab_notch(true)?;                       // RSP1A only
rx.start_stream(|samples, stats| { /* &[IqI16], runs on the USB thread */ })?;
// or: let it = rx.start_stream_iter(64)?; for chunk in it { ... }
rx.stop_stream()?;
rx.close()?;                                   // also on drop
```

Defaults: 100 MHz, Antenna A, zero IF, 2 MHz ADC clock, BW 1.536 MHz, IF GR 40, LNA 0,
notches/bias-T/ext-ref off. Opening with bias-T on is refused.

### Model differences

Everything on the MSi2500 and the tuner is shared. The board bytes differ (`docs/PROTOCOL.md`
2.11):

| | RSP2pro | RSP1A |
|---|---|---|
| Port A (byte 0x12) | antenna bits 1:0, filter bits 5:3, ref-out bit 6 | filter bits 4:0 (extra filter below 2 MHz), attenuator step bits 7:5 |
| Port B (byte 0x13) | bias-T bit 0, front-end step bits 5:2, notch bits 7:6 | DAB notch bit 0, step bit 1, FM/MW notch bit 2, bias-T bit 4 |
| LNA states HF / VHF / 250-375 / 420-1000 / L | 9 / 9 / 9 / 6 / 6 (Hi-Z 5) | 7 / 10 / 10 / 10 / 9 |
| Gain write | r1, port B | r1, port A, port B |
| Bandwidth change | r0 | r0, ports, IODIR, r3, r2, r1, ports (the API's sequence) |
| DAB notch | none | port B, then the gain write |
| Antenna ports, ref output | A, B, Hi-Z; ref output | one port; none | Samples are `IqI16` left-aligned to 16 bits whatever the wire
format (14-bit, 12-bit, 10-bit "384", 8-bit); `stream::to_f32` gives -1..1 floats. Stream
statistics count blocks, sample-counter gaps and lost samples.

Shutdown (`close` or drop): STOP, cancel transfers, bias-T off, tuner r12/standby, ADC
clock and reg 0x05 to their close values, alt 0, release the interface.

## What works on the real device (2026-10-25)

- Streaming at 2.0/2.048/3/6/8/9/10 MHz ADC clocks in all four sample formats, 0 counter
  gaps in steady state (one occasional gap in the first blocks after START).
- Tuning in every supported band: the internal 24 MHz harmonics land within one 122 Hz FFT
  bin of the expected offset in VHF, B3, HF AM, the 250-375 MHz AM down-conversion band,
  B4/5 and L band (`tests/hil.rs`).
- Changes while streaming, all with 0 counter gaps (`hil_live_reconfiguration`,
  `hil_gaps_during_control`): retune in band and across bands, gain, antenna A/B, notch,
  bandwidth, ext-ref output, ppm (+10 ppm moves the LO down 10 ppm: the internal harmonic
  moves +0.98 kHz at 95.8 MHz, as expected), sample rate 2 -> 6 -> 2 MHz (STOP, alt change,
  restart handled inside `set_sample_rate`), and the three low-IF modes.
- Low-IF modes: stream at the halved rate (1, 3, 4.096 MS/s) with the 504-per-block counter
  step confirmed (no gaps). **Where RF lands in the low-IF output is not characterised yet**
  (tuned 95.8 MHz, the strongest line was at -200 kHz / -600 kHz / -1712 kHz for the 450 k /
  1.62 M / 2.048 M modes); `set_frequency` writes the same LO as in zero IF, whereas the API moves the LO up by the IF
  in these modes (`docs/PROTOCOL.md` 2.14; correcting it is deferred),
  and the host-side IF down-conversion is left to the caller. Hi-Z port: switching verified
  on the mock and on hardware via `rsp_measure` (floor changes), no signal test.
- `examples/rsp_measure.rs` characterises the front end; results in
  `docs/measure_2026-10-25.txt`, summarised in `docs/PROTOCOL.md` section 1.13.7.

## Not implemented / open

- **375-420 MHz**: tuned with the 144/168 MHz first-IF plans, but the API's calibration
  handshake at each plan change (request 0x42 read, r13/r14 writes) is not replicated; the
  reference spurs land correctly without it (PROTOCOL.md 5.6). Its effect on sensitivity and
  phase noise is not measured, and the plan edges (about 375 and 395 MHz) are only bracketed.
- The API's close-time VCO calibration (0x42 read and r13/r14/063000/00800e writes) is not
  replicated; the driver closes the way `an earlier stand-alone verification program` did (works).
- No AGC, DC/IQ correction or decimation (all host-side in SDRplay's API).
- Low-IF modes only with their observed ADC clocks; ADC clocks between 2.688 and 3 MHz and
  above 10 MHz refused.
- Bias-T is never switched on by tests or examples (nothing is connected to the ports).
- Other models (RSP1, RSP1B, RSPduo, RSPdx) are recognised by PID and refused.
- RSP1A: port-B value written mid-tune is 0xdf at open and the value in force on later
  tunes; the second half is a fit of captures made with LNA state 0 and notches off only.
- RSP2pro bandwidth changes: the API's capture shows the full sequence (r0, ports, IODIR,
  r3, r2, r1, port B); the driver keeps the r0-only write that was verified on hardware.

## Function reference

Each entry is a semantic anchor in the source with a test citing it (`// Tests [@ANCHOR: ...]`);
the USB-level ones are exercised by the hardware tests in `tests/hil.rs`.

- [@ANCHOR: rsp_open:rsp_lib_check_state] Refuses a device before anything is sent to it: boot-ROM (bcdDevice 2.00) gives `NotRunning`, any other unknown bcdDevice `UnknownState`, other SDRplay products `UnsupportedModel`.
- [@ANCHOR: rsp_open:rsp_lib_model_for_pid] Maps an SDRplay USB product ID to a model.
- [@ANCHOR: rsp_open:rsp_lib_model_name] The model's display name.
- [@ANCHOR: rsp_open:rsp_lib_error_fmt] Human-readable error text, including the exact `NotRunning` message.
- [@ANCHOR: rsp_open:rsp_encode_band_for] RF frequency to RSP2pro band: tuner mode, first-LO plan, band-filter code, LNA table.
- [@ANCHOR: rsp_open:rsp_encode_band_in] Band lookup in a model's band table; refuses frequencies outside 1 kHz-2 GHz.
- [@ANCHOR: rsp_open:rsp_rsp1a_lna_table] The RSP1A LNA-state tables: attenuator code, port-B step bit and MSi001 gain-reduction bits per state.
- [@ANCHOR: rsp_open:rsp_rsp1a_board_a_byte] RSP1A port A ("byte 0x12"): band filter and attenuator step.
- [@ANCHOR: rsp_open:rsp_rsp1a_board_b_byte] RSP1A port B ("byte 0x13"): DAB notch, step bit, FM/MW notch, bias-T (all active low).
- [@ANCHOR: rsp_open:rsp_rsp1a_open_transient_b] RSP1A port-B value written in the middle of the open sequence.
- [@ANCHOR: rsp_open:rsp_encode_mode_byte] MSi001 band-mode byte for r0 (Hi-Z selects the second AM input).
- [@ANCHOR: rsp_open:rsp_encode_gain_table] Which LNA-state table applies for a band and antenna port.
- [@ANCHOR: rsp_open:rsp_encode_lna_table] The LNA-state tables: board front-end step plus MSi001 gain-reduction bits per state.
- [@ANCHOR: rsp_open:rsp_encode_lo_hz] First-LO frequency for an RF frequency (direct, +120 MHz up-conversion, -120 MHz down-conversion).
- [@ANCHOR: rsp_open:rsp_encode_tunermode_lo_div] LO output divider per tuner mode.
- [@ANCHOR: rsp_open:rsp_encode_pll] Tuner PLL words r2 and r3 (integer, 3000-step fraction, 12 extra fraction bits in r3), with ppm correction.
- [@ANCHOR: rsp_open:rsp_encode_ifmode_code] IF-mode code for r0.
- [@ANCHOR: rsp_open:rsp_encode_ifmode_hz] IF frequency of each IF mode.
- [@ANCHOR: rsp_open:rsp_encode_ifmode_required_adc_hz] The only ADC clock each low-IF mode was observed with.
- [@ANCHOR: rsp_open:rsp_encode_bandwidth_code] IF-bandwidth code for r0.
- [@ANCHOR: rsp_open:rsp_encode_bandwidth_at_most] Widest IF bandwidth not above a given width.
- [@ANCHOR: rsp_open:rsp_encode_board_band_byte] Board-control port A (MCP23S18 GPIOA, "byte 0x12"): antenna A/B, band filter, reference-clock output.
- [@ANCHOR: rsp_open:rsp_encode_board_front_byte] Board-control port B (MCP23S18 GPIOB, "byte 0x13"): notch, front-end step, bias-T.
- [@ANCHOR: rsp_open:rsp_encode_stream_config] Alternate setting, sample format and MSi2500 register 0x07 for an ADC clock and IF mode.
- [@ANCHOR: rsp_open:rsp_encode_streamconfig_counter_step] Block sample-counter advance per 1024-byte block.
- [@ANCHOR: rsp_open:rsp_encode_sampleformat_pairs_per_block] I/Q pairs per block for each sample format.
- [@ANCHOR: rsp_open:rsp_encode_adc_clock] ADC clock synthesiser words (MSi2500 registers 0x03/0x04), with ppm correction.
- [@ANCHOR: rsp_open:rsp_encode_ppm_to_ppb] Validates a ppm correction and converts it to parts per billion.
- [@ANCHOR: rsp_open:rsp_encode_r0_plan] MSi001 r0 with the first-LO plan bits (120 MHz and non-AM modes 2, 144 MHz 1, 168 MHz 0).
- [@ANCHOR: rsp_open:rsp_device_rsp_open_auto] Opens whichever supported receiver is present: a running one if there is one, otherwise the first cold one.
- [@ANCHOR: rsp_open:rsp_device_rsp_open_boot_rom_with] Opens the first receiver in the boot-ROM state (bcdDevice 2.00) and drives it without any firmware.
- [@ANCHOR: rsp_open:rsp_device_rsp_open_boot_rom_model_with] As above, but only a receiver of one model (RSP2pro, RSP1A or RSP1), so a second receiver on the bus is not driven by mistake.
- [@ANCHOR: rsp_open:rsp_device_rsp_need_board] Refuses a board control (antenna, notch, bias-T, reference output) in boot-ROM mode, where the board is not reachable.
- [@ANCHOR: rsp_open:rsp_device_rsp_set_notch] Switches the broadcast notch; `Error::Unsupported` in boot-ROM mode.
- [@ANCHOR: rsp_open:rsp_device_rsp_set_bias_t] Switches the bias-T (off at open, active low on the board); `Error::Unsupported` in boot-ROM mode.
- [@ANCHOR: rsp_open:rsp_device_sampleiter_next_chunk] The next sample chunk, or `Error::Stalled` when nothing arrived within the timeout, or `Error::Stream` when the stream ended.
- [@ANCHOR: rsp_open:rsp_kernel_inspect_in] Finds the kernel driver bound to a receiver's interface 0 and its device nodes from sysfs, and who has them open from /proc (roots given, for tests).
- [@ANCHOR: rsp_open:rsp_kernel_holders_of] The processes (pid and name) that have one of a set of device nodes open; reports when another user's processes could not be inspected.
- [@ANCHOR: rsp_open:rsp_stream_blockparser_push_packet] Splits isochronous packets into blocks, checks the sample counter (gaps, lost samples, wrap-around).
- [@ANCHOR: rsp_open:rsp_stream_decode_payload] Decodes 14-bit, 12-bit packed, 10-bit "384" and 8-bit payloads to 16-bit left-aligned I/Q.
- [@ANCHOR: rsp_open:rsp_dsp_fft] Radix-2 FFT used by the tests and examples.
- [@ANCHOR: rsp_open:rsp_dsp_power_spectrum_db] Averaged, windowed power spectrum in dBFS.
- [@ANCHOR: rsp_open:rsp_dsp_median] Median of a spectrum (noise-floor estimate).
- [@ANCHOR: rsp_open:rsp_dsp_peak_near] Strongest spectral line near an expected offset, and its level over the floor.
- [@ANCHOR: rsp_open:rsp_transport_mocktransport_vendor_in] Recording mock: answers request 0x48 as the device does.
- [@ANCHOR: rsp_open:rsp_transport_mocktransport_start_iso] Recording mock: delivers queued packets when streaming starts.
- [@ANCHOR: rsp_open:rsp_usb_context_new] Owned libusb context.
- [@ANCHOR: rsp_open:rsp_usb_with_devices] Walks the USB device list reading descriptors only.
- [@ANCHOR: rsp_open:rsp_usb_usbtransport_open] Opens and claims interface 0 of the selected device.
- [@ANCHOR: rsp_open:rsp_usb_usbtransport_serial] Reads the serial-number string descriptor.
- [@ANCHOR: rsp_open:rsp_usb_usbtransport_vendor_out] Vendor OUT request on EP0.
- [@ANCHOR: rsp_open:rsp_usb_usbtransport_vendor_in] Vendor IN request on EP0.
- [@ANCHOR: rsp_open:rsp_usb_usbtransport_set_alt] SET_INTERFACE, refusing the bulk alternate setting 3.
- [@ANCHOR: rsp_open:rsp_usb_usbtransport_start_iso] Submits isochronous transfers and runs the USB event thread.
- [@ANCHOR: rsp_open:rsp_usb_iso_callback] Per-transfer completion: delivers packets, resubmits while running.
- [@ANCHOR: rsp_open:rsp_usb_usbtransport_stop_iso] Cancels the transfers, joins the event thread, frees the transfers.
- [@ANCHOR: rsp_open:rsp_usb_usbtransport_release] Releases the interface and closes the device.
- [@ANCHOR: rsp_open:rsp_device_rsp2_open_with] Finds the first RSP2/RSP2pro or RSP1A, checks its state from the descriptor, opens it.
- [@ANCHOR: rsp_open:rsp_device_rsp_open_model_with] Opens the first device of one given model.
- [@ANCHOR: rsp_open:rsp_device_rsp_open_device] Checks the state, opens and claims the device, re-checks, runs the open sequence.
- [@ANCHOR: rsp_open:rsp_device_rsp2_with_transport] Runs the RSP2/RSP2pro open sequence on a transport.
- [@ANCHOR: rsp_open:rsp_device_rsp_with_transport_model] Validates the settings for a model and runs the open sequence on a transport.
- [@ANCHOR: rsp_open:rsp_device_model_band] The band table of a model.
- [@ANCHOR: rsp_open:rsp_device_model_table] The LNA-state table of a model for a band and port.
- [@ANCHOR: rsp_open:rsp_device_check_model_options] Refuses settings a model does not have (RSP1A reference output, RSP2pro DAB notch).
- [@ANCHOR: rsp_open:rsp_device_rsp_band_byte] Port A value for the current model, band and LNA state.
- [@ANCHOR: rsp_open:rsp_device_rsp_front_byte] Port B value for the current model, LNA state, notches and bias-T.
- [@ANCHOR: rsp_open:rsp_device_rsp_transient_b] Port B value written between port A and the direction registers in a full tune.
- [@ANCHOR: rsp_open:rsp_device_rsp_board] Board-expander write (request 0x49), remembering the last port-B value.
- [@ANCHOR: rsp_open:rsp_device_rsp_set_ext_ref_out] RSP2pro reference-clock output; refused on the RSP1A.
- [@ANCHOR: rsp_open:rsp_device_rsp_write_gain] The gain write (r1, then the model's board bytes).
- [@ANCHOR: rsp_open:rsp_device_rsp_set_dab_notch] RSP1A DAB notch (port B, then the gain write).
- [@ANCHOR: rsp_open:rsp_device_rsp_set_bandwidth] IF bandwidth (RSP2pro r0 only; RSP1A the API's bandwidth sequence).
- [@ANCHOR: rsp_open:rsp_device_rsp2_init] The open sequence, in the order SDRplay's API uses.
- [@ANCHOR: rsp_open:rsp_device_rsp2_full_tune] The full tune sequence used on band, port and gain-table changes.
- [@ANCHOR: rsp_open:rsp_device_rsp2_set_frequency] Tunes (r3/r2 only inside a band, full sequence across bands), clamping the LNA state to the new band.
- [@ANCHOR: rsp_open:rsp_device_rsp2_set_gain] IF gain reduction and LNA state.
- [@ANCHOR: rsp_open:rsp_device_rsp2_select_antenna] Antenna A, B or Hi-Z (Hi-Z only below 60 MHz).
- [@ANCHOR: rsp_open:rsp_device_check_antenna] Refuses Hi-Z at or above 60 MHz, and any port but A on the RSP1A.
- [@ANCHOR: rsp_open:rsp_device_rsp2_set_ppm] Reference correction applied to the ADC clock and the tuner PLL.
- [@ANCHOR: rsp_open:rsp_device_rsp2_set_sampling] Sample-rate and IF-mode changes, with the STOP/alt/restart sequence when the format changes.
- [@ANCHOR: rsp_open:rsp_device_rsp2_output_rate] Output sample rate (halved in the low-IF modes).
- [@ANCHOR: rsp_open:rsp_device_rsp2_sink_for] Packet sink that parses blocks and calls the user callback.
- [@ANCHOR: rsp_open:rsp_device_rsp2_restart_stream] (re)starts streaming: transfers first, then the START request.
- [@ANCHOR: rsp_open:rsp_device_rsp2_start_stream] Starts streaming to a callback.
- [@ANCHOR: rsp_open:rsp_device_rsp2_start_stream_iter] Starts streaming into a bounded channel and returns a blocking iterator.
- [@ANCHOR: rsp_open:rsp_device_rsp2_stop_stream] STOP request, then cancel the transfers.
- [@ANCHOR: rsp_open:rsp_device_rsp2_shutdown] Close: stop, bias-T off, tuner and ADC standby, alt 0, release (also on drop).

## Tests

```
cd daemons/rsp_open
CARGO_TARGET_DIR=<a directory of its own> cargo test -j 3   # unit tests (no hardware)
cargo test -j 3 --release --test hil -- --ignored --nocapture --test-threads=1   # hardware
```

The hardware tests skip a model that is not on the bus and open only PIDs 0x3010/0x3000 at
bcdDevice 2.07. `cargo run --release --example rsp_spectrum -- 95.8 30 0 2 rsp1a` prints a
coarse spectrum from an RSP1A.

# rsp_open

An open Rust driver for the SDRplay (a maker of software-defined radio receivers) **RSP2pro** (USB 1df7:3010), **RSP1A** (USB 1df7:3000)
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

`examples/rsp_coldcheck.rs` is a one-shot pass/fail check of a receiver (prints one JSON line, exit 0 or 1; a stream that delivers nothing is reported as `stalled`, not waited on). `tools/power_cycle_stress.py` wraps it in a loop: switch the port's power off, check the receiver leaves the bus, switch it on, check it comes back in its cold state (bcdDevice 2.00) with the expected USB ID, then run the check, optionally several open/close cycles per power cycle (`--hammer`). It logs one JSON line per iteration, stops at the first failure with a snapshot (lsusb, kernel log tail) and leaves the device powered for inspection. Power backends: `uhubctl` (a hub that really switches port power), `cmd` (relay boards, smart plugs), `sysfs` (does not cut VBUS (the USB power line) on many machines, including the development laptop, so it is not a real power cycle), `manual`, and `none`. USB port power on a given machine must be verified first: the receiver has to disappear from the bus and come back at bcdDevice 2.00.

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

  Everything else (band tables, board bytes 0x12/0x13 of both models, LNA (low-noise amplifier) tables, ADC (analog-to-digital converter) clock
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
- Tuning in every supported band: the internal 24 MHz harmonics land within one 122 Hz FFT (fast Fourier transform)
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
- The API's close-time VCO (voltage-controlled oscillator) calibration (0x42 read and r13/r14/063000/00800e writes) is not
  replicated; the driver closes the way `an earlier stand-alone verification program` did (works).
- No AGC (automatic gain control), DC/IQ correction or decimation (all host-side in SDRplay's API).
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

### `pub fn check_state(pid: u16, bcd_device: u16) -> Result<(), Error>` [@ANCHOR: rsp_open:rsp_lib_check_state]

Decides, from the USB device descriptor alone, whether a receiver may be driven as a running device. It is called by `Rsp::open_device` before the interface is claimed (so nothing is sent to a device that fails it) and again on the opened transport's descriptor.

| Argument | Type | Meaning |
|---|---|---|
| `pid` | `u16` | USB product ID; must be in `SUPPORTED_PIDS` (0x3010 RSP2/RSP2pro, 0x3000 RSP1A) |
| `bcd_device` | `u16` | descriptor bcdDevice; 0x0207 is the running firmware personality |

Returns `Ok(())` only for a supported PID at 0x0207. Errors: `Error::UnsupportedModel { pid, name }` for any other PID (the RSP1 included), `Error::NotRunning` for 0x0200 (boot ROM, no firmware), `Error::UnknownState { bcd_device }` for any other value. Pure function: no I/O, no locks, no side effects, safe to call from any thread. The boot-ROM paths (`open_boot_rom_with`) deliberately do not use it. Test: `lib.rs::state_detection`.

### `pub fn model_for_pid(pid: u16) -> Model` [@ANCHOR: rsp_open:rsp_lib_model_for_pid]

Maps a USB product ID to the `Model` enum: 0x2500 `Rsp1`, 0x3000 `Rsp1a`, 0x3010 `Rsp2` (covers the RSP2 and RSP2pro), 0x3020 `RspDuo`, 0x3030 `RspDx`, 0x3050 `Rsp1b` (the PID is unconfirmed), 0x3060 `RspDxR2`; every other value gives `Model::Unknown`. The `pid` argument is taken by value and the result is `Copy`; there are no errors, no panics and no I/O. Recognising a model does not mean the crate drives it: `check_state` and the open functions decide that. Used by `usb::list_devices` (via `describe`) to fill `DeviceInfo::model`, by `check_state` for the error text, and by the model-specific openers. Test: `lib.rs::state_detection`.

### `pub fn name(self) -> &'static str` on `Model` [@ANCHOR: rsp_open:rsp_lib_model_name]

Returns the display name of a model for messages and logs: "RSP1", "RSP1A", "RSP2/RSP2pro", "RSPduo", "RSPdx", "RSP1B (unconfirmed PID)", "RSPdx-R2" or "unknown SDRplay device". The string is static, so it is borrowed for the program's lifetime and `Error::UnsupportedModel` stores it directly. No errors, no I/O, no state. The RSP2 and RSP2pro share a USB ID and cannot be told apart from the descriptor, which is why one name covers both. Test: `lib.rs::state_detection` (asserts "RSP1" and "RSP2/RSP2pro").

### `impl std::fmt::Display for Error` (`fmt`) [@ANCHOR: rsp_open:rsp_lib_error_fmt]

Writes the human-readable text of every `Error` variant, so callers can print `{e}` and the relay can show it to the user. Fixed texts: `NotFound` "no supported SDRplay receiver (RSP2/RSP2pro, RSP1A) found"; `NotRunning` "device has no firmware loaded; load it with SDRplay's software first" (a test pins this exact string); `UnknownState` shows bcdDevice in hex; `UnsupportedModel` shows name and PID; `Usb` shows `what`, the libusb error name and code; `InvalidArgument`, `Unsupported` and `Stream` prefix their message; `KernelDriverInUse` names the driver, the device nodes and the holding programs and says to close them first; `Stalled` gives the milliseconds waited. Pure, no side effects; the text is not a stable wire format and must not be parsed. Test: `lib.rs::state_detection`.

### `fn band_for(rf_hz: u64) -> Result<Band, Error>` [@ANCHOR: rsp_open:rsp_encode_band_for]

Looks up the RF band of the RSP2pro (and RSP2) for a frequency. A `Band` (see `encode::BANDS`, PROTOCOL.md 1.9) carries the lower and upper edge in Hz, the tuner mode, the first-LO plan, the 3-bit band-filter code for board byte 0x12 and the gain table. The thirteen entries cover 1 kHz-2 GHz: three AM-mode HF bands, VHF (60-120 MHz), B3 (120-250 MHz), the 250-420 MHz AM down-conversion bands (the 375-420 MHz plans are only bracketed), UHF (420 MHz-1 GHz) and L band. It is a thin wrapper over `band_in` with the RSP2pro table, reached through `model_band`.

| Argument | Type | Meaning |
|---|---|---|
| `rf_hz` | `u64` | RF frequency in Hz, 1000..=2_000_000_000 |

Returns the matching `Band` by value. Error: `Error::InvalidArgument` from `band_in` for a frequency outside the range. Pure, thread-safe, no I/O. Tests: `encode.rs::band_table_matches_capture`, `pll_matches_captures`.

### `fn band_in(bands: &[Band], rf_hz: u64) -> Result<Band, Error>` [@ANCHOR: rsp_open:rsp_encode_band_in]

Finds the band of a frequency in any model's table (the RSP2pro's `BANDS` or the RSP1A's `rsp1a::BANDS`). The range check comes first: a frequency below `RF_MIN_HZ` (1 kHz) or above `RF_MAX_HZ` (2 GHz) is refused before the table is searched; then the first band with `lo_hz <= rf_hz < hi_hz` is returned (a copy). `bands` is borrowed read-only; the top band's upper edge is `RF_MAX_HZ + 1`, so exactly 2 GHz is accepted. Errors: `Error::InvalidArgument("frequency ... outside ...")` for the range, `Error::InvalidArgument("no band for ...")` if the table has a gap. Pure and thread-safe. Called through `model_band` by every tune (`set_frequency`, the open sequence). Test: `rsp1a.rs::rsp1a_band_table_matches_capture`.

### `pub fn lna_table(t: GainTable) -> &'static [FrontEndStep]` (RSP1A) [@ANCHOR: rsp_open:rsp_rsp1a_lna_table]

The RSP1A's LNA-state tables (PROTOCOL.md 2.8), one static slice per `GainTable`: AM 7 states (also used for Hi-Z), VHF 10, AM-down 10, UHF 10, L 9. A `FrontEndStep` here packs two board fields into `board_step` as `attenuator code (port A bits 7:5, one of 0b111, 0b110, 0b101, 0b011) << 1 | port-B bit 1`; `am_buf`, `mixer_gr` and `lna_gr` are the tuner r1 bits as on the RSP2pro. Unlike the RSP2pro, `AmDown` has its own table. Index = LNA state; callers clamp it. Pure. Test: `rsp1a.rs::rsp1a_lna_tables_match_capture`.

### `pub fn board_a_byte(filter: u8, board_step: u8) -> u8` [@ANCHOR: rsp_open:rsp_rsp1a_board_a_byte]

Builds the RSP1A board port A value ("byte 0x12"): attenuator code in bits 7:5 (taken from `(board_step >> 1) & 7`) and the 5-bit band filter in bits 4:0 (`filter & 0x1f`). `filter` comes from the RSP1A band table (`rsp1a::BANDS`, which has an extra filter below 2 MHz); `board_step` is the packed field from `lna_table`. Pure, no errors. Written with request 0x49 by `Rsp::board`. Tests: `rsp1a.rs::rsp1a_band_table_matches_capture`, `rsp1a_lna_tables_match_capture`.

### `pub fn board_b_byte(board_step: u8, fm_mw_notch: bool, dab_notch: bool, bias_t: bool) -> u8` [@ANCHOR: rsp_open:rsp_rsp1a_board_b_byte]

Builds the RSP1A board port B value ("byte 0x13"). Bits 7, 6 and 3 are always 1 and bit 5 always 0 (0b1100_1000 fixed); bit 4 is the bias-T, bit 2 the FM/MW notch and bit 0 the DAB notch, **all active low** (0 = on); bit 1 is the front-end step bit taken from `board_step & 1`. A switch that is off therefore has its bit set. The bias-T feeds DC to the antenna port; the driver never turns it on by itself. Pure, no errors. Tests: `rsp1a.rs::rsp1a_front_byte_bits`, `rsp1a_lna_tables_match_capture`.

### `pub fn open_transient_b(fm_mw_notch: bool, dab_notch: bool) -> u8` [@ANCHOR: rsp_open:rsp_rsp1a_open_transient_b]

The port B value the RSP1A open sequence writes in the middle of the sequence: `board_b_byte(1, ..., bias_t = false)`, i.e. the step bit set and bias-T off. In every captured open both notches were off, giving 0xdf; with a notch on at open the notch bits follow the setting, which is a guess that was not captured (to-do: capture it). The bias-T is always off here. Pure, no errors. `Rsp::transient_b` uses it only for the first full tune after open (no port-B value has been written yet); later tunes reuse the port-B value in force. Test: `rsp1a.rs::rsp1a_front_byte_bits`.

### `fn mode_byte(mode: TunerMode, antenna: Antenna) -> u32` [@ANCHOR: rsp_open:rsp_encode_mode_byte]

Gives the MSi001 band-mode byte that goes in bits 11:4 of tuner register r0 (PROTOCOL.md 1.6.1). Values: AM mode 0xe1 on antenna A or B, 0x61 on the Hi-Z port (the tuner's second AM input); VHF 0x42, B3 0x44, B45 0x48, L 0x50 whatever the antenna. The two arguments are `Copy` enums passed by value; the result is the byte widened to `u32` for `r0`/`r0_plan`. No errors and no I/O; whether Hi-Z is allowed at this frequency is checked elsewhere (`check_antenna`). Tests: `encode.rs::band_table_matches_capture`, `r0_variants`.

### `fn gain_table(band: &Band, antenna: Antenna) -> GainTable` [@ANCHOR: rsp_open:rsp_encode_gain_table]

Chooses which LNA-state table applies. Normally it is the band's own `gain_table`; the exception is the Hi-Z port in an AM-mode band whose upper edge is at or below `HIZ_MAX_HZ` (60 MHz), which uses `GainTable::HiZ` (five states). `band` is borrowed, `antenna` is by value; the result is a `Copy` enum that `lna_table` turns into the table. Pure, no errors, no I/O. The device layer's `model_table` adds the RSP1A's own rules on top. Test: `encode.rs::hiz_uses_its_own_gain_table_only_below_60_mhz`.

### `fn lna_table(t: GainTable) -> &'static [FrontEndStep]` (RSP2pro) [@ANCHOR: rsp_open:rsp_encode_lna_table]

Returns the RSP2pro's LNA-state table for a gain table: a static slice whose index is the LNA state the user sets with `set_gain`. Sizes: AM 9 states (`AmDown` shares it), VHF 9, UHF 6, L 6, Hi-Z 5. Each `FrontEndStep` holds `board_step` (board port B bits 5:2, one of 0b1110, 0b1101, 0b1001, 0b0101), `am_buf` (r1 bits 11:10, an AM buffer gain step 0-3) and the booleans `mixer_gr` (mixer gain reduction) and `lna_gr` (LNA gain reduction), which `r1()` and `board_front_byte` turn into register bits. Values come from the gain and toggle captures (PROTOCOL.md 1.8). Callers must keep the LNA state below the slice length (`set_gain` and `set_frequency` clamp it). Pure; `rsp1a::lna_table` is the RSP1A's counterpart. Test: `encode.rs::gain_words_match_capture`.

### `fn lo_hz(band: &Band, rf_hz: u64) -> u64` [@ANCHOR: rsp_open:rsp_encode_lo_hz]

Computes the first local-oscillator frequency in Hz for an RF frequency, following the band's `lo_plan`: `Direct` gives `rf_hz`; `UpBy120M` gives `rf_hz + 120 MHz` (up-conversion below 60 MHz); `DownBy120M`, `DownBy144M` and `DownBy168M` subtract 120, 144 and 168 MHz. The result feeds `pll` together with `TunerMode::lo_div`. `rf_hz` must already be inside `band` (callers get the band from `band_in`), otherwise the subtraction could underflow, which `pll` then refuses only if the result is zero; a debug build would panic on a true underflow. Pure, no I/O. Tests: `encode.rs::pll_matches_captures`, `pll_lo_is_close`.

### `fn tunermode_lo_div` (`TunerMode::lo_div(self) -> u64`) [@ANCHOR: rsp_open:rsp_encode_tunermode_lo_div]

Returns the divider between the tuner's VCO and its local-oscillator output for a tuner mode (PROTOCOL.md 1.6.2): AM 16, VHF 32, B3 16, B45 4, L 2. `pll` multiplies the wanted LO frequency by it to find the VCO frequency, so a wrong value moves the receiver far off frequency. The mode is by value; no errors, no I/O. Test: `encode.rs::pll_matches_captures` (covers every band).

### `fn pll(lo_hz: u64, div: u64, ppb: i64) -> Result<Pll, Error>` [@ANCHOR: rsp_open:rsp_encode_pll]

Calculates the tuner PLL words r2 and r3 for a first-LO frequency (PROTOCOL.md 1.6.2). The VCO runs at `lo_hz * div` from the 96 MHz doubled reference; one PLL unit is 32 kHz of VCO, the integer part is `n`, the fraction `frac` is out of 3000 and r3 carries 12 more fraction bits (`sub`). The value written is the largest integer strictly below the exact quotient (computed in exact 128-bit integers), which fits every captured pair (100 MHz gives r2 0x213e72, r3 0x00fff3).

| Argument | Type | Meaning |
|---|---|---|
| `lo_hz` | `u64` | first-LO frequency from `lo_hz()`, Hz |
| `div` | `u64` | LO divider from `TunerMode::lo_div` (2, 4, 16 or 32) |
| `ppb` | `i64` | reference correction in parts per billion, positive = reference fast (from `ppm_to_ppb`) |

Returns a `Pll` (`n`, `frac`, `sub`, the words `r2` and `r3` ready to send, and `lo_hz` actually produced at nominal reference). Errors: `Error::InvalidArgument` when `1_000_000_000 + ppb` is not positive or when `n` is outside 1..=255 (this also refuses a zero LO instead of underflowing). Pure; called from `full_tune`, `set_frequency` and `set_ppm`. Tests: `encode.rs::pll_matches_captures`, `pll_ppm_matches_capture`, `a_zero_lo_frequency_is_refused_not_a_panic`.

### `fn code(self) -> u32` on `IfMode` [@ANCHOR: rsp_open:rsp_encode_ifmode_code]

Gives the 2-bit code of an IF mode for MSi001 r0 bits 13:12: `Zero` 3, `Low450k` 2, `Low1620k` 1, `Low2048k` 0. `r0_plan` shifts it into place. Pure, no errors. Note the codes run opposite to the IF frequency. Tests: `encode.rs::r0_variants`, `band_table_matches_capture`.

### `fn hz(self) -> u32` on `IfMode` [@ANCHOR: rsp_open:rsp_encode_ifmode_hz]

Gives the IF frequency of the mode in Hz: 0 for `Zero`, 450 000, 1 620 000 or 2 048 000 for the three low-IF modes. Used to work out the output sample rate and by callers that do the host-side IF down-conversion (the driver does not; where RF lands in the low-IF output is not yet characterised, see Not implemented). Pure, no errors. Test: `encode.rs::if_and_bandwidth_helpers`.

### `fn required_adc_hz(self) -> Option<u32>` on `IfMode` [@ANCHOR: rsp_open:rsp_encode_ifmode_required_adc_hz]

Returns the only ADC clock a low-IF mode was observed with: `Low450k` 2.000 MHz, `Low1620k` 6.000 MHz, `Low2048k` 8.192 MHz, and `None` for `Zero` (any rate in range). `stream_config` refuses a different clock with `Error::Unsupported`, and `set_sampling` uses it to pick the clock when the mode changes. Pure, no errors. Tests: `encode.rs::if_and_bandwidth_helpers`, `stream_configs_match_capture`.

### `fn code(self) -> u32` on `Bandwidth` [@ANCHOR: rsp_open:rsp_encode_bandwidth_code]

Gives the 3-bit IF-bandwidth code for MSi001 r0 bits 16:14: 200k 0, 300k 1, 600k 2, 1.536M 3, 5M 4, 6M 5, 7M 6, 8M 7. `Bandwidth::hz` indexes its table with this code, so the two must stay in step. Pure, no errors. Tests: `encode.rs::r0_variants`, `band_table_matches_capture`.

### `fn at_most(hz: u32) -> Bandwidth` on `Bandwidth` [@ANCHOR: rsp_open:rsp_encode_bandwidth_at_most]

Chooses the widest IF bandwidth that does not exceed `hz` (Hz, any value); anything below 200 kHz still returns `K200`, the narrowest, and anything at or above 8 MHz returns `M8`. It scans all eight values in increasing order and keeps the last that fits. No errors, no I/O; used when a caller wants a bandwidth matching a sample rate. Test: `encode.rs::if_and_bandwidth_helpers`.

### `fn board_band_byte(filter: u8, antenna: Antenna, ext_ref_out: bool) -> u8` [@ANCHOR: rsp_open:rsp_encode_board_band_byte]

Builds the RSP2pro's board port A value (MCP23S18 GPIOA, "byte 0x12"), written with vendor request 0x49. Layout: bits 1:0 antenna (0b01 for A and Hi-Z, 0b10 for B), bits 5:3 the band-filter code (`filter & 7`), bit 2 always set, bit 6 the reference-clock output (clear = output enabled, because the expander outputs are open-drain; set when `ext_ref_out` is false). Bit 7 is 0. Arguments: `filter` from `Band::filter`, `antenna` by value, `ext_ref_out` the user setting. No errors, no I/O. Caller: `Rsp::band_byte` for the RSP2pro; the RSP1A uses `rsp1a::board_a_byte`. Tests: `encode.rs::board_bytes`, `band_table_matches_capture`.

### `fn board_front_byte(board_step: u8, notch: bool, bias_t: bool) -> u8` [@ANCHOR: rsp_open:rsp_encode_board_front_byte]

Builds the RSP2pro's board port B value (MCP23S18 GPIOB, "byte 0x13"): bits 7:6 the broadcast notch (0b01 on, 0b10 off), bits 5:2 the front-end step (`board_step & 0xf` from `FrontEndStep`), bit 0 the bias-T, which is **active low** (0 = bias-T on, 1 = off), bit 1 unused (0). Arguments: `board_step` from the LNA table, `notch` and `bias_t` the current settings. The bias-T supplies DC on the antenna port, so the driver refuses to open with it on and no test or example sets it. No errors, no I/O. `board_front_transient` calls it for the retune transient. Tests: `encode.rs::board_bytes`, `gain_words_match_capture`.

### `fn stream_config(adc_hz: u32, if_mode: IfMode) -> Result<StreamConfig, Error>` [@ANCHOR: rsp_open:rsp_encode_stream_config]

Picks the USB alternate setting, sample format and MSi2500 register 0x07 value for an ADC clock (PROTOCOL.md 1.7). Alt 2 carries one 1024-byte block per microframe (8000 blocks/s), alt 1 three (24000/s); the widest format whose block rate holds the clock is used (S14, S12, S10x384, S8). Only (alt, format, reg07) combinations seen on the wire are produced.

| Argument | Type | Meaning |
|---|---|---|
| `adc_hz` | `u32` | ADC clock, 2_000_000..=10_000_000 Hz |
| `if_mode` | `IfMode` | a low-IF mode forces its own clock and a decimate-by-2 config |

Returns `StreamConfig { alt, format, reg07, decim2 }`. Errors: `InvalidArgument` outside 2-10 MHz; `Unsupported` when a low-IF mode is combined with another clock, or for 2.688-3 MHz (never observed). Pure; its result sets the `BlockParser` and the alt setting. Test: `encode.rs::stream_configs_match_capture`.

### `fn counter_step(&self) -> u32` on `StreamConfig` [@ANCHOR: rsp_open:rsp_encode_streamconfig_counter_step]

Gives how far the 32-bit sample counter in each block header advances from one 1024-byte block to the next: the format's pairs per block, doubled when the device decimates by 2 (low-IF modes). `BlockParser::new` stores it as the expected step, so a mismatch is counted as a gap. Examples: S14 252, S8 504, low-IF S14 504. Pure, no errors. Test: `encode.rs::stream_configs_match_capture`.

### `fn pairs_per_block(self) -> u32` on `SampleFormat` [@ANCHOR: rsp_open:rsp_encode_sampleformat_pairs_per_block]

Returns the number of I/Q pairs in the 1008-byte payload of one block for each wire format: `S14` 252, `S12` 336, `S10x384` 384, `S8` 504. It underlies `counter_step`, and the stream tests use it to size canned blocks. Pure, no errors. Test: `encode.rs::stream_configs_match_capture`.

### `fn adc_clock(adc_hz: u32, ppb: i64, flag12: bool) -> Result<AdcClock, Error>` [@ANCHOR: rsp_open:rsp_encode_adc_clock]

Computes the MSi2500 ADC clock synthesiser words (registers 0x03 and 0x04, PROTOCOL.md 1.7): `f = 4 MHz * (N + frac/2^21) / (2(d+1))`. The divider is the largest even value up to 16 that keeps `f * divider` within 32-40 MHz (else 32-42 MHz); `frac` is rounded down. The range field of reg 0x03 depends on the clock (<6 MHz 0, <7 MHz 1, up to 8 MHz 2, else 3).

| Argument | Type | Meaning |
|---|---|---|
| `adc_hz` | `u32` | wanted ADC clock, 2-10 MHz |
| `ppb` | `i64` | reference correction in ppb (from `ppm_to_ppb`) |
| `flag12` | `bool` | reg 0x03 bit 12: set on the write at open and on ppm changes, clear on sample-rate changes (as the API does) |

Returns `AdcClock { reg03, reg04, actual_hz }` (actual rate at nominal reference). Errors: `InvalidArgument` for a clock out of range or a synthesiser `N` outside 1..=15, `Unsupported` if no divider fits. Pure. Test: `encode.rs::adc_clock_matches_capture`.

### `fn ppm_to_ppb(ppm: f64) -> Result<i64, Error>` [@ANCHOR: rsp_open:rsp_encode_ppm_to_ppb]

Converts the user's reference-oscillator correction from parts per million to the integer parts per billion that `pll` and `adc_clock` take (`ppm * 1000`, rounded to nearest). A positive value means the reference runs fast. Error: `Error::InvalidArgument` when `ppm` is NaN or infinite or its magnitude exceeds 1000 (a limit that also keeps the PLL divisor positive). Pure, no I/O. Called by `set_ppm` and at open. Test: `encode.rs::ppm_conversion_and_limits`.

### `fn r0_plan(mode_byte: u32, plan: LoPlan, if_mode: IfMode, bw: Bandwidth) -> u32` [@ANCHOR: rsp_open:rsp_encode_r0_plan]

Builds the 20-bit payload of MSi001 register r0: bits 19:17 select the first-LO plan, bits 16:14 the IF bandwidth code, bits 13:12 the IF mode code, bits 11:4 the band-mode byte. The plan bits are 2 for the 120 MHz first IF and for every non-AM mode (the `Direct`, `UpBy120M` and `DownBy120M` plans), 1 for `DownBy144M` and 0 for `DownBy168M`. Arguments are by value (`mode_byte` from `mode_byte()`, `plan` from the band, `if_mode` and `bw` from the current settings). The result is a payload to be wrapped by `tuner_word(0, ...)`; `r0()` is the same call with `LoPlan::Direct`. No errors, no I/O. Test: `device.rs::first_if_plan_bits_follow_the_band_in_375_to_420_mhz`.

- [@ANCHOR: rsp_open:rsp_device_rsp_open_auto] Opens whichever supported receiver is present: a running one if there is one, otherwise the first cold one.
- [@ANCHOR: rsp_open:rsp_device_rsp_open_boot_rom_with] Opens the first receiver in the boot-ROM state (bcdDevice 2.00) and drives it without any firmware.
- [@ANCHOR: rsp_open:rsp_device_rsp_open_boot_rom_model_with] As above, but only a receiver of one model (RSP2pro, RSP1A or RSP1), so a second receiver on the bus is not driven by mistake.
- [@ANCHOR: rsp_open:rsp_device_rsp_need_board] Refuses a board control (antenna, notch, bias-T, reference output) in boot-ROM mode, where the board is not reachable.
- [@ANCHOR: rsp_open:rsp_device_rsp_set_notch] Switches the broadcast notch; `Error::Unsupported` in boot-ROM mode.
- [@ANCHOR: rsp_open:rsp_device_rsp_set_bias_t] Switches the bias-T (off at open, active low on the board); `Error::Unsupported` in boot-ROM mode.
- [@ANCHOR: rsp_open:rsp_device_sampleiter_next_chunk] The next sample chunk, or `Error::Stalled` when nothing arrived within the timeout, or `Error::Stream` when the stream ended.
### `pub(crate) fn inspect_in(sys: &Path, dev: &Path, proc_: &Path, bus: u8, address: u8) -> Result<Option<KernelDriverUse>, Error>` [@ANCHOR: rsp_open:rsp_kernel_inspect_in]

Finds out whether a kernel driver is bound to the receiver at `bus`/`address` and who uses its device nodes. `inspect` calls it with `/sys`, `/dev` and `/proc`; tests pass a fake tree. It reads only: it scans `sys/bus/usb/devices` for the device directory whose `busnum`/`devnum` match, follows the `<dev>:1.0/driver` symlink for the driver name, finds the nodes by following `sys/class/video4linux/*/device` back to the interface, then calls `holders_of`.

Returns `Ok(None)` if there is no sysfs, no matching device or no bound driver, otherwise `Ok(Some(KernelDriverUse { driver, nodes (sorted), holders, holders_complete }))`. It never returns `Err` today. Nothing is opened, detached or changed, no locks are taken, and the result is a snapshot that can be stale a moment later. Test: `kernel.rs::bound_driver_with_a_process_using_the_node`.

### `fn holders_of(proc_: &Path, nodes: &[PathBuf]) -> (Vec<Holder>, bool)` [@ANCHOR: rsp_open:rsp_kernel_holders_of]

Lists the processes that have one of `nodes` open, by reading `proc_/<pid>/fd` links, with the name from `comm`. Returns `(holders sorted by pid, complete)`. `complete` is false when a `/proc/<pid>/fd` directory gave permission denied (another user's process, so it may hold a node unseen) or `/proc` could not be read; processes that exit meanwhile are skipped. With no nodes it returns an empty list and true at once. Read-only, no side effects; cost is one directory read per process. Used to refuse taking a receiver from under a streaming program. Test: `kernel.rs::bound_driver_with_a_process_using_the_node`.

### `pub fn push_packet(&mut self, pkt: &[u8], out: &mut Vec<IqI16>)` on `BlockParser` [@ANCHOR: rsp_open:rsp_stream_blockparser_push_packet]

Parses one isochronous USB packet. Each whole 1024-byte block is split off, its little-endian counter (bytes 0-3) is compared with the expected one (previous plus `counter_step`, with 32-bit wrap), the payload after the 16-byte header is decoded to `out`, and statistics are updated. A packet whose length is not a multiple of 1024, or is empty, adds one to `short_packets`; trailing bytes are ignored.

| Argument | Type | Meaning |
|---|---|---|
| `pkt` | `&[u8]` | one packet, normally 1 to 3 blocks |
| `out` | `&mut Vec<IqI16>` | samples are appended, never cleared |

A counter mismatch increments `stats.gaps` and adds the distance to `stats.lost_samples`; the first block after construction or `reset_counter` is not checked. No errors or panics. The parser is not thread-safe: it runs on the USB thread inside the packet sink. Tests: `stream.rs::s14_packet_with_gap`, `counter_wraps`, `three_blocks_per_packet_and_decim2_step`.

### `pub fn decode_payload(format: SampleFormat, p: &[u8], out: &mut Vec<IqI16>)` [@ANCHOR: rsp_open:rsp_stream_decode_payload]

Decodes the 1008-byte payload of one block to 16-bit left-aligned `IqI16` samples appended to `out`. S14: little-endian 16-bit words, I then Q, 14-bit signed, shifted left 2. S12: three bytes per pair, I the low 12 bits and Q the high 12, shifted left 4. S8: two signed bytes per pair, shifted left 8. S10x384: six groups of 164 bytes, each 16 runs of 10 bytes holding four I/Q pairs of 10-bit values plus a 32-bit word with a 2-bit scale per run (shift 4, 5 or 6); the last 24 bytes carry no samples. `p` must be exactly `PAYLOAD_LEN` (1008) bytes: a debug build asserts it and a shorter slice would panic in the group indexing; `push_packet` always passes the right length. Pure apart from the append. Tests: `stream.rs::s14_packet_with_gap`, `s12_decode`, `s384_decode`, `s8_decode`.

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
- [@ANCHOR: rsp_open:rsp_device_rsp_write_bandwidth] The bandwidth writes themselves; `set_bandwidth` restores the cached bandwidth when they fail. The other setters (`set_gain`, `select_antenna`, `set_notch`, `set_dab_notch`, `set_bias_t`, `set_ext_ref_out`) likewise restore the cached value when their write fails, as `set_frequency` and `set_ppm` do, so a later full tune never builds port B (the bias-T bit included) from a value the hardware did not accept.
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

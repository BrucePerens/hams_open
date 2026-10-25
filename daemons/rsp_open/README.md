# rsp_open

An open Rust driver for the SDRplay **RSP2pro** (USB 1df7:3010), library crate plus two
examples. Status: works on the author's RSP2pro as of 2026-10-25; see
"What works" below.

**Licence: TODO (Bruce to choose).** No licence has been granted yet; do not publish or
redistribute until one is chosen. The `Cargo.toml` has `publish = false`.

## Important limitation: firmware

The RSP2pro powers up in a boot-ROM state (bcdDevice 2.00). SDRplay's software uploads a
firmware image that turns it into the running device (bcdDevice 2.07). This crate contains
**no firmware and no firmware-loading code**. It drives only an already-running receiver,
i.e. one that SDRplay's software has opened at least once since power-up, and reports
`Error::NotRunning` ("device has no firmware loaded; load it with SDRplay's software first")
otherwise. Any other bcdDevice is refused (`Error::UnknownState`).

## Provenance (clean-room record)

- **Specification:** a private description (not published here) of the
  USB traffic between SDRplay's software and the author's own receiver, recorded with usbmon and
  filtered to the receiver only. No SDRplay binary was disassembled, decompiled, traced or
  strings-mined; no SDRplay code or documentation text was used.
- **Extra wire facts:** register values in the unit tests come from the decoded captures
  the private capture files c20..c26 (the request sequences the API sent), read with
  our own capture-decoding scripts. The r3 "12 extra PLL fraction bits"
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

  Everything else (band table, board bytes 0x12/0x13, LNA tables, ADC clock rules, alt and
  format selection, open/close sequences) comes from the wire captures.

- **Requests sent:** only those the protocol document lists as observed: 0x41 (register
  write), 0x43/0x45 (start/stop), 0x48 (one-byte read at open), 0x49 (board bytes 0x00,
  0x01, 0x12, 0x13), SET_INTERFACE alt 0/1/2, and the standard string-descriptor read.
  The unit test `only_observed_requests_are_emitted` exercises every API call against a
  recording mock and fails on anything else. Never sent: 0x40, 0x42, 0x44, 0x46, 0x47, bulk
  alt 3, USB resets.

## API

```rust
use rsp_open::{Rsp2, Antenna, Bandwidth, IfMode, Settings};
let mut rx = Rsp2::open()?;                    // first running RSP2/RSP2pro, defaults below
rx.set_frequency(7_074_000)?;                  // 1 kHz - 2 GHz except 375-420 MHz
rx.set_sample_rate(6_000_000)?;                // 2.0, 2.048 or 3..10 MHz ADC clock (zero IF)
rx.set_if_mode(IfMode::Low1620k)?;             // or low IF with its fixed ADC clock
rx.set_bandwidth(Bandwidth::K1536)?;
rx.set_gain(40, 0)?;                           // IF gain reduction dB (0..59), LNA state
rx.select_antenna(Antenna::HiZ)?;              // A, B, or HiZ (< 60 MHz)
rx.set_notch(true)?; rx.set_bias_t(false)?; rx.set_ext_ref_out(false)?; rx.set_ppm(0.0)?;
rx.start_stream(|samples, stats| { /* &[IqI16], runs on the USB thread */ })?;
// or: let it = rx.start_stream_iter(64)?; for chunk in it { ... }
rx.stop_stream()?;
rx.close()?;                                   // also on drop
```

Defaults: 100 MHz, Antenna A, zero IF, 2 MHz ADC clock, BW 1.536 MHz, IF GR 40, LNA 0,
notch/bias-T/ext-ref off. Samples are `IqI16` left-aligned to 16 bits whatever the wire
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
  1.62 M / 2.048 M modes); `set_frequency` writes the same LO as in zero IF, as the API did,
  and the host-side IF down-conversion is left to the caller. Hi-Z port: switching verified
  on the mock and on hardware via `rsp_measure` (floor changes), no signal test.
- `examples/rsp_measure.rs` characterises the front end; results in
  `docs/measure_2026-10-25.txt` and in the private hardware notes.

## Not implemented / open

- **375-420 MHz**: the API switches the AM-mode first IF to 144/168 MHz there and runs a
  tuner VCO calibration (request 0x42 read, r13/r14 writes) whose byte mapping is only
  partly known; returns `Error::Unsupported`.
- The API's close-time VCO calibration (0x42 read and r13/r14/063000/00800e writes) is not
  replicated; the driver closes the way `an earlier stand-alone verification program` did (works).
- No AGC, DC/IQ correction or decimation (all host-side in SDRplay's API).
- Low-IF modes only with their observed ADC clocks; ADC clocks between 2.688 and 3 MHz and
  above 10 MHz refused.
- Bias-T is never switched on by tests or examples (nothing is connected to the ports).
- Other models (RSP1, RSP1A, RSPduo, RSPdx) are recognised by PID and refused.

## Tests

```
CARGO_TARGET_DIR=... cargo test -j 3                     # unit tests (no hardware)
cargo test -j 3 --release --test hil -- --ignored --nocapture --test-threads=1   # hardware
```

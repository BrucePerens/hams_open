# vnwa_open

An open Rust driver for the DG8SAQ **VNWA 3E** vector network analyzer (USB 20a0:4118, plus two PCM2900C
audio codecs), written from observed USB and audio traffic of the owner's own unit. Status (2026-10-26): it measures.
`usb` sends the confirmed requests (control transfers only, no reset, no interface claim), `capture` records the two
codecs (48 kHz, 16 bit, stereo: all they offer) and `sweep` turns a frequency list into uncalibrated S11 and S21 on the
loopback. Calibration (`cal`): one-port open/short/load and through, tested on synthetic error terms; the open/short/load standards are not yet measured on the unit. `bench_mcp` uses it (`bench_vna_sweep`). Examples: `probe`, `sweep`.

Licence: AGPL-3.0-or-later. Not published to crates.io (`publish = false`). No vendor code and no vendor firmware
is used, loaded or flashed.

## What is decoded

See `docs/PROTOCOL.md` for the evidence. In short, all control is vendor control transfers on endpoint 0 (the
measured signal arrives as audio from the two codecs):

| Item | Request | Status |
|---|---|---|
| Generator frequency | 0x4c | confirmed at 1, 2, 10, 20 MHz |
| Generator on, off, level (`generator.rs`) | 0x62, 0x61, 0x98, 0x9a, 0x0e, 0x04, 0x58, 0x91, 0x66 | sequences from a capture of the vendor program; the output is verified on a scope (tone at the commanded frequency, about -17 dBm, flat to 10 MHz); the level steps are exact to 0.1 dB from 0 to -15 dB (take a negative attenuation); below -20 dB the commanded amplitude is tested for every 1 dB step to -50 dB (only the rounding of the 14-bit count: 0.08 dB at -50 dB); the scope check below -19 dB is not done, because the averaged time-domain reading loses its trigger (scope-limited, needs the scope FFT) |
| RF level | 0x58 (14 bits in `wValue`) | confirmed |
| Sweep frequency word and per-chip tuning words | 0x49 | confirmed at 1 to 100 MHz (nRF 10, nLO 9) |
| Offset between old and new frequency | 0x49 tail | confirmed, 14 values |
| IF (12 kHz / 6 kHz) | 0x42, 0x41 | confirmed by the codec audio |
| 18 kHz burst before a sweep | 0x43 | probable |
| The whole set-up block (nine requests) | 0x40, 0x41, 0x42, 0x43, 0x44, 0x47, 0x4b, 0x49, 0x64 | built by `setup::setup_block`, verified byte for byte against eight captured blocks; the time-per-point bytes against 84 settings from 0.2 to 100 ms |
| Everything else in the set-up block (0x44, 0x47, 0x4b and the constants in 0x40), the step counter, the audio to S-parameter maths | | not decoded |

| Audio to a complex measurement (`audio::measure_point`) | codec audio | tested on synthetic tones, on real loopback samples at 12, 6 and 4 kHz, and compared with the vendor program's own S21 (0.1 to 1 percent apart at three IFs, both codecs, four RF frequencies) |

**What the audio step is and is not.** It finds the amplitude and phase of the tone at the IF in whole IF periods (skipping
presamples and postsamples) and divides the left channel by the right (reference) channel; `vendor_convention` conjugates the
result to the vendor program's sign. Against the vendor program's own uncalibrated S21 on a TX-to-RX loopback (codec 2) it agrees
to 0.7 to 0.9 percent in magnitude and phase at 1, 10, 20 and 30 MHz, and to 1.5 percent rms over the first 15 points of a sweep when
the points are laid out in time (the unit drifts smoothly with time while it settles; the loopback is flat in RF frequency). It also
reproduces the unit's IF-dependent response (|S21| 0.874 at IF 12 kHz down to 0.767 at 1 kHz, nine IFs) to 0.1 to 1 percent on both codecs
(`Reference::Left` for codec 1, `Reference::Right` for codec 2). Not covered: a device under test with a frequency-dependent response, the open/short/load/through calibration (now `cal`, see below); the low-frequency range needs no special filtering down to 200 Hz
and the time-domain transforms with their windows and time gating (`timedomain::gate`) are in `timedomain` (details in `docs/PROTOCOL.md`).

The two DDS (direct digital synthesis, programmable signal generator) chips are an AD9859 and an AD9951 (identical register maps, no sweep engine), clocked from a 12 MHz TCXO (temperature-compensated crystal oscillator)
times a premultiplier of 3, then times nRF = 10 (RF, 360 MHz) and nLO = 9 (LO, 324 MHz).

## Modes this hardware can serve (per the vendor's Help file)

All of these are built from the same operations (set the two DDS frequencies and levels, set the IF, read the audio),
so an open driver can offer them once those are decoded:

1. **Vector network analyzer**: S11, S21 (both at once on a 3E), with the open/short/load/through calibration.
2. **Signal generator**: CW with variable level; on a VNWA3/3E also AM or FM with a sine up to about 3 kHz. Output is
   about -17 dBm into 50 ohms at 1 MHz, falling with frequency, and it cannot run during a sweep. AM and FM are decoded and verified on a scope (`generator::modulate`); pulse is not.
3. **Frequency meter**: the phase of the RX input against the unit's reference at equal time steps; accuracy is the
   12 MHz TCXO's (about 0.3 ppm in the vendor's own comparison). Needs no calibration kit.
4. **Spectrum analyzer** (rudimentary, per the vendor): the LO is unfiltered so ghost lines appear, worst at higher
   frequencies; use below about 100 MHz. The unit is a direct-conversion receiver in this mode, so it also responds to
   subharmonics.

## Reliability (2026-10-28)

The unit is bus powered on a power-switching hub port (`sudo -n uhubctl -l 3-2 -p 2 -a off`, then `-a on`: a verified cold reset,
the unit goes dark, keeps no settings, and is back about 9 s after power-on). The driver handles the failures seen on it:

* **Transient transfer errors** (`transport::classify`: libusb reports the kernel's EPROTO as an I/O error; a stall is a pipe error;
  also timeouts) are sent again up to 3 times (`usb::Vnwa`), 5 ms apart, and counted (`Vnwa::resends`). A vanished unit is
  reported at once, other errors are not repeated. The read of request 0x0d after a block is stalled by the unit in about 7 % of
  tries and succeeds on a resend; since the transfers are no longer ignored, a transfer that fails for good fails the point.
* **A point that fails** (no signal on the reference, an unsteady tone, missing audio, a transfer error that outlasted its
  resends) is measured again `Config::retries` times; if the config names a hub (`Config::hub`), the unit is then cold reset once,
  reopened, and the retries repeated; then a clear error lists every attempt (`sweep::recover`). A failed attempt never
  yields a number. `Vna::recovery()` reports the re-measurements, transfer resends and power cycles.
* **Which codec is which** is read from the USB tree (`topology`): the PCM2900C on the lower port of the unit's internal hub is S11,
  the other S21; the card names (`CODEC`, `CODEC_1`) are kernel order. Checked over 5 power cycles: the order did not change here.
* `examples/power_cycle.rs <hub> <port>` does the cold reset; `VNWA_HUB=3-2:2 examples/sweep` sweeps with recovery.

## Safety

The first-run overclock offer of the vendor program was declined; this crate assumes the stock clocks (360 / 324 MHz,
inside the data sheets' 400 MSPS (million samples per second) limit). The identity table the vendor program reads at start-up is not used.

## Tests

`cargo test` checks every encoding against bytes seen on the wire (generator word, sweep word, per-chip words,
offset, IF word).

## Checks this crate makes (anchors)

Each is a test against bytes or samples recorded from the vendor program.

### `protocol::tuning_word(freq_hz: f64) -> u32` [@ANCHOR: vnwa_open_tuning_word]

Converts a frequency in Hz to the 32-bit DDS (direct digital synthesis) tuning word of the RF chip: `round(f * 2^32 / 360 MHz)`, where 360 MHz is `DDS_CLOCK_HZ` (the stock clock of the observed unit). Argument `freq_hz` is the wanted generator frequency. It returns the word with no error path: the cast saturates, so a negative or NaN input gives 0 and a frequency beyond the clock gives a word for a different frequency, which is why `generator::check_frequency` guards the generator path before this is used. Pure, no I/O. Evidence: the test checks the words the vendor program sent for 1, 2, 10 and 20 MHz (`0x00b60b61`, `0x016c16c1`, `0x071c71c7`, `0x0e38e38e`).

### `generator::level_for_attenuation(db: f64) -> Result<u16, String>` [@ANCHOR: vnwa_open_generator_level]

Turns a generator attenuation into the unit's 14-bit amplitude: `round(16383 * 10^(dB/20))`. Argument `db` runs from 0 down to -50 dB (`MIN_ATTENUATION_DB`); anything outside, or NaN, is an error string naming the range. The vendor program sent 52, 163 and 518 for -50, -40 and -30 dB, within one count of this formula (its own rounding is not known). Returns the count only; sending is done by `generator::on`. Pure. Output level note: the generator is a low-level RF source on the TX port, about -17 dBm at full scale; this is the amplitude limit that keeps it so.

### `protocol::sweep_word(freq_hz: f64, n_rf: u8, n_lo: u8) -> u64` [@ANCHOR: vnwa_open_sweep_word]

Builds the 40-bit sweep-frequency word carried in bytes 4 to 8 of request 0x49: `round(f * 2^40 / (36 MHz * nRF * nLO))`, the frequency normalised to both chips' clock multipliers so that each chip's own tuning word is one integer multiply of it (`chip_tuning_word`). Arguments: `freq_hz` the point's frequency, `n_rf` and `n_lo` the two chips' clock multipliers (10 and 9 on the observed unit). Returns the word as a `u64`; the caller (`setup::setup_block`) refuses one that does not fit in 40 bits. Pure, no I/O, no error path. Evidence: matches the words of the captured sweeps at 1 to 100 MHz.

### `protocol::chip_tuning_word(sweep_word: u64, n_other: u8) -> u32` [@ANCHOR: vnwa_open_chip_tuning_word]

Derives one chip's 32-bit tuning word from the sweep word: `floor(word * n_other / 256)`, where `n_other` is the OTHER chip's multiplier (nLO for the RF chip, nRF for the LO chip). The multiply is done in 128 bits so it cannot overflow. For the RF chip the result equals the word the unit reports when the generator setting is read back (request 0x35), which is how the formula was confirmed; the LO-chip use follows the same rule and is checked against the captured per-chip words. Pure, no I/O, no error path.

### `protocol::if_word(if_hz: f64) -> u32` [@ANCHOR: vnwa_open_if_word]

Encodes an intermediate frequency as the 32-bit little-endian word of request 0x42 (bytes 0 to 3): `round(if * 256 * 2^32 / 324 MHz)`, where 324 MHz is the LO chip's clock. The same encoding is used for request 0x43 (the 18 kHz burst before a sweep) and 0x44 (300 kHz). Argument `if_hz` is the IF (12 kHz and 6 kHz confirmed by the tones in the codec audio). Pure, no I/O, no error path: out-of-range values saturate in the cast, so callers choose sane IFs.

### `protocol::if_scaled_fields(if_hz: f64) -> (u32, u16)` [@ANCHOR: vnwa_open_if_scaled_fields]

Returns the two fields of request 0x41 that scale with the IF: a 32-bit word `round(IF * 10944 * 2^32 / 324 MHz)` and a 16-bit field `round(IF * 0.466)`. The request's payload is the 32-bit word, the 16-bit field and the byte 0x02, all little endian, assembled by `setup::setup_block`. Confirmed for IF 12 kHz, 6 kHz and 4 kHz. What the fields drive on the unit is not known, and the constants 10944 and 0.466 are unexplained (fits to captured data), so an IF other than the three confirmed ones is unverified. Pure, no I/O, no error path.

### `protocol::point_time_byte(milliseconds: f64) -> u8` [@ANCHOR: vnwa_open_point_time_byte]

Computes the first byte of request 0x40's payload for a time per point: `ceil(1.5 * ms)` clamped to 1 to 255 (units of about 0.667 ms, the program's counting unit; a tiny epsilon stops floating-point error pushing exact multiples up). Argument `milliseconds` is the time per point (0.2 to 100 in practice). Pure, no error path. It replaced an earlier rounding rule that fitted only the settings seen then; the test now reproduces 84 observed settings from 0.2 to 100 ms. `setup::point_time_fields` derives the other two timing bytes from this one.

### `protocol::sweep_offset_tail(previous_hz: f64, new_hz: f64, counter: u8) -> [u8; 6]` [@ANCHOR: vnwa_open_sweep_offset]

Builds the six-byte signed little-endian tail of request 0x49's payload, the offset between the old and the new frequency: `round((previous - new) / divisor * 2^48 / 360 MHz)` with `divisor = 3 * max(counter - 1, 1)` (6 when the payload's first byte is 3, 9 when it is 4). Arguments: the frequency before the sweep started, this point's frequency, and the step counter. Returns the low six bytes of the 64-bit result. Pure, no error path. The divisor rule is unexplained and verified only for counters 3 and 4, so `setup::setup_block` refuses other counters when the frequency changes. Evidence: 14 captured offset values.

### `protocol::generator_frequency(freq_hz: f64, value: u16) -> Request` [@ANCHOR: vnwa_open_generator_frequency]

Builds the control request that sets the generator frequency (vendor request 0x4c, host to device, type 0x40): an eight-byte payload of four zero bytes then the little-endian `tuning_word`. Argument `value` is the `wValue`, 0x090a as the vendor program sends it while stepping the generator, and 0 or 0x00c8 in the first set-up; its meaning is not decoded. Returns the `Request` only, nothing is sent here; it is sent by `Vnwa::send`. It does not range-check the frequency (`generator::check_frequency` does). Pure. Evidence: the generator payload test against captured bytes.

### `protocol::rf_level_value(level: u16) -> Result<u16, String>` [@ANCHOR: vnwa_open_rf_level]

Validates an RF level for request 0x58, which carries it in `wValue` (a device-to-host read with no data stage): 14 bits, 0 to 0x3fff. Argument `level` is the count. Returns it unchanged when it fits; otherwise an error string naming the value and the maximum 0x3fff, so a value that would silently wrap in a 14-bit field is refused before anything is sent. Pure, no I/O. The generator output is low level (about -17 dBm at the TX port at full scale), and this is the range check on the amplitude word. Tests: the protocol unit tests.

### `setup::point_time_fields(point_ms: f64) -> Result<(u8, u8), String>` [@ANCHOR: vnwa_open_point_time_fields]

Returns bytes 2 and 5 of request 0x40 for a time per point: `timing_pair(point_time_byte(ms))`. Argument `point_ms` must be 0.2 to 100 ms (the vendor program's range), otherwise an error string naming the range, so a nonsense value never reaches the unit. The rule (byte 5 is `ceil(16 / b0)`, byte 2 the same except 3 at b0 = 5 and both 1 from 16 up) is the one that reproduces all 84 captured settings; why the program carries two such counts is not known. Pure, no I/O. Tests: the timing test in `setup.rs` over `TIMING_OBSERVED`.

### `setup::setup_block(p: &SetupParams) -> Result<Vec<Request>, String>` [@ANCHOR: vnwa_open_setup_block]

Builds the nine vendor requests the vendor program sends for each measurement point, always in the order 0x40, 0x41, 0x42, 0x43, 0x44, 0x47, 0x4b, 0x49, 0x64. `SetupParams` carries the point and previous frequency, the chips' multipliers, the IF, time per point, step counter, `mode` (`wValue` of 0x40 and 0x41, meaning unknown) and the 14-bit RF level that goes into `wIndex` of the first seven. Errors (strings): RF level above 0x3fff, a step counter other than 3 or 4 when the frequency changes, a time per point outside 0.2 to 100 ms, or a sweep word that does not fit in 40 bits. Fields not decoded are written as the constants seen on the wire. Pure; it builds the block and does not send it. Tests: the block matches eight captured blocks byte for byte (`VECTORS`).

### `audio::tone_at_if(x: &[f64], fs: f64, if_hz: f64) -> Complex` [@ANCHOR: vnwa_open_tone_at_if]

Finds the complex amplitude of the component at `if_hz` in the samples `x` taken at `fs`: a single-bin discrete Fourier sum scaled by 2/N, so a cosine of amplitude A and phase p returns `A e^(jp)`. It is exact when `x` holds a whole number of IF periods (the caller arranges that with `analysis_span`); otherwise there is leakage. An empty slice divides by zero and gives NaN, so callers pass at least one period. Pure, no I/O, no error path. Tests: exactness on synthetic tones at 12, 6 and 4 kHz to 1e-12 in amplitude.

### `audio::vendor_convention(c: Complex) -> Complex` [@ANCHOR: vnwa_open_vendor_convention]

Returns the complex conjugate of a measurement, which is the vendor program's sign convention for the same quantity (its phase has the opposite sign, as for a mixer whose LO is above the RF). Argument `c` is `measure_point`'s result. Pure, total. It exists so that results can be compared with the vendor program's own S21; established by the reference test against recorded loopback samples, where the two agree to 0.7 to 0.9 percent in magnitude and phase.

### `audio::measure_point(left, right, fs, if_hz, presamples, postsamples, reference, min_reference) -> Result<Complex, String>` [@ANCHOR: vnwa_open_measure_point]

Measures one point from the two channels of a codec: finds the tone at the IF in each channel over whole IF periods between the presamples and postsamples, then divides the measured channel by the reference channel. `Reference::Right` means the left channel is measured (the observed unit's second codec); `Reference::Left` swaps them. `min_reference` is the smallest reference amplitude, in sample counts, accepted as a signal. Errors (strings): channels of different length, too few samples for one IF period plus the pre and post samples, or a reference amplitude below the minimum ("no signal"). Pure, no I/O. Tests: the ratio of two synthetic tones and the agreement with the vendor program's S21.

### `Vnwa::attempt(&self, what, op) -> Result<T, String>` (`usb.rs`, private) [@ANCHOR: vnwa_open_transfer_retry]

The retry wrapper for every control transfer. It runs `op` up to 1 + `TRANSFER_RETRIES` (3) times: a success is returned; a `Transient` error (the intermittent EPROTO, a stall, a timeout) is recorded, counted as a resend if another try follows, and retried after `RETRY_PAUSE` (5 ms, kept short because a long pause shifted the analysis window and made a third of the points unsteady); `NoDevice` and `Other` errors fail at once without a retry. The final error names `what` and the last message. Every try is written to the transfer log. Side effects: a thread sleep per resend and an atomic resend counter; `&self` with an internal mutex, so one handle can be shared. Tests: the scripted-transport tests in `usb.rs`.

### `transport::classify(e: rusb::Error) -> TransferError` [@ANCHOR: vnwa_open_transfer_classes]

Sorts a libusb error into what recovery should do. `Io` (libusb's name for the kernel's EPROTO), `Pipe` (a stall), `Timeout` and `Overflow` are `Transient` (worth sending again); `NoDevice` and `NotFound` are `NoDevice` (the unit left the bus, so a cold reset is the remedy, not a resend); every other error, such as a permission problem or invalid argument, is `Other` and is not repeated. Carries the libusb error text. Pure, total. Tests: the classification test in `transport.rs`.

* The S11 and S21 codecs are told apart by their USB port, whatever order the kernel named the cards in. [@ANCHOR: vnwa_open_codec_topology]

### `power::power_cycle(run, port, off_time, back, ready) -> Result<(), String>` [@ANCHOR: vnwa_open_power_cycle]

Cold-resets the unit by cutting power to its hub port: `uhubctl off`, sleep `off_time` (3 s by default so the supply capacitors discharge), `uhubctl on`, then wait up to `back` for `ready` (the unit and both codecs present; about 8 s in practice). Errors name the failing step: an `uhubctl` failure, or "the unit did not come back within N s of power-on on hub H port P". After it the unit has lost its set-up and generator state, and its device numbers change. The caller must have closed audio streams first. Blocking for seconds. Tests: scripted-runner tests of the order of commands and each failure; the real thing is a hardware test.

* A failing point is measured again, then after one cold reset, and only a success is returned. [@ANCHOR: vnwa_open_recovery]
* This driver's S11 and S21 agree with the vendor program's on the same cables to 0.25 % and 0.6 degrees (S11) and 1 % and 0.6 degrees (S21, a constant 0.8 % offset). [@ANCHOR: vnwa_open_vendor_agreement]
* The one-port error terms are recovered from open, short and load, and a known load is corrected. [@ANCHOR: vnwa_open_cal_one_port]
* A kit definition models real standards, round-trips its file, and the calibration with it removes their imperfections. [@ANCHOR: vnwa_open_cal_kit] [@ANCHOR: vnwa_open_kit_file]
* A stored calibration is refused when the IF, time per point, RF level or cable set-up differ. [@ANCHOR: vnwa_open_cal_valid]
* A calibration survives its text file and a damaged file is refused. [@ANCHOR: vnwa_open_cal_file]
* The windows have their textbook shapes. [@ANCHOR: vnwa_open_window]
* A delay line peaks at its delay in low-pass and band-pass mode, windows lower the sidelobes, a reflection's step settles at its coefficient. [@ANCHOR: vnwa_open_time_domain]

### `meter::tone_offset(measured: &[f64], reference: &[f64], fs: f64, if_hz: f64) -> Result<(f64, f64), String>` [@ANCHOR: vnwa_open_meter_offset]

Returns the measured tone's frequency minus the reference tone's, in Hz, and the standard error of the fit. It cuts both channels into 20 ms segments of whole IF periods, takes the ratio of the tone in each segment (`tone_at_if`), unwraps the phase against the previous segment (valid while the offset stays under 25 Hz), and fits a line to phase against time; the slope over 2 pi is the offset. Dividing by the reference tone is what cancels the codec's clock drift. Errors: fewer than 8 segments ("too little data"), or a segment whose measured or reference tone is nearly zero ("no signal"). The sign follows from the signs of the channels; the sign against a real external signal is NOT verified on the unit. Pure, no I/O. Tests: synthetic tones with drift and noise.

### `power::power_cycle_escalating(run, port, schedule, ready) -> Result<(), String>` [@ANCHOR: vnwa_open_power_escalation]

Runs `power_cycle` for each (off time, wait for the unit) pair in the schedule in turn and stops at the first success. If a cycle fails, the next one uses the next pair, normally a longer off time: the control chip once failed to enumerate after a 3 s off that followed a modulation lock-up, and a second cycle with a longer off time brought it back. If every pair fails the error joins all the failures ("cycle 1 (off 3 s): ...; cycle 2 ..."). An empty schedule gives an empty error. Blocking for the sum of the cycles. Tests: scripted-runner tests of the escalation.

* The soak run compares every sweep with the first, and counts errors, wrong results and hangs separately: `examples/soak.rs`.
* A point is accepted only if the reference tone at the IF is steady across the analysis window (IF error 2 Hz or less, level change 3 % or less); a slipped DDS PLL (phase-locked loop), a retune inside the window or a stage boundary fails it. [@ANCHOR: vnwa_open_stability]

### `Capture::frames_after(&self, mark: u64, skip: usize, n: usize, timeout: Duration) -> Result<Vec<[i16; 2]>, String>` [@ANCHOR: vnwa_open_overrun_window]

Returns `n` stereo frames starting `skip` frames after `mark`, waiting up to `timeout` for them to arrive. If the audio layer reported an overrun anywhere from `OVERRUN_GUARD_FRAMES` before the mark to the end of the window, audio was lost and the timing from the mark is wrong, so the result is an error that `is_overrun_error` recognises ("audio overrun"), never frames; the caller measures the point again from a separate budget and never power cycles for it. Other errors: the stream closed (with a special text when a storm closed it), a storm of overrun reports detected while waiting (it closes the stream), or too few frames in time (naming overruns so far). Blocks on a condition variable up to `timeout`. Tests: capture unit tests with overruns injected.

### `Capture::close(&self)` (`capture.rs`) [@ANCHOR: vnwa_open_capture_close]

Stops the audio stream and releases the codec. It must happen before the unit's power is cut: a stream left open on a codec that vanishes keeps restarting and failing, and the audio layer reports an overrun for each try (about a thousand a second, seen as three storms of roughly 16,700 reports in the 2026-10-29 soak). `frames_after` also calls it itself when it sees a storm of overrun reports with no audio between them, so the window fails at once instead of waiting out its timeout. The stream is dropped outside every other lock, because its worker thread takes the ring lock in its callback and the drop waits for that thread. Idempotent, no error path. After it `is_open` is false and `frames_after` fails. Tests: capture unit tests with the synthetic feed.

### `Capture::mark(&self) -> u64` [@ANCHOR: vnwa_open_capture_mark]

Returns the frame count at this instant, the reference point for `frames_after`. Audio arrives one period at a time (5 ms at the driver default, 85 ms at a 4096-frame period), so frames delivered so far lag real time by up to a period; the mark adds the frames that have passed since the last delivery, computed from the clock by `frames_since`, which makes it equally exact for any ALSA period. Takes the ring lock briefly; no error path. Call it just before the set-up block is sent, so that "skip N frames after the mark" is measured from the send. Tests: capture unit tests, including different periods.

### `monitor::parse_version(b: &[u8]) -> Result<(u8, u8), String>` [@ANCHOR: vnwa_open_firmware_version]

Decodes the reply of request 0xff (`wValue` 3), the unit's firmware version: exactly two bytes, minor then major, returned as `(major, minor)`; the unit under test sends `16 05`, which is version 5.16. Any other length is an error string naming the length received, so a short read is never misread as a version. Pure, no I/O. `Vnwa::firmware_version` reads and parses it, and `temperature_counts` uses it to refuse a unit whose firmware predates the sensor. Tests: the monitor unit tests.

### `monitor::parse_temperature(b: &[u8]) -> Result<u16, String>` [@ANCHOR: vnwa_open_temperature]

Decodes the reply of request 0x85 (`wValue` 3), the unit's internal temperature sensor: exactly two bytes, little endian, as raw counts that rise as the unit warms. The counts are uncalibrated, so only differences are meaningful (the soak uses a change of counts to widen its tolerance). Any other length gives an error string. Pure, no I/O. Tests: the monitor unit tests.

### `Vnwa::recent_transfers(&self) -> Vec<String>` [@ANCHOR: vnwa_open_transfer_log]

Returns the last 16 transfer tries (oldest first), each as epoch milliseconds, what was sent and the outcome (`ok` or the error text), for the evidence record of a failure: `sweep.rs` includes it when a point failed and a retry or power cycle cured it. It copies the ring under a mutex and has no other side effect; it cannot fail beyond a poisoned lock. Not persistent: the log lives as long as the `Vnwa` handle. Tests: the transfer-log test in `usb.rs`.

* A failure that a retry or power cycle cured keeps its evidence (error text, raw channel levels, block send time, overruns, the last 16 transfers), and so does every power cycle of the recovery. [@ANCHOR: vnwa_open_failure_log] [@ANCHOR: vnwa_open_failure_evidence]
* The analysis window is timed from the end of the block send, not from before it. [@ANCHOR: vnwa_open_window_from_send_end]
* The installer runs only the commands on its allow-list (with or without `sudo`), and refuses every other command before it is run. [@ANCHOR: vnwa_open_install_commands]
* A USB device is the VNWA's codec only when it is a PCM2900C that has a VNWA control chip as a sibling on the same parent hub; no other sound card is reserved. [@ANCHOR: vnwa_open_reserved_codec]
* The codec cards are found in the `pw-dump` document of a PipeWire sound server. [@ANCHOR: vnwa_open_sound_server_pipewire]
* The codec cards are found in the text of `pactl list cards` of a PulseAudio sound server. [@ANCHOR: vnwa_open_sound_server_pulse]
* A sweep can measure a configured number of points at its start frequency first and throw the results away, to bring the unit to a steady state. [@ANCHOR: vnwa_open_discard_points]
* The three one-port error terms come from the three standards by solving a 3x3 complex system; standards that are not distinct give no solution. [@ANCHOR: vnwa_open_cal_solve3]
* A calibration is built from the open, short, load and through sweeps of one rising frequency grid; open, short and load must come together, sweeps of another length, a through with no signal or nothing to calibrate are refused. [@ANCHOR: vnwa_open_cal_build]
* The reflection of a standard is moved back through its offset line (delay and loss), there and back. [@ANCHOR: vnwa_open_cal_through_offset]
* A frequency is placed between two calibrated frequencies for interpolation, and one outside the calibrated range is refused. [@ANCHOR: vnwa_open_cal_locate]
* A raw reflection is corrected with the interpolated one-port terms. [@ANCHOR: vnwa_open_cal_s11]
* A raw transmission is divided by the interpolated through measurement. [@ANCHOR: vnwa_open_cal_s21]
* A measured point is corrected in each parameter the calibration covers and left `None` in the other. [@ANCHOR: vnwa_open_cal_correct]
* A calibration is written as a text file (header, frequency count, one line per frequency). [@ANCHOR: vnwa_open_cal_to_text]
* A kit definition is written as a text file that reads back to the same definition. [@ANCHOR: vnwa_open_kit_to_text]

### `Capture::open_with(needle: &str, options: Options) -> Result<Capture, String>` [@ANCHOR: vnwa_open_capture_open_with]

Opens the audio input whose id contains `needle` (case-insensitive, for example `hw:CARD=CODEC,`) at 48 kHz, stereo, 16 bit, with the ALSA period in `options`, and starts the stream into a ring buffer that records the frame total, the time of the last delivery and every overrun. Errors (strings): no input device with that text in its id, or the stream failing to build or play. It only listens; nothing is transmitted. The needle must be unique to one codec; `topology` finds which card is which. Needs a real codec, so the tests are hardware tests, while `synthetic_capture` covers the logic.

### `capture::frames_since(last_delivery: Option<Instant>, period: u64) -> u64` (private) [@ANCHOR: vnwa_open_capture_frames_since]

Estimates how many frames have passed since the last delivery of a period: the elapsed seconds since that delivery times 48,000, capped at one period, and 0 when nothing has been delivered yet. Used by `Capture::mark` so the mark does not trail real time by the delivery granularity. Pure apart from reading the clock, no error path. Tests: the capture unit tests that check the mark against the clock.

* The `vnwa_open` command line: `is-vnwa-codec` answers by exit status, `install-system-config` runs the installer, anything else prints the usage and exits 2. [@ANCHOR: vnwa_open_cli_main]
* The codec cards in sysfs are listed with their USB paths, and only the codec vendor is read. [@ANCHOR: vnwa_open_codec_find_cards]

### `generator::am_captured_500hz_100pct(usb: &Vnwa, freq_hz: f64) -> Result<(), String>` [@ANCHOR: vnwa_open_generator_am_captured]

Starts the captured AM at the vendor's defaults, a 500 Hz sine at 100 percent depth and -20 dB attenuation, on the caller's carrier frequency, by calling `modulate`. It is kept for the tests that compare the decoded sequence with the capture; the stalled status stage of the last request is not an error. Errors are those of `modulate` (carrier out of range, USB failure). Same side effects: the unit then modulates a low-level carrier until a USB reset or power cycle. Tests: the capture comparison in `generator.rs`.

* A managed file lives under the staging root when one is given. [@ANCHOR: vnwa_open_install_path]
* A managed file is missing, current or different from the packaged content. [@ANCHOR: vnwa_open_install_state]
* A command runs under `sudo -n` unless there is a staging root or the user is root. [@ANCHOR: vnwa_open_install_sudo_prefix]
* The installer runs a command only if it is on the allow-list, and logs it. [@ANCHOR: vnwa_open_install_do_run]
* What the installer did is appended to its log. [@ANCHOR: vnwa_open_install_note]
* The installer refuses while a codec is held open by a sweep or soak, unless forced. [@ANCHOR: vnwa_open_install_busy]
* The dry run lists the files and commands, and changes nothing. [@ANCHOR: vnwa_open_install_plan]
* The installer is run in install, check or uninstall mode, as a dry run or applied. [@ANCHOR: vnwa_open_install_run]
* Installing backs up what it replaces, is idempotent, and undoes its steps on a failure midway. [@ANCHOR: vnwa_open_install_install]
* Uninstalling removes what was installed and restores the backups. [@ANCHOR: vnwa_open_install_uninstall]
* The production executor runs a real command and returns its failure text. [@ANCHOR: vnwa_open_install_process_exec]

### `Vna::meter_once(&mut self, center_hz: f64, point_ms: f64) -> Result<Reading, String>` (`meter.rs`, private) [@ANCHOR: vnwa_open_meter_once]

One frequency-meter measurement, called by `Vna::meter` inside the recovery wrapper (which repeats it after a failure). It builds the set-up block with the meter's step counter (31), reads request 0x58, marks both codecs, sends the block, does the 0x0d read, then collects about 28 point times of audio from both codecs starting two stages in, checks each codec's reference tone against `min_reference`, and fits the tone offset on each. Errors (strings): no signal on a reference channel, no or too few frames, or the two codecs disagreeing by more than `0.02 + 5 * (e1 + e2)` Hz, which is an error and never a number. Returns a `Reading` with the TX DDS's real frequency, both offsets and their mean. Blocking: up to 10 s per codec for audio. It makes the unit's generator emit a low-level RF tone (about -17 dBm at the TX port); there is no antenna transmit path. Tests: the simulated-unit tests at 1, 10 and 50 MHz within 1 ppm.

### `power::cold_reset(port: &HubPort) -> Result<(), String>` [@ANCHOR: vnwa_open_power_cold_reset]

The production cold reset: cycle the hub port's power through the installed hook or `sudo -n uhubctl`, wait for the unit and its codecs, and if the unit does not come back try once more with a 12 s off time (the ladder is 3 s off with up to 25 s for the unit, then 12 s off with up to 45 s). After the unit is listed it sleeps 8 s more, because a request sent 3 s after enumeration stalled. Error: the failure text of the cycle. Blocking for tens of seconds. The caller must close the audio streams first (`Capture::close`) and must resend all set-up afterwards. Tests: a hardware test; `cold_reset_via` is tested with a recording hook.

### `ProcessRunner::run(&self, program: &str, args: &[String]) -> Result<(bool, String), String>` [@ANCHOR: vnwa_open_power_process_runner]

The production `Runner`: starts a real process with the arguments (no shell, so nothing is interpreted), waits for it, and returns whether it exited successfully plus its standard output and standard error as one string. Failure to start the program is an error `"<program>: <os error>"`. It blocks until the process exits and has no timeout of its own. Used to run `sudo -n uhubctl ...` for a hub port power cycle; tests substitute a scripted `Runner`, so no test touches real power. Tests: a test runs a real harmless program and checks status and both streams.

### `power::uhubctl(run: &dyn Runner, port: &HubPort, action: &str) -> Result<(), String>` (private) [@ANCHOR: vnwa_open_power_uhubctl]

Switches one hub port with `sudo -n uhubctl -l <hub> -p <port> -a <action>` through the `Runner` (action is `off` or `on`). It first refuses a hub name that fails `valid_hub_name`, returning "...is not a USB hub location such as 3-2 or 3-2.4; nothing was switched" without running anything. A non-zero exit is an error quoting the output. `sudo -n` never prompts, so without a sudo rule the call fails at once rather than hanging. Side effect: real power removal from the port in production, which is why the unit's streams must be closed first. Tests: scripted-runner tests check the exact argument list and both error cases.

### `power::valid_hub_name(name: &str) -> bool` [@ANCHOR: vnwa_open_power_valid_hub_name]

True when `name` looks like a USB hub location as `uhubctl -l` takes it: digits joined by `-` for the bus and ports, then optional dot-separated digit groups, so `3-2` and `3-2.4` pass. Empty parts, letters, spaces and a leading `-` fail; the last matters because `uhubctl` would read a name starting with `-` as an option. Called by `uhubctl` before any command is built, so a bad configured name never reaches `sudo`. Pure, total. Security: this is the input check for the one argument that comes from configuration. Tests: valid and invalid names in the power unit tests.

### `power::wait_until(timeout: Duration, every: Duration, ready: impl FnMut() -> bool) -> bool` [@ANCHOR: vnwa_open_power_wait_until]

Polls `ready` until it returns true (result true) or the timeout has passed (result false). The condition is checked first, so an already-true condition returns immediately with no sleep; between checks the thread sleeps `every`. No error path. It blocks the calling thread for up to `timeout` plus one interval. Used by `power_cycle` to wait for the unit and both codecs (polling every 500 ms). Tests: the power unit tests with a counting condition.

### `RusbTransport::open(vid: u16, pid: u16) -> Result<RusbTransport, String>` [@ANCHOR: vnwa_open_rusb_transport_open]

Opens the VNWA through libusb by vendor and product id (20a0:4118 in production): creates a libusb context and opens the first matching device. It does not claim an interface, reset the device or select a configuration; only endpoint 0 control transfers are made afterwards, with a 1 second timeout. Errors: a libusb context failure, or "no VNWA ... found, or no permission to open it", which usually means a missing udev rule. Needs the real unit, so there is no `cargo test` coverage; the hardware tests and examples (`probe`, `sweep`) exercise it.

### `Scripted::read` (`transport.rs`, `Transport` impl) [@ANCHOR: vnwa_open_scripted_read]

The test transport's control read: it plays the next scripted outcome for a read of the given request, copying at most the buffer length of its bytes into the buffer and returning the count, or returning the scripted error (transient, no device or other). When the script is exhausted it succeeds, so a test only scripts the failures it cares about. It records what was asked in the shared log. No hardware, no timing. This is what lets the retry and recovery logic be tested on the same code path that production uses. Tests: the usb retry tests.

* A session opens the unit and both codecs (found by topology unless named). (Hardware test.) [@ANCHOR: vnwa_open_session_open]
* The sound server is asked (`pw-dump`, else `pactl`) and judged; what could not be inspected is reported as unknown. [@ANCHOR: vnwa_open_sound_server_check]
* Only the cards on the USB port of a codec that has the control chip beside it are the VNWA's. [@ANCHOR: vnwa_open_sound_server_judge]
* The USB port is read from the card's bus path. [@ANCHOR: vnwa_open_sound_server_port_of]
* The exposure is stated plainly: protected, exposed or unknown. [@ANCHOR: vnwa_open_sound_server_protected]
* An exposed codec gives a warning that names the fixes. [@ANCHOR: vnwa_open_sound_server_warning]

### `Vnwa::temperature_counts(&self) -> Result<u16, String>` [@ANCHOR: vnwa_open_temperature_counts]

Reads the internal temperature in raw counts. It first reads the firmware version and refuses with an error naming both versions when the firmware is older than v5.16 (`SENSOR_MIN_VERSION`, the first with the sensor), then does a vendor read of request 0x85 and parses it with `parse_temperature`. Two USB control reads, each with the retry wrapper; a transport error is returned as a string. No state changes on the unit. `Vna::temperature_counts` forwards to it for the sweep session. Tests: the monitor tests over the scripted transport.

* A time-domain transform needs at least four points on a uniform, rising frequency grid. [@ANCHOR: vnwa_open_time_domain_check_grid]
* The delay of the largest peak is found by parabolic interpolation. [@ANCHOR: vnwa_open_time_domain_peak_time]
* A time gate keeps what arrives between two times (shaped by a window across the gate) and returns that part's frequency response on the same grid; the impulse response is taken with a rectangular window so nothing needs undoing. [@ANCHOR: vnwa_open_time_domain_gate]
* One point is traced raw: the audio of both codecs around the set-up block send. (Hardware test.) [@ANCHOR: vnwa_open_trace_point]

### `impl Display for TransferError` (`transport.rs`) [@ANCHOR: vnwa_open_transfer_error_text]

The text of a transfer error as it appears in messages and the transfer log: a `Transient` error reads as its message followed by `(transient)`, so a reader can tell a failure worth sending again from a final one; `NoDevice` and `Other` read as their message alone. Pure formatting, cannot fail. `Vnwa::attempt` and `recent_transfers` rely on the `(transient)` marker to explain why a transfer was repeated. Tests: the error-text test in `transport.rs`.

### `Vnwa::remember(&self, what: &str, outcome: &str)` (`usb.rs`, private) [@ANCHOR: vnwa_open_transfer_remember]

Appends one line, `<epoch ms> <what>: <outcome>`, to the ring of recent transfers, dropping the oldest when 16 are held (`TRANSFER_LOG`). Called by `attempt`, `read_patient` and `reset_device` for every try, success or failure. The time is read from the system clock, 0 if it cannot be read. Takes the mutex of the ring, so concurrent transfers record in the order they finish. No I/O. Tests: the transfer-log tests in `usb.rs`.

* The recovery's cold reset closes the streams, power cycles the hub port and reopens the session. (Hardware test.) [@ANCHOR: vnwa_open_vna_cold_reset]
* A window is chosen by name, several spellings each; an unknown name is refused with the list. [@ANCHOR: vnwa_open_window_from_name]

## Drift (2026-10-29)

The sensor of the unit shows a warm-up of about 100 counts in the first 5 minutes after a power-up, during which a loopback sweep changes by less than 0.04 % and 0.01 degrees on average
(`docs/PROTOCOL.md`). `examples/soak.rs` therefore judges a sweep against two baselines (the first of the run and one taken right after each power cycle) with a tolerance of 0.3 % and 0.3 degrees
plus 0.1 % and 0.1 degrees per 100 counts of temperature change since that baseline, and records an event for every deviation (the constants are to be refined from the second soak's data).

### `Vnwa::read_patient(&self, request, value, index, len, pauses_ms: &[u64]) -> Result<Vec<u8>, String>` [@ANCHOR: vnwa_open_patient_read]

A device-to-host vendor read for the 0x0d request, which the unit stalls while it is busy: after a transient error it waits the given pauses (milliseconds), one per resend, instead of the default 3 resends of 5 ms; the caller's table gives growing pauses, up to 6 resends. Returns the bytes read (truncated to what arrived). A `NoDevice` or `Other` error fails at once; running out of pauses returns an error naming the count and the last message. Every try is logged and each resend counted. It sleeps the calling thread for the pauses. Tests: the growing-pause test over the scripted transport in `usb.rs`.

* The IF tone starts 61 ms and the 18 kHz burst about 1 ms after the end of the send, in a recorded point. [@ANCHOR: vnwa_open_emission_onset]

### `generator::am_fields(level: u16, mod_hz: f64, depth_pct: f64) -> Result<AmFields, String>` [@ANCHOR: vnwa_open_am_fields]

Computes the settings-dependent fields of an AM start: the modulation phase step `round(mod_hz * 65536 / 18,750 Hz)` (the 0x92 field) and the 32-bit modulation amplitude `round(level / 16383 * m / (1 + m) * 2^23)` with `m = depth / 100`, split into a high and a low 16-bit half (the 0x94 field). Arguments: the output amplitude word, the modulating frequency (1 to 3000 Hz) and the depth (1 to 100 percent); anything outside is an error string naming the range. Matches the 0x94 fields of seven vendor captures to within one count. Pure, no I/O. Tests: the captured-values test in `generator.rs`.

### `generator::fm_fields(mod_hz: f64, deviation_hz: f64) -> Result<FmFields, String>` [@ANCHOR: vnwa_open_fm_fields]

Computes the settings-dependent fields of an FM start: the phase step `round(mod_hz * 65536 / 15,625 Hz)` (0x92) and the deviation step word `round(deviation * 2^32 / 360 MHz / 127)` (0x95). The modulation frequency must be 1 to 3000 Hz and the deviation 100 Hz to 300 kHz, otherwise an error string names the range. The values reproduce the captured starts at 500 and 1000 Hz (phase 2097 and 4194) and deviations of 5, 10 and 20 kHz (470, 939 and 1879). Pure, no I/O. Tests: the captured-values test in `generator.rs`.

### `generator::modulation_reads(level: u16, m: Modulation) -> Result<Vec<(u8, u16, u16)>, String>` [@ANCHOR: vnwa_open_modulation_sequence]

Builds the vendor's 23-step start sequence for AM or FM as (request, `wValue`, `wIndex`) tuples in the captured order: switch-off, two carrier-frequency writes (request 0x4c, with `wValue` 0 and then 0x0080, which `modulate` replaces with real frequency payloads), the phase, level and amplitude or step fields from `am_fields` or `fm_fields`, a few constant requests, and last the start request 0x90 (0x0a for AM, 0x0c for FM). Errors come from the field functions (out-of-range modulation frequency, depth or deviation). Pure, builds only. Tests: the sequence reproduces every captured start.

### `Vnwa::reset_device(&self) -> Result<(), String>` [@ANCHOR: vnwa_open_usb_reset]

Performs a USB port reset of the unit's control chip through the transport: no power change and the device number stays. Measured on 2026-10-29, after AM or FM every request stalls until this reset, and every request is answered after it, so it ends the modulation lock-up without a power cycle (the power cycle ladder is the fallback). The result is logged to the transfer log; an error is returned as `usb reset: <error>`. Side effect: the unit re-initialises its USB state and loses nothing the host holds. No transmit gate is involved. Tests: the reset test over the scripted transport in `usb.rs`.

### `generator::end_modulation_with(usb: Vnwa, reopen, tries: u32, pause: Duration) -> Result<(), String>` [@ANCHOR: vnwa_open_end_modulation]

Ends AM or FM without a power cycle: a USB reset of the control chip (which the vendor program does not try), after which it waits `pause` and reopens the unit up to `tries` times, because the reset sometimes makes the unit re-enumerate with a new device number. When the reopened unit answers request 0x91 (`answers`), it runs the normal switch-off (`off`). If it never answers the error says how many seconds were waited, shows the reset result, and says it needs a power cycle (`power::cold_reset_after_modulation`). Takes the unit by value and drops it before the pause. `end_modulation` calls it with 24 tries of 500 ms. Measured on 2026-10-29: after the reset every request answered again. Tests: scripted tests of both outcomes.

### `power::cold_reset_after_modulation(port: &HubPort) -> Result<(), String>` [@ANCHOR: vnwa_open_modulation_recovery]

The cold reset to use when the unit is in, or may be in, a modulation lock-up (during AM or FM the unit's microcontroller stalls every control request, including switch-off): it runs `modulation_ladder` through `cycle_with` (the installed hook or `sudo -n uhubctl`) and then sleeps 8 s for the unit to settle. Error: the cycle failure text, listing each attempt. Blocking for up to minutes in the worst case. The softer first remedy, a USB reset of the control chip, is `generator::end_modulation`; this is the fallback. Tests: hardware test; the ladder and hook are unit-tested.

### `power::set_cycle_hook(hook: CycleHook) -> bool` [@ANCHOR: vnwa_open_cycle_hook]

Lets a program that embeds this crate install its own power-cycle implementation (`bench_mcp` installs its USB hub module's), a boxed closure taking the hub port and the (off time, wait) ladder and returning when the unit is back. It is stored once in a process-wide `OnceLock`: the first call wins and returns true, a later call returns false and changes nothing. The recovery inside a measurement, the generator-off recovery and the power-cycle tool then all run one implementation with its hub protection rules and evidence record. This crate cannot depend on `bench_mcp`, so standalone programs use the built-in `sudo -n uhubctl` path. Tests: the hook test in `power.rs`.

* Spectrum analyzer: the audio power around the LO at each frequency (TX chip silent or a 12 kHz tracking generator), band power by DFT (discrete Fourier transform), settings refused if they would leave holes. [@ANCHOR: vnwa_open_sa_band_rms] [@ANCHOR: vnwa_open_sa_spectrum]
* A point with all four channels dead says nothing leaves the TX port and names the TX connector, pad, tee and leads first. [@ANCHOR: vnwa_open_no_reference_message]

### `generator::modulate(usb: &Vnwa, carrier_hz: f64, attenuation_db: f64, m: Modulation) -> Result<(), String>` [@ANCHOR: vnwa_open_generator_modulate]

Starts AM or FM on the generator by sending `modulation_reads` over USB. It checks the carrier (`check_frequency`) and the attenuation first, so a bad value sends nothing. The carrier writes use `generator_frequency`; the last request's status stage is often stalled and that is not an error. Afterwards the unit modulates in real time and stalls every control request, including switch-off, so the only ways to end it are `generator::end_modulation` (a USB reset) or a power cycle. Output is a low-level modulated RF carrier on the TX port (about -17 dBm at 0 dB attenuation, flat to 10 MHz), never an antenna transmission by this crate. Errors: validation strings or a USB failure. Tests: scripted-transport tests and a scope check recorded in the README.

### `power::cycle_with(hook: Option<&CycleHook>, port, schedule) -> Result<(), String>` (private) [@ANCHOR: vnwa_open_power_cycle_with]

Chooses the power-cycle implementation: with a hook installed it calls the hook with the port and the ladder and returns its result unchanged (so the hook's error text reaches the caller); without one it runs `power_cycle_escalating` with the real `ProcessRunner` and `unit_present` as the readiness test. Taking the hook as an argument keeps tests independent of the process-wide hook. Blocking, with the side effects of whichever implementation runs (removing power from the hub port). Tests: the hook test in `power.rs`.

### `power::modulation_ladder() -> Vec<(Duration, Duration)>` [@ANCHOR: vnwa_open_power_modulation_ladder]

Returns the power-cycle schedule used after AM or FM: five cycles with 3, 12, 12, 30 and 12 s off, each waiting up to 25 s (for the 3 s cycle) or 45 s (`BACK_TIMEOUT`, for the others) for the unit. It is that long because in 11 of 25 recoveries from a modulation lock-up measured on 2026-10-29 the control chip failed to come back after one or more cycles (1 cycle 14 times, 2 cycles 6 times, 3 cycles 3 times, 4 or more twice). The extra cycles cost time only when the unit does not come back. Pure, no I/O. Tests: the ladder shape test in `power.rs`.

* The functions that modulate, choose the power-cycle implementation, climb the modulation ladder and measure one spectrum point are anchored. [@ANCHOR: vnwa_open_spectrum_sa_once]

## Soak scope connection (`soak_scope`)

`src/soak_scope.rs` is the soak harness's scope connection; `examples/soak.rs` uses it, and `tools/soak_gonogo.sh` reports its failure. It follows the same rule as bench_mcp's `Link::tcp_checked`: a Siglent scope that still holds a dead single-client SCPI (the text command language of lab instruments) session (after a host power loss, as on 2026-10-29) resets queries, so the harness waits and retries, then says NO-GO with the evidence and the action.

### `vnwa_open_soak_scope_open_checked` [@ANCHOR: vnwa_open_soak_scope_open_checked]

**Purpose.** Connect to the scope and prove it answers `*IDN?` before the generator step uses it.

```rust
pub fn open_checked(addr: &str, waits: &[Duration], diagnose: &dyn Fn(&str) -> String) -> Result<ScopeStream, String>
pub type ScopeStream = (BufReader<TcpStream>, TcpStream); // reader and writer on one socket, 5 s read timeout
```

| Argument | Type | Meaning |
|---|---|---|
| `addr` | `&str` | `host:port`, for example `192.168.10.51:5025`. |
| `waits` | `&[Duration]` | Sleep after each failed attempt; one more attempt than waits. The soak uses 2, 5, 10 and 20 s. |
| `diagnose` | `&dyn Fn(&str) -> String` | Called once after the last failure for the state text; `soak_scope::state` in production. |

**Returns.** `Ok` with the connected stream after the first non-empty `*IDN?` reply. `Err` starting `NO-GO: scope <addr> did not answer *IDN? in N attempts over S s (...)`, with each attempt's error, the state, and the recommended action (power-cycle the scope).

**Special concerns.** Blocking, up to the sum of `waits` plus 5 s connect and 5 s read per attempt. Each failed attempt drops its socket. Sends only `*IDN?`. In the soak the error becomes a `generator_step_failed` event, which `soak_gonogo.sh` prints in full (up to 600 characters) as the NO-GO reason.

**See also.** `vnwa_open_soak_scope_state`. Tests: the unit tests in `src/soak_scope.rs` (fake scope on loopback that resets the first connections) and `tests/soak_gonogo.rs` (the verdict script on a recorded log).

### `vnwa_open_soak_scope_state` [@ANCHOR: vnwa_open_soak_scope_state]

**Purpose.** Evidence for the failure report.

```rust
pub fn state(addr: &str) -> String
```

| Argument | Type | Meaning |
|---|---|---|
| `addr` | `&str` | `host:port`; the port defaults to 5025 if missing. |

**Returns.** `ping answers|ping does not answer|ping unavailable, port <p> accepts|port <p>: <error>, port 80 ...`. Never an error. Blocking (about 1 s ping plus 2 s per port); briefly opens TCP connections to the SCPI port and port 80, so call it only after the real attempts failed.

**See also.** `vnwa_open_soak_scope_open_checked`. Test: `the_state_names_an_accepting_port_and_a_refused_one`.

### `vnwa_open_soak_scope_try_once` [@ANCHOR: vnwa_open_soak_scope_try_once]

**Purpose.** One connection attempt for `open_checked`; private.

```rust
fn try_once(addr: &str) -> Result<ScopeStream, String>
```

| Argument | Type | Meaning |
|---|---|---|
| `addr` | `&str` | `host:port`, resolved to its first address. |

**Returns.** `Ok(stream)` after a non-empty `*IDN?` reply. `Err` text starting `connect:`, `send:` or `read:` (with the OS error, for example `Connection reset by peer`), or `empty reply to *IDN? (the scope closed the connection)`.

**Special concerns.** Blocking: 5 s connect limit and 5 s read limit. Sends only `*IDN?`. Test: `a_scope_that_resets_the_first_connections_is_waited_out_and_then_answers`.

### `vnwa_open_soak_scope_connects` [@ANCHOR: vnwa_open_soak_scope_connects]

**Purpose.** A single TCP connect probe for `state`; private.

```rust
fn connects(host: &str, port: &str) -> Result<(), String>
```

| Argument | Type | Meaning |
|---|---|---|
| `host` | `&str` | Name or address. |
| `port` | `&str` | Port number as text. |

**Returns.** `Ok(())` if a connection was accepted within 2 s (it is dropped at once); `Err` with the resolver or OS error text otherwise. Blocking, 2 s at most. Test: `the_state_names_an_accepting_port_and_a_refused_one`.

## Soak USB topology check (`soak_topology`)

`src/soak_topology.rs` decides whether the USB tree changed in a way that matters to the unit. Before 2026-11-06 the soak compared the whole `lsusb -t` and recorded a false `usb_topology_changed` whenever an unrelated device (a USB-C dock, 2026-10-29) was unplugged. `examples/soak.rs` calls `compare` after the start, after each scheduled power cycle and after a recovery cycle. The test fixtures `tests/soak_fixtures/lsusb_start.txt` and `lsusb_cycle_2.txt` are the real pair from the second soak.

### `vnwa_open_soak_topology_change` [@ANCHOR: vnwa_open_soak_topology_change]

**Purpose.** The result of comparing two `lsusb -t` texts.

```rust
pub enum Change { None, OutsideOnly, Unit { first: String, now: String } }
```

| Variant | Meaning |
|---|---|
| `None` | The whole normalised trees are equal. |
| `OutsideOnly` | Only devices outside the unit's part differ. The soak logs an `INFO` line and records no event. |
| `Unit { first, now }` | The unit's part differs; `first` and `now` are its normalised subtrees (or `(bus not present)`). The soak records a `usb_topology_changed` event with both as the detail. |

Plain value, `Clone`, created by `compare`, owned by the caller; no serialized form and no shared state.

### `vnwa_open_soak_topology_normalise` [@ANCHOR: vnwa_open_soak_topology_normalise]

**Purpose.** Makes two `lsusb -t` texts comparable by removing the device numbers (`Dev 17,`), which change at every enumeration.

```rust
pub fn normalise(lsusb_t: &str) -> String
```

| Argument | Type | Meaning |
|---|---|---|
| `lsusb_t` | `&str` | Output of `lsusb -t`, borrowed. |

**Returns.** The same lines with each `Dev N,` removed (the space after the comma stays, so lines have two spaces there), joined with `\n`. Lines without a device number are unchanged. Pure; never fails.

### `vnwa_open_soak_topology_unit_subtree` [@ANCHOR: vnwa_open_soak_topology_unit_subtree]

**Purpose.** Picks out of `lsusb -t` the lines that belong to the unit: the root hub line of its bus (the controller), every line of each hub on the way down to the switched port, and the device at that port with everything below it (the unit's hub, both codecs and their interfaces, with ids, classes, drivers and speeds).

```rust
pub fn unit_subtree(lsusb_t: &str, hub: &HubPort) -> Option<String>
```

| Argument | Type | Meaning |
|---|---|---|
| `lsusb_t` | `&str` | Output of `lsusb -t`, borrowed. |
| `hub` | `&HubPort` | The hub in uhubctl's naming (`3-2`, or `3-2.1` for a hub behind a hub) and the switched port (2). The path is the hub's ports plus this port. |

**Returns.** `Some(text)` (normalised, lines joined with `\n`) if the hub's bus is in the output, even when the unit's branch is gone (the text then lacks the branch, so it differs). `None` if the bus is absent or the hub name is not of the form `<bus>-<port>[.<port>...]`. Siblings of the unit's branch, other ports of its hubs, and other buses are left out. Pure; reads nothing from the system.

### `vnwa_open_soak_topology_compare` [@ANCHOR: vnwa_open_soak_topology_compare]

**Purpose.** The soak's single decision: is this a change to report?

```rust
pub fn compare(first: &str, now: &str, hub: &HubPort) -> Change
```

| Argument | Type | Meaning |
|---|---|---|
| `first` | `&str` | The first `lsusb -t` of the run, raw or normalised. |
| `now` | `&str` | The current `lsusb -t`. |
| `hub` | `&HubPort` | As for `unit_subtree`. |

**Returns.** `Change::Unit` if the unit subtrees differ (a codec missing, a different port, a different speed or driver, the bus gone); otherwise `OutsideOnly` if the rest differs; otherwise `None`. Pure.

**See also.** Tests in `src/soak_topology.rs`: the dock removal in the recorded pair is `OutsideOnly`; a missing codec, a changed port, a changed speed and a missing bus are `Unit`. Real-hardware follow-up: none needed for the logic; the next real soak will show whether an `INFO` line appears when a device is unplugged.

## USB, generator, set-up and sweep-check functions (ADR 0113 entries)

### `vnwa_open_usb_send` [@ANCHOR: vnwa_open_usb_send]

**Purpose.** Send one host-to-device vendor control transfer (a `Request` built by `protocol`) to the unit. Every block of the sweep set-up, the generator frequency and the modulation sequences go through it, so every send gets the same bounded resend of transient errors.

```rust
impl Vnwa { pub fn send(&self, r: &Request) -> Result<(), String> }
```

| Argument | Type | Meaning |
|---|---|---|
| `r` | `&Request` | Borrowed; `request_type`, `request`, `value`, `index` and the `data` stage, all used as given. |

**Returns.** `Ok(())` when the transport accepted all of `r.data`. `Err` text `request 0x<nn>: ...` when a transient error outlasted `TRANSFER_RETRIES` (`still failing after 3 resends (...)`), when the unit has left the bus or the transfer failed for another reason (the transport's text), or when fewer bytes than `r.data.len()` were written (`N of M bytes sent`).

**Special concerns.** Blocking; waits `RETRY_PAUSE` (5 ms) between resends, so at worst about 15 ms of sleeping plus the transport timeout (1 s) per try. Every try, and each resend, is counted in `resends()` and remembered by `recent_transfers()` for the evidence record. Not idempotent in general (a repeated write reaches the unit twice), but the requests this crate sends are state settings, safe to repeat. No transmitter is keyed; the unit is a measuring instrument.

**See also.** `vnwa_open_usb_read`, `vnwa_open_transfer_retry`, `vnwa_open_transfer_log`. Test: `a_read_returns_only_the_bytes_the_unit_sent_and_a_send_is_a_write_of_the_request` (over the `Scripted` transport).

### `vnwa_open_usb_read` [@ANCHOR: vnwa_open_usb_read]

**Purpose.** Send one device-to-host vendor control transfer and return what the unit answered. Used for the generator read-back (0x3a), the 0x0e/0x04 handshakes and the reads the vendor program makes to switch modes.

```rust
impl Vnwa { pub fn read(&self, request: u8, value: u16, index: u16, len: usize) -> Result<Vec<u8>, String> }
```

| Argument | Type | Meaning |
|---|---|---|
| `request` | `u8` | The vendor request number (`bRequest`). |
| `value` | `u16` | `wValue`; for 0x58 it is the RF level, for the generator reads a fixed vendor constant. |
| `index` | `u16` | `wIndex`; 0 for every request this crate uses. |
| `len` | `usize` | The size of the receive buffer, which is the most the unit may return. |

**Returns.** `Ok(bytes)` truncated to the number of bytes the unit actually sent (it may be fewer than `len`, even none). `Err` text `read request 0x<nn>: ...`, with the same causes as `vnwa_open_usb_send`.

**Special concerns.** Blocking, with the same resend rule (3 resends, 5 ms apart) and counters as `send`. The 0x0d read, which the unit stalls while busy, uses `read_patient` instead. Allocates a zeroed `len`-byte buffer per call.

**See also.** `vnwa_open_usb_send`, `vnwa_open_patient_read`. Test: as for `send`.

### `vnwa_open_generator_run` [@ANCHOR: vnwa_open_generator_run]

**Purpose.** Play a fixed list of vendor reads in order (the `ENTER` list that switches the generator mode on); private to `generator`.

```rust
fn run(usb: &Vnwa, reads: &[Read]) -> Result<(), String> // Read { request: u8, value: u16, len: usize }
```

| Argument | Type | Meaning |
|---|---|---|
| `usb` | `&Vnwa` | The open unit. |
| `reads` | `&[Read]` | The reads, each sent as `usb.read(request, value, 0, len)`, with the answers discarded. |

**Returns.** `Ok(())` after the last read; the first `Err` from a read stops the list (nothing after it is sent) and is returned unchanged.

**Special concerns.** Blocking; each read has the bounded resend of `vnwa_open_usb_read`. Order matters: the unit takes the vendor sequence only in the order the vendor program sends it. Test: `switching_the_generator_on_sends_the_vendor_sequence_and_a_bad_level_sends_nothing` checks all four reads with their wValues.

### `vnwa_open_generator_set_frequency` [@ANCHOR: vnwa_open_generator_set_frequency]

**Purpose.** Set the generator frequency of a unit already in generator mode; private, used by `on` (first frequency) and `retune`.

```rust
fn set_frequency(usb: &Vnwa, freq_hz: f64, first: bool) -> Result<(), String>
```

| Argument | Type | Meaning |
|---|---|---|
| `usb` | `&Vnwa` | The open unit, already in generator mode. |
| `freq_hz` | `f64` | The output frequency in hertz; converted to the unit's tuning word by `protocol::generator_frequency` (see `vnwa_open_generator_frequency` for the range). |
| `first` | `bool` | `true` for the first frequency after switching on: the write carries `wValue` 0x00c8, as the vendor program does; `false` for a retune: `wValue` 0. |

**Returns.** `Ok(())` after three transfers: read 0x0e (64 bytes), write 0x4c (the frequency, 8 data bytes), read 0x04 with `wValue` 0x090a (8 bytes). The first failing transfer returns its error and stops the sequence.

**Special concerns.** Blocking. Changes the frequency of the RF output on the unit's port at once; it does not change the level. Safe to repeat. Test: the generator sequence test above, including the retune without 0x00c8.

### `vnwa_open_generator_on` [@ANCHOR: vnwa_open_generator_on]

**Purpose.** Switch the unit's signal generator on at a frequency and a level and leave it running. The public entry of the generator mode, used by the generator example and the bench tools.

```rust
pub fn on(usb: &Vnwa, freq_hz: f64, attenuation_db: f64) -> Result<(), String>
```

| Argument | Type | Meaning |
|---|---|---|
| `usb` | `&Vnwa` | The open unit, borrowed. |
| `freq_hz` | `f64` | The output frequency in hertz (see `set_frequency`). |
| `attenuation_db` | `f64` | 0 (full scale) down to -50 dB, inclusive. |

**Returns.** `Ok(())` once the level is set. `Err` `generator attenuation <x> dB is outside -50 to 0 dB` before anything is sent; otherwise the first transfer error, with the later transfers not sent.

**Special concerns.** Blocking, 8 control transfers in the order: four mode reads (0x62, 0x61, 0x98, 0x9a), the frequency (0x0e, 0x4c, 0x04), the level (0x58). The output stays on until `generator::off` or another mode is entered. The level is checked first so a bad argument cannot leave the unit half set up. It emits a measurement signal at the unit's own low output level, not through a transmitter or a relay.

**See also.** `vnwa_open_generator_level`, `vnwa_open_generator_run`, `vnwa_open_generator_set_frequency`. Test: `switching_the_generator_on_sends_the_vendor_sequence_and_a_bad_level_sends_nothing`.

### `vnwa_open_generator_check_frequency` [@ANCHOR: vnwa_open_generator_check_frequency]

**Purpose.** Refuse a generator frequency the unit's DDS cannot be programmed with, before anything is sent. `protocol::tuning_word` converts with a saturating cast, so without this check a NaN or negative frequency became 0 Hz and 360 MHz or more became the largest word, a tone at a frequency other than the one asked for and no error.

```rust
pub fn check_frequency(freq_hz: f64) -> Result<(), String>
```

| Argument | Type | Meaning |
|---|---|---|
| `freq_hz` | `f64` | The requested output frequency in hertz. |

**Returns.** `Ok(())` when `freq_hz` is finite, above 0 and its tuning word `round(f * 2^32 / 360 MHz)` is at most `u32::MAX`. Otherwise `Err` naming the frequency and the 360 MHz DDS clock.

**Special concerns.** Pure and non-blocking, no side effects. Called first by `on`, `set_frequency` (so `retune`) and `modulate`, so a refused frequency sends no transfer. Between 180 MHz (the DDS Nyquist frequency) and 360 MHz the word is valid and the output is the spectral image; that range has not been measured and is not refused. It limits a signal at the unit's own low output level, not a transmitter. Test: `a_frequency_whose_tuning_word_does_not_fit_is_refused_before_anything_is_sent`.

### `vnwa_open_power_cold_reset_via` [@ANCHOR: vnwa_open_power_cold_reset_via]

**Purpose.** The body of `cold_reset`: cycle the unit's hub port through the program's installed power-cycle implementation (`set_cycle_hook`, the bench's hub module with its protection rules) when there is one, else through `sudo -n uhubctl`, using the two-step ladder (3 s off with 25 s for the unit, then 12 s off with 45 s), then pause so the unit and its codecs answer reliably.

```rust
fn cold_reset_via(hook: Option<&CycleHook>, port: &HubPort, settle: Duration) -> Result<(), String>
```

| Argument | Type | Meaning |
|---|---|---|
| `hook` | `Option<&CycleHook>` | The installed implementation, or `None` for the built-in one. |
| `port` | `&HubPort` | The hub and port to cycle. |
| `settle` | `Duration` | The pause after the unit is back (8 s in production, zero in tests). |

**Returns.** `Ok(())` once the unit is back and `settle` has passed; otherwise the error of the hook or the failures of every rung of the ladder.

**Special concerns.** Blocking for up to about a minute. Removes and restores power on the hub port, so every setting the unit held is lost. Not idempotent in effect, but safe to repeat. Test: `the_production_cold_reset_goes_through_an_installed_cycle_implementation`.

### `vnwa_open_audio_analysis_span` [@ANCHOR: vnwa_open_audio_analysis_span]

**Purpose.** Choose which part of one point's audio to analyse: a whole number of IF periods, so the DFT of the tone has no leakage. Called by `measure_point`.

```rust
pub fn analysis_span(len: usize, fs: f64, if_hz: f64, presamples: usize, postsamples: usize) -> Option<(usize, usize)>
```

| Argument | Type | Meaning |
|---|---|---|
| `len` | `usize` | Samples in one channel of the point. |
| `fs` | `f64` | Sample rate in hertz (48000). |
| `if_hz` | `f64` | The measurement IF in hertz; `fs / if_hz` is the samples per period (not necessarily a whole number). |
| `presamples` | `usize` | Samples skipped at the start (the settling). |
| `postsamples` | `usize` | Samples reserved at the end. |

**Returns.** `Some((start, count))`: `start` is `presamples`, `count` is `round(periods * fs / if_hz)` with `periods = floor((len - presamples - postsamples) / (fs / if_hz))`. `None` when not even one period fits, or when `presamples + postsamples` exceeds `len`; the vendor program refuses to sweep in that case too.

**Special concerns.** Pure, no allocation, no blocking, any thread. No panic: the subtraction is checked. `if_hz` of 0 gives an infinite period and so `None`.

**See also.** `vnwa_open_measure_point`. Test: `the_analysis_span_is_whole_if_periods_between_the_presamples_and_postsamples`.

### `vnwa_open_setup_timing_pair` [@ANCHOR: vnwa_open_setup_timing_pair]

**Purpose.** The two timing counts (bytes 2 and 5 of request 0x40) the unit wants for a time per point; private to `setup`, called by `point_time_fields`.

```rust
fn timing_pair(b0: u8) -> (u8, u8)
```

| Argument | Type | Meaning |
|---|---|---|
| `b0` | `u8` | The first byte: the time per point in the vendor program's 0.667 ms units. |

**Returns.** `(byte2, byte5)`: `ceil(16 / b0)` for both, except `(3, 4)` at `b0` = 5, and `(1, 1)` from `b0` = 16 up (and for 0). Fitted to the 84 captured settings in `TIMING_OBSERVED`; why the program has two counts is not known.

**Special concerns.** Pure; no panic. The rule is empirical, so a setting outside the captured ones is a fit, not a measurement. Test: `the_timing_pair_follows_the_ceiling_rule_with_the_one_exception_at_five`.

### `vnwa_open_sweep_check` [@ANCHOR: vnwa_open_sweep_check]

**Purpose.** Refuse a `Config` the unit or the analysis cannot use, before any hardware is opened. `Vna::open` calls it first.

```rust
pub fn check(c: &Config) -> Result<(), String>
```

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&Config` | Borrowed; read-only. |

**Returns.** `Ok(())`, or `Err` text for the first rule broken, in this order: `if_hz` not finite, not positive, not dividing 48 kHz into a whole number of samples of at least 4 (`does not divide 48 kHz`); `point_ms` outside 0.2 to 100 ms (from `point_time_fields`); `rf_level` above 0x3fff (`14-bit`); the analysis window (`settle` + `window`) ending later than 5 point times + 15 ms (`the analysis window ... must end by`); a window shorter than one IF period.

**Special concerns.** Pure and instant; any thread; no panic (a NaN or zero IF is an error). Safe to call repeatedly. Test: `a_configuration_the_unit_or_the_analysis_cannot_use_is_refused_with_the_reason`.

### `vnwa_open_input_names` [@ANCHOR: vnwa_open_input_names]

**Purpose.** List the ids of the audio input devices (for example `alsa:hw:CARD=CODEC,DEV=0`) so a caller can choose a codec needle for `Capture::open`.

```rust
pub fn input_names() -> Result<Vec<String>, String>
```

**Returns.** The ids in the host's order, or `Err("audio: ...")` when the host cannot list devices. It asks the real sound host (`CpalHost`); `input_names_on(&dyn InputHost)` is the same function over any host, which is how the tests run it with a fixed list and never open a sound device.

**Special concerns.** Reads only; opens no stream. Test: `the_input_ids_are_the_hosts_in_its_order_and_a_host_error_is_passed_on`.

### `vnwa_open_vna_measure` [@ANCHOR: vnwa_open_vna_measure]

**Purpose.** Measure one point (S11 and S21 at one frequency), measuring again when an attempt fails, and after one cold reset through the hub when the retries are used up.

```rust
pub fn measure(&mut self, freq_hz: f64, previous_hz: f64) -> Result<Point, String>
```

**Returns.** The `Point`, or an `Err` that lists every attempt with its evidence; a failed attempt's numbers are never returned. Every attempt is logged in `attempt_timings`.

**Special concerns.** Sends the set-up block to the unit and reads two codecs; one caller at a time. The tests build the handle with `Vna::with_session` over a transport that accepts every transfer and two codecs fed by synthetic tones (`capture::fake_capture`), so no hardware is touched. Test: `a_point_is_the_measured_tone_over_the_reference_and_a_dead_reference_is_an_error`.

### `vnwa_open_vna_sweep` [@ANCHOR: vnwa_open_vna_sweep]

**Purpose.** Measure a linear sweep of `points` points from `start_hz` to `stop_hz`.

```rust
pub fn sweep(&mut self, start_hz: f64, stop_hz: f64, points: usize) -> Result<Vec<Point>, String>
```

**Returns.** One `Point` per step (a single point is measured at `start_hz`); `Err` for zero points or for the first point that fails after its recovery. `discard_points` throw-away measurements at the start frequency come first and are not returned.

**Special concerns.** Same hardware use as `measure`, sequential. Test: `a_sweep_steps_linearly_discards_first_and_needs_a_point`.

### `vnwa_open_synthetic_capture` [@ANCHOR: vnwa_open_synthetic_capture]

**Purpose.** A capture that runs without a codec: a thread feeds the ring at the real 48 kHz rate from a generator of frames, so the code above the ring (`mark`, `frames_after`, `Vna::measure`) can be tested or demonstrated with no hardware. `Vna::with_parts` builds a handle from a `usb::Vnwa::with_transport` unit and two of these.

```rust
pub fn synthetic_capture(gen: impl Fn(u64) -> [i16; 2] + Send + 'static) -> (Capture, SyntheticFeed)
```

| Argument | Type | Meaning |
|---|---|---|
| `gen` | closure | Moved into the feeder thread; called once per frame with the running frame index and returns the `[left, right]` samples. |

**Returns.** The open `Capture` and a guard; dropping the guard stops and joins the feeder thread (the capture then stops receiving frames).

**Special concerns.** Never opens an audio device. The feeder wakes every 2 ms and delivers the frames due by the clock. Test: `a_synthetic_capture_delivers_the_generated_frames_after_a_mark_until_it_is_closed`.

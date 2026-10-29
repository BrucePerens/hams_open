# vnwa_open

An open Rust driver for the DG8SAQ **VNWA 3E** vector network analyzer (USB 20a0:4118, plus two PCM2900C
audio codecs), written from observed USB and audio traffic of the owner's own unit. Status (2026-10-26): it measures.
`usb` sends the confirmed requests (control transfers only, no reset, no interface claim), `capture` records the two
codecs (48 kHz, 16 bit, stereo: all they offer) and `sweep` turns a frequency list into uncalibrated S11 and S21 on the
loopback. No calibration yet. `bench_mcp` uses it (`bench_vna_sweep`). Examples: `probe`, `sweep`.

Licence: AGPL-3.0-or-later. Not published to crates.io (`publish = false`). No vendor code and no vendor firmware
is used, loaded or flashed.

## What is decoded

See `docs/PROTOCOL.md` for the evidence. In short, all control is vendor control transfers on endpoint 0 (the
measured signal arrives as audio from the two codecs):

| Item | Request | Status |
|---|---|---|
| Generator frequency | 0x4c | confirmed at 1, 2, 10, 20 MHz |
| Generator on, off, level (`generator.rs`) | 0x62, 0x61, 0x98, 0x9a, 0x0e, 0x04, 0x58, 0x91, 0x66 | sequences from a capture of the vendor program; the output is verified on a scope (tone at the commanded frequency, about -17 dBm, flat to 10 MHz); the level steps are exact to 0.1 dB from 0 to -15 dB (take a negative attenuation); below -20 dB is not yet verified |
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
(`Reference::Left` for codec 1, `Reference::Right` for codec 2). Not covered: a device under test with a frequency-dependent response, the open/short/load/through calibration, the vendor's low-frequency digital
filtering (below the codec's cut-off) and the windowing used for time-domain transforms, none of which is implemented
(details in `docs/PROTOCOL.md`).

The two DDS chips are an AD9859 and an AD9951 (identical register maps, no sweep engine), clocked from a 12 MHz TCXO
times a premultiplier of 3, then times nRF = 10 (RF, 360 MHz) and nLO = 9 (LO, 324 MHz).

## Modes this hardware can serve (per the vendor's Help file)

All of these are built from the same operations (set the two DDS frequencies and levels, set the IF, read the audio),
so an open driver can offer them once those are decoded:

1. **Vector network analyzer**: S11, S21 (both at once on a 3E), with the open/short/load/through calibration.
2. **Signal generator**: CW with variable level; on a VNWA3/3E also AM or FM with a sine up to about 3 kHz. Output is
   about -17 dBm into 50 ohms at 1 MHz, falling with frequency, and it cannot run during a sweep. The AM/FM commands
   are not captured yet.
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
inside the data sheets' 400 MSPS limit). The identity table the vendor program reads at start-up is not used.

## Tests

`cargo test` checks every encoding against bytes seen on the wire (generator word, sweep word, per-chip words,
offset, IF word).

## Checks this crate makes (anchors)

Each is a test against bytes or samples recorded from the vendor program.

* Frequency to the generator's tuning word. [@ANCHOR: vnwa_open_tuning_word]
* The generator amplitude for an attenuation of 0 to -50 dB. [@ANCHOR: vnwa_open_generator_level]
* The sweep word (start, step and count) of a captured sweep. [@ANCHOR: vnwa_open_sweep_word]
* The per-chip tuning word of the RF chip equals the generator read-back. [@ANCHOR: vnwa_open_chip_tuning_word]
* The IF word of the set-up request. [@ANCHOR: vnwa_open_if_word]
* The fields of request 0x41 that follow the IF. [@ANCHOR: vnwa_open_if_scaled_fields]
* The point-time byte that follows the time per point. [@ANCHOR: vnwa_open_point_time_byte]
* The offset tails of captured sweeps. [@ANCHOR: vnwa_open_sweep_offset]
* The generator payload: four zero bytes, then the little-endian word. [@ANCHOR: vnwa_open_generator_frequency]
* The RF level is fourteen bits. [@ANCHOR: vnwa_open_rf_level]
* The time-per-point fields reproduce all 84 observed settings. [@ANCHOR: vnwa_open_point_time_fields]
* The set-up block reproduces every captured block byte for byte. [@ANCHOR: vnwa_open_setup_block]
* Amplitude and phase of a tone at the IF are exact on a synthetic tone. [@ANCHOR: vnwa_open_tone_at_if]
* The ratio of two tones is their amplitude ratio and phase difference, in the vendor program's convention. [@ANCHOR: vnwa_open_vendor_convention]
* A measurement of a recorded sweep agrees with the vendor program's own S21, and the reference channel can be swapped. [@ANCHOR: vnwa_open_measure_point]
* A transient transfer error is sent again a bounded number of times, a vanished unit is reported, others are not repeated. [@ANCHOR: vnwa_open_transfer_retry]
* libusb errors are classified as transient, no device or other. [@ANCHOR: vnwa_open_transfer_classes]
* The S11 and S21 codecs are told apart by their USB port, whatever order the kernel named the cards in. [@ANCHOR: vnwa_open_codec_topology]
* The power cycle is off, wait, on through `sudo -n uhubctl`, then waits for the unit; failures are clear errors. [@ANCHOR: vnwa_open_power_cycle]
* A failing point is measured again, then after one cold reset, and only a success is returned. [@ANCHOR: vnwa_open_recovery]
* A point is accepted only if the reference tone at the IF is steady across the analysis window (IF error 2 Hz or less, level change 3 % or less); a slipped DDS PLL, a retune inside the window or a stage boundary fails it. [@ANCHOR: vnwa_open_stability]

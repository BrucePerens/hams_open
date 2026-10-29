---
title: bench_hamlib_rx_check
summary: Receiver test with the rig's audio: S-meter against audio level, noise floor, squelch, strongest tone and its offset, time station in the audio.
see_also: [instruments/hamlib/overview, instruments/hamlib/limits, instruments/hamlib/measurement-recipes]
tools: [bench_hamlib_rx_check]
instrument: hamlib
tier: read_config
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_rx_check

Tunes to `frequency_hz` (mode AM below 30 MHz and FM above unless `mode` is given), reads the S-meter five times and the squelch, then records `seconds` of the rig's audio from the sound input named in the rig's `audio` setting (see [setup](instruments/hamlib/setup)) and analyses it: RMS level in dBFS, the noise floor (the median spectrum level from 300 Hz to 3 kHz), the strongest steady tone, and the tick and tone detectors for WWV and WWVH. With `expect_tone_hz` the offset of the strongest tone from it is reported: for a carrier of known frequency heard in a single-sideband mode this is the receiver's tuning error (add the sound card clock's own small error). The rig's frequency, mode and AF level are put back. It never transmits. Without an `audio` setting it reports the S-meter and squelch and says there is no audio.

## Arguments

<!-- arguments -->

## Returns

`signal` (mean and spread in dB relative to S9, S-units, the readings), `squelch_open`, and `audio` with `rms_dbfs`, `noise_floor_dbfs`, `dominant_tone`, `tone_offset_hz`, `findings` (the detectors) and `signal_vs_audio` in words. `rig_settings_not_restored` lists anything that could not be put back.

## Failure modes

* `outside the receive range`: the limit and its source are named.
* `the rig's audio cannot be recorded`: check the sound input with the [audio tools](instruments/sound/overview).
* `PTT on`: nothing is changed while the rig reports transmit.

## Example

`bench_hamlib_rx_check {"frequency_hz": 10000000, "seconds": 8}` listens for WWV on 10 MHz. Silence means the station or the propagation is not there, not that the receiver is faulty.

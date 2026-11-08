---
title: bench_elad_tune
summary: ELAD receiver, receive only: applies a frequency, filter and attenuator state and streams nothing.
see_also: [instruments/elad/operation, instruments/elad/limits]
tools: [bench_elad_tune]
instrument: elad
tier: read_config
verified: fixtures only
---

# bench_elad_tune

Opens the receiver, tunes it to `freq_hz` and sets the FDM-S2 `filters` and `attenuator`, then closes it. Starts no stream, so it works on an FDM-S1/S2 with no FPGA image. The receiver keeps no tuning between calls; this checks that the receiver accepts the state.

## Arguments

<!-- arguments -->

## Returns

`tuned_hz`, `tuning_word`, `rate_hz`, `streaming` (always false), `front_end_switches`, `calibration_offset_db`.

## Failure modes

As [bench_elad_capture](tools/bench_elad_capture), and a refusal of the arguments before the receiver is opened. While the relay or another bench call has the receiver, the error names the holder: the receiver is single-owner (a lock file per receiver, see [limits](instruments/elad/limits)).

Example (fake unit): `freq_hz` 7100000 returns `streaming` false and the tuning word for that frequency. Not yet run on a real unit.

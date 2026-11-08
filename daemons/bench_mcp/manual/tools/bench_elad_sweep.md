---
title: bench_elad_sweep
summary: ELAD receiver, receive only: a band swept step by step to a CSV with the noise floor and strongest signals.
see_also: [instruments/elad/operation, instruments/elad/limits]
tools: [bench_elad_sweep]
instrument: elad
tier: read_config
verified: fixtures only
---

# bench_elad_sweep

Retunes from `start_hz` to `stop_hz` in steps (default 40 % of the sample rate, at most 80 %), keeps the bins within half a step of each centre and writes `freq_hz,power_dbfs` to a new CSV in capture_dir. Bounded: at most 400 steps, and an estimated running time of at most 6 times `max_capture_s` in the config; a larger request is refused before anything is opened. Bins within two bins of each centre (the DC spike) are written but not reported as signals, so a signal exactly at a step centre is missed: sweep again with `start_hz` shifted by half a step.

## Arguments

<!-- arguments -->

## Returns

`file`, `points`, `steps`, `step_hz`, `noise_floor_dbfs` (the median point), `signals` (up to five local maxima at least 10 dB over the floor, each `freq_hz`, `power_dbfs`, `snr_db`), `elapsed_s`.

## Failure modes

As [bench_elad_capture](tools/bench_elad_capture), and a refusal of the arguments before the receiver is opened. While the relay or another bench call has the receiver, the error names the holder: the receiver is single-owner (a lock file per receiver, see [limits](instruments/elad/limits)).

Example (fake unit, two tones across 7.0 to 7.3 MHz): both found within one bin (187.5 Hz).

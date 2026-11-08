---
title: bench_elad_spectrum
summary: ELAD receiver, receive only: a spectrum around a frequency as a CSV with noise floor and peaks.
see_also: [instruments/elad/operation, instruments/elad/limits]
tools: [bench_elad_spectrum]
instrument: elad
tier: read_config
verified: fixtures only
---

# bench_elad_spectrum

Streams about 16 FFT blocks of I/Q, computes a Welch average with a Hann window and writes `offset_hz,freq_hz,power_dbfs` for every bin to the new CSV `name` in `capture_dir`. The summary is the same as [bench_elad_capture](tools/bench_elad_capture) plus the noise floor and the five strongest peaks. The span is the sample rate.

## Arguments

<!-- arguments -->

## Returns

`file`, `bins`, `bin_hz`, `noise_floor_dbfs`, `peaks` (each `offset_hz`, `freq_hz`, `power_dbfs`), and the summary fields of `bench_elad_capture`.

## Failure modes

As [bench_elad_capture](tools/bench_elad_capture); `fft_len ... must be a power of two`.

Example (fake unit, tone at +10 kHz, amplitude 0.1): the first peak is at offset 10000 Hz and about -20 dBFS.

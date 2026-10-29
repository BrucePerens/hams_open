---
title: bench_sdr_survey
summary: Power survey over a frequency span by stepping the tuning and stitching spectra; CSV, floor and strongest peaks.
see_also: [tools/bench_sdr_capture, concepts/units-and-conventions]
tools: [bench_sdr_survey]
instrument: [bladerf, limesdr]
tier: read_config
verified: code only, 2026-10-29
---

# bench_sdr_survey

Steps the tuning from `start_hz` to `stop_hz`, keeps 70 % of each capture's bandwidth and stitches the spectra. At most 400 steps. The radio's own DC spike is removed at every step. Use it to look for signals or noise sources; levels are relative to full scale at the set gain.

## Arguments

<!-- arguments -->

* `instrument`: the SDR alias.
* `start_hz`: first frequency in hertz. Required; within the radio's tuning range (see [bench_sdr_capture](tools/bench_sdr_capture)).
* `stop_hz`: last frequency; default `start_hz` plus 1 MHz; must be above `start_hz`.
* `sample_rate_hz`: default 10e6; the span per step is 70 % of it.
* `gain_db`: default 30, 0 to 70.
* `antenna`: LimeSDR receive input `LNAH`, `LNAL` (default) or `LNAW`.
* `fft_len`: a power of two from 64 to 65536, default 4096; the bin width is the sample rate over this.
* `samples_per_step`: default 262144, at least four times `fft_len`.

## Returns

`start_hz`, `stop_hz`, `steps`, `sample_rate_hz`, `gain_db`, `antenna`, `bin_hz`, `points`, `median_floor_dbfs`, `peaks` (the ten strongest), `overflows`, `csv_file` (frequency and dBFS), `seconds`. A level step at a step boundary (every 0.7 times the sample rate) is the radio's response edge.

## Failure modes

A span needing more than 400 steps is refused (`raise sample_rate_hz or narrow the span`). A survey of a wide span at a high rate takes minutes.

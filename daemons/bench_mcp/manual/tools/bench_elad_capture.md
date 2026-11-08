---
title: bench_elad_capture
summary: ELAD receiver, receive only: stream I/Q to a new cf32 file with rate, dropout, level and clipping summary.
see_also: [instruments/elad/operation, instruments/elad/limits]
tools: [bench_elad_capture]
instrument: elad
tier: read_config
verified: fixtures only
---

# bench_elad_capture

Tunes to `freq_hz`, streams `seconds` of I/Q at `rate_hz` and writes it to a new file `name` in the configured `capture_dir` as interleaved little-endian 32-bit floats, I then Q, full scale 1.0 (cf32). A file that exists is never overwritten. An FDM-S1/S2 not declared loaded is refused before the stream starts.

## Arguments

<!-- arguments -->

`seconds` is in seconds, at most the config's `max_capture_s`, and the sample count at most 16 million. `rate_hz` is in hertz.

## Returns

`file`, `file_bytes`, `format`, `samples`, `rate_hz`, `measured_rate_hz`, `elapsed_s`, `bulk_timeouts`, `bulk_empty_reads`, `bytes`, `rms`, `peak`, `dc_i`, `dc_q`, `clipped_samples`, `calibration_offset_db`, `centre_hz`.

## Failure modes

`FPGA image` errors and `delivered no samples`: see [troubleshooting](instruments/elad/troubleshooting). Nothing is written when the capture fails.

Example (fake unit): `bench_elad_capture {"freq_hz": 7100000, "seconds": 0.25, "name": "t.cf32"}` returned `{"samples": 48000, "rms": 0.1, "clipped_samples": 0, ...}`.

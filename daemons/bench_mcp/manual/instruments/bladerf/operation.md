---
title: bladeRF operation
summary: Identify, capture and survey procedures for the bladeRF.
see_also: [tools/bench_sdr_capture, tools/bench_sdr_survey, instruments/bladerf/limits]
tools: [bench_identify, bench_status, bench_sdr_capture, bench_sdr_survey]
instrument: bladerf
verified: code only, 2026-10-29
---

# bladeRF operation

1. `bench_identify {"instrument": "blade"}`: driver, hardware key, firmware, gateware, serial. A failure here is a missing library or radio.
2. Look at one frequency: `bench_sdr_capture {"instrument": "blade", "freq_hz": 7.1e6, "sample_rate_hz": 2e6, "gain_db": 30}`. Check `clipped_samples` is 0 and `rms_dbfs` is well below 0; lower `gain_db` if it clips, raise it if the floor is near the bottom. Read `continuous`: false means samples were lost.
3. Find signals over a span: `bench_sdr_survey {"instrument": "blade", "start_hz": 1e6, "stop_hz": 30e6, "sample_rate_hz": 10e6}` (at most 400 steps). The `median_floor_dbfs` and `peaks` identify emitters; the `csv_file` has every point.
4. Compare spots with the same gain to compare levels; changing gain changes dBFS.

The saved I/Q is little-endian f32 pairs; the DC spike at the centre is the radio's own and is skipped in the peaks.

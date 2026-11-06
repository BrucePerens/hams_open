---
title: bench_rx888_frontend
summary: RX-888 through radiod: sample rate, A/D overrange count, RF gain, attenuation, RF AGC, IF power, usable edges. Changes nothing.
see_also: [instruments/rx888/overview, instruments/rx888/operation, instruments/rx888/limits]
tools: [bench_rx888_frontend]
instrument: rx888
tier: read_only
verified: measured on the unit (RX-888 MkII, no antenna), 2026-11-05
---

# bench_rx888_frontend

Polls `radiod`'s front end (SSRC 0). This creates no channel. While no channel exists the front end idles: `front_end_running` is false and `if_power_dbfs` is null.

## Arguments

<!-- arguments -->

## Returns

`input_sample_rate_hz`, `real_samples`, `ad_bits`, `ad_overrange_count`, `samples_since_overrange`, `rf_gain_db`, `rf_atten_db`, `rf_agc`, `if_power_dbfs`, `front_end_running`, `rf_level_cal_db`, `usable_low_hz`, `usable_high_hz`, `description`.

## Failure modes

* `radiod does not answer`: start it with bench_rx888_radiod_start.

Example: `bench_rx888_frontend {}` returned on 2026-11-05 (no antenna) `{"input_sample_rate_hz": 64800000, "ad_overrange_count": 0, "rf_gain_db": 34.0, "rf_agc": true, "if_power_dbfs": -52.4, ...}`.

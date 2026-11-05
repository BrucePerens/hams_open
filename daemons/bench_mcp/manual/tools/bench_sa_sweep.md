---
title: bench_sa_sweep
summary: SSA3000X: set range, bandwidths, reference level, attenuation, detector and averaging, run one sweep, save a 751-point CSV and return a summary and peaks.
see_also: [instruments/sa/overview, instruments/sa/operation, instruments/sa/limits]
tools: [bench_sa_sweep]
instrument: sa
tier: read_config
verified: measured on the unit (Siglent SSA3021X, firmware 1.3.9.10), 2026-11-05
---

# bench_sa_sweep


Sets the range (`start_hz` and `stop_hz`, or `center_hz` and `span_hz`) and, when given, `rbw_hz`, `vbw_hz`, `ref_level_dbm`, `attenuation_db`, `detector` and `average_count`; arguments left out keep the unit's present values. It runs one sweep (or `average_count` sweeps) with continuous sweep off, reads the trace in dBm, writes `sa_sweep_*.csv` (`freq_hz,level_dbm`, 751 rows) into `BENCH_MCP_OUT`, and puts the unit back in continuous sweep unless `continuous_after` is false. The settings stay as set. The number of points is fixed by the unit.

It does not touch the tracking generator unless `tg_off_after` is true: then the generator is switched off after the sweep (also when the sweep failed) and verified.

## Arguments

<!-- arguments -->

## Returns

The unit's own readback of the settings (`start_hz`, `stop_hz`, `rbw_hz`, `vbw_hz`, `ref_level_dbm`, `attenuation_db`, `detector`, `sweep_time_s`, `average_count`), `points`, `bin_hz`, `min_dbm`, `median_dbm`, `max_dbm`, `peaks` (up to `peaks`, each `freq_hz` and `level_dbm`, local maxima at least `peak_threshold_db` above the median), `tracking_generator` (state and level at the end, or `off (verified)`), and `file`.

## Failure modes

* An argument outside its range is refused before anything is sent.
* `the analyzer holds ... not the requested`: the unit clamped a value. `would take ... s`: over the 120 s limit. `the sweep did not finish`: see [troubleshooting](instruments/sa/troubleshooting).

Example: `bench_sa_sweep {"start_hz": 1000000, "stop_hz": 101000000, "rbw_hz": 100000}`.

---
title: bench_sa_status
summary: SSA3000X: identity, installed options, tracking generator state and level, and the sweep settings. Changes nothing.
see_also: [instruments/sa/overview, instruments/sa/operation, instruments/sa/limits]
tools: [bench_sa_status]
instrument: sa
tier: read_only
verified: measured on the unit (Siglent SSA3021X, firmware 1.3.9.10), 2026-11-05
---

# bench_sa_status


Reads the identity, the options (`:SYSTem:OPTions?`), the generator and the sweep settings. Changes nothing.

## Arguments

<!-- arguments -->

## Returns

`make`, `model`, `serial`, `firmware`, `max_frequency_hz`, `options` (names only; the trial options Meas, EMI and CAT may lapse), `tracking_generator` (`state` on or off, `level_dbm`, `level_offset_db`, `max_tg_dbm`) and `sweep` (`start_hz`, `stop_hz`, `rbw_hz`, `vbw_hz`, `ref_level_dbm`, `attenuation_db`, `detector`, `sweep_time_s`, `continuous`, `points`).

## Failure modes

* `no usable reply after 3 tries`: see [troubleshooting](instruments/sa/troubleshooting).
* `not an SSA3000X spectrum analyzer`: wrong address; nothing else was sent.

Example: `bench_sa_status {}` returns `{"model": "SSA3021X", "tracking_generator": {"state": "off", "level_dbm": -20.0}, ...}`.

---
title: bench_sa_marker
summary: SSA3000X: marker 1 to 4 on trace 1, optional peak search or placement, and its frequency and level.
see_also: [instruments/sa/overview, instruments/sa/operation, instruments/sa/limits]
tools: [bench_sa_marker]
instrument: sa
tier: read_config
verified: measured on the unit (Siglent SSA3021X, firmware 1.3.9.10), 2026-11-05
---

# bench_sa_marker


Switches marker `marker` (default 1) on for trace 1 in normal mode, then optionally searches (`search`: `max` the highest peak, `next` the next lower one, `left` or `right` the next peak on that side of the marker) or places it at `freq_hz` (not both), and reads its frequency and level. It works on the trace the unit holds, so run [bench_sa_sweep](tools/bench_sa_sweep) first for a fresh one. It leaves the marker on.

## Arguments

<!-- arguments -->

## Returns

`marker`, `trace`, `freq_hz`, `level_dbm`.

## Failure modes

* Both `search` and `freq_hz`, a marker outside 1 to 4, or a frequency above the model's top: refused before anything is sent.

Example: `bench_sa_marker {"search": "max"}` returns `{"marker": 1, "freq_hz": 69133333.0, "level_dbm": -64.56}`.

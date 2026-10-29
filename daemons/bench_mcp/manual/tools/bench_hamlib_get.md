---
title: bench_hamlib_get
summary: One value from the rig, read now: frequency, mode, VFO, split, RIT/XIT, PTT, squelch, a level, a function, a memory channel, or repeated signal readings.
see_also: [instruments/hamlib/overview, instruments/hamlib/operation, instruments/hamlib/limits]
tools: [bench_hamlib_get]
instrument: hamlib
tier: read_only
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_get

`item` selects the value. `level` and `function` need `name` (for example `AF`, `ATT`, `PREAMP`, `NB`); names are letters, digits and underscore only. `signal` reads the signal strength `count` times, `interval_ms` apart, and returns the readings, their mean, the spread (highest minus lowest) and the S-unit text of the mean: the spread is how far one reading can be trusted. `channel` returns the lines the backend reports for a memory channel.

## Arguments

<!-- arguments -->

## Returns

The value with its unit in the key, for example `{"frequency_hz": 7100000.0}`, or for `signal` `{"readings": [...], "mean_db_relative_to_s9": ..., "spread_db": ..., "s_units": ..., "text": "S4"}`.

## Failure modes

* `RPRT -11` or `-4`: the rig does not offer it; not a fault, see [bench_hamlib_models](tools/bench_hamlib_models).
* A reading that is not a finite number is an error, never a zero.

## Example

`bench_hamlib_get {"item": "signal", "count": 5, "interval_ms": 200}`.

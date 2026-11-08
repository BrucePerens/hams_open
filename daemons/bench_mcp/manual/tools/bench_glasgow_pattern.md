---
title: bench_glasgow_pattern
summary: Glasgow: host-timed pattern and pulse generator over up to 8 pins. Full tier, confirm, dry_run, bounded duration.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_pattern]
instrument: glasgow
tier: full
verified: fixtures only
---

# bench_glasgow_pattern

`steps` are `{value, hold_ms}`: `value` is a bit mask over `pins` (bit 0 = the first pin), `hold_ms` 0.5 to 5000, at most 256 steps, repeated `repeat` times, the whole run within max_capture_s. Timing comes from the host (about a millisecond), not the FPGA clock. The pins are released at the end even if the run fails.

## Arguments

<!-- arguments -->

## Returns

`{"steps_run": 9, "elapsed_s": 0.047, "released": true, "note": "levels are set from the host: ..."}`.

## Failure modes

As [bench_glasgow_i2c_scan](tools/bench_glasgow_i2c_scan). A run past its time limit raises inside the helper and the pins are released.

## Example

`bench_glasgow_pattern {"pins": "A0", "steps": [{"value": 1, "hold_ms": 10}, {"value": 0, "hold_ms": 90}], "repeat": 50, "confirm": true}`

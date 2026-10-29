---
title: bench_rotator_point
summary: Point the antenna at an azimuth and elevation inside the limits; read the position back; optional wait; dry_run.
see_also: [instruments/rotator/overview, instruments/rotator/limits, instruments/rotator/operation]
tools: [bench_rotator_point]
instrument: rotator
tier: read_config
verified: dummy rotator only, 2026-10-29
---

# bench_rotator_point

**Moves a motor.** Checks the target against the limits (the rotator's own narrowed by `min_az`, `max_az`, `min_el`, `max_el` in the config) before anything is sent; with `dry_run` returns the plan and sends nothing. Otherwise sends `set_pos`, and with `wait_s` waits up to that long for the position to arrive within the tolerance. The position is always read back at the end. Elevation omitted means the current elevation.

## Arguments

<!-- arguments -->

## Returns

`target`, `position` (the read-back), `waited`, `arrived`, `waited_s`, `tolerance_deg` and a note. `arrived: false` after a wait means the rotator may still be moving: use [bench_rotator_stop](tools/bench_rotator_stop).

## Failure modes

* `outside the limits (...); source: ... Nothing was sent`: the limits and where they come from are named.
* `a rotator is not moved without limits`: set them in the config.
* `RPRT -21`: the rotator refused the position.

## Example

`bench_rotator_point {"azimuth_deg": 90, "elevation_deg": 10, "wait_s": 30}`.

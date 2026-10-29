---
title: bench_rotator_stop
summary: Stop the rotator's motion and check that it stopped. Works while another call runs.
see_also: [instruments/rotator/overview, instruments/rotator/limits, instruments/rotator/operation]
tools: [bench_rotator_stop]
instrument: rotator
tier: read_config
verified: dummy rotator only, 2026-10-29
---

# bench_rotator_stop

Sends `stop` and checks with two position reads, half a second apart, that the position no longer changes; tries up to three times. It does not take the instrument lock, so it works while another call is waiting for a move, which it ends. Safe at any time.

## Arguments

<!-- arguments -->

## Returns

`{"stopped": true, "moving": false, "azimuth_deg": ..., "elevation_deg": ...}`.

## Failure modes

* `STOP NOT CONFIRMED`: the rotator still moves or could not be asked. Remove power from the rotator's controller or use its own stop control, and do not use the rotator until the owner has looked at it.

## Example

`bench_rotator_stop {}`.

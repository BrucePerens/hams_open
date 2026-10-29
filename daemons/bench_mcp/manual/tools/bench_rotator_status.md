---
title: bench_rotator_status
summary: Rotator position, limits, info and whether it is moving. Changes nothing.
see_also: [instruments/rotator/overview, instruments/rotator/limits, instruments/rotator/operation]
tools: [bench_rotator_status]
instrument: rotator
tier: read_only
verified: dummy rotator only, 2026-10-29
---

# bench_rotator_status

Reads the position twice, half a second apart, to say whether the rotator is moving, and reports the limits (the rotator's own narrowed by the config) and the rotator's info text. Changes nothing.

## Arguments

<!-- arguments -->

## Returns

`{"azimuth_deg": 0.0, "elevation_deg": 0.0, "moving": false, "limits": {...}, "info": "..."}`. `limits` is null when neither the rotator nor the config gives any.

## Failure modes

* `cannot connect` or no answer: the text ends with the next action for a rotator; see [troubleshooting](instruments/rotator/troubleshooting).

## Example

`bench_rotator_status {}`.

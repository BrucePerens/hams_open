---
title: bench_rotator_park
summary: Send the rotator to its park position; optional wait for the motion to end.
see_also: [instruments/rotator/overview, instruments/rotator/limits, instruments/rotator/operation]
tools: [bench_rotator_park]
instrument: rotator
tier: read_config
verified: dummy rotator only, 2026-10-29
---

# bench_rotator_park

Sends `park`. With `wait_s` it waits up to that long for the position to stop changing and says whether the motion ended; without it, it reports the position at once and does not claim the end of the motion.

## Arguments

<!-- arguments -->

## Returns

`{"parked_command_sent": true, "motion_ended": true, "position": {...}, "waited_s": 4.5}`.

## Failure modes

* No answer or a refusal: the text ends with the next action; use [bench_rotator_stop](tools/bench_rotator_stop) if it may be moving.

## Example

`bench_rotator_park {"wait_s": 60}`.

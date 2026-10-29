---
title: Rotator operation
summary: The normal order of calls and an example.
see_also: [instruments/rotator/overview, instruments/rotator/limits, tools/bench_rotator_point]
instrument: rotator
verified: dummy rotator only, 2026-10-29
---

# Rotator operation

1. `bench_rotator_status`: position, limits and whether it is moving.
2. `bench_rotator_point {"azimuth_deg": 90, "elevation_deg": 10, "dry_run": true}` shows what would be sent; then without `dry_run`, with `wait_s` to wait for arrival. A move is slow: the dummy rotator turns at about 6.6 degrees per second, a real one differs.
3. `bench_rotator_stop` at any time; it does not wait for a call that is waiting for a move. `bench_rotator_park` at the end of a job.

Calls to one rotator are serialized except the stop. Position is read back after every move; it is a read, not a promise that the antenna points there.

Example: `bench_rotator_status {}` returns `{"azimuth_deg": 0.0, "elevation_deg": 0.0, "moving": false, "limits": {"min_az": 0.0, "max_az": 360.0, "min_el": 0.0, "max_el": 90.0, "source": "..."}}`.

---
title: bench_hamlib_scan
summary: Step the receiver across a range with a bounded dwell, list signal strength and squelch per frequency, and restore the rig.
see_also: [instruments/hamlib/overview, instruments/hamlib/operation, instruments/hamlib/limits]
tools: [bench_hamlib_scan]
instrument: hamlib
tier: read_config
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_scan

Sets each frequency from `start_hz` to `stop_hz` in `step_hz` steps, waits `dwell_ms`, and reads the signal strength (and the squelch state when the rig reports one). At most 400 steps and 120 seconds in one call; both limits are checked before the first byte is sent, and the whole range must be inside the receive limits. `stop_when_above_db` ends the scan at the first frequency stronger than that many dB relative to S9 (a seek). The frequency and mode the rig had are put back on every path, including after an error.

## Arguments

<!-- arguments -->

## Returns

`points` (each `frequency_hz`, `strength_db_rel_s9`, `s_units` and `squelch_open` where known), `strongest` (the top ones, strongest first), `steps`, `stopped_early`, and `restored` saying whether the frequency and mode were put back.

## Failure modes

* `N steps is more than the 400 allowed`: raise `step_hz` or split the range.
* `would take about N s`: lower `dwell_ms` or the number of steps. The estimate includes about 60 ms of rig time per step.
* An error in the middle ends the scan; the rig is still restored, and `restored` tells.

## Example

`bench_hamlib_scan {"start_hz": 7000000, "stop_hz": 7200000, "step_hz": 5000, "dwell_ms": 150, "mode": "USB"}`.

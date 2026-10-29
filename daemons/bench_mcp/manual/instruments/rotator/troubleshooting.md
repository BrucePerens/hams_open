---
title: Rotator troubleshooting
summary: Symptom, cause and next action.
see_also: [instruments/rotator/limits, concepts/errors]
instrument: rotator
verified: dummy rotator only, 2026-10-29
---

# Rotator troubleshooting

| Symptom | Cause | Next action |
|---|---|---|
| `cannot connect to rigctld at ...` (the shared client's wording) | `rotctld` is not running on the host and port in the config | start it (see setup) and check `rotctld_port` |
| no answer within the timeout, or `RPRT -5`, `-6` | the rotator's controller is off, the cable or baud rate is wrong | check power and cable, restart `rotctld`, then `bench_rotator_stop` if it may be moving |
| `RPRT -21` or `-1` on a move | the position is outside what the rotator accepts | `bench_rotator_status` shows the limits |
| `a rotator is not moved without limits` | the rotator reports none and the config sets none | set `min_az`, `max_az`, `min_el`, `max_el` |
| `STOP NOT CONFIRMED` | the position still changes after the stop | remove power from the controller or use its own stop; do not use the rotator until the owner has looked |
| `arrived: false` | the wait ended first, or the tolerance is too tight for the rotator's resolution | `bench_rotator_status`; raise `wait_s` or `tolerance_deg` |

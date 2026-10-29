---
title: bench_hamlib_status
summary: A receiver's state in one call: frequency, mode, VFO, split, RIT/XIT, signal strength, gains, AGC, attenuator, preamp, NB/NR, PTT, power state.
see_also: [instruments/hamlib/overview, instruments/hamlib/operation, instruments/hamlib/limits]
tools: [bench_hamlib_status]
instrument: hamlib
tier: read_only
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_status

Reads the frequency, mode and passband, VFO, split, RIT and XIT, PTT state (read only), carrier detect, power state, the rig's info string, every level and function from a fixed receiver list that the rig says it supports (signal strength, AF and RF gain, squelch, IF shift, noise reduction and blanker, AGC, attenuator, preamp and others), and the transmit-side meters, which read zero in receive. A function the rig does not offer is not an error: it is listed under `unavailable` with the reason. A link failure ends the call with an error.

## Arguments

<!-- arguments -->

## Returns

Numbers carry their unit in the key (`frequency_hz`, `passband_hz`, `rit_hz`). `levels.STRENGTH` has `db_relative_to_s9`, `s_units` and `text`. `levels.AGC` has the code and a name. `ptt` is `receive`, `TRANSMIT` or `unknown (this rig does not report PTT)`. `functions` are booleans. Levels from AF to NR are fractions from 0 to 1 as Hamlib reports them; ATT and PREAMP are in dB.

## Failure modes

* Link errors (no answer, closed connection): the text ends with the next action; see [troubleshooting](instruments/hamlib/troubleshooting).
* The S-unit text assumes 6 dB per unit; see [operation](instruments/hamlib/operation).

## Example

`bench_hamlib_status {}` on the dummy rig returns `"frequency_hz": 145000000.0, "mode": "FM"` and a `levels` object.

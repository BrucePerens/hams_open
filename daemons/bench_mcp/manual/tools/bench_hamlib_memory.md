---
title: bench_hamlib_memory
summary: Recall a memory channel and report the frequency and mode the rig then shows.
see_also: [instruments/hamlib/overview, instruments/hamlib/operation, instruments/hamlib/limits]
tools: [bench_hamlib_memory]
instrument: hamlib
tier: read_config
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_memory

Selects the memory VFO (where the backend accepts that) and channel `channel`, then reads the frequency and mode. A channel may hold a frequency outside the receive limits; the answer then carries a `warning`. Refused while the rig reports PTT on.

## Arguments

<!-- arguments -->

## Returns

`{"channel": 3, "frequency_hz": ..., "mode": {...}, "note": null}`. `note` says when the rig did not switch to the memory VFO and the channel was chosen by number alone.

## Failure modes

* `channel is required`: give a whole number from 0 to 99999.
* `RPRT -11`: the rig has no such memory function; see [bench_hamlib_models](tools/bench_hamlib_models).

## Example

`bench_hamlib_memory {"channel": 12}`.

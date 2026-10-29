---
title: bench_hamlib_rigs
summary: The configured rigs: tier, whether rigctld runs, whether the rig answers. Starts nothing.
see_also: [instruments/hamlib/overview, instruments/hamlib/operation, instruments/hamlib/limits]
tools: [bench_hamlib_rigs]
instrument: hamlib
tier: read_only
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_rigs

For each hamlib instrument: its tier, the state of its rigctld (managed or existing, running, port, restarts, last exit) and, when the daemon is reachable, whether the rig answers a frequency read. A managed rig whose daemon is not running is reported as not started; the next call that needs it starts it. Nothing is started or changed.

## Arguments

<!-- arguments -->

## Returns

`{"rigs": [{"name", "tier", "rigctld": {...}, "rig": {"answers": true, "frequency_hz": ...}}]}`. When the rig does not answer, `rig.error` carries the text and the next action.

## Failure modes

* `no hamlib instrument is configured`: add an entry with `"driver": "hamlib"`; see [setup](instruments/hamlib/setup).

## Example

`bench_hamlib_rigs {}` returns each rig with `"answers": true` when its frequency can be read.

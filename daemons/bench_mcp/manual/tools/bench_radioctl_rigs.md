---
title: bench_radioctl_rigs
summary: The configured rigs: tier, whether radioctld runs, whether the rig answers. Starts nothing.
see_also: [instruments/radioctl/overview, instruments/radioctl/operation, instruments/radioctl/limits]
tools: [bench_radioctl_rigs]
instrument: radioctl
tier: read_only
verified: virtual radioctl rig only, 2026-11-11
---

# bench_radioctl_rigs

For each radioctl instrument: its tier, the state of its radioctld (managed or existing, running, port, restarts, last exit) and, when the daemon is reachable, whether the rig answers a frequency read. A managed rig whose daemon is not running is reported as not started; the next call that needs it starts it. Nothing is started or changed.

## Arguments

<!-- arguments -->

## Returns

`{"rigs": [{"name", "tier", "radioctld": {...}, "rig": {"answers": true, "frequency_hz": ...}}]}`. When the rig does not answer, `rig.error` carries the text and the next action.

## Failure modes

* `no radioctl instrument is configured`: add an entry with `"driver": "radioctl"`; see [setup](instruments/radioctl/setup).

## Example

`bench_radioctl_rigs {}` returns each rig with `"answers": true` when its frequency can be read.

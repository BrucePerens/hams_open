---
title: bench_radioctl_watch
summary: Watches the rig's dial by polling and returns the changes of frequency, mode, VFO and PTT with their times.
see_also: [instruments/radioctl/overview, tools/bench_radioctl_status, instruments/radioctl/limits]
tools: [bench_radioctl_watch]
instrument: radioctl
tier: read_only
verified: virtual radioctl rig only, 2026-11-11
---

# bench_radioctl_watch

For someone turning the dial, or software retuning the rig: polls frequency, mode, passband, VFO and PTT every `interval_ms` for `seconds` and returns the starting values and every change with the time it was seen. radioctld has no event channel on this protocol, so the tool polls: a change that reverts between two polls is not seen, and a time is the time of the poll that saw it. At most 200 events. Changes nothing.

## Arguments

<!-- arguments -->

## Returns

`initial` (the starting values as text), `events` (each `t_s`, `what`, `from`, `to`), `polls`, `events_truncated`.

## Failure modes

* A rig that stops answering ends the call with an error and its next action; no stale value is returned.

## Example

`bench_radioctl_watch {"seconds": 20, "interval_ms": 250}` while turning the tuning knob.

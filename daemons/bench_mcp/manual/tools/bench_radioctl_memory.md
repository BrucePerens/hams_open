---
title: bench_radioctl_memory
summary: Recall a memory channel and report the frequency and mode the rig then shows.
see_also: [instruments/radioctl/overview, instruments/radioctl/operation, instruments/radioctl/limits]
tools: [bench_radioctl_memory]
instrument: radioctl
tier: read_config
verified: virtual radioctl rig only, 2026-11-11
---

# bench_radioctl_memory

**radioctld serves no memory channels today (`RPRT -4`), so with it this tool reports that and changes nothing;** the rest of this page is what it does with a daemon that does. Selects the memory VFO (where the daemon accepts that) and channel `channel`, then reads the frequency and mode. A channel may hold a frequency outside the receive limits; the answer then carries a `warning`. Refused while the rig reports PTT on.

## Arguments

<!-- arguments -->

## Returns

`{"channel": 3, "frequency_hz": ..., "mode": {...}, "note": null}`. `note` says when the rig did not switch to the memory VFO and the channel was chosen by number alone.

## Failure modes

* `channel is required`: give a whole number from 0 to 99999.
* `RPRT -11`: the rig has no such memory function; see [bench_radioctl_models](tools/bench_radioctl_models).

## Example

`bench_radioctl_memory {"channel": 12}`.

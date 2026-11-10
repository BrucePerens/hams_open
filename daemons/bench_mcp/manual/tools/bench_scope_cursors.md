---
title: bench_scope_cursors
summary: Values of a channel at chosen times, and differences between two cursors, from the waveform memory.
see_also: [tools/bench_capture, instruments/scope]
tools: [bench_scope_cursors]
instrument: scope
tier: read_only
verified: fixtures only
---

# bench_scope_cursors

Reads the channel's waveform memory and returns its value at each time in `times_s` (seconds from the first sample, linear interpolation, `null` outside the record). With exactly two times it also returns `delta_time_s`, `delta_volts` and `frequency_hz` (1 over the time difference). This is a host-side cursor: the scope's own cursor commands were not used. Times are not relative to the trigger.

## Returns

`cursors` (time and volts), `record_s`, `sample_interval_s`, a note, and for two cursors the differences.

## Arguments

<!-- arguments -->

* `channel`: `1` or `2`. Required.
* `times_s`: one to eight times in seconds from the first sample. Required.
* `allow_stopped`: true to read a stopped scope's frozen frame; otherwise a stopped scope is refused.
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

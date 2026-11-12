---
title: bench_scope_cursor_native
summary: The scope's own cursors: mode, type, positions and values.
see_also: [tools/bench_scope_cursors, instruments/scope]
tools: [bench_scope_cursor_native]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_cursor_native

Uses the scope's own cursors on `channel`. `mode` is `OFF`, `MANUAL` or `TRACK` and `type` is `X`, `Y` or `X-Y` (`CRMS`, `CRTY`); the cursor positions are set with `CRST`: `x1_s` and `x2_s` (times) and `y1_v` and `y2_v` (voltages) in manual mode, `track_x1_s` and `track_x2_s` in track mode. With the cursors on, their values are read (`CRVA? HREL`: the time difference, its reciprocal, the two times; `CRVA? VREL`: the voltage difference and the two voltages). When the cursors are off the scope gives no values and `no_answer` is returned. This differs from `bench_scope_cursors`, which reads values at times of your choice from the waveform memory and does not use the scope's cursors. From the SDS1000X-E programming guide; not yet checked on firmware 1.3.27.

## Returns

`sent`, `readback`, `settings` (`mode`, `type`) and `values`: `hrel` (`delta_s`, `inverse_hz`, `x1_s`, `x2_s`) and `vrel` (`delta_v`, `y1_v`, `y2_v`), each as numbers or `{no_answer: <text>}`.

## Arguments

<!-- arguments -->

* `channel`: 1 or 2: the trace the cursors belong to.
* `mode`: `OFF`, `MANUAL` or `TRACK`.
* `type`: `X`, `Y` or `X-Y` (manual mode).
* `x1_s, x2_s`: manual X cursors, seconds from the screen centre.
* `y1_v, y2_v`: manual Y cursors, volts.
* `track_x1_s, track_x2_s`: track-mode cursors, seconds.
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

---
title: bench_scope_math
summary: Add, subtract, multiply or divide channel 1 and channel 2 on the host from one frame.
see_also: [tools/bench_capture, tools/bench_scope_spectrum, instruments/scope]
tools: [bench_scope_math]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_math

Computes `channel 1 op channel 2` sample by sample from the waveform memory. The scope's own math trace is not used. A running scope is stopped for the two readings so both channels come from the same frame, then set back to the trigger mode it was in (`AUTO`, `NORM` or `SINGLE`); that is why the tool needs tier `read_config`. The result is saved as little-endian f32 and summarised. Both channels must have the same sample rate.

## Returns

`samples`, `sample_interval_s`, `min`, `max`, `mean`, `rms`, `undefined_points` (divisions by a channel 2 value under 1 uV, which give 0 in the file), `frame_frozen_for_the_reading` and `file`. For `mul` the unit is volts squared; for `div` a ratio.

## Arguments

<!-- arguments -->

* `op`: `add`, `sub`, `mul` or `div`. Required.
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

---
title: bench_scope_measure_custom
summary: The scope's own measurement list: install, statistics, delay, gate and snapshot.
see_also: [tools/bench_measure, instruments/scope]
tools: [bench_scope_measure_custom]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_measure_custom

Uses the scope's own measurement functions. `install` (up to 5 of `{item, channel}`) installs measurements with `PACU` (a command, not a query: the earlier probe's silence on `PACU?` was a query of a command); `clear` removes them (`MEACL`); `statistics` (`on`, `off`, `reset`, `PASTAT`) turns on the count, mean, minimum, maximum and standard deviation the scope keeps for each installed measurement; `delay` (`{type, sources}`, `MEAD`) starts a delay or phase measurement between the two channels; `gate`, `gate_a_s` and `gate_b_s` (`MEGS`, `MEGA`, `MEGB`) limit what is measured to a time window; `snapshot` (a channel) reads every item at once (`PAVA? ALL`; the earlier probe got no answer to this on firmware 1.3.27, so a silent reply is returned as `no_answer`). The host-side `statistics` argument of `bench_measure` still exists and does not depend on the scope's list. From the SDS1000X-E programming guide; not yet checked on firmware 1.3.27.

## Returns

`sent`, `readback`, `gate` (as the scope reports it), `custom` (one object per slot `CUST1` to `CUST5`: `channel`, `item`, `value`, or `off: true`), `statistics` (with statistics on: per installed slot `cur`, `mean`, `min`, `max`, `std-dev`, `count` and the scope's `text`), `delay` and `snapshot` when asked.

## Arguments

<!-- arguments -->

* `clear`: true: remove all installed measurements first.
* `install`: up to 5 objects `{item, channel}`; `item` is one of the guide's list (PKPK, MAX, MIN, AMPL, TOP, BASE, CMEAN, MEAN, STDEV, VSTD, RMS, CRMS, OVSN, FPRE, OVSP, RPRE, LEVELX, DELAY, TIMEL, PER, FREQ, PWID, NWID, RISE, FALL, WID, DUTY, NDUTY), `channel` 1 or 2.
* `statistics`: `on`, `off` or `reset`.
* `delay`: `{type, sources}`: type PHA, FRR, FRF, FFR, FFF, LRR, LRF, LFR, LFF or SKEW; sources `C1-C2` or `C2-C1`.
* `gate`: true or false: measure only between the gate times.
* `gate_a_s, gate_b_s`: gate start and end, seconds (A not after B).
* `snapshot`: 1 or 2: read every measurement of that channel at once.
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

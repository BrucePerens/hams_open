---
title: bench_scope_reference
summary: Reference waveforms: select, store, show, scale and close.
see_also: [tools/bench_screenshot, instruments/scope]
tools: [bench_scope_reference]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_reference

Selects a reference waveform (`location` REFA to REFD, `REFLA`) and its source (`REFSR`: C1, C2 or MATH), stores the source's screen waveform in it (`save`, `REFSA`: the stored trace is shown), shows or hides it (`REFDS`), sets its vertical position (`REFPO`) and scale (`REFSC`, 500 uV to 10 V per division), or closes the reference function (`close`, `REFCL`). Commands are sent in that order whatever the argument order. The guide has no command that reads a reference trace back or compares it, so the trace is checked on the screen with `bench_screenshot`. From the SDS1000X-E programming guide; not yet checked on firmware 1.3.27.

## Returns

`sent`, `readback` and `settings`: `location`, `source`, `display`, `position_v`, `scale_v` as the scope reports them. The scope ignores a display, position or scale setting for a location that holds no stored trace, and the read-back shows the old value.

## Arguments

<!-- arguments -->

* `location`: `REFA`, `REFB`, `REFC` or `REFD`.
* `source`: `C1`, `C2` or `MATH`.
* `save`: true: store the source's waveform in the location.
* `display`: true or false: show or hide the stored trace.
* `position_v`: vertical offset in volts.
* `scale_v`: vertical scale in volts per division, 0.0005 to 10.
* `close`: true: close the reference function.
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

---
title: bench_scope_channel
summary: Bandwidth limit and inversion of a scope channel, read back.
see_also: [tools/bench_set, instruments/scope]
tools: [bench_scope_channel]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_channel

Switches a channel's 20 MHz bandwidth limit and inversion on or off (`BWL`, `INVS`) and reads both back. Probe attenuation, coupling, volts per division and offset stay in [bench_set](tools/bench_set). The commands are not yet confirmed on firmware 1.3.27; the read-back shows what was accepted.

## Returns

`sent`, `readback`, `bandwidth_limit`, `invert`.

## Arguments

<!-- arguments -->

* `channel`: `1` or `2`. Required.
* `bandwidth_limit`: true or false.
* `invert`: true or false.
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

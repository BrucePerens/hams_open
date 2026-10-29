---
title: bench_vna_generator_off
summary: Switch the VNA generator's TX output off; safe at any time.
see_also: [tools/bench_vna_generator_on]
tools: [bench_vna_generator_off]
instrument: vna
tier: read_config
verified: code only, 2026-10-29
---

# bench_vna_generator_off

Switches the TX output off. Safe at any time, including when nothing is on. It does not stop the AM generator, which needs [bench_vna_power_cycle](tools/bench_vna_power_cycle).

## Arguments

<!-- arguments -->

* `instrument`: the VNA alias.

## Returns

`{"generator": "off"}`.

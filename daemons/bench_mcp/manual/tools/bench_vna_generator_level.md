---
title: bench_vna_generator_level
summary: Change the level of a running VNA generator.
see_also: [tools/bench_vna_generator_on]
tools: [bench_vna_generator_level]
instrument: vna
tier: read_config
verified: levels 0 to -40 dB checked on the scope within 0.3 dB, 2026-10-29
---

# bench_vna_generator_level

Changes the level of a running generator without changing its frequency.

## Arguments

<!-- arguments -->

* `level_db`: negative attenuation from the top level, 0 to -50 dB (0 dB is about -17 dBm). Required.
* `instrument`: the VNA alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

## Returns

`{"generator": "level set", "level_db": <value>}`. Refused when out of range.

## NanoVNA

With a `nanovna` instrument this tool is not supported (it returns a typed error). See [the NanoVNA page](instruments/vna/nanovna).

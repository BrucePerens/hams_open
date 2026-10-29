---
title: bench_vna_generator_am
summary: VNA generator with the one decoded amplitude modulation setting; only a power cycle stops it.
see_also: [tools/bench_vna_power_cycle, tools/bench_vna_generator_off, concepts/errors]
tools: [bench_vna_generator_am]
instrument: vna
tier: read_config
verified: code only, 2026-10-29
---

# bench_vna_generator_am

Carrier at `freq_hz` and -20 dB, amplitude modulated with a 500 Hz sine at 100 %. This is the only modulation setting decoded so far: other frequencies, depths and FM are not available. The unit then modulates in real time and refuses most control requests, so only [bench_vna_power_cycle](tools/bench_vna_power_cycle) stops it. Plan for that before starting it.

## Arguments

<!-- arguments -->

* `freq_hz`: carrier in hertz, above 0 and at most 1.3e9. Required.
* `instrument`: the VNA alias.

## Returns

`generator: "AM on"`, `freq_hz`, `level_db` (-20), `modulation` and a warning.

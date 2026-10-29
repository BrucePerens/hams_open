---
title: bench_vna_generator_am
summary: VNA generator with amplitude modulation at a chosen frequency and depth; only a power cycle stops it.
see_also: [tools/bench_vna_generator_fm, tools/bench_vna_generator_off, tools/bench_vna_power_cycle, instruments/vna/modulation, concepts/errors]
tools: [bench_vna_generator_am]
instrument: vna
tier: read_config
verified: 2026-10-29 on a scope at 10 MHz (depth within 3 points, frequency within 5 Hz, 500 to 3000 Hz, 10 to 100 %, -6 to -30 dB)
---

# bench_vna_generator_am

A carrier at `freq_hz` and `level_db`, amplitude modulated by a sine of `mod_hz` at `depth_pct`. The unit's microcontroller makes the modulation in real time. Afterwards the unit refuses the switch-off request, so [bench_vna_generator_off](tools/bench_vna_generator_off) ends it by power cycling the unit through its hub; plan for the 15 to 90 seconds and the loss of the unit's settings. Nothing else can use the VNA while it modulates.

## Arguments

<!-- arguments -->

* `freq_hz`: carrier in hertz, above 0 and at most 1.3e9. Required.
* `level_db`: the carrier's peak level, 0 to -50 dB (a negative attenuation); default -20. With depth m the unpeaked carrier is lower: the envelope peaks at this level.
* `mod_hz`: modulation frequency, 1 to 3000 Hz; default 500. Above about 3 kHz the unit's 18.75 kHz modulation clock gives too few steps per cycle.
* `depth_pct`: modulation depth, 1 to 100 percent; default 100.
* `instrument`: the VNA alias.

## Returns

`generator: "AM on"`, the settings, what was verified and a warning.

---
title: bench_vna_generator_fm
summary: VNA generator with frequency modulation at a chosen frequency and deviation; only a power cycle stops it.
see_also: [tools/bench_vna_generator_am, tools/bench_vna_generator_off, tools/bench_vna_power_cycle, instruments/vna/modulation]
tools: [bench_vna_generator_fm]
instrument: vna
tier: read_config
verified: 2026-10-29 on a scope at 10 MHz (deviation within 3 % at 1 and 2 kHz modulation, 10 and 20 kHz deviation)
---

# bench_vna_generator_fm

A carrier at `freq_hz` and `level_db`, frequency modulated by a sine of `mod_hz` with a peak deviation of `deviation_hz`. The unit switches between 255 discrete frequencies around the carrier. Like AM, it leaves the unit modulating and refusing the switch-off request: [bench_vna_generator_off](tools/bench_vna_generator_off) power cycles it through the hub.

## Arguments

<!-- arguments -->

* `freq_hz`: carrier in hertz, above 0 and at most 1.3e9. Required.
* `deviation_hz`: peak deviation in hertz, 100 to 300000. Required.
* `level_db`: 0 to -50 dB; default -20.
* `mod_hz`: modulation frequency, 1 to 3000 Hz; default 500.
* `instrument`: the VNA alias.

## Returns

`generator: "FM on"`, the settings, what was verified and a warning.

## NanoVNA

With a `nanovna` instrument this tool is not supported (it returns a typed error). See [the NanoVNA page](instruments/vna/nanovna).

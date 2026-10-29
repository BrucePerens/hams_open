---
title: bench_vna_generator_on
summary: VNA signal generator: continuous wave on the TX port at a frequency and level until switched off.
see_also: [tools/bench_vna_generator_off, tools/bench_vna_generator_level, tools/bench_vna_generator_am, concepts/etiquette]
tools: [bench_vna_generator_on]
instrument: vna
tier: read_config
verified: generator output confirmed on the scope at 1, 3.5, 10 and 14 MHz (about 31 mV rms into 50 ohm = -17 dBm) and levels 0 to -40 dB within 0.3 dB, 2026-10-29
---

# bench_vna_generator_on

CW on the TX port. It stays on until [bench_vna_generator_off](tools/bench_vna_generator_off) (or a power cycle). Do not run a sweep or the meter while it is on.

## Arguments

<!-- arguments -->

* `freq_hz`: carrier frequency in hertz, above 0 and at most 1.3e9.
* `level_db`: level as a negative attenuation from the top level, 0 to -50 dB; 0 dB is about -17 dBm.
* `instrument`: the VNA alias.

## Returns

`generator: "on"`, `freq_hz`, `level_db` and a note.

## Failure modes

Arguments outside the ranges are refused. Level steps were verified on a scope from 0 to -40 dB; below that, and the absolute level at other frequencies, are not verified. Always switch off at the end of the job.

## Example

`bench_vna_generator_on {"freq_hz": 10e6, "level_db": -6}` then measure on the scope with [bench_scope_spectrum](tools/bench_scope_spectrum).

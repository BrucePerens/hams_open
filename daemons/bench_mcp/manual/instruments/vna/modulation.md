---
title: VNA generator modulation (AM, FM)
summary: What the VNWA's AM and FM do, how they were checked, and why only a power cycle ends them.
see_also: [instruments/vna/monitoring-and-recovery, instruments/vna/troubleshooting, tools/bench_vna_generator_am, tools/bench_vna_generator_fm, tools/bench_vna_generator_off]
tools: [bench_vna_generator_am, bench_vna_generator_fm, bench_vna_generator_off]
instrument: vna
verified: 2026-10-29 on the unit (VNWA 3E, firmware v5.22) with a scope on the TX tee
---

# VNA generator modulation (AM, FM)

The unit makes the modulation itself, in its microcontroller, from a software oscillator that steps at 18.75 kHz for AM and 15.625 kHz for FM. Measured on a scope at a 10 MHz carrier through the pad and tee: AM depth 10, 50, 80 and 100 % came out as 10.3, 49 to 50, 77 to 80 and 98 to 100 %; the modulation frequency was right to 5 Hz at 500, 1000 and 3000 Hz; levels from -6 to -30 dB all gave the same envelope ratio. FM deviations of 10 and 20 kHz at 1 and 2 kHz modulation fitted 9.9 and 19.4 to 19.9 kHz.

**It cannot be stopped by software.** After the start request the unit keeps modulating at constant depth and refuses the request that switches the generator off (and every other request that was tried), for as long as it is powered; this was the same with silence after the start, with status polling every 10 ms, with the codecs closed, at every frequency, depth and level tried, and for FM. The only way out is to cut its power through the hub. The tools do that for you in [bench_vna_generator_off](tools/bench_vna_generator_off). After a modulation the unit's control chip sometimes does not enumerate at power-on; a longer ladder of power cycles (up to five) brings it back. The unit loses all settings.

Use the modulated carrier with the scope or a receiver; do not run a sweep, the meter or the calibration while it modulates.

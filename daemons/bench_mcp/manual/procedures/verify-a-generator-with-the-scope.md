---
title: Verify a generator with the scope
summary: Check the frequency and level of the VNA generator (or any source) with the scope's host-side spectrum.
see_also: [tools/bench_vna_generator_on, tools/bench_scope_spectrum, instruments/scope/measurement-recipes]
tools: [bench_vna_generator_on, bench_vna_generator_off, bench_scope_spectrum, bench_set]
instrument: [vna, scope]
verified: measured 2026-10-28 (10 MHz, 0 to -40 dB within 0.3 dB)
---

# Verify a generator with the scope

1. The person cables TX through a pad and a tee to scope channel 1, with the other tee port in 50 ohms. Confirm it.
2. `bench_set {"instrument": "scope", "channel": 1, "coupling": "D1M", "attenuation": 1, "vdiv_v": 0.01, "timebase_s": 1e-7, "trigger_mode": "AUTO"}`.
3. `bench_vna_generator_on {"freq_hz": 10e6, "level_db": -10}`.
4. `bench_scope_spectrum {"channel": 1, "points": 131072}`. Expected: peak within a few ppm of 10 MHz and about 3.27 mV rms through the pad and tee.
5. Repeat for other levels with `bench_vna_generator_level`; ignore lines more than 45 dB below the strongest.
6. `bench_vna_generator_off`. Restore the scope settings.

---
title: Scope measurement recipes
summary: Worked examples with expected results: a generator's level and frequency, harmonics, a supply's ripple.
see_also: [instruments/scope/operation, instruments/scope/limits, tools/bench_scope_spectrum]
instrument: scope
verified: generator check measured 2026-10-28; ripple recipe code only
---

# Scope measurement recipes

## Check a generator's frequency and level

Source: the VNA generator at 10 MHz, -6 dB (`bench_vna_generator_on {"freq_hz": 10e6, "level_db": -6}`), through a pad and tee into channel 1 with the other tee port in 50 ohms.

1. `bench_set {"channel": 1, "coupling": "D1M", "attenuation": 1, "vdiv_v": 0.01, "timebase_s": 1e-7, "trigger_mode": "AUTO"}`.
2. `bench_scope_spectrum {"channel": 1, "points": 131072}`.

Expected (measured 2026-10-28): strongest peak at 9 999 965 Hz (the scope's timebase 3.5 ppm off, or the unit's), 5.22 mV rms = -45.65 dBV = -32.6 dBm, 1.5 % from the prediction; the generator's 360 MHz DDS clock line 36 dB down and images at 350 and 370 MHz (made lower than real by the 200 MHz bandwidth); second and third harmonics below the 8-bit floor. Switch the generator off afterwards.

## A generator's level steps

Repeat with `bench_vna_generator_level` from 0 to -40 dB; the steps were verified within 0.3 dB with this method.

## Ripple on a supply rail

(code only) Supply output on at a known load, `coupling` `A1M`, `vdiv_v` 0.005, timebase 1e-3, then `bench_measure {"channel": 1, "items": ["PKPK", "RMS", "FREQ"]}`. Keep the probe ground lead short.

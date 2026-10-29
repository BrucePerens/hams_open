---
title: VNWA measurement recipes
summary: Worked examples with expected results: loopback, calibrated through, generator level check, frequency meter loopback.
see_also: [instruments/vna/operation, instruments/vna/limits, instruments/scope/measurement-recipes]
instrument: vna
verified: measured on the unit (loopback, scope), 2026-10-28; calibrated through 2026-10-28
---

# VNWA measurement recipes

## Loopback sweep (TX to RX through a pad and tee)

`bench_vna_sweep {"start_hz": 100e3, "stop_hz": 150e6, "points": 60}`. Expected: S21 flat in RF frequency apart from the cable (about -1 dB at 10 MHz rising to +2.6 dB at 300 MHz with a phase linear in frequency; raw, uncalibrated). Repeating after a power cycle agrees to 0.09 % and 0.05 degrees.

## Through calibration check

Measure `thru` (session `s1`, cable text `pad+tee`), build, then `bench_vna_sweep` with `calibration` and the same `cable`: S21 reads 1 to 0.013 % and 0.014 degrees on the same cabling at the same RF level.

## Generator level against the scope

`bench_vna_generator_on {"freq_hz": 10e6, "level_db": -10}`, `bench_scope_spectrum {"channel": 1}`: expected 3.27 mV rms through the pad and tee at -10 dB (3.254 predicted), 0.321 mV at -30 dB; every step from 0 to -40 dB within 0.3 dB of `16383 x 10^(dB/20)`. -50 dB was not measurable (below the scope's strongest spur). Then `bench_vna_generator_off`.

## Frequency meter loopback

`bench_vna_meter {"freq_hz": 10e6}` with TX looped to RX: offset 0.00000 to 0.00002 Hz, standard error 0.00001 Hz at 10, 15.5 and 100 MHz. That shows precision (TX and reference share the clock), not accuracy against a standard.

## Low frequencies

200 Hz to 3.8 kHz in 20 points at IF 4000 and 60 ms agrees with the vendor program to 0.1 % (S11); 100 Hz needs the minimum reference amplitude lowered (not an argument of the tool); 70 Hz and below is refused by the steadiness check.

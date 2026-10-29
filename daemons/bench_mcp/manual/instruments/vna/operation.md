---
title: VNWA operation
summary: Step-by-step procedures for every VNA function: sweeps, calibration, generator, AM, frequency meter, power cycle.
see_also: [instruments/vna/measurement-recipes, instruments/vna/limits, instruments/vna/troubleshooting, concepts/errors]
tools: [bench_vna_sweep, bench_vna_calibrate_measure, bench_vna_calibrate_build, bench_vna_generator_on, bench_vna_generator_off, bench_vna_generator_level, bench_vna_generator_am, bench_vna_meter, bench_vna_power_cycle, bench_status]
instrument: vna
verified: measured on the unit (VNWA 3E, firmware v5.22), 2026-10-29; AM and the meter's sign see limits
---

# VNWA operation

One call at a time on the VNA. Everything the unit holds is lost at a power cycle.

## Sweep (S11 and S21)

1. `bench_status`: unit present, codecs found.
2. `bench_vna_sweep {"start_hz": 1e6, "stop_hz": 100e6, "points": 101}`. Each point takes about 0.2 s at 60 ms per point; the unit emits for six point times. Up to 401 points.
3. Read `s21_db_min`/`max` and, for at most 51 points, `rows`; the CSV `file` has every point (S21 and S11 as real, imaginary, dB, degrees).
4. Check `recovery`: re-measured points or power cycles mean the link was troubled; `recovered_failures` shows why.

Choices: lower `if_hz` and longer `point_ms` reduce noise and slow the sweep. Use the same `rf_level` for a sweep as for the calibration you apply: the ratio depends on the drive level (half level read 11 to 16.5 % low with a full-level through calibration).

## Calibrate and apply

Measure each standard with the same start, stop, points, IF, point time, RF level and cable text, then build and apply. Procedure: [calibrate the VNA](procedures/calibrate-the-vna). A calibration is refused at other settings or cable text, or outside its frequency range.

## Generator

`bench_vna_generator_on {"freq_hz": 10e6, "level_db": -6}`; change the level with `bench_vna_generator_level`; `bench_vna_generator_off` when done. Wait about 3 s after a sweep or meter reading before using the generator: a command sent during a burst is overwritten. Do not sweep while it is on.

## AM

`bench_vna_generator_am {"freq_hz": 10e6}` (500 Hz, 100 %, carrier at -20 dB). The unit then stalls control requests; only `bench_vna_power_cycle` stops it. Plan the power cycle first and tell the person.

## Frequency meter

Connect the signal to RX, `bench_vna_meter {"freq_hz": <expected frequency>}`: the reading is the TX tuning-word frequency plus the measured offset, valid within +/-25 Hz of the centre; takes about 2 s. A TX-to-RX loopback reads 0. Switch the generator off first.

## Power cycle

`bench_vna_power_cycle` when the sweep says there is no signal on the reference channel, `bench_status` says `present but not answering`, or after AM. About 12 s plus 8 s settle; verified by a one-point sweep at 10 MHz unless `verify` is false.

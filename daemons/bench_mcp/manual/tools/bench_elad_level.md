---
title: bench_elad_level
summary: ELAD receiver, receive only: the level at a frequency in dBFS with the calibration offset; the dBm estimate is unverified.
see_also: [instruments/elad/operation, instruments/elad/limits]
tools: [bench_elad_level]
instrument: elad
tier: read_config
verified: fixtures only
---

# bench_elad_level

Tunes to `freq_hz`, averages the spectrum and returns the strongest bin and the summed power within `bandwidth_hz` of the centre. `bandwidth_hz` is a half-width in hertz, 100 to 20000, default 1000.

## Arguments

<!-- arguments -->

## Returns

`peak_bin_dbfs`, `band_power_dbfs`, `band_half_width_hz`, `calibration_offset_db`, `level_dbm_estimate_unverified` (the peak plus the EEPROM offset; not checked against a generator) and the summary fields of [bench_elad_capture](tools/bench_elad_capture).

## Failure modes

As [bench_elad_capture](tools/bench_elad_capture). Compare the numbers yourself: no judgement is returned.

Example (fake unit, tone amplitude 0.1): `peak_bin_dbfs` about -20.

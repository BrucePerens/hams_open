---
title: bench_sa_tg_s21
summary: SSA3000X: scalar S21 sweep with the tracking generator, in dB against a stored through reference or the generator level; CSV of dB against Hz.
see_also: [instruments/sa/overview, instruments/sa/operation, instruments/sa/limits]
tools: [bench_sa_tg_s21]
instrument: sa
tier: read_config
verified: fixtures only
---

# bench_sa_tg_s21


Switches the generator on at `tg_level_dbm`, takes one sweep, and writes `sa_tg_s21_*.csv` (`freq_hz,s21_db,measured_dbm`, 751 rows). It outputs RF on the TG OUTPUT connector at the level set (0 dBm at most). The generator is switched off afterwards and verified, unless `leave_tg_on` is true.

With `reference` (a name from [bench_sa_tg_normalize](tools/bench_sa_tg_normalize)) or `reference_file`, `s21_db` is the measured level minus the through's, and any sweep argument or level not given is taken from the reference; a sweep whose range, RBW, VBW, attenuation, detector, averaging or level differs is refused with the field named. Without a reference, `s21_db` is the measured level minus the generator level, which includes the cable and the generator's own flatness (the level is "uncalibrated").

## Arguments

<!-- arguments -->

## Returns

The sweep summary, `s21` (`relative_to`, `max_db` and `max_at_hz`, `min_db` and `min_at_hz`, `mean_db`), `tg_level_dbm`, `file` and `tracking_generator`. A reading near the sweep's noise floor (see `min_dbm` and `median_dbm`) is not reliable.

## Failure modes

* An unknown reference name: run the normalize tool first. Both `reference` and `reference_file`: refused.

Example: `bench_sa_tg_s21 {"reference": "thru"}`.

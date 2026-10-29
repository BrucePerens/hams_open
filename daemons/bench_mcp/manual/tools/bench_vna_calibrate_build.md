---
title: bench_vna_calibrate_build
summary: Calibration step 2: build the calibration file of a session from its stored standards.
see_also: [tools/bench_vna_calibrate_measure, tools/bench_vna_sweep]
tools: [bench_vna_calibrate_build]
instrument: vna
tier: read_config
verified: code only, 2026-10-29
---

# bench_vna_calibrate_build

Builds `cal_<session>.cal` in the output directory from the stored standards: open, short and load give S11; thru gives S21. Standards that are missing leave that parameter uncalibrated.

## Arguments

<!-- arguments -->

* `session`: the session name used in [bench_vna_calibrate_measure](tools/bench_vna_calibrate_measure).
* `kit_file`: optional path of a kit definition (the real kit's published numbers; the text format of `vnwa_open` `cal::KitDefinition`). Without it ideal standards are assumed.
* `created`: optional text recorded in the file (a date or a note).
* `instrument`: the VNA alias.

## Returns

`file`, `start_hz`, `stop_hz`, `points`, `s11_calibrated`, `s21_calibrated`, the `if_hz`, `point_ms`, `rf_level` and `cable` of the calibration, and the note on how to apply it: `bench_vna_sweep` with `calibration=<file>` and the same `cable`. It is refused at other settings, cable text or frequency range.

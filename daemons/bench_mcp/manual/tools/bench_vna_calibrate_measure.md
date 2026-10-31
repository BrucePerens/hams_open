---
title: bench_vna_calibrate_measure
summary: Calibration step 1: sweep one standard (open, short, load or thru) and store it under a session name.
see_also: [tools/bench_vna_calibrate_build, tools/bench_vna_sweep]
tools: [bench_vna_calibrate_measure]
instrument: vna
tier: read_config
verified: code only, 2026-10-29
---

# bench_vna_calibrate_measure

Sweeps one calibration standard and stores it with the settings and the `cable` text. Repeat for each standard with the same `start_hz`, `stop_hz`, `points` and `cable`, then run [bench_vna_calibrate_build](tools/bench_vna_calibrate_build).

## Arguments

<!-- arguments -->

* `session`: a name of 1 to 40 letters, digits, `-` or `_` that groups the standards.
* `standard`: `open`, `short`, `load` (reflection standards on the reflection port, for S11) or `thru` (TX cabled to RX, for S21).
* `cable`: free text describing what is between the ports and the reference plane; it must be identical later when the calibration is applied.
* `start_hz`, `stop_hz`, `points`, `if_hz`, `point_ms`, `rf_level`: as in [bench_vna_sweep](tools/bench_vna_sweep); `points` is at least 2 here.
* `instrument`: the VNA alias.

## Returns

`session`, `standard`, `points`, `file` and a note naming the next step. The person must connect the standard before each call; ask them.

## NanoVNA

With a `nanovna` instrument this tool is supported (cable text only in the settings line). See [the NanoVNA page](instruments/vna/nanovna).

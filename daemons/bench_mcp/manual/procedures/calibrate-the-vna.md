---
title: Calibrate the VNA with a kit
summary: Open, short, load and through calibration of the VNWA and applying it to a sweep.
see_also: [tools/bench_vna_calibrate_measure, tools/bench_vna_calibrate_build, tools/bench_vna_sweep, instruments/vna/limits]
tools: [bench_vna_calibrate_measure, bench_vna_calibrate_build, bench_vna_sweep]
instrument: vna
verified: through calibration checked on the unit 2026-10-28; open, short and load standards not yet measured on the unit
---

# Calibrate the VNA with a kit

Needs the person at the bench: they connect each standard. Decide the frequency range, `points`, IF, point time and RF level once and use them for every step and the later sweeps. Write one `cable` text naming what is between the ports and the reference plane (for example `SMA short cable 30 cm`); it must be identical everywhere.

1. `bench_status`: unit present.
2. Ask for the open on the reflection port; `bench_vna_calibrate_measure {"session": "k1", "standard": "open", "cable": "<text>", "start_hz": 1e6, "stop_hz": 100e6, "points": 101}`.
3. Same with `short`, then `load`.
4. For S21, ask for TX cabled to RX with the same cables as the later measurement; `standard: thru`.
5. `bench_vna_calibrate_build {"session": "k1", "kit_file": "<optional kit definition>"}`: check `s11_calibrated` and `s21_calibrated`.
6. Apply: `bench_vna_sweep` with the same range and points (or a range inside), `calibration` set to the file and `cable` set to the same text. A refusal names the setting that differs.
7. Check: sweep the load or the through again; the calibrated load should read near -40 dB or lower, the through near 0 dB and 0 degrees.

A calibration is valid only at its IF, point time, RF level and cable text; the real kit's published numbers go in a kit file. Open, short and load standards have not yet been measured on the unit, so report that the S11 calibration is untested on hardware.

---
title: Units and conventions
summary: Units, level scales, file formats and naming used in tool arguments and results.
see_also: [concepts/configuration]
verified: code only, 2026-10-29
---

# Units and conventions

* The suffix of a field names its unit: `_hz` hertz, `_v` volts, `_a` amperes, `_w` watts, `_s` seconds, `_ms` milliseconds, `_db` decibels, `_deg` degrees, `_dbfs` decibels relative to full scale. Frequencies are plain JSON numbers in hertz (`10000000`, or `10e6`), never "10M".
* Levels: `dbv` is dB relative to 1 V rms at the instrument input; `dbm_50` is dBV plus 13.01 (a 50 ohm source into 50 ohms); `dBFS` is relative to the converter's full scale at the gain that was set, so it is not calibrated power. The generator level of the VNA is a negative attenuation in dB from its top level (0 dB is about -17 dBm).
* VNA values are raw ratios of the unit's own channels unless a calibration was applied; see the VNA pages.
* Channel numbers: the scope's channels are `1` and `2`; the SDR channels are `0` and `1` (the LimeSDR's second receiver is `1`).
* SCPI-style settings: coupling `A1M` (AC, 1 MOhm), `D1M` (DC, 1 MOhm), `GND`; trigger modes `AUTO`, `NORM`, `SINGLE`, `STOP`. A scope that is in `STOP` is stopped: its memory is a frozen old frame.
* Result files: scope captures are little-endian `f32` volts; SDR captures are little-endian `f32` I/Q pairs (full scale 1.0); sweeps and surveys are CSV with a header line; screenshots are BMP or PNG. They are written to the output directory and the result gives the path.
* Results are JSON objects printed as text. A field that could not be measured is `null` or carries an `error` text, never a made-up number.
* Dates in `verified` lines are `YYYY-MM-DD`.
* Tool and argument names are lower case with underscores; enumerated values are exactly as listed in the argument table (scope tokens are upper case).

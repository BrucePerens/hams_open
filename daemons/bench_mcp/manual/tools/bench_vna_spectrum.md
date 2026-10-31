---
title: bench_vna_spectrum
summary: The VNA as a rudimentary, uncalibrated spectrum analyzer on its RX port, below about 100 MHz.
see_also: [instruments/vna/limits, instruments/vna/spectrum-analyzer, tools/bench_vna_sweep, tools/bench_scope_spectrum]
tools: [bench_vna_spectrum]
instrument: vna
tier: read_config
verified: 2026-10-29 on the unit (loopback tone, noise floor, IF settings, TX frequency on the scope); amplitude, ghost lines and subharmonics NOT verified
---

# bench_vna_spectrum

At each of `points` frequencies from `start_hz` to `stop_hz` the unit's local oscillator is set there and the tool reports the rms of the audio (the S21 codec's left channel) between 100 Hz and `band_hz`: the power of whatever the RX port carries within `band_hz` either side of the LO, in codec counts. The result is a CSV file plus the noise floor and the strongest peaks. The tracking generator (the TX chip) is silent unless `tracking_db` is given. Read [the spectrum analyzer page](instruments/vna/spectrum-analyzer) before trusting a line.

## Arguments

<!-- arguments -->

* `start_hz`, `stop_hz`: the LO range; above 12 kHz and at most 1.3e9. Required.
* `points`: 2 to 2000. The spacing `(stop - start) / (points - 1)` must not exceed `band_hz`, or the spectrum has holes (the tool refuses).
* `band_hz`: the audio band, 200 to 20000 Hz; default 5000. It is the resolution: a line is seen at every LO setting within this band of it.
* `point_ms`: the unit's time per point, 5 to 100; default 20. The analysis window is twice this.
* `tracking_db`: 0 to -50; turns the tracking generator on at that level (a negative attenuation). In a loopback the TX then appears as a line at 12 kHz, so use a `band_hz` above 12000 to see it.
* `instrument`: the VNA alias.

## Returns

`noise_floor_counts` (the median), `peaks` (up to ten local maxima more than 3 times the floor), `rows` (the points, when 51 or fewer), `file` (the CSV with `freq_hz`, `rms_counts`, `db_rel_floor`), a note and the recovery counters.

## NanoVNA

With a `nanovna` instrument this tool is not supported (it returns a typed error). See [the NanoVNA page](instruments/vna/nanovna).

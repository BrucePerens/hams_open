---
title: bench_vna_time_domain
summary: Impulse or step response and time gating of a stored VNA sweep, on the host; sends nothing to the unit.
see_also: [instruments/vna/limits, instruments/vna/operation, tools/bench_vna_sweep]
tools: [bench_vna_time_domain]
instrument: vna
tier: read_config
verified: fixtures only
---

# bench_vna_time_domain

Takes a sweep that [bench_vna_sweep](tools/bench_vna_sweep) wrote (the raw CSV, not the `_calibrated` one) and computes, on the host, the impulse or step response of S11 or S21 with the vendor's window set. The peak delay is returned, with a distance when `velocity_factor` is given (the round trip for S11). With a gate it also keeps only what arrives between two times and writes the gated frequency response. The unit is not touched, so the tool works with the unit unplugged.

The sweep must be uniform. Low-pass mode needs the first frequency to be a multiple of the step (2 MHz to 200 MHz in 100 points, for example), and the response repeats every `1 / step` seconds (`unaliased_until_s`). A transform of an uncalibrated sweep shows the cables and the test set as well as the device; calibrate first for true reflection sizes.

## Arguments

<!-- arguments -->

* `file`: the name of a `.csv` that `bench_vna_sweep` wrote, in the output directory. Required; a path is refused.
* `trace`: `s11` (default) or `s21`.
* `window`: `rectangular`, `hamming` (default), `hanning`, `blackman` or `blackman_squared`.
* `mode`: `lowpass` (default; a real response) or `bandpass` (complex; the step response is not available).
* `response`: `impulse` (default) or `step`.
* `time_points`: 8 to 8192; default 512.
* `t_max_s`: the last time computed; default half of `unaliased_until_s`.
* `velocity_factor`: 0.1 to 1; adds `distance_m` for that cable.
* `gate_start_s`, `gate_stop_s`: keep only this time range (both or neither); the gated response goes to a CSV with `freq_hz,re,im,db,deg`.
* `gate_shape`: the window across the gate; default `rectangular`. A shaped gate lowers its peak off its centre.
* `instrument`: the VNA alias (any VNA entry; the file is all that is read).
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

## Returns

`peak_time_s`, `peak_magnitude`, `time_resolution_s`, `unaliased_until_s`, `time_file` (a CSV of time, real, imaginary and magnitude), and when asked `distance_m`, `gated_file` and `gate`.

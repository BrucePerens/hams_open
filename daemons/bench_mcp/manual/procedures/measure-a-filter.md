---
title: Measure a filter
summary: Measure the S21 of a filter with the VNA (calibrated through) and cross-check one point with the scope.
see_also: [procedures/calibrate-the-vna, tools/bench_vna_sweep, procedures/verify-a-generator-with-the-scope]
tools: [bench_vna_sweep, bench_vna_calibrate_measure, bench_vna_calibrate_build]
instrument: [vna, scope]
verified: code only, 2026-10-29
---

# Measure a filter

1. Calibrate the through with the cables that will carry the filter ([calibrate the VNA](procedures/calibrate-the-vna)); measure only `thru` for S21.
2. Ask the person to insert the filter between TX and RX.
3. `bench_vna_sweep` with the calibration, the same cable text and a range that covers the passband and skirts, enough points to resolve the corners (for 401 points over 100 MHz the step is 250 kHz).
4. Read the CSV (`calibrated_file`): the passband loss, the corner (-3 dB), the stop-band depth. Stop-band depth is limited by the unit's dynamic range; do not trust readings below about -60 dB without a check.
5. Cross-check one frequency: `bench_vna_generator_on` at it and measure the output on the scope; compare with the VNA's level difference.

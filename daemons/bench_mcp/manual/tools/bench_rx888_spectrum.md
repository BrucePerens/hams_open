---
title: bench_rx888_spectrum
summary: RX-888 through radiod: one spectrum snapshot over a span, saved as a CSV, with the floor, peaks and zero-bin count.
see_also: [instruments/rx888/overview, instruments/rx888/operation, instruments/rx888/limits]
tools: [bench_rx888_spectrum]
instrument: rx888
tier: read_config
verified: measured on the unit (RX-888 MkII, no antenna), 2026-11-05
---

# bench_rx888_spectrum

Checks the span against the front end: at most half the sample rate for this real-sampling receiver. Opens a spectrum channel (SSRC `ssrc_base + 1`) and asks twice: the first response may wait for FFT planning, and the second has the requested averaging. Writes `rx888_<instrument>_spectrum_<ms>.csv` (`freq_hz,level_db`, ascending), then removes the channel.

## Arguments

<!-- arguments -->

## Returns

`csv`, `center_hz`, `span_hz`, `bins`, `bin_hz`, `noise_bandwidth_hz`, `fft_n`, `averaging`, `floor_db` (median), `min_db`, `max_db`, `zero_bins`, `peaks` (`freq_hz`, `level_db`, `above_floor_db`; at least 10 dB over the floor), `front_end` (overrange count, RF gain).

## Failure modes

* `wider than the front end can fill`: lower `span_hz`, or start `radiod` at the higher rate.
* One zero bin at exactly 0 Hz is normal.

Example: `bench_rx888_spectrum {"bins": 8192, "span_hz": 32350000, "center_hz": 16175000}` returned with no antenna `{"floor_db": -122.7, "peaks": [{"freq_hz": 3949, ...}, {"freq_hz": 24301990, "above_floor_db": 12.4}], "zero_bins": 1, ...}`.

---
title: RTL-SDR operation
summary: Identify, capture and survey procedures for the RTL-SDR, with the NOAA weather radio and FM checks.
see_also: [tools/bench_sdr_capture, tools/bench_sdr_survey, instruments/rtlsdr/limits]
tools: [bench_identify, bench_status, bench_sdr_capture, bench_sdr_survey]
instrument: rtlsdr
verified: hardware, 2026-11-11
---

# RTL-SDR operation

1. `bench_identify {"instrument": "rtl"}`: reports hardware `R820T`, one receive channel.
2. Capture NOAA weather radio: one capture at 162.475 MHz with `sample_rate_hz` 1.024e6 covers all seven channels (162.400, .425, .450, .475, .500, .525, .550 MHz): `bench_sdr_capture {"instrument": "rtl", "freq_hz": 162475000, "sample_rate_hz": 1024000, "gain_db": 38.6, "samples": 1048576}`. The strongest carrier shows in `peaks`, well above `noise_floor_dbfs`. Plus-or-minus 4 to 5 kHz deviation is normal for these transmitters.
3. Survey the FM broadcast band: `bench_sdr_survey {"instrument": "rtl", "start_hz": 88e6, "stop_hz": 108e6}` (the default rate is 2.4 MS/s, 12 steps).
4. If stations sit at the wrong frequency, find the crystal error from a known carrier and pass it as `ppm` (for example `"ppm": -35`).
5. HF: set `"direct_samp": "2"` in the config, restart the server, then capture between 0.1 and 28.8 MHz. Expect poor sensitivity.

The first capture after opening takes up to about 0.3 s longer than the signal lasts (USB start-up); the `continuous` flag allows for that. Levels are dBFS at the set gain, not power; a clean 8-bit capture has a floor around -57 dBFS at 38.6 dB gain. The R820T's PLL occasionally logs "PLL not locked" on the first tune; it is harmless.

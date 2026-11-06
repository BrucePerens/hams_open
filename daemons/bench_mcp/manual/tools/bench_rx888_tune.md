---
title: bench_rx888_tune
summary: RX-888 through radiod: a channel at a frequency; baseband power, noise density, SNR, AGC gain, measured input rate. Receive only.
see_also: [instruments/rx888/overview, instruments/rx888/operation, instruments/rx888/limits]
tools: [bench_rx888_tune]
instrument: rx888
tier: read_config
verified: measured on the unit (RX-888 MkII, no antenna), 2026-11-05
---

# bench_rx888_tune

Opens a channel (SSRC `ssrc_base`) at `freq_hz` in `mode`, optionally with a `bandwidth_hz` passband, lets it settle for 0.5 s, and averages `readings` status reads 100 ms apart. The channel is then removed. The first channel after `radiod` starts can take seconds while the input FFT is planned.

## Arguments

<!-- arguments -->

## Returns

`frequency_hz`, `mode`, `passband_hz`, `baseband_power_db`, `noise_density_db_per_hz`, `noise_in_passband_db`, `snr_db` (-30: no signal above the noise), `agc`, `channel_gain_db`, `output_level_dbfs`, `readings`, `input_sample_rate_measured_sps`, `rate_interval_s`, `rate_resolution_fraction`, `note`.

## Failure modes

* `radiod did not acknowledge`: see [troubleshooting](instruments/rx888/troubleshooting).

Example: `bench_rx888_tune {"freq_hz": 7074000, "mode": "usb", "bandwidth_hz": 2400}` returned with no antenna `{"baseband_power_db": -127.7, "noise_density_db_per_hz": -161.0, "snr_db": -30.0, ...}`.

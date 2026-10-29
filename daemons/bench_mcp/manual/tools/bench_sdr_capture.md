---
title: bench_sdr_capture
summary: Tune a bladeRF or LimeSDR, capture complex baseband samples (receive only) and report level, noise floor and peaks.
see_also: [tools/bench_sdr_survey, tools/bench_sdr_coherence, concepts/units-and-conventions]
tools: [bench_sdr_capture]
instrument: [bladerf, limesdr]
tier: read_config
verified: code only, 2026-10-29
---

# bench_sdr_capture

Tunes the radio to `freq_hz`, captures `samples` complex samples at `sample_rate_hz` and `gain_db`, saves the raw I/Q and returns statistics. Receive only: nothing in bench_mcp transmits.

## Arguments

<!-- arguments -->

* `instrument`: the SDR alias; needed when both radios are configured.
* `freq_hz`: centre frequency in hertz. Required. LimeSDR: 30 MHz to 3.8 GHz. bladeRF: 237.5 MHz to 3.8 GHz, or 9 kHz to 3.8 GHz when the entry has `xb200` (the XB-200 transverter; below 237.5 MHz the bladeRF cannot tune without it).
* `sample_rate_hz`: default 2e6; 0.25e6 up to 30.72e6 (LimeSDR) or 20e6 (bladeRF).
* `gain_db`: default 30, 0 to 70.
* `antenna`: LimeSDR only: receive input `LNAH`, `LNAL` or `LNAW` (the H, L and W connectors of each channel). The default is `LNAL`, which is what the driver selects at every frequency; a signal on an H or W connector is not heard unless you choose it. The bladeRF has one input.
* `samples`: default 262144, 4096 to 16777216.
* `channel`: `0`, or `1` for the LimeSDR's second receiver.

## Returns

`freq_hz`, `sample_rate_hz`, `gain_db`, `samples`, `channel`, `antenna`, `rms_dbfs`, `peak_amplitude`, `dc_offset` (I, Q), `clipped_samples`, `overflows_reported_by_driver`, `noise_floor_dbfs`, `fft_bin_hz`, `peaks` (the strongest, with the radio's DC spike removed), `iq_file` and `iq_format` (little-endian f32 I,Q pairs, full scale 1.0), `stream_seconds`, `real_time_seconds` and `continuous` (whether the samples arrived at the sample rate), and a note. Levels are dBFS at the set gain, not calibrated power.

## Failure modes

An argument outside the radio's limits is refused before the radio is opened. A missing SoapySDR library or module, or a radio that is not plugged in, is reported by name. The bladeRF reports an overrun about every 504 samples with its FPGA 0.16 without losing samples; the timing check (`continuous`) is what to trust. A capture that took longer than its signal lasts is flagged as having lost samples.

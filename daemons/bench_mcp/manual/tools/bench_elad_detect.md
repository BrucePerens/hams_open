---
title: bench_elad_detect
summary: ELAD receiver, receive only: signals near a frequency with refined frequency, level and SNR; false when nothing stands out.
see_also: [instruments/elad/operation, instruments/elad/limits]
tools: [bench_elad_detect]
instrument: elad
tier: read_config
verified: fixtures only
---

# bench_elad_detect

Tunes to `freq_hz`, averages the spectrum (16 FFT lengths of samples) and lists peaks at least `threshold_db` (3 to 60, default 10) over the noise floor, the median bin. Each frequency is refined below one bin by a parabola through the three bins around the peak. Peaks beyond the usable 80 % of the span and the DC bins are ignored.

## Arguments

<!-- arguments -->

## Returns

`detected`, `signals` (each `freq_hz`, `offset_hz`, `power_dbfs`, `snr_db`), `noise_floor_dbfs`, `bin_hz`, `usable_half_span_hz`, `threshold_db` and the summary fields of [bench_elad_capture](tools/bench_elad_capture). `snr_db` is the peak bin over the median bin: a one-bin bandwidth, not the bandwidth of the signal.

## Failure modes

As [bench_elad_capture](tools/bench_elad_capture), and a refusal of the arguments before the receiver is opened. While the relay or another bench call has the receiver, the error names the holder: the receiver is single-owner (a lock file per receiver, see [limits](instruments/elad/limits)).

Example (fake unit, tone amplitude 0.05, noise 0.01 rms per component): frequency within 5 Hz of the tone, `snr_db` about 45.

---
title: bench_scope_math_native
summary: The scope's own math and FFT: equation, scale, FFT settings, trace read.
see_also: [tools/bench_scope_math, tools/bench_scope_spectrum, instruments/scope]
tools: [bench_scope_math_native]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_math_native

Uses the scope's own math. `equation` (`DEFINE EQN`) is one of `C1+C2`, `C1-C2`, `C1*C2`, `C1/C2` (or the channels the other way round) or a transform `FFTC1`, `INTGC1`, `DIFFC1`, `SQRTC1` (also for C2); `scale_v` and `position` (`MTVD`, `MTVP`) and `invert` (`MATH:INVS`) set an arithmetic result; `fft_window` (`FFTW`), `fft_center_hz` (`FFTC`), `fft_unit` (`FFTU`), `fft_scale` (`FFTS`), `fft_position_v` (`FFTP`) and `fft_screen` (`FFTF`) set the FFT. With `read` the math trace (`MATH:WF? DAT2`) is read: an arithmetic or transform trace is converted with the guide's formula (volts = code * scale / 25 - position / 50 * scale), summarised and saved as little-endian f32. The guide says the FFT trace cannot be read this way, so for an FFT the raw trace's summary is returned with the peak's index and an assumed frequency (ten divisions of `FFTT?` round `FFTC?`) marked as an assumption, and the level is not converted: use `bench_scope_spectrum` for levels. The guide has no command to switch the math trace on; if the scope shows no trace, switch it on at the front panel. The host-side `bench_scope_math` and `bench_scope_spectrum` do not depend on any of this. From the SDS1000X-E programming guide; not yet checked on firmware 1.3.27.

## Returns

`sent`, `readback`, `settings` (each setting as the scope reports it), `equation` (`DEFINE?`) and, with `read`, `trace` (`points`, `min`, `max`, `file`) or, for an FFT, `fft_trace` (`points`, `peak_index`, `peak_code`, `peak_frequency_hz_assumed`, `assumption`).

## Arguments

<!-- arguments -->

* `equation`: one of C1+C2, C1-C2, C1*C2, C1/C2, C2+C1, C2-C1, C2*C1, C2/C1, FFTC1, INTGC1, DIFFC1, SQRTC1 and the same for C2 (any case).
* `scale_v`: vertical scale of the result, 0.0005 to 100 V per division.
* `position`: vertical position, -255 to 255 screen pixels.
* `invert`: true or false: invert the result.
* `fft_window`: `RECT`, `BLAC`, `HANN`, `HAMM` or `FLATTOP`.
* `fft_center_hz`: FFT centre frequency, 0 to 1e9 Hz.
* `fft_unit`: `VRMS`, `DBM` or `DBVRMS`.
* `fft_scale`: FFT vertical scale (0.1 to 20 for the dB units, 0.001 to 20 for Vrms).
* `fft_position_v`: FFT vertical offset in volts (Vrms scale only).
* `fft_screen`: `OFF` (split screen), `ON` (full screen) or `EXCLU` (exclusive).
* `read`: true: read the math trace.
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

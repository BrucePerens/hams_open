---
title: bench_scope_spectrum
summary: Spectrum of a scope channel computed on the host from the waveform memory, with peaks in dBV and dBm.
see_also: [tools/bench_capture, tools/bench_set, instruments/scope]
tools: [bench_scope_spectrum]
instrument: scope
tier: read_config
verified: measured on the scope (SDS1202X-E, firmware 1.3.27) with the VNWA generator at 10 MHz, 2026-10-28
---

# bench_scope_spectrum

Captures the channel, windows the first `points` samples, transforms them on the host and lists the strongest peaks. The scope's own FFT and trigger are not involved. Use it to check a generator's fundamental and harmonics or to look for spurs.

## Arguments

<!-- arguments -->

* `channel`: `1` or `2`.
* `window`: `rect`, `hanning` (default), `hamming` or `blackman`. Hanning and Blackman keep leakage down; `rect` is exact only for a tone that fits the record.
* `points`: a power of two from 16 to 1048576, default 131072. The frequency resolution (`bin_hz`) is the sample rate divided by `points`.
* `memory_depth`: sets the scope's memory depth first (`14K`, `140K`, `1.4M`, `14M`); the call waits one second. The scope must be able to deliver `points` samples.
* `timebase_s`: sets seconds per division first (waits 1.5 s). A longer timebase lowers the sample rate and gives finer resolution; the span is half the sample rate.
* `peaks`: how many peaks, 1 to 50, default 10.
* `range_db`: only peaks within this many dB of the strongest, default 60.
* `allow_stopped`: as in [bench_capture](tools/bench_capture).
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

## Returns

`channel`, `window`, `points`, `sample_rate_hz`, `bin_hz`, `span_hz`, `enbw_bins`, a `note`, and `peaks`: each with `freq_hz`, `vrms`, `dbv` (rms at the 1 MOhm scope input) and `dbm_50` (dBV plus 13.01, the level a 50 ohm source terminated in 50 ohms would read).

## Limits

The samples are 8 bit: lines more than about 45 dB below the strongest are not reliable. The scope's bandwidth is 200 MHz, so lines above that read lower than they are. Frequency accuracy is the scope's timebase (about 3.5 ppm off on the tested unit).

## Example

With the VNA generator at 10 MHz and -6 dB through a pad and tee to channel 1: `bench_scope_spectrum {"channel": 1, "points": 131072}` found the strongest peak at 9 999 965 Hz, 5.22 mV rms (-45.65 dBV, -32.6 dBm; 1.5 % from the prediction), with the generator's 360 MHz clock line 36 dB down. Second and third harmonics were below the 8-bit floor.

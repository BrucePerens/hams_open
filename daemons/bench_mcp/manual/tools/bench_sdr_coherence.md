---
title: bench_sdr_coherence
summary: LimeSDR: phase and gain of receive channel 0 against channel 1 at the strongest tone, across retunes.
see_also: [tools/bench_sdr_capture, instruments/limesdr]
tools: [bench_sdr_coherence]
instrument: limesdr
tier: read_config
verified: code only, 2026-10-29
---

# bench_sdr_coherence

Captures both receivers together at `freq_hz`, repeats with a fresh tune each time, and reports the phase and gain of channel 0 relative to channel 1 at the strongest tone, how stable the phase is within a capture and across retunes, and the coherence. It needs one signal on both inputs (a splitter); with different or no signals the numbers are meaningless, check that coherence is near 1.

## Arguments

<!-- arguments -->

* `instrument`: the SDR alias; the radio must have two receive channels (the LimeSDR; a bladeRF is refused).
* `freq_hz`: frequency in hertz, 30 MHz to 3.8 GHz. Required.
* `sample_rate_hz`: default 2e6, 0.25e6 to 30.72e6.
* `gain_db`: default 30, 0 to 70.
* `antenna`: receive input `LNAH`, `LNAL` (default) or `LNAW`.
* `repeats`: 1 to 10, default 4 fresh tunes.

## Returns

`freq_hz`, `mean_phase_deg` (circular mean over the repeats), `phase_std_across_retunes_deg`, a note, and `runs`: for each repeat `tone_offset_hz`, `phase_deg`, `phase_std_within_capture_deg`, `gain_db` (channel 0 relative to channel 1), `coherence` and `blocks`.

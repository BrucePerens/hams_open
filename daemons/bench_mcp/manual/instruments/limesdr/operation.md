---
title: LimeSDR operation
summary: Identify, capture, survey and coherence procedures for the LimeSDR.
see_also: [tools/bench_sdr_capture, tools/bench_sdr_survey, tools/bench_sdr_coherence]
tools: [bench_identify, bench_status, bench_sdr_capture, bench_sdr_survey, bench_sdr_coherence]
instrument: limesdr
verified: code only, 2026-10-29
---

# LimeSDR operation

1. `bench_identify {"instrument": "lime"}`.
2. Capture: `bench_sdr_capture {"instrument": "lime", "freq_hz": 145e6, "antenna": "LNAL", "channel": 0}`. Use `channel` 1 for the second receiver.
3. Survey: `bench_sdr_survey {"instrument": "lime", "start_hz": 30e6, "stop_hz": 300e6, "sample_rate_hz": 30e6}`; 540 steps at 10 MS/s is refused (limit 400), so raise the rate or narrow the span.
4. Coherence: split a signal to both inputs, then `bench_sdr_coherence {"instrument": "lime", "freq_hz": 100e6, "repeats": 4}`. A coherence near 1 means the numbers mean something; `phase_std_across_retunes_deg` shows how repeatable the phase is after a retune (it is only fixed if the oscillator is shared and the retune keeps the relation).

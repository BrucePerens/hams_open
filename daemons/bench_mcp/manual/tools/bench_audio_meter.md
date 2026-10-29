---
title: bench_audio_meter
summary: Level readings from an input every interval for N seconds.
see_also: [instruments/sound/overview, instruments/sound/safety]
tools: [bench_audio_meter]
instrument: sound
tier: read_config
verified: run on the dev box (virtual sink and source, and the speaker with the digital microphone), 2026-10-29
---

# bench_audio_meter

Level readings from an input every interval for N seconds.

Arguments: `input`, `seconds`, `interval_ms`. Nothing is saved. `input` is named, else the configured `verify_input`, else the input the last discovery found; the system default source is never used, so with none of these the call is refused.

## Arguments

<!-- arguments -->

---
title: bench_audio_loopback
summary: Play a tone on one output while recording one input to prove the path.
see_also: [instruments/sound/overview, instruments/sound/safety]
tools: [bench_audio_loopback]
instrument: sound
tier: full
verified: run on the dev box (virtual sink and source, and the speaker with the digital microphone), 2026-10-29
---

# bench_audio_loopback

Play a tone on one output while recording one input to prove the path.

Arguments: `output`, `input`, `freq_hz`, `seconds`, `level_dbfs`, `volume`, `urgent`. Returns heard or not with level, signal-to-noise ratio, the noise floor before the tone and an indicative latency (it includes process start-up).

## Arguments

<!-- arguments -->

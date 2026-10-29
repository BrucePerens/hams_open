---
title: bench_audio_play
summary: Play a generated tone or a managed WAV on an output inside the volume cap.
see_also: [sound/overview, sound/safety]
tools: [bench_audio_play]
instrument: sound
tier: full
verified: run on the dev box (virtual sink and source, and the speaker with the digital microphone), 2026-10-29
---

# bench_audio_play

Play a generated tone or a managed WAV on an output inside the volume cap.

Arguments: `output`, `tone` (sine, sweep, two_tone, noise), `freq_hz`, `freq2_hz`, `seconds`, `level_dbfs`, `file`, `volume`, `unmute`. The output volume is limited for the playback and restored afterwards.

---
title: bench_audio_speak
summary: Speak text on an output or write it to a WAV file.
see_also: [sound/overview, sound/safety]
tools: [bench_audio_speak]
instrument: sound
tier: full
verified: virtual sink and source with espeak-ng, 2026-10-29
---

# bench_audio_speak

Speak text on an output or write it to a WAV file.

Arguments: `text`, `output`, `engine`, `voice`, `language`, `rate_wpm`, `wav_file`, `volume`, `unmute`. speech-dispatcher plays on the system default output only; espeak-ng and Piper play on the chosen one.

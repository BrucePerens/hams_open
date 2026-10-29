---
title: bench_audio_speak
summary: Speak text on an output or write it to a WAV file.
see_also: [instruments/sound/overview, instruments/sound/safety]
tools: [bench_audio_speak]
instrument: sound
tier: full
verified: virtual sink and source with espeak-ng, 2026-10-29
---

# bench_audio_speak

Speak text on an output or write it to a WAV file.

Arguments: `text`, `output`, `engine`, `voice`, `language`, `rate_wpm`, `wav_file`, `volume`, `unmute`, `dry_run`. speech-dispatcher plays on the system default output only; espeak-ng and Piper play on the chosen one. `dry_run: true` returns the plan and the volume that would be used (current volume and mute are read) and plays nothing, changes no volume or mute and writes nothing. The output is the one named in `output`, else the configured `alert_output`, else the one the last discovery found; the system default output is never used, so with none of these the call is refused (name one, set `alert_output`, or run `bench_audio_discover`).

## Arguments

<!-- arguments -->

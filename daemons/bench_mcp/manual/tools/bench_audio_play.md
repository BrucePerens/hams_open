---
title: bench_audio_play
summary: Play a generated tone or a managed WAV on an output inside the volume cap.
see_also: [instruments/sound/overview, instruments/sound/safety]
tools: [bench_audio_play]
instrument: sound
tier: full
verified: run on the dev box (virtual sink and source, and the speaker with the digital microphone), 2026-10-29
---

# bench_audio_play

Play a generated tone or a managed WAV on an output inside the volume cap.

Arguments: `output`, `tone` (sine, sweep, two_tone, noise), `freq_hz`, `freq2_hz`, `seconds`, `level_dbfs`, `file`, `volume`, `unmute`, `dry_run`. The output volume is limited for the playback and restored afterwards. `dry_run: true` returns the plan and the volume that would be used (current volume and mute are read) and plays nothing, changes no volume or mute and writes nothing. The output is the one named in `output`, else the configured `alert_output`, else the one the last discovery found; the system default output is never used, so with none of these the call is refused (name one, set `alert_output`, or run `bench_audio_discover`).

## Arguments

<!-- arguments -->

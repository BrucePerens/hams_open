---
title: bench_audio_events
summary: Listen for N seconds and list sound events with times, levels and pitch.
see_also: [sound/overview, sound/safety]
tools: [bench_audio_events]
instrument: sound
tier: read_config
verified: virtual sink and source (beeps, wrong pitch, no sound, reference), and the dev box digital microphone for silence and events, 2026-10-29
---

# bench_audio_events

Listen for N seconds and list sound events with times, levels and pitch.

Arguments: `seconds`, `min_over_noise_db`, `threshold_dbfs`, `merge_gap_ms`, `min_event_ms`, `decode_morse`. For relays clicking, a rotor motor, an alarm, CW keying. It hears the room; nothing is saved.

---
title: bench_audio_events
summary: Listen for N seconds and list sound events with times, levels and pitch.
see_also: [instruments/sound/overview, instruments/sound/safety]
tools: [bench_audio_events]
instrument: sound
tier: read_config
verified: virtual sink and source (beeps, wrong pitch, no sound, reference), and the dev box digital microphone for silence and events, 2026-10-29
---

# bench_audio_events

Listen for N seconds and list sound events with times, levels and pitch.

Arguments: `seconds`, `min_over_noise_db`, `threshold_dbfs`, `merge_gap_ms`, `min_event_ms`, `decode_morse`. For relays clicking, a rotor motor, an alarm, CW keying. It hears the room; nothing is saved.

## Arguments

<!-- arguments -->

`input` is the microphone: an alias, node name or fragment of its name; when omitted the auto-selected input is used. `sample_rate_hz` is the capture rate, 8000 to 96000, default 48000. `track_hz` is described in the table.

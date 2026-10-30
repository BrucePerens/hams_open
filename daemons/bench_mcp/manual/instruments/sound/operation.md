---
title: Using the sound tools
summary: The normal order of calls for playing, listening, verifying and alerting, one call at a time, with the limits that apply.
see_also: [instruments/sound/setup, instruments/sound/safety, instruments/sound/limits, instruments/sound/discovery]
tools: [bench_audio_status, bench_audio_devices, bench_audio_levels, bench_audio_play, bench_audio_record, bench_audio_meter, bench_audio_verify, bench_audio_safe_state]
instrument: sound
tier: read_only
verified: code only, 2026-10-29
---

# Using the sound tools

1. `bench_audio_status`: the helper programs found and the limits in effect (volume cap, quiet hours and whether they are in force now).
2. `bench_audio_devices`: the outputs and inputs, with the alias, node name and `reserved` flag of each. Reserved devices (the VNWA codecs) appear in the plain list only and every other tool refuses them.
3. Name the device you mean. A tool uses the one you name, else the configured `alert_output` or `verify_input`, else the one the last `bench_audio_discover` found, else it refuses. The system default is never used.
4. `bench_audio_levels` reads volume and mute without opening a stream.
5. Before anything audible, `dry_run: true`: the answer names the output, the volume now and the volume that would be used, and the plan. Nothing sounds.
6. `bench_audio_play`, `bench_audio_speak`, `bench_audio_alert`, `bench_audio_loopback`: the output is lowered to the limit for the sound and put back afterwards, mute included. One call at a time per sound instrument.
7. To check that a sound was made: `bench_audio_verify` (an action named in the config's `verify_actions`, or whatever you do in parallel), `bench_audio_events` for onsets and levels, `bench_audio_meter` for a live level. All of them listen to the room through an input; the recording stays in the capture directory.
8. After anything went wrong, `bench_audio_safe_state`: it stops this server's playback and lowers any output above its cap.

Concurrency: the server holds one lock per instrument, so a long recording delays other sound calls but not the scope or the supply. Tones are generated, not streamed: `seconds` is at most `max_play_s` (default 10), recordings at most `max_record_s` (default 30).

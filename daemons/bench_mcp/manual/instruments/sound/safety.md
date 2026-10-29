---
title: Sound tool safety
summary: Volume cap, quiet hours, restore after use, managed files, and the reserved VNA codecs.
see_also: [instruments/sound/overview, instruments/sound/setup]
tools: [bench_audio_play, bench_audio_loopback, bench_audio_set_volume, bench_audio_record, bench_audio_safe_state]
instrument: sound
tier: full
verified: loopback on the dev box speaker and digital microphone at volume 0.2, volume restored to 0.52, 2026-10-29
---

# Sound tool safety

* Every audible operation is limited by the volume cap (`volume_cap`, or the output's own). The output is lowered to the limit for the operation and put back afterwards, mute included.
* In quiet hours (default 22:00 to 07:00 local) the quiet volume is the limit. Only an explicit `urgent: true` (loopback, alert) uses the cap.
* Every tool that makes sound or changes a volume (`bench_audio_play`, `_speak`, `_alert`, `_loopback`, `_set_volume`, `_discover`) takes `dry_run: true`: it returns the device and the volume it would use and does nothing.
* The system default output and input are never used. A tool uses the device you name, else the configured `alert_output` or `verify_input`, else the one the last discovery found, else it refuses. Streams also carry `node.dont-fallback`, so a device that has gone is an error and not a silent move to the default.
* `bench_audio_safe_state` stops any playback this server started and lowers any output above its cap to the cap; it makes no sound.
* Generated sound is limited to `max_play_s`; recordings to `max_record_s`; tone level to -1 dBFS.
* Files for playback come from the managed capture directory only.
* Recordings can contain speech: they stay in the capture directory (owner only, pruned by `retention_files` and `retention_days`) and no tool uploads them. The tool descriptions of the recording tools say so.
* The VNWA codecs (`08bb:29c0`) are reserved and refused by name in every tool; the check uses the kernel's usb id of each card, not the card number.

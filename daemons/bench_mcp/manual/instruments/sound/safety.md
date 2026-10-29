---
title: Sound tool safety
summary: Volume cap, quiet hours, restore after use, managed files, and the reserved VNA codecs.
see_also: [overview, setup]
tools: [bench_audio_play, bench_audio_loopback, bench_audio_set_volume, bench_audio_record]
instrument: sound
tier: full
verified: loopback on the dev box speaker and digital microphone at volume 0.2, volume restored to 0.52, 2026-10-29
---

# Safety

* Every audible operation is limited by the volume cap (`volume_cap`, or the output's own). The output is lowered to the limit for the operation and put back afterwards, mute included.
* In quiet hours (default 22:00 to 07:00 local) the quiet volume is the limit. Only an explicit `urgent: true` (loopback, alert) uses the cap.
* Generated sound is limited to `max_play_s`; recordings to `max_record_s`; tone level to -1 dBFS.
* Files for playback come from the managed capture directory only.
* Recordings can contain speech: they stay in the capture directory (owner only, pruned by `retention_files` and `retention_days`) and no tool uploads them. The tool descriptions of the recording tools say so.
* The VNWA codecs (`08bb:29c0`) are reserved and refused by name in every tool; the check uses the kernel's usb id of each card, not the card number.

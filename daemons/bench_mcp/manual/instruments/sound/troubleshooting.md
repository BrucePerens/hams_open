---
title: Sound troubleshooting
summary: Symptom, cause and next action for the sound tools, in the order to try.
see_also: [instruments/sound/operation, instruments/sound/limits, tools/bench_audio_safe_state]
instrument: sound
verified: code only, 2026-10-29
---

# Sound troubleshooting

| Symptom | Cause | Next action |
|---|---|---|
| `no output is named, configured or found by discovery, and the system default output is never used` | no `output`, no `alert_output` in the config, no stored discovery | name one with `output` (`bench_audio_devices` lists them), set `alert_output`, or run `bench_audio_discover` |
| `... is reserved: ...` | the device is a VNWA codec or on the `reserved` list | use another device; do not try to unreserve a VNWA codec |
| `pw-play is not installed (not found on PATH)` (also `pw-record`, `wpctl`) | the PipeWire tools are missing | `bench_audio_status` shows which programs are found; install the package that provides them |
| `defined target not found` or `pw-record ended early` | the device went away between listing and use; the stream refuses to fall back to another device on purpose | `bench_audio_devices` again; if the device is gone, plug it in and rediscover |
| `pw-play did not finish within N s and was stopped` | the sink stalled | `bench_audio_safe_state`; check the sound server is running; retry once |
| `the output is muted, so nothing was audible` | the output is muted | repeat with `unmute: true` for the playback only |
| `all_zero` or `dead: exact zeros` | the input is muted, unplugged or its switch is off | unmute it with `bench_audio_set_volume` (mute false) or fix the hardware |
| `quiet hours are in effect` | audible discovery in 22:00 to 07:00 | run it later, or `urgent: true` (quiet volume only) |
| `SAFE STATE NOT CONFIRMED` | an output would not go down to its cap, or the search for running streams failed | lower the volume by hand and tell the owner |
| The VNWA's audio disappears or card numbers move | something restarted the sound server or re-enumerated USB; these tools never do | see [sound server protection](instruments/vna/sound-server-protection); do not restart WirePlumber or PipeWire from a tool |

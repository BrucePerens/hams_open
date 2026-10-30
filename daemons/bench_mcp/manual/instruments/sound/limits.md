---
title: Sound limits
summary: The caps, the quiet hours, the reserved devices, what the measurements mean and what is not verified.
see_also: [instruments/sound/safety, instruments/sound/setup]
instrument: sound
verified: code only, 2026-10-29
---

# Sound limits

* Volume is the wpctl scale; the signal amplitude factor is its cube, so 0.5 is about -18 dB. The cap is `volume_cap` (default 0.5), per output no higher than that output's own cap. In quiet hours (default 22:00 to 07:00 local) the limit is `quiet_volume` (default 0.15), except for an `urgent` request.
* Generated tones are at most `max_play_s` (default 10 s) and at most -1 dBFS (default -12); recordings at most `max_record_s` (default 30 s); an alert at most 30 s in all and 5 repeats; speech at most 600 characters.
* Files for playback come only from the managed capture directory. Recordings are kept `retention_files` (default 50) files and `retention_days` (default 14) days.
* Levels are dBFS of the capture, not sound pressure: no microphone is calibrated. "Heard" means a tone's level at its exact frequency was over the noise before it and over neighbouring frequencies, by thresholds in the result.
* A muted or disconnected input records exact zeros; the tools report `all_zero` and never pass such an input.
* The latency from `bench_audio_loopback` includes process start-up and the graph's buffering; it is not the device latency.
* speech-dispatcher plays on its own path (the default output) and cannot target another device; `espeak-ng` and Piper play on the chosen one.
* Reserved devices: the two USB AUDIO CODEC cards (`08bb:29c0`) of the VNWA, refused in every tool by their USB ids and place in the tree, and any device listed in `reserved`. Whatever the owner reserves by name is also refused.
* Not verified: HDMI and Bluetooth outputs beyond listing them; Piper voices; behaviour with the sound server stopped (the tools report the missing program or the failed stream).
* Nothing here transmits. Legal compliance of any sound that is broadcast or recorded (consent to record speech) is the owner's responsibility.

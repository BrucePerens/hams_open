---
title: bench_audio_safe_state
summary: Sound: stop this server's playback and lower any output above its volume cap; makes no sound.
see_also: [instruments/sound/safety, tools/bench_audio_play, tools/bench_audio_levels]
tools: [bench_audio_safe_state]
instrument: sound
tier: read_config
verified: fixtures only, 2026-10-29
---

# bench_audio_safe_state

Stops any `pw-play` stream that this server started and is still running (found by its exact command line, which includes `node.dont-fallback=true node.dont-reconnect=true`, so another program's playback is not touched). Then it reads the volume of every non-reserved output and lowers any that is above its cap to the cap, reading it back. It never mutes or unmutes and never touches a reserved device. It makes no sound.

Use it first when anything went wrong with sound, for instance after the server was killed in the middle of a playback that had raised a volume.

## Arguments

<!-- arguments -->

## Returns

`verified: true`, what happened to running streams, the outputs lowered to their cap, and every output with its volume and cap.

## Failure modes

* `SAFE STATE NOT CONFIRMED`: an output would not go down to its cap, or the search for running streams failed. Lower the volume by hand and tell the owner.

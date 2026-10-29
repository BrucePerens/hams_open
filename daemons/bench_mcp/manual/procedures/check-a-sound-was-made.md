---
title: Check that a sound was made
summary: Prove with the microphone that a device under test (a relay, an alarm, a rotor) made a sound.
see_also: [tools/bench_audio_verify, tools/bench_audio_events, instruments/sound/safety, instruments/sound/discovery]
tools: [bench_audio_verify, bench_audio_events, bench_audio_status]
instrument: sound
verified: virtual sink and source and the dev box microphone, 2026-10-29
---

# Check that a sound was made

1. `bench_audio_status`: the configuration must have a `verify_actions` entry for the action that triggers the device (the name you pass as `action`). The microphone hears the room, including any person speaking; tell the person.
2. `bench_audio_verify {"action": "<name>", "delay_s": 0.5, "expect_hz": 1000}`: it listens to the noise floor first, runs the configured action, and passes only if the checks hold (level over noise, pitch, duration, beep count, reference similarity). A dead input never passes.
3. Read `pass`, the reason of every failed check and the evidence; a failure for no sound is different from a dead microphone.
4. For something that clicks or runs for a while use `bench_audio_events` for N seconds and read the event list with times and levels.

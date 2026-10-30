---
title: Sound measurement recipes
summary: Checking that a path works, that a device made a sound, and what a room sounds like, using several sound tools together.
see_also: [instruments/sound/operation, procedures/check-a-sound-was-made, instruments/sound/discovery]
tools: [bench_audio_loopback, bench_audio_verify, bench_audio_events, bench_audio_record, bench_audio_meter]
instrument: sound
verified: code only, 2026-10-29
---

# Sound measurement recipes

**Does this output reach this input?** `bench_audio_loopback {"output": ..., "input": ..., "dry_run": true}`, then without `dry_run`. Read `result.heard`, the signal-to-noise ratio and `noise_before`. A tone heard 10 dB or more over its neighbours is a working path; "not heard" with a live input means a speaker that is absent, muted or out of range of the microphone.

**Did the device under test make a sound?** Follow [check-a-sound-was-made](procedures/check-a-sound-was-made): `bench_audio_verify` with the action, `expect_hz` and `tolerance_hz`, and read the evidence (noise floor, events, spectrum peaks), not only `pass`.

**What is in the room?** `bench_audio_meter` for a live level, `bench_audio_events` for onsets (relays, beeps, a rotor), `track_hz` for a tone far under the broadband noise. Nothing is saved; `bench_audio_record` keeps a WAV in the owner-only capture directory.

**Leave it safe.** `bench_audio_safe_state` after an interrupted run.

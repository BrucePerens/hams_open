---
title: bench_audio_levels
summary: Volume, gain in dB, mute and cap of one or all devices.
see_also: [sound/overview, sound/safety]
tools: [bench_audio_levels]
instrument: sound
tier: read
verified: run on the dev box (virtual sink and source, and the speaker with the digital microphone), 2026-10-29
---

# bench_audio_levels

Volume, gain in dB, mute and cap of one or all devices.

Arguments: `device`, `direction`. Volume is in wpctl units; the signal amplitude factor is the cube.

---
title: bench_audio_devices
summary: List sound outputs and inputs with USB path, jack availability, volume and the reserved flag.
see_also: [instruments/sound/overview, instruments/sound/setup]
tools: [bench_audio_devices]
instrument: sound
tier: read_only
verified: run on the dev box, 2026-10-29
---

# bench_audio_devices

List sound outputs and inputs with USB path, jack availability, volume and the reserved flag.

Opens no audio stream. The reserved VNWA codecs are listed (devices) but never opened or queried.

## Arguments

<!-- arguments -->

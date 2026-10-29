---
title: Finding what works by listening
summary: How audio path discovery decides which outputs make sound and which inputs hear, and how auto selection uses it.
see_also: [overview, setup, safety]
tools: [bench_audio_discover, bench_audio_discovery, bench_audio_alert, bench_audio_verify]
instrument: sound
tier: full
verified: dev box 2026-10-29 10:28 local: HDMI 1 monitor speakers heard by the digital microphone and the webcam microphone; HDMI 2, HDMI 3 and Headphones not heard; analog microphone live but silent; 4.8 s of tones at volume 0.15 to 0.3, volumes restored
---

# Finding what works by listening

Run `bench_audio_discover` once on a machine, and again when hardware changes. Each output plays a short quiet tone of its own frequency while every input records. A speaker is present only if a microphone heard it; a microphone is present if it heard a tone or has the noise floor of a live capture. An output that nobody heard is reported as "no evidence of sound", not as failed. Without a working microphone the result says so and asks the operator to confirm by ear.

`bench_audio_discovery` shows the stored result, whether it is stale, and what `auto` selects now. `alert_output: auto` and `verify_input: auto` use the stored result; an alias in the config overrides it.

Not run in quiet hours unless `urgent: true`. The VNWA codecs are never used.

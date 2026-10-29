---
title: bench_audio_verify
summary: Verify that a device under control made a sound, with checks and evidence.
see_also: [instruments/sound/overview, instruments/sound/safety]
tools: [bench_audio_verify]
instrument: sound
tier: read_config
verified: virtual sink and source (beeps, wrong pitch, no sound, reference), and the dev box digital microphone for silence and events, 2026-10-29
---

# bench_audio_verify

Verify that a device under control made a sound, with checks and evidence.

Arguments: `action` (a name from the config's verify_actions), `delay_s`, `calibrate_s`, `window_s`, `min_over_noise_db`, `min_peak_dbfs`, `expect_hz`, `tolerance_hz`, `freq_min_hz`, `freq_max_hz`, `min_duration_s`, `max_duration_s`, `expect_beeps`, `min_beeps`, `expect_silence`, `reference`, `min_similarity`, `save_reference`. It hears the room. The answer is pass plus the reason of every failed check and the evidence; a dead (all-zero) input never passes.

## Arguments

<!-- arguments -->

`input` is the microphone (alias, node name or fragment; the auto-selected input when omitted). `sample_rate_hz` is the capture rate, 8000 to 96000, default 48000. `label` names the evidence in the result and in a saved reference; the default is the input's alias.

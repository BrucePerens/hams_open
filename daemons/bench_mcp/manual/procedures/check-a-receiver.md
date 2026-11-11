---
title: Check a receiver
summary: Read a rig, listen on a time station and judge the audio chain, without transmitting.
see_also: [instruments/radioctl/measurement-recipes, tools/bench_radioctl_rx_check, tools/bench_radioctl_status, instruments/sound/overview]
tools: [bench_radioctl_rigs, bench_radioctl_status, bench_radioctl_set, bench_radioctl_rx_check, bench_radioctl_stations]
instrument: radioctl
verified: virtual radioctl rig only, 2026-11-11
---

# Check a receiver

1. `bench_radioctl_rigs`: the rig must answer. If it does not, [bench_radioctl_diagnose](tools/bench_radioctl_diagnose) before anything else.
2. `bench_radioctl_status`: note the frequency, mode, AF and RF gain, AGC, attenuator and preamp. Anything listed under `unavailable` is a function the rig lacks; do not treat it as a fault.
3. `bench_radioctl_stations` with the band: pick references that fit the time of day. The table is a starting point, not a promise that a station is on the air.
4. `bench_radioctl_rx_check` at two or three of them, `seconds` 8 for a time station. A rig and an audio path that work show a signal on the S-meter and the ticks or tones in the audio at the same time; the S-meter without audio points at the audio path (check the sound input with the audio tools), audio without the S-meter at the rig\'s meter.
5. The rig\'s frequency, mode and AF level are put back by the tools; confirm with `bench_radioctl_status`.

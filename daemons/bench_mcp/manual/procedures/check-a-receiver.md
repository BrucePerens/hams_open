---
title: Check a receiver
summary: Read a rig, listen on a time station and judge the audio chain, without transmitting.
see_also: [instruments/hamlib/measurement-recipes, tools/bench_hamlib_rx_check, tools/bench_hamlib_status, instruments/sound/overview]
tools: [bench_hamlib_rigs, bench_hamlib_status, bench_hamlib_set, bench_hamlib_rx_check, bench_hamlib_stations]
instrument: hamlib
verified: dummy rig only, 2026-10-29
---

# Check a receiver

1. `bench_hamlib_rigs`: the rig must answer. If it does not, [bench_hamlib_diagnose](tools/bench_hamlib_diagnose) before anything else.
2. `bench_hamlib_status`: note the frequency, mode, AF and RF gain, AGC, attenuator and preamp. Anything listed under `unavailable` is a function the rig lacks; do not treat it as a fault.
3. `bench_hamlib_stations` with the band: pick references that fit the time of day. The table is a starting point, not a promise that a station is on the air.
4. `bench_hamlib_rx_check` at two or three of them, `seconds` 8 for a time station. A rig and an audio path that work show a signal on the S-meter and the ticks or tones in the audio at the same time; the S-meter without audio points at the audio path (check the sound input with the audio tools), audio without the S-meter at the rig\'s meter.
5. The rig\'s frequency, mode and AF level are put back by the tools; confirm with `bench_hamlib_status`.

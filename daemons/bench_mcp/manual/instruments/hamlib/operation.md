---
title: Hamlib operation
summary: The normal order of calls, concurrency, and example requests and answers.
see_also: [instruments/hamlib/overview, instruments/hamlib/troubleshooting, tools/bench_hamlib_status]
instrument: hamlib
verified: dummy rig only, 2026-10-29
---

# Hamlib operation

1. `bench_hamlib_rigs` shows the rigs and whether each answers. It starts nothing.
2. `bench_hamlib_status` reads everything the rig reports in one call; the first call starts the managed rigctld, which takes about a second.
3. `bench_hamlib_get` reads one value, or several signal-strength readings with their spread.
4. A call that fails says why and what to do. Read [troubleshooting](instruments/hamlib/troubleshooting) for the order of the steps.

Calls to one rig are serialized, so the serial port has one conversation at a time; calls to different rigs run in parallel. A rig and the sound card that carries its audio are separate instruments: see [the audio tools](instruments/sound/overview).

## Example

`bench_hamlib_status {}` on the Hamlib dummy rig returns, among other fields:

```json
{"rig": "dummy", "frequency_hz": 145000000.0, "mode": "FM", "passband_hz": 15000.0, "vfo": "VFOA",
 "split": {"enabled": false, "tx_vfo": "None"}, "power_on": true,
 "levels": {"STRENGTH": {"db_relative_to_s9": -27.0, "s_units": 4.5, "text": "S4"}},
 "unavailable": {}}
```

Signal strength is Hamlib's `STRENGTH`: dB relative to S9. S-units here follow 6 dB per unit below S9 and are a convention; most receivers' own meters are not calibrated to it. Use the values to compare readings of one rig, not as a measurement of field strength.

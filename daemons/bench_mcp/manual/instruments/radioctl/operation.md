---
title: radioctl operation
summary: The normal order of calls, concurrency, and example requests and answers.
see_also: [instruments/radioctl/overview, instruments/radioctl/troubleshooting, tools/bench_radioctl_status]
instrument: radioctl
verified: virtual radioctl rig only, 2026-11-11
---

# radioctl operation

1. `bench_radioctl_rigs` shows the rigs and whether each answers. It starts nothing.
2. `bench_radioctl_status` reads everything the rig reports in one call; the first call starts the managed radioctld, which takes about a second.
3. `bench_radioctl_get` reads one value, or several signal-strength readings with their spread.
4. A call that fails says why and what to do. Read [troubleshooting](instruments/radioctl/troubleshooting) for the order of the steps.

Calls to one rig are serialized, so the serial port has one conversation at a time; calls to different rigs run in parallel. A rig and the sound card that carries its audio are separate instruments: see [the audio tools](instruments/sound/overview).

## Example

`bench_radioctl_status {}` on a virtual Elecraft KX3 returns, among other fields:

```json
{"rig": "virtual", "frequency_hz": 14060000.0, "mode": "CW", "passband_hz": 500.0, "vfo": "VFOA",
 "split": {"enabled": false, "tx_vfo": "VFOA"}, "power_on": true,
 "levels": {"STRENGTH": {"db_relative_to_s9": -54.0, "s_units": 0.0, "text": "S0"}},
 "unavailable": {}}
```

Signal strength is radioctld's `STRENGTH` (Elecraft rigs): dB relative to S9. S-units here follow 6 dB per unit below S9 and are a convention; most receivers' own meters are not calibrated to it. Use the values to compare readings of one rig, not as a measurement of field strength.

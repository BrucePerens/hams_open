---
title: bench_radioctl_set
summary: Receiver control with read-back: frequency, mode and passband, VFO, split, RIT/XIT, levels and functions. Never keys the transmitter.
see_also: [instruments/radioctl/overview, instruments/radioctl/operation, instruments/radioctl/limits]
tools: [bench_radioctl_set]
instrument: radioctl
tier: read_config
verified: virtual radioctl rig only, 2026-11-11
---

# bench_radioctl_set

Changes a receiver's settings. Every argument is checked before the first byte is sent: the frequency against the receive range (the rig's own, narrowed by `rx_ranges_hz` in the config), the mode against the protocol's mode names, each level and function against the lists below. Each change is then read back and the read-back is returned, so a rig that quietly ignored a setting shows. The rig is left alone if it reports PTT on.

Levels that can be set: AF, RF, SQL, IF, APF, NR, NB, PBT_IN, PBT_OUT, CWPITCH, NOTCHF, BAL, AGC (0 to 6, see the status codes), ATT and PREAMP (dB). Functions: NB, NB2, NR, ANF, APF, AIP, MN, LOCK, MUTE, SQL, AFLT, ANL, FAGC. Settings that change what the transmitter does (RF power, microphone gain, keyer speed, compression, VOX, the antenna tuner, tone functions) are refused here; only the transmit tools, for a rig the owner attested into a dummy load, touch them.

## Arguments

<!-- arguments -->

`passband_hz` needs `mode` because radioctld sets them together; 0 asks for the rig's default width. Fractions (AF, RF, SQL, NR) are 0 to 1 as radioctld reports them.

## Returns

`{"changes": [{"set": "frequency_hz", "requested": 7100000, "read_back": 7100000.0}, {"set": "mode", "requested": "USB", "read_back": "USB", "passband_hz": "2400"}, ...]}`. A `warning` is added when the mode read back differs from the request.

## Failure modes

* `outside the receive range (...); source: ...`: the limit and where it comes from (the rig, or the config). Nothing was sent.
* `the rig reports PTT on`: nothing was changed.
* `<change> failed: ... (already applied before it: ...)`: the changes made before the failure stay made; read [bench_radioctl_status](tools/bench_radioctl_status).
* `RPRT -11`: the rig does not offer that setting.

## Example

`bench_radioctl_set {"frequency_hz": 7100000, "mode": "USB", "passband_hz": 2400, "levels": {"AF": 0.4}, "functions": {"NB": true}}`.

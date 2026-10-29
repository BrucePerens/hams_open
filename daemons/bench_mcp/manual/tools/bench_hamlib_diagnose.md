---
title: bench_hamlib_diagnose
summary: Connection test for a rig that does not answer: program, model, serial speed, device, DTR/RTS settings, daemon and log. Opens no serial port.
see_also: [instruments/hamlib/overview, instruments/hamlib/operation, instruments/hamlib/limits]
tools: [bench_hamlib_diagnose]
instrument: hamlib
tier: read_only
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_diagnose

Reads the configuration and the system and reports findings, each with `ok`, a detail and the next action: whether `rigctld` is installed; whether the model is known and the configured `speed` is one the backend allows; whether the device exists, who may open it and which other processes hold it open; what the config says about the DTR and RTS lines; whether the managed daemon runs and the rig answers; and the end of the daemon's log. It never opens the serial port itself and never starts the daemon, because opening a port changes DTR and RTS and some interfaces key a transmitter from those lines.

## Arguments

<!-- arguments -->

## Returns

`{"rig": ..., "checks": [{"check": "device access", "ok": true, "detail": ..., "next": ""}, ...], "failing": 0}`. `ok` is null when a check does not apply.

## Failure modes

None of its own. A finding with `ok: false` carries what to do. For the order of steps see [troubleshooting](instruments/hamlib/troubleshooting).

## Example

`bench_hamlib_diagnose {}`. If it shows `rigctld running: false`, `bench_hamlib_rigctld {"action": "start"}` starts the daemon (this opens the serial port; read the DTR/RTS finding first).

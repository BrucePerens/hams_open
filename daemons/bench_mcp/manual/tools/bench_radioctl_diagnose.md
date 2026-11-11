---
title: bench_radioctl_diagnose
summary: Connection test for a rig that does not answer: program, model, serial speed, device, DTR/RTS settings, daemon and log. Opens no serial port.
see_also: [instruments/radioctl/overview, instruments/radioctl/operation, instruments/radioctl/limits]
tools: [bench_radioctl_diagnose]
instrument: radioctl
tier: read_only
verified: virtual radioctl rig only, 2026-11-11
---

# bench_radioctl_diagnose

Reads the configuration and the system and reports findings, each with `ok`, a detail and the next action: whether `radioctld` is installed; whether the model is known and the configured `speed` is one the backend allows; whether the device exists, who may open it and which other processes hold it open; what the config says about the DTR and RTS lines; whether the managed daemon runs and the rig answers; and the end of the daemon's log. It never opens the serial port itself and never starts the daemon, because opening a port changes DTR and RTS and some interfaces key a transmitter from those lines.

## Arguments

<!-- arguments -->

## Returns

`{"rig": ..., "checks": [{"check": "device access", "ok": true, "detail": ..., "next": ""}, ...], "failing": 0}`. `ok` is null when a check does not apply.

## Failure modes

None of its own. A finding with `ok: false` carries what to do. For the order of steps see [troubleshooting](instruments/radioctl/troubleshooting).

## Example

`bench_radioctl_diagnose {}`. If it shows `radioctld running: false`, `bench_radioctl_daemon {"action": "start"}` starts the daemon (this opens the serial port; read the DTR/RTS finding first).

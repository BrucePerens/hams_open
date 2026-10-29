---
title: Hamlib tools
summary: What the bench_hamlib_* tools do, the tiers, how a rig is reached (rigctld on localhost) and what is left out.
see_also: [instruments/hamlib/setup, instruments/hamlib/operation, instruments/hamlib/limits, instruments/hamlib/troubleshooting, instruments/hamlib/protocol]
tools: [bench_hamlib_models, bench_hamlib_rigs, bench_hamlib_status, bench_hamlib_get, bench_hamlib_rigctld]
instrument: hamlib
tier: read_only
verified: dummy rig only, 2026-10-29
---

# Hamlib tools

Hamlib is a library of drivers for amateur and shortwave radios. bench_mcp does not link it (it is LGPL): it starts or finds Hamlib's `rigctld` daemon, which holds the serial port, and speaks the daemon's documented text protocol over a socket on this machine. One configured instrument is one rig; the rig's name is the instrument name.

## Tools and tiers

| Tool | Tier | Does |
|---|---|---|
| [bench_hamlib_models](tools/bench_hamlib_models) | read_only | lists Hamlib's models, and what one model can do; needs no rig |
| [bench_hamlib_rigs](tools/bench_hamlib_rigs) | read_only | the configured rigs, whether rigctld runs, whether the rig answers |
| [bench_hamlib_status](tools/bench_hamlib_status) | read_only | frequency, mode, VFO, split, RIT/XIT, signal strength, gains, AGC, attenuator, preamp, NB/NR, PTT, power state, in one call |
| [bench_hamlib_get](tools/bench_hamlib_get) | read_only | one value; repeated signal-strength readings with their spread |
| [bench_hamlib_rigctld](tools/bench_hamlib_rigctld) | read_config | status, start, stop, restart of the managed rigctld |

Later pages of this manual add receiver control, receiver tests and the transmit tools as they are merged; `bench_manual_whats_new` lists them.

## How a rig is reached

Either this server starts a `rigctld` for the rig (`model` in the config), or it connects to one that already runs (`rigctld_port`). The managed daemon listens on 127.0.0.1 only; there is no option to share it on a network. It is started by the first call that needs it, restarted if it dies (at most four starts in ten minutes without the owner asking again), and stopped when the server stops. A state file in the managed directory lets the next start stop a daemon that a crashed server left behind.

## What the driver does not do

It does not link or copy Hamlib. It has no raw-command tool. It does not set a rig up for you: the model number, the device path and the baud rate are the owner's, in the config. Rotators (`rotctld`) and `rigctld` transceive events are not covered. Every call has a bounded timeout; a rig that does not answer is an error with a next step, never a stale value.

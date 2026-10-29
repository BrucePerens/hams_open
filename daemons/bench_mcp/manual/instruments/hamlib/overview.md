---
title: Hamlib tools
summary: What the bench_hamlib_* tools do, the tiers, how a rig is reached (rigctld on localhost) and what is left out.
see_also: [instruments/hamlib/setup, instruments/hamlib/operation, instruments/hamlib/limits, instruments/hamlib/troubleshooting, instruments/hamlib/protocol]
tools: [bench_hamlib_models, bench_hamlib_rigs, bench_hamlib_status, bench_hamlib_get, bench_hamlib_diagnose, bench_hamlib_set, bench_hamlib_memory, bench_hamlib_scan, bench_hamlib_rigctld, bench_hamlib_stations, bench_hamlib_station_set, bench_hamlib_rx_check, bench_hamlib_load_check, bench_hamlib_load_baseline, bench_hamlib_watch, bench_hamlib_safe_state, bench_hamlib_tx_key_test, bench_hamlib_tx_power_sweep]
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
| [bench_hamlib_watch](tools/bench_hamlib_watch) | read_only | the dial for a few seconds, by polling: changes of frequency, mode, VFO and PTT with times |
| [bench_hamlib_diagnose](tools/bench_hamlib_diagnose) | read_only | connection test: program, model, speed, device, who holds it, DTR/RTS settings; opens no port |
| [bench_hamlib_set](tools/bench_hamlib_set) | read_config | frequency, mode and passband, VFO, split, RIT/XIT, levels and functions, each read back; never transmit-side |
| [bench_hamlib_memory](tools/bench_hamlib_memory) | read_config | recall a memory channel |
| [bench_hamlib_scan](tools/bench_hamlib_scan) | read_config | bounded scan or seek with signal strength per frequency; restores the rig |
| [bench_hamlib_rigctld](tools/bench_hamlib_rigctld) | read_config | status, start, stop, restart of the managed rigctld |
| [bench_hamlib_stations](tools/bench_hamlib_stations) | read_only | the reference station table (WWV, WWVH, AM band, weather radio, the owner's): notes, last heard, retired |
| [bench_hamlib_station_set](tools/bench_hamlib_station_set) | read_config | add, retire, restore, remove, note |
| [bench_hamlib_rx_check](tools/bench_hamlib_rx_check) | read_config | receiver test with the rig's audio: S-meter against audio level, noise floor, tone offset, station in the audio |
| [bench_hamlib_load_check](tools/bench_hamlib_load_check) | read_config | dummy-load plausibility check, receive only; only for a rig with a transmit attestation |
| [bench_hamlib_load_baseline](tools/bench_hamlib_load_baseline) | full | record the check's baseline; `confirm_dummy_load`, `dry_run`; only for a rig with a transmit attestation |
| [bench_hamlib_safe_state](tools/bench_hamlib_safe_state) | read_config | PTT off and read back, power to the minimum; works while another call runs |
| [bench_hamlib_tx_key_test](tools/bench_hamlib_tx_key_test) | full | key into the attested dummy load at a bounded power and time, meters read; watchdog, `dry_run`, `confirm_dummy_load` |
| [bench_hamlib_tx_power_sweep](tools/bench_hamlib_tx_power_sweep) | full | the same in rising steps, one keyed interval each |

The transmit tools radiate only into a dummy load that the owner has attested in the config, and the software cannot know what is connected: read [limits](instruments/hamlib/limits) and the procedure [test a transmitter into a dummy load](procedures/test-a-transmitter-into-a-dummy-load) first. Left out: rotator control (`rotctld`), an audio-through-the-rig transmit check, and automatic generator sweeps (see [measurement recipes](instruments/hamlib/measurement-recipes)).

## How a rig is reached

Either this server starts a `rigctld` for the rig (`model` in the config), or it connects to one that already runs (`rigctld_port`). The managed daemon listens on 127.0.0.1 only; there is no option to share it on a network. It is started by the first call that needs it, restarted if it dies (at most four starts in ten minutes without the owner asking again), and stopped when the server stops. A state file in the managed directory lets the next start stop a daemon that a crashed server left behind.

## What the driver does not do

It does not link or copy Hamlib. It has no raw-command tool. It does not set a rig up for you: the model number, the device path and the baud rate are the owner's, in the config. Rotators (`rotctld`) are not covered, and rig events are watched by polling (`bench_hamlib_watch`), not by rigctld's event channel. Every call has a bounded timeout; a rig that does not answer is an error with a next step, never a stale value.

---
title: radioctl tools
summary: What the bench_radioctl_* tools do, the tiers, how a rig is reached (radioctld on localhost) and what is left out.
see_also: [instruments/radioctl/setup, instruments/radioctl/operation, instruments/radioctl/limits, instruments/radioctl/troubleshooting, instruments/radioctl/protocol]
tools: [bench_radioctl_models, bench_radioctl_rigs, bench_radioctl_status, bench_radioctl_get, bench_radioctl_diagnose, bench_radioctl_set, bench_radioctl_memory, bench_radioctl_scan, bench_radioctl_daemon, bench_radioctl_stations, bench_radioctl_station_set, bench_radioctl_rx_check, bench_radioctl_load_check, bench_radioctl_load_baseline, bench_radioctl_watch, bench_radioctl_safe_state, bench_radioctl_tx_key_test, bench_radioctl_tx_power_sweep]
instrument: radioctl
tier: read_only
verified: virtual radioctl rig only, 2026-11-11
---

# radioctl tools

radioctl is this project's own library of radio drivers, described by one description file per rig (`daemons/radioctl/rigs/*.toml`). bench_mcp starts or finds its `radioctld` daemon, which holds the serial port, and speaks the daemon's text protocol (the `rigctld` line protocol) over a socket on this machine. One configured instrument is one rig; the rig's name is the instrument name.

## Tools and tiers

| Tool | Tier | Does |
|---|---|---|
| [bench_radioctl_models](tools/bench_radioctl_models) | read_only | lists radioctl's rig descriptions, and what one rig declares; needs no rig |
| [bench_radioctl_rigs](tools/bench_radioctl_rigs) | read_only | the configured rigs, whether radioctld runs, whether the rig answers |
| [bench_radioctl_status](tools/bench_radioctl_status) | read_only | frequency, mode, VFO, split, RIT/XIT, signal strength, gains, AGC, attenuator, preamp, NB/NR, PTT, power state, in one call |
| [bench_radioctl_get](tools/bench_radioctl_get) | read_only | one value; repeated signal-strength readings with their spread |
| [bench_radioctl_watch](tools/bench_radioctl_watch) | read_only | the dial for a few seconds, by polling: changes of frequency, mode, VFO and PTT with times |
| [bench_radioctl_diagnose](tools/bench_radioctl_diagnose) | read_only | connection test: program, rig description, device, who holds it, DTR/RTS note; opens no port |
| [bench_radioctl_set](tools/bench_radioctl_set) | read_config | frequency, mode and passband, VFO, split, RIT/XIT, levels and functions, each read back; never transmit-side |
| [bench_radioctl_memory](tools/bench_radioctl_memory) | read_config | recall a memory channel (radioctld serves no memory channels today: it reports that) |
| [bench_radioctl_scan](tools/bench_radioctl_scan) | read_config | bounded scan or seek with signal strength per frequency; restores the rig |
| [bench_radioctl_daemon](tools/bench_radioctl_daemon) | read_config | status, start, stop, restart of the managed radioctld |
| [bench_radioctl_stations](tools/bench_radioctl_stations) | read_only | the reference station table (WWV, WWVH, AM band, weather radio, the owner's): notes, last heard, retired |
| [bench_radioctl_station_set](tools/bench_radioctl_station_set) | read_config | add, retire, restore, remove, note |
| [bench_radioctl_rx_check](tools/bench_radioctl_rx_check) | read_config | receiver test with the rig's audio: S-meter against audio level, noise floor, tone offset, station in the audio |
| [bench_radioctl_load_check](tools/bench_radioctl_load_check) | read_config | dummy-load plausibility check, receive only; only for a rig with a transmit attestation |
| [bench_radioctl_load_baseline](tools/bench_radioctl_load_baseline) | full | record the check's baseline; `confirm_dummy_load`, `dry_run`; only for a rig with a transmit attestation |
| [bench_radioctl_safe_state](tools/bench_radioctl_safe_state) | read_config | PTT off and read back, power to the minimum; works while another call runs |
| [bench_radioctl_tx_key_test](tools/bench_radioctl_tx_key_test) | full | key into the attested dummy load at a bounded power and time, meters read; watchdog, `dry_run`, `confirm_dummy_load` |
| [bench_radioctl_tx_power_sweep](tools/bench_radioctl_tx_power_sweep) | full | the same in rising steps, one keyed interval each |

The transmit tools radiate only into a dummy load that the owner has attested in the config, and the software cannot know what is connected: read [limits](instruments/radioctl/limits) and the procedure [test a transmitter into a dummy load](procedures/test-a-transmitter-into-a-dummy-load) first. Left out: an audio-through-the-rig transmit check, and automatic generator sweeps (see [measurement recipes](instruments/radioctl/measurement-recipes)).

## How a rig is reached

Either this server starts a `radioctld` for the rig (`rig` in the config), or it connects to one that already runs (`radioctld_port`). The managed daemon listens on 127.0.0.1 only; there is no option to share it on a network. It is started by the first call that needs it, restarted if it dies (at most four starts in ten minutes without the owner asking again), and stopped when the server stops. A state file in the managed directory lets the next start stop a daemon that a crashed server left behind.

## What the driver does not do

It has no raw-command tool. It does not set a rig up for you: the rig description id, the device path and the baud rate are the owner's, in the config. Rotators have their own tools ([rotator](instruments/rotator/overview)), and rig events are watched by polling (`bench_radioctl_watch`), not by radioctld's event channel. Every call has a bounded timeout; a rig that does not answer is an error with a next step, never a stale value.

---
title: bench_radioctl_safe_state
summary: Puts the rig in its safe state: PTT off and read back, power setting to the minimum. Works while another call runs and ends a keyed interval.
see_also: [instruments/radioctl/limits, procedures/test-a-transmitter-into-a-dummy-load, instruments/radioctl/setup]
tools: [bench_radioctl_safe_state]
instrument: radioctl
tier: read_config
verified: virtual radioctl rig only, 2026-11-11
---

# bench_radioctl_safe_state

Turns PTT off, reads it back, and (unless `lower_power` is false) sets the power setting to the rig's minimum, saying what it was. It does not wait for any other call to the rig, so it works while a keyed interval is running, and ends that interval (the interval reports that the rig was un-keyed by something else). Use it first when anything went wrong. It is listed for every radioctl rig at `read_config`, whether or not the rig has a transmit attestation: moving toward the safe state is always allowed.

## Arguments

<!-- arguments -->

## Returns

`{"ptt": "receive", "verified": true, "ptt_was": "on", "power_setting_before": 0.5, "power_set_to_minimum": 0.05, "keyed_interval_was_running_here": false}`. `verified: false` means the rig accepted `set_ptt 0` but cannot report PTT, so it could not be read back: look at the rig.

## Failure modes

* `SAFE STATE NOT CONFIRMED`: the rig still reads as transmitting, or the connection failed. The message says to switch the rig off at its front panel, or pull its power or its antenna feed, and not to use it until the owner has looked. The transmit watchdog, if one is running, is also trying.

## Example

`bench_radioctl_safe_state {}`.

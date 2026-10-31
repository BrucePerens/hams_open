---
title: bench_vna_power_cycle
summary: Cold-reset the VNA by switching its hub port off and on, then verify it with a one-point sweep.
see_also: [concepts/errors, instruments/vna/monitoring-and-recovery, tools/bench_usb_port_power, instruments/vna/troubleshooting]
tools: [bench_vna_power_cycle]
instrument: [vna, usb-hubs]
tier: read_config
verified: power cycle through hub 3-2 port 2 confirmed by the person at the bench (unit dark, settings lost), 2026-10-29
---

# bench_vna_power_cycle

Switches the unit's power-switching hub port off, waits, switches it on, waits for the unit and both codecs to enumerate (about 12 s in all, plus 8 s for them to answer) and, unless `verify` is false, checks the unit with a one-point sweep at 10 MHz (RF at the unit's own level, about -17 dBm, on the TX port). The unit keeps no settings: a running generator is off afterwards.

## Arguments

<!-- arguments -->

* `instrument`: the VNA alias.
* `verify`: default `true`; `false` skips the check sweep.

## Returns

`power_cycled`, `back_after_s`, `codec_s11`, `codec_s21`, `hub_power` (the timings and device numbers of the off and on, from the same code as [bench_usb_port_power](tools/bench_usb_port_power); a unit that does not come back gets a second cycle with a longer off time), `note`, and `verified` (`freq_hz`, `s21_db`, `s21_deg`, `s11_db` of the check point).

## Failure modes

`no hub configured for this VNA` when the entry lacks `hub` and `hub_port`. A hub that does not exist fails with the `uhubctl` error. `sudo -n uhubctl` must work (see the USB hub setup page). Use it to clear the silent state or a unit that does not answer, and to stop the AM generator. `bench_vna_sweep` already does this once by itself.

## NanoVNA

With a `nanovna` instrument this tool is supported only when hub and hub_port are configured. See [the NanoVNA page](instruments/vna/nanovna).

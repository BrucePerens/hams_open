---
title: bench_glasgow_list
summary: Glasgow: the devices on the bus, their serial and revision, the USB link speed, and whether the pinned install is present.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_list]
instrument: glasgow
tier: read_only
verified: measured on the unit (Glasgow revC3, device firmware API level 3, glasgow software 70cb38c9), 2026-11-08
---

# bench_glasgow_list

Finds the Glasgow without driving anything. Reads the kernel's view of the USB device (sysfs, so it works with no install) and, when the install exists, runs `glasgow list`.

## Arguments

<!-- arguments -->

## Returns

`{"installed": true, "devices": [{"serial": "<serial>", "revision": "C3", "port_voltage_range_v": [1.65, 5.0]}], "usb": {"link_speed_mbps": 480, "high_speed": true, "bus": 1, "device": 4}, "pinned_glasgow_commit": "...", "warning": null, "log_problems": [...]}`. `warning` is set when the link is full speed. `log_problems` carries glasgow's own warnings, for example that the device firmware is older than the software (do not run `glasgow flash` from here: no tool does).

## Failure modes

* `N Glasgow devices are connected`: set `serial` in the config entry.
* `another process holds ... lock`: one session at a time; wait or end the other `glasgow` command.
* `installed: false`: run `install_glasgow.py` ([setup](instruments/glasgow/setup)). `usb` is still reported.

## Example

`bench_glasgow_list {}`

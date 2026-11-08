---
title: bench_glasgow_status
summary: Glasgow: every port's voltage, limit, measured voltage and current as the device reports them, and what this server recorded.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_status]
instrument: glasgow
tier: read_only
verified: measured on the unit (Glasgow revC3, device firmware API level 3, glasgow software 70cb38c9), 2026-11-08
---

# bench_glasgow_status

Runs `glasgow voltage` (a read) and combines it with the server's own record of what a person set. `outputs_enabled` is true when any port's regulator reads above 0.05 V, from the device, not from the record.

## Arguments

<!-- arguments -->

## Returns

`{"outputs_enabled": false, "pins": "no port is powered: every pin is a high-impedance input", "ports": [{"port": "A", "supply_set_v": 0.0, "limit_v": 5.5, "measured_v": 0.0, "current_a": 0.0, "short": false, "current_trip_a": 0.1, "fault": false}], "recorded_by_this_server": null, "limits": {...}}`. `recorded_by_this_server` lists each port set through this server with `seconds_left` before the max_on_s watchdog.

## Failure modes

* `missing permissions`: install the udev rule ([setup](instruments/glasgow/setup)).
* an unparsable table is an error, never a guess; the next step is `bench_glasgow_list`, then `bench_glasgow_safe_state`.

## Example

`bench_glasgow_status {}`

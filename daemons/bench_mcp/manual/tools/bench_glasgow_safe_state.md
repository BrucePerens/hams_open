---
title: bench_glasgow_safe_state
summary: Glasgow: all port regulators and pin drivers off, read back to prove 0 V; forgets the recorded settings.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation]
tools: [bench_glasgow_safe_state]
instrument: glasgow
tier: read_config
verified: fixtures only
---

# bench_glasgow_safe_state

`glasgow safe` (every port's voltage to 0, which turns the level shifters and so every pin driver off), then `glasgow voltage` and a check that every port reads below 0.05 V. The server's record of set ports is cleared first, so a failure cannot leave the server believing a port is still set.

## Arguments

<!-- arguments -->

## Returns

`{"outputs_enabled": false, "verified": true, "ports": [{"port": "A", "vsupply_v": 0.0, "vsense_v": 0.0}, ...]}`.

## Failure modes

* `SAFE STATE NOT CONFIRMED`: do what the message says: unplug the Glasgow's USB cable (every output goes off) and do not use it until the owner has looked at the target.

## Example

`bench_glasgow_safe_state {}`

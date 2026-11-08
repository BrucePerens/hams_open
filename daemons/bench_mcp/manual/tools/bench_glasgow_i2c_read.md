---
title: bench_glasgow_i2c_read
summary: Glasgow: read bytes from an I2C device, optionally after writing a register address. Full tier, dry_run.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_i2c_read]
instrument: glasgow
tier: full
verified: fixtures only
---

# bench_glasgow_i2c_read

Writes `register_hex` (if given), then a repeated start and a read of `count` bytes. A device that does not acknowledge is a result (`acknowledged: false` with the exception name), not a crash.

## Arguments

<!-- arguments -->

## Returns

`{"acknowledged": true, "hex": "010203", "count": 3}` or `{"acknowledged": false, "error": "I2CNotAcknowledged: ..."}`.

## Failure modes

As [bench_glasgow_i2c_scan](tools/bench_glasgow_i2c_scan).

## Example

`bench_glasgow_i2c_read {"address": 80, "register_hex": "00", "count": 16}`

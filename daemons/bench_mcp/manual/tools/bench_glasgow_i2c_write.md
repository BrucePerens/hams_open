---
title: bench_glasgow_i2c_write
summary: Glasgow: write bytes to an I2C device. Full tier, confirm, dry_run.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_i2c_write]
instrument: glasgow
tier: full
verified: fixtures only
---

# bench_glasgow_i2c_write

Sends `data_hex` to `address` (the register address first if the device has one). Writing can change the target: `confirm: true` must come from the person at the bench who knows what the bytes do.

## Arguments

<!-- arguments -->

## Returns

`{"acknowledged": true, "written": 2}`.

## Failure modes

As [bench_glasgow_i2c_scan](tools/bench_glasgow_i2c_scan); without `confirm` the call is refused before anything is sent.

## Example

`bench_glasgow_i2c_write {"address": 80, "data_hex": "00 ff", "confirm": true}`

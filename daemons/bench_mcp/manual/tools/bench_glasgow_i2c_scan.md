---
title: bench_glasgow_i2c_scan
summary: Glasgow: probe I2C addresses 0x08 to 0x77 and list those that acknowledge. Full tier, dry_run.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_i2c_scan]
instrument: glasgow
tier: full
verified: fixtures only
---

# bench_glasgow_i2c_scan

Drives SCL and SDA lines (open-drain) of the port set by `bench_glasgow_port_set`. Every address gets an empty write; the pull-ups are the target's. Pins are parked afterwards.

## Arguments

<!-- arguments -->

## Returns

`{"addresses": [32, 80], "addresses_hex": ["0x20", "0x50"], "range": ["0x08", "0x77"], "pins": ["A0", "A1"], "pins_state": "parked: ...", "outputs_enabled": true}`.

## Failure modes

* `port A has no voltage set`: call `bench_glasgow_port_set` first.
* `linked at 12 Mb/s (full speed)`: see [troubleshooting](instruments/glasgow/troubleshooting).
* An empty list means nothing acknowledged: check wiring, pull-ups and the port voltage.

## Example

`bench_glasgow_i2c_scan {"scl": "A0", "sda": "A1", "frequency_khz": 100}`

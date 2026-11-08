---
title: bench_glasgow_swd_idcode
summary: Glasgow: ARM SWD line reset and switch sequences, then read DPIDR. Full tier, dry_run.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_swd_idcode]
instrument: glasgow
tier: full
verified: fixtures only
---

# bench_glasgow_swd_idcode

Initialises the debug port and reads only DPIDR. It never reads memory or changes the target's debug state beyond the initialisation the protocol needs.

## Arguments

<!-- arguments -->

## Returns

`{"dpidr": "0x2ba01477", "version": 2, "min": false, "designer": 571, "partno": 186, "revision": 1}`.

## Failure modes

As [bench_glasgow_i2c_scan](tools/bench_glasgow_i2c_scan).

## Example

`bench_glasgow_swd_idcode {"swclk": "A0", "swdio": "A1"}`

---
title: bench_glasgow_jtag_scan
summary: Glasgow: scan a JTAG chain and report each TAP's IR length and IDCODE fields. Full tier, dry_run.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_jtag_scan]
instrument: glasgow
tier: full
verified: fixtures only
---

# bench_glasgow_jtag_scan

Runs `jtag-probe scan` (never the dangerous `enumerate-ir`). The IDCODE is split into version, part number and manufacturer id.

## Arguments

<!-- arguments -->

## Returns

`{"taps": [{"tap": 0, "ir": "5", "idcode": "0x4ba00477", "manufacturer_id": 571, "part": 47616, "version": 4, "bypass": false}], "tap_count": 1}`.

## Failure modes

As [bench_glasgow_i2c_scan](tools/bench_glasgow_i2c_scan). Zero TAPs means nothing answered on TDO.

## Example

`bench_glasgow_jtag_scan {"frequency_khz": 100}`

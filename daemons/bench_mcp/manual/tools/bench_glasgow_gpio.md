---
title: bench_glasgow_gpio
summary: Glasgow: read pins and drive or pull them for a bounded hold, then release everything. Full tier, confirm to drive, dry_run.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_gpio]
instrument: glasgow
tier: full
verified: fixtures only
---

# bench_glasgow_gpio

Actions apply in order: `A0=1`/`A0=0` strong drive, `A1=H`/`A1=L` weak pull, `A2=Z` release, `A3` read. Strong levels are held `hold_s` (0 to 30), then all pins are parked. Uses register transfers only, so it also works on a full-speed link.

## Arguments

<!-- arguments -->

## Returns

`{"reads": {"A3": true}, "held_s": 0.2, "released": true, "pins_state": "parked: ..."}`.

## Failure modes

As [bench_glasgow_i2c_scan](tools/bench_glasgow_i2c_scan).

## Example

`bench_glasgow_gpio {"pins": "A0:3", "actions": ["A0=1", "A3"], "hold_s": 0.5, "confirm": true}`

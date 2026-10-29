---
title: bench_measure
summary: Scope parameter measurements on a channel, or the supply's measured volts, amps and watts.
see_also: [tools/bench_capture, tools/bench_scope_spectrum, instruments/scope]
tools: [bench_measure]
instrument: [scope, psu]
tier: read_only
verified: code only, 2026-10-29
---

# bench_measure

The scope's own automatic measurements (the same ones its front panel shows) on one channel, or the supply's measured output.

## Arguments

<!-- arguments -->

* `instrument`: the alias. With both a scope and a supply configured it must be given.
* `channel`: scope only, `1` or `2`, and mandatory for the scope (`channel must be 1 or 2` otherwise). Ignored for the supply.
* `items`: scope only, a list of parameter names from the enumeration; default `PKPK`, `MEAN`, `FREQ`, `DUTY`. Voltages are in volts, `FREQ` and counters in hertz, `PER`, `RISE`, `FALL`, `WID`, `PWID`, `NWID` in seconds, `DUTY` and `NDUTY` in percent, `OVSN`, `OVSP` in percent of amplitude.

## Returns

Scope: an object with one entry per item. The value `****` means the scope could not measure it (no edges, signal off screen); treat it as missing, never as zero. Supply: `voltage_v`, `current_a`, `power_w` and an `info` string from the supply.

## Failure modes and limits

A measurement is only as good as the display: set the vertical scale so the signal fills a few divisions and the timebase so a few periods show (`bench_set`). The scope's frequency counter is wrong above about 150 MHz; use [bench_scope_spectrum](tools/bench_scope_spectrum) for a tone's frequency. `PAVA? ALL` and the scope's statistics query do not answer on the tested firmware, so there is no all-at-once mode.

## Example

`bench_measure {"instrument": "scope", "channel": 1, "items": ["PKPK", "FREQ"]}` returns `{"PKPK": <volts>, "FREQ": <hertz>}`.

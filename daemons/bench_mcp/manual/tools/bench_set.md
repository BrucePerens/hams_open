---
title: bench_set
summary: Change scope settings (timebase, vertical, coupling, probe, trigger) or supply setpoints within the configured caps.
see_also: [tools/bench_status, tools/bench_psu_output, concepts/safety-tiers, instruments/psu]
tools: [bench_set]
instrument: [scope, psu]
tier: read_config
verified: code only, 2026-10-29
---

# bench_set

One tool for the settings of the scope and the setpoints of the supply. Several settings can be given in one call; they are sent in the order timebase, vertical (volts per division, offset, coupling, probe), trigger mode, trigger type, and the reply reads the state back.

## Arguments

<!-- arguments -->

Scope:

* `instrument`: the alias (needed when both a scope and a supply are configured).
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).
* `timebase_s`: seconds per division (for example `1e-6`).
* `channel`: which input `vdiv_v`, `offset_v`, `coupling`, `attenuation` and `trigger_type` apply to; `1` or `2`.
* `vdiv_v`: volts per division at the scope input (after the probe ratio is applied by the scope).
* `offset_v`: vertical offset in volts.
* `coupling`: `A1M` (AC, 1 MOhm), `D1M` (DC, 1 MOhm) or `GND`.
* `attenuation`: the probe ratio (1, 10, ...); it must match the probe, or all voltages are wrong by that factor.
* `trigger_mode`: `AUTO`, `NORM`, `SINGLE` or `STOP`. `STOP` freezes the memory; `bench_capture` refuses a stopped scope.
* `trigger_type`: `EDGE`, `SLEW`, `GLIT`, `INTV`, `RUNT` or `TV` on `channel`; the type's other parameters keep their values. Window, pattern, drop-out and serial-bus triggers were not accepted by the tested unit.

Supply:

* `voltage_v`: the voltage setpoint in volts, from 0 to the configured `max_volts` (default 5.5).
* `current_a`: the current limit setpoint in amperes, from 0 to the configured `max_amps` (default 1.0).

## Returns

`sent` (the commands sent) and the new state (`status` for the scope, `state` for the supply, as in [bench_status](tools/bench_status)).

## Failure modes

A value outside the caps or not finite is refused before anything is sent (`voltage_v 6 is outside 0..=5.5 (the cap in the server's config)`). With the supply, give at least one of `voltage_v` and `current_a`. This tool never changes the supply's output state: use [bench_psu_output](tools/bench_psu_output).

## Examples

`bench_set {"instrument": "scope", "channel": 1, "vdiv_v": 0.1, "timebase_s": 1e-6, "trigger_mode": "AUTO"}`

`bench_set {"instrument": "psu", "voltage_v": 3.3, "current_a": 0.2}`

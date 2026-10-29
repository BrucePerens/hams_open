---
title: bench_psu_output
summary: Switch the bench supply output on (full tier, within the caps) or off.
see_also: [tools/bench_set, tools/bench_status, concepts/safety-tiers, concepts/etiquette]
tools: [bench_psu_output]
instrument: psu
tier: read_config
verified: code only, 2026-10-29
---

# bench_psu_output

Switches the output of the bench supply. Off is allowed at `read_config` and is always safe. On needs the instrument's tier to be `full`.

## Arguments

<!-- arguments -->

* `on`: `true` to switch the output on, `false` to switch it off. Required.
* `instrument`: the supply alias; optional when only one supply is configured.

## Returns

The supply state after a 0.5 s wait, as in [bench_status](tools/bench_status).

## Failure modes

* `switching the output on needs tier Full; "psu" is configured at ReadConfig`: the tier decision belongs to the bench owner.
* `refused: the supply is set to V V / A A, above the caps`: the setpoints stored in the supply (a front-panel value may have been left there; one unit came up set to 12 V and 7.8 A) exceed `max_volts` or `max_amps`. Lower them with [bench_set](tools/bench_set) first, then retry.

## Procedure for a load

1. `bench_status` and check the setpoints and that `output` is `off`.
2. `bench_set` the voltage and the current limit for the load.
3. Confirm with the person that the load is connected (see [etiquette](concepts/etiquette)).
4. `bench_psu_output {"on": true}`, then `bench_status` to read `measured_voltage_v` and `measured_current_a`.
5. When finished, `bench_psu_output {"on": false}`.

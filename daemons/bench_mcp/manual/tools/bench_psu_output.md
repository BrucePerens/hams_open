---
title: bench_psu_output
summary: Switch the bench supply output on, within the caps and for a limited time, with a dry run and a confirmation; off is bench_psu_safe_state.
see_also: [tools/bench_psu_safe_state, tools/bench_set, tools/bench_status, concepts/safety-tiers, concepts/etiquette]
tools: [bench_psu_output]
instrument: psu
tier: full
verified: code only, 2026-10-29
---

# bench_psu_output

Switches the output of the bench supply and drives whatever is connected to it. The tool is `full` tier. The driver knows of no output timer in the supply (none is used), so this server switches the output off again after `for_s` seconds and the config caps that time with `max_on_s` (default 600).

## Arguments

<!-- arguments -->

* `on`: `true` to switch the output on, `false` to switch it off and read it back (the same as [bench_psu_safe_state](tools/bench_psu_safe_state)). Required.
* `confirm_load_connected`: must be `true` for `on: true`, and that must come from the person at the bench who has checked the load.
* `for_s`: seconds until the server switches the output off, 1 to `max_on_s`; default `max_on_s`.
* `dry_run`: reads the setpoints stored in the supply and returns the plan, including whether the call would be refused. Nothing is switched.
* `instrument`: the supply alias; optional when only one supply is configured.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

## Returns

The supply state after a 0.5 s wait, as in [bench_status](tools/bench_status), plus `off_after_s`. For `on: false`, `{"output": "off", "verified": true}`.

## Failure modes

* `bench_psu_output needs tier Full; "psu" is configured at ReadConfig`: the tier decision belongs to the bench owner. Switching off is still possible with [bench_psu_safe_state](tools/bench_psu_safe_state).
* `confirm_load_connected: true` missing: ask the person at the bench; do not pass it on your own judgement.
* `refused: the supply is set to V V / A A, above the caps`: the setpoints stored in the supply (a front-panel value may have been left there; one unit came up set to 12 V and 7.8 A) exceed `max_volts` or `max_amps`. Lower them with [bench_set](tools/bench_set) first, then retry.
* `SAFE STATE NOT CONFIRMED`: the supply would not say off. Press its output button or switch off its mains and tell the owner.

## Procedure for a load

1. `bench_status` and check the setpoints and that `output` is `off`.
2. `bench_set` the voltage and the current limit for the load.
3. `bench_psu_output {"on": true, "dry_run": true}` and read the plan.
4. Confirm with the person that the load is connected (see [etiquette](concepts/etiquette)).
5. `bench_psu_output {"on": true, "confirm_load_connected": true, "for_s": 120}`, then `bench_status` to read `measured_voltage_v` and `measured_current_a`.
6. When finished, `bench_psu_safe_state`.

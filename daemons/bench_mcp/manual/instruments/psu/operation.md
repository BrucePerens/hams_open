---
title: Supply operation
summary: Setting, switching on, reading back and switching off the supply.
see_also: [tools/bench_psu_output, tools/bench_set, concepts/etiquette]
tools: [bench_status, bench_measure, bench_set, bench_psu_output]
instrument: psu
verified: code only, 2026-10-29
---

# Supply operation

1. `bench_status`: `output` must be `off` and the setpoints known.
2. `bench_set {"voltage_v": 3.3, "current_a": 0.2}`: values above the caps are refused before anything is sent. The output state does not change.
3. Ask the person to confirm the load is connected.
4. `bench_psu_output {"on": true}` (tier `full`). Refused if the stored setpoints exceed the caps.
5. `bench_status` or `bench_measure`: `measured_voltage_v`, `measured_current_a`, `power_w`. If the current sits at the limit the supply is in constant-current mode and the voltage will be lower than the setpoint.
6. Finish with `bench_psu_output {"on": false}` (allowed at `read_config`) and verify `output: off`.

`bench_measure` also returns the supply's `MEAS:ALL:INFO?` text unparsed.

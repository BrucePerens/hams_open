---
title: bench_vna_generator_off
summary: Switch the VNA generator's TX output off; if the unit is modulating it is power cycled through its hub.
see_also: [tools/bench_vna_generator_on, tools/bench_vna_generator_am, tools/bench_vna_generator_fm, tools/bench_vna_power_cycle, instruments/vna/modulation]
tools: [bench_vna_generator_off]
instrument: vna
tier: read_config
verified: 2026-10-29 on the unit, after AM and FM (the switch-off request stalled in every case; the power cycle brought the unit back)
---

# bench_vna_generator_off

Switches the TX output off. Safe at any time, including when nothing is on.

If the generator was started with [bench_vna_generator_am](tools/bench_vna_generator_am) or [bench_vna_generator_fm](tools/bench_vna_generator_fm), the unit refuses the request (it stalls it) and no other request ends the modulation. Then, when the instrument has a hub configured, the tool cold resets the unit through the hub with the longer ladder for modulation lock-ups (3, 12, 12, 30 and 12 seconds off; one cycle was enough 14 times in 25, up to four were needed twice) and checks that it answers again. This takes 15 to 90 seconds and the unit loses all its settings. Without a hub the tool fails and says to power the unit's USB port off and on yourself.

## Arguments

<!-- arguments -->

* `instrument`: the VNA alias.

## Returns

`{"generator": "off"}`, or with `recovered_by_power_cycle: true` when the unit had to be power cycled.

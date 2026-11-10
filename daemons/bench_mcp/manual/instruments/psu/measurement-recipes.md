---
title: Supply measurement recipes
summary: Ripple with the scope and load regulation with the supply's own readback, as procedures with the expected shape of the result.
see_also: [instruments/psu/operation, instruments/psu/limits, tools/bench_psu_output, tools/bench_measure, instruments/scope/measurement-recipes]
instrument: psu
verified: n/a (procedures; no measurement has been made on the bench yet)
---

# Supply measurement recipes

Both recipes switch the output on, so they need tier `full`, `confirm_load_connected: true` from the person at the bench, and a load that is safe at the voltage and current chosen. Nothing here has been run on the bench: the numbers to expect are stated as shapes, not as results.

## Ripple on the output

1. Connect a resistive load that draws a known current well under the supply's limit, and the scope probe on the output with the ground spring on the output's ground (no long ground lead).
2. `bench_set {"instrument": "psu", "voltage_v": 5.0, "current_a": 0.5}`, then `bench_psu_output {"instrument": "psu", "on": true, "for_s": 60, "confirm_load_connected": true}`.
3. `bench_set {"instrument": "scope", "channel": 1, "coupling": "A1M", "attenuation": 1, "vdiv_v": 0.005, "timebase_s": 0.001, "trigger_mode": "AUTO"}`. AC coupling removes the DC level so the ripple fills the screen.
4. `bench_measure {"instrument": "scope", "channel": 1, "items": ["PKPK", "RMS", "FREQ"], "statistics": 20}` for the peak-to-peak ripple with its spread.
5. `bench_scope_spectrum {"channel": 1}` shows the ripple and switching lines; a linear supply shows 100 or 120 Hz, a switching one a line at its switching frequency.
6. `bench_psu_safe_state {"instrument": "psu"}`.

Read it as: peak-to-peak well under the load's tolerance, a stable `FREQ`. A `PKPK` that jumps from reading to reading (large `std_dev`) is noise picked up by the ground lead, not ripple.

## Load regulation

1. Output on at 5 V into the first load; record `bench_measure {"instrument": "psu"}` (`voltage_v`, `current_a`).
2. Change to a second, heavier load (still inside the current limit) with the output on, and measure again.
3. Regulation is the voltage change divided by the nominal voltage. Use the supply's own readback for the voltage only to compare the two readings with each other; for an absolute figure measure the output with the scope or a meter, since the readback has its own error.
4. `bench_psu_safe_state` when done.

## Not covered

Transient response (load step) needs a switched load and the scope in single-shot mode and has not been tried. Verified status of both recipes: none.

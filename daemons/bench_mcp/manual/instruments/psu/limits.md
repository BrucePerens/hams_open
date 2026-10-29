---
title: Supply limits
summary: Ranges, caps, the output state spelling and what is not verified.
see_also: [instruments/psu/operation]
instrument: psu
verified: code only, 2026-10-29
---

# Supply limits

* The output stays on at most `max_on_s` seconds per switch-on (default 600, config key on the instrument): the supply has no output timer of its own, so this server holds the limit. If the server process dies while the output is on, the output stays on; use `bench_psu_safe_state` or the front panel.
* Hardware range 0 to 60 V, 0 to 10 A; the server's caps (defaults 5.5 V, 1.0 A) apply to every setpoint and are the effective range.
* Setpoints are sent with three decimals.
* The supply answers `OUTP?` with `ON`/`OFF` (its manual says 0/1); both are accepted.
* Over-voltage and over-current protection values are read (`ovp_v`, `ocp_a`) but not set by the tools.
* Measured values are the supply's own readback, not an independent measurement; use the scope for ripple.
* Not verified: behaviour at the top of its range; the tools were exercised at low voltage only.

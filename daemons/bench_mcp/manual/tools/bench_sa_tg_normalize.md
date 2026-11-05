---
title: bench_sa_tg_normalize
summary: SSA3000X: sweep the through (TG OUTPUT cabled to RF INPUT) with the generator on and store it as the 0 dB reference for bench_sa_tg_s21.
see_also: [instruments/sa/overview, instruments/sa/operation, instruments/sa/limits]
tools: [bench_sa_tg_normalize]
instrument: sa
tier: read_config
verified: fixtures only
---

# bench_sa_tg_normalize


With the TG OUTPUT cabled straight to the RF INPUT, switches the generator on at `tg_level_dbm` (default -20), takes one sweep with the sweep arguments given, and stores the trace under `name` (default `default`) in this server. It also writes `sa_tg_reference_<name>_*.csv` with the settings as `# key=value` lines, which [bench_sa_tg_s21](tools/bench_sa_tg_s21) accepts as `reference_file` after a restart. The generator is switched off afterwards and verified, unless `leave_tg_on` is true. It outputs RF on the TG OUTPUT connector at the level set (0 dBm at most). The unit's own Normalize function is not used.

## Arguments

<!-- arguments -->

## Returns

The sweep summary (see [bench_sa_sweep](tools/bench_sa_sweep)), `name`, `tg_level_dbm`, `file` and `tracking_generator`.

## Failure modes

* A bad name (1 to 40 letters, digits, `-`, `_`), level or sweep argument: refused. A failed sweep still switches the generator off; `SAFE STATE NOT CONFIRMED` means do it by hand.

Example: `bench_sa_tg_normalize {"start_hz": 1000000, "stop_hz": 101000000, "rbw_hz": 10000, "name": "thru"}`.

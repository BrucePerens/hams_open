---
title: bench_sa_tg_level
summary: SSA3000X tracking generator: set the level in dBm without changing whether the output is on, and read it back.
see_also: [instruments/sa/overview, instruments/sa/operation, instruments/sa/limits]
tools: [bench_sa_tg_level]
instrument: sa
tier: read_config
verified: fixtures only
---

# bench_sa_tg_level


Sets the generator level (`tg_level_dbm`, -20 to 0 dBm, at most the config's `max_tg_dbm`) and reads it back; whether the output is on or off is not changed. It outputs RF on the TG OUTPUT connector at the level set (0 dBm at most). If the output is on, the new level is on the connector at once.

## Arguments

<!-- arguments -->

## Returns

`tracking_generator` (`on` or `off`) and `level_dbm`.

## Failure modes

* A missing or out-of-range level: refused, nothing sent. `reads back`: the unit did not keep the level.

Example: `bench_sa_tg_level {"tg_level_dbm": -15}`.

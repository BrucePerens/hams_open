---
title: bench_sa_tg_on
summary: SSA3000X tracking generator: output on at a level, set and verified before it is switched on. Stays on until switched off.
see_also: [instruments/sa/overview, instruments/sa/operation, instruments/sa/limits]
tools: [bench_sa_tg_on]
instrument: sa
tier: read_config
verified: fixtures only
---

# bench_sa_tg_on


Switches the tracking generator output on. It outputs RF on the TG OUTPUT connector at the level set (0 dBm at most). With `tg_level_dbm` (-20 to 0 dBm, and at most the config's `max_tg_dbm`) the level is set and read back before the output goes on; without it the level already set is used, and refused if it is above `max_tg_dbm`. The output is read back, and if anything fails it is switched off again. It stays on until [bench_sa_tg_off](tools/bench_sa_tg_off), or a tool with `tg_off_after`.

## Arguments

<!-- arguments -->

## Returns

`tracking_generator` (`on`) and `level_dbm` as the unit reports them.

## Failure modes

* A level outside the range or above the cap: refused, nothing sent. No TG option listed: refused.
* `reads back`: the unit did not keep the level; the output was not left on.

Example: `bench_sa_tg_on {"tg_level_dbm": -10}`.

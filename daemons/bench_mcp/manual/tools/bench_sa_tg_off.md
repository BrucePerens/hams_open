---
title: bench_sa_tg_off
summary: SSA3000X tracking generator: output off, read back to prove it, tried up to 3 times. Safe at any time.
see_also: [instruments/sa/overview, instruments/sa/operation, instruments/sa/limits]
tools: [bench_sa_tg_off]
instrument: sa
tier: read_config
verified: measured on the unit (Siglent SSA3021X, firmware 1.3.9.10), 2026-11-05
---

# bench_sa_tg_off


Switches the tracking generator output off and reads it back, up to three times. Use it first when anything went wrong. If the unit will not confirm off, the error says `SAFE STATE NOT CONFIRMED` and what to do by hand (press TG then TG off on the front panel, or disconnect what is on TG OUTPUT). Checked on the unit with the generator already off.

## Arguments

<!-- arguments -->

## Returns

`tracking_generator` (`off`), `verified` (true) and `level_dbm`.

## Failure modes

* `SAFE STATE NOT CONFIRMED`: the output may still be on; do as the message says.

Example: `bench_sa_tg_off {}`.

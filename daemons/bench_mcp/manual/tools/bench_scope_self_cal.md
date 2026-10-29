---
title: bench_scope_self_cal
summary: Run the scope's self-calibration; probes must be disconnected and the person at the bench must have confirmed.
see_also: [tools/bench_set, concepts/safety-tiers, concepts/etiquette]
tools: [bench_scope_self_cal]
instrument: scope
tier: full
verified: code only, 2026-10-29
---

# bench_scope_self_cal

Runs the scope's `*CAL?` self-calibration. It rewrites the stored calibration, takes a minute or two and drives its own levels onto the inputs.

## Arguments

<!-- arguments -->

* `confirm_inputs_disconnected`: must be `true`, and it must be true because the person at the bench has unplugged every probe and cable. Without it the call is refused. Never set it on your own judgement.
* `dry_run`: returns the plan and sends nothing; allowed without the confirmation.
* `instrument`: the scope alias.

## Returns

`cal_reply` (`0` means the scope accepted the command) and a `note`. The calibration may keep running after the reply; wait a couple of minutes before measuring. The vertical settings can be reset by the calibration (one self-cal changed 1 V/div to 20 mV/div): read `bench_status` and restore them.

## Failure modes

`refused: confirm_inputs_disconnected must be true, ...`; the scope does not answer within 300 s. Needs tier `full`. There is no safe-state tool for it: the scope runs the calibration to its end and cannot abort it, and nothing is left driven afterwards.

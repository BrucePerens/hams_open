---
title: bench_psu_safe_state
summary: Supply: output off, read back to prove it; says what to do by hand if it cannot.
see_also: [tools/bench_psu_output, instruments/psu/troubleshooting, concepts/safety-tiers]
tools: [bench_psu_safe_state]
instrument: psu
tier: read_config
verified: code only, 2026-10-29
---

# bench_psu_safe_state

Sends `OUTP OFF` and reads `OUTP?` back, up to three times. It succeeds only when the supply itself says off. It also cancels a pending timed switch-off from [bench_psu_output](tools/bench_psu_output), which has nothing left to do. It is allowed one tier below the tool that switches on, because it can only reduce risk.

## Arguments

<!-- arguments -->

## Returns

`{"output": "off", "verified": true}`.

## Failure modes

* `SAFE STATE NOT CONFIRMED`: the output may be on. Press the output button on the supply or switch off its mains, then tell the owner.

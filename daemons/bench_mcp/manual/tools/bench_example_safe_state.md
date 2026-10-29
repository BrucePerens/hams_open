---
title: bench_example_safe_state
summary: Example instrument: output off, read back to prove it; says what to do by hand if it cannot.
see_also: [instruments/example/troubleshooting, tools/bench_example_pulse]
tools: [bench_example_safe_state]
instrument: example
tier: read_config
verified: fixtures only
---

# bench_example_safe_state

Sends `OUTP OFF` and reads `OUTP?` back, up to three times. It succeeds only when the instrument itself says off. Every instrument that can actuate or radiate has a tool like this; it is allowed one tier below the tool that switches on, because it can only reduce risk.

## Arguments

<!-- arguments -->

## Returns

`{"output": "off", "verified": true}`.

## Failure modes

* `SAFE STATE NOT CONFIRMED`: the output may be on. Switch it off at the front panel or pull its power, then tell the owner.

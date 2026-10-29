---
title: Example instrument operation
summary: The normal order of calls for the example instrument. The model for an operation page.
see_also: [instruments/example/setup]
tools: [bench_example_status, bench_example_measure, bench_example_pulse, bench_example_safe_state]
instrument: example
verified: fixtures only
---

# Example instrument operation

1. `bench_example_status`: confirms the address is an EXAMPLE-1000, reports the output state and level. If the output is on and you did not switch it on, call `bench_example_safe_state` and ask the owner.
2. `bench_example_measure` with the meter input connected: read the voltage before driving anything.
3. `bench_example_pulse` with `dry_run: true`: read the plan. With the owner's agreement, repeat with `confirm: true`.
4. `bench_example_measure` during the pulse is not possible (a call holds the instrument until it returns); measure before and after, or use the scope.

An operation page also states concurrency: calls to one instrument are serialized by the server; other instruments stay usable meanwhile.

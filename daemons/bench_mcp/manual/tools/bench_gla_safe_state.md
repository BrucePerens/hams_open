---
title: bench_gla_safe_state
summary: Logic analyzer and pattern generator: the Glasgow's safe state (every port off, every pin an input), the same as bench_glasgow_safe_state.
see_also: [tools/bench_glasgow_safe_state, tools/bench_gla_pattern]
tools: [bench_gla_safe_state]
instrument: glasgow
tier: read_config
verified: fixtures only
---

# bench_gla_safe_state

Runs [bench_glasgow_safe_state](tools/bench_glasgow_safe_state) and returns its answer: every port's regulator and every pin driver off, read back at 0 V, the recorded voltages forgotten. It is the companion of [bench_gla_pattern](tools/bench_gla_pattern). Use it first when anything went wrong; if it cannot confirm, the answer says to unplug the USB cable.

## Arguments

<!-- arguments -->

## Returns

As bench_glasgow_safe_state: `{"outputs_enabled": false, "verified": true, "ports": [...]}`.

## Failure modes

`SAFE STATE NOT CONFIRMED` with what to do by hand.

## Example

`bench_gla_safe_state {}`

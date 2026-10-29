---
title: bench_example_status
summary: Example instrument: identity, output state and level; refuses an address that is not an EXAMPLE-1000.
see_also: [instruments/example/overview, instruments/example/operation]
tools: [bench_example_status]
instrument: example
tier: read_only
verified: fixtures only
---

# bench_example_status

Reads the identity (`*IDN?`), whether the output is on, and the level. Changes nothing. A tool page states the purpose, the arguments, what is returned (with units), side effects, failure modes with the next action, and a real example.

## Arguments

<!-- arguments -->

## Returns

`make`, `model`, `serial`, `firmware`, `output` (`on` or `off`), `level_v` (volts) and `limits` (the config's `max_level_v` and `max_pulse_s`).

## Failure modes

* `no usable reply after 3 tries`: see [troubleshooting](instruments/example/troubleshooting).
* `answered *IDN? with ..., not an EXAMPLE-1000`: wrong address; nothing else was sent.

## Example

`bench_example_status {}` returns `{"model": "EXAMPLE-1000", "output": "off", "level_v": 0.25, ...}`.

---
title: bench_example_measure
summary: Example instrument: the meter voltage, mean and spread of 1 to 10 readings.
see_also: [instruments/example/overview, tools/bench_example_status]
tools: [bench_example_measure]
instrument: example
tier: read_only
verified: fixtures only
---

# bench_example_measure

Reads the meter input `count` times (`count` 1 to 10, default 1) and returns the mean and the spread (highest minus lowest). A reading that is not a finite number is an error and no value is returned.

## Arguments

<!-- arguments -->

## Returns

`voltage_v` (volts, the mean), `readings` (how many) and `spread_v` (volts). The tool does not judge whether the value is good; compare it with what you expect.

## Failure modes

* `is not a finite number`: the input is open or the instrument is faulted; see [troubleshooting](instruments/example/troubleshooting).

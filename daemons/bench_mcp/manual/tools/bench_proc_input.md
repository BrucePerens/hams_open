---
title: bench_proc_input
summary: Answer the operator prompt a run is waiting on; the run continues.
see_also: [concepts/procedures, tools/bench_proc_status, tools/bench_proc_run]
tools: [bench_proc_input]
tier: read_config
verified: fixtures only, 2026-10-29
---

# bench_proc_input

A procedure can pause for a step only a person can do (`bench.ask_operator("Connect the dummy load, then answer done.")`). The run's state becomes `waiting_operator` and `bench_proc_status` shows the message in `awaiting`. Ask the person at the bench, then answer with what they did or said. Do not answer on your own judgement: the prompt exists because a person must do something.

## Arguments

<!-- arguments -->

## Returns

`{"run", "answered": true}`. The program receives `answer` as the value of `bench.ask_operator`. The time spent waiting does not count against the run's wall-clock budget, and has its own limit (the entry's `operator_timeout_s`, default 3600 s, or the `timeout` the program passed): past it the run is stopped as `timed_out` with the reason `operator_timeout`.

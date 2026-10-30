---
title: bench_proc_status
summary: The state, elapsed time, step, last tool call and counters of a procedure run.
see_also: [concepts/procedures, tools/bench_proc_run, tools/bench_proc_output, tools/bench_proc_result]
tools: [bench_proc_status]
tier: read_only
verified: fixtures only, 2026-10-29
---

# bench_proc_status

The record of one run, read from its directory, so it answers for a running run, a finished one and one of an earlier server process.

## Arguments

<!-- arguments -->

## Returns

`state` (`starting`, `running`, `cancelling`, `succeeded`, `failed`, `cancelled`, `timed_out`), `finished`, `elapsed_s`, `step` (the last `bench.step` name), `line` (with `trace`), `last_call` (tool, arguments, result or error, milliseconds), `calls` and `call_errors`, `hook_violations`, `wall_s`, `max_calls`, `tier_cap`, `label`, `pid`, `note`, `cancel_reason`, `events` (how many events exist, the top of the cursor range) and `events_dropped`.

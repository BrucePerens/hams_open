---
title: bench_proc_output
summary: Progress events after a cursor, stdout and stderr after byte offsets, and the latest emitted values of a run.
see_also: [concepts/procedures, tools/bench_proc_status, tools/bench_proc_result]
tools: [bench_proc_output]
tier: read_only
verified: fixtures only, 2026-10-29
---

# bench_proc_output

Poll with the cursor and offsets from the previous answer and you get only what is new. Each part is bounded by `max_bytes`, so a chatty run cannot flood your context: when `more_events` or `more` is true, call again.

## Arguments

<!-- arguments -->

## Returns

`events`: objects with `seq`, `t` (seconds since the start) and `kind`: `state`, `step`, `call` (tool, arguments, result or error, `ms`), `emit` (key, value), `log`, `checkpoint`, `violation` (an audit-hook refusal), `trace`. `cursor` is the last `seq` returned. `stdout` and `stderr` hold `text`, `next` (the offset to pass back) and `more`. `emitted` is the latest value of every key. The event list is capped at 8 MiB and each output stream at 1 MiB; past the cap the rest is dropped and said so.

---
title: bench_proc_result
summary: The final value of a finished procedure run, or its structured failure report with the locals of its frames.
see_also: [concepts/procedures, tools/bench_proc_status, tools/bench_proc_output]
tools: [bench_proc_result]
tier: read_only
verified: fixtures only, 2026-10-29
---

# bench_proc_result

## Arguments

<!-- arguments -->

## Returns

`state`, `ok`, `finished`, `value` (what the program set with `bench.result`), `emitted` (the latest value per key, also for a failed or killed run), `calls`, `call_errors`, `elapsed_s`, `hook_violations`, `stderr_tail`, and `failure` (null on success).

`failure.kind` is `exception` (an error in the program), `cancelled`, `timed_out` or `crash` (the process died without reporting: `message` has the exit status and `stderr` its last output). For an exception: `type`, `message`, `traceback`, `frames` (the program's own frames, each with `function`, `line` and `locals` as bounded reprs), `context` (from `bench.expect`), `tool` and `kind` for a failed tool call. Always: `last_calls` (the last ten tool calls with arguments and results) and `emitted`. For a run still going the answer says so and returns what has been emitted.

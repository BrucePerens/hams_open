---
title: bench_proc_run
summary: Start a Python procedure that calls bench tools in loops; returns a run id at once. Tier cap, budgets, dry run.
see_also: [concepts/procedures, tools/bench_proc_status, tools/bench_proc_output, tools/bench_proc_result, tools/bench_proc_cancel, concepts/safety-tiers]
tools: [bench_proc_run]
tier: read_config
verified: fixtures only, 2026-10-29
---

# bench_proc_run

Starts a program (Python 3 with numpy and scipy, the `bench` module injected; see [the procedure runner](concepts/procedures)) in a process of its own and returns a run id at once. The tool call returns after the process has started, not after it ended: poll with [bench_proc_status](tools/bench_proc_status).

## Arguments

<!-- arguments -->

`source` is the whole program as text. `args` reaches it as `bench.args`. `tier` is the highest tier of tool the run may call; it defaults to `read_config` and is never above the tier of the runner's config entry. `tier: full` needs `confirm_full_tier: true`, and that must come from the person at the bench, because a run at that tier can call tools that drive outputs (each such tool still wants its own `confirm` and honours its own `dry_run`). `wall_s` is the wall-clock budget (default 600 s, at most the entry's `max_wall_s`), `max_calls` the number of tool calls (default 1000), `memory_mb` the address-space limit (default 2048 MiB: numpy needs room), `cpu_s` the CPU-time limit (default `wall_s` plus 10). `trace` records the line numbers the program reaches, so status shows where it is.

`dry_run` compiles the source and returns, for each tool it names as a string literal in `bench.call`, the tier it needs and whether this run would be allowed it. It starts nothing.

## Returns

`{"run": "<id>", "state": "running", "tier_cap": ..., "wall_s": ..., "max_calls": ..., "next": ...}`. With `dry_run`: `{"dry_run": true, "started": false, "source": {"ok": true, "calls": [...]}, "tool_calls": [{"tool", "tier", "allowed"}], ...}`; a syntax error is `source.ok: false` with the line.

## Failure modes

* `N runs are already active`: the entry's `max_runs` is reached. Wait, or [cancel](tools/bench_proc_cancel) one.
* `a run with tier full needs confirm_full_tier true`.
* `tier full is above the ReadConfig tier of the procedure runner`: the config entry caps what any run may have.
* A bad argument is refused before anything starts.

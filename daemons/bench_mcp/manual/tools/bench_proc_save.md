---
title: bench_proc_save
summary: Save a program as a named, versioned procedure with a description and the tier it needs.
see_also: [concepts/procedures, tools/bench_proc_load, tools/bench_proc_run, tools/bench_proc_delete]
tools: [bench_proc_save]
tier: read_config
verified: fixtures only, 2026-10-29
---

# bench_proc_save

Saving puts the source in the managed directory under `name`. Saving to an existing name adds a version (the last 20 are kept). The source must compile (a syntax error is refused with its line), and `tier` must be at least the tier of every tool the source names as a string literal in `bench.call`: a procedure that calls a full-tier tool cannot be saved as read-only. Tools that are computed (`bench.call(name, ...)`) are not seen, so the declared tier is the author's promise for them; a run is capped at the tier it is started with anyway. Nothing is run by saving.

## Arguments

<!-- arguments -->

## Returns

`{"name", "version", "tier", "tool_calls": [{"tool", "tier"}]}`; a literal tool name this bench does not offer is listed with `unavailable` and the reason.

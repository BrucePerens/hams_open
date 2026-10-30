---
title: bench_proc_list
summary: The procedure runs in the managed directory, newest first, with their states.
see_also: [concepts/procedures, tools/bench_proc_status]
tools: [bench_proc_list]
tier: read_only
verified: fixtures only, 2026-10-29
---

# bench_proc_list

## Arguments

<!-- arguments -->

## Returns

`saved`: the saved procedures (`name`, `description`, `tier`, `latest`, `versions`); `runs`: for each run `run`, `state`, `label`, `started_unix`, `ended_unix`, `calls`; and `active_here`, the number of runs this server process has going. Finished runs are deleted after the entry's `retention_days`, and beyond `keep_runs`.

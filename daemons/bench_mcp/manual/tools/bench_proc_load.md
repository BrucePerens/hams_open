---
title: bench_proc_load
summary: Read a saved procedure: description, declared tier, versions and the source of one version.
see_also: [concepts/procedures, tools/bench_proc_save, tools/bench_proc_run]
tools: [bench_proc_load]
tier: read_only
verified: fixtures only, 2026-10-29
---

# bench_proc_load

## Arguments

<!-- arguments -->

## Returns

`name`, `version`, `tier`, `description`, `versions` (each with `version`, `saved_unix`, `bytes`, `tier`, `note`) and `source`.

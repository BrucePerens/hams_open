---
title: bench_proc_delete
summary: Delete one version of a saved procedure, or the whole procedure; needs confirm.
see_also: [concepts/procedures, tools/bench_proc_save]
tools: [bench_proc_delete]
tier: read_config
verified: fixtures only, 2026-10-29
---

# bench_proc_delete

Saved procedures live in the managed directory of the runner, not in a repository; deleting one cannot be undone. Runs that already started keep their own copy of the source.

## Arguments

<!-- arguments -->

`confirm` must be true. Deleting the last remaining version deletes the procedure.

---
title: bench_proc_cancel
summary: Ask a procedure run to stop; cleanup handlers run for the grace time, then the process is killed.
see_also: [concepts/procedures, tools/bench_proc_status]
tools: [bench_proc_cancel]
tier: read_only
verified: fixtures only, 2026-10-29
---

# bench_proc_cancel

Asks the run to stop. `bench.sleep` and the next `bench.call` in the program raise `BenchCancelled`, the registered cleanup handlers run (they may call tools), and if the program has not ended within the entry's `grace_s` (default 5 s) its process group is killed. It needs only the lowest tier because it only stops work. Afterwards the instruments the run changed are put in their safe state (see [the procedure runner](concepts/procedures)). Cancelling a finished run changes nothing.

## Arguments

<!-- arguments -->

`wait_s` makes the call wait (up to 60 s) until the run has ended, so the answer is the final state; without it the answer is the state at that moment, usually `cancelling`.

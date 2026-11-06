---
title: bench_rx888_radiod_stop
summary: RX-888: stops the radiod this server started, by its own pid only.
see_also: [instruments/rx888/overview, instruments/rx888/operation, instruments/rx888/limits]
tools: [bench_rx888_radiod_stop]
instrument: rx888
tier: read_config
verified: measured on the unit (RX-888 MkII, no antenna), 2026-11-05
---

# bench_rx888_radiod_stop

Stops the `radiod` this server holds as a child, or, after a server restart, the pid in its pid file. The pid file is used only when `/proc` shows that process is the configured `radiod` with this instrument's config. Sends SIGTERM, then SIGKILL after 5 s. Never selects a process by name.

## Arguments

<!-- arguments -->

## Returns

`stopped`, `pid`, `exit`, `sigkill_needed`, `log_tail`.

## Failure modes

* `no radiod was started by this server`: nothing to stop.
* `pid ... is not radiod with ...`: the pid file was stale; it was removed and nothing was signalled.

Example: `bench_rx888_radiod_stop {}` returned `{"stopped": true, "exit": "exit status: 0", "sigkill_needed": false, ...}`.

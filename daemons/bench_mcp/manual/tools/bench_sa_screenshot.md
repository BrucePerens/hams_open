---
title: bench_sa_screenshot
summary: SSA3000X: the screen as a 1024 x 600 BMP file in BENCH_MCP_OUT. Changes nothing.
see_also: [instruments/sa/overview, instruments/sa/operation, instruments/sa/limits]
tools: [bench_sa_screenshot]
instrument: sa
tier: read_only
verified: measured on the unit (Siglent SSA3021X, firmware 1.3.9.10), 2026-11-05
---

# bench_sa_screenshot


Reads the screen with `:HCOPy:SDUMp:DATA?` and saves `sa_screen_*.bmp` (about 1.8 MB) in `BENCH_MCP_OUT`. Takes about two seconds. Changes nothing.

## Arguments

<!-- arguments -->

## Returns

`file`, `bytes`, `width`, `height`, `format`.

## Failure modes

* `the BMP says N bytes and M arrived`: the reply was cut short; call again, and do not use USB.
* `do not start like a BMP`: the unit was busy or hung; try `bench_sa_status`.

Example: `bench_sa_screenshot {}` returns `{"file": ".../sa_screen_...bmp", "width": 1024, "height": 600}`.

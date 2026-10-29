---
title: bench_mcp manual
summary: What bench_mcp is, how to find things in this manual, and a ten-line quickstart.
verified: n/a (navigation)
---

# bench_mcp manual

bench_mcp is one MCP server for the bench test equipment: an oscilloscope, a bench power supply, a vector network analyser, two software-defined radios, the host's USB hub ports and sound devices. It is the only way to touch that equipment. There is no raw-command tool: each function of each instrument is a tool with checked arguments, and the server (not the model) refuses a tool above the permission tier configured for the instrument.

This manual is inside the server. Read it with `bench_manual_index`, `bench_manual_read`, `bench_manual_search`, `bench_manual_for_tool` and `bench_manual_whats_new`; the same pages are MCP resources `bench-manual://<path>` for clients that show resources. Every tool's description ends with `Manual: tools/<tool name>`.

## Quickstart

1. `bench_list` shows the configured instruments (their alias, driver and tier). Only tools some instrument may use are listed.
2. `bench_manual_for_tool {"tool": "<name>"}` gives the page of one tool and the instrument pages that apply; `bench_manual_search {"query": "..."}` finds a word.
3. Read [safety tiers](concepts/safety-tiers) and [etiquette](concepts/etiquette) before the first call that changes something.
4. `bench_status` (with `"instrument"` when several fit) says what an instrument is doing and whether it answers.
5. Measure with the read-only tools first: `bench_measure`, `bench_capture`, `bench_scope_spectrum`, `bench_sdr_capture`.
6. Change settings with `bench_set` and the instrument tools. Nothing here transmits on the air: the VNA and generator put low-level RF on a cable port only, and the Hamlib transmit tools exist only for a rig the owner has attested into a dummy load (and the software cannot know what is connected: read [Hamlib limits](instruments/hamlib/limits) first).
7. A tool error is plain text. Read [errors](concepts/errors) for what to retry and when to stop and ask the person at the bench.
8. Files (captures, CSVs, screenshots) go to the output directory; the result names the file and its format.
9. A page's `verified` line says what was checked on real hardware and when. Trust it, and do not assume more.
10. Several instruments at once, and cross-instrument jobs: [procedures](procedures) and [several instruments](concepts/several-instruments).

## Contents

<!-- children -->

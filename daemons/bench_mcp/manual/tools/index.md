---
title: Tools
summary: One page per MCP tool: purpose, arguments, results, tier, side effects, failures, examples.
verified: n/a (navigation)
---

# Tools

Each page is named after the tool: `tools/<tool name>`. The argument table on every page is generated from the tool's JSON schema, so it is always the server's actual interface. `bench_manual_for_tool` returns a tool's page together with the pages of the instrument it applies to.

Naming: `bench_<function>` tools take an optional `instrument` and apply to several instruments (list, identify, status, measure, set); `bench_scope_*`, `bench_psu_*`, `bench_vna_*`, `bench_sdr_*`, `bench_usb_*`, `bench_audio_*` belong to one kind of instrument; `bench_manual_*` read this manual.

<!-- children -->

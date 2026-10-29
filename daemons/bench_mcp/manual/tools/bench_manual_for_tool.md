---
title: bench_manual_for_tool
summary: The page of one MCP tool plus the instrument pages that apply to it.
see_also: [tools/bench_manual_read, tools/bench_manual_search]
tools: [bench_manual_for_tool]
tier: read_only
verified: fixtures only, 2026-10-29
---

# bench_manual_for_tool

Maps a tool name to its page, to the other pages that document it, and to the pages of the instrument it applies to (overview, setup, operation, limits, troubleshooting), so one call tells you where to read.

## Arguments

<!-- arguments -->

* `tool`: the tool name, for example `bench_vna_sweep`. Required.

## Returns

`tool`, `registered`, `tier`, `page` (path, title, summary), `also_documented_in` (other pages whose `tools` list the tool), `instrument_pages`, `concepts` (the pages on tiers and errors) and `read_with`. An unknown name is an error that lists registered tools with similar names.

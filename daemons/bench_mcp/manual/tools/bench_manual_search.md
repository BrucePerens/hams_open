---
title: bench_manual_search
summary: Full-text search of the manual (BM25) with excerpts, optionally limited to one instrument or tool.
see_also: [tools/bench_manual_read, tools/bench_manual_for_tool]
tools: [bench_manual_search]
tier: read_only
verified: fixtures only, 2026-10-29
---

# bench_manual_search

Ranks the pages by BM25 over their title, summary, tool names and text (titles and tool names weigh more). A name with underscores such as `bench_vna_sweep` or `rf_level` also matches its parts. There is no stemming: search for `sweep`, not `sweeping`, and try a synonym when nothing matches.

## Arguments

<!-- arguments -->

* `query`: the words. Required; it must contain at least one letter or digit.
* `instrument`: only pages about this instrument, as the directory name under `instruments/` (`vna`, `scope`, `psu`, `bladerf`, `limesdr`, `usb-hubs`, `sound`).
* `tool`: only pages that document this tool.
* `limit`: 1 to 30, default 8.

## Returns

`query`, `matches` (how many pages matched), `results`: for each `path`, `title`, `summary`, `score`, `section` (the heading the excerpt is under) and `excerpt` (about 300 characters around the best paragraph). Read the page with [bench_manual_read](tools/bench_manual_read), passing `section` to read just that part. No match gives an empty list.

## Example

`bench_manual_search {"query": "silent state", "instrument": "vna"}` finds the pages on the reference channels reading zero and the power cycle that clears it.

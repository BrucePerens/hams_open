---
title: bench_manual_index
summary: The top of the manual tree with one-line summaries.
see_also: [tools/bench_manual_read, tools/bench_manual_search, index]
tools: [bench_manual_index]
tier: read_only
verified: fixtures only, 2026-10-29
---

# bench_manual_index

The entry point of the manual: the sections (concepts, instruments, procedures, tools, the changelog), each with its summary, the pages of each instrument, and the number of pages. Read the returned `read_with` hint for the next call.

## Arguments

<!-- arguments -->

This tool takes no arguments.

## Returns

`start` (what the manual is), `read_with`, `pages_total`, and `sections`: for each top-level entry `path`, `title`, `summary`, and for directories `page_count` and (except for `tools`, which is long) `children` with the same three fields.

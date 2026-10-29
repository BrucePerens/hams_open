---
title: bench_manual_whats_new
summary: Manual pages added or changed in each build version.
see_also: [whats-new, tools/bench_manual_read]
tools: [bench_manual_whats_new]
tier: read_only
verified: fixtures only, 2026-10-29
---

# bench_manual_whats_new

Lists the releases recorded in the manual's `whats-new` page, newest first, each with the pages it added or changed and a note. Use it after the server was updated to see which documentation to re-read.

## Arguments

<!-- arguments -->

* `version`: one build version such as `0.1.0`; default all. An unknown version is an error that lists the known ones.

## Returns

`build_version` (the version of the running server) and `releases`: each with `version` and `pages` (`path`, `title`, `exists`, `note`).

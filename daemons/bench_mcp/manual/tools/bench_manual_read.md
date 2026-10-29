---
title: bench_manual_read
summary: Read one manual page, or one section of it, with its metadata, related pages and children.
see_also: [tools/bench_manual_index, tools/bench_manual_search]
tools: [bench_manual_read]
tier: read_only
verified: fixtures only, 2026-10-29
---

# bench_manual_read

Returns a page. Paths run from the root of the manual without `.md`: `index`, `concepts/errors`, `instruments/vna/operation`, `tools/bench_vna_sweep`. The path of a directory (`instruments/vna`) returns its index page, whose text lists its children. A `bench-manual://` prefix, a leading slash or a trailing `.md` are tolerated.

## Arguments

<!-- arguments -->

* `path`: the page path. Required; use `index` for the top page.
* `section`: return only the section under a heading of the page: matched case-insensitively, exact heading first, then a heading that starts with the text, then one that contains it. The section runs to the next heading of the same or higher level. The returned `headings` list shows what exists; an unmatched section is an error listing them.
* `max_chars`: at most this many characters of text, 200 to 1000000, default 24000. A longer text is cut at a line end; `truncated` is `true` and `note` says how much exists. Pages are at most 16000 characters, so the default never cuts a page; use a smaller value to skim.

## Returns

`path`, `uri` (the MCP resource), `title`, `summary`, `instrument` (list), `tier`, `tools` (the tools the page documents), `verified` (what was checked on hardware and when), `see_also` (pages with their summaries), `children`, `up` (the page that lists this one), `headings`, `text`, `chars_total`, `truncated`. The text has the generated parts filled in: the argument table of a tool page and the children list of an index page.

## Failure modes

An unknown path gives `no manual page "..."; try: ...` with pages whose paths are similar.

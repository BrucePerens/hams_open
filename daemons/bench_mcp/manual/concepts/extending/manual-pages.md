---
title: Documentation a module must ship
summary: The page set per instrument, a page per tool, the README row and config example; what each must say. The tests check them.
see_also: [concepts/extending, instruments/example, concepts/extending/checks]
verified: code only, 2026-10-29
---

# Documentation a module must ship

The pages of `manual/instruments/example/` and `manual/tools/bench_example_*.md` are the model; the script copies them. Rewrite every page for the real instrument and delete the sentences that describe the template.

## Per instrument: `manual/instruments/<name>/`

| Page | Says |
|---|---|
| `index` | one paragraph and the children marker line copied from another instrument's index (the list is generated) |
| `overview` | what it is, model and firmware written against, the tools with tiers, what the driver does not do |
| `setup` | config entry with every key, cabling, safe starting levels, what the owner must do first (installing a library or rule, which port) |
| `operation` | the normal order of calls, concurrency and exclusive use, example requests and answers |
| `recipes` (optional) | measurement recipes for several tools together |
| `limits` | ranges and where each comes from (rating, owner cap, driver), RF/mains/legal notes (compliance is the owner's), what is not verified |
| `troubleshooting` | symptom, cause, next action, in escalation order |
| `protocol` | commands used, provenance table ([provenance](concepts/extending/provenance)) |

## Per tool: `manual/tools/<tool name>.md`

Front matter: `title` (the tool name), `summary` (one line, 200 characters at most), `see_also` (page paths from the root, no `.md`), `tools: [<tool name>]` first, `instrument: <name>` (a directory under `instruments/`), `tier` (the tool's own registered tier), `verified`. Body: one `#` heading equal to the title; purpose; `## Arguments` containing the marker line that every page under `manual/tools/` has (copy it from one; the table is generated from the JSON schema, so it cannot drift; explain each argument whose schema has no `description`, naming it in backticks with unit, range and default); returns with units; side effects; failure modes with the next action; a real example. A tool's description ends with `Manual: tools/<name>` automatically.

## Elsewhere

* `README.md`: a config example entry with `"driver": "<name>"`, a short section for the driver, and the anchors list.
* `manual/whats-new.md`: one line `- instruments/<name>: <what>` under the current build version.
* Pages link with the page path from the root of the manual in the link target, as in `[limits](instruments/vna/limits)`. Headings do not skip a level. No tabs. Names of pages and tools are lower case with hyphens or underscores as above.

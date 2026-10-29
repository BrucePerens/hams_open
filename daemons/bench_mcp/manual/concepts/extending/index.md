---
title: Extending bench_mcp
summary: How an agent adds an instrument or function to bench_mcp for its owner's bench and submits it as a pull request.
see_also: [procedures/add-an-instrument, instruments/example, concepts/safety-tiers, concepts/verification]
verified: code only, 2026-10-29
---

# Extending bench_mcp

This guide is for an AI agent (Claude Code or similar) working for a person who owns an instrument that the maintainer, Bruce Perens, does not. Your goal: add that instrument, or a new function of an existing one, to bench_mcp so it can be reviewed and merged without the maintainer owning the hardware. The source is `daemons/bench_mcp` in https://github.com/BrucePerens/hams_open (AGPL-3.0-or-later). The guide is in the repository as `daemons/bench_mcp/docs/EXTENDING.md` (the index) and `daemons/bench_mcp/manual/concepts/extending/` (the pages), and the running server serves it: `bench_manual_read` with path `concepts/extending`.

bench_mcp is one MCP server (stdio) that gives an agent a bench: a scope, a supply, a VNA, SDRs, USB hubs, sound devices. The server, not the model, enforces each instrument's permission tier. Every tool ships a manual page.

## What makes a good contribution

* One instrument, or one function of one instrument, per pull request, with its tools, a safety tier for each, a manual page for each tool and the standard instrument pages, tests that need no hardware, and an honest statement of what was run on real hardware, by whom.
* Small. A feature bundled with unrelated refactors is sent back. Fix a structural gap in its own pull request first.

## What does not belong

* Anything that cannot run on the owner's bench safely, or that needs the maintainer's hardware to review.
* A raw "send any command" tool.
* Copied vendor code, firmware, drivers, DLL disassembly or manual text ([provenance](concepts/extending/provenance)).
* Fixtures or pages with serial numbers, MAC or IP addresses, licence keys or recordings of speech.

## The short path

1. Read [before you write code](concepts/extending/before-you-code), ask the owner the questions on it and get explicit permission before touching the hardware.
2. Follow [add an instrument](procedures/add-an-instrument): copy the skeleton with `python3 tools/new_instrument.py <name>`, make the listed edits, write the dialect.
3. Obey the [standing rules](concepts/extending/standing-rules); they come from real incidents.
4. Write the tests and pages ([checks](concepts/extending/checks), [manual pages](concepts/extending/manual-pages)); `cargo test` fails until they are complete.
5. Have the owner run the ignored hardware test and approve the text, then [submit](concepts/extending/submit).

## Pages

<!-- children -->

Limits of the current structure: there is no instrument trait. A driver is a module plus about eleven one-line edits in `src/config.rs` and `src/main.rs` (the script prints them); two drivers added at once conflict in those lines, so rebase often. Some older tools predate the rules (`dry_run` on every actuating tool; some sound tools, when no alias is configured, choose the system default device as the target; their streams do set `node.dont-fallback`); the legacy tables in `src/checklist.rs` list them, and a new module is never added to one.

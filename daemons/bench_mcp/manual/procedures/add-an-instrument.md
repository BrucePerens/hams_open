---
title: Add an instrument to bench_mcp
summary: Step by step with commands, from the skeleton copy to the pull request. Read concepts/extending first.
see_also: [concepts/extending, concepts/extending/checks, concepts/extending/submit, instruments/example]
verified: followed as a dry run on the dev box by copying the skeleton as a new driver (acme) and breaking each check in turn, 2026-10-29
---

# Add an instrument to bench_mcp

Prerequisites: you read [Extending bench_mcp](concepts/extending), the owner has answered the questions in [before you write code](concepts/extending/before-you-code) and said you may connect. Work in a branch of a clone of https://github.com/BrucePerens/hams_open, in `daemons/bench_mcp`. Build tools: Rust stable and the system libraries the crate needs (ALSA headers for the audio code).

1. Baseline: `cargo test` passes before you change anything. If not, stop and report.
2. Name it: a lower-case vendor or model name, 2 to 16 letters and digits (`acme`).
3. Copy: `python3 tools/new_instrument.py acme`. It writes `src/acme/`, `manual/instruments/acme/`, `manual/tools/bench_acme_*.md`, `tests/acme_fixtures/`, `tests/hil_acme.rs`, and prints the shared edits.
4. Make the shared edits it lists in `src/config.rs` and `src/main.rs`, one line each. `cargo build` tells you if one is missing (the `connect()` match is exhaustive).
5. `cargo test`. It passes except for the checklist leftovers: README example, `whats-new` line, template wording, the header comment. This is your to-do list.
6. Read-only first: with the owner's permission, record a session of identify and status (capture or log what the vendor tool or a terminal sends), scrub it ([provenance](concepts/extending/provenance)), and put it in `tests/acme_fixtures/status.txt`.
7. Rewrite `src/acme/mod.rs`: `Config` and `parse_config`; `MODEL` and the identity check; the read tools; strict parsing of every reply; bounded tries and timeouts. Keep the anchors and move the tests with them.
8. Add control functions one at a time, each: its limits checked before any byte, a tier, `dry_run`, `confirm` when it drives or radiates, a hard time limit, a safe-state tool, tests for every fault in the fake device. Ask the owner before the first control command is sent to the real unit.
9. Pages: rewrite every page and each tool page ([documentation](concepts/extending/manual-pages)); `verified: fixtures only` until the owner reports a run.
10. README config example and driver section, anchors list, `manual/whats-new.md` line.
11. Checks: `cargo fmt`, then `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` ([checks](concepts/extending/checks)); the two Python checks from the repository root when you have `hams_shared`.
12. The owner runs `BENCH_MCP_CONFIG=<their config> cargo test --test hil_acme -- --ignored --nocapture` and gives you the output and a description of the unit and firmware. Update `verified` only to what that run shows, with the date.
13. Rebase on upstream main, re-run step 11, show the owner the diff, fixtures and the filled template, then [submit](concepts/extending/submit).

If a check fails, fix the cause. Do not edit a test, a legacy table or a fixture to make it pass.

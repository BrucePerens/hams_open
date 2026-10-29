---
title: Worked examples, do and don't, mistakes, maintenance
summary: How existing modules are structured with real paths, the mistakes seen in this project, and how to keep a module working across releases.
see_also: [concepts/extending, instruments/example, instruments/scope/overview, instruments/vna/protocol]
verified: code only, 2026-10-29
---

# Worked examples, do and don't, mistakes, maintenance

## The scope (SCPI over the LAN): the simplest real pattern

`src/scope.rs` (functions that take a `&mut Link`), `src/link.rs` (line link with timeouts and `#9` blocks), the `siglent_sds` arm of `config::parse`, tool specs in the `TOOLS` table of `src/main.rs`. Tests: `fake_scope` in `src/scope.rs` serves SCPI on a local socket. Pages: `manual/instruments/scope/*` and `manual/tools/bench_screenshot.md` and friends. Hardware test: `tests/hil.rs`. Newer modules own their tool list instead of using `TOOLS` (copy that).

## The VNA (clean-room protocol work)

`daemons/vnwa_open/` is a separate crate that speaks the unit's USB protocol; `src/vna.rs` and `src/vna_tools.rs` adapt it. The protocol notes are `daemons/vnwa_open/docs/PROTOCOL.md` (confirmed-on-the-wire table, corrections kept) and the manual page `instruments/vna/protocol`. Tests use recorded captures. The recovery path (retry, cold reset through the hub port, clear error) is `instruments/vna/monitoring-and-recovery`. A new instrument whose protocol must be worked out gets its own crate when the protocol is large enough to be reused.

## USB hubs and sound (newer, multi-file modules)

`src/usb_hub/` and `src/audio/`: `mod.rs` with `SPECS`, `tool_defs`, `call_with(runner, root, ...)` that takes the program runner and the sysfs root, so tests pass a recorded tree (`testkit.rs`, `tests/usb_fixtures`, `tests/audio_fixtures`). The safety code is separate and tested on its own (`audio/safety.rs`, `usb_hub/protect.rs`). Pages: `manual/instruments/usb-hubs/`, `manual/instruments/sound/`.

## Do

* Read before you write; ask the owner; start read-only.
* Copy `src/example/`; keep its checks: identity before any command, bounded tries, finite numbers, `dry_run`, `confirm`, device-side time limit, verified safe state.
* Put the cause of every limit in the error message.
* Keep the diff to your files plus the one-line shared edits.

## Don't

* Do not add a raw command tool, a free-text command argument, or a blanket "retry until it works".
* Do not claim verification you did not run. Do not edit `verified` to a date without the owner's report.
* Do not bundle refactors, formatting of other files, or changes to other instruments.
* Do not commit serial numbers, addresses, keys, recordings; do not paste vendor text.
* Do not add yourself to a LEGACY table in `src/checklist.rs`.

## Mistakes seen in this project

* Silent retries: a read repeated until it succeeds, with a stale or zero value returned on the last failure.
* Hanging reads: a socket or serial read with no timeout; one stuck instrument froze every tool. Always set the timeout.
* Claiming verification without hardware: a page marked verified from the manual's description of the command.
* Scope creep: a pull request that also reorganised the config parser.
* Leaking a serial number or address in a fixture, a page or a commit message.
* Using a default device: a stream that followed the system default to a reserved codec.
* Tests that made real sound or re-enumerated USB devices on the dev box.
* A tool judged "done" without its safe-state action.

## Maintenance

* Your module keeps working across bench_mcp releases through its tests, which run on every change to the crate (`cargo test`). A change elsewhere that breaks your fake-device tests is the other author's to fix; a change that makes your driver wrong is yours, found through the PR thread of the change or through an issue that mentions your module.
* The contact is the pull request thread of your contribution (and the issue tracker of the repository). The maintainer may ask the owner to rerun the hardware test after a release that touched your module; the answer is posted there.
* Deprecation: a tool is removed only after a release that lists it as deprecated in `manual/whats-new.md` and in its page, with the replacement; its page is deleted in the removal release, together with the tool (the tests require both).
* An instrument nobody can test any more is marked `code only` in its pages and, after notice in an issue, may be removed.

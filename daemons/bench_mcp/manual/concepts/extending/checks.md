---
title: Tests, fixtures, verification and the commands to run
summary: Testing without the maintainer's hardware, the fake device, the owner's hardware test, honest verified tags, anchors, and the exact commands.
see_also: [concepts/extending, concepts/verification, concepts/extending/manual-pages]
verified: code only, 2026-10-29
---

# Tests, fixtures, verification and the commands to run

## Test without the maintainer's hardware

* **Pure logic** (argument checks, parsing, unit conversion): ordinary unit tests next to the code.
* **The dialect** against a recorded, scrubbed session: `FakeDevice::from_transcript("tests/<name>_fixtures/status.txt")` in `src/<name>/testkit.rs` serves the replies of the recording on a local TCP socket; the code under test opens it with the production `Link::tcp`, so the whole path is the production one. A command not in the recording gets silence and shows in `received()`, so a changed conversation fails the test.
* **Faults**: a small simulator with switches (`Sim`): lost replies, a garbage reading, a stuck output, a dropped connection. Test each rung of your error ladder. This is the pattern `fake_scope` in `src/scope.rs` uses. Serial instruments and USB devices follow `src/usb_hub/testkit.rs` and `src/audio/testkit.rs` (a recorded system tree and a scripted program runner).
* **Never in unit tests**: real hardware, real sound, real transmission, real `sudo`, a network address outside 127.0.0.1.

## What the owner runs

`tests/hil_<name>.rs`: `#[test] #[ignore]`, run by the owner with `BENCH_MCP_CONFIG=<their config> cargo test --test hil_<name> -- --ignored --nocapture`. Read tools first and only; it prints what it observed so the owner can paste it into the pull request; it leaves the instrument as found; anything that drives or radiates needs an environment variable the owner sets for that run and is separately documented. The agent does not run it unattended.

## Verified tags (front matter `verified`)

| Value | When |
|---|---|
| `measured on the unit (<description of the unit and firmware>), <YYYY-MM-DD>` | the owner ran it on real hardware on that date and it behaved as the page says |
| `fixtures only` | tested against recordings and the fake device |
| `code only` | written from documentation, not run |
| `in progress`, `see <page>`, `n/a` | placeholder, inherits a page's statement, navigation |

The default for a new instrument is `fixtures only` (or `code only`). Upgrade a page to a dated statement only with the owner's report of what was run. An unverified claim is not acceptable because the next reader, an agent, trusts the page and will act on it with real equipment. The test requires a date or one of the keywords; it cannot know whether the date is true, so the reviewer reads the pull request's account of the hardware test.

## Anchors

Each non-trivial function carries `// [@ANCHOR: <name>]` inside it, the module's README lists it (a line ending with the anchor), and a test above it carries `// Tests [@ANCHOR: <name>]`. Anchors pair up: a test citing a missing anchor, or an anchor no test cites, fails `cargo test` (`src/checklist.rs`). Name them `<instrument>_<thing>`.

## Commands (run from `daemons/bench_mcp`)

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

On the maintainer's dev box wrap them: `CARGO_TARGET_DIR=$HOME/.cache/hams-cargo-target-<job> nice cargo test`, and delete the directory afterwards. From the root of the repository, scoped to your files:

```
python3 hams_shared/tools/check_rust_function_test_anchors.py . --baseline hams_shared/tools/rust_function_test_anchor_baseline_hams_open.json 2>&1 | grep 'bench_mcp/src/<name>'
HAMS_COM_TREE=<hams_com checkout, if you have it> python3 hams_shared/tools/verify_anchors.py . 2>&1 | grep '<name>_'
```

The first reports every function that lacks an anchor, across the repository (the existing bench_mcp code has many; yours must have none). The second needs the `hams_shared` submodule (`git submodule update --init`) and reports an anchor missing from the README. Contributors without the submodule rely on `cargo test`, which checks pairing, and the reviewer runs the rest.

`cargo test` also checks: the manual (a page per tool, tier agreeing with the registered tool, argument tables, links, one `#` heading, 16000-character limit), the checklist (tool names, `instrument` argument, `dry_run` on every `full` tool, a safe-state companion for every `full` tool, standard instrument pages, `whats-new` line, README config example, template wording gone, scrub of fixtures and pages for addresses, credentials and serial numbers, and a real, not future, date in every `verified` line).

## What no test catches

Whether the safe-state tool really reaches the safe state on the hardware; a fixture directory per instrument; whether what a `verified` line says was really done (the test checks only that it has a date that exists and is not in the future); a serial number written in a form the detector does not know; template wording beyond a few phrases. The reviewer and the pull request template cover these.

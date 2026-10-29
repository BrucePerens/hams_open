# Extending bench_mcp (index)

For AI agents working for people who own instruments the maintainer does not. The guide is the manual of the server, so the running MCP serves it (`bench_manual_read` with path `concepts/extending`) and there is one source, under `daemons/bench_mcp/manual/`:

| Page | File |
|---|---|
| Overview, what belongs, the short path | `manual/concepts/extending/index.md` |
| Before you write code: existing work, owner's facts, permission | `manual/concepts/extending/before-you-code.md` |
| Anatomy: files, registration, config, tools, errors, locking | `manual/concepts/extending/anatomy.md` |
| Standing safety rules (tiers, RF, privacy, five incident rules) | `manual/concepts/extending/standing-rules.md` |
| Provenance, clean-room protocol work, capture hygiene | `manual/concepts/extending/provenance.md` |
| Tests, fixtures, verification tags, anchors, commands | `manual/concepts/extending/checks.md` |
| Documentation a module ships | `manual/concepts/extending/manual-pages.md` |
| Submitting: fork, PR, no-GitHub route | `manual/concepts/extending/submit.md` |
| Worked examples, do/don't, mistakes, maintenance | `manual/concepts/extending/worked-example.md` |
| Step by step with commands | `manual/procedures/add-an-instrument.md` |

Code to start from: `src/example/` (`python3 tools/new_instrument.py <name>` copies it). Tests that enforce the checklist: `src/checklist.rs`, `src/manual/tests.rs`. PR template: `.github/PULL_REQUEST_TEMPLATE/bench_instrument.md`; plan issue: `.github/ISSUE_TEMPLATE/bench_instrument_plan.md`.

The rules that matter most, in case you read nothing else:

1. Ask the owner, and get an explicit yes, before connecting to the hardware or sending a command. Read-only first.
2. Never exceed a rated limit. Nothing radiates; no antenna unless the owner says so.
3. Never restart or reload a user's desktop audio service or trigger device re-enumeration.
4. Identify reserved devices by topology and ids; refuse them by name; no fallback to the system default (`node.dont-fallback`).
5. Every tool that can radiate or actuate has `dry_run`, a hard timeout and a safe-state action.
6. Tests never make audible sound outside allowed hours or key a transmitter.
7. System files we install: comments say what, why, scope, caveat, how to remove, "added by bench_mcp", with https://github.com/BrucePerens/hams_open.
8. No vendor code, firmware, drivers or manual text. No serials, addresses or keys in the repository.
9. Say honestly what was run on real hardware. `verified: fixtures only` is fine; an untrue date is not.
10. The owner approves the pull request text and fixtures before anything is published.

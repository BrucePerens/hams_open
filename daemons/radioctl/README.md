# radioctl

A thread-safe, async-only Rust rig-control library intended to replace Hamlib in the relay, plus a
`rigctld`-protocol server (`radioctld`). Licence: AGPL-3.0-or-later (Bruce's decision of
2026-10-30; every Rust file carries an SPDX header and the manifests inherit the licence from the
workspace). Planning lives in `hams_com/docs/proposals/RADIOCTL_RUST_RIG_CONTROL_LIBRARY.md`.

Status: Phase 0/1 foundation. There is no real serial, USB or Ethernet transport and no transmit
path to real hardware. Anything that can transmit must fail closed behind an explicit capability gate.

## Layout

```
daemons/radioctl/
  Cargo.toml              workspace (resolver 2), shared lints: no unsafe, no unwrap/expect/panic
  rust-toolchain.toml     pinned toolchain
  crates/radioctl-core/       typed values, capability model, errors, rig actor (PR 3 on)
  crates/radioctl-protocols/  sans-IO protocol engines, one per family (PR 3 on)
  crates/radioctld/           rigctld-protocol server (skeleton)
  docs/DESIGN.md          design note (ADR style)
```

Layout changes from the proposal (record every change here):

- None yet. The proposal's `radioctl-core`, `radioctl-protocols`, `radioctld` are used as named.
- No `deny.toml`: no other Rust crate in hams_open has one, so this workspace follows suit. Add one
  when the repository gets a shared policy.

## Build and test

```
CARGO_TARGET_DIR=$HOME/.cache/hams-cargo-target-radioctl nice -n 10 cargo fmt --check
CARGO_TARGET_DIR=... nice -n 10 cargo clippy --all-targets -- -D warnings
CARGO_TARGET_DIR=... nice -n 10 cargo test
```

Never a per-worktree `target/`; delete the target directory when the job is done (AGENTS.md).

## Conventions

- Anchors (ADR 0090): a function carries `// [@ANCHOR: id]` in source, its test carries
  `// Tests [@ANCHOR: id]`, and this README cites it as `[@ANCHOR: radioctl:id]` below.
  `crates/radioctl-core/tests/conventions.rs` fails when source, tests and README disagree, and
  when a Rust file lacks its SPDX header.
- Protocol engines are sans-IO (see `docs/DESIGN.md`).
- Tests use the same code paths as production; no test-only features.

## Function reference

- [@ANCHOR: radioctl:radioctl_core_hz_from_khz] Builds a frequency in hertz from whole kilohertz, refusing overflow.
- [@ANCHOR: radioctl:radioctl_core_hz_as_mhz] A frequency in megahertz for display.
- [@ANCHOR: radioctl:radioctld_banner] The line the daemon skeleton prints.
- [@ANCHOR: radioctl:radioctld_main] The skeleton daemon entry point; exits with status 2 because it serves nothing yet.

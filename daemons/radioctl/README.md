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
  tools/hamlib_caps_matrix.py   Tier 1 generator (rigctl --dump-caps -> matrix)
  data/hamlib-<version>/  generated capability_matrix.json and HAMLIB_DIFFERENCES.md
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

## Tier 1 capability matrix

`tools/hamlib_caps_matrix.py` runs `rigctl -l` and `rigctl -m N --dump-caps` for every model and writes
`data/hamlib-<version>/capability_matrix.json` and `HAMLIB_DIFFERENCES.md`. The input is program output
only; no Hamlib source is read or copied. Parsed fields are under `dump`; the protocol family, VFO model,
scope flag and quirk hints under `derived` are radioctl's own classification rules (from the script, with
the rule that fired recorded as `family_basis`) and are planning hints, not statements about a wire protocol.

```
nice -n 10 python3 tools/hamlib_caps_matrix.py --out-dir data/hamlib-4.6.2           # regenerate
nice -n 10 python3 tools/hamlib_caps_matrix.py --check --out-dir data/hamlib-4.6.2   # drift check
cd tools && python3 -m unittest test_hamlib_caps_matrix                            # tests
```

For a new Hamlib release, run it into a new `data/hamlib-<version>/` directory and diff against the old one.
Hamlib 4.6.2 lists 313 models (the proposal said 314; that count included the header line); 312 dump,
model 2 (NET rigctl) fails with `rig does not have tx_range` and is recorded as not dumpable.

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
- [@ANCHOR: radioctl:radioctl_caps_parse_rig_list] Parses `rigctl -l` into model id, manufacturer, model, backend version, status and macro.
- [@ANCHOR: radioctl:radioctl_caps_parse_dump] Parses one `--dump-caps` report: identity, serial settings, timing, VFO, functions, levels, parameters, modes, memories, scope data and the Y/N/E capability flags.
- [@ANCHOR: radioctl:radioctl_caps_parse_ranges] Summarises TX and RX frequency range blocks (count, lowest and highest hertz, highest TX power).
- [@ANCHOR: radioctl:radioctl_caps_classify_family] Derived protocol family from manufacturer, model name and port type, with the rule that fired.
- [@ANCHOR: radioctl:radioctl_caps_vfo_model] Derived VFO model (a_b, main_sub, single, none) from the printed VFO list.
- [@ANCHOR: radioctl:radioctl_caps_quirk_hints] Derived timing and behaviour hints from dump fields only.
- [@ANCHOR: radioctl:radioctl_caps_build_entry] One matrix entry: list row, dump fields and derived fields, or the dump error.
- [@ANCHOR: radioctl:radioctl_caps_build_matrix] Runs rigctl for every listed model and assembles the matrix with provenance.
- [@ANCHOR: radioctl:radioctl_caps_render_markdown] Renders the generated differences document from the matrix.
- [@ANCHOR: radioctl:radioctl_caps_main] Command line: write the files, or `--check` for drift.
- [@ANCHOR: radioctl:radioctld_main] The skeleton daemon entry point; exits with status 2 because it serves nothing yet.

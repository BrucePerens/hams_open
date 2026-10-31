# 11. Testing

Tests in this repository follow the standing rule: they use the production code paths, with no
test-only features. A virtual rig is the same engine-and-actor path as a real one with a different
transport. Rust is tested on the development machine, not in CI (AGENTS.md); record what you ran in the
commit or pull request.

## Running everything

From `daemons/radioctl`, under `nice`, with a target directory of your own that you delete when the job
is done (never a per-worktree `target/`, and do not share one between jobs building at the same time):

```sh
export CARGO_TARGET_DIR=$HOME/.cache/hams-cargo-target-mywork
nice -n 10 cargo fmt --check
nice -n 10 cargo clippy --all-targets --all-features -- -D warnings
nice -n 10 cargo test --all-features --no-fail-fast
rm -rf "$CARGO_TARGET_DIR"
```

`--all-features` turns on the `hamlib-oracle` tests, which need Hamlib's `rigctld` and `rigctl` binaries
(Hamlib 4.6.2 is installed on the development machine); without the binaries they fail loudly rather than
skip. Without the feature the rest runs on a machine with no Hamlib. The workspace lints (`missing_docs`
as a warning, `unwrap`, `expect`, `panic` and `unsafe` denied) apply to tests and examples too; test files
that must use `expect` say so with an `allow` at the top.

## Layers of test

| Layer | Where | What it proves |
|---|---|---|
| Unit tests | next to the code | encoders, decoders, name tables, the schedule and cache logic |
| Virtual rigs | `radioctl-protocols/src/virtual_rig.rs`, `io.rs` | an engine and a simulated radio generated from the same description agree on every operation |
| Transcript replay | `rigs/transcripts/`, `tests/rig_files.rs`, `conformance::replay` | a description, its engine and its virtual rig agree on the bytes of a recorded or hand-written session |
| FlexRadio over TCP | `radioctl-protocols/tests/flex_actor.rs` | the Flex engine, the actor and the TCP transport against a virtual radio on a loopback listener; no keying line reaches the wire while the gate is closed |
| The actor | `radioctl-core/tests/actor.rs` | priorities, deadlines, preemption, the cache, polling, callbacks, two runtimes; time is paused with `start_paused` so the tests are fast and deterministic |
| Transports | `radioctl-core/tests/transports.rs` | real pseudo-terminals, a vanished and reopened port, control lines, TCP, a fake sysfs tree |
| Transmit frequency | `radioctl-protocols/tests/tx_frequency.rs` | on virtual Kenwood, Elecraft, Yaesu newcat and CI-V radios in split, `active_tx_frequency` equals the transmit VFO's frequency and the callback reports it |
| The splitter | `radioctl-splitter/tests/splitter.rs` | shared reads, CI-V echo, unsolicited frames, PTT ownership, real ptys, TCP clients |
| radioctld | `radioctld/tests/*.rs` | recorded protocol transcripts, several clients, arbitration, the closed gate leaving no byte on the transport, the bounds |
| Property tests | `radioctl-protocols/tests/robustness.rs`, `radioctld/tests/parser_props.rs` | no byte sequence panics a parser; chunking does not change what a parser finds; whole-pipeline lines never transmit through a closed gate |
| Hamlib oracle | `radioctl-protocols/tests/rigctld_oracle.rs`, `radioctld/tests/rigctl_oracle.rs` | agreement with Hamlib 4.6.2 (below) |
| Conventions | `radioctl-core/tests/conventions.rs` | SPDX headers, AGPL manifests, anchor parity |
| This manual | `radioctld/tests/manual.rs` | the pages match the code and the examples run |

## Virtual rigs

`VirtualRig::from_description` builds a simulated radio of a description's family. It owns all its state,
shares nothing (any number run at once), and is sans-IO: `feed(bytes, now)` returns what the radio would
answer. `io::spawn_virtual_rig` runs one behind an in-memory duplex stream with an optional reply delay and
returns a `VirtualRigControl` to read its state, turn its knobs (`front_panel(PanelChange::...)`) and
change its speed. `radioctld`'s `virtual` transport uses the same function.

Virtual rigs implement what the engines use plus a few fixed answers that Hamlib's backends request at
start-up (so the oracle can run). They are **not** models of radios: a few starting values (levels,
firmware, SWR) are our own choices.

## Transcript replay

See chapter [6](06-rig-descriptions.md). `cargo test -p radioctl-protocols --test rig_files` loads every
description and replays every transcript. The recorded sessions of `radioctld`
(`crates/radioctld/tests/transcripts/*.txt`) are text files of `>` client lines and `<` server lines
replayed against a daemon over TCP.

## The Hamlib oracle

Hamlib is used as an independent implementation to compare with; its binaries are invoked and nothing
is copied.

- **Engine against `rigctld`** (`radioctl-protocols`, feature `hamlib-oracle`). Each radio's virtual twin is
  driven twice with the same scenario: once through Hamlib's `rigctld`, started on a pty whose master is the
  virtual rig, and once through radioctl's engine. The readings and final states are compared and the
  bytes Hamlib sent are printed (`--nocapture`). Disagreements that have been looked at are listed in
  `known_*_divergences` functions with the step; a new disagreement fails the test.
- **`radioctld` against `rigctl`** (`radioctld`, feature `hamlib-oracle`). Hamlib's own client, `rigctl -m 2`,
  drives `radioctld` over TCP against a virtual KX3, IC-7300 and TS-590SG, reads back the capabilities the
  daemon advertises in `\dump_state`, is refused while the gate is closed, keys only when the configuration
  opened the gate, and works in VFO mode.

```sh
nice -n 10 cargo test -p radioctl-protocols --features hamlib-oracle --test rigctld_oracle -- --nocapture
nice -n 10 cargo test -p radioctld --features hamlib-oracle
```

Set `RIGCTLD=/path` or `RIGCTL=/path` if the binaries are not on the `PATH`. The tests ran and passed
for this manual's release. Recorded disagreements at that time: the TS-590 and TS-590SG VFO B frequency
and AF gain; the IC-7610's split flow and VFO B; the IC-9700's VFO B; the Xiegu X108G's final VFO A; for the legacy Yaesu files, Hamlib's reads of frequency and mode fail against the virtual radio (its writes agree, and its FT-817 backend is not run because it sends an EEPROM read no manual documents). The Yaesu newcat files have none. The Flex files are not run through the oracle at all. Each is an open question for a bench check, not a verdict.

## Property tests and fuzzing

There is **no coverage-guided fuzzing** (no `cargo-fuzz` target). The tests named "standing in for
fuzzing" are `proptest` tests: arbitrary byte sequences of up to 600 bytes through each engine and the
virtual rigs, arbitrary text through the description and transcript parsers, arbitrary lines through the
`radioctld` parser, and a whole-pipeline run that must never transmit through a closed gate. The proposal
asks for fuzzing of every parser; real fuzz targets are **planned**.

## The anchor convention (ADR 0090)

ADR 0090 (`hams_shared/docs/adrs/0090_universal_function_test_anchor_ratchet.md`) requires every function
to have a test anchor. In this crate:

- A non-test function carries a comment line immediately above it with the anchor marker and a unique
  id. The ids here look like `radioctl_core_rig_spawn`: the crate, then the thing.
- Its test carries a comment with the same id and the word `Tests` before the marker.
- `README.md` documents the id in its function reference, written with the prefix `radioctl:`.
- `crates/radioctl-core/tests/conventions.rs` fails when source, tests and README disagree, and when a Rust
  file lacks its SPDX header. `hams_shared/tools/check_rust_function_test_anchors.py` is the repository
  ratchet; it does not scan `examples/`, `tests/`, `benches/`, `fuzz/` or `build.rs`, which is why the
  examples and this manual's test carry no anchors.
- When you add a function, add its marker, its test and its README line in the same change. When you move
  logic, move the anchor with it. Never remove an anchor to quiet a linter. This manual avoids writing the marker
  in its text so that the anchor scanner does not read it as a reference.

## Before you merge

Follow AGENTS.md. In short: work in a worktree under `~/workspace/tmp` made with `git worktree add -b` from
the `hams_open` checkout and run `python3 fix_worktree_symlinks.py` in it (again after any rebase); fetch and
rebase often because other agents edit these crates; run the commands above; then, from the worktree root with
the work committed:

```sh
HAMS_COM_TREE=$HOME/workspace/hams_com nice -n 10 python3 hams_shared/tools/run_linters.py --changed
nice -n 10 python3 hams_shared/tools/run_linters.py --only verify_anchors
nice -n 10 python3 hams_shared/tools/run_linters.py --only check_rust_function_test_anchors
nice -n 10 python3 hams_shared/tools/run_linters.py --only check_cargo_clippy
nice -n 10 python3 hams_shared/tools/run_linters.py --only check_cargo_fmt
```

Merge by pull request (`devbox_tools/prmerge.sh`), put the test results in the PR text, and remove the
worktree and branch at once after the merge.

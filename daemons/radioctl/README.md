# radioctl

A thread-safe, async-only Rust rig-control library intended to replace Hamlib in the relay, plus a
`rigctld`-protocol server (`radioctld`). Licence: AGPL-3.0-or-later (Bruce's decision of
2026-10-30; every Rust file carries an SPDX header and the manifests inherit the licence from the
workspace). Planning lives in `hams_com/docs/proposals/RADIOCTL_RUST_RIG_CONTROL_LIBRARY.md`.

Status: Phase 0/1 foundation (vocabulary, CI-V and Kenwood engines, virtual rigs; actor not yet). There is no real serial, USB or Ethernet transport and no transmit
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

- The crates are used as named. Dependency direction: `radioctl-protocols` depends on `radioctl-core`
  (which holds the vocabulary: `Hz`, `Mode`, `Vfo`, `Op`, `Observation`, `Capability`, `RigDescription`,
  `RigError`, and the `ProtocolEngine` trait); `radioctld` depends on both. The actor in `radioctl-core`
  is generic over `ProtocolEngine`, so core never depends on the protocols crate.
- `RigDescription` is a Rust struct for now; the two built-in descriptions (IC-7300 style CI-V, TS-2000
  style Kenwood) are written in code from our own simulator and bridges and are unverified. The TOML
  format with provenance and `verified` fields deserialises into the same struct (a later PR).
- Virtual rigs live in `radioctl-protocols::virtual_rig` and are sans-IO like the engines; the tokio
  glue that exposes one as a duplex stream arrives with the actor.
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
- [@ANCHOR: radioctl:radioctl_core_capability_transmits] (radioctl-core) True when using the capability can put RF on the air.
- [@ANCHOR: radioctl:radioctl_core_capability_set_of] (radioctl-core) Builds a set from a list.
- [@ANCHOR: radioctl:radioctl_core_capability_set_has] (radioctl-core) True when the capability is present.
- [@ANCHOR: radioctl:radioctl_core_capability_set_iter] (radioctl-core) The capabilities in a stable order.
- [@ANCHOR: radioctl:radioctl_core_description_mode_code] (radioctl-core) The wire code for a mode, if the model has one.
- [@ANCHOR: radioctl:radioctl_core_description_code_mode] (radioctl-core) The mode for a wire code, if the model has one.
- [@ANCHOR: radioctl:radioctl_core_description_ic7300] (radioctl-core) An Icom IC-7300 style CI-V description (address 0x94). Unverified; see the module note.
- [@ANCHOR: radioctl:radioctl_core_description_ts2000] (radioctl-core) A Kenwood TS-2000 style ASCII description. Unverified; see the module note.
- [@ANCHOR: radioctl:radioctl_core_error_new] (radioctl-core) Builds an error with the default retry hint for its kind and an empty transcript.
- [@ANCHOR: radioctl:radioctl_core_error_with_transcript] (radioctl-core) Attaches the transcript tail.
- [@ANCHOR: radioctl:radioctl_core_observation_field] (radioctl-core) The cache field this observation updates.
- [@ANCHOR: radioctl:radioctl_core_op_capability] (radioctl-core) The capability a rig needs for this command.
- [@ANCHOR: radioctl:radioctl_core_op_transmits] (radioctl-core) True when the command can put RF on the air. Unkeying never transmits.
- [@ANCHOR: radioctl:radioctl_core_op_is_emergency] (radioctl-core) True for the commands that jump the queue: unkeying the transmitter.
- [@ANCHOR: radioctl:radioctl_civ_frame_to_bytes] (radioctl-protocols) The wire form of the frame.
- [@ANCHOR: radioctl:radioctl_civ_encode_bcd_freq] (radioctl-protocols) Encodes a frequency as 5 BCD bytes, or `None` above 9,999,999,999 Hz.
- [@ANCHOR: radioctl:radioctl_civ_decode_bcd_freq] (radioctl-protocols) Decodes up to 5 BCD bytes (least-significant pair first); `None` on a non-BCD nibble.
- [@ANCHOR: radioctl:radioctl_civ_decode_bcd_meter] (radioctl-protocols) Decodes a 2-byte BCD meter value, most-significant pair first (`01 20` is 120).
- [@ANCHOR: radioctl:radioctl_civ_encode_bcd_meter] (radioctl-protocols) Encodes a meter value 0 to 9999 as 2 BCD bytes, most-significant pair first.
- [@ANCHOR: radioctl:radioctl_civ_parser_feed] (radioctl-protocols) Adds bytes received at `now` and returns every complete frame and every run of junk.
- [@ANCHOR: radioctl:radioctl_civ_engine_new] (radioctl-protocols) Builds an engine from a CI-V description.
- [@ANCHOR: radioctl:radioctl_civ_engine_encode] (radioctl-protocols) Encodes an Op as a CI-V request with the reply to expect; refuses VFO B, transceive and anything the description lacks.
- [@ANCHOR: radioctl:radioctl_civ_engine_feed] (radioctl-protocols) Decodes CI-V replies and unsolicited transceive frames; skips echoes of our own frames and frames from other stations.
- [@ANCHOR: radioctl:radioctl_kenwood_tokenizer_feed] (radioctl-protocols) Adds bytes received at `now` and returns every complete token.
- [@ANCHOR: radioctl:radioctl_kenwood_parse_digits] (radioctl-protocols) Parses a string of ASCII digits only (no sign, no spaces).
- [@ANCHOR: radioctl:radioctl_kenwood_engine_new] (radioctl-protocols) Builds an engine from a Kenwood-style description.
- [@ANCHOR: radioctl:radioctl_kenwood_engine_encode] (radioctl-protocols) Encodes an Op as a Kenwood ASCII request; refuses PTT readback, power meter and CW.
- [@ANCHOR: radioctl:radioctl_kenwood_engine_feed] (radioctl-protocols) Decodes Kenwood replies, auto-information and `?;` refusals.
- [@ANCHOR: radioctl:radioctl_virtual_from_description] (radioctl-protocols) Builds a virtual rig of the described model, in its default state.
- [@ANCHOR: radioctl:radioctl_virtual_state] (radioctl-protocols) The rig's current state, for assertions.
- [@ANCHOR: radioctl:radioctl_virtual_feed] (radioctl-protocols) Consumes bytes the controller wrote at `now`; returns the bytes the radio sends back.
- [@ANCHOR: radioctl:radioctl_virtual_front_panel] (radioctl-protocols) Changes state as the radio's own knobs would; returns the unsolicited bytes the rig sends
- [@ANCHOR: radioctl:radioctl_civ_parser_drop_leading_junk] (radioctl-protocols) Reports bytes before any preamble as junk, keeping a trailing lone FE.
- [@ANCHOR: radioctl:radioctl_civ_frame_or_junk] (radioctl-protocols) Turns the bytes FE FE ... FD into a frame or junk; a second preamble restarts the frame.
- [@ANCHOR: radioctl:radioctl_civ_engine_decode] (radioctl-protocols) Decodes one CI-V frame into an Inbound; drops echoes and other stations.
- [@ANCHOR: radioctl:radioctl_kenwood_engine_decode] (radioctl-protocols) Decodes one Kenwood reply token into an Inbound.
- [@ANCHOR: radioctl:radioctl_virtual_state_power] (radioctl-protocols) Power meter of a virtual rig: nonzero only while keyed.
- [@ANCHOR: radioctl:radioctl_virtual_civ_frame] (radioctl-protocols) Answers one CI-V frame: address filter, bus echo, OK/NG.
- [@ANCHOR: radioctl:radioctl_virtual_civ_dispatch] (radioctl-protocols) The virtual CI-V command table (frequency, mode, PTT, CW, ID, meters).
- [@ANCHOR: radioctl:radioctl_virtual_civ_mode_reply] (radioctl-protocols) Mode read reply of the virtual CI-V rig.
- [@ANCHOR: radioctl:radioctl_virtual_kenwood_command] (radioctl-protocols) The virtual Kenwood command table.
- [@ANCHOR: radioctl:radioctl_virtual_kenwood_id] (radioctl-protocols) The ID the virtual Kenwood rig reports.
- [@ANCHOR: radioctl:radioctl_virtual_set_frequency] (radioctl-protocols) Sets VFO A or B from 11 Kenwood digits, else `?;`.
- [@ANCHOR: radioctl:radioctl_virtual_set_kenwood_mode] (radioctl-protocols) Sets the mode from one Kenwood digit, else `?;`.
- [@ANCHOR: radioctl:radioctl_virtual_data_reply] (radioctl-protocols) Builds a CI-V frequency reply.
- [@ANCHOR: radioctl:radioctl_virtual_meter_reply] (radioctl-protocols) Builds a CI-V meter reply (BCD).

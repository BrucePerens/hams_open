# radioctl

A thread-safe, async-only Rust rig-control library intended to replace Hamlib in the relay, plus a
`rigctld`-protocol server (`radioctld`). Licence: AGPL-3.0-or-later (Bruce's decision of
2026-10-30; every Rust file carries an SPDX header and the manifests inherit the licence from the
workspace). Planning lives in `hams_com/docs/proposals/RADIOCTL_RUST_RIG_CONTROL_LIBRARY.md`.

Status: Phase 0/1 foundation (vocabulary, CI-V, Kenwood and Elecraft KX3 engines, virtual rigs, rig actor with queue, cache, polling, events). Real serial (USB-serial identified by vid/pid/serial, hot-plug reconnect) and TCP transports exist; there is no transmit
path to real hardware. Anything that can transmit must fail closed behind an explicit capability gate.

## Layout

```
daemons/radioctl/
  Cargo.toml              workspace (resolver 2), shared lints: no unsafe, no unwrap/expect/panic
  rust-toolchain.toml     pinned toolchain
  crates/radioctl-core/       typed values, capability model, errors, rig actor (PR 3 on)
  crates/radioctl-protocols/  sans-IO protocol engines, one per family (PR 3 on)
  crates/radioctl-splitter/   virtual IC-7300 CI-V / TS-2000 CAT endpoints (pty, TCP) in front of one rig actor
  crates/radioctld/           rigctld text-protocol server (library + binary `radioctld`)
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
- Virtual rigs live in `radioctl-protocols::virtual_rig` and are sans-IO like the engines; the optional
  `io` module (cargo feature `tokio-io`, default on) runs one behind an in-memory duplex stream with a
  reply delay and a control handle. `radioctl-core` dev-depends on `radioctl-protocols` for its actor tests.
- `radioctl-splitter` (new crate, so the library crates stay free of pty code) depends on core and protocols; `radioctld` uses its PTT arbiter (`PttOwner`), so the two share one set of ownership semantics.
- `radioctl-core` has the cargo feature `owned-runtime` (off by default): the one place the library builds a
  tokio runtime of its own, for programs that are not tokio programs (the planned C ABI). `radioctld` does not
  use it: the `radioctld` binary builds the one multi-thread runtime of its process and passes its `Handle`
  to the library, which creates none.
- No `deny.toml`: no other Rust crate in hams_open has one, so this workspace follows suit. Add one
  when the repository gets a shared policy.

## Build and test

```
CARGO_TARGET_DIR=$HOME/.cache/hams-cargo-target-radioctl nice -n 10 cargo fmt --check
CARGO_TARGET_DIR=... nice -n 10 cargo clippy --all-targets -- -D warnings
CARGO_TARGET_DIR=... nice -n 10 cargo test
```

Differential tests against Hamlib's `rigctld` (needs the binary, or `RIGCTLD=/path`; they fail
loudly without it, so they sit behind a feature):

```
CARGO_TARGET_DIR=... nice -n 10 cargo test -p radioctl-protocols --features hamlib-oracle --test rigctld_oracle -- --nocapture
```

Each rig's virtual twin is driven twice, once through `rigctld` (started on a pty whose master is the
virtual rig) and once through radioctl's engine, and the readings and final states are compared;
the bytes Hamlib sent are printed. First results (Hamlib 4.6.2): KX3 and IC-7300 agree on frequency,
mode, PTT read, RIT offset and keyer speed; the TS-2000 differs because Hamlib's backend reads
the frequency from `IF;`, which the virtual TS-2000 refuses (layout unverified), and that known gap
is pinned by its own test. The oracle also found and fixed two virtual-rig errors (IC-7300 `26 00`
lacked the DATA flag byte; the virtual KX3 lacked `PS`, `K2`/`K3`, `OM`, `IF`, `RV` for other
modules, which Hamlib's KX3 backend sends at start-up).

Never a per-worktree `target/`; delete the target directory when the job is done (AGENTS.md).

## Rig descriptions and conformance

`rigs/*.toml` are rig description files (schema 1; format documented at the top of
`crates/radioctl-core/src/rigfile.rs`). Every datum must be explained by a `[[facts]]` entry with a
`provenance` (manual, capture, owner, repo-code, program-output) and a `verified` state (`unverified`,
`manual-checked`, `transcript-checked`, `hardware-verified` with radio, firmware, date and what was
tested); a file with an unexplained or unknown datum does not load. `rigs/transcripts/*.toml` are wire
transcripts replayed by `radioctl_protocols::conformance::replay` against the engine and the virtual rig
generated from the same description (tests in `crates/radioctl-protocols/tests/rig_files.rs`).

The descriptions are `rigs/elecraft-kx3.toml` and four Icom CI-V files (`icom-ic-7300`, `icom-ic-705`, `icom-ic-7610`, `icom-ic-9700`). **The Icom files are not written from Icom's CI-V guides**: the download terms of icomjapan.com forbid reuse of the manuals' data and radioctl skips sources whose terms forbid reuse, so their facts were read from Hamlib 4.6.2 source (facts only, provenance kind `hamlib-source`, file and function named per fact) and from `rigctl --dump-caps` output. Their scope support (command 0x27) is in `crates/radioctl-protocols/src/civ_scope.rs`; it is not yet reachable through `Op` (radioctl-core has no scope operation), `CivEngine` collects complete sweeps for `take_scope_lines`. Disagreements with Hamlib's `rigctld` are pinned in `tests/rigctld_oracle.rs` (`known_icom_divergences`). The Kenwood files (`kenwood-ts-2000`, `kenwood-ts-590s`, `kenwood-ts-590sg`) are written from Kenwood's published PC command references (document, page and, for the TS-590 guide, the PDF's sha256 in each fact); the TS-2000 text was read from a third-party transcription of its manual, which the facts say. The `IF;` layout that was unverified is resolved in `kenwood.rs`; the TS-890S dialect (no `IF`, `OM` for the mode, a bandscope) is not implemented yet. The Xiegu files (`xiegu-g90`, `xiegu-x6100`, `xiegu-x108g`) are deliberately small and use Hamlib facts only: the one public Xiegu CI-V document found (Radioddity's X6200 extract) forbids use without written permission, so it was skipped; the X5105 and X6200 have no description (address unknown, document unusable). The Yaesu newcat files (`yaesu-ft-991a`, `yaesu-ftdx10`, `yaesu-ftdx101d`) are written from Yaesu's published CAT Operation Reference Manuals (document, PDF page and sha256 per fact; the FTDX101D copy is a mirror of the yaesu.com PDF). They use the new family `yaesu-newcat` with a `[yaesu]` block. Split is claimed for the FT-991A only (the FTDX10 has a separate `ST`, the FTDX101's FA/FB are the MAIN and SUB bands); the legacy five-byte family (`yaesu-ft-817`, `yaesu-ft-857d`, `yaesu-ft-897d`, family `yaesu-legacy`, binary transcripts) is written from the operating manuals' CAT chapters, with the PTT-bit level taken from Hamlib (the manual's figure is silent, so that fact is `hamlib-source`); the FT-818 has no manual and no description. The KX3 file (written from Elecraft's published Programmer's Reference Rev. G5, cited by command heading and page since the manual has no numbered sections, with the PDF's sha256; its transcript is hand-written from the same manual). **Everything in these files is unverified**, and its
transcript is hand-written from our own simulator and bridge code (its `origin` says so): it proves that
the description, the engine and the virtual rig agree with each other, not that they agree with an
IC-7300. A real capture or an owner's bench run is what upgrades a `verified` field.

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

## radioctld

`radioctld --config FILE [--check]` serves the Hamlib `rigctld` text protocol (as documented in the
`rigctl`/`rigctld` man pages and as observed from Hamlib 4.6.2's `rigctld`) for any radioctl rig, real
or virtual, so WSJT-X, fldigi, loggers and `rigctl -m 2` work against it unchanged. One `[[rig]]` per
radio, each with its own listen address (as with one `rigctld` per radio); see `radioctld.example.toml`.

- Commands: `f F m M v V t T s S i I x X j J z Z l L u U b` (Morse), `1`/`\dump_caps`, `\dump_state`,
  `\chk_vfo`, `\get_powerstat`, `\get_info`, `\get_lock_mode`, `\get_vfo_info`, `\get_vfo_list`,
  `\get_split_freq_mode`, `\set_split_freq_mode`, `q`; the extended `+` response format; every
  VFO-taking command accepts the VFO argument or not, in either VFO mode (`vfo_mode`). Other Hamlib
  commands answer `RPRT -4`; unknown words `RPRT -1`; what the rig cannot do `RPRT -11`.
  The raw `send_cmd` (`w`), power control and Morse stop are never served (`RPRT -11`).
  Arguments are checked strictly: a missing, surplus or malformed argument is `RPRT -1`, never a wait
  for another line (Hamlib's `rigctld` waits).
- Several clients share a rig through its single actor queue; reads come from the actor's cache when
  newer than `cache_max_age_ms`. No locks around the rig.
- PTT: one owner at a time, with the splitter's `PttOwner`. The first client to key owns the
  transmitter; another client's `T 1` or `b` is `RPRT -9` (rejected) until the owner unkeys or
  disconnects (which unkeys the rig); anyone may unkey. Transmit commands are refused with `RPRT -9`
  before anything is queued while the rig's transmit gate is closed or its link is down. The gate is
  closed unless the configuration says `transmit = true`; `\dump_state` advertises a transmit range only
  when it is open. Stopping the daemon unkeys a rig a client was keying.
- Fail closed in the actor itself, below every client: a key or Morse command that was written
  to the rig but never answered (a timeout, a lost reply) is followed at once by an unkey, because
  the rig may be transmitting; when the last handle to a rig is dropped the actor unkeys a rig whose
  cache says keyed or whose gate was open before it ends; a `LineKeyer` makes its line inactive when
  it is dropped. Each is written to the transcript as `FAIL CLOSED unkey`.
- Network exposure: every listener binds loopback by default and the allow list defaults to loopback.
  Serving beyond it needs `allow_non_loopback_bind = true` together with an explicit `allow` list
  (checked at load; the allow list is applied on accept, before a byte is read).
- Bounds: line length (`max_line_bytes`), clients per rig, idle time, time to finish a started line,
  write time; an over-long line gets `RPRT -1` and the connection is closed.
- Runtime: the `radioctld` binary owns the one multi-thread tokio runtime; the library takes a `Handle`.
- Deviations from `rigctld`, all on purpose: PTT is released when its owner disconnects; the VFO
  selected by `V` is radioctld's own state (radioctl addresses VFOs explicitly); split is VFO A to
  receive and VFO B to transmit, and `X`/`\set_split_freq_mode` accept only the mode the rig is already
  in; passbands the rig has no filter control for are the mode's typical width; levels are scaled
  (0 to 1, or the rig's own integer for keyer speed) by the ranges the engines enforce, repeated in
  `names::level_range` and pinned to the engines by `tests/levels.rs`; the CI-V keyer speed is not
  served because that engine does not carry it.
- Tests: protocol transcripts (`tests/transcripts/`), two or more clients, arbitration and the closed
  gate (a refused transmit command leaves no byte on the rig's transport), the bounds, property tests
  of the parser and a whole-pipeline fuzz, and the Hamlib `rigctl` client as an oracle against a virtual
  KX3, IC-7300 and TS-590SG (`--features hamlib-oracle`, needs `rigctl` or `RIGCTL=/path`):

```
CARGO_TARGET_DIR=... nice -n 10 cargo test -p radioctld --features hamlib-oracle
```

## Conventions

- Anchors (ADR 0090): a function carries `// [@ANCHOR: id]` in source, its test carries
  `// Tests [@ANCHOR: id]`, and this README cites it as `[@ANCHOR: radioctl:id]` below.
  `crates/radioctl-core/tests/conventions.rs` fails when source, tests and README disagree, and
  when a Rust file lacks its SPDX header.
- Protocol engines are sans-IO (see `docs/DESIGN.md`).
- Tests use the same code paths as production; no test-only features.

## Function reference

- [@ANCHOR: radioctl:radioctl_core_hz_from_khz] Builds a frequency in hertz from whole kilohertz, refusing overflow.
- [@ANCHOR: radioctl:radioctl_core_level_name] (radioctl-core) The name of a level (AF gain, RF gain, mic gain, monitor level, keyer speed, RF power) in description and transcript files.
- [@ANCHOR: radioctl:radioctl_core_level_from_name] (radioctl-core) The level with a file-format name.
- [@ANCHOR: radioctl:radioctl_core_transcript_step_bytes] (radioctl-core) One direction of a transcript step, as hex or as ASCII text (not both).
- [@ANCHOR: radioctl:radioctl_core_hz_as_mhz] A frequency in megahertz for display.
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
- [@ANCHOR: radioctl:radioctl_elecraft_level_wire] (radioctl-protocols) The Elecraft command prefix and manual range of a level.
- [@ANCHOR: radioctl:radioctl_elecraft_signed_offset] (radioctl-protocols) Formats a RIT/XIT offset as `+0050` / `-0050`.
- [@ANCHOR: radioctl:radioctl_elecraft_parse_offset] (radioctl-protocols) Parses `ROsnnnn` data (sign or blank, four digits).
- [@ANCHOR: radioctl:radioctl_elecraft_parse_firmware] (radioctl-protocols) Parses the `NN.NN` revision of an `RVx` answer.
- [@ANCHOR: radioctl:radioctl_elecraft_decode_if] (radioctl-protocols) Decodes an Elecraft `IF` answer into frequency, offset, RIT, XIT, transmit state, mode and split.
- [@ANCHOR: radioctl:radioctl_elecraft_engine_new] (radioctl-protocols) Builds an engine from an Elecraft description.
- [@ANCHOR: radioctl:radioctl_elecraft_engine_encode] (radioctl-protocols) Encodes an Op as an Elecraft command; ranges are checked against the manual, `KY` text and values the engine cannot express are refused.
- [@ANCHOR: radioctl:radioctl_elecraft_encode_set] (radioctl-protocols) The setting commands of the Elecraft engine (frequency, mode, PTT, AI, split, RIT/XIT, filter, levels, CW text).
- [@ANCHOR: radioctl:radioctl_elecraft_check_ky_text] (radioctl-protocols) Accepts `KY` text of 1 to 24 printable characters without `;`, `<` or `>`.
- [@ANCHOR: radioctl:radioctl_elecraft_engine_feed] (radioctl-protocols) Decodes Elecraft replies, `IF` answers, auto-information and `?;`.
- [@ANCHOR: radioctl:radioctl_elecraft_decode] (radioctl-protocols) Decodes one Elecraft reply token into observations.
- [@ANCHOR: radioctl:radioctl_elecraft_decode_one] (radioctl-protocols) Decodes the single-value Elecraft replies (`FA`, `MD`, `TQ`, `SM`, levels, ...).
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_command] (radioctl-protocols) The virtual Elecraft (KX3) command table.
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_set_meta] (radioctl-protocols) Stores a `K2n`/`K3n` meta-command value of the virtual Elecraft rig (Basic formats are still used).
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_level] (radioctl-protocols) Reads or sets a level of the virtual Elecraft rig, clamped to the manual's range.
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_if] (radioctl-protocols) The `IF` answer of the virtual Elecraft rig.
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_panel] (radioctl-protocols) What the virtual Elecraft rig sends on its own after a panel change, by `AI` mode.
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_power] (radioctl-protocols) The virtual Elecraft `PO` reading: nonzero only while keyed.
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_id] (radioctl-protocols) The ID the virtual Elecraft rig reports.
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
- [@ANCHOR: radioctl:radioctl_core_backoff_delay] (radioctl-core) The wait after N failed attempts: exponential, at least 1 ms, capped.
- [@ANCHOR: radioctl:radioctl_core_usb_classify] (radioctl-core) The USB-to-serial chip family (FTDI, CP210x, CH340, CDC ACM) for a vendor and product id.
- [@ANCHOR: radioctl:radioctl_core_usb_port_chip] (radioctl-core) The chip family of a listed port.
- [@ANCHOR: radioctl:radioctl_core_usb_select_port] (radioctl-core) The one port matching vid/pid/serial; none or several are distinct typed errors.
- [@ANCHOR: radioctl:radioctl_core_usb_read_trimmed] (radioctl-core) The trimmed text of a sysfs attribute, or nothing if missing or empty.
- [@ANCHOR: radioctl:radioctl_core_usb_list_sysfs] (radioctl-core) Lists `ttyUSB*`/`ttyACM*` ports with USB ids from a sysfs tree.
- [@ANCHOR: radioctl:radioctl_core_usb_list_ports] (radioctl-core) Lists this machine's serial ports that carry USB ids (sysfs on Linux).
- [@ANCHOR: radioctl:radioctl_core_keyer_new] (radioctl-core) A control-line keyer; makes its line inactive when built.
- [@ANCHOR: radioctl:radioctl_core_keyer_key] (radioctl-core) Keys through the line; refused unless the rig's transmit gate is open.
- [@ANCHOR: radioctl:radioctl_core_keyer_release] (radioctl-core) Unkeys the line; always allowed.
- [@ANCHOR: radioctl:radioctl_core_keyer_drop] (radioctl-core) A dropped keyer makes its line inactive, so a holder that returns or panics while keyed never leaves RF on the air.
- [@ANCHOR: radioctl:radioctl_core_tcp_connect] (radioctl-core) Connects over TCP with a per-attempt timeout and backed-off retries.
- [@ANCHOR: radioctl:radioctl_core_serial_config_new] (radioctl-core) A serial configuration: 8N1, lines managed and inactive, default backoff, one open attempt.
- [@ANCHOR: radioctl:radioctl_core_serial_control_set_line] (radioctl-core) Sets RTS or DTR immediately on the open port.
- [@ANCHOR: radioctl:radioctl_core_serial_link_status] (radioctl-core) The link status: connected (with generation), reconnecting, closed.
- [@ANCHOR: radioctl:radioctl_core_serial_link_watch] (radioctl-core) A receiver that wakes on link status changes.
- [@ANCHOR: radioctl:radioctl_core_serial_link_stats] (radioctl-core) Bytes in, out, dropped while down, and reconnects.
- [@ANCHOR: radioctl:radioctl_core_serial_resolve] (radioctl-core) The device path for a target: the path, or the one port matching the USB ids.
- [@ANCHOR: radioctl:radioctl_core_serial_open_port] (radioctl-core) Opens and configures the port and makes DTR/RTS inactive (or fails).
- [@ANCHOR: radioctl:radioctl_core_serial_write_fully] (radioctl-core) Writes all bytes, riding out short timeouts for about two seconds.
- [@ANCHOR: radioctl:radioctl_core_serial_supervisor_run] (radioctl-core) The supervisor thread: first open, sessions, reconnects with backoff.
- [@ANCHOR: radioctl:radioctl_core_serial_sleep_dropping] (radioctl-core) Waits out a backoff delay, dropping and counting bytes written meanwhile.
- [@ANCHOR: radioctl:radioctl_core_serial_session] (radioctl-core) One connection: the writer loop plus a reader thread.
- [@ANCHOR: radioctl:radioctl_core_serial_read_loop] (radioctl-core) The reader thread: port to async side until stop or failure.
- [@ANCHOR: radioctl:radioctl_core_serial_open] (radioctl-core) Opens a serial port on a caller-supplied runtime handle and returns the byte stream, control lines and status.
- [@ANCHOR: radioctl:radioctl_splitter_civ_status_frame] (radioctl-splitter) An OK or NG frame to the sender.
- [@ANCHOR: radioctl:radioctl_splitter_personality] (radioctl-splitter) A new personality for the configured protocol.
- [@ANCHOR: radioctl:radioctl_splitter_set] (radioctl-splitter) A set: asks the rig and reports done or refused.
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_code] (radioctl-splitter) The TS-2000 mode digit for a mode.
- [@ANCHOR: radioctl:radioctl_splitter_arbiter_try_key] (radioctl-splitter) Claims the transmitter for `client`. `Ok` when it was free or already theirs; `Err` with the
- [@ANCHOR: radioctl:radioctl_splitter_arbiter_release] (radioctl-splitter) Gives the transmitter up if `client` holds it. Returns whether it did.
- [@ANCHOR: radioctl:radioctl_splitter_arbiter_clear] (radioctl-splitter) Clears ownership because the transmitter was unkeyed by anyone (unkeying is always allowed).
- [@ANCHOR: radioctl:radioctl_splitter_arbiter_owner] (radioctl-splitter) The current owner, if any.
- [@ANCHOR: radioctl:radioctl_splitter_civ_new] (radioctl-splitter) A personality answering at CI-V `address` (0x94 for the IC-7300), with the bus echo on or
- [@ANCHOR: radioctl:radioctl_splitter_civ_mode_code] (radioctl-splitter) The CI-V mode byte for a mode; data modes use the sideband they sit on.
- [@ANCHOR: radioctl:radioctl_splitter_civ_classify] (radioctl-splitter) Turns one frame addressed to us into an action.
- [@ANCHOR: radioctl:radioctl_splitter_civ_parse] (radioctl-splitter) Parses CI-V frames from a client: echoes them if asked, ignores other stations, classifies the rest.
- [@ANCHOR: radioctl:radioctl_splitter_civ_reply] (radioctl-splitter) Renders a CI-V reply (value, OK or NG) addressed to the sender.
- [@ANCHOR: radioctl:radioctl_splitter_civ_unsolicited] (radioctl-splitter) The transceive frames for a VFO A or mode change.
- [@ANCHOR: radioctl:radioctl_splitter_civ_reset] (radioctl-splitter) Forgets a partial frame when the client disconnects.
- [@ANCHOR: radioctl:radioctl_splitter_pty_path] (radioctl-splitter) The path programs should open: the symlink if one was asked for, else the slave device.
- [@ANCHOR: radioctl:radioctl_splitter_pty_slave_path] (radioctl-splitter) The slave device itself (`/dev/pts/N`).
- [@ANCHOR: radioctl:radioctl_splitter_pty_drop] (radioctl-splitter) Stops the endpoint threads and removes the symlink.
- [@ANCHOR: radioctl:radioctl_splitter_pty_attach] (radioctl-splitter) Creates a pty, attaches it to `splitter` as a persistent client, and optionally links
- [@ANCHOR: radioctl:radioctl_splitter_pty_read_master] (radioctl-splitter) Master to async side. A read error is the program closing the slave: report one disconnect and
- [@ANCHOR: radioctl:radioctl_splitter_pty_write_master] (radioctl-splitter) Async side to master. Bytes written while no program has the slave open are dropped.
- [@ANCHOR: radioctl:radioctl_splitter_config_new] (radioctl-splitter) Defaults: cached reads up to 5 seconds old, 2 second deadline for the rig.
- [@ANCHOR: radioctl:radioctl_splitter_client_wait] (radioctl-splitter) Waits until the client's task has ended.
- [@ANCHOR: radioctl:radioctl_splitter_client_abort] (radioctl-splitter) Stops the client's task. Its PTT ownership is not released by this; drop the stream to
- [@ANCHOR: radioctl:radioctl_splitter_new] (radioctl-splitter) A splitter for `rig`.
- [@ANCHOR: radioctl:radioctl_splitter_stats] (radioctl-splitter) A snapshot of the counters.
- [@ANCHOR: radioctl:radioctl_splitter_ptt_owner] (radioctl-splitter) The client that owns the transmitter, if any.
- [@ANCHOR: radioctl:radioctl_splitter_attach_channels] (radioctl-splitter) Attaches a client given as channels. With `persistent` a `ClientInput::Disconnected`
- [@ANCHOR: radioctl:radioctl_splitter_attach_stream] (radioctl-splitter) Attaches a client that is any byte stream (a TCP connection, an in-memory duplex). The end
- [@ANCHOR: radioctl:radioctl_splitter_serve_tcp] (radioctl-splitter) Accepts connections on `listener` until the returned task is aborted; each connection is a
- [@ANCHOR: radioctl:radioctl_splitter_cleanup] (radioctl-splitter) Gives up the client's PTT ownership, unkeying the rig if the client held it.
- [@ANCHOR: radioctl:radioctl_splitter_read] (radioctl-splitter) A read: the cached value if fresh enough, else asks the rig.
- [@ANCHOR: radioctl:radioctl_splitter_execute] (radioctl-splitter) Carries out one request for `client`, with PTT arbitration.
- [@ANCHOR: radioctl:radioctl_splitter_key] (radioctl-splitter) Keys for `client` if no other client owns the transmitter; the claim is given back if the
- [@ANCHOR: radioctl:radioctl_splitter_run_client] (radioctl-splitter) The task of one client: input, rig changes, until the client ends.
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_new] (radioctl-splitter) A TS-2000 personality with auto-information off.
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_classify] (radioctl-splitter) Turns one command (without its `;`) into an action.
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_parse] (radioctl-splitter) Splits a client's bytes into TS-2000 commands and classifies each.
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_reply] (radioctl-splitter) Renders a TS-2000 reply (`FA`, `MD`, `SM0`, nothing for a set, `?;` for a refusal).
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_unsolicited] (radioctl-splitter) `FA`/`FB`/`MD` pushes, only for a client that switched `AI1` on.
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_reset] (radioctl-splitter) Clears partial input and the auto-information flag on disconnect.
- [@ANCHOR: radioctl:radioctl_core_handle_execute_from] (radioctl-core) Runs a command like `execute_within`, recording the given source on the events it causes (the splitter passes `Splitter`).
- [@ANCHOR: radioctl:radioctl_core_dispatcher_start] (radioctl-core) Starts the dispatcher thread for `callback` with a queue of `capacity` events.
- [@ANCHOR: radioctl:radioctl_core_dispatcher_submit] (radioctl-core) Queues an event without ever waiting for the callback.
- [@ANCHOR: radioctl:radioctl_core_dispatcher_counters] (radioctl-core) A snapshot of the counters.
- [@ANCHOR: radioctl:radioctl_core_event_merge] (radioctl-core) Folds a later event of the same rig into this one. Per field the earliest `old` and the
- [@ANCHOR: radioctl:radioctl_core_poll_for_capabilities] (radioctl-core) Builds the schedule for the fields the capabilities allow, first due at `now`.
- [@ANCHOR: radioctl:radioctl_core_poll_next_due] (radioctl-core) The earliest due time, if any field is polled.
- [@ANCHOR: radioctl:radioctl_core_poll_take_due] (radioctl-core) The entry to poll now, if one is due. While the rig transmits only transmit state and
- [@ANCHOR: radioctl:radioctl_core_poll_report] (radioctl-core) Records the outcome of polling `field`: a change resets the interval to its minimum, no
- [@ANCHOR: radioctl:radioctl_core_poll_note_transceive] (radioctl-core) Notes that the rig reports on its own; polling slows to the maximum interval.
- [@ANCHOR: radioctl:radioctl_core_poll_interval] (radioctl-core) The current interval of `field`, for tests and diagnostics.
- [@ANCHOR: radioctl:radioctl_core_rig_options_with_callback] (radioctl-core) Sets the status-change callback. It runs on a dedicated thread, never on the actor.
- [@ANCHOR: radioctl:radioctl_core_transmit_permit_new] (radioctl-core) A permit recording who granted it (an operator, an attestation, a test name).
- [@ANCHOR: radioctl:radioctl_core_transmit_permit_granted_by] (radioctl-core) Who granted the permit.
- [@ANCHOR: radioctl:radioctl_core_rig_spawn] (radioctl-core) Spawns a rig actor on a caller-supplied tokio runtime Handle and returns its clonable handle.
- [@ANCHOR: radioctl:radioctl_core_rig_spawn_current] (radioctl-core) Spawns on the runtime of the calling task; fails if there is none.
- [@ANCHOR: radioctl:radioctl_core_rig_owned_shutdown] (radioctl-core) Stops an owned runtime without waiting.
- [@ANCHOR: radioctl:radioctl_core_rig_spawn_with_owned_runtime] (radioctl-core) Feature `owned-runtime` only: builds a one-worker runtime of its own for radioctld and the C ABI.
- [@ANCHOR: radioctl:radioctl_core_handle_id] (radioctl-core) The rig's identifier.
- [@ANCHOR: radioctl:radioctl_core_handle_capabilities] (radioctl-core) What the rig can do.
- [@ANCHOR: radioctl:radioctl_core_handle_open_gate] (radioctl-core) Opens the transmit gate for this rig only. It starts closed.
- [@ANCHOR: radioctl:radioctl_core_handle_close_gate] (radioctl-core) Closes the transmit gate. Transmit commands still queued are refused when they run.
- [@ANCHOR: radioctl:radioctl_core_handle_gate_state] (radioctl-core) Who opened the gate, or `None` while it is closed.
- [@ANCHOR: radioctl:radioctl_core_handle_stats] (radioctl-core) A snapshot of the counters.
- [@ANCHOR: radioctl:radioctl_core_handle_link_up] (radioctl-core) False once the transport has failed.
- [@ANCHOR: radioctl:radioctl_core_handle_events] (radioctl-core) An async stream of status changes, starting now.
- [@ANCHOR: radioctl:radioctl_core_handle_cached] (radioctl-core) The cached value of a field with its age, without touching the rig.
- [@ANCHOR: radioctl:radioctl_core_handle_execute] (radioctl-core) Runs a command with the default deadline.
- [@ANCHOR: radioctl:radioctl_core_handle_execute_within] (radioctl-core) Runs a command that must finish within `timeout`. Unkeying the transmitter takes the
- [@ANCHOR: radioctl:radioctl_core_handle_frequency] (radioctl-core) A VFO's frequency.
- [@ANCHOR: radioctl:radioctl_core_handle_set_frequency] (radioctl-core) Sets a VFO's frequency.
- [@ANCHOR: radioctl:radioctl_core_handle_mode] (radioctl-core) The mode.
- [@ANCHOR: radioctl:radioctl_core_handle_set_mode] (radioctl-core) Sets the mode.
- [@ANCHOR: radioctl:radioctl_core_handle_ptt] (radioctl-core) The transmit state.
- [@ANCHOR: radioctl:radioctl_core_handle_set_ptt] (radioctl-core) Keys (needs the transmit gate open) or unkeys (always allowed, jumps the queue).
- [@ANCHOR: radioctl:radioctl_core_handle_signal] (radioctl-core) The S-meter in raw units.
- [@ANCHOR: radioctl:radioctl_core_event_stream_next] (radioctl-core) The next event, or `None` once the rig has stopped. A subscriber that falls behind skips
- [@ANCHOR: radioctl:radioctl_core_actor_run] (radioctl-core) The actor loop: emergency channel first, then ordinary commands, unsolicited bytes from the rig, and polling last.
- [@ANCHOR: radioctl:radioctl_core_actor_serve] (radioctl-core) Runs one command and answers its caller; counts successes, failures and preemptions.
- [@ANCHOR: radioctl:radioctl_core_actor_run_command] (radioctl-core) Executes one command: deadline and gate checks, write, wait for the reply with retries, preemption by an emergency command, typed errors with the transcript tail.
- [@ANCHOR: radioctl:radioctl_core_actor_observe] (radioctl-core) Updates the cache and, if the value changed, emits an event to the stream and the callback dispatcher.
- [@ANCHOR: radioctl:radioctl_core_actor_poll_once] (radioctl-core) Polls the next due field at lowest priority and adapts its interval from whether it changed.
- [@ANCHOR: radioctl:radioctl_virtual_io_spawn] (radioctl-protocols) Starts `rig` on `runtime`; returns the stream to hand to an actor and a control handle.
- [@ANCHOR: radioctl:radioctl_virtual_io_state] (radioctl-protocols) A copy of the rig's current state.
- [@ANCHOR: radioctl:radioctl_virtual_io_front_panel] (radioctl-protocols) Turns a knob on the rig; any unsolicited bytes go out without delay.
- [@ANCHOR: radioctl:radioctl_virtual_io_set_reply_delay] (radioctl-protocols) Changes how long answers take from now on.
- [@ANCHOR: radioctl:radioctl_virtual_io_bytes_received] (radioctl-protocols) How many bytes the controller has written to the rig.
- [@ANCHOR: radioctl:radioctl_core_actor_absorb] (radioctl-core) Feeds received bytes to the engine; decides the waiting command, updates the cache, counts unrecognised bytes.
- [@ANCHOR: radioctl:radioctl_core_actor_apply_set] (radioctl-core) Records a successful set in the cache.
- [@ANCHOR: radioctl:radioctl_core_actor_await_reply] (radioctl-core) Waits for the reply until the attempt deadline; an emergency command abandons the wait.
- [@ANCHOR: radioctl:radioctl_core_actor_log] (radioctl-core) Appends to the 16-line transcript kept for error reports.
- [@ANCHOR: radioctl:radioctl_core_actor_spacing] (radioctl-core) Enforces the description's minimum gap between commands.
- [@ANCHOR: radioctl:radioctl_core_handle_read] (radioctl-core) Cached or fresh read of a field, by freshness.
- [@ANCHOR: radioctl:radioctl_core_handle_submit] (radioctl-core) Checks capability and transmit gate, then queues the command without waiting (full queue is a typed error).
- [@ANCHOR: radioctl:radioctl_core_actor_unkey_fail_closed] (radioctl-core) Writes an unkey and waits for its answer, not preemptible: used when a transmit command was written but never answered (the rig may be keyed) and when the actor stops.
- [@ANCHOR: radioctl:radioctl_core_actor_unkey_on_stop] (radioctl-core) When the last handle is dropped, a rig whose cache says keyed or whose gate was open is unkeyed before the actor ends.
- [@ANCHOR: radioctl:radioctl_core_deadline_after] (radioctl-core) Saturating `Instant + Duration`: an absurd timeout (a caller's `Duration::MAX`, a rig file's timing) never panics the actor or the caller.
- [@ANCHOR: radioctl:radioctl_core_wait_until] (radioctl-core) Sleeps until the next poll is due, or forever when nothing is polled.
- [@ANCHOR: radioctl:radioctl_core_dispatcher_run] (radioctl-core) The callback thread loop: pop, call under catch_unwind, count.
- [@ANCHOR: radioctl:radioctl_core_capability_name] (radioctl-core) The name used in rig description files.
- [@ANCHOR: radioctl:radioctl_core_capability_from_name] (radioctl-core) The capability with a file-format name.
- [@ANCHOR: radioctl:radioctl_core_rigfile_parse] (radioctl-core) Parses and validates a description file. Any unknown field, unknown name, duplicate,
- [@ANCHOR: radioctl:radioctl_core_rigfile_expected_keys] (radioctl-core) Every key a description must explain.
- [@ANCHOR: radioctl:radioctl_core_rigfile_fact_for] (radioctl-core) The fact that explains `key`: an exact one if present, else a wildcard.
- [@ANCHOR: radioctl:radioctl_core_rigfile_verification_summary] (radioctl-core) How many keys are in each verified state, keyed by state name.
- [@ANCHOR: radioctl:radioctl_core_rigfile_is_hardware_verified] (radioctl-core) True only when every key of the description is hardware-verified.
- [@ANCHOR: radioctl:radioctl_core_transcript_hex] (radioctl-core) Parses hex bytes written with optional spaces (`fe fe 94` or `fefe94`).
- [@ANCHOR: radioctl:radioctl_core_transcript_parse] (radioctl-core) Parses and validates a transcript file. A step needs either an `op` with its `tx`, or a
- [@ANCHOR: radioctl:radioctl_core_mode_name] (radioctl-core) The name used in rig description files.
- [@ANCHOR: radioctl:radioctl_core_mode_from_name] (radioctl-core) The mode with a file-format name.
- [@ANCHOR: radioctl:radioctl_core_vfo_name] (radioctl-core) The name used in rig description files.
- [@ANCHOR: radioctl:radioctl_core_vfo_from_name] (radioctl-core) The VFO with a file-format name.
- [@ANCHOR: radioctl:radioctl_conformance_engine_for] (radioctl-protocols) The engine for a description's protocol family.
- [@ANCHOR: radioctl:radioctl_conformance_replay] (radioctl-protocols) Replays a transcript against the engine and the virtual rig built from one description; reports every byte-level disagreement.
- [@ANCHOR: radioctl:radioctl_conformance_replay_solicited] (radioctl-protocols) Replays one command step: engine encoding, reply expectation and virtual rig answer against the transcript bytes.
- [@ANCHOR: radioctl:radioctl_core_rigfile_build_description] (radioctl-core) Builds the RigDescription from the parsed file: family, capabilities, defaults, timing, mode codes.
- [@ANCHOR: radioctl:radioctl_core_rigfile_build_facts] (radioctl-core) Builds the provenance facts; duplicates and empty references are errors.
- [@ANCHOR: radioctl:radioctl_core_rigfile_build_mode_codes] (radioctl-core) Builds the mode code table; duplicate modes or codes are errors.
- [@ANCHOR: radioctl:radioctl_core_rigfile_build_verified] (radioctl-core) Builds a verified state; each state requires its own evidence fields.
- [@ANCHOR: radioctl:radioctl_core_rigfile_check_coverage] (radioctl-core) Every key needs a fact and every fact a key.
- [@ANCHOR: radioctl:radioctl_core_rigfile_matches_key] (radioctl-core) Whether a fact key (exact or prefix wildcard) covers a description key.
- [@ANCHOR: radioctl:radioctl_core_transcript_build_expect] (radioctl-core) Builds the expected decoding of a transcript step.
- [@ANCHOR: radioctl:radioctl_core_transcript_build_op] (radioctl-core) Builds the command of a transcript step.
- [@ANCHOR: radioctl:radioctl_core_transcript_build_panel] (radioctl-core) Builds the knob change of an unsolicited transcript step.
- [@ANCHOR: radioctl:radioctl_conformance_check_decoding] (radioctl-protocols) Checks that the engine decodes a step's rx exactly as the transcript expects.
- [@ANCHOR: radioctl:radioctl_conformance_expected_inbound] (radioctl-protocols) Maps a transcript expectation to the engine's Inbound value.
- [@ANCHOR: radioctl:radioctl_conformance_panel_change] (radioctl-protocols) Maps a transcript knob change to the virtual rig's PanelChange.
- [@ANCHOR: radioctl:radioctl_core_rigfile_build_scope] (radioctl-core) Validates the optional `[scope]` block (icom-civ only; receivers 1 or 2, positive sizes, a span list).
- [@ANCHOR: radioctl:radioctl_civ_engine_scope_request] (radioctl-protocols) The request for a scope command (command 27). Fails when the model has no scope or a value does not fit its field.
- [@ANCHOR: radioctl:radioctl_civ_engine_take_scope_lines] (radioctl-protocols) The complete scope sweeps received since the last call.
- [@ANCHOR: radioctl:radioctl_civ_engine_scope_dropped] (radioctl-protocols) Scope frames or sweeps discarded as out of order or malformed.
- [@ANCHOR: radioctl:radioctl_civ_encode_rit_offset] (radioctl-protocols) The 3 data bytes of the RIT/XIT offset after `21 00`: tens and ones, thousands and hundreds, then a sign byte (1 is negative).
- [@ANCHOR: radioctl:radioctl_civ_decode_rit_offset] (radioctl-protocols) The offset in hertz from the 3 data bytes after `21 00`.
- [@ANCHOR: radioctl:radioctl_civ_filter_index_to_hz] (radioctl-protocols) The width in hertz of filter index `index` (command `1A 03`): 0 to 9 are 50 to 500 Hz in steps of 50, 10 to 40 are 600 to 3600 Hz in steps of 100 (SSB, CW, RTTY); for AM the index 0 to 49 is 200 to 10000 Hz in steps of 200. `None` outside the table.
- [@ANCHOR: radioctl:radioctl_civ_filter_hz_to_index] (radioctl-protocols) The filter index whose width is exactly `hz`, or `None` when the table has no such width.
- [@ANCHOR: radioctl:radioctl_civ_engine_decode_filter] (radioctl-protocols) The filter width for index byte `index`, by the mode last reported.
- [@ANCHOR: radioctl:radioctl_civ_engine_decode_scope] (radioctl-protocols) Feeds a waveform frame to the assembler; settings replies are not kept.
- [@ANCHOR: radioctl:radioctl_scope_mode_code] (radioctl-protocols) The wire code of the mode.
- [@ANCHOR: radioctl:radioctl_scope_mode_from_code] (radioctl-protocols) The mode for a wire code.
- [@ANCHOR: radioctl:radioctl_scope_speed_code] (radioctl-protocols) The wire code of the speed.
- [@ANCHOR: radioctl:radioctl_scope_speed_from_code] (radioctl-protocols) The speed for a wire code.
- [@ANCHOR: radioctl:radioctl_scope_command_is_query] (radioctl-protocols) True for the commands that read a setting (the rig answers with data, not OK).
- [@ANCHOR: radioctl:radioctl_scope_command_data] (radioctl-protocols) The sub-command and data bytes (after the `27`) for a scope command, or `None` when a value does not fit its field.
- [@ANCHOR: radioctl:radioctl_scope_decode_reference] (radioctl-protocols) The reference level, in hundredths of a dB, from the 3 data bytes after `27 19 <rx>`.
- [@ANCHOR: radioctl:radioctl_scope_parse_segment] (radioctl-protocols) Decodes the data after `27 00` (receiver, frame number, frame count, then the rest). `None` for a malformed frame.
- [@ANCHOR: radioctl:radioctl_scope_assembler_new] (radioctl-protocols) An assembler for a model's scope.
- [@ANCHOR: radioctl:radioctl_scope_assembler_dropped] (radioctl-protocols) How many frames or sweeps were thrown away as out of order, oversized or short.
- [@ANCHOR: radioctl:radioctl_scope_assembler_note_malformed] (radioctl-protocols) Counts a waveform frame that could not be decoded.
- [@ANCHOR: radioctl:radioctl_scope_assembler_push] (radioctl-protocols) Adds a frame; returns the sweep it completes, if any.
- [@ANCHOR: radioctl:radioctl_scope_sim_new] (radioctl-protocols) A scope in its power-on state: display on, output off, centred, narrowest span.
- [@ANCHOR: radioctl:radioctl_scope_sim_output_on] (radioctl-protocols) True when waveform output is switched on.
- [@ANCHOR: radioctl:radioctl_scope_sim_handle] (radioctl-protocols) Answers a scope command (the bytes after `27`).
- [@ANCHOR: radioctl:radioctl_scope_sim_sweep] (radioctl-protocols) The frames (each `27 00 ...` data, without the CI-V envelope) of one sweep of a receiver whose tuned frequency is `tuned`, or none when output is off, the receiver does not exist or hold is on. Samples are a deterministic ramp that moves each sweep, so a reassembled line can be checked sample by sample.
- [@ANCHOR: radioctl:radioctl_scope_sample_value] (radioctl-protocols) The deterministic synthetic amplitude of pixel `index` in sweep `sweep`.
- [@ANCHOR: radioctl:radioctl_virtual_civ_slot] (radioctl-protocols) The frequency of the selected VFO, or of the other one when `other` is true.
- [@ANCHOR: radioctl:radioctl_virtual_civ_set_freq] (radioctl-protocols) Sets the selected (or the other) VFO from 5 BCD bytes; NG on a malformed field.
- [@ANCHOR: radioctl:radioctl_virtual_civ_level] (radioctl-protocols) Answers `14 <sub>` with the level as 4-digit BCD; NG for a level the rig does not have.
- [@ANCHOR: radioctl:radioctl_virtual_civ_level_set] (radioctl-protocols) Sets a level from `14 <sub> <BCD>`; NG above 255 or on a non-BCD value.
- [@ANCHOR: radioctl:radioctl_virtual_civ_filter] (radioctl-protocols) Answers `1A 03` with the filter index for the current width and mode; NG when the width is not in the mode's table.
- [@ANCHOR: radioctl:radioctl_virtual_civ_filter_set] (radioctl-protocols) Sets the filter width from a `1A 03` index byte, by the mode's table.
- [@ANCHOR: radioctl:radioctl_virtual_scope_sweep] (radioctl-protocols) The bytes the rig sends on its own for one scope sweep of the Main receiver at the VFO A frequency, as broadcast frames; empty when the model has no scope, output is off or the scope is held.
- [@ANCHOR: radioctl:radioctl_virtual_civ_level_of] (radioctl-protocols) The level behind a `14` sub-command (AF 01, RF 02, power 0A, mic 0B, monitor 15).
- [@ANCHOR: radioctl:radioctl_civ_level_sub] (radioctl-protocols) Wire sub-command of a level under `14`, for the levels this engine supports.
- [@ANCHOR: radioctl:radioctl_civ_engine_filter_table_is_am] (radioctl-protocols) Internal helper of the CI-V engine or the scope module; see the source and its test.
- [@ANCHOR: radioctl:radioctl_civ_engine_encode_vfo_b_split] (radioctl-protocols) Internal helper of the CI-V engine or the scope module; see the source and its test.
- [@ANCHOR: radioctl:radioctl_civ_engine_encode_rit_xit] (radioctl-protocols) Internal helper of the CI-V engine or the scope module; see the source and its test.
- [@ANCHOR: radioctl:radioctl_civ_engine_encode_levels_and_filter] (radioctl-protocols) Internal helper of the CI-V engine or the scope module; see the source and its test.
- [@ANCHOR: radioctl:radioctl_scope_line_from] (radioctl-protocols) Internal helper of the CI-V engine or the scope module; see the source and its test.
- [@ANCHOR: radioctl:radioctl_scope_sim_try_handle] (radioctl-protocols) Internal helper of the CI-V engine or the scope module; see the source and its test.
- [@ANCHOR: radioctl:radioctl_kenwood_engine_decode_if] (radioctl-protocols) The observations in an `IF` answer (without `IF` and `;`): 35 characters, laid out as the module note says. `None` when the length or a field is wrong.
- [@ANCHOR: radioctl:radioctl_kenwood_level_wire] (radioctl-protocols) The wire form of a level. Ranges are the intersection of the TS-2000 and TS-590 guides.
- [@ANCHOR: radioctl:radioctl_kenwood_decode_level] (radioctl-protocols) A level answer: the command name and its digits (`AG0128` is name `AG`, digits `0128`).
- [@ANCHOR: radioctl:radioctl_kenwood_parse_if_offset] (radioctl-protocols) The RIT/XIT offset in the 10 characters that hold P2 and P3 of an `IF` answer: the sign nearest the end, then 4 or 5 digits to the end.
- [@ANCHOR: radioctl:radioctl_virtual_kenwood_if] (radioctl-protocols) The IF answer in the TS-2000 or TS-590 layout (38 characters).
- [@ANCHOR: radioctl:radioctl_virtual_kenwood_select_vfo] (radioctl-protocols) Sets the receive (FR) or transmit (FT) VFO; split is when they differ.
- [@ANCHOR: radioctl:radioctl_virtual_kenwood_move_offset] (radioctl-protocols) RU/RD: moves the RIT/XIT offset by five digits of hertz; refuses beyond 9999 Hz.
- [@ANCHOR: radioctl:radioctl_virtual_kenwood_level] (radioctl-protocols) A Kenwood level read or set (AG, RG, MG, ML, PC, KS), refusing values outside the guides' ranges.
- [@ANCHOR: radioctl:radioctld_access_normalise] (radioctld) An IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) as the IPv4 address it stands for.
- [@ANCHOR: radioctl:radioctld_access_prefix_matches] (radioctld) True when the first `prefix` bits of `a` and `b` agree.
- [@ANCHOR: radioctl:radioctld_access_octets] (radioctld) The address bytes of an IP address.
- [@ANCHOR: radioctl:radioctld_access_loopback_only] (radioctld) The default: this machine only (`127.0.0.0/8` and `::1`).
- [@ANCHOR: radioctl:radioctld_access_parse] (radioctld) Builds a list from entries such as `192.168.1.0/24`, `10.0.0.5` or `fd00::/8`.
- [@ANCHOR: radioctl:radioctld_access_allows] (radioctld) True when `peer` is inside an allowed network.
- [@ANCHOR: radioctl:radioctld_caps_modes] (radioctld) The modes of the description, in a stable order.
- [@ANCHOR: radioctl:radioctld_caps_range_line] (radioctld) One frequency range line of `\dump_state`.
- [@ANCHOR: radioctl:radioctld_caps_gran] (radioctld) The `level_gran` text: min, max and step of every level, zeros for the ones not served.
- [@ANCHOR: radioctl:radioctld_caps_wire_float] (radioctld) A value as the `level_gran` text of Hamlib's `rigctld` writes it for the first levels (AF, RF):
- [@ANCHOR: radioctl:radioctld_caps_wire_int] (radioctld) An integer level value as the text Hamlib's client expects for an integer level: the integer's
- [@ANCHOR: radioctl:radioctld_caps_granularity] (radioctld) Min, max and step of one served level in the form the client parses for that level.
- [@ANCHOR: radioctl:radioctld_caps_dump_state] (radioctld) The `\dump_state` text.
- [@ANCHOR: radioctl:radioctld_caps_dump_caps] (radioctld) The `\dump_caps` text: a readable summary, ended by a blank line.
- [@ANCHOR: radioctl:radioctld_command_linebuffer_new] (radioctld) A buffer that accepts lines of at most `max` bytes.
- [@ANCHOR: radioctl:radioctld_command_linebuffer_has_partial] (radioctld) True while the start of a line has arrived but its newline has not.
- [@ANCHOR: radioctl:radioctld_command_linebuffer_push] (radioctld) Adds received bytes and returns the lines they completed. A line longer than the limit is
- [@ANCHOR: radioctl:radioctld_command_into_line] (radioctld) Text if the bytes are UTF-8 without control characters (a tab is fine), else invalid.
- [@ANCHOR: radioctl:radioctld_command_resolve] (radioctld) The canonical long name of a command word, and whether radioctld implements it. `None` when
- [@ANCHOR: radioctl:radioctld_command_takes_vfo] (radioctld) True for commands that accept an optional leading VFO token.
- [@ANCHOR: radioctl:radioctld_command_parse_hz] (radioctld) A frequency in hertz from decimal text such as `14074000` or `14074000.000000`; a fraction of
- [@ANCHOR: radioctl:radioctld_command_parse_int] (radioctld) A signed integer of at most 12 characters.
- [@ANCHOR: radioctl:radioctld_command_parse_float] (radioctld) A finite floating-point number of at most 32 characters.
- [@ANCHOR: radioctl:radioctld_command_sanitise] (radioctld) Text safe to echo in a response header: control characters become `?`.
- [@ANCHOR: radioctl:radioctld_command_morse_text] (radioctld) Morse text for `send_morse`: 1 to [`MAX_MORSE_CHARS`] printable ASCII characters.
- [@ANCHOR: radioctl:radioctld_command_mode_and_passband] (radioctld) `mode passband` arguments.
- [@ANCHOR: radioctl:radioctld_command_flag] (radioctld) A 0/1 flag; any nonzero integer is on.
- [@ANCHOR: radioctl:radioctld_command_build] (radioctld) Builds the command `name` from its arguments (the VFO token already taken off). `rest` is the
- [@ANCHOR: radioctl:radioctld_command_take_vfo] (radioctld) Splits an optional leading VFO token off `rest`. For `send_morse` only the unambiguous
- [@ANCHOR: radioctl:radioctld_command_parse_line] (radioctld) Parses one request line. `None` for a blank line (nothing to answer).
- [@ANCHOR: radioctl:radioctld_config_in_range] (radioctld) A value checked against an inclusive range, naming the key in the error.
- [@ANCHOR: radioctl:radioctld_config_server] (radioctld) Validates the `[server]` section.
- [@ANCHOR: radioctl:radioctld_config_transport] (radioctld) Validates a rig's `[rig.transport]`.
- [@ANCHOR: radioctl:radioctld_config_rig] (radioctld) Validates one `[[rig]]`. `index` picks the default port; `non_loopback_ok` is the server's
- [@ANCHOR: radioctl:radioctld_config_parse] (radioctld) Parses and validates a configuration file's text. Relative description paths are taken
- [@ANCHOR: radioctl:radioctld_daemon_start] (radioctld) Brings up every rig of `config` (descriptions given in the same order) and starts listening.
- [@ANCHOR: radioctl:radioctld_daemon_start_one] (radioctld) Brings up one rig and its listener.
- [@ANCHOR: radioctl:radioctld_daemon_stop] (radioctld) Stops every listener and client, and unkeys any rig whose transmitter a client held.
- [@ANCHOR: radioctl:radioctld_main_parse_args] (radioctld) Parses the command line (without the program name).
- [@ANCHOR: radioctl:radioctld_main_load] (radioctld) Reads the configuration file and every rig description it names.
- [@ANCHOR: radioctl:radioctld_main_run] (radioctld) Runs the daemon until the process is interrupted.
- [@ANCHOR: radioctl:radioctld_main] (radioctld) The daemon entry point: reads the command line, loads the configuration, runs the daemon on the one runtime it owns.
- [@ANCHOR: radioctl:radioctld_names_mode_from_hamlib] (radioctld) The mode a Hamlib mode name stands for (case-insensitive).
- [@ANCHOR: radioctl:radioctld_names_mode_to_hamlib] (radioctld) Hamlib's name for a mode.
- [@ANCHOR: radioctl:radioctld_names_mode_mask] (radioctld) The bit mask of a set of modes, as `\dump_state` writes it.
- [@ANCHOR: radioctl:radioctld_names_mode_bit] (radioctld) The single mask bit of one mode.
- [@ANCHOR: radioctl:radioctld_names_nominal_width] (radioctld) A typical receive bandwidth for the mode, reported when the rig cannot tell its own.
- [@ANCHOR: radioctl:radioctld_names_parse_vfo] (radioctld) The VFO a token names, if it is a VFO name at all (case-insensitive).
- [@ANCHOR: radioctl:radioctld_names_vfo_name] (radioctld) Hamlib's name for a VFO.
- [@ANCHOR: radioctl:radioctld_names_level_lookup] (radioctld) Finds a level by its Hamlib name (case-insensitive).
- [@ANCHOR: radioctl:radioctld_names_level_range] (radioctld) The rig's own range for a level, when radioctld knows it for the rig's family. Levels of
- [@ANCHOR: radioctl:radioctld_names_served_levels] (radioctld) The levels this rig can serve: its capabilities, and a known range for numeric levels.
- [@ANCHOR: radioctl:radioctld_names_func_lookup] (radioctld) Finds a function by its Hamlib name (case-insensitive).
- [@ANCHOR: radioctl:radioctld_names_func_mask] (radioctld) The `has_get_func` / `has_set_func` mask for a rig that has RIT and XIT or neither.
- [@ANCHOR: radioctl:radioctld_names_level_mask] (radioctld) The `has_get_level` / `has_set_level` mask of a list of levels; `settable_only` leaves out
- [@ANCHOR: radioctl:radioctld_reply_error_code] (radioctld) The `RPRT` code for a failure of the rig actor.
- [@ANCHOR: radioctl:radioctld_reply_vfo_echo] (radioctld) The VFO text a response header echoes: what the client sent, else `currVFO` in VFO mode.
- [@ANCHOR: radioctl:radioctld_reply_header_line] (radioctld) The extended-format header line: `name:` then the VFO and arguments.
- [@ANCHOR: radioctl:radioctld_reply_render] (radioctld) Renders the answer to one request.
- [@ANCHOR: radioctl:radioctld_rigs_boxed_encode] (radioctld) See the source.
- [@ANCHOR: radioctl:radioctld_rigs_boxed_feed] (radioctld) See the source.
- [@ANCHOR: radioctl:radioctld_rigs_load_description] (radioctld) Reads and validates a rig description file.
- [@ANCHOR: radioctl:radioctld_rigs_service_config] (radioctld) The per-rig service settings from the configuration.
- [@ANCHOR: radioctl:radioctld_rigs_build_rig] (radioctld) Brings up one rig on `runtime`: engine, transport, actor, service. The transmit gate is
- [@ANCHOR: radioctl:radioctld_server_limits_from_config] (radioctld) The limits of a configuration.
- [@ANCHOR: radioctl:radioctld_server_serve] (radioctld) Starts serving `service` on `listener`. The caller binds the listener (loopback unless the
- [@ANCHOR: radioctl:radioctld_server_local_addr] (radioctld) The address clients connect to.
- [@ANCHOR: radioctl:radioctld_server_stats] (radioctld) A snapshot of the counters.
- [@ANCHOR: radioctl:radioctld_server_shutdown] (radioctld) Stops accepting, disconnects every client (a client that owned the transmitter unkeys the
- [@ANCHOR: radioctl:radioctld_server_accept_loop] (radioctld) Accepts connections until aborted. Peers outside the allow-list are dropped before a byte is
- [@ANCHOR: radioctl:radioctld_server_answer] (radioctld) The text to send for one request line, and whether the connection closes after it.
- [@ANCHOR: radioctl:radioctld_server_render_invalid] (radioctld) The answer to a line that is not text.
- [@ANCHOR: radioctl:radioctld_server_write] (radioctld) Writes a reply within the write timeout; false when the client is gone or stalled.
- [@ANCHOR: radioctl:radioctld_server_run_client] (radioctld) The task of one client, until it leaves, misbehaves, or the server stops.
- [@ANCHOR: radioctl:radioctld_service_fmt_float] (radioctld) A level value as text: six decimals with trailing zeros dropped (`0.5`, `1`, `0.117647`).
- [@ANCHOR: radioctl:radioctld_service_new] (radioctld) A service for `rig`, described by `description`.
- [@ANCHOR: radioctl:radioctld_service_rig] (radioctld) The rig's handle.
- [@ANCHOR: radioctl:radioctld_service_config] (radioctld) The service settings.
- [@ANCHOR: radioctl:radioctld_service_stats] (radioctld) A snapshot of the counters.
- [@ANCHOR: radioctl:radioctld_service_ptt_owner] (radioctld) The client that owns the transmitter, if any.
- [@ANCHOR: radioctl:radioctld_service_caps_view] (radioctld) What `\dump_state` and `\dump_caps` are generated from.
- [@ANCHOR: radioctl:radioctld_service_disconnect] (radioctld) A client went away: if it owned the transmitter, give it up and unkey the rig.
- [@ANCHOR: radioctl:radioctld_service_shutdown] (radioctld) The server is stopping: if a client holds the transmitter, unkey the rig.
- [@ANCHOR: radioctl:radioctld_service_exec] (radioctld) Runs one operation on the rig's queue.
- [@ANCHOR: radioctl:radioctld_service_read] (radioctld) A read: the cached value if fresh enough, else asks the rig.
- [@ANCHOR: radioctl:radioctld_service_set] (radioctld) A set: asks the rig and reports done.
- [@ANCHOR: radioctl:radioctld_service_resolve_vfo] (radioctld) The VFO a selection means right now.
- [@ANCHOR: radioctl:radioctld_service_frequency] (radioctld) A VFO's frequency in hertz.
- [@ANCHOR: radioctl:radioctld_service_mode_and_width] (radioctld) The mode and the receive bandwidth in hertz (the mode's typical width when the rig has no
- [@ANCHOR: radioctl:radioctld_service_mode_fields] (radioctld) The `Mode` and `Passband` answer fields, with the given labels.
- [@ANCHOR: radioctl:radioctld_service_set_mode] (radioctld) Sets the mode, and the receive bandwidth when one is asked for and the rig has the
- [@ANCHOR: radioctl:radioctld_service_require_mode] (radioctld) A split transmit mode other than the current one cannot be set: radioctl rigs have one
- [@ANCHOR: radioctl:radioctld_service_refuse] (radioctld) Counts a refused transmit command and answers with the refusal code.
- [@ANCHOR: radioctl:radioctld_service_check_transmit] (radioctld) The fail-closed check in front of every command that can put RF on the air: the gate must
- [@ANCHOR: radioctl:radioctld_service_key] (radioctld) Keys the transmitter for `client` if the gate is open and no other client owns it. The
- [@ANCHOR: radioctl:radioctld_service_unkey] (radioctld) Unkeys the transmitter. Open to every client, gate open or not.
- [@ANCHOR: radioctl:radioctld_service_send_morse] (radioctld) Sends Morse text unless the gate is closed or another client owns the transmitter.
- [@ANCHOR: radioctl:radioctld_service_split_vfo] (radioctld) `s`: whether split is on and which VFO transmits.
- [@ANCHOR: radioctl:radioctld_service_set_split_vfo] (radioctld) `S`: split on means receive on VFO A and transmit on VFO B.
- [@ANCHOR: radioctl:radioctld_service_rit_offset] (radioctld) `j` and `z`: the offset in hertz while the function is on, else 0.
- [@ANCHOR: radioctl:radioctld_service_set_rit_offset] (radioctld) `J` and `Z`: 0 switches the function off; any other offset sets it and switches on.
- [@ANCHOR: radioctl:radioctld_service_served_level] (radioctld) The level info if this rig serves the named level.
- [@ANCHOR: radioctl:radioctld_service_get_level] (radioctld) `l`: a level as the client sees it (a 0 to 1 fraction, or the rig's own integer).
- [@ANCHOR: radioctl:radioctld_service_set_level] (radioctld) `L`: sets a numeric level; meters are read-only.
- [@ANCHOR: radioctl:radioctld_service_get_func] (radioctld) `u`: RIT or XIT on or off.
- [@ANCHOR: radioctl:radioctld_service_set_func] (radioctld) `U`: RIT or XIT on or off.
- [@ANCHOR: radioctl:radioctld_service_vfo_info] (radioctld) `\get_vfo_info`: frequency, mode, width and split of the selected VFO.
- [@ANCHOR: radioctl:radioctld_service_execute] (radioctld) Executes one command for `client`. The answer is the body of a success, or the `RPRT`
- [@ANCHOR: radioctl:radioctl_virtual_yaesu_command] (radioctl-protocols) The newcat command interpreter of a virtual Yaesu (FA/FB, MD0, TX, FT/ST, RT/XT/RC/RU/RD, SM0, RM5, IF, ID, AI, levels and the fixed answers Hamlib needs).
- [@ANCHOR: radioctl:radioctl_virtual_yaesu_set_frequency] (radioctl-protocols) Sets VFO A or B from nine digits; `?;` otherwise.
- [@ANCHOR: radioctl:radioctl_virtual_yaesu_if] (radioctl-protocols) The 28-character IF answer.
- [@ANCHOR: radioctl:radioctl_virtual_yaesu_move_offset] (radioctl-protocols) RU/RD: moves the clarifier offset by four digits of hertz; refuses beyond 9999 Hz.
- [@ANCHOR: radioctl:radioctl_virtual_yaesu_level] (radioctl-protocols) A newcat level read or set (AG0, RG0, MG, PC, KS), refusing values outside the guides' ranges.
- [@ANCHOR: radioctl:radioctl_yaesu_level_wire] (radioctl-protocols) The wire form and range of a level for the newcat family.
- [@ANCHOR: radioctl:radioctl_yaesu_decode_level] (radioctl-protocols) A newcat level answer (command name and digits).
- [@ANCHOR: radioctl:radioctl_yaesu_parse_clarifier] (radioctl-protocols) The clarifier offset in the five characters of IF P3: a sign and four digits.
- [@ANCHOR: radioctl:radioctl_yaesu_engine_new] (radioctl-protocols) Builds a newcat engine from a yaesu-newcat description.
- [@ANCHOR: radioctl:radioctl_yaesu_engine_decode] (radioctl-protocols) Decodes one newcat reply into observations.
- [@ANCHOR: radioctl:radioctl_yaesu_engine_decode_if] (radioctl-protocols) The observations in an IF answer (frequency of VFO A, mode, RIT, XIT, clarifier offset).
- [@ANCHOR: radioctl:radioctl_yaesu_engine_encode] (radioctl-protocols) Encodes an operation as a newcat command.
- [@ANCHOR: radioctl:radioctl_yaesu_engine_feed] (radioctl-protocols) Feeds received bytes to the newcat engine.
- [@ANCHOR: radioctl:radioctl_virtual_legacy_block] (radioctl-protocols) One five-byte block of a virtual FT-817/857/897: reads answer, settings change state, unknown opcodes are silent.
- [@ANCHOR: radioctl:radioctl_ylegacy_encode_bcd_frequency] (radioctl-protocols) The four BCD bytes of a frequency in 10 Hz units (multiples of 10 Hz below 1 GHz).
- [@ANCHOR: radioctl:radioctl_ylegacy_decode_bcd_frequency] (radioctl-protocols) The frequency from four BCD bytes of 10 Hz units.
- [@ANCHOR: radioctl:radioctl_ylegacy_engine_new] (radioctl-protocols) Builds a legacy five-byte engine from a yaesu-legacy description.
- [@ANCHOR: radioctl:radioctl_ylegacy_decode_frequency_mode] (radioctl-protocols) The frequency of VFO A and the mode from a five-byte 03 answer.
- [@ANCHOR: radioctl:radioctl_ylegacy_engine_encode] (radioctl-protocols) Encodes an operation as a five-byte block and remembers which answer to count bytes for.
- [@ANCHOR: radioctl:radioctl_ylegacy_engine_feed] (radioctl-protocols) Counts answer bytes (5 for frequency and mode, 1 for a status read) and decodes them.

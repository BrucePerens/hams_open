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
- Virtual rigs live in `radioctl-protocols::virtual_rig` and are sans-IO like the engines; the optional
  `io` module (cargo feature `tokio-io`, default on) runs one behind an in-memory duplex stream with a
  reply delay and a control handle. `radioctl-core` dev-depends on `radioctl-protocols` for its actor tests.
- `radioctl-splitter` (new crate, so the library crates stay free of pty code) depends on core and protocols; `radioctld` will use it.
- `radioctl-core` has the cargo feature `owned-runtime` (off by default, on for `radioctld`): the one place
  the library builds a tokio runtime of its own.
- No `deny.toml`: no other Rust crate in hams_open has one, so this workspace follows suit. Add one
  when the repository gets a shared policy.

## Build and test

```
CARGO_TARGET_DIR=$HOME/.cache/hams-cargo-target-radioctl nice -n 10 cargo fmt --check
CARGO_TARGET_DIR=... nice -n 10 cargo clippy --all-targets -- -D warnings
CARGO_TARGET_DIR=... nice -n 10 cargo test
```

Never a per-worktree `target/`; delete the target directory when the job is done (AGENTS.md).

## Rig descriptions and conformance

`rigs/*.toml` are rig description files (schema 1; format documented at the top of
`crates/radioctl-core/src/rigfile.rs`). Every datum must be explained by a `[[facts]]` entry with a
`provenance` (manual, capture, owner, repo-code, program-output) and a `verified` state (`unverified`,
`manual-checked`, `transcript-checked`, `hardware-verified` with radio, firmware, date and what was
tested); a file with an unexplained or unknown datum does not load. `rigs/transcripts/*.toml` are wire
transcripts replayed by `radioctl_protocols::conformance::replay` against the engine and the virtual rig
generated from the same description (tests in `crates/radioctl-protocols/tests/rig_files.rs`).

The worked descriptions are `rigs/icom-ic-7300.toml` and `rigs/elecraft-kx3.toml` (the KX3 one is written from Elecraft's published Programmer's Reference Rev. G5, cited by command heading and page since the manual has no numbered sections, with the PDF's sha256; its transcript is hand-written from the same manual). **Everything in it is unverified**, and its
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

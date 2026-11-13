# radioctl function anchors, batch 26

Anchors for the `radioctl-core` and `radioctl-protocols` crates of the radioctl daemon (ADR 0113 standard): the rig description file loader, the transcript reader, the mode and VFO name tables, the Kenwood `KY` (send CW text) parameters, the easy-start helpers and the conformance replay. Tests cite each anchor with `Tests [@ANCHOR: ...]`.
Transmit gate: nothing in this batch opens the transmit gate. The parsers only read text; the easy helpers return a rig with the gate closed, and the one start-up key operation they perform is an unkey, which needs no gate.


## `rigfile.rs`

### `fn parse(text: &str) -> Result<LoadedDescription, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_parse]

Parses the text of a rig description file (TOML) into a `LoadedDescription`, the typed `RigDescription` plus its provenance facts. `text` is borrowed and is the whole file contents; nothing is read from disk here (loading by id or path is `rigfile::load`). It is the single entry every description goes through, whether bundled or user supplied, so a rig is never driven from a half-understood file.
Steps: deserialize with `toml::from_str` (unknown fields, wrong types and duplicates are rejected by the schema structs); check `schema` equals the supported `SCHEMA` constant; build the description, build the facts, then check that every expected key has a fact and every fact a key. Any failure returns a `RigError` of kind `Malformed` with a sentence naming the problem. Pure: no I/O, no locks, no side effects, safe to call repeatedly.

### `fn expected_keys(description: &RigDescription) -> Vec<String>` [@ANCHOR: radioctl:radioctl_core_rigfile_expected_keys]

Lists every provenance key a description has to explain, as strings such as `rig.id`, `timing.retries`, `civ.address` or `scope.spans_hz`. `description` is borrowed read-only. The result is a fresh `Vec<String>` in a fixed order: a base set (rig identity, timing, defaults), then keys that depend on which optional blocks the description carries (CI-V, scope, power meter, non-default levels, Kenwood, and the other family blocks), so a rig is only asked to explain what it actually declares.
It is used by `check_coverage` (every key needs a fact, every fact a key) and by `verification_summary`, which counts states over these keys. It never fails and has no side effects. Optional keys appear only when their value departs from the default, for example `civ.frequency_bytes` only when it is not 5. The full capability and mode key lists come from further code in the same function; check it when adding a block.

### `fn fact_for(&self, key: &str) -> Option<&Fact>` [@ANCHOR: radioctl:radioctl_core_rigfile_fact_for]

Finds the provenance fact that explains one description key. `key` is a dotted key such as `capability.set-ptt`, borrowed; the result borrows from `self` and is `None` when no fact covers it. An exact key match is preferred; only if there is none does it fall back to the first fact whose key is a wildcard (`prefix.*`) that covers the key, by `matches_key`.
It lets a file state one fact for a whole group (for example all capabilities from one manual table) while a single key can still be overridden with its own, stricter fact, such as hardware verification of the PTT (push to talk) capability. Pure and read-only, linear in the number of facts, no errors. When several wildcards match, the first in file order wins.

### `fn verification_summary(&self) -> BTreeMap<&'static str, usize>` [@ANCHOR: radioctl:radioctl_core_rigfile_verification_summary]

Counts how many of the description's keys are in each verification state. The returned map is keyed by the state names `unverified`, `manual-checked`, `transcript-checked` and `hardware-verified`, with the count of keys in that state; a state with no keys is absent from the map. For each key from `expected_keys` it looks up the fact with `fact_for`; a key with no fact counts as `unverified`.
The summary is how the tooling and the status display report how much of a rig file is backed by evidence. It does not decide whether a transmit is allowed. Read-only, pure, deterministic (a `BTreeMap` orders the names). It cannot fail. A description that loaded through `parse` always has a fact for every key, so the `None` case only matters for hand-built values.

### `fn is_hardware_verified(&self) -> bool` [@ANCHOR: radioctl:radioctl_core_rigfile_is_hardware_verified]

Reports whether the whole description is hardware-verified: true only when `verification_summary` has exactly one entry and it is `hardware-verified`, meaning every expected key has a fact in that state (a real radio, firmware and date recorded). Any key left unverified, manual-checked or transcript-checked makes it false. Takes `&self`, no arguments.
It is a claim about evidence, used by tests and tooling to keep a file from advertising bench verification it does not have (the rig file tests assert that only files of radios actually run on a bench carry hardware-verified facts). Pure and read-only; never errors. Note that a description with zero expected keys would give an empty summary and so false.


## `transcript.rs`

### `fn parse_hex(text: &str) -> Result<Vec<u8>, RigError>` [@ANCHOR: radioctl:radioctl_core_transcript_hex]

Parses a hex byte string from a transcript file into bytes. `text` may contain spaces or other whitespace between digits, so `fe fe 94` and `fefe94` give the same three bytes; whitespace is removed first, then the digits are read in pairs, case-insensitively. An empty or all-whitespace string gives an empty vector.
Errors: an odd number of digits or any character that is not a hex digit returns a `Malformed` `RigError` whose detail quotes the offending text. Used for the `tx`, `rx` and similar byte fields of transcript steps, which record what was sent to and received from a real radio. Pure, allocates the result, no side effects.

### `fn parse(text: &str) -> Result<Transcript, RigError>` [@ANCHOR: radioctl:radioctl_core_transcript_parse]

Parses a transcript file (TOML) into a `Transcript`: the recorded dialogue between a controller and one radio, used for conformance replay. `text` is the file contents, borrowed. Returns the rig id, the origin (where the capture came from) and the typed steps, or a `Malformed` `RigError` naming the problem and, for step errors, the step index.
Rules enforced: `schema` must be 1; `origin` must not be blank and there must be at least one step. Each step must be one of three shapes: a solicited step (an `op` and its `tx` bytes), an unsolicited step (only a front-panel `panel` change), or, as step 0 only, `rx` alone as the radio's greeting (the prologue, such as a FlexRadio's version lines). An `expect` needs an `rx`, and `expect_more` needs an `expect`. Hex fields are parsed by `parse_hex`. No I/O, no side effects.


## `units.rs`

### `fn name(self) -> &'static str` [@ANCHOR: radioctl:radioctl_core_mode_name]

Returns the lowercase text name of an operating mode as it is written in rig description files and transcripts, such as `usb`, `cw-r`, `data-r` or `d-star`. `self` is a `Mode` passed by value (it is `Copy`). The names come from the `MODE_NAMES` table, which holds 15 modes, the inverse of `Mode::from_name`.
If a mode were ever missing from the table the function returns the string `unknown` instead of panicking; the round-trip test is what keeps the table complete. Pure, no allocation (returns a `'static` string), no errors.

### `fn from_name(name: &str) -> Option<Mode>` [@ANCHOR: radioctl:radioctl_core_mode_from_name]

Looks up an operating mode by its file-format name, the inverse of `Mode::name`. `name` is borrowed and must match exactly, case-sensitive, one of the entries of `MODE_NAMES` (`lsb`, `usb`, `am`, `cw`, `cw-r`, `rtty`, `rtty-r`, `fm`, `wfm`, `data`, `data-r`, `d-star`, `sam`, `sam-l`, `sam-u`).
Returns `Some(mode)` for a known name and `None` otherwise, with no trimming or case folding; callers that read files turn `None` into a `Malformed` error such as `unknown mode`. Pure, linear in 15 entries, no side effects.

### `fn name(self) -> &'static str` [@ANCHOR: radioctl:radioctl_core_vfo_name]

Returns the file-format name of a VFO (variable frequency oscillator, the tuning control): `a` for VFO A and `b` for VFO B, lowercase. `self` is a `Vfo` passed by value. Used when writing or comparing description and transcript fields that name a VFO.
It is the inverse of `Vfo::from_name`; the round-trip test checks that for both VFOs. Pure, returns a `'static` string, no errors, no allocation. Only these two VFOs exist in the type, so a rig's own list of usable VFOs is held separately in its description.

### `fn from_name(name: &str) -> Option<Vfo>` [@ANCHOR: radioctl:radioctl_core_vfo_from_name]

Looks up a VFO by its file-format name. `name` is borrowed and must be exactly `a` or `b`, lowercase; anything else, including `A`, ` a` or an empty string, gives `None`. Callers reading description or transcript files convert `None` into a `Malformed` error naming the unknown VFO.
It is the inverse of `Vfo::name`; the round-trip test covers both VFOs. Pure, constant time, borrowed input, no allocation, no side effects, no panics. Only the two VFOs of the type exist, so which VFOs a given rig actually offers comes from its description.


## `conformance.rs`

### `fn engine_for(description: &RigDescription) -> Result<Box<dyn ProtocolEngine>, RigError>` [@ANCHOR: radioctl:radioctl_conformance_engine_for]

Builds the protocol engine for a description's protocol family, as a boxed `ProtocolEngine`. `description` is borrowed; each engine gets its own clone. The family selects the engine: Icom CI-V, Kenwood ASCII (the TS-890 dialect gets its own engine, otherwise the classic one), Elecraft, Yaesu Newcat and legacy, Kenwood handheld, FlexRadio, HPSDR, radiod, serial vendor, receiver and USB device.
Errors: an engine constructor can refuse a description that lacks the block its family needs, and that `RigError` is returned unchanged; there are no panics. Constructing an engine does no I/O and opens no port, and it does not key a transmitter: the engine only encodes and decodes bytes. It is the single dispatch point used by the easy helpers, the daemon and the conformance replay.


## `easy.rs`

### `fn spawn_virtual(runtime: &Handle, description: RigDescription, options: RigOptions, reply_delay: Duration) -> Result<SpawnedVirtualRig, RigError>` [@ANCHOR: radioctl:radioctl_easy_spawn_virtual]

Builds a virtual (simulated) radio and its protocol engine from `description`, connects them over an in-memory stream and spawns the rig actor, returning a `SpawnedVirtualRig` that holds the rig handle and a control handle for the simulation. `runtime` is the tokio `Handle` the actor runs on (borrowed); `description` is taken by value; `options` is the `RigOptions` for the actor (timing, link watch and so on); `reply_delay` is how long the virtual radio waits before it answers, `Duration::ZERO` for at once.
Errors: the virtual rig or engine constructor, or the spawn, may fail and the `RigError` is returned. The transmit gate of the spawned rig starts closed, so a test must open it explicitly to key the virtual rig. No hardware is touched. Spawns tasks on the runtime; call it from a context where that runtime is alive.

### `fn spawn(which: &str) -> Result<SpawnedVirtualRig, RigError>` [@ANCHOR: radioctl:radioctl_easy_virtual_rig_spawn]

The one-call way to get a working virtual rig. `which` is borrowed and is either a rig id such as `icom-ic-7300` (looked up among the bundled rig files) or the path of a description file, as in `rigfile::load`. It takes the current tokio runtime, loads the description, and calls `spawn_virtual` with default `RigOptions` and a zero reply delay, so the stand-in answers at once.
Returns a `SpawnedVirtualRig` or the `RigError` from the runtime lookup (called outside a tokio runtime), from the load (unknown id, unreadable or malformed file), or from the spawn. The transmit gate starts closed. No hardware is touched; it replaces four separate calls in example code and tests.

### `async fn open_serial_rig_with(runtime: &Handle, description: &RigDescription, config: SerialConfig, mut options: RigOptions) -> Result<SerialRig, RigError>` [@ANCHOR: radioctl:radioctl_easy_open_serial_rig_with]

Opens a real serial radio. `runtime` is the tokio handle; `description` is borrowed and chooses the engine; `config` is the `SerialConfig` (port, baud, framing), taken by value; `options` is the `RigOptions`, taken mutably, so the function can set its `link_watch` to the port's watch (a port loss marks the link down, a reopen unkeys the rig). It is async because opening the port is.
It then spawns the rig actor and, if the description has the `SetPtt` capability, sends an unkey. The reason: a process killed while it had the radio keyed leaves the radio keyed. Unkeying needs no gate. If the unkey fails the error keeps its kind and says `the start-up unkey failed, the radio may be keyed`, and no rig is returned. The returned `SerialRig` has its transmit gate closed. Errors also come from the engine and the port open.

### `async fn open_serial_rig(which: &str, port: &str, baud: u32) -> Result<SerialRig, RigError>` [@ANCHOR: radioctl:radioctl_easy_open_serial_rig]

Convenience wrapper that opens a serial rig in one call. `which` is a rig id or a description file path (borrowed); `port` is the serial device path such as `/dev/ttyUSB0`; `baud` is the line speed in baud, with 8N1 framing and default `RigOptions`. It gets the current tokio runtime, loads the description, builds the `SerialConfig` including a description line for diagnostics, and calls `open_serial_rig_with`, so the start-up unkey and closed gate described there apply.
Errors: no tokio runtime, an unknown or malformed description (the error names it), or a port that cannot be opened (the error names the port). Async; talks to hardware, so it blocks the task only for the open and the unkey exchange.


## `conformance.rs`

### `fn replay(description: &RigDescription, transcript: &Transcript) -> Result<usize, Vec<String>>` [@ANCHOR: radioctl:radioctl_conformance_replay]

Replays a recorded transcript against the engine and the stand-in radio built from one description and reports every byte-level disagreement. `description` and `transcript` are borrowed. If `transcript.rig` differs from `description.id` it returns at once with one problem. Otherwise it builds the engine (`engine_for`) and the stand-in (`StandIn::for_description`) and walks the steps.
Solicited steps: the engine must encode the op to exactly `tx`, the reply expectation must agree with whether `rx` is empty, the stand-in fed `tx` must answer exactly `rx`, and the engine fed `rx` must decode what `expect` says. Unsolicited steps: the stand-in's front-panel change must send exactly `rx`, decoded as `expect`. A first rx-only step checks the prologue. Returns `Ok(number of steps)` or `Err(list of messages)`; it never panics on mismatch. No I/O; nothing is keyed.

### `fn replay_solicited(engine: &mut dyn ProtocolEngine, rig: &mut StandIn, op: &radioctl_core::op::Op, step: &radioctl_core::transcript::Step, index: usize, problems: &mut Vec<String>) -> bool` [@ANCHOR: radioctl:radioctl_conformance_replay_solicited]

Replays one command step of a transcript. `engine` and `rig` are mutated (their parse state advances); `op` is the command; `step` holds the recorded `tx`, `rx` and `expect`; `index` is the step number used in messages; `problems` is appended to. The engine must encode the op to exactly the recorded `tx` bytes, and its reply expectation (`ReplyKind::None` or not) must agree with whether `rx` is empty; an engine refusal is a problem.
It then feeds `tx` to the stand-in and requires the answer to equal `rx`, and calls `check_decoding` for the decoded side. Returns a `bool` saying whether the stand-in answered with the expected bytes (see the caller for how it is used); `unknown` beyond that, check the lines after the answer comparison. Never panics, no I/O; problems accumulate rather than abort.


## `rigfile.rs`

### `fn build_description(doc: &FileDoc) -> Result<RigDescription, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_build_description]

Turns the parsed file document into the typed `RigDescription`. `doc` is borrowed. It maps `rig.family` (`icom-civ`, `kenwood-ascii`, `elecraft-ascii`, `yaesu-newcat`, `yaesu-legacy`, `flex-smartsdr`, `hpsdr-network`, `radiod-channel`, `kenwood-hh`, `serial-vendor`, `receiver`, `usb-device`) to a `Family` and checks that exactly the family's own block is present and no other family block is, so the family and its blocks agree.
It then validates and builds each block: CI-V (`frequency_bytes` must be 4, 5 or 6), Kenwood (dialect, `KY`, scope link), Elecraft, Yaesu (`frequency_digits` 8 or 9), scope, levels, power meter, mode codes, timing and defaults. An unknown family, a mismatched block or an out-of-range value returns a `Malformed` `RigError` (message such as `the family block does not match rig.family`). Pure, no I/O. The remaining blocks are built by helper functions beyond this reading; check the source for the complete list.

### `fn build_ky(k: &KySection) -> Result<KyParams, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_build_ky]

Builds the Kenwood `KY` (send CW (continuous wave, Morse code) text) parameters from the `[kenwood.ky]` block. `k` is borrowed. `form` is `undocumented`, `padded` or `variable`; `undocumented` must stand alone (a `width`, `lowercase`, `at_sign` or `stop` alongside it is an error) and gives `KyParams::UNDOCUMENTED`, meaning the manual does not document `KY` for the model.
Otherwise `width` is required, 1 to 64 characters, and `stop` must be `none`, `ky0` or `blank`, how a message in progress is stopped. `lowercase` and `at_sign` say whether those characters are in the model's CW character table. Errors are `Malformed` `RigError`s naming the field. Pure, no I/O.


## `description.rs`

### `fn offers_send_cw(&self) -> bool` [@ANCHOR: radioctl:radioctl_core_ky_offers_send_cw]

Says whether a model gets the `send-cw` capability: true only when its `KY` form is documented (not `Undocumented`) and a way to stop a message is documented (the stop is not `None`). `&self`, no other arguments.
The rule exists because a CW message that cannot be stopped keys the transmitter for its whole length with no way to abort, so such models are not offered the capability. Pure, no errors, no side effects. The check also gates `encode_text`.

### `fn encode_text(&self, text: &str) -> Result<String, String>` [@ANCHOR: radioctl:radioctl_core_ky_encode_text]

Builds the wire bytes (as a `String`) of a `KY` send-CW message. `text` is borrowed and must be 1 to `width` characters, contain at least one non-space (an all-space field is a stop on some models), contain no `;` and use only characters in the model's table: upper case, digits, a fixed punctuation set, plus lower case and `@` when the model allows them.
Errors are returned as `Err(String)` sentences: model does not offer send-CW, wrong length, only spaces, or a character not in the table. The `Padded` form sends `KY <text padded to width>;` and the other form `KY2<text>;`. Pure; it builds bytes for a command that keys the transmitter when sent, but the transmit gate and band checks are applied by the layer that sends it, not here.

### `fn encode_stop(&self) -> Option<String>` [@ANCHOR: radioctl:radioctl_core_ky_encode_stop]

Gives the wire form of the command that stops a `KY` message in progress, or `None` when the model documents none. `&self`; the result is `KY0;` for the `ky0` stop, or `KY` followed by an all-space field of `width` characters and `;` for the `blank` stop.
Stopping is the safe direction (it ends a transmission), so it needs no gate. Pure, no errors. Callers send it when cancelling send-CW or unkeying; a model with `None` cannot offer send-CW at all (see `offers_send_cw`).


## `rigfile.rs`

### `fn build_facts(doc: &FileDoc) -> Result<Vec<Fact>, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_build_facts]

Builds the list of provenance facts from the file document. `doc` is borrowed. Each entry has a `key` (exact, or a `prefix.*` wildcard), a provenance reference saying where the claim comes from, and a verified state built by `build_verified`.
Errors (`Malformed` `RigError`): the same key appearing twice, or a provenance `reference` that is empty or only whitespace. Returns the facts in file order, which matters because `fact_for` picks the first matching wildcard. Pure, no I/O.

### `fn build_mode_codes(entries: &[ModeEntry], may_be_empty: bool) -> Result<Vec<(Mode, u8)>, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_build_mode_codes]

Builds the table that maps operating modes to the radio's own mode codes. `entries` are the file's mode rows (borrowed); `may_be_empty` says whether an empty table is allowed (families that do not use mode codes). Each row names a mode (via `Mode::from_name`) and a code that is a byte or a one-character string; a longer string is an error.
Duplicates are errors: the same mode twice, or the same code twice. The one exception is a sideband and its data variant (`data` with `usb`, `data-r` with `lsb`), which may share a code if the plain mode is listed first, as in CI-V where data is the sideband plus a DATA flag. An unknown mode name, or an empty table where one is required, is an error. Returns `Vec<(Mode, u8)>` in file order.

### `fn build_verified(key: &str, entry: &VerifiedEntry) -> Result<Verified, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_build_verified]

Builds the verification state of one fact. `key` is used only in messages; `entry` is the file's state and evidence fields. `unverified` needs nothing. `manual-checked` and `transcript-checked` each need a non-empty `reference`. `hardware-verified` needs `radio`, `firmware`, `date` and `tested`, all non-empty, and `date` must look like `YYYY-MM-DD`.
Each state therefore requires its own evidence, so a file cannot claim bench verification without naming the radio, firmware and day. An unknown state name or missing field returns a `Malformed` `RigError` naming the key. Pure, no I/O.

### `fn check_coverage(description: &RigDescription, facts: &[Fact]) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_check_coverage]

Checks that the facts and the description agree both ways. `description` and `facts` are borrowed. Every key from `expected_keys` must be covered by at least one fact (exact or wildcard, via `matches_key`), else the error `no provenance fact covers <key>`; and every fact must match at least one expected key, else `fact <key> matches no key of this description`, which catches typos and stale entries.
Returns `Ok(())` or the first `Malformed` `RigError`. It enforces the rule that nothing in a rig file is unexplained. Pure, quadratic in keys times facts, which is small.

### `fn matches_key(fact_key: &str, key: &str) -> bool` [@ANCHOR: radioctl:radioctl_core_rigfile_matches_key]

Tests whether a fact key covers a description key. `fact_key` is either an exact key, matched by equality, or ends in `.*`, a wildcard that matches every key beneath that prefix: `capability.*` covers `capability.set-ptt` but not `capability` itself nor `capabilityx.y`, because the remainder must start with a dot.
Returns a plain bool; `key` is the description key being explained. Both arguments are borrowed, the function is pure and has no errors or side effects. It is used by `fact_for`, to find the fact for a key, and by `check_coverage`, to prove every key has a fact and every fact a key.


## `transcript.rs`

### `fn build_expect(spec: ExpectSpec) -> Result<Expect, RigError>` [@ANCHOR: radioctl:radioctl_core_transcript_build_expect]

Converts the file's expectation (`ExpectSpec`, by value) of a transcript step into the typed `Expect`: what the engine must decode from the `rx` bytes. It covers observations (frequency with VFO and hertz, mode, PTT, signal, power, split, RX and TX VFO, RIT, XIT, RIT offset, filter width, level, SWR: standing wave ratio, meter, model id, firmware major and minor) and the plain `Ack` and `Nak`.
Names for VFO, mode, level and meter are looked up through the name tables; an unknown name returns a `Malformed` `RigError`. Pure, no I/O.

### `fn build_op(spec: OpSpec) -> Result<Op, RigError>` [@ANCHOR: radioctl:radioctl_core_transcript_build_op]

Converts the file's command (`OpSpec`, by value) of a transcript step into the typed `Op` the engine would encode, such as `GetFrequency`, `SetFrequency` (VFO and hertz), `SetMode`, `SetPtt`, `SendCw`, `SetLevel` and the get and set operations for split, RIT (receiver incremental tuning), XIT (transmitter incremental tuning) and filter width.
Names for VFO, mode, level and meter go through the name tables; an unknown one returns a `Malformed` `RigError`. Pure and transcripts are replayed only against stand-ins, so a `SetPtt` or `SendCw` step here never keys a real transmitter.

### `fn build_panel(spec: PanelSpec) -> Result<Panel, RigError>` [@ANCHOR: radioctl:radioctl_core_transcript_build_panel]

Converts the file's front-panel change (`PanelSpec`, by value) of an unsolicited step into the typed `Panel`: frequency in hertz, mode, PTT on or off, signal value, split, or RIT offset. These stand for the operator turning a knob or pressing a button on the radio.
An unknown mode name returns a `Malformed` `RigError`; the other variants cannot fail. The result is a value of the typed `Panel` enum, used by the conformance replay to make the stand-in radio act as if the knob had moved. Pure, no I/O; replayed only against stand-in radios, never a real one.


## `conformance.rs`

### `fn check_decoding(engine: &mut dyn ProtocolEngine, step: &radioctl_core::transcript::Step, index: usize, problems: &mut Vec<String>)` [@ANCHOR: radioctl:radioctl_conformance_check_decoding]

Checks the decoding half of a replayed step. `engine` is mutated by feeding it the step's `rx` bytes at the fixed test time `NOW`; `step` supplies `expect` and `expect_more`; `index` labels messages; `problems` is appended to. The list of `Inbound` events the engine returns must equal the expected observation or acknowledgement, followed by each `expect_more`, in order.
On a difference it pushes one message showing both lists. It does not return a value and never panics. No I/O.

### `fn for_description(description: &RigDescription) -> Result<StandIn, RigError>` [@ANCHOR: radioctl:radioctl_conformance_stand_in_for]

Chooses the radio stand-in a transcript replays against, by `description.family`. `description` is borrowed. Flex uses the virtual FlexRadio, serial vendor and USB device families use their own twins, HPSDR and radiod use small bridge stand-ins that start at the description's default frequency, and every other family uses the generic `VirtualRig`.
Errors: a stand-in constructor can refuse the description and the `RigError` is returned. No I/O. Used by `replay`.

### `fn prologue(&self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_conformance_stand_in_prologue]

Returns the bytes the stand-in radio sends before it is asked anything. `&self`; only the FlexRadio stand-in has one, its greeting with the version and handle lines, and every other stand-in returns an empty vector.
`replay` compares it with a transcript's first step when that step has no op and only `rx`. Pure, allocates the result, no errors.

### `fn feed(&mut self, bytes: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_conformance_stand_in_feed]

Gives the stand-in's reply to bytes the controller sent. `&mut self`, so the stand-in's state advances (a set command changes what a later get returns); `bytes` is borrowed. It dispatches to the feed function of the variant (generic rig, Flex, vendor twin, HPSDR bridge, radiod bridge, USB device) and returns the reply, which is empty when the stand-in stays silent.
The stand-in is simulated, so no hardware is touched. Time is fixed at `NOW` for the variants that take it.

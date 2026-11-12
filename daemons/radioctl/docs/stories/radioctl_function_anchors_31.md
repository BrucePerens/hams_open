# radioctl function anchors, batch 31

Function anchors for the `radioctld` crate (the Hamlib-compatible daemon): its name tables (`names.rs`), the response renderer (`reply.rs`), rig start-up (`rigs.rs`), the volunteer self-test (`selftest.rs`) and the candidate-description listing (`candidates.rs`). Each entry is written to the ADR 0113 standard and is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
None of these functions keys a transmitter. The self-test entries state explicitly that the transmit gate stays closed; the reply entries only translate an already-decided refusal (gate closed) into a Hamlib error code.

## `crates/radioctld/src/names.rs`

### `pub fn mode_bit(mode: Mode) -> u64` [@ANCHOR: radioctl:radioctld_names_mode_bit]

Returns the one-bit mask of a single mode in Hamlib's mode-mask numbering, for example USB gives 0x4 and LSB 0x8. It is a thin wrapper that calls `mode_mask` with a one-element slice, so the bit positions live in one table (`MODES`) only. The argument is a `radioctl_core::units::Mode` passed by value. A mode that has no row in the table yields 0, never a panic. Pure, no side effects; used when building the mode lists that `\dump_state` writes.

### `pub fn nominal_width(mode: Mode) -> u32` [@ANCHOR: radioctl:radioctld_names_nominal_width]

Gives a typical receive bandwidth in hertz for a mode, used as the passband that `get_mode` reports when the rig cannot say its own. Values come from the `MODES` table: 500 Hz for CW and RTTY, 2400 for USB and LSB, 3000 for the packet modes, 6000 for AM and the synchronous AM modes, 6250 for D-STAR, 15000 for FM and 230000 for wide FM. A mode with no table row returns 0, meaning "no typical width". Pure and total; it is a nominal figure, not a measured filter width.

### `pub fn parse_vfo(token: &str) -> Option<VfoSel>` [@ANCHOR: radioctl:radioctld_names_parse_vfo]

Decides whether one word of a client request names a VFO, so the command parser can tell `VFOB` from a frequency or a mode. Matching ignores case. `currVFO` gives `Current`; `VFOA`, `Main` and `MainA` give `A`; `VFOB` and `MainB` give `B`; `RX` and `TX` give `Rx` and `Tx`; the names Hamlib knows but radioctl rigs lack (`VFOC`, `Sub`, `SubA`, `SubB`, `MEM`, `Mem`, `None`) give `Absent` so the caller can answer "not available". Any other token, such as `14074000` or `USB`, returns `None`. Pure; never fails.

### `pub fn vfo_name(vfo: Vfo) -> &'static str` [@ANCHOR: radioctl:radioctld_names_vfo_name]

Hamlib's text for one of radioctl's two VFOs: `Vfo::A` is `VFOA` and `Vfo::B` is `VFOB`. The result is a static string in upper case, used when a response echoes a VFO or `\dump_state` names one. The argument is the core `Vfo` enum passed by value; the match is exhaustive, so there is no error or fallback value and a future third VFO would not compile until it is added here. Pure, no side effects.

### `pub fn level_lookup(name: &str) -> LevelLookup` [@ANCHOR: radioctl:radioctld_names_level_lookup]

Classifies a level name from a `get_level` or `set_level` request, ignoring case. It searches the `LEVELS` table first and returns `Known(info)` (a copy of the `LevelInfo`: name, mask bit, source and whether the client sees a 0..1 fraction). If the name is one of the 18 Hamlib levels radioctld never serves (`VOXDELAY`, `SQL`, `NB`, `TEMP_METER` and so on) it returns `Unserved`, which callers answer as "not available". Anything else is `Unknown`, answered as an invalid parameter. `Known` means radioctld can serve it, not that this rig has it: check `served_levels`. Pure.

### `pub fn level_range(description: &RigDescription, level: Level) -> Option<(u16, u16)>` [@ANCHOR: radioctl:radioctld_names_level_range]

Returns the inclusive (minimum, maximum) raw integer range the rig's protocol engine accepts for a numeric level, so a client's 0..1 fraction can be scaled and out-of-range values refused. `description` is borrowed read-only; `level` is the core `Level`. The attenuator and preamp ranges come from the description's own `[levels]` block (0 to the maximum attenuation in dB, 0 to the preamp step count) whatever the family. Otherwise the range depends on the family: Elecraft reads it from the engine, Icom CI-V is 0..255 except keyer speed (the CI-V WPM limits), Kenwood has a table (the TS-890S dialect has its own: no monitor level, keyer speed from 4), Yaesu Newcat asks the Yaesu engine, Flex uses 0..100 for AF gain, mic gain and RF power. Returns `None` where there is no level command (legacy Yaesu, Kenwood HH, HPSDR, radiod, vendor serial, receivers, USB devices) or the level is not supported for that family. The CI-V and Kenwood numbers are copies of private engine tables; the test `level_ranges_match_what_each_engine_accepts` pins them to the engines. Pure.

### `pub fn served_levels(description: &RigDescription) -> Vec<LevelInfo>` [@ANCHOR: radioctl:radioctld_names_served_levels]

Computes the list of levels this particular rig can serve, in table order, as an owned `Vec` of `LevelInfo`. The description is borrowed. A radio-sourced level needs the `Levels` capability, must not be listed as lacking in the `[levels]` block and must have a known range. `SWR` needs `ReadSwr` or `ReadMeters`; raw meters need `ReadMeters`; the power fraction needs `ReadPower`; power in watts also needs a power-meter calibration; `RAWSTR` needs `ReadSignal`; `STRENGTH` needs `ReadSignal` and a decibel table for the family (only Elecraft has one). The result drives the `has_get_level` and `has_set_level` masks and `\dump_state`. An empty vector is valid and means no levels. Pure, allocates once.

### `pub fn strength_db(description: &RigDescription, raw: u16) -> Option<i32>` [@ANCHOR: radioctl:radioctld_names_strength_db]

Converts the rig's raw signal-meter reading `raw` (the engine's unit, a `u16`) into decibels relative to S9, the unit Hamlib's `STRENGTH` level uses; S9 is 0, weaker signals are negative, stronger positive. Only the Elecraft family has a table (the `SM` scale, delegated to `radioctl_protocols::elecraft::strength_db`), so any other family returns `None`, and `served_levels` then does not offer `STRENGTH` at all. It may also return `None` from the Elecraft table for a value the table cannot map; the exact range of valid `raw` values is `unknown` here, check the Elecraft engine. Pure, no side effects.

### `pub fn func_lookup(name: &str) -> FuncLookup` [@ANCHOR: radioctl:radioctld_names_func_lookup]

Classifies a function (on/off switch) name from `get_func` or `set_func`, ignoring case. `RIT` and `XIT` are `Known` with a `FuncKind` of `Rit` or `Xit`, meaning radioctld serves them if the rig has RIT and XIT. The 30 other Hamlib function names it does not serve (`NB`, `VOX`, `TUNER`, `LOCK`, `MUTE` and so on) give `Unserved`, answered as "not available"; anything else gives `Unknown`, answered as an invalid parameter. Pure, no allocation, no side effects; a function switch is a setting, not a transmit command.

### `pub fn func_mask(rit_xit: bool) -> u64` [@ANCHOR: radioctl:radioctld_names_func_mask]

Builds the `has_get_func` and `has_set_func` bit mask that `\dump_state` and `\dump_caps` report. `rit_xit` is true when the rig description has RIT and XIT. If true the result has Hamlib's RIT bit (24) and XIT bit (31) set; if false the result is 0 because radioctld serves no other function. The argument is a plain bool by value and there is no error case. Pure; the same mask is used for both the get and the set direction because the two functions are always read-write.

### `pub fn level_mask(levels: &[LevelInfo], settable_only: bool) -> u64` [@ANCHOR: radioctl:radioctld_names_level_mask]

ORs together `1 << bit` for each level in the borrowed slice, giving the `has_get_level` or `has_set_level` mask for `\dump_state`. Pass the output of `served_levels`. With `settable_only` true, only levels whose source is a radio setting (`LevelSource::Radio`) count, because the SWR, power, S-meter and other meters are read-only; with false every listed level counts. An empty slice gives 0. Bits come from each level's own `bit` field (all below 64). Pure, no errors.

## `crates/radioctld/src/reply.rs`

### `pub fn error_code(error: &RigError) -> i32` [@ANCHOR: radioctl:radioctld_reply_error_code]

Maps a failure reported by the rig actor to the negative Hamlib `RPRT` code a client sees. Timeout gives -5 (`ETIMEOUT`); Rejected, TransmitGateClosed and TransmitFrequencyUnknown give -9 (`ERJCTED`), so a refused transmit looks like a rejected command; Unsupported gives -11 (`ENAVAIL`); LinkDown and Cancelled give -6 (`EIO`); Malformed gives -8 (`EPROTO`); QueueFull gives -14 (`EBUSBUSY`). The match is exhaustive, so a new error kind will not compile until mapped. The error is borrowed and only its kind is read, never its detail text. Pure. It reports a gate refusal; it does not decide it.

### `pub fn vfo_echo(sent: Option<&(String, VfoSel)>, vfo_mode: bool) -> Option<String>` [@ANCHOR: radioctl:radioctld_reply_vfo_echo]

Chooses the VFO text that an extended-format response header echoes back. `sent` is the VFO token the client supplied, with its parsed selection, if any; its original spelling is returned unchanged (cloned) so `vfob` is echoed as `vfob`. If the client sent none and the connection is in VFO mode (`vfo_mode` true) the result is `currVFO`. Otherwise it is `None` and the header shows no VFO. No errors; pure. It is called when the request is parsed, before the rig answers, so the echo does not depend on the outcome.

### `fn header_line(header: &Header) -> String` [@ANCHOR: radioctl:radioctld_reply_header_line]

Formats the first line of an extended-format (`+`) response: the command's long name and a colon, then the echo. The echo is the VFO if present, joined to the arguments with a colon when there are arguments (`VFOB:14074000`), or the arguments alone when there is no VFO; with nothing to echo the line is just `name:`. The line ends with a newline. The `Header` is borrowed; the function is private to `reply.rs` and called only by `render`. Pure, no errors. The layout matches Hamlib 4.6.2 `rigctld`, as observed.

### `pub fn render(header: &Header, outcome: &Result<Body, i32>) -> String` [@ANCHOR: radioctl:radioctld_reply_render]

Produces the complete text answer to one request, ready to write to the socket. `header` says whether extended format was asked for, the command name, VFO echo and arguments; `outcome` is either a `Body` or an already-mapped `RPRT` code. An error gives `RPRT <code>`. Done and Close give `RPRT 0`. Fields give one value per line (with `Label: value` in extended format), followed by `RPRT 0` in extended format or for `get_lock_mode`. Text is written as is, with `RPRT 0` only if the body asks. Extended format puts the header line first, and `chk_vfo` in extended format answers `ChkVFO: <value>` with no `RPRT`. Pure; no I/O.

## `crates/radioctld/src/rigs.rs`

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctld_rigs_boxed_encode]

The `encode` method of `BoxedEngine`, the daemon's wrapper that lets one `spawn` call serve every protocol family. It forwards to the wrapped `Box<dyn ProtocolEngine>` and returns exactly what the family engine returns: the bytes to send and how to expect the reply, or a `RigError` (for example Unsupported when the rig cannot do the operation). The operation is borrowed; the engine is mutably borrowed because encoding may advance engine state. Called by the rig actor for each client command. It does no I/O and adds no checks, so the transmit gate and unkey logic stay in the actor.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<radioctl_core::engine::Inbound>` [@ANCHOR: radioctl:radioctld_rigs_boxed_feed]

The `feed` method of `BoxedEngine`: hands bytes received from the radio to the wrapped engine and returns the events it decoded (replies, unsolicited updates, NAKs or unrecognised bytes), in order, as an owned vector. `bytes` is a borrowed slice that may hold a partial frame, which the engine buffers; `now` is the actor's monotonic time, which engines use for reply timeouts and expiry. An empty vector is normal. It has no error return and no side effect beyond the engine's own state; malformed input becomes an `Unrecognised` event, never a panic.

### `fn encode_lines(&mut self, op: &Op) -> Result<Option<Vec<radioctl_core::engine::LineStep>>, RigError>` [@ANCHOR: radioctl:radioctld_rigs_boxed_encode_lines]

The control-line hook of `BoxedEngine`, forwarded to the wrapped engine. For a receiver tuned by changing serial control lines (RTS, DTR, break) instead of sending bytes, `Ok(Some(steps))` is the ordered list the actor drives on the port; `Ok(None)` is the usual answer and means the command is an ordinary byte request. An `Err` is the engine's refusal. Without this forwarding the wrapper would use the trait default (always `None`) and silently drop such rigs' tuning, which is what the test guards against. A transmitting op is never driven this way; the actor refuses it. No I/O here.

### `fn link_restored(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctld_rigs_boxed_link_restored]

The link-restored hook of `BoxedEngine`, forwarded to the wrapped engine. After a serial or network link comes back, the engine forgets half-received frames and unanswered requests and returns, in order, the requests that re-establish its session (for example turning auto-information back on, or Flex subscriptions). The actor sends them once, after the fail-closed unkey and before the link-restored event. An empty vector means nothing to redo. The daemon's wrapper once omitted this method and so used the default (empty); forwarding it is the fix.

### `fn open_requests(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctld_rigs_boxed_open_requests]

The open hook of `BoxedEngine`, forwarded to the wrapped engine. It returns, in order, the requests that make a freshly opened rig ready to talk, such as the Icom PCR power-on commands that Hamlib also sends. The actor writes them once when it starts with the link up and before it serves any client command. A rig that needs nothing returns an empty vector, which is also the trait default. These requests never key the transmitter. Forwarding matters because without it the daemon's boxed engine would use the empty default and skip the open step.

### `pub fn load_description(path: &Path) -> Result<RigDescription, String>` [@ANCHOR: radioctl:radioctld_rigs_load_description]

Reads one rig description TOML file from disk and returns its validated `RigDescription` (the verification facts are dropped; only the description is kept). `path` is borrowed. Reading uses blocking `std::fs`, so call it at start-up, not from the hot path. Errors are strings that name the file: `cannot read <path>: <io error>` for a missing or unreadable file, or `<path>: <parse error>` for any unknown field, name, duplicate, missing block or incomplete verification record (rejected by `rigfile::parse`). It never writes anything and touches no hardware.

## `crates/radioctld/src/selftest.rs`

### `pub fn is_read_only(op: &Op) -> bool` [@ANCHOR: radioctl:radioctld_selftest_is_read_only]

The self-test's safety filter: true only for operations that read from the radio (frequency, mode, PTT state, signal, power, split, TX VFO, RIT and XIT and offset, filter width, levels, SWR, meters, model id, firmware, power amplifier), false for everything else including every set and any transmit or PTT command. It is a whitelist, so a new `Op` variant is refused until someone adds it here. The operation is borrowed; pure; no errors. `run_checks_logged` calls it before every execution, so even a planned write would be refused and the transmit gate is never an issue.

### `pub fn plan_checks(description: &RigDescription) -> Vec<(String, Op)>` [@ANCHOR: radioctl:radioctld_selftest_plan_checks]

Builds the ordered list of named read-only checks for what a rig description claims. Each pair is a report label (such as `read frequency A`) and the `Op` to run. The order is fixed: identify (model id, firmware, and the Elecraft power amplifier when levels are claimed), frequency A and B, mode, transmit state, S-meter, power, SWR, the ALC, comp, Vd and Id meters, split and TX VFO, RIT and XIT, filter width, then eight levels. Each check is added only when the matching capability is present, so an empty plan is possible. The description is borrowed. Every planned op passes `is_read_only`. Pure.

### `pub fn redact_target(target: &str) -> String` [@ANCHOR: radioctl:radioctld_selftest_redact_target]

Prepares the port target for the report header so the report can be shared without leaking a device serial number. A target `usb:VID:PID:SERIAL` becomes `usb:VID:PID:<serial removed>`; `usb:VID:PID` is kept as is; a device path such as `/dev/ttyUSB0` or the word `virtual` is returned unchanged. The ids are not validated here. Anything after the second colon counts as the serial, even if it contains colons. Pure, allocates a new string, no errors.

### `pub fn utc_stamp(unix_seconds: u64) -> String` [@ANCHOR: radioctl:radioctld_selftest_utc_stamp]

Formats a Unix time (whole seconds since 1970-01-01 UTC) as `YYYY-MM-DD HH:MM:SS UTC` for the report header. It splits seconds into days and time of day, then converts days to a civil date with the integer proleptic-Gregorian algorithm, so it needs no time library and no time zone database, and leap years come out right. The year is zero-padded to four digits. Every `u64` is accepted for the arithmetic (extreme values give a distant year); there is no error. Pure; the caller supplies the clock reading.

### `pub fn render_report(header: &ReportHeader, results: &[CheckResult], wire: &str) -> String` [@ANCHOR: radioctl:radioctld_selftest_render_report]

Builds the plain-text report that the volunteer attaches to a bug report. Contents in order: a title saying nothing was transmitted, radioctl version and OS/architecture, start time, description id, name and family, verified-fact counts, target and baud, USB identity and baud-probe notes when present, one line per check (`PASS`, `N/A` or `DID NOT PASS`, name, operation, milliseconds, detail), a summary count, the failure note when any check failed, and finally the wire log, one line at a time. All arguments are borrowed; the return value is a new string. Pure, no I/O; the caller writes the file.

### `pub fn wire_tap<T>(transport: T, wire: Arc<Mutex<File>>) -> DuplexStream` [@ANCHOR: radioctl:radioctld_selftest_wire_tap]

Puts a recording tap between the rig actor and a transport. It consumes the transport (any `AsyncRead + AsyncWrite + Send + Unpin + 'static`) and returns the actor's end of an in-memory duplex stream (8192-byte buffer). A spawned tokio task copies bytes in both directions, up to 1024 bytes at a time, and appends each chunk to the shared file as `SECONDS tx|rx HEX` (seconds since the tap started, three decimals; tx is sent to the radio). It must be called inside a tokio runtime. The task ends silently when either side closes or errors. A failed log write or a poisoned lock is ignored; the data still passes through. Bytes are forwarded unchanged and never filtered, so the tap does not affect transmit safety.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctld_selftest_any_engine_encode]

The `encode` method of `AnyEngine`, the self-test's sized wrapper around a boxed engine (needed because `spawn_current` takes a sized engine type). It forwards the operation to the family engine and returns its result unchanged: the request bytes and reply kind, or a `RigError` such as Unsupported, which the self-test reports as not applicable. It adds no checks of its own; the read-only guarantee is enforced earlier by `is_read_only`. No I/O, no locks.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctld_selftest_any_engine_feed]

The `feed` method of `AnyEngine`: passes bytes received from the radio and the actor's monotonic time to the boxed family engine and returns the decoded events as an owned vector, in order. A partial frame is buffered by the engine and yields an empty vector for now. Nothing is filtered or added, and there is no error return; undecodable bytes appear as an `Unrecognised` event, which the self-test counts as a check that did not pass. No I/O.

### `pub fn parse_target(target: &str) -> Result<PortTarget, String>` [@ANCHOR: radioctl:radioctld_selftest_parse_target]

Turns the command-line port argument into a `PortTarget`. `usb:VID:PID` or `usb:VID:PID:SERIAL` gives a USB matcher with the two ids read as hexadecimal 16-bit numbers (`0x`-less, for example `usb:10c4:ea60`) and an optional serial string; anything else is taken as a serial device path (`/dev/ttyUSB0`). The special word `virtual` is not handled here (`open` checks for it first) and would become a path. Error: a VID or PID that is missing, not hexadecimal or above 16 bits returns `"<target>": VID and PID must be hexadecimal`. Pure; it does not touch the device.

### `pub async fn open(description: &RigDescription, target: &str, baud: u32, unmanaged: bool, wire: Arc<Mutex<File>>) -> Result<RigHandle, String>` [@ANCHOR: radioctl:radioctld_selftest_open]

Opens the rig for the self-test with `RigOptions::default()`, so the transmit gate stays closed and nothing in this module can open it. `target` of `virtual` starts the in-memory `VirtualRig` built from the description; otherwise it is parsed with `parse_target` and a serial port opened at `baud` with the description's control-line settings, three first-open attempts, and, if `unmanaged` is true, the RTS and DTR lines left alone. Both paths put `wire_tap` on the transport and spawn the actor. A real serial open also watches the link. Errors come back as strings (no engine for the family, bad target, port cannot be opened, actor cannot start). Needs a tokio runtime. The port monitor is deliberately leaked (`mem::forget`) and lives until the program ends. Hardware side effect: opens a real serial port, but sends no transmit command.

### `pub async fn run_checks(rig: &RigHandle, plan: Vec<(String, Op)>) -> Vec<CheckResult>` [@ANCHOR: radioctl:radioctld_selftest_run_checks]

Runs the planned checks one after another against the open rig and returns one `CheckResult` per plan entry, in plan order. It is `run_checks_logged` with no wire file, so each result's `wire` is empty. Each check is awaited in turn (no parallelism); a pass, a not-applicable (the engine says Unsupported) or a failure carrying the error kind and detail is recorded with the elapsed milliseconds. An operation that is not read-only is not sent and is recorded as `refused: not a read-only operation`. The function never returns an error itself, so a dead radio shows up as failed checks. Cancel-safe only between checks; it does not transmit.

## `crates/radioctld/src/candidates.rs`

### `pub fn highest_state(loaded: &LoadedDescription) -> &'static str` [@ANCHOR: radioctl:radioctld_candidates_highest_state]

Summarises how well a description's facts have been checked. It ranks each fact's verified state (`unverified` 0, `manual-checked` 1, `transcript-checked` 2, `hardware-verified` 3) and returns the name of the highest one found across `loaded.facts`. A description with no facts, or only unverified ones, gives `unverified`. The argument is borrowed read-only; the result is one of four static strings. Shown in the `--list` output so a volunteer knows which models still need hardware testing. Pure, no errors.

### `pub fn load_summaries(dir: &Path) -> Result<Vec<RigSummary>, String>` [@ANCHOR: radioctl:radioctld_candidates_load_summaries]

Reads every file ending in `.toml` in one directory (not recursive) and returns a `RigSummary` (id, display name, family name, highest verified state) for each, sorted by id. Other files are skipped; unreadable directory entries are silently skipped too. It uses blocking file I/O. Errors are strings: the directory cannot be read, a file cannot be read, or a description fails to parse, in which case the message names that file (all handled by `summaries_from`). One bad file fails the whole listing. Read-only; touches no hardware.

### `pub fn squash(text: &str) -> String` [@ANCHOR: radioctl:radioctld_candidates_squash]

Normalises text for comparing model names: keeps only ASCII letters and digits and lower-cases them, so `IC-7300` and `ic 7300` both become `ic7300`. Spaces, punctuation and non-ASCII characters (accented letters included) are dropped. The input is borrowed and a new string returned; an empty or all-punctuation input gives an empty string. Used by `candidates_for_product` on both the USB product string and the model part of each description name. Pure, no errors.

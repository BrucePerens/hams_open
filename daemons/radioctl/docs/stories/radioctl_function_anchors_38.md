# radioctl function anchors, batch 38

Function anchors for the Kenwood handheld and mobile engine and its virtual handheld (`kenwood_hh.rs`), and for the Kenwood TS-890S engine, its LAN login frames and its bandscope helpers (`kenwood_ts890.rs`), all in `radioctl-protocols`, written to the ADR 0113 standard. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Anything that can key a transmitter says how the gate applies: the engines only encode `TX`/`RX` (handheld) and the TS-890S key commands; the transmit gate, band check and frequency check are applied by the rig layer before `encode` is called, and the virtual handheld only flips a state flag.

## `crates/radioctl-protocols/src/kenwood_hh.rs`

### `pub fn new(desc: RigDescription) -> Result<KenwoodHhEngine, RigError>` [@ANCHOR: radioctl:radioctl_hh_engine_new]

Builds the Kenwood handheld and mobile engine (TH-D7A, TM-D710, TH-D72, TH-D74 and relatives) and takes ownership of `desc`. The description must be of family `KenwoodHh` and carry kenwood-hh parameters; their `dialect` (`th`, `tm710`, `th72`, `th74`) is copied into the engine. Any other description returns `RigError` of kind `Unsupported` ("not a Kenwood handheld description"). The new engine has nothing pending, an empty receive buffer and no remembered `FO` lines (`lines` is `[None, None]`, one per band). No I/O; one engine per rig, driven by the rig actor.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_hh_engine_encode]

Encodes an `Op` as one carriage-return ended command and records what answer to expect in `pending`. First requires the op's capability in the description (`Unsupported` otherwise). Supported ops: frequency get and set, mode get and set, PTT, and the signal read (`SM 0`). The `FO` dialects (`tm710`, `th72`, `th74`) read with `FO b` and set by rewriting the whole remembered line (`edited_line`), so a set before the band has been read is refused. The `th` dialect uses `FQ` (eleven digits, step code chosen by `th_step_and_frequency`), and only band A, since `FQ` has no band argument.

Errors, all `Unsupported`: VFO B on `th`, a frequency over ten digits (`FO`) or eleven (`FQ`), a mode without a code of 0 to 9, a set before the band is read, any other op. The TH-D74 sets the mode with `MD 0,k`. Keying: `SetPtt(true)` is `TX` and false is `RX`, answered by an echo; the engine does not decide whether transmitting is allowed, the rig layer does before calling. Side effect: it replaces `pending` and clears the receive buffer, so call it once per request, in order.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_hh_engine_feed]

Takes received serial bytes (`bytes`, borrowed) and their monotonic arrival time `now`, and returns the decoded `Inbound` items. Replies end in a carriage return (CR); the engine appends the bytes to its buffer and cuts a line at each CR, discarding empty lines and passing each remaining one to `decode_line`. A partial line older than `PARTIAL_FRAME_TIMEOUT` (500 ms) is first reported as `Inbound::Unrecognised`. Bytes that are not valid UTF-8 are reported as `Unrecognised` without touching the pending state. Never blocks, never fails; returns an empty vector until a CR arrives. Called by the rig actor only.

### `fn decode_line(&mut self, line: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_hh_decode_line]

Interprets one reply line, without its CR, against what the last `encode` asked (`pending`). It takes the pending state and resets it to nothing first, so each request gets one answer. A line of `?` is `Inbound::Nak`. A line with nothing pending is `Unrecognised`. For a setting, the radio echoes the command: the echo gives `Inbound::Ack`, and for an `FO` set the echo must equal the line that was sent, then it becomes the remembered line of that band; a different echo is `Unrecognised`.

Reads: a frequency answer is parsed with `parse_fo` (stored as the band's line) or, on the `th` dialect, as `FQ <digits>,<step>`; the mode is the `FO 0` mode digit or `MD n`; the signal is `SM 0,n`. Mode digits go through the description's mode codes. A line that does not parse is `Unrecognised` with its bytes. No I/O.

### `fn parse_fo(&self, line: &str, b: usize) -> Option<(Hz, u8)>` [@ANCHOR: radioctl:radioctl_hh_parse_fo]

Reads the frequency and the mode digit out of an `FO` line of the engine's dialect for band `b` (0 or 1). The line must have exactly the dialect's length (`fo_layout`: 48, 52 or 72 characters), start with `FO b,`, and have a comma at column 15; ten ASCII digits at columns 5 to 14 are the frequency in hertz, and the byte at the dialect's mode column must be a digit (its value is the mode code, 0 to 9). Returns `Some((Hz, digit))`, or `None` for the `th` dialect (no `FO`), a wrong length or prefix, a non-digit or an unparsable frequency. The caller maps the digit to a mode. Pure.

### `pub fn new(dialect: HhDialect, freq: Hz, mode: u8) -> HhSim` [@ANCHOR: radioctl:radioctl_hh_sim_new]

Creates a virtual handheld, the radio side of a kenwood-hh description, kept apart from the engine so the engine can be tested against it. `dialect` picks the command set; `freq` is the starting frequency in hertz and `mode` the starting mode code (0 to 9 as the dialect numbers them), both applied to both bands alike. The buffer is empty and no bytes have been seen. Pure construction; the virtual rig owns the value and feeds it bytes through `HhSim::feed`.

### `pub fn fo_line(&self, b: usize) -> String` [@ANCHOR: radioctl:radioctl_hh_sim_fo_line]

The `FO` line the virtual handheld sends for band `b` (0 or 1): `FO b,` then the frequency as ten zero-padded digits and a comma, the line padded with `0` filler to the dialect's length, and the mode digit written at the dialect's mode column. Only those documented columns carry data; every other column is a filler of our own, not a recorded radio value. Returns an empty string for the `th` dialect, which has no `FO` line. Used for `FO b` reads and by tests. An out-of-range `b` panics on the array index, so callers pass 0 or 1. Pure.

### `pub fn feed(&mut self, bytes: &[u8], now: Duration, desc: &RigDescription, state: &mut RigState) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_hh_sim_feed]

The virtual handheld's answers to command lines. `bytes` are what the engine wrote; `now` is the arrival time (a partial line older than 500 ms is dropped); `desc` supplies the mode code table; `state` is the shared virtual rig state, mutated. The bytes are split at CR, each line is answered by the simulator (`line`) and every reply is ended with a CR, then returned. After the lines are handled the simulator copies its frequencies to `state.freq_a` and `state.freq_b`, and the band 0 mode (through the description) to `state.mode`, so assertions on the virtual rig see the handheld's state. No I/O, no blocking.

### `fn command(text: String, reply: ReplyKind) -> Request` [@ANCHOR: radioctl:radioctl_hh_command]

Builds the request for one handheld command: the bytes of `text` (taken by value) followed by a carriage return, since the handheld dialects end commands with CR and not with a semicolon, plus the `ReplyKind` the caller expects (an observation of a field, or an acknowledgement echo). No validation of `text`; the callers format it. Pure.

### `fn edited_line(&self, vfo: Vfo, edit: impl FnOnce(&mut Vec<u8>)) -> Result<(usize, String), RigError>` [@ANCHOR: radioctl:radioctl_hh_edited_line]

Prepares an `FO` set: an `FO` set rewrites the whole fixed-column line, so the engine takes the line it last read for the band of `vfo` (A is band 0, B is 1), clones its bytes, lets `edit` change them (a frequency splice, or a mode digit), and returns the band index and the new line text. `edit` is called once and may change bytes in place.

Errors, `Unsupported`: no line remembered for that band yet ("read the band first: an FO set rewrites the line"), or the edited bytes are no longer valid UTF-8. The engine does not mutate its stored line here; the new line is stored only after the radio echoes it back (`decode_line`). Pure apart from the clone.

### `fn line(&mut self, line: &str, state: &mut RigState, signal: u16) -> String` [@ANCHOR: radioctl:radioctl_hh_sim_line]

The virtual handheld's answer to one command line (without CR). `state` is mutated for PTT, `signal` is the S-meter value to report. It handles `FO b` reads and `FO` line sets (the line must have the dialect's exact length; frequency digits and mode digit are checked, then both stored and the line echoed), `FQ` read and `FQ <11 digits>,<step>` set on the `th` dialect, `MD` read and set (`MD b,k` on the TH-D74), `TX` and `RX` (set the virtual PTT flag and echo), and `SM 0` (`SM 0,n`). Anything else, or malformed, answers `?`. Sets answer with an echo of the line, as the radios do. Virtual: no RF.

## `crates/radioctl-protocols/src/kenwood_ts890.rs`

### `fn level_wire(level: Level) -> Option<LevelWire>` [@ANCHOR: radioctl:radioctl_ts890_level_wire]

Maps a `Level` to the TS-890S command that carries it and the guide's valid range, as a `LevelWire` (fields `name`, the two-letter command, `min`, `max`). Unlike the classic Kenwood family, the TS-890S has no receiver digit: `AG` 0..255, `RG` 0..255, `MG` 0..100, `KS` 4..60 (keyer speed), `PC` 5..100 (RF power), all followed by three digits. Returns `None` for levels with no such command here (monitor, attenuator, preamp, dither, preselector, antenna), which the engine handles by other commands or refuses. Pure.

### `pub fn lan_connect_request() -> Vec<u8>` [@ANCHOR: radioctl:radioctl_ts890_lan_connect_request]

Returns the first frame of the TS-890S LAN session, the fixed bytes `##CN;` (guide p. 71): it asks the radio to allow a connection. The radio answers `##CN1;` (authorised) or `##CN0;` (denied); read it with `lan_reply`. No arguments, no errors, no I/O; a fresh vector each call. Sending this does not key the radio.

### `pub fn lan_login(administrator: bool, account: &str, password: &str) -> Result<Vec<u8>, RigError>` [@ANCHOR: radioctl:radioctl_ts890_lan_login]

Builds the second LAN frame, `##ID<type><len><len><account><password>;` (guide p. 72). `administrator` true selects account type 0 and false type 1 (user); the lengths are the two-digit lengths of `account` and `password`, each 1 to 32 printable ASCII characters. Both strings are borrowed and copied into the frame.

Returns the frame bytes. Errors: `Unsupported` for an empty or over-long account or password, a non-printable or non-ASCII byte, or a `;` (it could not be told apart from the terminator). Secret handling: the frame contains the account and password in clear text, so the caller must not log it. Pure; the radio answers `##ID1;` or `##ID0;`.

### `pub fn lan_reply(text: &str) -> Option<bool>` [@ANCHOR: radioctl:radioctl_ts890_lan_reply]

Reads the radio's answer to a LAN frame: `Some(true)` for `##CN1;` and `##ID1;` (authorised), `Some(false)` for `##CN0;` and `##ID0;` (denied), and `None` for anything else. A trailing semicolon is accepted and may be absent. The text is matched exactly, so a different case or extra characters give `None`. The caller decides what denial means (for example stop and report a wrong account). Pure.

### `pub fn new(desc: RigDescription) -> Result<Ts890Engine, RigError>` [@ANCHOR: radioctl:radioctl_ts890_engine_new]

Builds the TS-890S engine; takes ownership of the description. It must be family `KenwoodAscii` with the dialect `ts890`, else `RigError` of kind `Unsupported` ("not a TS-890S description"). The engine starts with a tokenizer limited to 2048 bytes (a `##DD2` frame is 1286 bytes), transceive and scope output off, empty scope context (last frequencies, axis, span and edges unknown, dropped count 0) and no partial scope sweeps. No I/O. One engine per rig, driven by the rig actor.

### `fn serial_scope(&self) -> bool` [@ANCHOR: radioctl:radioctl_ts890_engine_serial_scope]

True when the description says the TS-890S scopes come over the serial link (`kenwood.scope_link = "serial"`, the `scope_serial` flag), false for the LAN link. It chooses the scope output setting the engine sends (`DD0 4`/`DD1 2` for COM/USB linked to auto information, `DD0 2`/`DD1 1` for LAN) and whether sweeps arrive as `DD2`/`DD3` splits. Reads the description only; pure.

### `fn serial_split(&mut self, text: &str) -> Option<Inbound>` [@ANCHOR: radioctl:radioctl_ts890_engine_serial_split]

Handles one serial scope split: `DD2` or `DD3`, a two-digit split number, then digits, without the `;`. A sweep arrives in several splits (32 for the bandscope, 12 for the filter scope) and `Splits::push` assembles them in order. Returns `None` while more splits are due, and `Some(Inbound::ScopeFrame)` or `Some(Inbound::FilterScopeFrame)` when the last split completes the sweep and it can be placed. A split with an unparsable or out-of-range number, the wrong size or out of order is not an error: it is counted in `scope.dropped` (once per bad sweep) and `None` is returned. Mutates the assembly state and the counter; no I/O.

### `fn push(&mut self, index: u8, payload: &str, splits: u8, split_digits: usize, last_digits: usize) -> Result<Option<String>, ()>` [@ANCHOR: radioctl:radioctl_ts890_splits_push]

Collects the splits of one serial scope sweep in order. `index` is the split number, `payload` its digits (borrowed), `splits` how many make a sweep, `split_digits` how long each payload must be, `last_digits` how many of the last split's digits carry samples. Index 0 starts a new sweep and clears any earlier state. Returns `Ok(None)` while more splits are due (and while the rest of a broken sweep is skipped), `Ok(Some(digits))` with the whole sweep once the last split arrives, and `Err(())` once per sweep when a split is out of order, non-ASCII or the wrong size, which discards the sweep so far. Mutates `self`; no I/O.

### `pub fn scope_frames_dropped(&self) -> u64` [@ANCHOR: radioctl:radioctl_ts890_engine_scope_frames_dropped]

Number of bandscope sweeps the engine received and did not publish since it was built: sweeps not in center mode, sweeps arriving before the frequency, span or axis was known (or with expand state unknown), and malformed frames or splits. A monotonic counter, never reset. A sweep carries no frequency of its own, so the engine places it from the last values it saw; this count shows how many it could not place. Read only; cheap; no side effects.

### `fn decode(&self, text: &str) -> Inbound` [@ANCHOR: radioctl:radioctl_ts890_engine_decode]

Decodes one complete command or answer (`text`, without the `;`) from the TS-890S into one `Inbound`. `?` is `Nak`. The recognised forms: `FA`/`FB` (eleven digit frequency), `OM0m` (mode through the description's code table), `SM` (four digit S-meter), `ID` (three digits), `BS` scope answers (`decode_scope`), `DD0n` scope output on or off, `FR` and `FT` (VFO), `TB` (split), `RT` and `XT`, `RF` (RIT offset, `decode_offset`), `AN` (antenna), `RA` and `PA` (front-end steps), and the numeric levels. A reply that matches none, or has the wrong shape, is `Inbound::Unrecognised` with the text and `;` restored. Never fails; pure.

### `fn decode_step(&self, level: Level, digits: &str) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_ts890_engine_decode_step]

Decodes an `RA` (attenuator) or `PA` (preamp) answer on the TS-890S, where the argument is exactly one digit, the wire step. The step is mapped to a value (decibels or step number) through `attenuation::value_of` and the description, and returned as `Observation::Level(level, value)`. `None` for a wrong length, a non-digit, or a step the description does not have, so the reply is reported as unrecognised and never guessed. Pure.

### `fn antenna_port(&self, port: u16) -> Result<u8, RigError>` [@ANCHOR: radioctl:radioctl_ts890_engine_antenna_port]

Converts an antenna port number into the digit the `AN` command takes. `port` must be 1 or 2 and not above the description's `levels.antenna_ports` (capped at 2). Returns the digit, or `RigError` of kind `Unsupported` ("no such antenna port on this model") for port 0, a port beyond what the description lists, or a value that does not fit a byte. Used when encoding an antenna level set. Pure.

### `fn decode_antenna(digits: &str) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_ts890_decode_antenna]

Decodes an `AN` answer: four digits `P1 P2 P3 P4` (guide p. 47). Only P1 is used: 1 or 2 is the antenna in use, returned as `Observation::Level(Level::Antenna, 1 or 2)`. Returns `None` when there are not exactly four ASCII digits or P1 is any other digit, so the reply is reported as unrecognised. The other three digits are not interpreted. Pure.

### `fn vfo_of(digit: &str) -> Vfo` [@ANCHOR: radioctl:radioctl_ts890_vfo_of]

Maps the argument of an `FR` or `FT` answer to a VFO: `"1"` is VFO B, anything else (including `"0"`) is VFO A. The caller has already checked the digit is `0` or `1`. Total, never fails, pure.

### `fn decode_offset(digits: &str) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_ts890_decode_offset]

Decodes the `RF` answer (the RIT/XIT offset) after the command name: five characters, a direction digit (0 plus, 1 minus) then four digits of hertz. Returns `Observation::RitOffset(hz)` as a signed `i16` (up to plus or minus 9999). `None` for a direction digit other than 0 or 1, a non-digit, or a wrong length. Pure; the offset is set separately with `RC`, `RU` and `RD`.

### `fn decode_level(name: &str, digits: &str) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_ts890_decode_level]

Decodes a numeric level answer: the two-letter command `name` (`AG`, `RG`, `MG`, `KS`, `PC`) and exactly three digits, for example `AG128` gives AF gain 128. Returns `Observation::Level(level, value)`, or `None` for an unknown name, a digit count other than three, or a non-digit. The value is not checked against the model range. Unlike newcat there is no leading zero. Pure.

### `fn scope_axis_code(axis: ScopeAxis) -> Option<u8>` [@ANCHOR: radioctl:radioctl_ts890_scope_axis_code]

The `BS3` digit for a scope axis mode: `Center` is 0, `Fixed` is 1, `ScrollFixed` (the rig's auto scroll) is 2. `ScrollCenter` has no digit on this radio and returns `None`, so the engine refuses to set it. Pure; the inverse is `scope_axis_of`.

### `fn scope_axis_of(digit: &str) -> Option<ScopeAxis>` [@ANCHOR: radioctl:radioctl_ts890_scope_axis_of]

The axis mode a `BS3` digit names: `"0"` center, `"1"` fixed, `"2"` auto scroll (`ScrollFixed`); `None` for anything else, including a longer string. Pure; the inverse of `scope_axis_code`. A sweep is only published in center mode, so this value decides whether sweeps can be placed.

### `fn scope_span_code(span: Hz) -> Option<usize>` [@ANCHOR: radioctl:radioctl_ts890_scope_span_code]

The `BS4` digit for a bandscope span: the index of `span` in the table 5, 10, 25, 50, 100, 200, 500 kHz (digits 0 to 6). Returns `None` when the rig offers no such span (the exact hertz value must match), and the engine then refuses the set with `Unsupported`. Pure.

### `fn scope_reference_code(hundredths: i16) -> Option<u16>` [@ANCHOR: radioctl:radioctl_ts890_scope_reference_code]

The three `BSC` digits for a scope reference level given in hundredths of a decibel: code 000 is -20 dB, 040 is 0 dB and 060 is +10 dB, half a decibel per step (the code is `(hundredths + 2000) / 50`). Returns `None` when the level is not an exact multiple of 0.5 dB or lies outside -20 dB to +10 dB. Pure; the decode side inverts it.

### `fn decode_scope(rest: &str) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_ts890_decode_scope]

Decodes a `BS` answer, `rest` being what follows the command name `BS`: the sub-command letter and its value. `3` is the axis (`ScopeAxis`), `4` the span (one digit 0 to 6, mapped through the span table to hertz), `C` the reference level (three digits, at most 60, converted to hundredths of a dB), and any other letter a scope setting (`scope_setting_of`) with a one-digit code that must lie inside the setting's range. All observations are for receiver 0. Returns `None` for an unmapped letter, a wrong digit count or an out-of-range value, so that reply is reported as unrecognised. Pure.

### `fn scope_setting_wire(setting: ScopeSetting) -> ScopeWire` [@ANCHOR: radioctl:radioctl_ts890_scope_setting_wire]

Returns how a scope setting is written, as a `ScopeWire` with `letter` (the character after `BS`), `min` and `max` (the smallest and largest digit the guide allows) and `writable` (false when the guide has no set form). For example Display is `BS0` 0..1, Layout `BS1` 0..4, Waterfall speed `BSB` 1..4, Expand `BSO` 0..1, and Overflow `BSL` 0..1 read only. Covers all 17 settings of `ScopeSetting`. Pure; used to validate a set and to decode an answer.

### `fn scope_setting_of(letter: &str) -> Option<ScopeSetting>` [@ANCHOR: radioctl:radioctl_ts890_scope_setting_of]

The inverse of `scope_setting_wire` for the letter: given the text after `BS` (only the first character is used), finds the `ScopeSetting` among the 17 mapped ones whose letter matches. Returns `None` for an empty string or a letter that is not one of the mapped settings (the axis, span, reference and edge commands `3`, `4`, `C` and `M` have their own handling). Pure.

### `fn scope_action_letter(action: ScopeAction) -> char` [@ANCHOR: radioctl:radioctl_ts890_scope_action_wire]

The command letter, without `BS`, of a one-shot scope action: `ClearWaterfall` is `D` (`BSD`, clears the waterfall) and `MarkerShift` is `E` (`BSE`, shifts the marker in auto scroll mode or centres it in fixed mode; the rig ignores it in center mode). Total over the two actions. Pure; `encode` sends `BS` plus this letter with no value, and checks the center-mode rule for `MarkerShift` itself (see `encode`).

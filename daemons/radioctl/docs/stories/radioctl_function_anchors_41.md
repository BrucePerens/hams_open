# radioctl function anchors, batch 41

Function anchors for the `radioctl-protocols` crate: the second half of the JRC dialect and its twin (`jrc.rs`), the Icom PCR (`icom_pcr.rs`), Icom marine (`icom_marine.rs`), Rohde and Schwarz (`rs.rs`) and AOR AR3030/AR3000A (`aor_classic.rs`) engines and virtual twins, and the `raw` default of the receiver twin trait. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Only the Icom marine dialect (`TRX,TX` / `TRX,RX`) and the JRC JST models can key a transmitter, and the encoders here only spell the command; the transmit gate, lock and band check are applied by the caller that builds the `SetPtt` operation. The rest are receivers.

## `crates/radioctl-protocols/src/jrc.rs`

### `fn info_mode(&self, desc: &RigDescription, line: &str) -> Option<radioctl_core::units::Mode>` [@ANCHOR: radioctl:radioctl_jrc_info_mode]

Reads the operating mode out of an `I` answer. `line` is the received line without its terminator; `desc` supplies the mode-code table. The line must first pass `info_checked` (right first character and right length for the model), otherwise the result is `None`. Then the byte at index 3 (the fourth character) is looked up with `desc.code_mode`; a character that is not in the description's table also gives `None`, and so does a line too short to have the byte. The caller (`decode`) turns `None` into an unrecognised-line result. Pure, no side effects.

### `pub fn new(desc: &RigDescription) -> Result<JrcTwin, RigError>` [@ANCHOR: radioctl:radioctl_jrc_twin_new]

Builds the virtual JRC radio used by tests and the virtual rig. `desc` is borrowed; the twin clones it and builds its `JrcDialect` with `from_description`, so an unknown `receiver.dialect` name returns `ErrorKind::Unsupported` ("unknown JRC dialect"). The twin has no frequency, mode or signal of its own: those live in the shared `RigState` that the caller passes to each `line` call. No I/O, cannot panic.

### `fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_jrc_twin_line]

The virtual JRC radio's reaction to one command line (CR already removed). `state` is mutated like the real radio would be. On NRD-535D/545, `I1` or `I` answers `I00`, the mode character (default `4`) and the frequency zero-padded to the model's digit count, then padded with `0` to one character less than the model's answer length, plus CR. On JST, `I` answers `I00`, mode, eight digits of hertz and a `0`. On NRD, `M` answers `M` and the signal as three digits, capped at 999. On JST, `X` answers `X0` or `X1` from `state.ptt`, and `X0`/`X1` set `state.ptt` (the virtual radio's transmit flag; no real transmitter is involved) with no answer. Any other line is handed to `set`. Settings produce no reply because the real radios send none. No errors.

### `fn set(&mut self, kind: Kind, state: &mut RigState, line: &str)` [@ANCHOR: radioctl:radioctl_jrc_twin_set]

Applies a setting line to the virtual radio's shared state. A line starting `F` has trailing `A` or `B` (the JST VFO letter) removed and the rest parsed as a decimal `u64`; on success `state.freq_a` is set, multiplied by 10 for the NRD-525 (units of 10 Hz) and as hertz otherwise. A line starting `D` takes its first following byte as a mode code and, when the description knows it, sets `state.mode`. Anything else, an unparseable number or an unknown mode code is silently ignored, because these radios never refuse a command. `kind` is the model variant (copied), `state` is mutated, `line` borrowed. No reply, no I/O.

## `crates/radioctl-protocols/src/icom_pcr.rs`

### `pub fn engine(desc: RigDescription) -> Result<PcrEngine, RigError>` [@ANCHOR: radioctl:radioctl_pcr_engine]

Builds the protocol engine for an Icom PCR computer-controlled receiver (IC-PCR100, PCR1000, PCR1500, PCR2500). `desc` is taken by value. It wraps `LineEngine::new` with `ReceiverProtocol::IcomPcr`: an `ErrorKind::Unsupported` error comes back if the description is not a receiver with that protocol, or (from `PcrDialect::from_description`) if the dialect name is neither `pcr` nor `pcr1000`. The engine remembers the last frequency and mode it set because the radio cannot report them. These radios cannot transmit. No I/O.

### `pub fn tuning_command(hz: Hz, mode: u8, filter: u8) -> Option<String>` [@ANCHOR: radioctl:radioctl_pcr_tuning_command]

Spells the PCR `K0` tuning command, which sets frequency, mode and filter in one. `hz` is the frequency in hertz and must not exceed 9,999,999,999 (the ten digits it is written in), otherwise `None`. `mode` is the radio's mode character (`0` LSB, `1` USB, `2` AM, `3` CW, `5` FM, `6` WFM) and `filter` the filter character (the engine passes `2`, 15 kHz); neither is validated here. The result is `K0`, ten zero-padded digits, `0`, the mode, `0`, the filter and `00`, ended by a line feed. Public so the virtual twin and tests build the same bytes. Pure.

### `fn from_description(desc: &RigDescription) -> Result<PcrDialect, RigError>` [@ANCHOR: radioctl:radioctl_pcr_dialect_from_description]

Builds the PCR dialect from `receiver.dialect`: `pcr` (answers four characters and CR LF) or `pcr1000` (each answer is framed by a leading line feed and a trailing CR, recorded as `lead`). The starting memory is the description's `default_frequency` and the mode code of `default_mode` (character `5`, FM, if the description has no code for it); nothing is pending. Any other name, including an empty one, returns `ErrorKind::Unsupported` ("unknown PCR dialect"). `desc` borrowed. Pure.

### `fn open_commands(&mut self, _desc: &RigDescription) -> Vec<(Vec<u8>, ReplyKind)>` [@ANCHOR: radioctl:radioctl_pcr_dialect_open_commands]

The PCR override of the open hook, copying Hamlib's open. It returns two requests in order: `H101` (power the receiver on) then `G300` (stop its automatic updates), each ended by a line feed and each expecting `ReplyKind::Ack`. They are sent when the rig actor starts and again after a link loss (through `link_restored`). The description argument is ignored, the list is the same for every model, and neither command transmits. No errors, no I/O.

### `fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError>` [@ANCHOR: radioctl:radioctl_pcr_dialect_encode]

Encodes an operation for a radio that can only be tuned with one combined command. `SetFrequency(A, hz)` and `SetMode(mode)` both send `K0` through `tuning_command` with the filter fixed at 15 kHz: a frequency set reuses the remembered mode, a mode set (code from the description, else `Unsupported` "mode not in the description") reuses the remembered frequency. The new pair is stored as `pending` and committed only when the radio accepts it (see `status`). A frequency above 9,999,999,999 Hz gives `Unsupported` ("frequency out of range"). Setting operations expect an `Ack`. `GetFrequency(A)` and `GetMode` send the liveness query `H1?` and expect an observation, answered from memory; `GetSignal` sends `I1?`. Every other operation, including VFO B and transmit, is `Unsupported`. No I/O.

### `fn status(&mut self, expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>>` [@ANCHOR: radioctl:radioctl_pcr_dialect_status]

Handles the PCR's four-character status answers before `decode`. Any leading junk before the first `G`, `H` or `I` is skipped (the PCR1000's extra lead character). `H101` is a bare acknowledgement unless an observation was expected (then it is data for `decode`); `G000` accepts the last command: it commits `pending` (frequency and mode) to the dialect's memory and gives `Inbound::Ack` if an `Ack` was expected, nothing otherwise; `G001` rejects it: it discards `pending` and gives `Inbound::Nak`. An empty line is swallowed. Everything else returns `None`. Mutates the dialect; no I/O.

### `fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_pcr_dialect_decode]

Interprets a data line when an observation was expected (otherwise `Inbound::Unrecognised`). Leading characters before `G`, `H` or `I` are dropped first. `H100` (power off) gives `Inbound::Nak` for any field. `H101` answers a frequency read with the remembered frequency and a mode read with the remembered mode (mapped through the description, `None` if unknown): the radio itself never reports them. `I1` and two hex digits, with the line exactly four characters long, is the signal, parsed as `u16`. Anything else is `Unrecognised` with the line's bytes. Pure apart from reading the memory.

### `pub fn new(desc: &RigDescription) -> Result<PcrTwin, RigError>` [@ANCHOR: radioctl:radioctl_pcr_twin_new]

Builds the virtual PCR radio. It derives the dialect from the description (an unknown dialect name returns `ErrorKind::Unsupported`) only to learn whether this is the PCR1000-style framing (`lead`), keeps a clone of the description, and starts with the filter set to the 15 kHz character `2`. `desc` is borrowed. Frequency, mode and signal are held in the shared `RigState` that is passed to each `line` call. No I/O.

### `fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_pcr_twin_line]

The virtual radio's reaction to one command line (line feed removed). Answers are `text` plus CR LF, or on the PCR1000 a line feed, the text and a CR. `H1?` and `H101` answer `H101`; `G300` and `G301` answer `G000`; the identity queries `G2?`, `G4?`, `GD?` and `GE?` answer fixed values; a five-character `J` plus four hex digits (volume, squelch) is accepted and not kept (`G000`); `I1?` answers `I1` and the signal as two hex digits (capped at 255). Otherwise the line must be a well-formed 18-character `K0` or `K1` tuning command (digits, fixed zeros, a known mode character): `K0` sets `state.freq_a`, `state.mode` and the filter and answers `G000`, `K1` (the PCR2500's second receiver) is accepted but not modelled, and anything malformed answers `G001`. Mutates `state`; no I/O.

## `crates/radioctl-protocols/src/icom_marine.rs`

### `pub fn engine(desc: RigDescription) -> Result<MarineEngine, RigError>` [@ANCHOR: radioctl:radioctl_marine_engine]

Builds the protocol engine for an Icom marine transceiver (IC-M700PRO, IC-M710, IC-M802, IC-M803), which speaks `$PICOA` NMEA-style sentences. `desc` is taken by value. It wraps `LineEngine::new` with `ReceiverProtocol::IcomMarine`; an `ErrorKind::Unsupported` error is returned for the wrong family or protocol, for an unknown dialect name, or when the description has no remote id from 0 to 99 (see `from_description`). These are transmitters: PTT (push to talk) is encoded, so the transmit gate is applied by the caller. No I/O.

### `pub fn sentence(id: u8, command: &str) -> String` [@ANCHOR: radioctl:radioctl_marine_sentence]

Builds one complete command sentence. `id` is the radio's remote id (the controller's own id 90 is added), printed as two decimal digits; `command` is the command name with its optional `,param` already appended (for example `TXF,145.500000`). The body is `PICOA,90,<id>,<command>`; the checksum is the XOR of all its bytes, shown as two uppercase hex digits after `*`, and the sentence is `$`, the body, `*XX`, CR and LF. Used by the engine for requests and by the twin for echoes and answers. It does not validate `command`; an id above 99 prints three digits. Pure.

### `fn from_description(desc: &RigDescription) -> Result<MarineDialect, RigError>` [@ANCHOR: radioctl:radioctl_marine_dialect_from_description]

Builds the marine dialect. The `receiver.dialect` string selects the RTTY mode name: `icmarine` uses `J2B`, `icm710` uses `AFS`; any other name returns `ErrorKind::Unsupported` ("unknown marine dialect"). The `receiver.address` is the radio's remote id and must be present and at most 99, else `Unsupported` ("marine description needs a remote id 0 to 99"). The dialect starts with no echoes outstanding. `desc` borrowed. Pure.

### `fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError>` [@ANCHOR: radioctl:radioctl_marine_dialect_encode]

Turns an operation into one or more sentences. `SetFrequency(A, hz)` sends `TXF` then `RXF` (megahertz with six decimals) so the radio never transmits on a stale frequency, expecting an `Ack`; `GetFrequency(A)` sends `RXF`; `SetMode` sends `MODE,<name>` (`Unsupported`, "mode not in the description", if there is no name); `GetMode` sends `MODE`; `SetPtt(on)` sends `TRX,TX` or `TRX,RX`; `GetPtt` sends `TRX`; `GetSignal` sends `SIGM`. Anything else is `Unsupported`. For a setting, the commands sent are remembered in `waiting` so the echoes can be checked. `SetPtt(true)` keys a transmitter: the engine does no gating itself, the caller must hold the transmit gate, lock and band check. No I/O.

### `fn more_lines(&self) -> bool` [@ANCHOR: radioctl:radioctl_marine_dialect_more_lines]

Tells the shared engine whether more answer lines are expected for the request in flight. True while the dialect's list of unanswered echoes is not empty, which is the case after a frequency setting (two commands, two echoes) until both have arrived. While true, the engine keeps the expected reply kind instead of resetting it to none after each line. Read-only, no arguments, cannot fail.

### `fn status(&mut self, _expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>>` [@ANCHOR: radioctl:radioctl_marine_dialect_status]

The marine override of the status hook. The radio has no bare acknowledgement; success and refusal are told by comparing echoes in `decode`. So the only line handled here is an empty one, which gives an empty result (ignored); every other line returns `None` and goes on to `decode`. The expected reply kind is ignored. Pure.

### `fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_marine_dialect_decode]

Interprets one received sentence via `Fields::parse`; a sentence that fails parsing (bad prefix, bad checksum) clears the outstanding echoes and gives `Inbound::Unrecognised`. For a setting (`Ack` expected) the sentence is compared with the oldest outstanding command: a mismatch clears the list and gives `Inbound::Nak`; a match removes it and gives `Ack` only when the last echo has arrived (nothing before that, for the two-sentence frequency setting). For a query the last field is the value: frequency in megahertz (empty means 0 Hz, values 0 to under 10000 accepted), mode via `mode_of`, PTT from a value of at least two characters starting `TX`, and signal as a single leading digit. An unexpected line gives `Unrecognised`. Mutates the outstanding list.

### `fn mode_of(&self, desc: &RigDescription, value: &str) -> Option<Mode>` [@ANCHOR: radioctl:radioctl_marine_mode_of]

Maps the value of a `MODE` answer to a `Mode`. A value that starts with the dialect's RTTY name (`J2B` or `AFS`) is RTTY; otherwise the value is matched by prefix against `LSB`, `USB`, `CW` and `AM`. The mode is returned only if the description has a code for it, else `None`. `value` is the text after the last comma of the sentence and is borrowed. Pure.

### `fn parse(line: &'a str) -> Option<Fields<'a>>` [@ANCHOR: radioctl:radioctl_marine_fields_parse]

Splits a received `$PICOA` sentence into the parts the dialect and the twin need. The line must start with `$`, contain `*` with two hex digits after it that equal the XOR of the text between, and the text must start `PICOA,`. The text is split into at most four parts on commas, dropping the first three (`PICOA`, controller id, radio id); the rest is `command_and_param`, and `last` is the text after its last comma. Returns `None` on any failure, including too few commas. No minimum total length is checked in this code. Pure; the returned fields borrow from `line`.

### `pub fn new(desc: &RigDescription) -> Result<MarineTwin, RigError>` [@ANCHOR: radioctl:radioctl_marine_twin_new]

Builds the virtual marine radio. It builds the dialect from the description (so an unknown dialect name or a missing or out-of-range remote id returns `ErrorKind::Unsupported`), clones the description, and starts the transmit frequency `tx` at the description's `default_frequency`. `desc` is borrowed. Receive frequency, mode, PTT and signal live in the shared `RigState`. No I/O. The twin only simulates; it keys nothing.

### `fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_marine_twin_line]

The virtual radio's answer to one command sentence (the line feed removed). A sentence that fails `Fields::parse` is ignored (empty result). `TXF,<mhz>` stores the transmit frequency and `RXF,<mhz>` sets `state.freq_a`; both echo the command sentence back; a value outside 0 to under 10000 MHz or unparseable gets no answer. `RXF` alone answers with the frequency. `MODE,<name>` sets `state.mode` if the name matches one of the dialect's modes and echoes it; `MODE` answers the current mode (default `USB`). `TRX,TX` or `TRX,RX` set `state.ptt` and echo; `TRX` answers `TX` or `RX`; `SIGM` answers the signal capped at 9. Everything else is silently not answered. Mutates `state`; no I/O.

## `crates/radioctl-protocols/src/rs.rs`

### `pub fn engine(desc: RigDescription) -> Result<RsEngine, RigError>` [@ANCHOR: radioctl:radioctl_rs_engine]

Builds the protocol engine for a Rohde and Schwarz receiver (ESMC, EB200) with the `FREQ` / `DEM` / `SENS` command set. `desc` is taken by value. A thin wrapper over `LineEngine::new` with `ReceiverProtocol::RohdeSchwarz`; an `ErrorKind::Unsupported` error comes back for the wrong family or protocol or an unknown dialect name. These receivers cannot transmit. No I/O.

### `fn from_description(desc: &RigDescription) -> Result<RsDialect, RigError>` [@ANCHOR: radioctl:radioctl_rs_dialect_from_description]

Builds the Rohde and Schwarz dialect, which has no variants: the only accepted `receiver.dialect` is `rs`, giving the unit value `RsDialect`. Any other name, including an empty one, returns `ErrorKind::Unsupported` ("unknown Rohde and Schwarz dialect"). `desc` borrowed. Pure, no I/O.

### `fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError>` [@ANCHOR: radioctl:radioctl_rs_dialect_encode]

Spells an operation framed with a carriage return before and after the text. `SetFrequency(A, hz)` sends `FREQ <hertz>` as an integer, with no range check, and expects no reply (`ReplyKind::None`); `GetFrequency(A)` sends `FREQ?`; `SetMode` sends `DEM <name>` (`AM`, `FM`, `CW`, `USB`, `LSB`), failing with `ErrorKind::Unsupported` if the description lacks the mode or its code is not 1 to 5; `GetMode` sends `DEM?`; `GetSignal` sends `SENS:DATA? "VOLT:AC"`. Other operations, including transmit, are `Unsupported`. Reads expect an observation. No I/O.

### `fn status(&mut self, _expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>>` [@ANCHOR: radioctl:radioctl_rs_dialect_status]

The Rohde and Schwarz override of the status hook. These receivers send no acknowledgement or refusal for settings, so the only line it handles is an empty one, which gives an empty result (ignored); everything else returns `None` and goes on to `decode`. The expected reply kind is ignored. Pure.

### `fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_rs_dialect_decode]

Interprets a received line when an observation is expected (otherwise `Inbound::Unrecognised`). A frequency is the whole line parsed as an unsigned integer of hertz. A mode is the line matched exactly against `AM`, `FM`, `CW`, `USB`, `LSB`, kept only if the description has a code for it. The signal is the line parsed as a floating-point number in dBuV, which must be finite; the integer part is truncated and clamped to 0 through 65535. Anything else gives `Unrecognised` with the line's bytes; no error is returned. Pure.

### `pub fn new(desc: &RigDescription) -> Result<RsTwin, RigError>` [@ANCHOR: radioctl:radioctl_rs_twin_new]

Builds the virtual Rohde and Schwarz receiver. It checks the description with `RsDialect::from_description` (an unknown dialect name returns `ErrorKind::Unsupported`) and keeps a clone for mode lookups. `desc` is borrowed. The twin keeps no state of its own; frequency, mode and signal are in the `RigState` passed to each `line` call. No I/O.

### `fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_rs_twin_line]

The virtual receiver's reaction to one command line (CR removed). `FREQ <hz>` sets `state.freq_a` if the number parses, and `DEM <name>` sets `state.mode` if the name is known and the description has that mode; neither is answered, and a bad value is ignored without any refusal. `FREQ?` answers the integer hertz, `DEM?` the mode name (default `FM`), and `SENS:DATA? "VOLT:AC"` the raw signal as an integer; each answer ends with CR. Any other line gets no answer. Mutates `state`; no I/O.

## `crates/radioctl-protocols/src/receiver.rs`

### `fn raw(&mut self, _state: &mut RigState, _bytes: &[u8]) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_receiver_twin_raw_default]

The default body of the `ReceiverTwin::raw` hook, for twins of protocols that are not line oriented (for instance a binary one). It ignores `_state` and `_bytes` and returns `None`, which tells `VirtualReceiver::feed` to use the normal line framing for these bytes. A twin that overrides it gets the raw controller bytes first and may return `Some(answer)` to bypass line framing for that call; an empty `Some` answers nothing. No errors, no side effects in the default.

## `crates/radioctl-protocols/src/aor_classic.rs`

### `pub fn engine(desc: RigDescription) -> Result<AorClassicEngine, RigError>` [@ANCHOR: radioctl:radioctl_aorc_engine]

Builds the protocol engine for the AOR AR3030 or AR3000A, whose command sets differ from the `RF` / `MD` / `LM` family of `aor.rs`. `desc` is taken by value. It wraps `LineEngine::new` with `ReceiverProtocol::Aor` (the same protocol as the plain AOR engine; `handles` tells them apart by the dialect name). It returns `ErrorKind::Unsupported` for a description that is not an AOR receiver, or for a dialect other than `ar3030` and `ar3000a`. These radios cannot transmit. No I/O.

### `fn style_of(desc: &RigDescription) -> Option<Style>` [@ANCHOR: radioctl:radioctl_aorc_style_of]

Maps the description's `receiver.dialect` string to the internal `Style`: `ar3030` gives the AR3030 style, `ar3000a` the AR3000A style, and anything else (including no `[receiver]` section, where the name is empty) gives `None`. `desc` is borrowed. Used by `handles` and the dialect builder to choose the command set. Pure.

### `pub fn handles(desc: &RigDescription) -> bool` [@ANCHOR: radioctl:radioctl_aorc_handles]

True when this module's engine, not the plain AOR one, serves the description: its `receiver.dialect` is `ar3030` or `ar3000a` (it is `style_of(desc).is_some()`). The engine and twin factories (`conformance.rs` and `VirtualReceiver::new`) test it before the plain AOR case, so the order of checks matters. `desc` is borrowed; pure, no errors.

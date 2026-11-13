# radioctl function anchors, batch 40

Function anchors for the `radioctl-protocols` crate: the shared receiver engine (`receiver.rs`) and the AOR, Uniden and JRC receiver dialects with their virtual twins, written to the ADR 0113 standard. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Receivers have no transmit path; the one transmit-capable dialect here is the JRC JST-145/245 (`X1` keys, `X0` unkeys), and for it the transmit gate, lock and band check are applied by the caller that builds the `SetPtt` operation, not by these encoders.

## `crates/radioctl-protocols/src/receiver.rs`

### `fn description(&self) -> &RigDescription` [@ANCHOR: radioctl:radioctl_receiver_engine_description]

The `ProtocolEngine` accessor that hands back the rig description a `LineEngine` was built from. The caller borrows it read-only for as long as it holds the engine; nothing is copied and no argument is taken beyond `&self`. The description is the one passed to `LineEngine::new` and never changes afterwards, so the actor and the capability checks can rely on it staying the same for the life of the engine. It cannot fail, does no I/O, takes no lock and has no side effects.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_receiver_engine_encode]

Turns one operation into the bytes to write and the kind of reply to expect. `op` is borrowed; the engine is mutated because it remembers what it now expects. First it gates on the description: if `op.required_capability()` is not in the description's capability set it returns `ErrorKind::Unsupported` ("capability not in the rig description") and writes nothing, which is how a receiver is kept from ever being asked to transmit. Otherwise the dialect's `encode` produces the bytes and `ReplyKind`; any error from the dialect (model cannot do it, value out of range) is passed through unchanged. On success it stores the reply kind in `expecting`, clears the receive buffer so a late fragment of an earlier answer cannot be taken for this one, and returns the `Request`. Pure computation, no I/O, not async.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_receiver_engine_feed]

Takes bytes received from the radio and returns what they mean, in order. `bytes` is borrowed and may be any fragment, including empty or several lines; `now` is a monotonic time supplied by the caller (the engine reads no clock). If the buffer already holds a partial line and more than `PARTIAL_FRAME_TIMEOUT` (500 ms) passed since the previous byte, the stale bytes are emitted first as one `Inbound::Unrecognised` and dropped. The new bytes are appended and every complete line (up to the dialect terminator, normally CR) is taken out. For each line the dialect's `status` hook runs first (bare acknowledgements, refusals); if it declines, the dialect's `decode` interprets the line against the reply kind that was expected. After each line `expecting` is reset to `ReplyKind::None` unless the dialect says `more_lines()` is true. A trailing partial line stays buffered. Never fails and never blocks; a line that nothing recognises comes back as `Unrecognised`, not an error.

### `fn open_requests(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_receiver_engine_open_requests]

Returns the commands that make a freshly opened receiver ready to talk, as `Request`s, by converting the dialect's `open_commands` list (bytes plus expected reply) one for one. Takes `&mut self` because it primes the decoder: when the list is not empty it clears the receive buffer and sets `expecting` to the reply kind of the last request. The comment in the code gives the reason: the actor writes and waits for the requests one at a time, so the answer being decoded is always the one for the request just written, and they share one reply kind. An empty list (the default dialect, which sends nothing at open) leaves the engine state untouched. Called by the actor when the port opens and, through `link_restored`, after a reconnect. Cannot fail; never contains a transmit command.

### `fn link_restored(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_receiver_engine_link_restored]

Called by the actor when the serial link has been re-established after a loss. A half line or an unanswered request left over from the old link must not be taken for the new link's answers, so it clears the receive buffer and resets `expecting` to `ReplyKind::None`, then returns `open_requests()` so the radio is brought back to the same ready state as at first open (for an Icom PCR: powered on and told to stop automatic updates). No arguments besides `&mut self`; no errors. The returned requests are for the caller to write; nothing is written here. For dialects with no open commands the result is empty and only the buffers are cleared.

### `fn open_commands(&mut self, _desc: &RigDescription) -> Vec<(Vec<u8>, ReplyKind)>` [@ANCHOR: radioctl:radioctl_receiver_dialect_open_commands_default]

The default body of the `Dialect::open_commands` hook: it returns an empty vector, so a receiver dialect sends nothing when the rig is opened or reconnected unless it overrides this. `_desc` is the rig description, ignored here; a dialect that overrides the method may read it to choose commands by model. An override returns the commands in the order to send them, each with the reply kind expected; they must never transmit, because a receiver cannot. Takes `&mut self` so an override may keep state. Pure, no errors, no I/O.

### `pub fn new(desc: &RigDescription) -> Result<VirtualReceiver, RigError>` [@ANCHOR: radioctl:radioctl_receiver_virtual_new]

Builds the virtual (simulated) radio of a receiver description, used by the virtual rig and conformance tests as the twin of the real protocol. `desc` is borrowed; it must have a `[receiver]` section or the call returns `ErrorKind::Unsupported` ("description lacks [receiver]"). The receiver protocol picks the twin: AOR chooses the AR7030, the AOR-classic or the plain AOR twin depending on which `handles(desc)` matches first; Uniden chooses the digital or the plain twin; Rohde and Schwarz the HF (high frequency, 3 to 30 MHz) or the base twin; Icom marine, Icom PCR and JRC have one each. Any error from the chosen twin's own constructor (for example an unknown dialect name) is returned. The result starts with an empty line buffer and a last-byte time of zero. No I/O.

### `pub fn feed(&mut self, bytes: &[u8], now: Duration, state: &mut RigState) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_receiver_virtual_feed]

Plays the radio's side: consumes bytes the controller wrote and returns the bytes the radio answers. `bytes` is borrowed; `now` is a caller-supplied monotonic time; `state` is the shared virtual rig state, mutated by the twin as a real command would change the radio (frequency, mode). If the previous byte is more than 500 ms old the half command in the buffer is discarded. The twin's `raw` hook gets first refusal for protocols that are not line oriented; if it answers, that answer is returned at once and the line framing is skipped. Otherwise bytes are buffered, each complete line (up to the twin's terminator) has line feeds removed and is trimmed and given to the twin's `line`, and all answers are concatenated. No error value: bad commands are answered the way the real radio refuses them. Synchronous, no I/O.

## `crates/radioctl-protocols/src/aor.rs`

### `pub fn engine(desc: RigDescription) -> Result<AorEngine, RigError>` [@ANCHOR: radioctl:radioctl_aor_engine]

Builds the protocol engine for an AOR receiver or scanner (AR8000, AR8200, AR8600, AR5000, AR5000A, AR2700, SR2200). `desc` is taken by value and kept inside the engine. It is a one-line wrapper over `LineEngine::new` with `ReceiverProtocol::Aor`, so it returns `ErrorKind::Unsupported` ("not a ... description") when the description is not of the receiver family with the AOR protocol, and passes through the error from `AorDialect::from_description` when the dialect name is unknown. The engine starts with nothing expected and an empty buffer. These radios cannot transmit. No I/O.

### `fn from_description(desc: &RigDescription) -> Result<AorDialect, RigError>` [@ANCHOR: radioctl:radioctl_aor_dialect_from_description]

Chooses the AOR command-set variant from the description's `receiver.dialect` string (empty if absent). `ar8000` selects the hex, bit-7-masked signal style and allows `VA`/`VB` frequency tags; `ar8200` and `ar8600` select status-plus-decimal signal; `ar5000` and `ar2700` status-plus-hex; `sr2200` the `LB` dB style and also sets `echo_frequency` (the SR2200 takes the frequency unrounded and answers with it). Any other name returns `ErrorKind::Unsupported` ("unknown AOR dialect"). `desc` is only borrowed. Pure, no I/O.

### `fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError>` [@ANCHOR: radioctl:radioctl_aor_dialect_encode]

Spells an operation as an AOR command ended with CR, with the reply to expect. Supported: `GetFrequency(A)` sends `RX` expecting a frequency observation; `SetFrequency(A, hz)` sends `RF` and ten digits (rounded to 50 Hz by `round_to_50_hz`, or unrounded on the SR2200, which expects an `RF` echo as the acknowledgement); `GetMode` sends `MD`; `SetMode` sends `MD` and the description's one-character mode code; `GetSignal` sends `LM` (`LB` on the SR2200). A frequency above 9,999,999,999 Hz (after rounding where it applies) gives `ErrorKind::Unsupported` ("frequency out of range"); a mode missing from the description's table gives Unsupported too, as does every other operation (including VFO (variable frequency oscillator, the tuning control) B and all PTT/transmit operations), "not implemented for the AOR engine". Settings expect `ReplyKind::Ack`. No I/O.

### `fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_aor_dialect_decode]

Interprets one received line (no terminator, no line feed) given the reply that was expected. If an observation was expected: a frequency is read by `tagged_frequency`, a mode by finding `MD` and mapping the next byte through the description's code table, a signal by `signal_reading`; success returns one `Inbound::Observation` (frequency always reported for VFO A). If the line cannot be parsed it returns `Inbound::Unrecognised` with its bytes, never an error. If an `Ack` was expected, only the SR2200 case matters: a line starting `RF` is its acknowledgement and gives `Inbound::Ack`; any other line is Unrecognised. Empty lines and `?` refusals never reach here because the default `status` hook handles them first.

### `fn tagged_frequency(&self, line: &str) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_aor_tagged_frequency]

Finds the frequency digits in an `RX` answer. It tries the tag `RF` and, on a dialect with `vfo_tags` (AR8000), then `VA` and `VB`, in that order; for the first tag that occurs anywhere in `line` it takes the run of ASCII digits straight after it and parses them as hertz (`u64`). Returns `None` when no tag is present, no digits follow, or the number does not parse (overflow). `line` is borrowed and has no terminator. Does no range or plausibility check; a value of zero parses as `Hz(0)`. Pure.

### `fn signal_reading(&self, line: &str) -> Option<u16>` [@ANCHOR: radioctl:radioctl_aor_signal_reading]

Extracts the raw signal value from a signal answer. The line must start with `LM` (`LB` for the SR2200) and be at least four characters long, otherwise `None`. By style: AR8000 (`HexMasked`) takes two hex digits after the tag and clears bit 7 (the squelch-status bit); `StatusDecimal` (AR8200, AR8600) skips a status character and parses decimal; `StatusHex` (AR5000, AR2700) skips a status character and parses hex; SR2200 reads a signed decimal dB figure after `LB ` and adds Hamlib's offset of 73, floored at zero. Trailing blanks are trimmed. Non-numeric text, or a result that does not fit `u16`, gives `None`. The unit is the radio's raw reading, not calibrated dB.

### `pub fn round_to_50_hz(hz: Hz) -> Option<u64>` [@ANCHOR: radioctl:radioctl_aor_round_to_50_hz]

Rounds a frequency in hertz to the multiple of 50 Hz the AOR `RF` command takes, as Hamlib does: within each hundred, a remainder below 25 rounds down to 0, below 75 goes to 50, otherwise up to the next 100 (so 24 gives 0, 25 gives 50, 74 gives 50, 75 gives 100). Returns the value as `u64` hertz, or `None` if the rounded result exceeds 9,999,999,999, the largest the ten-digit field holds. `hz` is passed by value (`Hz` newtype of `u64`). Pure, no panics.

### `pub fn new(desc: &RigDescription) -> Result<AorTwin, RigError>` [@ANCHOR: radioctl:radioctl_aor_twin_new]

Builds the virtual AOR radio from a description. It derives the dialect with `AorDialect::from_description` (so an unknown dialect name returns `ErrorKind::Unsupported`), copies the list of valid mode code bytes from the description's `mode_codes`, and keeps a clone of the description so it can translate between `Mode` and code characters. `desc` is borrowed and not changed. The twin keeps no radio state of its own; frequency, mode and signal live in the shared `RigState` passed to each `line` call. No I/O.

### `fn reply(text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_aor_twin_reply]

Formats one answer line the way Hamlib reads an AOR reply: a leading line feed, the text, then the closing carriage return, as bytes. `text` is borrowed and may be empty, which gives `"\n\r"` and is how the twin acknowledges a setting (the engine treats an empty line as `Ack`). An associated function with no `self`; pure, cannot fail, allocates one `Vec`.

### `fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_aor_twin_line]

The twin's reaction to one command line (CR removed) from the controller. `state` is mutated like the radio would be. `RX` answers `RF` plus ten digits of `freq_a` and `MD` and the mode code; `RFnnnnnnnnnn` (exactly ten digits) sets `freq_a` and answers an empty line, or, for the SR2200, echoes `RF` and the digits; a non-SR2200 refuses a frequency that is not a multiple of 50 Hz. `MD` reads the mode; `MDc` sets it if the code is in the description's table. `LM` answers the signal in the model's style (status character `%` for zero signal, `$` otherwise), `LB` does on the SR2200 as signed dB minus 73. Everything else, a malformed or unsupported command, is answered with `?`. No errors, no I/O.

## `crates/radioctl-protocols/src/uniden.rs`

### `pub fn engine(desc: RigDescription) -> Result<UnidenEngine, RigError>` [@ANCHOR: radioctl:radioctl_uniden_engine]

Builds the protocol engine for a Uniden or Bearcat scanner (BC245XLT, BC250D, BC780XLT, BC895XLT, BC898T). `desc` is taken by value. It wraps `LineEngine::new` with `ReceiverProtocol::Uniden`, so it returns `ErrorKind::Unsupported` if the description is not a receiver with the Uniden protocol, and passes on the error of `UnidenDialect::from_description` for an unknown dialect. The BCD-396T and BCD-996T digital models use another backend. Scanners cannot transmit. No I/O.

### `fn from_description(desc: &RigDescription) -> Result<UnidenDialect, RigError>` [@ANCHOR: radioctl:radioctl_uniden_dialect_from_description]

Picks the Uniden command set from the description's `receiver.dialect` string. `uniden` gives the normal dialect, which reads the frequency with `RF`; `bc780` gives the BC780XLT variant (`frequency_via_sg` true), which reads it through `SG` as Hamlib does. Any other name, including an empty one, returns `ErrorKind::Unsupported` ("unknown Uniden dialect"). `desc` is borrowed. Pure, no I/O.

### `fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError>` [@ANCHOR: radioctl:radioctl_uniden_dialect_encode]

Spells an operation as a Uniden command ended by CR. `GetFrequency(A)` sends `RF` (`SG` on the BC780XLT); `SetFrequency(A, hz)` sends `RF` and eight digits in units of 100 Hz (the value is `hz / 100`, so the last two digits are dropped); more than 99,999,999 units is `ErrorKind::Unsupported` ("frequency out of range"). `GetMode` sends `RM`; `SetMode` sends `RM` and a name (`AM`, `FM`, `WFM`), failing with Unsupported when the mode is not in the description or its code is not 1, 2 or 3. `GetSignal` sends `SG`. Every other operation, including transmit, is Unsupported. Settings expect an `Ack`, reads an observation. No I/O.

### `fn status(&mut self, expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>>` [@ANCHOR: radioctl:radioctl_uniden_dialect_status]

Classifies bare status lines before `decode` sees them. `OK` is an acknowledgement: it gives `Inbound::Ack` only when an `Ack` was expected, and nothing otherwise. `NG`, `ORER` and `ERR` are refusals and give `Inbound::Nak`. An empty line produces an empty result (ignored). Any other text returns `None`, meaning the line is data for `decode`. `expecting` is the reply kind of the request in flight; `line` has no terminator. Pure, no errors.

### `fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_uniden_dialect_decode]

Reads a data line into an observation. Only when an observation is expected; any other expectation gives `Inbound::Unrecognised` of the line. Frequency: on the BC780XLT the digits follow the space-F in an `SG` answer, elsewhere they follow `RF`, and `parse_units` converts them to hertz. Mode: the line must start `RM ` and the name goes through `mode_named`. Signal: the line must start `S` and the text up to the next space is parsed as a `u16` (so `S182 F08594375` reads 182). A line that does not fit gives `Unrecognised` carrying its bytes; no error is returned. Pure.

### `fn parse_units(digits: &str) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_uniden_parse_units]

Converts the eight-digit frequency field of a Uniden answer into hertz. The input must be exactly eight ASCII digits, otherwise `None`; the number is in units of 100 Hz and is multiplied by 100 (so `08594375` is 859,437,500 Hz). Used both by the dialect to read answers and by the twin to read commands. `digits` is borrowed. Pure, cannot overflow (at most 99,999,999 times 100).

### `fn mode_named(desc: &RigDescription, name: &str) -> Option<Mode>` [@ANCHOR: radioctl:radioctl_uniden_mode_named]

Maps a radio's mode name to a `Mode`. `NFM` is first treated as `FM`, because radioctl has no narrow-FM mode. The name is then looked up in the table `AM`, `FM`, `WFM` (the `MODE_NAMES` constant) and the result is kept only if the rig description also has a mode code for that `Mode`, so a model that lacks WFM does not report it. Returns `None` for an unknown name or a mode the description omits. Case matters (uppercase only). Pure.

### `pub fn new(desc: &RigDescription) -> Result<UnidenTwin, RigError>` [@ANCHOR: radioctl:radioctl_uniden_twin_new]

Builds the virtual Uniden scanner. It validates the description by calling `UnidenDialect::from_description` (discarding the result), so an unknown dialect name returns `ErrorKind::Unsupported`, and keeps a clone of the description for mode-code lookups. `desc` is borrowed. The twin has no state of its own; frequency, mode and signal are in the shared `RigState` handed to each `line` call. No I/O.

### `fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_uniden_twin_line]

The twin's reaction to one command line (CR removed). Every answer ends with CR. `RF` answers `RF` and eight digits of `freq_a` in units of 100 Hz; `RM` answers `RM` and the name of the current mode (default FM); `SG` answers `S` plus three-digit signal, space, `F` and the eight digits. `RFnnnnnnnn` sets `freq_a` (via `parse_units`) and answers `OK`, or `ERR` if the digits are malformed. `RM name` sets the mode and answers `OK`; `RM NFM` is refused with `NG` because narrow FM is never sent; an unknown name gives `ERR`. Any other command answers `ERR`. Mutates `state`; no I/O.

## `crates/radioctl-protocols/src/conformance.rs`

### `fn receiver_engine(description: &RigDescription) -> Result<Box<dyn ProtocolEngine>, RigError>` [@ANCHOR: radioctl:radioctl_conformance_receiver_engine]

Chooses the engine for a description of the receiver family, by its receiver protocol and, for some protocols, by a `handles(description)` check on a specialised backend (for example AOR resolves to the AR7030, AOR-classic or the plain AOR engine; Uniden to the digital or plain one). Called from the conformance engine factory when the family is `Receiver`. `description` is borrowed and cloned into the engine; the engine is returned boxed as `dyn ProtocolEngine`. Errors come from the chosen engine's constructor; a description with no `[receiver]` section falls to the last arm and returns `ErrorKind::Unsupported` ("no engine for this receiver protocol yet"). No I/O.

## `crates/radioctl-protocols/src/jrc.rs`

### `pub fn engine(desc: RigDescription) -> Result<JrcEngine, RigError>` [@ANCHOR: radioctl:radioctl_jrc_engine]

Builds the protocol engine for a JRC radio (NRD-525, NRD-535D, NRD-545 DSP, JST-145, JST-245). `desc` is taken by value. A thin wrapper over `LineEngine::new` with `ReceiverProtocol::Jrc`: it returns `ErrorKind::Unsupported` if the description is not a receiver with the JRC protocol and passes on the dialect error for an unknown dialect name. The JST models are transceivers, so the engine can encode PTT (push to talk) for them; the transmit gate is applied by the caller. No I/O.

### `fn from_description(desc: &RigDescription) -> Result<JrcDialect, RigError>` [@ANCHOR: radioctl:radioctl_jrc_dialect_from_description]

Selects the JRC command set from the description's `receiver.dialect`: `nrd535` (8 frequency digits, 14-character `I` answer, state read through the pair `I1` then `I0`), `nrd545` (10 digits, 18-character answer, a single `I`), `nrd525` (write-only, no read back) and `jst145` (which also serves the JST-245). Any other name returns `ErrorKind::Unsupported` ("unknown JRC dialect"). `desc` is borrowed; pure, no I/O.

### `fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError>` [@ANCHOR: radioctl:radioctl_jrc_dialect_encode]

Spells an operation in the model's JRC command set. Frequency: `F` plus zero-padded hertz (8 or 10 digits on NRD, ended by CR); NRD-525 sends 8 digits in units of 10 Hz with no terminator; JST sends 8 digits of hertz and the VFO letter `A` or `B`. Out-of-range values give `ErrorKind::Unsupported` ("frequency out of range"). Reads of frequency and mode send `I` (`I1` and `I0` on the NRD-535D) and expect an observation; signal on NRD sends `M`. Mode: `D` and the description's mode character (the NRD-525 sends it without the CR). On JST only, `SetPtt(on)` sends `X1` or `X0` and `GetPtt` sends `X`; this keys the transmitter, so the caller must hold the transmit gate. Settings use `ReplyKind::None` because the radios send no acknowledgement. Anything else: Unsupported.

### `fn status(&mut self, _expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>>` [@ANCHOR: radioctl:radioctl_jrc_dialect_status]

The JRC override of the status hook. These radios never send an acknowledgement or a refusal, so the only line it handles is an empty one, which gives an empty result (ignored); every other line returns `None` and goes on to `decode`. The expected reply kind is ignored. Because the dialect has no `Nak` form, a refused or failed command can only show up as a timeout or as an unrecognised line. Pure.

### `fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_jrc_dialect_decode]

Interprets one received line when an observation is expected (otherwise it returns `Inbound::Unrecognised`). Frequency comes from `info_frequency`, mode from `info_mode`, signal from a line of `M` and exactly three digits parsed as `u16`, and PTT from `X0` (false) or `X1` (true). Success returns one `Inbound::Observation` (frequency for VFO A). A line that does not match the expected shape, or any other field, returns `Unrecognised` with the bytes; it never errors. Pure.

### `fn info_checked<'a>(&self, line: &'a str) -> Option<&'a str>` [@ANCHOR: radioctl:radioctl_jrc_info_checked]

Validates the shape of an `I` answer before its fields are read, returning the same borrowed string when it passes. The line must start with `I`. On NRD models its length plus one (the CR) must equal the model's answer length (14 for NRD-535D, 18 for NRD-545). On the JST the line must be at least 12 characters. NRD-525 has no read-back so it always gives `None`. A wrong-length answer (a truncated or merged line) is thereby rejected rather than misread. Pure.

### `fn info_frequency(&self, line: &str) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_jrc_info_frequency]

Reads the frequency from an `I` answer. The line must pass `info_checked`. The digits are taken from character index 4 (the fifth character): `digits` long on NRD (8 or 10), and 8 on JST (the module comment says after three characters; the code uses the same offset 4 for both, so check against a real JST answer, which is `unknown` here). All characters must be ASCII digits; the value is parsed as hertz. Anything else gives `None`. Pure.

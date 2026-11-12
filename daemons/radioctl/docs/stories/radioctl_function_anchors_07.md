# radioctl function anchors, batch 07

Function anchors for `radioctl-core` (capability sets, the mode-code lookups and the two built-in descriptions) and for the Drake, Alinco and Racal vendor engines and virtual twins in `radioctl-protocols`, written to the ADR 0113 standard. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
The Drake and Racal engines are receive-only and have no PTT. The Alinco DX-SR8 engine can encode `SetPtt`; an engine only checks that the description lists the capability, and the lock, band check and transmit gate are applied by the caller that owns the engine (unknown from these files; check `radioctld`), never by the engine or the twin.

## `crates/radioctl-core/src/capability.rs`

### `pub fn of(list: &[Capability]) -> CapabilitySet` [@ANCHOR: radioctl:radioctl_core_capability_set_of]

Builds a `CapabilitySet` from a slice of capabilities by copying each into an ordered set. The slice is borrowed and not kept. Duplicates collapse to one entry and order in the list does not matter, because the set is a `BTreeSet`. It cannot fail, and an empty slice gives an empty set. It is how the built-in descriptions (IC-7300, TS-2000) and tests declare what a rig can do.

### `pub fn has(&self, capability: Capability) -> bool` [@ANCHOR: radioctl:radioctl_core_capability_set_has]

Reports whether a capability is present in the set. Every engine calls it at the top of `encode` with `op.required_capability()`, so an operation the description does not list is refused with an `Unsupported` error before any bytes are built. Borrows the set, takes the capability by value, cannot fail, and is a pure logarithmic lookup.

### `pub fn iter(&self) -> impl Iterator<Item = Capability> + '_` [@ANCHOR: radioctl:radioctl_core_capability_set_iter]

Returns an iterator over the capabilities in the set, by value (`Capability` is `Copy`). The order is the stable `Ord` order of the enum, not insertion order, so listings such as the `\dump_caps` text and rig-file round trips are deterministic. It borrows the set for the life of the iterator and does not allocate. No errors.

## `crates/radioctl-core/src/description.rs`

### `pub fn mode_code(&self, mode: Mode) -> Option<u8>` [@ANCHOR: radioctl:radioctl_core_description_mode_code]

Looks up the wire code a model uses for an operating mode, by scanning the description's `mode_codes` list of `(Mode, u8)` pairs and returning the first match. The code is a raw byte whose meaning is per family: a CI-V binary value such as 0x01 for USB, or an ASCII character such as `b'2'` for a Kenwood or Drake. `None` means the model has no code for that mode, and engines then refuse `SetMode` with an `Unsupported` error. Pure.

### `pub fn code_mode(&self, code: u8) -> Option<Mode>` [@ANCHOR: radioctl:radioctl_core_description_code_mode]

The reverse of `mode_code`: finds the `Mode` for a wire code received from the rig, scanning `mode_codes` for the first pair with that code. `None` means the model's description does not list the code; engines treat that as an unrecognised reply instead of guessing, which is why a radio reporting a mode the description lacks shows up as an error and not a wrong mode. Pure, cannot fail.

### `pub fn ic7300() -> RigDescription` [@ANCHOR: radioctl:radioctl_core_description_ic7300]

Constructs an in-memory Icom IC-7300 style description: family CI-V, radio address 0x94, controller 0xE0, five frequency bytes, standard command set, no echo, the capabilities read and set frequency, read and set mode, read and set PTT, signal, power, CW and transceive, the CI-V mode codes 0x00 to 0x08, a 1000 ms timeout with 3 retries, and a default of 14.074 MHz USB. It comes from our own simulator and relay bridge code, not a radio, and is unverified; the shipped `rigs/` files are the real data. Used by tests and the virtual rig. Allocates a new value each call and cannot fail.

### `pub fn ts2000() -> RigDescription` [@ANCHOR: radioctl:radioctl_core_description_ts2000]

Constructs an in-memory Kenwood TS-2000 style description: family Kenwood ASCII, model id 19, classic dialect, the capabilities read and set frequency, second VFO, read and set mode, set PTT, signal and transceive, ASCII mode codes `1` to `7` and `9`, a 200 ms timeout with 3 retries, and a default of 14.074 MHz USB. It is derived from our own bridge code and is unverified against a radio. Used by tests and the virtual rig. Pure, allocates, cannot fail.

## `crates/radioctl-protocols/src/alinco.rs`

### `fn end(self) -> &'static [u8]` [@ANCHOR: radioctl:radioctl_alinco_dialect_end]

Gives the command terminator of an Alinco dialect: a single carriage return for the DX-77 and CR LF for the DX-SR8. `command` appends it to every request, and the twin's framing mirrors it. Takes the dialect by value, returns a static slice, cannot fail, and is private to the module.

### `fn command(&self, text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_alinco_command]

Builds the bytes of one request: the command text (for example `AL3H` or `AL~RR_RXF`) as ASCII followed by the dialect's terminator from `end`. `text` is borrowed and copied into a new vector; the engine state is unchanged. It does no validation, since `encode` has already produced well-formed text. Pure and cannot fail.

### `pub fn from_name(name: &str) -> Option<Dialect>` [@ANCHOR: radioctl:radioctl_alinco_dialect_from_name]

Maps the dialect name used in a rig description's vendor block to a `Dialect`: `dx77` or `dxsr8`. Matching is exact and case sensitive; anything else returns `None`, and `AlincoEngine::new` and `AlincoTwin::new` turn that into an `Unsupported` error. The input is borrowed. Pure.

### `pub fn decode_status(line: &[u8]) -> Option<(u8, Hz)>` [@ANCHOR: radioctl:radioctl_alinco_decode_status]

Decodes the DX-77 status line returned by `AL3H`, which must be exactly 26 bytes (without CR LF). It returns the mode character at index 3 and the receive frequency in hertz from the ten decimal digits at indexes 6 to 15 (`Hz`). A wrong length, or digits that are not all ASCII 0 to 9, returns `None`. The two hexadecimal setting characters at the start are not decoded. Pure; borrows the slice.

### `pub fn new(desc: RigDescription) -> Result<AlincoEngine, RigError>` [@ANCHOR: radioctl:radioctl_alinco_engine_new]

Builds the client engine for an Alinco vendor description, taking ownership of the description. It requires the description to be a serial-vendor one (`vendor_of`) and to name a known dialect (`dx77` or `dxsr8`); otherwise it returns a `RigError` of kind `Unsupported`. The new engine has no request outstanding and an empty receive buffer. It does no I/O.

### `fn decode_reply(&self, field: Field, line: &[u8]) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_alinco_decode_reply]

Interprets the second line of an Alinco answer for the field that was asked. DX-77 frequency and mode come from the 26-character status line (`decode_status`), with the lower CW code `2` read as the upper code `3` that the description lists. DX-SR8 frequency, mode and PTT are plain decimal numbers (hertz, a two-digit mode code, and a PTT flag where nonzero is on). A mode code the description does not list, a malformed number or an unsupported combination returns `None`. Pure; the caller reports the line as unrecognised.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_alinco_engine_encode]

The `ProtocolEngine` encode step for Alinco. It first refuses any op whose capability the description lacks (`Unsupported`). It then builds the bytes: get and set frequency on VFO A (DX-77 `AL3H` / `AL0B<digits>`, DX-SR8 `AL~RR_RXF` / `AL~RW_RXF<8 digits>`), get and set mode (`AL2G<c>`, `AL~RW_RFM<nn>`), and on the DX-SR8 only get and set PTT (`AL~RR_PTT`, `AL~RW_PTT<00|01>`). A frequency of 10 GHz or more, a mode with no code, VFO B and every other op return `Unsupported`. A set expects an `OK` ack; a get expects an observation. It records the pending request, resets the echo flag and clears the receive buffer, so only one request may be outstanding. Pure bytes; `SetPtt` keys the radio, and the transmit gate is the caller's job.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_alinco_engine_feed]

Accepts bytes received from the radio at time `now` and returns zero or more `Inbound` events. A partial line left over longer than the frame timeout is discarded and reported as `Unrecognised`. Each LF-ended line is handled: with no request outstanding it is `Unrecognised`; the first line after a request is the radio echoing the command and is skipped; the second line is the answer. For a set, `OK` gives `Ack` and anything else `Nak`; for a read, a decodable value gives an `Observation` and anything else `Unrecognised`. A trailing CR is removed. It does no I/O and never blocks.

### `pub fn new(desc: &RigDescription) -> Result<AlincoTwin, RigError>` [@ANCHOR: radioctl:radioctl_alinco_twin_new]

Builds the virtual Alinco radio from a description, borrowing it. The twin starts at the description's default frequency in hertz, with the mode code of the default mode (0 if the description has none), PTT off, and the list of the description's mode codes it will accept. Like the engine it returns an `Unsupported` error if the description is not a serial-vendor one or the dialect is unknown. No I/O.

### `fn answer(&mut self, cmd: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_alinco_twin_answer]

Works out the second line (without CR LF) a real radio would send for one command text, and updates the twin's state for settings. DX-77: `AL3H` returns a 26-character status line, `AL0B` sets the frequency, `AL2G` sets the mode if its code is one the description lists. DX-SR8: the `RR_RXF`, `RR_RFM` and `RR_PTT` reads and the `RW_RXF`, `RW_RFM`, `RW_PTT` writes. Filter writes (`AL2J`, `AL~RW_NAR`) are acknowledged without keeping the width. Settings answer `OK`. An unknown command, an unparsable number or an unknown mode code returns `None` and the twin stays silent. Mutates the twin only.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_alinco_twin_feed]

The virtual radio's receive side. It adds bytes received at `now` to its buffer and, for each complete command (CR terminated on the DX-77, LF on the DX-SR8), strips a trailing CR, asks `answer` and, if there is an answer, emits the command echoed back plus a CR LF, then the answer plus CR LF. Commands with no answer produce no bytes. The result is the bytes to put on the simulated wire. Mutates the twin, no I/O, never blocks.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_alinco_twin_front_panel]

Applies a front-panel action from a transcript script to the twin, as if an operator turned a knob. `Panel::Frequency(hz)` sets the frequency in hertz and `Panel::Ptt(on)` sets the transmit flag; any other panel action returns an `Err` string naming the unsupported knob. It returns an empty byte vector, because the Alinco radios send nothing unsolicited. Affects only the in-memory twin; no real hardware.

## `crates/radioctl-protocols/src/drake.rs`

### `pub fn encode_frequency(hz: Hz) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_drake_encode_frequency]

Builds the Drake set-frequency command: `F`, seven zero-padded decimal digits of the frequency in units of 10 Hz, then CR (for 14.2 MHz, `F1420000` CR). The frequency must be an exact multiple of 10 Hz and the digit count must fit in seven digits (under 100 MHz, at most 99,999,990 Hz); otherwise it returns `None` and the engine reports `Unsupported`. Takes `Hz` by value, pure.

### `pub fn decode_frequency(line: &[u8]) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_drake_decode_frequency]

Decodes a Drake read-frequency answer of the form `*fffff.ff*uHz` (CR and LF already removed). The digits before the point are kilohertz when the unit letter `u` is anything but `m`, or megahertz when it is `m` or `M`; two digits follow the point. The result is exact integer hertz (hundredths times 10, or times 10,000 for megahertz). Anything that does not match the shape, has non-digits, or overflows returns `None`. Pure; borrows the line.

### `pub fn decode_mode(mode: u8, width: u8) -> Option<Mode>` [@ANCHOR: radioctl:radioctl_drake_decode_mode]

Decodes the mode from the mode and width bytes of an `RM` state line (bytes 3 and 4 of the 8-byte line; the synchronous-detector byte is ignored). The width byte, after masking flag bits, must be an ASCII digit `0` to `4`; if it is a plain digit `0` to `4` (bit 3 clear) the lower-sideband family applies (0 LSB, 1 RTTY, 2 FM), and with bit 3 set the upper family (0 USB, 1 CW, 2 AM). The mode byte is masked with 0x33 first. Unknown combinations return `None`. Pure.

### `pub fn new(desc: RigDescription) -> Result<DrakeEngine, RigError>` [@ANCHOR: radioctl:radioctl_drake_engine_new]

Builds the engine for a Drake R-8A or R-8B description, taking it by value. It must be a serial-vendor description whose dialect is `r8`; anything else returns `Unsupported`. The engine starts with nothing pending and an empty receive buffer. The radios are receivers with no PTT, so nothing in this engine can transmit. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_drake_engine_encode]

Turns an operation into the bytes for the radio. Refuses with `Unsupported` any op whose capability the description lacks. Supported: get frequency on VFO A (`RF` CR), set frequency on VFO A (`encode_frequency`, which fails for a frequency that is not a multiple of 10 Hz below 100 MHz), get mode (`RM` CR) and set mode (`M<code>` CR, using the description's code, refused if the mode has no code). The set requests expect an acknowledgement, the gets an observation. Everything else, including VFO B, is `Unsupported`. It remembers what is pending and discards the receive buffer, so one request at a time. No PTT exists.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_drake_engine_feed]

Processes bytes received at `now` and returns events. A stale partial frame older than the timeout is dropped and reported `Unrecognised`. Each LF-ended line, with a trailing CR removed, is matched against the pending request: after a set, any line is accepted as an `Ack` (Hamlib does not examine the content); for a frequency read, the line is decoded with `decode_frequency`; for a mode read, the six-byte body is decoded with `decode_mode` and accepted only if the description has a code for that mode. Anything else is `Unrecognised`. Only one request is tracked, and the pending state is cleared on every line. No I/O.

### `pub fn new(desc: &RigDescription) -> Result<DrakeTwin, RigError>` [@ANCHOR: radioctl:radioctl_drake_twin_new]

Builds a virtual Drake receiver from a vendor description (borrowed). The twin holds the description's default frequency in 10 Hz units and default mode, and a copy of its mode codes. It returns `Unsupported` when the description is not a serial-vendor one. Starts with an empty buffer; no I/O.

### `fn state_line(&self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_drake_twin_state_line]

Builds the `RM` answer for the twin's current mode: the bytes `R`, `M`, a space, the mode byte, the width byte, a synchronous byte of `0`, then CR LF (eight bytes). The mode and width bytes encode LSB, RTTY and FM in the lower family and USB, CW and AM in the upper (width `9` has bit 3 set); other modes, such as a mode the twin cannot hold, fall back to AM. Reads only the twin; cannot fail.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_drake_twin_feed]

The virtual receiver's command parser. It buffers bytes received at `now` and, for each CR-terminated command, answers: `RF` with a `*fffff.ff*kHz` line, `RM` with `state_line`, `F` plus seven digits by storing the frequency and acknowledging with CR LF, and `M` plus a character by switching to that mode if the description has the code and acknowledging. Bad digits, an unknown mode code or an unknown command get `?` CR LF. Returns the response bytes, in order. Mutates the twin; no I/O; never blocks.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_drake_twin_front_panel]

Applies a scripted front-panel action to the twin: `Panel::Frequency(hz)` sets the frequency (stored in 10 Hz units, so a finer value is truncated) and `Panel::Mode(mode)` sets the mode. Any other panel action returns an `Err` naming the knob a Drake receiver lacks. On success returns an empty vector, since the receiver sends nothing on its own. Only the in-memory twin changes.

## `crates/radioctl-protocols/src/racal.rs`

### `pub fn format_mhz(hz: Hz) -> String` [@ANCHOR: radioctl:radioctl_racal_format_mhz]

Formats a frequency as megahertz text for a Racal command: the whole megahertz, and when the remainder is not zero a point and the fractional digits with trailing zeros removed (14.2 MHz is `14.2`, 7 MHz is `7`, 500 kHz is `0.5`, resolution 1 Hz). It is pure integer arithmetic, so there is no rounding error, and it cannot fail. The result is a new string.

### `pub fn parse_mhz(text: &[u8]) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_racal_parse_mhz]

Parses a megahertz decimal such as `14.2`, `7` or `0.5` into integer hertz. It allows a whole part with up to six fractional digits; more than six decimals, an empty or non-digit part, a trailing point with no digit, or an overflow returns `None`. The fraction is scaled exactly (`.2` is 200,000 Hz). Pure, borrows the bytes, the inverse of `format_mhz`.

### `pub fn new(desc: RigDescription) -> Result<RacalEngine, RigError>` [@ANCHOR: radioctl:radioctl_racal_engine_new]

Builds the engine for a Racal-family receiver description (RA6790, RX-340, RX-331 or RA3702 dialects), taking it by value. It returns an `Unsupported` error if the description is not a serial-vendor one or its dialect name is not known. The engine starts with nothing pending and an empty buffer. These are receivers: no PTT. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_racal_engine_encode]

Builds the bytes for an operation in the description's dialect, after refusing (`Unsupported`) any op whose capability the description lacks. Supported ops are get and set frequency on VFO A and get and set mode. Each command is the dialect prefix (such as `$0`), the verb and CR: `TF` or `QF` to ask the frequency, `F<text>` to set it, `TDI` or `QM` to ask the mode, `D<code>` or `M<code>` to set it. Setting commands expect no reply. A mode with no code is `Unsupported`, as is any other op. It records what is pending and clears the receive buffer.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_racal_engine_feed]

Processes bytes received at `now` and returns events. A stale partial frame is dropped and reported `Unrecognised`. Each CR-terminated line has the dialect's reply prefix removed; if that fails the line is `Unrecognised`. A frequency answer `F<number>` gives a frequency observation (megahertz, or hertz on the RA3702). A mode answer gives a mode observation: `M<code>` on the RA3702, `D<code>I<bandwidth>` elsewhere, where the two CW codes 3 and 4 read as whichever the description lists. Anything unexpected, or an answer with no matching request, is `Unrecognised`. The pending state is cleared for every line. No I/O.

### `pub fn new(desc: &RigDescription) -> Result<RacalTwin, RigError>` [@ANCHOR: radioctl:radioctl_racal_twin_new]

Builds a virtual Racal receiver from a description (borrowed): the description's default frequency, the mode code of the default mode (`1`, AM, if none is listed) and a copy of the list of mode codes. It returns `Unsupported` if the description is not a serial-vendor one or the dialect is unknown. Empty buffer; no I/O.

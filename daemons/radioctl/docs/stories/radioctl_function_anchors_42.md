# radioctl function anchors, batch 42

Function anchors for the `radioctl-protocols` crate: the rest of the AOR AR3030/AR3000A engine and twin (`aor_classic.rs`), the AOR AR7030 binary engine and twin (`ar7030.rs`), and the Rohde and Schwarz HF engine (`rs_hf.rs`) up to its decoder. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
None of these radios is keyed by these functions: the dialects have no PTT operation (an operation not listed is refused as `Unsupported`), so the transmit gate, lock and band check never come into play here.

## `crates/radioctl-protocols/src/aor_classic.rs`

### `fn megahertz(hz: u64, decimals: u32) -> String` [@ANCHOR: radioctl:radioctl_aorc_megahertz]

Writes a frequency in hertz as megahertz text using integer arithmetic only, so no floating-point rounding can alter a digit. `hz` is the frequency; `decimals` is how many decimal places to print (the engine uses 6 for the AR3030, giving `7.074000`, and 5 for the AR3000A, giving `7.07400`) and should be 0 to 6, because the divisor is `10^(6 - decimals)` and a larger value would underflow in debug builds. Digits below the printed precision are truncated, not rounded: the AR3000A caller rounds to 50 Hz first. Returns the whole-MHz part, a point and the zero-padded fraction. Pure, allocates one string.

### `fn parse_megahertz(text: &str) -> Option<u64>` [@ANCHOR: radioctl:radioctl_aorc_parse_megahertz]

Parses a decimal megahertz entry (`7.074`, `0145.50000`, `.5`) into hertz for the virtual radio. The text is split at the first point; both parts must be only ASCII digits (a missing whole or fraction part is allowed, but not both empty). The fraction is padded with zeros or cut to six digits, the value converted to hertz with checked arithmetic, and the result rounded to the nearest 10 Hz (half rounds up). Returns `None` for an empty string, any other character, a sign, or an overflow. Pure, no panics.

### `fn from_description(desc: &RigDescription) -> Result<AorClassicDialect, RigError>` [@ANCHOR: radioctl:radioctl_aorc_dialect_from_description]

Builds the AR3030/AR3000A dialect from the description. It uses `style_of`: `ar3030` and `ar3000a` give the matching style; any other dialect name, or none, returns `ErrorKind::Unsupported` ("not an AR3030 or AR3000A dialect"). `desc` is borrowed; the dialect holds only the style, no counters or buffers. Pure, no I/O.

### `fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError>` [@ANCHOR: radioctl:radioctl_aorc_dialect_encode]

Spells an operation for one of the two models. Line ending: CR for the AR3030, CR LF for the AR3000A. Reads: `GetFrequency(A)` and `GetMode` send `D` (the dial state), `GetSignal` sends `Y`; all expect an observation. Settings: on the AR3030 `SetFrequency(A, hz)` sends megahertz with six decimals and expects an `Ack` (range 30 kHz to 30 MHz, else `Unsupported` "frequency out of range"); on the AR3000A the frequency is rounded to 50 Hz, must lie within 100 kHz to 2036 MHz, is sent in megahertz with five decimals, and expects no reply, since that radio does not acknowledge settings. `SetMode(mode)` sends the description's one-letter mode code (`Unsupported` if the description lacks it), expecting an `Ack` on the AR3030 and nothing on the AR3000A. Everything else, including VFO B, is `Unsupported`. No transmit. No I/O.

### `fn status(&mut self, expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>>` [@ANCHOR: radioctl:radioctl_aorc_dialect_status]

Handles the lines that carry no data. An empty line is the AR3030's acknowledgement: it gives `Inbound::Ack` when an `Ack` was expected and nothing otherwise. A line that is just `>` is the AR3000A's ready prompt, which is not a reply, so it is swallowed (empty result). Every other line returns `None` and goes to `decode`. This dialect has no refusal form, so `Nak` is never produced. Pure.

### `fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_aorc_dialect_decode]

Reads a data line when an observation is expected (any other expectation gives `Inbound::Unrecognised`). The result depends on style and field. Frequency and mode come from the dial-state line parsed by `ar3030_state` or `ar3000a_state`; the mode letter is mapped through the AR3030 letter table or the AR3000A read table (`Q` FM, `R` WFM, `S` AM, `T` LSB, `U` USB, `V` CW) and kept only if the description has a code for that mode. The AR3030 signal is two hex digits no larger than 0x3F. The AR3000A signal is `%` (squelch closed) as 0, or a letter `A` to `P` as 1 to 16. Anything that does not match gives `Unrecognised` carrying the bytes; no error is returned. Pure.

### `fn ar3030_state(line: &str) -> Option<(u64, char)>` [@ANCHOR: radioctl:radioctl_aorc_ar3030_state]

Parses an AR3030 dial-state line such as `D R0 G0 B0 T0 F00707400 A`. The line must start with `D`; after the first `F` exactly eight ASCII digits are the frequency in units of 10 Hz (multiplied by 10 to give hertz), and the first non-blank character after them is the mode letter. Returns `(hertz, letter)`, or `None` when the `D` prefix, the `F`, the eight digits or the letter is missing. `line` is borrowed with no terminator. Pure, no panics (all slicing is checked).

### `fn ar3000a_state(line: &str) -> Option<(u64, char)>` [@ANCHOR: radioctl:radioctl_aorc_ar3000a_state]

Parses an AR3000A dial-state line, which starts with a memory channel and other fields and contains `Y` before the frequency. After the first `Y`, digits are collected, a point or a space is skipped (it may sit between the fourth and fifth digit), and the first other character ends the scan and is the mode letter. Exactly nine digits are required (megahertz with five decimals, so the value is in units of 10 Hz and is multiplied by 10). Returns `(hertz, letter)`, or `None` if there is no `Y`, the digit count is not nine, or no letter follows. Pure.

### `pub fn new(desc: &RigDescription) -> Result<AorClassicTwin, RigError>` [@ANCHOR: radioctl:radioctl_aorc_twin_new]

Builds the virtual AR3030 or AR3000A. The dialect is derived with `from_description` (so an unsupported dialect name returns `ErrorKind::Unsupported`) and only its style is kept, together with a clone of the description for mode-letter lookups. `desc` is borrowed. Frequency, mode and signal live in the shared `RigState`. No I/O.

### `fn reply(text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_aorc_twin_reply]

Formats one answer line the way both radios send it: the text followed by CR and LF, as bytes. `text` is borrowed and may be empty, which gives a bare CR LF (the AR3030's acknowledgement). An associated function without `self`; pure and infallible.

### `fn state_line(&self, state: &RigState) -> String` [@ANCHOR: radioctl:radioctl_aorc_twin_state_line]

Produces the dial-state answer to `D` from the shared state. For the AR3030: `D R0 G0 B0 T0 F`, the frequency as eight digits in units of 10 Hz, a space and the mode letter (default `A`, AM, when the mode has no letter). For the AR3000A: the fixed prefix `#--J0X Z000.05 Y`, the frequency as nine digits of 10-Hz units with a point after the fourth, then the mode letter from the read table (default `S`, AM). The attenuator, AGC, filter, tone, memory and step fields are constants. `state` is borrowed read-only. Pure.

### `fn apply(&mut self, state: &mut RigState, line: &str)` [@ANCHOR: radioctl:radioctl_aorc_twin_apply]

Applies a frequency and/or mode entry to the shared state, as the radio does when it is typed in: `7.074000`, `C` or `128.8A`. The line is split at its first letter; a number part is read by `parse_megahertz` and used only if it is within the model's range (AR3030 30 kHz to 30 MHz, AR3000A 100 kHz to 2036 MHz), and the AR3000A rounds it to 50 Hz. A letter part picks the mode whose code character in the description's table matches the first letter; an unknown letter is ignored. A part that fails its check is silently left unchanged, since the radios give no refusal. Mutates `state`; no reply here, no I/O.

### `fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_aorc_twin_line]

The virtual radio's reaction to one command line (terminator removed). `D` answers the dial-state line. `Y` answers the signal: AR3030 two hex digits of the signal capped at 0x3F; AR3000A `%` for zero or a letter from `A` (1) to `P` (16 or more). An empty line answers a bare CR LF. Any other line is applied as a frequency/mode entry by `apply`; the AR3030 then answers with an empty line (its acknowledgement), the AR3000A answers nothing. Never refuses. Mutates `state`; no I/O.

## `crates/radioctl-protocols/src/ar7030.rs`

### `pub fn handles(desc: &RigDescription) -> bool` [@ANCHOR: radioctl:radioctl_ar7030_handles]

True when the description's `receiver.dialect` is `ar7030` or `ar7030p` (AR7030 or AR7030 Plus). The engine and twin factories test it before the other AOR cases, because the AR7030 speaks a binary protocol and is served by `Ar7030Engine` instead of the line engine. `desc` is borrowed; a description with no `[receiver]` section gives false. Pure, no errors.

### `fn counts_of(hz: u64) -> u32` [@ANCHOR: radioctl:radioctl_ar7030_counts_of]

Converts a frequency to the radio's 24-bit tuning count. One count is 44.545 MHz divided by 2^24 (about 2.655 Hz), so the count is `hz * 2^24 / 44,545,000` rounded to nearest (half up) in integer arithmetic. The input is first limited to 1 GHz so the multiplication cannot overflow, and the result is limited to 0xFFFFFF. Example: 7,074,000 Hz gives 0x28A77E. The caller (`encode`) refuses frequencies above 32 MHz before calling it. Pure, cannot panic.

### `fn hz_of(counts: u32) -> u64` [@ANCHOR: radioctl:radioctl_ar7030_hz_of]

The inverse of `counts_of`: converts a 24-bit tuning count into hertz, `counts * 44,545,000 / 2^24`, rounded to the nearest hertz. Used to report a frequency read from the radio and by the twin to apply what the controller wrote. Because a count is about 2.655 Hz wide the round trip returns the nearest representable hertz, not always the exact value originally set. `counts` should be below 2^24 (the callers pass three bytes). Pure, no overflow with 24-bit input.

### `fn set_address(page: u8, address: u16) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_ar7030_set_address]

Builds the command bytes that select a memory page and address in the AR7030 protocol, where each byte has a one-nibble operation in its top four bits. Output: `0x50 | page` (select page, only the low four bits of `page` are used), `0x30 | (address >> 4 & 0xF)` (load the H register) and `0x40 | (address & 0xF)` (set the low address byte from H and the low nibble); for an address above 0xFF a fourth byte `0x10 | (address >> 8)` sets the high nibble. Example: page 0, address 0x1A gives `50 31 4A`. Pure.

### `fn write_byte(value: u8) -> [u8; 2]` [@ANCHOR: radioctl:radioctl_ar7030_write_byte]

Builds the two command bytes that write one byte of data at the current address: `0x30 | (value >> 4)` loads the high nibble into the H register, and `0x60 | (value & 0xF)` writes the byte H:d and steps the address up by one. For 0x4D the result is `[0x34, 0x6D]` (high nibble 4, low nibble D). `value` is passed by value; the page and address must already have been selected with `set_address`. Pure, cannot fail.

### `pub fn engine(desc: RigDescription) -> Result<Ar7030Engine, RigError>` [@ANCHOR: radioctl:radioctl_ar7030_engine_new]

Builds the engine for an AR7030 or AR7030 Plus. `desc` is taken by value and must be of the receiver family, have the dialect `ar7030` or `ar7030p`, and use the AOR protocol; otherwise `ErrorKind::Unsupported` ("not an AR7030 or AR7030 Plus description"). The new engine expects nothing, has an empty buffer and no outstanding byte count. It implements `ProtocolEngine` itself, not through the line engine, because the protocol is binary and one byte per command. The radio cannot transmit through this engine. No I/O.

### `fn description(&self) -> &RigDescription` [@ANCHOR: radioctl:radioctl_ar7030_engine_description]

The `ProtocolEngine` accessor that returns the description the AR7030 engine was built from. Borrowed read-only, never changes after construction, no side effects, cannot fail. Callers (the rig actor and the capability checks) use it for capability and mode-table lookups; because the AR7030 engine is not a line engine, it holds its own copy of the description, which is the one passed to `engine` and the only one it ever reports.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_ar7030_engine_encode]

Turns an operation into the AR7030's byte commands and records what the reply will be. A capability absent from the description gives `ErrorKind::Unsupported` ("capability not in the rig description"). Reads: `GetFrequency(A)` selects page 0 address 0x1A and sends three read-and-advance bytes `71 71 71` (three reply bytes expected); `GetMode` selects address 0x1D and sends `71` (one byte); `GetSignal` sends routine 14 (`2E`, one byte). Settings follow Hamlib's order: lock level 1 (`81`), set the address, write the bytes, execute routine 4 (`24`, apply everything), unlock (`80`); no reply is expected. Frequency above 32 MHz is `Unsupported` ("frequency out of range"); a mode not in the description is `Unsupported` too, as is every other operation. It also stores the expected reply and byte count and clears the buffer. No I/O.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_ar7030_engine_feed]

Collects reply bytes from the radio and decodes them once the expected count has arrived. `now` is the caller's monotonic time: a partial buffer older than `PARTIAL_FRAME_TIMEOUT` (500 ms) is first emitted as `Inbound::Unrecognised` and dropped. When no observation is expected, any bytes received are reported as `Unrecognised` (the radio never sends unasked). Otherwise bytes are buffered until `want` (3 for frequency, 1 for mode or signal) are present; extra bytes are discarded. Three bytes are a big-endian count converted by `hz_of`; one byte is a mode code mapped through the description (an unknown code gives `Unrecognised`) or a raw signal level 0 to 255. Then `expecting` returns to none. No errors, no I/O.

### `pub fn new(desc: &RigDescription) -> Result<Ar7030Twin, RigError>` [@ANCHOR: radioctl:radioctl_ar7030_twin_new]

Builds the virtual AR7030. `desc` is borrowed and must satisfy `handles` (dialect `ar7030` or `ar7030p`), otherwise `ErrorKind::Unsupported` ("not an AR7030 or AR7030 Plus dialect"). The twin starts with page 0, address 0, H and lock registers zero and a 256-byte working memory of zeros; it is filled from the shared state each time a read happens. A clone of the description is kept for mode codes. No I/O.

### `fn refresh(&mut self, state: &RigState)` [@ANCHOR: radioctl:radioctl_ar7030_twin_refresh]

Copies the front-panel state (the shared `RigState`) into the twin's working memory so a read sees the current values: the three bytes of the tuning count of `freq_a` at address 0x1A and the mode code at 0x1D (code 1, AM, if the description has none for the current mode). `state` is borrowed read-only; the twin's memory is overwritten at those four bytes. Called before each read byte. Pure apart from that.

### `fn read(&self) -> u8` [@ANCHOR: radioctl:radioctl_ar7030_twin_read]

Returns the byte at the twin's current page and address. Page 0 returns the working memory byte (0 beyond 255), page 15 returns the ASCII of the firmware identity `7030_14A` at the address (0 beyond it), and every other page returns 0 (not modelled). Takes no arguments and changes nothing; does not advance the address (the caller does).

### `fn execute(&mut self, state: &mut RigState, routine: u8) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_ar7030_twin_execute]

Runs one AR7030 routine on the virtual radio. Routines 1, 2 and 4 (set frequency, set mode, set everything) apply the working memory to the shared state: the three bytes at 0x1A are converted by `hz_of` and stored in `state.freq_a` only if at most 32 MHz, and the byte at 0x1D is mapped through the description to `state.mode` if known. Routine 14 returns one byte, the signal strength capped at 255. Any other routine does nothing. Returns the bytes the radio would send (empty except for routine 14). Mutates `state`; no I/O.

### `fn line(&mut self, _state: &mut RigState, _line: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_ar7030_twin_line]

The line hook of the twin trait, which the AR7030 never uses: the protocol has no text lines and all bytes arrive through `raw`. It ignores both arguments and always returns an empty vector. It exists only because the trait requires it, and `VirtualReceiver` never calls it for this twin, since `raw` always answers. No side effects.

### `fn raw(&mut self, state: &mut RigState, bytes: &[u8]) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_ar7030_twin_raw]

Executes the controller's bytes as AR7030 commands, one byte each: the high nibble is the operation and the low nibble its data. `1x` sets the high address nibble; `2x` runs routine x (via `execute`); `3x` loads H; `4x` sets the low address byte from H and x and clears H; `5x` selects page x; `6x` writes H:x at the address on page 0 (ignored on other pages) and advances the address; `7x` refreshes working memory from `state`, appends the byte at the address to the output and advances by x; `8x` sets the lock level (stored, not enforced); other bytes do nothing. Always returns `Some` with the concatenated output (possibly empty), so line framing is never used. Mutates the twin and `state`; no I/O.

## `crates/radioctl-protocols/src/rs_hf.rs`

### `pub fn engine(desc: RigDescription) -> Result<RsHfEngine, RigError>` [@ANCHOR: radioctl:radioctl_rshf_engine]

Builds the protocol engine for a Rohde and Schwarz HF receiver with the line-feed framed `F` / `I` commands (EK 895, EK 896, XK 2100) or the `*F` / `*I` / `*O1` set (XK 852). `desc` is taken by value. A thin wrapper over `LineEngine::new` with `ReceiverProtocol::RohdeSchwarz`; `ErrorKind::Unsupported` comes back for the wrong family or protocol or a dialect other than `ek895`, `ek896`, `xk2100` and `xk852`. PTT is not implemented, so nothing here keys a transmitter. No I/O.

### `pub fn handles(desc: &RigDescription) -> bool` [@ANCHOR: radioctl:radioctl_rshf_handles]

True when the description's dialect names one of the HF command sets (`ek895`, `ek896`, `xk2100` or `xk852`), that is, when `style_of` finds a style. The factories test it before the basic Rohde and Schwarz case (`rs.rs`, dialect `rs`) so the right engine serves the model. `desc` is borrowed; a description without `[receiver]` gives false. Pure.

### `fn style_of(desc: &RigDescription) -> Option<Style>` [@ANCHOR: radioctl:radioctl_rshf_style_of]

Maps the description's `receiver.dialect` string to the command-set style: `ek895` and `ek896` to the EK style (`F<hz>`, `I<n>`, `L?`), `xk2100` to the XK 2100 style (`F<hz>,<hz>`), `xk852` to the XK 852 style (`*F`, `*I`, `*O1`); any other name or a missing `[receiver]` section gives `None`. `desc` is borrowed. Pure.

### `fn from_description(desc: &RigDescription) -> Result<RsHfDialect, RigError>` [@ANCHOR: radioctl:radioctl_rshf_dialect_from_description]

Builds the dialect from the description with `style_of`. An unsupported or missing dialect name returns `ErrorKind::Unsupported` with the message listing the valid names ("not a Rohde and Schwarz HF dialect (ek895, ek896, xk2100, xk852)"). `desc` is borrowed; the dialect holds only the style. Pure, no I/O.

### `fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError>` [@ANCHOR: radioctl:radioctl_rshf_dialect_encode]

Spells an operation, each command framed with a leading line feed and a trailing CR. Frequency set: EK sends `F<hertz>` and the XK 2100 `F<hertz>,<hertz>`, both only up to 30 MHz and expecting no reply; the XK 852 sends `*F` and seven digits in units of 10 Hz (the hertz value rounded to nearest), expecting an `Ack`, with a maximum of 9,999,999 units. A frequency over its model's limit gives `ErrorKind::Unsupported` ("frequency out of range"). Reads: `F?` (EK, XK 2100) or `*O1` (XK 852) for frequency; `I?` or `*O1` for mode; `L?` for the signal on the EK only. `SetMode` sends `I<code>` or `*I<code>` using the description's code, `Unsupported` if the mode is missing. Everything else (XK signal, VFO B, PTT) is `Unsupported`. No I/O.

### `fn status(&mut self, expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>>` [@ANCHOR: radioctl:radioctl_rshf_dialect_status]

Handles lines that are not data. The XK 852 answers every setting with some line whose content Hamlib discards, so when an `Ack` is expected any line at all (it never reads `line`) gives `Inbound::Ack`; for other expectations the line goes on to `decode`. The EK and XK 2100 do not answer settings, so for them only an empty line is dropped (empty result), and everything else goes to `decode`. These dialects have no refusal form, so `Nak` is never produced. Pure.

### `fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_rshf_dialect_decode]

Interprets a data line when an observation is expected (otherwise `Inbound::Unrecognised`). On the XK 852 the `*O1` status line is searched with `tagged_digits`: frequency is the seven digits after the first `*F` times 10 Hz, and the mode is the single digit after `*I` mapped through the description's table. On the other models the line must begin with the tag `F`, `I` or `L` followed by the number (`number_after`): hertz for frequency, a mode code for mode (unknown codes are unrecognised) and the level in dBuV for the signal (clamped to 65535). Anything else gives `Unrecognised` with the line's bytes; no error is returned. Pure.

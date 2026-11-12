# radioctl function anchors, batch 43

Function anchors for the `radioctl-protocols` crate: the end of the Rohde and Schwarz HF module (`rs_hf.rs`: two parsing helpers and the virtual twin), the Uniden BCD396T/BCD996T digital scanner engine and twin (`uniden_digital.rs`), and three small leftovers (`mode_name` of the Icom marine dialect, the PCR `terminator`, the receiver `status` default). Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Nothing in this batch keys a transmitter: the functions are receiver-only dialect code, twins or pure helpers (the Icom marine `mode_name` only names a mode), so the transmit gate, lock and band check are not applied here.

## `crates/radioctl-protocols/src/rs_hf.rs`

### `fn number_after(line: &str, tag: &str) -> Option<u64>` [@ANCHOR: radioctl:radioctl_rshf_number_after]

Reads the unsigned number that follows a one-letter tag at the very start of a reply line, for the EK 895/896 and XK 2100 answers such as `F7074000`, `I16` or `L34`. `line` is the received line without its LF and CR; `tag` is the expected prefix (`F`, `I` or `L`). If the line does not start with `tag` the result is `None`. Otherwise the run of ASCII digits straight after the tag is parsed as `u64`; no digits, or a value that overflows, also gives `None`. Text after the digits is ignored. Pure, no panics.

### `fn tagged_digits(line: &str, tag: &str, count: usize) -> Option<u64>` [@ANCHOR: radioctl:radioctl_rshf_tagged_digits]

Reads a fixed-width number from the XK 852's fixed-column `*O1` status line. It looks for the first occurrence of `tag` (such as `*F` or `*I`) anywhere in `line`, then takes exactly `count` characters after it; all of them must be ASCII digits, else `None` (so a short field, a blank or a letter fails). The digits are parsed as `u64` (7 for the frequency in 10-Hz units, 1 for the mode code). A missing tag gives `None`. `line` and `tag` are borrowed; pure, no panics (slicing is checked).

### `pub fn new(desc: &RigDescription) -> Result<RsHfTwin, RigError>` [@ANCHOR: radioctl:radioctl_rshf_twin_new]

Builds the virtual Rohde and Schwarz HF receiver. The dialect style (EK, XK 2100 or XK 852) is derived with `RsHfDialect::from_description`, so a dialect that is not `ek895`, `ek896`, `xk2100` or `xk852` returns `ErrorKind::Unsupported`; the style and a clone of the description (for mode-code lookups) are kept. `desc` is borrowed. The twin has no frequency or mode of its own: they live in the shared `RigState` passed to each `line` call. No I/O.

### `fn reply(text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_rshf_twin_reply]

Formats one answer the way these radios send it: a line feed, the text and a closing carriage return, as bytes (the engine drops the leading LF). `text` is borrowed and may be empty, giving `LF CR`. An associated function without `self`; pure, infallible, allocates one `Vec`.

### `fn status_line(&self, state: &RigState) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_rshf_twin_status_line]

Builds the XK 852's status answer from the shared state, with each field at the column Hamlib reads it from (column 0 is the LF): `*F` and seven digits of the frequency in units of 10 Hz at column 1, `*I` and the mode code (code 1 if the description has none for the current mode) at column 12, `*N0` at column 22 and `*S1` at column 30, with spaces between. The line is built by padding to those lengths and then framed by `reply`. `state` is read only. The noise-blanker and squelch fields are constants. Pure.

### `fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_rshf_twin_line]

The twin's reaction to one command line (the framing LF and CR removed). XK 852 lines are passed to `xk852_line`. Otherwise `F?` answers `F` and the frequency in hertz, `I?` answers `I` and the mode code (0 if unknown), and `L?` answers `L` and the raw signal on the EK only. `F<hz>` (for the XK 2100 `F<hz>,<hz>`, only the first number is used) sets `state.freq_a` if the number parses and is not above 30 MHz; `I<n>` sets `state.mode` if the description knows the code. Settings are not answered, and bad values are ignored with no refusal. Any other line gets no answer. Mutates `state`; no I/O.

### `fn xk852_line(&mut self, state: &mut RigState, line: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_rshf_twin_xk852_line]

Handles one command line of the virtual XK 852. `*O1` answers with the status line. `*F<digits>` sets `state.freq_a` to the number times 10 Hz if it parses (no range check) and answers with the status line; `*I<n>` sets `state.mode` if the code is known (an unknown or bad code is ignored) and also answers with the status line. Every setting is thus answered with a full status line, which is what the real radio does and what the engine's `status` hook counts as the acknowledgement. Other lines get no answer. Mutates `state`; no I/O.

## `crates/radioctl-protocols/src/uniden_digital.rs`

### `pub fn engine(desc: RigDescription) -> Result<UnidenDigitalEngine, RigError>` [@ANCHOR: radioctl:radioctl_unidig_engine]

Builds the protocol engine for a Uniden BCD396T or BCD996T digital trunking scanner, which speaks the `GLG` / `QSH` command set. `desc` is taken by value. It wraps `LineEngine::new` with `ReceiverProtocol::Uniden`, so `ErrorKind::Unsupported` is returned for the wrong family or protocol, or from the dialect when the dialect name is neither `bcd396t` nor `bcd996t`. Scanners cannot transmit. The module comment notes a licence question about these command facts that is for the project owner to decide. No I/O.

### `pub fn handles(desc: &RigDescription) -> bool` [@ANCHOR: radioctl:radioctl_unidig_handles]

True when the description's `receiver.dialect` is `bcd396t` or `bcd996t`. The engine and twin factories test it before the older Uniden case (`uniden.rs`), so these two models get the digital backend. `desc` is borrowed; a description without a `[receiver]` section gives false. Pure, no errors.

### `fn from_description(desc: &RigDescription) -> Result<UnidenDigitalDialect, RigError>` [@ANCHOR: radioctl:radioctl_unidig_dialect_from_description]

Builds the dialect, which has no variants or state. If `handles(desc)` is true it returns the unit value `UnidenDigitalDialect`; otherwise `ErrorKind::Unsupported` ("not a BCD396T or BCD996T dialect"). `desc` is borrowed. Pure, no I/O.

### `fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError>` [@ANCHOR: radioctl:radioctl_unidig_dialect_encode]

Spells an operation as a CR-ended command. Reads of frequency and mode both send `GLG` and expect an observation. `SetFrequency(A, hz)` sends a quick-search-hold command `QSH` with the frequency as eight digits in units of 100 Hz; `hz` must be a multiple of 100 Hz and within 25 MHz to 1.3 GHz, otherwise `ErrorKind::Unsupported` ("frequency out of range or not a multiple of 100 Hz"). `SetMode` sends `QSH` with the modulation name `AM`, `FM` or `WFM` from the description's code (`Unsupported` if the mode is missing or the code is not 1 to 3). Settings expect an `Ack`. Signal strength is not offered; everything else (including transmit) is `Unsupported`. No I/O.

### `fn status(&mut self, expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>>` [@ANCHOR: radioctl:radioctl_unidig_dialect_status]

Classifies bare status lines. An empty line is ignored (empty result). `ERR`, `NG`, `FER` and `ORER`, and any line ending in `,NG` (for example `QSH,NG` when the scanner is in a menu), give `Inbound::Nak`. A line ending in `,OK` gives `Inbound::Ack` only when an `Ack` was expected. Everything else returns `None` and goes to `decode`. `expecting` is the reply kind in flight; `line` has no terminator. Pure.

### `fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_unidig_dialect_decode]

Reads a `GLG` answer when an observation is expected (otherwise `Inbound::Unrecognised`). The line is split on commas; it must start with `GLG` and have at least three fields, else `Unrecognised`. If the frequency field is empty (nothing is being received) the result is `Inbound::Nak`: there is no value to report. Frequency comes from `frequency`; mode from `modulation`, kept only if the description has that mode. Other fields (such as signal) are not offered and give `Unrecognised`, as does any parse failure. No error is returned. Pure.

### `fn qsh(frequency: &str, modulation: &str) -> String` [@ANCHOR: radioctl:radioctl_unidig_qsh]

Builds a `QSH` (quick search hold) command line with only the frequency and/or modulation filled in: `QSH,<frequency>,,<modulation>,,,,,,` and CR, which makes nine fields where empty ones tell the scanner to leave that setting unchanged. `frequency` is the eight-digit text in units of 100 Hz or empty; `modulation` is the name (`AM`, `FM`, `WFM`) or empty. Neither is validated here; the caller checks range first. Pure.

### `fn frequency(field: &str) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_unidig_frequency]

Parses the frequency field of a `GLG` answer. If it contains a point it is decimal megahertz: the whole part is parsed, the fraction padded or cut to six digits and the result is converted to hertz. Otherwise it is an integer in units of 100 Hz, multiplied by 100. Surrounding blanks on the whole part or the integer are trimmed. Returns `None` for text that is not a number or a fraction with a non-digit. Used by the dialect decoder; the twin checks its commands separately. Pure. The multiplications and the addition are unchecked, so an absurdly large number from a faulty scanner would panic in a debug build and wrap in a release build; the scanner's real answers are far below that.

### `fn modulation(name: &str) -> Option<Mode>` [@ANCHOR: radioctl:radioctl_unidig_modulation]

Maps a modulation name to a radioctl mode: `AM` gives AM; `FM` and `NFM` both give FM (radioctl has no narrow FM); `WFM` and `FMB` give WFM. Any other name, including `AUTO` and an empty string, gives `None`; the case matters. The caller decides whether the description has a code for the mode. Used by the decoder and by the twin for `QSH`. Pure.

### `pub fn new(desc: &RigDescription) -> Result<UnidenDigitalTwin, RigError>` [@ANCHOR: radioctl:radioctl_unidig_twin_new]

Builds the virtual BCD396T/BCD996T. It validates the description with `UnidenDigitalDialect::from_description`, returning `ErrorKind::Unsupported` for another dialect, and keeps a clone of the description for mode lookups and the model name. `desc` is borrowed. Frequency, mode and signal are in the shared `RigState`. No I/O.

### `fn reply(text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_unidig_twin_reply]

Formats one answer line as the scanner sends it: the text followed by a single CR (no line feed), as bytes. `text` is borrowed and may be empty. An associated function without `self`; pure, infallible, allocates one `Vec`.

### `fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_unidig_twin_line]

The virtual scanner's reaction to one command line (CR removed), split on commas. `GLG` alone answers `GLG,<frequency as eight digits of 100 Hz>,<mode name>,0,0,,,,<squelch 1 if the signal is above zero, else 0>,0`. `QSH` with exactly ten fields applies a frequency (eight digits in 25 MHz to 1.3 GHz) and/or a modulation (`AUTO` and empty leave it unchanged) to `state` and answers `QSH,OK`; a bad frequency or modulation answers `ERR` and changes nothing. `MDL` answers the model name taken from the description's last word without hyphens. Every other line answers `ERR`. Mutates `state`; no I/O.

## `crates/radioctl-protocols/src/icom_marine.rs`

### `fn mode_name(&self, desc: &RigDescription, mode: Mode) -> Option<&'static str>` [@ANCHOR: radioctl:radioctl_marine_mode_name]

Gives the name the Icom marine radio uses for a mode. `mode` is looked up in the description's code table; no code for it gives `None`. Code 5 (RTTY) returns the dialect's own RTTY name (`J2B`, or `AFS` on the IC-M710); codes 1 to 4 return `LSB`, `USB`, `CW` and `AM`; any other code gives `None`. The result is a static string. Used when encoding `SetMode` and by the twin to name modes. Pure, read-only.

## `crates/radioctl-protocols/src/icom_pcr.rs`

### `fn terminator(&self) -> u8` [@ANCHOR: radioctl:radioctl_pcr_dialect_terminator]

Tells the shared line engine which byte ends a received line for this PCR model. The PCR1000 frames each answer as a line feed, four characters and a CR, so for it the terminator is CR (`\r`) and the leading LF is dropped by the engine; the other PCR models end the four-character answer with CR LF, so the terminator is the line feed (`\n`) and the CR left before it is trimmed. Takes only `&self`, reads the `lead` flag set from the dialect name, cannot fail and changes nothing.

## `crates/radioctl-protocols/src/receiver.rs`

### `fn status(&mut self, expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>>` [@ANCHOR: radioctl:radioctl_receiver_dialect_status_default]

The default body of the `Dialect::status` hook, which decides whether a line is a bare acknowledgement or refusal before the dialect's `decode` sees it. An empty line is a setting's acknowledgement: it gives `Inbound::Ack` if an `Ack` was expected, and an empty result (swallowed) otherwise. A line starting with `?` is a refusal and gives `Inbound::Nak`. Every other line returns `None` and is handed to `decode`. `expecting` is the reply kind of the request in flight; `line` has no terminator or line feed. A dialect overrides this when its radio says OK or NG in words. Pure, no errors.

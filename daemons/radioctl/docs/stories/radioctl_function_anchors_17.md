# radioctl function anchors, batch 17

Function anchors for `radioctl-protocols`: the Icom CI-V engine and frame parser, the Kenwood ASCII tokenizer and engine, the Elecraft ASCII engine and its parsers, and the FlexRadio SmartSDR text helpers. Tests cite each anchor with `Tests [@ANCHOR: ...]`. The engines only build and decode bytes; the rig actor applies the transmit gate, the band check and the transmit lock before it asks an engine to encode `SetPtt(true)` or `SendCw`, and the engine functions here do not repeat that check. `line_may_transmit` is the one helper that classifies a line for the gate.

## `crates/radioctl-protocols/src/civ.rs`

### `pub fn decode_bcd_meter(bytes: &[u8]) -> Option<u16>` [@ANCHOR: radioctl:radioctl_civ_decode_bcd_meter]

Decodes the two-byte BCD value CI-V uses for levels and meters, most significant pair first: `01 20` is 120 and `02 55` is 255. `bytes` is borrowed and must be exactly two bytes; an empty slice, one byte, or three or more bytes gives `None`, as does any nibble above 9. The result is 0 to 9999 and is returned raw: callers apply their own limit afterwards (the engine's `decode` drops levels above its maximum and meters above its own). Pure, no panics. The inverse is `encode_bcd_meter`.

### `pub fn encode_bcd_meter(value: u16) -> [u8; 2]` [@ANCHOR: radioctl:radioctl_civ_encode_bcd_meter]

Encodes a level or meter value as two BCD bytes, most significant pair first (120 is `01 20`). `value` is clamped to 9999 rather than rejected, so an out-of-range number is sent as 9999 and callers that care must range-check first (the level encoder checks the model's range before it gets here). Returns a new fixed array. Pure, never fails, no side effects. The inverse is `decode_bcd_meter`, whose input is 0 to 9999 as well.

### `pub fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Parsed>` [@ANCHOR: radioctl:radioctl_civ_parser_feed]

`FrameParser::feed`: appends newly received `bytes` (borrowed, any length including empty) stamped `now` (a monotonic duration from any fixed origin) and returns, in order, every complete frame as `Parsed::Frame` and every run of unusable bytes as `Parsed::Junk`. If a partial frame has been waiting longer than the 500 ms partial-frame timeout when new bytes arrive, it is emitted as junk first. A frame runs from `FE FE` to `FD`; bytes before a preamble are junk, a second preamble inside a frame restarts it (a bus collision), a frame with fewer than three body bytes is junk, and an unterminated frame over 256 bytes is dropped as junk. A trailing lone `FE` is kept for the next call. Mutates the parser's buffer; no I/O, no panics, not thread safe (one parser per engine).

### `pub fn new(desc: RigDescription) -> Result<CivEngine, RigError>` [@ANCHOR: radioctl:radioctl_civ_engine_new]

Builds the Icom CI-V client engine. `desc` is taken by value and kept; it must be an `IcomCiv` family description that carries CI-V parameters (rig address, controller address, command set, frequency field length). Otherwise it returns `ErrorKind::Unsupported` with "not a CI-V description". If the description has a scope section the engine also creates a scope assembler for command 27 waveform frames. The new engine has an empty parser, no known mode, filter number 1 and no PTT acknowledgement pending. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_civ_engine_encode]

`ProtocolEngine::encode` for CI-V: builds the frame bytes and the reply the actor must wait for. Reads wait for the matching observation; writes wait for the rig's OK (`FB`) or refusal (`FA`). Frequency reads use `03` and sets use `05` (VFO B uses `25 01`), mode uses `04`/`06`, or `26 00` on models with data modes (and then the filter number last reported is resent); PTT is `1C 00` plus `00` or `01`; CW text is `17` plus the text; `StopCw` is `17 FF`; meters, levels, RIT/XIT, split, filter width, the model id and the scope ops have their own commands. Errors, all `Unsupported`: the capability for the op is not in the description, a frequency with more digits than the model's field, a mode with no code, CW text that is empty, over 30 characters or not printable ASCII, a filter width the model has no exact setting for, `SetTransceive` (a menu setting on these rigs), and any other op. Every call also records whether a PTT acknowledgement is awaited (some Xiegu rigs answer `1C 00 <state>` instead of OK), and `SetMode` remembers the new mode for the filter tables. `SetPtt(true)` and `SendCw` can transmit; the gate, band check and lock are applied by the actor before this is called.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_civ_engine_feed]

`ProtocolEngine::feed`: passes received bytes through the frame parser and decodes each frame into an `Inbound`. Junk becomes `Unrecognised`. A frame is silently dropped (no event) when it is our own echo, is addressed to some other station, did not come from the configured rig address, or is a scope waveform frame that does not complete a sweep. OK and NG frames become `Ack` and `Nak`; a recognised read becomes an `Observation`; a frame the engine does not understand becomes `Unrecognised` with the frame's bytes. A mode observation also updates the engine's stored mode, which the filter-width tables depend on. Returns an empty vector when no complete frame has arrived. No I/O, no blocking; mutates engine state only.

## `crates/radioctl-protocols/src/kenwood.rs`

### `pub fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Token>` [@ANCHOR: radioctl:radioctl_kenwood_tokenizer_feed]

`Tokenizer::feed`: splits received bytes on the `;` terminator. `bytes` is borrowed; `now` is a monotonic duration. A partial command older than the 500 ms partial-frame timeout is emitted first as `Token::Junk`. Each complete command becomes `Token::Command` with the terminator removed and surrounding blanks trimmed (a `KY` message only has its start trimmed, because its trailing spaces are part of the field). A command that contains a non-printable byte, or is empty after trimming, becomes `Token::Junk` with the raw bytes. If the unterminated remainder exceeds the buffer limit (256 bytes, or the size given to `with_max_pending`) it is dropped as junk. Mutates the buffer only; no I/O or panics.

### `pub fn with_max_pending(max_pending: usize) -> Tokenizer` [@ANCHOR: radioctl:radioctl_kenwood_tokenizer_with_max_pending]

Creates a tokenizer whose unterminated-bytes limit is `max_pending` bytes instead of the default 256. It exists for the TS-890S bandscope frame, 1286 bytes long, where every other command is shorter than the default. `max_pending` is a byte count; 0 means the default (256). Once the buffer holds more than the limit without a `;`, `feed` discards it as junk, so too small a value loses long frames and too large a value lets a stream with no terminator grow to that size. Pure constructor, no errors.

### `pub fn parse_digits(text: &str) -> Option<u64>` [@ANCHOR: radioctl:radioctl_kenwood_parse_digits]

Parses a decimal number out of a string made only of ASCII digits. `text` is borrowed. Returns `None` for an empty string, a string longer than 18 characters (which keeps the result inside `u64`), or any character that is not `0` to `9`, including signs, blanks and decimal points. Leading zeros are fine (`00014074000` is 14,074,000). Used by the Kenwood and Elecraft decoders for frequency, level and meter fields. Pure, no panics.

### `pub fn new(desc: RigDescription) -> Result<KenwoodEngine, RigError>` [@ANCHOR: radioctl:radioctl_kenwood_engine_new]

Builds the Kenwood-style ASCII engine. `desc` is taken by value and kept. Errors, both `Unsupported`: the description is not a `KenwoodAscii` family description or has no Kenwood section, or its dialect is `Ts890`, which has its own engine (the generic one would send it `IF` and `MD`). The dialect selects behaviour flags: the older TS-440/TS-450/R-5000 forms (split through `FN`/`SP` for TS-440 and R-5000), the TS-990 mode through `OM0`, the R-5000 shorter `IF` answer and the TRC-80 frequency read through `IF`. Auto-information starts off. No I/O.

### `fn ky(&self) -> Result<radioctl_core::description::KyParams, RigError>` [@ANCHOR: radioctl:radioctl_kenwood_engine_ky]

Returns the description's `KY` (send Morse text) record for this model, or `Unsupported` with "send-CW-text (KY) is not documented for this model" when the description has no `[kenwood.ky]` block, its form is undocumented, or it has no way to stop the keying. The record is `Copy` and is returned by value. The check is deliberately strict because `KY` can key the transmitter; it means only models whose manual documents the form, width, character table and stop can send CW. Used by `SendCw` and `StopCw` encoding. Pure.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_kenwood_engine_encode]

`ProtocolEngine::encode` for Kenwood ASCII: commands end in `;`. Reads wait for an observation (`FA;`/`FB;`, `MD;`, `SM0;`, `ID;`, and `IF;` for PTT, split, RIT, XIT and offset); writes wait for nothing (`FA` plus 11 digits, `MD<c>`, `RT`, `XT`, `FR0;FT<n>;` for split). `SetPtt(true)` sends `TX;` and `SetPtt(false)` sends `RX;`. Older and TS-990 dialects try their own encoder first. `SetRitOffset` sends `RC;` then `RU` or `RD` with five digits. `SendCw` and `StopCw` use `KY` only when `ky()` allows it. `SetTransceive` sends `AI2;` or `AI0;` and remembers it for reconnection. Errors, all `Unsupported`: capability not in the description, frequency above 11 digits, a mode with no code, RIT offset beyond 9999 Hz, a level the rig does not have or outside its range, `KY` text or stop the model cannot express, and any other op. `TX;` and `KY` text can transmit; the actor applies the gate, band check and lock before calling.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_kenwood_engine_feed]

`ProtocolEngine::feed`: runs the received bytes through the tokenizer and decodes every command token into inbound events with the engine's `decode`. Junk tokens become `Inbound::Unrecognised`. A command token can produce zero, one or several events (an `IF` answer yields several observations; `?` yields `Nak`). The result is empty while a command is incomplete. Mutates only the tokenizer buffer; no I/O, blocking or panics.

## `crates/radioctl-protocols/src/elecraft.rs`

### `pub fn level_wire(level: Level) -> LevelWire` [@ANCHOR: radioctl:radioctl_elecraft_level_wire]

Gives the two-letter command prefix and allowed numeric range of a level on the generic Elecraft K3 scale: `AG` 0 to 255 (AF gain), `RG` 0 to 250, `MG` 0 to 60, `ML` 0 to 60 (monitor), `KS` 8 to 50 words per minute, `PC` 0 to 110 (watts, largest the manual lists with a KXPA100). Attenuator is `RA` and preamp `PA` with range 0 to 0 because they carry steps, not numbers. `Dither`, `Preselector` and `Antenna` have an empty prefix and no command; the encoder refuses them. Pure, total.

### `pub fn level_wire_for(desc: &RigDescription, level: Level) -> LevelWire` [@ANCHOR: radioctl:radioctl_elecraft_level_wire_for]

Gives the wire command and range of a level on the described model: the `level_wire` result with its minimum and maximum replaced by the description's per-model range (`[elecraft] levels`) when that gives one, and the prefix unchanged. This matters because a real KX3 clamps `AG` to 0 to 60, `RG` to 190 to 250, `MG` to 0 to 80 and `PC` to its own maximum power, so the K3 scale made half scale mean a different value. `desc` is borrowed. Pure; used by the encoder for range checks and by the decoder to accept a reply.

### `pub fn strength_db(sm: u16) -> Option<i32>` [@ANCHOR: radioctl:radioctl_elecraft_strength_db]

Converts an `SM` S-meter reading (0 to 15, basic format) to decibels relative to S9 from a fixed 16-entry table taken from Hamlib 4.6.2: -54 at 0, 0 at 6 (S9), +20 at 9, +40 at 12, +60 at 15. `sm` is the raw reading. Returns `None` for a value above 15. Pure, no panics, no calibration of the individual radio; unknown whether a given rig matches the table beyond the manual's anchor points.

### `pub fn parse_power_amplifier(data: &str) -> Option<bool>` [@ANCHOR: radioctl:radioctl_elecraft_parse_power_amplifier]

Reads the data of an `OM` answer (option modules) for whether the power amplifier module is installed. The data is an optional leading blank then exactly 12 ASCII option positions; the second position is `P` for installed (the KPA3 on a K3, K3S; the external KXPA100 on a KX3 or KX2) and `-` for absent. Returns `Some(true)` or `Some(false)` accordingly, and `None` when the length is not 12, the text is not ASCII, or the position holds anything else. `data` is borrowed. Pure.

### `pub fn signed_offset(hz: i16) -> String` [@ANCHOR: radioctl:radioctl_elecraft_signed_offset]

Formats a RIT/XIT offset as the sign and four digits of an `RO` command: 50 is `+0050`, -50 is `-0050`, 0 is `+0000`. `hz` is in hertz; the magnitude is padded to four digits, so a magnitude above 9999 would print five digits (the caller, `encode_set`, rejects those before calling). Pure, total over `i16`, no errors.

### `pub fn parse_offset(data: &str) -> Option<i16>` [@ANCHOR: radioctl:radioctl_elecraft_parse_offset]

Parses the data of an `RO` answer or the offset field of `IF`: one sign character (`+` or a blank for plus, `-` for minus) followed by exactly four digits, giving hertz. `data` is borrowed. `None` when the text is not five characters, has a bad sign, non-digit characters, or the magnitude does not fit `i16`. Examples: `+0050` is 50, `-0050` is -50, ` 0050` is 50. Pure.

### `pub fn parse_firmware(text: &str) -> Option<(u8, u8)>` [@ANCHOR: radioctl:radioctl_elecraft_parse_firmware]

Parses the `NN.NN` revision of an `RVM` answer (after the engine has removed the `M`): two digits, a point, two digits; `02.37` is `(2, 37)`. `text` is borrowed. Returns `None` when there is no point, either part is not exactly two characters, or a part is not all digits. Result is (major, minor), each 0 to 99. Pure.

### `pub fn decode_if(payload: &str, desc: &RigDescription) -> Option<Vec<Observation>>` [@ANCHOR: radioctl:radioctl_elecraft_decode_if]

Decodes the answer to `IF`, which Elecraft sends as one long fixed-column line. `payload` is the text after `IF` with the terminator removed, 34 or 35 ASCII characters; `desc` supplies the mode-code table. It returns eight observations in order: VFO A frequency (11 digits, hertz), RIT/XIT offset, RIT on/off, XIT on/off, transmit state, mode, split, and the transmit VFO (B when split is on, A otherwise, as a KX3 does). `None` when the length is wrong, a fixed blank or `" 00"` field is wrong, a number or flag is malformed, or the mode digit has no entry in the description. Pure; no side effects.

### `pub fn new(desc: RigDescription) -> Result<ElecraftEngine, RigError>` [@ANCHOR: radioctl:radioctl_elecraft_engine_new]

Builds the Elecraft ASCII engine. `desc` is taken by value and kept. It must be an `ElecraftAscii` description with an Elecraft section; otherwise `Unsupported` with "not an Elecraft description". The engine starts with an empty tokenizer and auto-information not requested. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_elecraft_engine_encode]

`ProtocolEngine::encode` for Elecraft: builds the command text and reply to wait for. First checks the description has the op's capability (`Unsupported` "capability not in the rig description" otherwise). Read ops (`FA;`, `FB;`, `MD;`, `TQ;`, `SM;`, `PO;`, `FT;`, `RT;`, `XT;`, `RO;`, `BW;`, level reads, `SW;`, `ID;`, `OM;`, `RVM;`) wait for the matching observation; everything else goes to `encode_set`. A read of `Dither` or `Preselector` is refused, and attenuator or preamp reads need the description to define them. No I/O. A transmit-capable command comes out of `encode_set`; the gate is applied by the actor.

### `fn encode_set(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_elecraft_encode_set]

Builds the Elecraft commands that change something; none waits for a reply. Frequency `FA`/`FB` and 11 digits (above 11 digits is refused); mode `MD<c>`; `SetPtt(true)` is `TX;` and false `RX;`; transceive `AI2;`/`AI0;` (remembered so it can be resent after a reconnect); split `FT1;`/`FR0;`; RIT and XIT `RT`/`XT`; offset `RO` with sign and four digits (beyond 9999 Hz refused); filter width `BW` in 10 Hz units (must be a multiple of 10 and at most 99990 Hz); levels as numbers within the model's range or as attenuator/preamp steps; `SendCw` as `KY ` plus the text after `check_ky_text`. `StopCw` is `KY <EOT>;RX;`: the EOT empties the buffer on radios that honour it and `RX;` ends the transmission on all, because a real KX3 kept keying after the EOT alone. Errors, all `Unsupported`. `TX;`, `KY` and the stop's `RX;` touch the transmitter; the gate is applied by the actor, and an unkey or stop bypasses it as an emergency command.

### `fn check_ky_text(text: &str) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_elecraft_check_ky_text]

Validates CW text before it is put into a `KY` command: 1 to 24 characters, each printable ASCII (0x20 to 0x7E), and none of `;`, `<` or `>`. A `;` would end the command and let the text smuggle a second one; `<` and `>` switch the radio between TX TEST and TX NORM. Anything else returns `Unsupported` ("CW text must be 1 to 24 printable ASCII characters without ; < >"). The length is in bytes. `text` is borrowed. Pure, no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_elecraft_engine_feed]

`ProtocolEngine::feed`: runs bytes through the shared Kenwood tokenizer (`;` terminated, 500 ms partial timeout) and decodes each command with `decode`. Junk becomes `Unrecognised`. Returns zero or more events per call, empty while a command is incomplete. Mutates only the tokenizer; no I/O or panics.

### `fn decode(&self, text: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_elecraft_decode]

Decodes one tokenized reply (terminator removed). `?` is the radio's refusal and gives `Nak`. An `IF` reply is expanded by `decode_if` into its eight observations (or `Unrecognised` if malformed). Any other reply goes to `decode_one`; an `FT` reply gives the split observation plus a derived transmit-VFO observation (B on, A off). A reply that cannot be decoded becomes `Unrecognised` holding the text plus `;`. `text` is borrowed. Pure.

### `fn decode_one(&self, text: &str) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_elecraft_decode_one]

Decodes one non-`IF` reply by its two-letter name and data. Handles attenuator `RA` (two digits) and preamp `PA` (one digit) via the description's step tables; the numeric levels (three digits, within the model's range); `FA`/`FB` (11-digit hertz); `MD`; `TQ` (transmit state); `FT`, `RT`, `XT` flags; `RO`; `SM` (4 digits, at most 15); `PO`; `SW` (0, or 10 to 999 as the K3-family reports it; a KX3 sends `SW000` in receive, passed on as 0); `BW` (units of 10 Hz); `ID`; `OM`; `RVM` revision. Returns `None` for an unknown name, wrong length, out-of-range value or unknown mode, and the caller reports it as unrecognised. Pure.

## `crates/radioctl-protocols/src/flex.rs`

### `pub fn parse_mhz(text: &str) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_flex_parse_mhz]

Parses a SmartSDR frequency in megahertz (`14.2012`) into hertz with integer arithmetic only. `text` is borrowed. Up to 5 whole digits and up to 15 decimal digits are accepted; either part may be empty but not both (`.5` and `14.` are fine). The first six decimals are exact, and the seventh digit rounds the result up by one hertz when it is 5 or more; further digits are ignored. Returns `None` for any other character (no sign, no exponent), an empty string, or more than five whole digits. Pure, no panics.

### `pub fn parse_mhz_signed(text: &str) -> Option<i64>` [@ANCHOR: radioctl:radioctl_flex_parse_mhz_signed]

Like `parse_mhz` but accepts one leading `-` or `+` and returns signed hertz, for values such as filter edges that are negative on the lower sidebands (`-2.700000` is -2700 Hz). `text` is borrowed. Returns `None` whenever `parse_mhz` rejects the digits or the value does not fit `i64`. A sign with nothing after it is `None`. Pure.

### `pub fn format_mhz(hz: Hz) -> String` [@ANCHOR: radioctl:radioctl_flex_format_mhz]

Formats hertz as megahertz with exactly six decimals, the form a SmartSDR frequency argument takes: 14,201,200 Hz is `14.201200`, 500 Hz is `0.000500`. `hz` is whole hertz and is not range-checked, so a value above the radio's limit is formatted anyway and the radio refuses it. The inverse is `parse_mhz`. Pure, never fails.

### `pub fn mode_name(mode: Mode) -> Option<&'static str>` [@ANCHOR: radioctl:radioctl_flex_mode_name]

Returns SmartSDR's spelling of a mode for a `slice s ... mode=` command: `lsb`, `usb`, `cw`, `am`, `sam`, `fm`, `rtty`, and the data modes as `digu` for `Mode::Data` and `digl` for `Mode::DataR`. Modes the radio has no name for give `None`; the caller must refuse the set. The returned string is static and lowercase. Pure, total.

### `pub fn mode_from_name(name: &str) -> Option<Mode>` [@ANCHOR: radioctl:radioctl_flex_mode_from_name]

Maps a mode name reported in a status line back to a `Mode`, ignoring case (`USB` and `usb` are the same). It accepts the names of `mode_name` and `rtty`; for example `digu` gives `Mode::Data`. `name` is borrowed. Returns `None` for any name not in the table (the radio has modes this vocabulary cannot express), and the caller should then leave the mode field unchanged or report it unrecognised. Pure.

### `pub fn line_may_transmit(line: &str) -> bool` [@ANCHOR: radioctl:radioctl_flex_line_may_transmit]

Classifies a SmartSDR command line (without its `C<seq>|` prefix) as one that can put RF on the air or play a recording to the transmitter; the transmit gate is applied to those lines. Deliberately broad: `xmit` with an argument that is not clearly off (`0`, `-`, `f`, `off`), `transmit tune` not clearly off, any `cwx` command except `cwx clear`, any `dvk` command, and `atu start`. The verb and arguments are compared case-insensitively. Everything else, including an empty line, is false. Errs on the side of true so an unknown spelling is gated. Pure, no panics.

### `pub fn cwx_text(text: &str) -> Result<String, RigError>` [@ANCHOR: radioctl:radioctl_flex_cwx_text]

Builds the quoted argument of `cwx send`: spaces are replaced by byte 0x7f (as the protocol requires) and the whole text is wrapped in double quotes. `text` is borrowed. Errors, all `ErrorKind::Rejected` with a detail: empty text, more than 100 characters (our limit; the docs give none), a double quote or bar `|` (they would break the command framing), or any character that is not printable ASCII. Result is a new `String`. Pure, no I/O. The `cwx send` line it feeds is a transmit command and is gated through `line_may_transmit`.

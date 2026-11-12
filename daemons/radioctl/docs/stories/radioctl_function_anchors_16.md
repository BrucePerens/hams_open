# radioctl function anchors, batch 16

Function anchors for `radioctl-core` (errors and the command vocabulary) and `radioctl-protocols` (the Racal, Ten-Tec, Alinco, Drake and Icom CI-V helpers), written to the ADR 0113 standard. Tests cite its anchor with `Tests [@ANCHOR: ...]`. The protocol engines here only turn an `Op` into bytes and bytes into observations; they never open a port. Anything that can key a transmitter (`SetPtt(true)`, `SendCw`) is refused by the rig actor when the transmit gate is closed or the transmit frequency is unknown, before an engine is asked to encode it; the engine functions below do not repeat that check, and the twins only model a transmitter in memory.

## `crates/radioctl-protocols/src/racal.rs`

### `pub fn from_name(name: &str) -> Option<Dialect>` [@ANCHOR: radioctl:radioctl_racal_dialect_from_name]

Maps the dialect name written in a rig description file (`vendor.dialect`) to the Racal-family `Dialect` enum. The match is exact and case sensitive: `ra6790`, `rx340`, `rx331` and `ra3702` give the four variants, anything else (including an empty string or different case) gives `None`. `name` is borrowed and not kept. Pure, no errors or panics; the engine and twin constructors turn `None` into an `Unsupported` error ("unknown Racal dialect in the description"). It is the only place a dialect name is spelled.

### `pub fn frequency_text(self, hz: Hz) -> String` [@ANCHOR: radioctl:radioctl_racal_dialect_frequency]

Formats a frequency as the text field of a Racal set-frequency command, which differs per dialect. `hz` is whole hertz. `Ra6790` gives the shortest megahertz decimal with trailing zeros removed (14.2 MHz is `14.2`, 7 MHz is `7`); `Rx340` and `Rx331` always give six decimals (`14.200000`); `Ra3702` gives plain hertz (`14200000`). Pure, never fails, no range check: a value the receiver cannot tune is formatted anyway and the radio decides. Used by the engine to build commands and by the twin to answer a frequency read.

### `fn body(self, line: &[u8]) -> Option<&[u8]>` [@ANCHOR: radioctl:radioctl_racal_dialect_body]

Strips the dialect's reply framing from one received line (CR already removed) and returns the command body, or `None` when the line is not addressed to us. `Rx331` replies must start with `$` and a receiver number; the number must be exactly `0` (the receiver the engine addressed, as Hamlib does), otherwise `None`. `Ra3702` replies must start with LF, which is removed; a line without it is `None`. `Ra6790` and `Rx340` return the line unchanged. `line` is borrowed and the result borrows from it. Pure; a `None` becomes an unrecognised-bytes event in the engine, not a panic.

### `fn prefix(self) -> &'static str` [@ANCHOR: radioctl:radioctl_racal_dialect_prefix]

The text every command of a dialect starts with: empty for `Rx340`, `"\n"` (LF) for `Ra3702`, and `"$0"` (dollar sign, receiver number 0) for `Ra6790` and `Rx331`. The engine puts it in front of each command and the twin removes it from each received line, ignoring lines that lack it. Private, pure, no errors. The receiver number is fixed at 0 because that is the Hamlib default; there is no way to address another receiver.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_racal_twin_snapshot]

Implements `VendorTwin::snapshot` for the virtual Racal receiver: a copy of what the twin holds now, used by the Hamlib oracle comparison tests and the serial-vendor test, not by the daemon. Always `Some`: `frequency` is the twin's tuned frequency, `mode_code` is `Some` of the dialect's mode byte (the code from the description's modes table, for example `b'1'` for AM), and `ptt` is always `false` because a Racal receiver never transmits. Borrows the twin immutably; pure, no side effects, no locks.

## `crates/radioctl-protocols/src/tentec.rs`

### `pub fn from_name(name: &str) -> Option<Dialect>` [@ANCHOR: radioctl:radioctl_tentec_dialect_from_name]

Maps the dialect name of a Ten-Tec description to the `Dialect` enum: `orion` (Orion TT-565/566 and Eagle TT-599, ASCII), `argonaut` (Argonaut V TT-516, binary with `G` acknowledgements), `jupiter` (TT-538) and `omnivii` (TT-588), both binary without acknowledgements. Exact, case-sensitive match; anything else is `None`. `name` is borrowed. Pure, no panics. The engine and twin constructors report `None` as an `Unsupported` error. Do not confuse it with the `rx320` and `tt550` names, which belong to the separate `tentec_rx320` module and are routed before this one.

### `pub fn vfo_letter(vfo: Vfo) -> u8` [@ANCHOR: radioctl:radioctl_tentec_vfo_letter]

Returns the ASCII letter Ten-Tec commands use for a VFO: `b'A'` for `Vfo::A`, `b'B'` for `Vfo::B`. It appears in Orion text commands (`?AF`, `*BF...`) and in the binary frames (`?A`, `*B` plus four bytes, and the `A`/`B` that starts a frequency reply). The engine also compares the letter of a reply with the VFO it asked for, so a reply for the wrong VFO is reported as unrecognised. Pure, total over the two-variant `Vfo`, no errors.

### `pub fn encode_frequency(hz: Hz) -> Option<[u8; 4]>` [@ANCHOR: radioctl:radioctl_tentec_encode_frequency]

Encodes a frequency as the four big-endian bytes the binary dialects (Argonaut, Jupiter, Omni VII) carry: 14,200,000 Hz is `00 D8 AC C0`. `hz` is whole hertz. Returns `None` when the value does not fit in 32 bits (above 4,294,967,295 Hz); the engine turns that into an `Unsupported` error "frequency does not fit four bytes" instead of sending a truncated number. Pure; the inverse is `decode_frequency`.

### `pub fn decode_frequency(bytes: &[u8]) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_tentec_decode_frequency]

Decodes the four big-endian bytes of a binary Ten-Tec frequency into hertz. `bytes` is borrowed and must be exactly four long; any other length (including three or five) gives `None`, so a short or over-long reply is never half-read. The result is a `u32` value widened to `Hz`, so it never exceeds 4,294,967,295. Used by the engine to read frequency replies and by the twin to accept `*A`/`*B` writes (a `None` there makes the twin answer `Z` CR, a refusal). Pure.

### `pub fn new(desc: RigDescription) -> Result<TentecEngine, RigError>` [@ANCHOR: radioctl:radioctl_tentec_engine_new]

Builds the Ten-Tec client engine from a vendor serial description. `desc` is taken by value and kept inside the engine (it supplies the capability set and the mode-code table). The dialect comes from `vendor.dialect` through `vendor_of` and `Dialect::from_name`. Errors, both `ErrorKind::Unsupported`: the description is not a vendor serial description, or its dialect is not one of the four Ten-Tec names. The new engine has nothing pending, an empty receive buffer and no remembered VFO B mode. No I/O, no locks.

### `fn encode_mode_write(&self, mode: Mode) -> Result<Vec<u8>, RigError>` [@ANCHOR: radioctl:radioctl_tentec_encode_mode_write]

Builds the command that sets the receive mode. The character comes from the description's mode table. Orion sends `*RMM<c>` CR. The binary dialects send `*M<a><b>` CR, where both VFOs' modes are written at once: `a` is the new code and `b` repeats the VFO B mode of the last `M` reply, or the new code too until a reply has been read (the documented compromise). Error: `Unsupported` ("mode not in the description") when the description has no code for `mode`. Borrows the engine immutably; pure, sends nothing.

### `fn encode_ptt(&self, on: bool) -> Result<Vec<u8>, RigError>` [@ANCHOR: radioctl:radioctl_tentec_encode_ptt]

Builds the key or unkey command: Orion `*TK` / `*TU` CR, Argonaut `#1` / `#0` CR, Jupiter `Q1` / `Q0` CR, Omni VII `*T` followed by byte 4 (key) or 0 (unkey), byte 1 and CR. `on` true is the transmit command. It never fails (the `Result` is for symmetry with the mode encoder) and has no side effects. It does not apply the transmit gate, band check or lock: the rig actor checks those before `encode` is called for `Op::SetPtt(true)`, and an unkey is an emergency command that bypasses the gate.

### `fn frame_len(&self, pending: Pending) -> usize` [@ANCHOR: radioctl:radioctl_tentec_frame_len]

Gives the exact byte count of the answer the binary dialects send for the awaited reply, so the engine knows when a whole frame has arrived. Frequency read: 8 bytes on Argonaut (letter, four bytes, CR, `G`, CR), 6 on the others. Mode read: 6 on Argonaut, 4 on the others. PTT read: 6. Bare acknowledgement: 2 (`G` CR). Nothing pending: 0. Pure, private. Orion does not use it because its replies are CR-terminated lines. Only Argonaut can answer `PTT`, so 6 only matters there.

### `fn decode_binary(&mut self, pending: Pending, frame: &[u8]) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_tentec_decode_binary]

Turns one complete binary frame into inbound events given what was awaited. `G` CR becomes `Ack`; a frequency frame (`A`/`B`, four bytes, CR) whose letter matches the VFO asked becomes `Observation::Frequency`; an `M<a><b>` frame becomes `Observation::Mode` from the code of `a` and remembers `(a, b)` for later mode writes (the only mutation); a `C` frame becomes `Observation::Ptt` from bit 0 of the second status byte. On Argonaut a read frame must end with `CR G CR`. Anything else, an unknown mode code or a wrong letter gives one `Unrecognised(frame)`. Never panics.

### `fn decode_orion_line(&mut self, pending: Pending, line: &[u8]) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_tentec_decode_orion_line]

Decodes one CR-stripped Orion line. A line starting `Z!` is the radio's refusal and gives `Nak`, whatever was pending. `@<letter>F<digits>` while a frequency of that VFO is pending gives `Observation::Frequency` (the digits are a decimal up to 18 digits, parsed by `parse_decimal`); `@RMM<c>` while the mode is pending gives `Observation::Mode` through the description's code table. Everything else, an unknown mode character or non-digit frequency gives `Unrecognised(line)`. Mutates nothing in the engine; pure apart from allocation.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_tentec_engine_encode]

`ProtocolEngine::encode` for Ten-Tec: turns one `Op` into the bytes to write and the reply to wait for. Supported: get/set frequency per VFO, get/set mode, `SetPtt`, and `GetPtt` on Argonaut only; set commands expect `Ack` on Argonaut and nothing on the other dialects (Orion never acknowledges writes). Errors, all `Unsupported`: the capability the op needs (`required_capability`) is not in the description, a frequency above 32 bits on a binary dialect, a mode missing from the table, or any other op. On success it records what is pending and clears the receive buffer, so call it once per outstanding command, from the actor. It does not apply the transmit gate itself (see the batch note).

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_tentec_engine_feed]

`ProtocolEngine::feed`: appends received `bytes` (stamped `now`, a monotonic duration) and returns the decoded events. A partial frame older than the library's partial-frame timeout is dropped and reported as `Unrecognised`. Orion: every complete CR line is decoded against what was pending, which is then cleared. Binary dialects: with nothing pending all bytes are junk (`Unrecognised`); on Argonaut a bare `Z` CR is a `Nak` and clears the pending read; otherwise it waits until `frame_len` bytes are present, decodes them and reports any extra bytes as `Unrecognised`. Returns an empty vector while a frame is incomplete. No I/O, no blocking, never panics.

### `pub fn new(desc: &RigDescription) -> Result<TentecTwin, RigError>` [@ANCHOR: radioctl:radioctl_tentec_twin_new]

Builds the virtual Ten-Tec radio in the description's default state. `desc` is only borrowed. Both VFOs start at `default_frequency`; both modes start at the code of `default_mode`, or the first code in the table, or `b'0'` when the table is empty; PTT is off; the code list is copied so the twin can validate mode writes. Errors, both `Unsupported`: not a vendor serial description, or an unknown Ten-Tec dialect. Used by tests, the simulator and the oracle harness; it never touches a real port or transmitter.

### `fn orion_line(&mut self, line: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_tentec_twin_orion_line]

The twin's Orion command handler for one CR-stripped line; returns the bytes to send back. `?<A|B>F` answers `@<letter>F` and the frequency in eight zero-padded digits; `*<A|B>F<digits>` stores the frequency and answers nothing; `?RMM` answers `@RMM<code>` for VFO A; `*RMM<c>` sets both modes when `c` is in the code table; `*TK` sets the in-memory PTT flag and `*TU` clears it, silently. Anything else (unknown VFO, bad digits, a mode code not in the table, an unknown command) returns `Z!` CR, the refusal. Mutates only the twin.

### `fn binary_frame(&mut self) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_tentec_twin_binary_frame]

Handles one binary command at the front of the twin's buffer. Returns `None` when more bytes are needed (so the caller stops looping), otherwise `Some(reply)`, possibly empty. Command length is inferred from the first two bytes (`*A`/`*B` 7 bytes, `*M`/`*T` 5, `*W` 4, other 3). It serves frequency read and write, mode read and write, Hamlib's filter-width `*W` (acknowledged, ignored), PTT (`#`, `Q`, `*T` per dialect), and the Argonaut-only `?C`, `?E` (always VFO A) and `?O` (split off) queries. The `G` CR acknowledgement is added only on Argonaut. An unknown or malformed frame is answered `Z` CR on Argonaut and silently ignored on the others. Mutates frequency, mode and PTT.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_tentec_twin_snapshot]

Reports what the Ten-Tec twin holds, for the Hamlib oracle comparison and the serial-vendor test. Always `Some`: `frequency` is VFO A's frequency in hertz, `mode_code` is `Some` of VFO A's mode byte, and `ptt` is the twin's in-memory key flag, which is true after a key command or a front-panel PTT. VFO B is not reported. Borrows the twin immutably; pure, no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_tentec_twin_feed]

`VendorTwin::feed`: the twin consumes the controller's bytes (stamped `now`) and returns what the radio would send back. Orion: each complete CR line goes to `orion_line`. Binary dialects: `binary_frame` is called repeatedly until it needs more bytes. Partial input stays buffered, and a partial frame older than the partial-frame timeout is dropped by the buffer. The returned bytes are the concatenation of all replies, empty when nothing is due. No I/O or locks; state changes are limited to the twin's own frequency, mode and PTT. It cannot transmit.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_tentec_twin_front_panel]

Simulates the operator touching the radio. `Panel::Frequency(hz)` sets VFO A's frequency, `Panel::Ptt(on)` sets the key flag; both return an empty byte vector (this family sends no unsolicited reports). `Panel::Mode(_)` is refused with the message that the twin takes mode changes over the wire, and any other panel knob with a message naming it. The `String` is a human-readable reason for the test or simulator. `panel` is owned. In-memory only; the flag models a transmitter but no RF exists.

## `crates/radioctl-protocols/src/alinco.rs`

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_alinco_twin_snapshot]

Reports the Alinco twin's state to the oracle and serial-vendor tests. Always `Some`: `frequency` is the held frequency in hertz, `mode_code` is `Some` of the stored mode byte (a code from the description's table), and `ptt` is the twin's key flag, set by the `PTT` write of the dialect (Dx77 and Dxsr8 differ in framing, not in this accessor). Borrows the twin immutably. Pure, no side effects; it never reads from a port and cannot transmit.

## `crates/radioctl-protocols/src/drake.rs`

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_drake_twin_snapshot]

Reports the Drake receiver twin's state. Always `Some`. The twin stores frequency in 10 Hz units, so `frequency` is that count times 10, in hertz. `mode_code` is the byte the description's mode table gives for the twin's current `Mode`, and is `None` if the table has no entry for it. `ptt` is always `false` because a Drake receiver does not transmit. Borrows immutably; pure, no side effects. Used by the Hamlib oracle comparison and the serial-vendor test.

## `crates/radioctl-core/src/error.rs`

### `pub fn new(kind: ErrorKind, detail: impl Into<String>) -> RigError` [@ANCHOR: radioctl:radioctl_core_error_new]

Builds a `RigError` with a default retry hint chosen from the kind and an empty transcript. `kind` is the failure class; `detail` is any string-like value, converted and owned. Default hints: `Timeout`, `QueueFull`, `Cancelled` and `TransmitFrequencyUnknown` give `RetryHint::Now`; `LinkDown` gives `After(1 s)`; `Rejected`, `Unsupported`, `TransmitGateClosed` and `Malformed` give `Never`. A closed transmit gate is never retryable by itself, while an unknown transmit frequency is, because reading the frequency fixes it. Pure, cannot fail.

### `pub fn with_transcript(mut self, tail: Vec<String>) -> RigError` [@ANCHOR: radioctl:radioctl_core_error_with_transcript]

Consumes the error and returns it with the transcript tail set: the last wire lines (hex, direction-prefixed, oldest first) seen before the failure, so a log or a client can show what was on the wire. `tail` is owned and replaces any previous tail; it is not truncated or validated here (the caller decides how many lines). Builder style, used after `RigError::new`; the kind, retry hint and detail are unchanged. Pure, no errors.

## `crates/radioctl-core/src/op.rs`

### `pub fn field(&self) -> Field` [@ANCHOR: radioctl:radioctl_core_observation_field]

Returns the cache `Field` an `Observation` updates, so the actor can store the value and match it to a waiting request. The mapping is one to one: for observations that carry a key it copies it (`Frequency(vfo, _)` gives `Field::Frequency(vfo)`, `Level(l, _)` gives `Field::Level(l)`, `Meter(m, _)` gives `Field::Meter(m)`, scope variants keep the receiver number and setting), the others give the plain field, and `Alarm` gives `Field::Alarm`. Borrows the observation; pure, total, no errors.

### `pub fn required_capability(&self) -> Capability` [@ANCHOR: radioctl:radioctl_core_op_capability]

Gives the `Capability` a rig description must list before an `Op` may be sent: for example `GetFrequency(A)` needs `ReadFrequency`, VFO B reads and writes need `SecondVfo`, `SetPtt` needs `SetPtt`, `SendCw` and `StopCw` need `SendCw`, split reads and writes and `GetTxVfo` need `Split`, RIT/XIT ops need `RitXit`, `GetModelId`, `GetFirmware` and `GetPowerAmplifier` need `Identify`, and every scope op needs `Scope`. Pure, total over `Op`. Engines call it first in `encode` and answer `Unsupported` when the rig lacks the capability.

### `pub fn transmits(&self) -> bool` [@ANCHOR: radioctl:radioctl_core_op_transmits]

True when the command can put RF on the air: `SetPtt(true)` and `SendCw(_)`. `SetPtt(false)`, `StopCw` and everything else, including frequency changes and reads, return false. This is the predicate the actor uses to decide that the transmit gate, the band check on the transmit frequency and the transmit lock must be satisfied before the command is sent; a refusal is `TransmitGateClosed` or `TransmitFrequencyUnknown`. Pure, total, no side effects.

### `pub fn is_emergency(&self) -> bool` [@ANCHOR: radioctl:radioctl_core_op_is_emergency]

True for `SetPtt(false)` (unkey) and `StopCw`, the commands that must get through when things go wrong. The actor lets them jump the queue, preempt a command that is waiting for a reply, skip the spacing between commands, and does not require the transmit gate for them. Everything else is false, including `SetPtt(true)`. Pure, total. A superset of `is_unkey`.

### `pub fn is_unkey(&self) -> bool` [@ANCHOR: radioctl:radioctl_core_op_is_unkey]

True only for `SetPtt(false)`, the one command whose failure can mean the rig is still transmitting. The actor uses it to coalesce unkeys when the emergency queue is full and to raise `Alarm::UnkeyFailed` when an unkey fails after every attempt. A failed `StopCw` is not an unkey and is reported only to its caller. Pure, total.

## `crates/radioctl-protocols/src/civ.rs`

### `pub fn to_bytes(&self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_civ_frame_to_bytes]

Serialises a CI-V `Frame` to the wire form: `FE FE`, destination address `to`, source address `from`, command byte `cmd`, the `data` bytes (sub-command and payload, may be empty), then `FD`. Returns a new vector; the frame is borrowed and unchanged. No length limit or validation is applied here (the parser, not this function, enforces the 256 byte pending cap), and no byte in `data` is escaped, so the caller must not put `FD` in it. Pure.

### `pub fn encode_bcd_freq(hz: Hz) -> Option<[u8; 5]>` [@ANCHOR: radioctl:radioctl_civ_encode_bcd_freq]

Encodes a frequency as the five-byte BCD field of CI-V, least-significant digit pair first (14,200,000 Hz is `00 00 20 14 00`). `hz` is whole hertz, 0 to 9,999,999,999. Above that it returns `None`, and the engine reports an unsupported frequency rather than sending a wrong one. Each byte holds two decimal digits in its nibbles, so every output nibble is 0 to 9. Pure; the inverse is `decode_bcd_freq`. Models with other field widths use `encode_bcd_freq_len`.

### `pub fn decode_bcd_freq(bytes: &[u8]) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_civ_decode_bcd_freq]

Decodes a CI-V BCD frequency, least-significant pair first, from 1 to 6 bytes (4 on eight-digit models, 5 on ten-digit models, 6 on twelve-digit ones). `bytes` is borrowed. Returns `None` for an empty slice, more than 6 bytes, or any nibble above 9 (non-BCD, a sign that the bytes are not a frequency). Otherwise the exact value in hertz, up to twelve digits. Pure, no panics, no overflow because the maximum is 10^12 - 1.

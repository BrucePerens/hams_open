# radioctl function anchors, batch 13

Function anchors for the `radioctl-protocols` crate (Transfox, Paragon, MICOM and HiQSDR vendor families), written to the ADR 0113 standard. Each anchor names a function in the crate source and is exercised by a test that cites it with `Tests [@ANCHOR: ...]`. Engines only convert operations to bytes and bytes to observations; the transmit lock, band check and transmit gate are applied by the actor in `radioctl-core` that calls them, not here. Entries that can put a key-down on the wire (Transfox `TX;`, MICOM key frame, HiQSDR keyed bit) say so. Twins are virtual radios for tests and never touch hardware.

## `crates/radioctl-protocols/src/transfox.rs`

### `pub fn new(desc: RigDescription) -> Result<TransfoxEngine, RigError>` [@ANCHOR: radioctl:radioctl_transfox_engine_new]

Builds the client engine for a Transfox transceiver. `desc` is owned and kept by the engine. The dialect in the vendor part of the description must be `transfox`; otherwise, or for a description that is not a vendor serial one, it returns `Unsupported`. The receive buffer starts empty. No I/O; the engine has no open sequence.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_transfox_engine_encode]

Builds one `;` terminated command. `op` is borrowed. `GetFrequency(A)` sends `FA;` and expects a frequency observation; `GetPtt` sends `Cs;` and expects a PTT observation. `SetFrequency(A, hz)` sends `FA` plus eleven zero padded digits of hertz and expects no reply; the frequency must be 1,000,000 to 1,450,000,000 Hz inclusive or `Unsupported` is returned. `SetPtt(on)` sends `TX;` to key or `RX;` to unkey and expects no reply. Anything else, a missing capability or another VFO returns `Unsupported`. The engine builds bytes only: the actor applies the transmit gate and the fail-closed unkey around `SetPtt`. The receive buffer is cleared on success.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_transfox_engine_feed]

Splits received bytes at `;` and decodes each answer. A partial answer older than 500 ms is returned as `Inbound::Unrecognised` and dropped. Each piece is trimmed; an empty piece (a bare `;`) is ignored. Pieces `decode` understands become observations (frequency of VFO A, PTT); others are returned as `Unrecognised` with the raw bytes. The engine does not track which request is outstanding, so an answer is accepted whenever it arrives. Returns the items in order; never blocks.

### `pub fn new(desc: &RigDescription) -> Result<TransfoxTwin, RigError>` [@ANCHOR: radioctl:radioctl_transfox_twin_new]

Builds a virtual Transfox for tests. `desc` is borrowed; the dialect must be `transfox` or `Unsupported` is returned. The frequency starts at the description's default (not clamped here) and the radio starts unkeyed. No I/O.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_transfox_twin_snapshot]

Reports the twin's state for the Hamlib oracle batch. The result is always `Some`: `frequency` is the current frequency in hertz, `ptt` is the modelled transmit state (true after `TX;`) and `mode_code` is `None` because the radio has no mode in this protocol. Immutable, no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_transfox_twin_feed]

Consumes controller bytes and returns replies. Commands end at `;`; bytes enter the buffer with monotonic time `now` (a partial command older than 500 ms is dropped). `FA` answers `FA` plus eleven digits, `Cs` answers `TX;` or `RX;`, `TX`, `TX0` and `TX1` set the modelled key state and `RX` clears it (no reply), and `FA<digits>` sets the frequency clamped to 1 to 1450 MHz (no reply). Unknown or unparseable commands are ignored. Nothing is transmitted; the key state is a flag.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_transfox_twin_front_panel]

Simulates a knob on the radio. `panel` is owned. `Panel::Frequency(hz)` sets the frequency clamped to 1 to 1450 MHz; `Panel::Ptt(on)` sets the modelled transmit state. Other controls return `Err("a Transfox twin has no <panel> knob")`. Success returns an empty vector (the radio does not report changes unprompted). Nothing is transmitted.

## `crates/radioctl-protocols/src/paragon.rs`

### `pub fn frequency_text(hz: Hz) -> String` [@ANCHOR: radioctl:radioctl_paragon_frequency_text]

Builds the Paragon frequency command: megahertz with five decimals, the letter `W` in place of the decimal point, and `@` at the end, for example 14,074,000 Hz gives `14W07400@`. `hz` is in hertz, rounded to the nearest 10 Hz (`(hz + 5) / 10`); the fractional part is five digits (units of 10 Hz). Pure, cannot fail and does not range check (the engine checks 100 kHz to 29,999,990 Hz first).

### `pub fn mode_command(mode: Mode) -> Option<&'static str>` [@ANCHOR: radioctl:radioctl_paragon_mode_command]

Returns the command text that selects a mode on the Paragon: LSB `N`, USB `O`, CW `P`, FM `L`, AM `M`, RTTY `XP`. `mode` is the shared `Mode` enum, passed by value. Any other mode returns `None`, which the engine reports as `Unsupported`. Pure.

### `pub fn decode_status(block: &[u8]) -> Option<(Hz, Option<Mode>)>` [@ANCHOR: radioctl:radioctl_paragon_decode_status]

Decodes the 30 byte status block the radio sends after a `\` request. `block` must be exactly 30 bytes (else `None`). The first seven bytes are decimal digits 0 to 9, most significant first, in units of 10 Hz (any byte above 9 returns `None`); the result frequency is that number times 10 in hertz. Byte 7 is the mode bitmask: the first bit found, testing in the order CW 0x02, USB 0x04, LSB 0x08, AM 0x10, FM 0x20, RTTY 0x40, gives the mode, and the mode is `None` if no bit is set. The other bytes (filter, VFO, split, level) are ignored. Pure.

### `pub fn new(desc: RigDescription) -> Result<ParagonEngine, RigError>` [@ANCHOR: radioctl:radioctl_paragon_engine_new]

Builds the client engine for a Paragon HF transceiver. `desc` is owned and kept. The dialect must be `paragon`, otherwise (or for a non-vendor description) `Unsupported`. The buffer starts empty. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_paragon_engine_encode]

Builds the bytes for an operation. `op` is borrowed. `GetFrequency(A)` and `GetMode` both send a single backslash, which makes the radio send its 30 byte status block, and expect an observation of the field asked for. `SetFrequency(A, hz)` sends `frequency_text` and expects no reply; the frequency must be 100,000 to 29,999,990 Hz or `Unsupported` is returned. `SetMode` sends the `mode_command` letters after checking the mode is in the description; a mode without a command, or absent from the description, returns `Unsupported`. Other operations (there is no PTT) and missing capabilities return `Unsupported`. The receive buffer is cleared.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_paragon_engine_feed]

Collects status blocks. Bytes are pushed with monotonic time `now`; a partial block older than 500 ms is returned as `Inbound::Unrecognised` and dropped. Whenever 30 bytes are held they are taken and passed to `decode_status`: a valid block yields a VFO A frequency observation and, if a mode bit was set, a mode observation; an invalid block (a digit above 9) is returned as `Unrecognised`. Several blocks in one chunk give several results in order. Fewer than 30 bytes wait for the rest.

### `pub fn new(desc: &RigDescription) -> Result<ParagonTwin, RigError>` [@ANCHOR: radioctl:radioctl_paragon_twin_new]

Builds a virtual Paragon for tests. `desc` is borrowed; the dialect must be `paragon` or `Unsupported` is returned. The frequency is the description's default divided by 10 (units of 10 Hz). The mode bit is the default mode's code, or 0x04 (USB) if the description has none; the filter bit starts 0x40, VFO A is selected and split is off. No I/O.

### `fn status(&self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_paragon_twin_status]

Builds the 30 byte status block the twin sends in answer to a backslash. The first seven bytes are the frequency as decimal digits (units of 10 Hz, most significant first), byte 7 is the mode bit, byte 8 the filter bit, byte 9 holds 0x08 when VFO A is selected and 0x02 when split is on, byte 11 is fixed at 63 and all other bytes are zero. Returns a new vector; immutable, no side effects. The layout follows what Hamlib reads; nothing else is verified.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_paragon_twin_snapshot]

Reports the twin's state for the Hamlib oracle batch. Always `Some`: `frequency` is the held frequency in hertz (units times 10), `mode_code` is the raw mode bit byte (for example 0x04 for USB, not an ASCII letter) and `ptt` is always false because this radio has no software PTT. Immutable, no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_paragon_twin_feed]

Consumes controller bytes and returns the status block when asked. Bytes are buffered with monotonic time `now` (partial data older than 500 ms is dropped). The loop takes one command at a time and stops when the next one is incomplete. A command starting with a digit or `W` runs to an `@`; with `W` read as the decimal point it is parsed as megahertz and, if the value fits (below 1000 MHz), sets the frequency in units of 10 Hz; an unparseable one is consumed and ignored. `XP` selects RTTY mode and another `X` letter is consumed and ignored; `:` and `<` consume three bytes. Single letters set mode (`N` LSB, `O` USB, `P` CW, `L` FM, `M` AM), filter (`R` to `V`), toggle VFO (`F`) or split (`J`), and a backslash appends the 30 byte status block to the reply. Unknown bytes are dropped.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_paragon_twin_front_panel]

Simulates a knob turned on the radio. `panel` is owned. `Panel::Frequency(hz)` sets the frequency, clamped to 100,000 to 29,999,990 Hz and rounded down to 10 Hz. `Panel::Mode(mode)` selects a mode that has a status bit (CW, USB, LSB, AM, FM, RTTY); any other mode returns `Err("a Paragon has no <mode> mode")`. Every other control returns `Err("a Paragon twin has no <panel> knob")`. Success returns an empty vector. No PTT exists, so nothing is transmitted.

## `crates/radioctl-protocols/src/micom.rs`

### `pub fn checksum(bytes: &[u8]) -> u8` [@ANCHOR: radioctl:radioctl_micom_checksum]

Computes the MICOM frame checksum: the sum of the given bytes modulo 256 (wrapping addition). `bytes` is borrowed and must be the leading part of the frame, from the start byte through the data, without the checksum itself and the end byte. An empty slice gives 0. Pure, cannot fail. Used to build the frequency frame and the twin's answers.

### `pub fn set_frequency_frames(hz: Hz) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_micom_set_frequency_frames]

Builds the byte sequence that sets the frequency, as Hamlib's captured exchange does: one frequency frame followed by four fixed frames, all written back to back. The first frame is start 0x24, length 0x06, command bytes 0x18 0x05 0x01, the frequency as four big endian bytes, the checksum of those bytes, the end byte 0x03 and a trailer 0x15. `hz` is in hertz; a value above `u32::MAX` saturates to `u32::MAX` (the engine rejects those earlier). Returns a new vector; pure, no I/O. The meaning of the four fixed frames is unknown beyond "copied from Hamlib's capture"; check the Hamlib source before changing them.

### `pub fn decode_frequency(frame: &[u8]) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_micom_decode_frequency]

Reads the frequency out of an answer frame. `frame` must be exactly ten bytes, start with 0x24 and end with 0x03; otherwise `None`. Bytes 4 to 7, big endian, are the frequency in hertz. The checksum is not verified. Shorter frames (acknowledgements) give `None`. Pure.

### `fn take_frame(rx: &mut RxBuffer) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_micom_take_frame]

Cuts one MICOM frame from the front of the receive buffer `rx` (mutated). Bytes before a start byte 0x24 are discarded one at a time. The second byte is the length L and the frame is L + 5 bytes long; if the whole frame has not arrived it returns `None` and keeps the start of it in the buffer (the discarded stray bytes stay discarded). A trailing 0x15 after the frame is removed too. The checksum is not checked; callers inspect the frame. Used by the engine and the twin.

### `pub fn new(desc: RigDescription) -> Result<MicomEngine, RigError>` [@ANCHOR: radioctl:radioctl_micom_engine_new]

Builds the client engine for a MICOM HF transceiver. `desc` is owned and kept. The dialect must be `micom` else `Unsupported` (also for a non-vendor description). The buffer starts empty. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_micom_engine_encode]

Builds the bytes for an operation. `op` is borrowed. `GetFrequency(A)` sends a fixed six byte read frame and expects a frequency observation. `SetFrequency(A, hz)` sends the five frames of `set_frequency_frames` and expects no reply; a frequency that does not fit in 32 bits returns `Unsupported`. `SetPtt(true)` sends the fixed key-down frame (command 0x81 0x13) and `SetPtt(false)` the key-up frame (0x81 0x14), neither with a reply; these put the transmitter on air, so the actor's transmit gate and fail-closed unkey apply before and around them. Other operations or missing capabilities return `Unsupported`. The receive buffer is cleared.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_micom_engine_feed]

Splits the received stream into frames and decodes frequency answers. A partial frame older than 500 ms is returned as `Inbound::Unrecognised`. Each frame from `take_frame` that `decode_frequency` accepts becomes a VFO A frequency observation. A six byte frame (the short acknowledgement) is dropped silently because nothing uses it; any other frame is returned as `Unrecognised`. Results are in arrival order. Never blocks.

### `pub fn new(desc: &RigDescription) -> Result<MicomTwin, RigError>` [@ANCHOR: radioctl:radioctl_micom_twin_new]

Builds a virtual MICOM for tests. `desc` is borrowed; the dialect must be `micom` or `Unsupported` is returned. The frequency is the description's default in hertz as a 32-bit value (0 if it does not fit) and the radio starts unkeyed. No I/O.

### `fn frequency_frame(&self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_micom_twin_frequency_frame]

Builds the ten byte frame the twin sends in answer to a frequency read: start 0x24, length 0x05, bytes 0x80 and 0x06, the frequency as four big endian bytes (bytes 4 to 7), the checksum and the end byte 0x03. Bytes 2 and 3 are an assumption, since Hamlib reads only the frequency out of the answer; check a real radio before relying on them. Immutable, returns a new vector.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_micom_twin_snapshot]

Reports the twin's state for the Hamlib oracle batch. Always `Some`: `frequency` is the held frequency in hertz, `ptt` is the modelled key state (set by the key-down frame) and `mode_code` is `None`. Immutable, no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_micom_twin_feed]

Consumes controller bytes and returns replies. Bytes are buffered with monotonic time `now` (a partial frame older than 500 ms is dropped) and cut into frames by `take_frame`. A frame with command bytes 0x18 0x05 0x01 and at least ten bytes sets the frequency from bytes 5 to 8 and answers the acknowledgement frame; 0x18 0x36 answers the acknowledgement; 0x18 0x06 answers the acknowledgement followed by the frequency frame; 0x81 0x13 sets the modelled key state and 0x81 0x14 clears it, with no reply. Other frames are ignored. Checksums are not verified. Nothing is transmitted.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_micom_twin_front_panel]

Simulates a knob on the radio. `panel` is owned. `Panel::Frequency(hz)` sets the frequency (a value that does not fit 32 bits leaves it unchanged) and `Panel::Ptt(on)` sets the modelled key state. Other controls return `Err("a MICOM twin has no <panel> knob")`. Success returns an empty vector. Nothing is transmitted.

## `crates/radioctl-protocols/src/hiqsdr.rs`

### `pub fn phase_word(hz: Hz) -> u32` [@ANCHOR: radioctl:radioctl_hiqsdr_phase_word]

Computes the HiQSDR tuning phase word of a frequency: `hz / 122,880,000 * 2^32`, rounded to nearest, using 128-bit integer arithmetic so it is exact. `hz` is in hertz. The result is saturated to `u32::MAX` if the quotient does not fit (frequencies about the reference clock and above). Pure, no errors. The engine range checks 100 kHz to 66 MHz before calling it and sends the word little endian.

### `pub fn word_frequency(word: u32) -> Hz` [@ANCHOR: radioctl:radioctl_hiqsdr_word_frequency]

The inverse of `phase_word`: the frequency a phase word tunes to, `word * 122,880,000 / 2^32` rounded to the nearest hertz. `word` is the raw 32-bit value. Pure integer arithmetic, no errors. Used by tests and the twin to read back what was sent.

### `pub fn initial_frame() -> [u8; FRAME_LEN]` [@ANCHOR: radioctl:radioctl_hiqsdr_initial_frame]

Returns the 22 byte control frame Hamlib builds when it opens a HiQSDR: bytes 0 and 1 are `S` and `t`, both tuning words are zero, byte 10 (level) is 120, byte 11 (mode) is `MODE_OTHER` (0x02, not CW, not keyed) and byte 12 (sample rate selector) is 4, meaning 48 kHz; all other bytes are zero. It transmits nothing by itself. Pure.

### `pub fn new(desc: RigDescription) -> Result<HiqsdrEngine, RigError>` [@ANCHOR: radioctl:radioctl_hiqsdr_engine_new]

Builds the client engine for a HiQSDR. `desc` is owned and kept. The dialect must be `hiqsdr` else `Unsupported` (also for a non-vendor description). The engine holds one control frame, starting as `initial_frame()`, that it modifies and resends in whole for every command. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_hiqsdr_engine_encode]

Updates the engine's 22 byte control frame and returns the whole frame as the request (no reply is expected). `op` is borrowed. `SetFrequency(A, hz)` must be 100 kHz to 66 MHz or `Unsupported`; it writes the phase word, little endian, into both the receive (bytes 2 to 5) and transmit (bytes 6 to 9) tuning fields. `SetMode` accepts only CW (mode byte 0x01) and AM (0x02); any other mode returns `Unsupported`. Because Hamlib replaces the whole mode byte, a mode write clears the keyed bit. `SetPtt(on)` sets or clears the keyed bit 0x08 in the mode byte, so the next frame keys the transmitter; it is refused with a `Rejected` error while the mode is CW (this radio does not key in CW mode). Other operations or missing capabilities return `Unsupported`. The actor applies the transmit gate and fail-closed unkey around `SetPtt`; the frame is only bytes until it is written.

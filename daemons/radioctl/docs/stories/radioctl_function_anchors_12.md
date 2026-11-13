# radioctl function anchors, batch 12

Function anchors for the `radioctl-protocols` crate (Kachina twin, Lowe HF-235, MDS data radios, Elecraft-style XG3 signal source and Transfox), written to the ADR 0113 standard. Each anchor names a function in the crate source and is exercised by a test that cites it with `Tests [@ANCHOR: ...]`. Engines only convert operations to bytes and bytes to observations; the transmit lock, band check and transmit gate are applied by the actor in `radioctl-core` that calls them, not here. Where an entry can put a key-down or output-on command on the wire it says so. Twins are virtual radios for tests and never touch hardware.

## `crates/radioctl-protocols/src/kachina.rs`

### `pub fn new(desc: &RigDescription) -> Result<KachinaTwin, RigError>` [@ANCHOR: radioctl:radioctl_kachina_twin_new]

Builds a virtual Kachina 505DSP for tests. `desc` is borrowed and read only: the dialect must be `505dsp` (else `Unsupported`, which is also returned for a non-vendor description). The default frequency in hertz becomes both the receive and the transmit frequency. The mode is the description's default mode code, or the first code in its mode table, or ASCII `4` as a last resort; the list of valid mode codes is kept so a later `M` command can be checked. PTT (push to talk) starts off and the receive buffer is empty. No I/O.

### `fn take_frame(&mut self) -> Option<u8>` [@ANCHOR: radioctl:radioctl_kachina_twin_frame]

Handles one command frame at the front of the twin's receive buffer and returns the single answer byte, or `None` when more bytes are needed. A first byte that is not STX (0x02) is discarded and answered with the error byte 0xFE. Otherwise the second byte is the command letter, which fixes the data length: 4 for `R` and `T`, 1 for `M` and `x`, 0 for anything else. When the whole frame (STX, letter, data, ETX) has arrived it is removed; if the last byte is not ETX (0x03) the answer is 0xFE. `R` and `T` set the receive or transmit frequency from the four DDS bytes, `M` sets the mode when the code (digit plus ASCII `0`) is in the description, `x` with 0 or 1 sets the modelled PTT. All of those answer 0xFF; an unknown letter or a bad argument answers 0xFE. Mutates the twin; no I/O.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_kachina_twin_snapshot]

Reports the twin's state for oracle comparison. Always `Some`: `frequency` is the receive frequency in hertz, `mode_code` is the current ASCII mode code and `ptt` is the modelled key flag. The transmit frequency is not part of the snapshot. Immutable, no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_kachina_twin_feed]

Consumes the controller's bytes and returns the radio's answer bytes. `bytes` go into the receive buffer with monotonic time `now` (a partial frame older than 500 ms is dropped by the buffer, silently). It then repeatedly takes frames and collects one answer byte per frame, 0xFF for good or 0xFE for error, so two frames in one write give two bytes in order. Incomplete frames give an empty vector until the rest arrives. No I/O, never blocks.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_kachina_twin_front_panel]

Simulates the operator turning a knob on the radio. `panel` is owned. `Panel::Frequency(hz)` sets both the receive and transmit frequency; `Panel::Ptt(on)` sets the modelled key flag. Other controls return `Err("a Kachina twin has no <panel> knob")`. Success returns an empty vector because this radio never reports changes by itself. The frequency is not range checked. Nothing is transmitted.

## `crates/radioctl-protocols/src/lowe.rs`

### `pub fn khz_text(hz: Hz) -> String` [@ANCHOR: radioctl:radioctl_lowe_khz_text]

Formats a frequency the way the HF-235 command `FRQ` wants it: kilohertz with six decimals, for example 14,200,000 Hz gives `14200.000000`. `hz` is in hertz; the whole kilohertz is `hz / 1000` and the fraction is the remaining hertz as three digits followed by `000` (milli-hertz resolution is not used). Returns a new string, pure, cannot fail and does not range check.

### `pub fn parse_khz(text: &str) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_lowe_parse_khz]

Reads a kilohertz decimal and returns hertz, rounded to the nearest hertz. `text` is borrowed. The whole part must be present and all digits; the optional fraction after `.` may have up to six digits, all digits, and is padded to six places. Whole kilohertz are multiplied by 1000 with overflow checking and the milli-hertz fraction is added with rounding. Returns `None` for an empty whole part, more than six decimals, any non-digit (including a sign or a second point) or overflow. Used on radio answers and by the twin to read `FRQ` commands. Pure.

### `pub fn mode_name(code: u8) -> Option<&'static str>` [@ANCHOR: radioctl:radioctl_lowe_mode_name]

Maps a description mode code byte to the mode name the HF-235 uses: `U` is `USB`, `L` is `LSB`, `C` is `CW`, `F` is `FM`, `A` is `AM`. Any other code returns `None`, which the engine reports as a mode the radio cannot do. Pure and case sensitive.

### `pub fn new(desc: RigDescription) -> Result<LoweEngine, RigError>` [@ANCHOR: radioctl:radioctl_lowe_engine_new]

Builds the client engine for a Lowe HF-235 receiver. `desc` is owned and kept. The dialect must be `hf235`, otherwise (or for a non-vendor description) `Unsupported`. The engine starts with no outstanding command and an empty buffer. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_lowe_engine_encode]

Builds the command text for an operation, CR terminated. `op` is borrowed. `GetFrequency(A)` sends `FRQ?` and expects a frequency observation; `SetFrequency(A, hz)` sends `FRQ` plus `khz_text(hz)` and expects an acknowledgement (no range check is done here); `GetMode` sends `MOD?`; `SetMode` sends `MOD` plus the mode name, which must come from the description's code through `mode_name` or `Unsupported` is returned. All other operations, other VFOs (variable frequency oscillator, the tuning control), and operations whose capability the description lacks return `Unsupported`. A receiver, so nothing keys a transmitter. The engine remembers what answer it expects and clears its receive buffer.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_lowe_engine_feed]

Decodes the receiver's CR terminated answers. A partial line older than 500 ms is returned as `Inbound::Unrecognised`. Each complete line consumes the pending expectation. Both kinds of answer carry a one byte tag that is skipped. If an acknowledgement was expected any line gives `Inbound::Ack`; for a frequency request the rest is read with `parse_khz` and gives a VFO A frequency observation or `Unrecognised`; for a mode request the text is matched against the mode names of the description's codes and gives a mode observation or `Unrecognised`. A line when nothing is expected is `Unrecognised`. There is no Nak form.

### `pub fn new(desc: &RigDescription) -> Result<LoweTwin, RigError>` [@ANCHOR: radioctl:radioctl_lowe_twin_new]

Builds a virtual HF-235 for tests. `desc` is borrowed; the dialect must be `hf235` or `Unsupported` is returned. The frequency starts at the description's default and the mode at the default mode's code, with ASCII `A` (AM) if the description has none. No I/O.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_lowe_twin_snapshot]

Reports the twin's state so a test can compare it with a Hamlib oracle or with the engine's cache. The result is always `Some`: `frequency` is the current frequency in hertz and `mode_code` is the ASCII mode code. `ptt` is always false because this is a receiver and cannot transmit. Borrows the twin immutably and has no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_lowe_twin_feed]

Consumes controller bytes and returns the radio's replies. Lines end in CR. `FRQ?` answers `F` plus `khz_text`, `MOD?` answers `M` plus the mode name (AM if the code has no name). `FRQ<kHz>` sets the frequency when `parse_khz` accepts the text and answers a bare CR either way; `MOD<name>` sets the mode to the first code letter whose name matches and answers a bare CR. Unknown lines produce no reply. Mutates the twin; a stale partial line (over 500 ms) is dropped by the buffer.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_lowe_twin_front_panel]

Simulates a knob turned on the receiver. `panel` is owned. Only `Panel::Frequency(hz)` is accepted and sets the frequency; every other control returns `Err("a Lowe twin has no <panel> knob")`. Returns an empty vector on success; the receiver sends nothing unprompted. No range check.

## `crates/radioctl-protocols/src/mds.rs`

### `pub fn mhz_text(hz: Hz) -> String` [@ANCHOR: radioctl:radioctl_mds_mhz_text]

Formats a frequency in megahertz with four decimals for the MDS `TX` and `RX` commands, for example 450,000,000 Hz gives `450.0000`. `hz` is in hertz and is rounded to the nearest 100 Hz (`(hz + 50) / 100`). Returns a new string; pure; no range check.

### `pub fn parse_mhz(text: &str) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_mds_parse_mhz]

Reads a megahertz number and returns hertz. `text` is borrowed and trimmed of surrounding white space. A whole part of at least one digit is required; an optional fraction after `.` may have up to four digits and is scaled to units of 100 Hz (`450.1` is 450,100,000 Hz). Anything else, such as a sign, a second point, letters or more than four decimals, returns `None`, as does arithmetic overflow. Pure.

### `pub fn new(desc: RigDescription) -> Result<MdsEngine, RigError>` [@ANCHOR: radioctl:radioctl_mds_engine_new]

Builds the client engine for an MDS data radio. `desc` is owned and kept. The dialect must be `mds`, else `Unsupported` (also for a non-vendor description). The engine starts not waiting for an answer, with an empty buffer. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_mds_engine_encode]

Builds the command for an operation. `op` is borrowed. `GetFrequency(A)` sends `TX` and LF and waits for a frequency observation (the radio answers with its transmit frequency). `SetFrequency(A, hz)` sends `TX<MHz>` and `RX<MHz>` as two LF terminated lines with the same value, expects no answer (`ReplyKind::None`) and clears any wait; it sets the frequency the radio will transmit on but does not key it. Every other operation, other VFOs, and operations without a capability in the description return `Unsupported`. No range check on the frequency. The receive buffer is cleared.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_mds_engine_feed]

Decodes the radio's CR terminated lines. A partial line older than 500 ms is returned as `Inbound::Unrecognised`. Each line clears the waiting flag; if a frequency request was outstanding and the line parses with `parse_mhz` it becomes a VFO A frequency observation, otherwise the raw bytes are returned as `Unrecognised`. Lines that arrive when nothing is awaited are `Unrecognised`.

### `pub fn new(desc: &RigDescription) -> Result<MdsTwin, RigError>` [@ANCHOR: radioctl:radioctl_mds_twin_new]

Builds a virtual MDS radio for tests. `desc` is borrowed; the dialect must be `mds` or `Unsupported` is returned. Both the transmit and receive frequencies start at the description's default frequency in hertz. No I/O.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_mds_twin_snapshot]

Reports the twin's state for oracle comparison. The result is always `Some`: `frequency` is the receive frequency in hertz (the transmit frequency is not included), `mode_code` is `None` because the radio has no mode setting in this protocol, and `ptt` is always false because the twin models no keying. Borrows immutably, no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_mds_twin_feed]

Consumes controller bytes (LF terminated lines) and returns replies, each ended by CR. `TX` alone answers the transmit frequency from `mhz_text`; `MODEL` answers `MDS 4710`, `SER` answers `000000`, `SREV` answers `1.0` and `SHOW DC` answers `DC OFF`. `TX<MHz>` and `RX<MHz>` set the transmit or receive frequency when `parse_mhz` accepts the text and produce no reply; unparseable or unknown lines are ignored. Mutates the twin; no I/O.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_mds_twin_front_panel]

Simulates a front-panel frequency change. `panel` is owned. `Panel::Frequency(hz)` sets both transmit and receive frequency; any other control returns `Err("an MDS twin has no <panel> knob")`. Success gives an empty vector. No range check; nothing is transmitted.

## `crates/radioctl-protocols/src/xg3.rs`

### `pub fn frequency_text(hz: Hz) -> String` [@ANCHOR: radioctl:radioctl_xg3_frequency_text]

Formats the frequency argument of the XG3 `F` command: the value in hertz zero padded to eleven digits (14,074,000 Hz gives `00014074000`). `hz` is a `Hz` in hertz; a value wider than eleven digits would print longer, so callers range check first (the engine does, 1.5 to 1400 MHz). Pure, cannot fail.

### `pub fn decode(text: &str) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_xg3_decode]

Interprets one answer from the XG3 signal source, given without its terminating `;`. The text must be a letter, a comma and a value. `F,<11 digits>` is a VFO A frequency in hertz; `O,<2 digits>` is the output state (`00` off, anything else on, reported as PTT true); `V,<major>.<minor>` is the firmware version as two integers. Wrong lengths, non-digits, an unknown letter or a missing comma return `None`. Pure; used by the engine's `feed`.

### `pub fn new(desc: RigDescription) -> Result<Xg3Engine, RigError>` [@ANCHOR: radioctl:radioctl_xg3_engine_new]

Builds the client engine for an XG3 signal source. `desc` is owned and kept. The dialect must be `xg3`, else `Unsupported` (also for a non-vendor description). The buffer starts empty. No I/O; the engine has no open sequence of its own.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_xg3_engine_encode]

Builds a `;` terminated command. `op` is borrowed. `GetFrequency(A)` sends `F;`, `GetPtt` sends `O;` and `GetFirmware` sends `V;`, each expecting an observation. `SetFrequency(A, hz)` sends `F,` plus eleven digits and must be 1,500,000 to 1,400,000,000 Hz inclusive, otherwise `Unsupported`. `SetPtt(on)` sends `O,01;` or `O,00;`, switching the RF (radio frequency) output on or off (the radio's equivalent of keying: a signal is produced) and expecting no answer. Other operations or missing capabilities return `Unsupported`. The actor applies the transmit gate before the output-on command; the engine only builds bytes. The receive buffer is cleared.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_xg3_engine_feed]

Splits received bytes at `;` and decodes each piece. A stale partial answer (over 500 ms) is returned as `Inbound::Unrecognised`. Each piece is trimmed; an empty one (a bare `;`, the echo of a terminator) is skipped. A piece that `decode` understands becomes an observation, any other is returned as `Unrecognised` with its raw bytes. The engine does not track what was asked, so an answer is accepted whenever it arrives.

### `pub fn new(desc: &RigDescription) -> Result<Xg3Twin, RigError>` [@ANCHOR: radioctl:radioctl_xg3_twin_new]

Builds a virtual XG3 for tests. `desc` is borrowed; the dialect must be `xg3` or `Unsupported` is returned. The frequency starts at the description's default in hertz. The output starts off (the real unit switches it on at power-up, manual page 22; every twin starts unkeyed and the unkey that radioctld sends at start-up would switch the real one off). Level, memory and backlight start at 0 and the twelve memories at 1.5 MHz. No I/O.

### `fn command(&mut self, text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_xg3_twin_command]

Executes one command of the virtual XG3, split at commas and spaces; the letter is case insensitive. A command with no argument answers, one with an argument sets and answers nothing. `F` sets or returns the frequency (11 digits, clamped to 1.5 to 1400 MHz); `O` sets or returns the output (0 off, other on); `L` and `G` set or return level and backlight (value 0 to 3, others ignored); `C` selects memory 0 to 11 and loads its frequency; `M,<slot>` returns a memory as `M,<slot>,<11 digits>` and `M,<slot>,<hz>` stores one (slot 0 to 11, clamped); `V` answers `V,1.00`; `I` answers an information line. Unparseable numbers and out-of-range values are ignored without a reply. Mutates the twin only.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_xg3_twin_snapshot]

Reports the twin's state for the Hamlib oracle batch. The result is always `Some`: `frequency` is the current output frequency in hertz, `ptt` carries the output switch (true when the modelled RF output is on) and `mode_code` is `None` because the XG3 has no modes. Borrows immutably, no side effects, cannot fail.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_xg3_twin_feed]

Consumes controller bytes and returns replies. A command ends at `;` or CR (CR is converted to `;` before buffering). Bytes enter the receive buffer with the monotonic time `now` (a stale partial command is dropped at 500 ms). Each complete trimmed non-empty piece is run through `command` and its reply bytes are appended. Unknown commands are ignored without reply.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_xg3_twin_front_panel]

Simulates a knob on the XG3. `panel` is owned. `Panel::Frequency(hz)` sets the frequency clamped to 1.5 to 1400 MHz; `Panel::Ptt(on)` switches the modelled output. Other controls return `Err("an XG3 twin has no <panel> knob")`. Empty vector on success. Nothing is transmitted.

## `crates/radioctl-protocols/src/transfox.rs`

### `pub fn decode(text: &str) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_transfox_decode]

Interprets one answer from a Transfox transceiver, given without its `;`. `FA` followed by exactly eleven decimal digits is a VFO A frequency in hertz (a decimal that does not parse, or a different digit count, gives `None`); `TX` is PTT true and `RX` is PTT false. Any other text returns `None`. Pure; used by the engine's `feed` to turn lines into observations.

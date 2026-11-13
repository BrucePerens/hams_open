# radioctl function anchors, batch 10

Function anchors for the PRM80 engine and twin, the Ten-Tec helpers, the vendor-protocol name table, the shared vendor serial plumbing (`RxBuffer`, `parse_decimal`, `vendor_of`, `engine_for`, `twin_for`), and the Barrett and CODAN engines and twins, written to the ADR 0113 standard. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
The Barrett and CODAN engines can encode `SetPtt` (`XP1` and `XP0`; `connect tcvr rf ptt on` and `off`), and the Barrett engine can write the transmit frequency; `engine_for` can return such an engine. The engines only check that the description lists the capability; the lock, band check and transmit gate are applied by the caller that owns the engine (unknown from these files; check `radioctld`), never by an engine or twin.

## `crates/radioctl-protocols/src/prm80.rs`

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_prm80_engine_feed]

Processes bytes received from the PRM8060 at `now`. A stale partial frame is discarded and reported `Unrecognised`. A `>` prompt left over from the previous command may arrive first: while one appears among the first 22 buffered bytes, the bytes up to and including it are dropped. With a read pending and 22 bytes available, they are taken as the `E` state answer and decoded with `decode_state`; the pending field then yields the receive frequency (VFO (variable frequency oscillator, the tuning control) A), the transmit frequency (VFO B), the PTT (push-to-talk) flag or the raw signal strength as an observation. A line that does not decode is `Unrecognised`. No answer in the buffer produces nothing. No I/O.

### `pub fn new(desc: &RigDescription) -> Result<Prm80Twin, RigError>` [@ANCHOR: radioctl:radioctl_prm80_twin_new]

Builds the virtual PRM8060 from a description (borrowed): receive and transmit frequency both at the description's default, PTT off and signal strength 0. It returns `Unsupported` if the description is not a serial-vendor one. No I/O.

### `pub fn state_line(&self) -> String` [@ANCHOR: radioctl:radioctl_prm80_twin_state_line]

Builds the twin's 22-character answer to `E`: eleven bytes as uppercase hex. The mode byte is 0x0A while transmitting and 0x02 otherwise, followed by fixed channel state, squelch and volume bytes (0, 0, 0x0F, 0x08, 0), the receive PLL word from `pll_from_rx` (0 if the frequency does not fit), the transmit PLL word (the frequency divided by 12.5 kHz, 0 if it does not fit) and the signal strength byte. Reads only the twin; it does not append the prompt.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_prm80_twin_feed]

The virtual radio's input. It takes all bytes received at `now` and, for each `E` byte, outputs the 22-character state line followed by the `>` prompt. Any other byte is ignored, since the setters are not modelled. Returns the bytes to put on the simulated wire; no I/O; the twin's state is unchanged.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_prm80_twin_front_panel]

Applies a scripted front-panel action to the twin: `Panel::Frequency(hz)` sets both the receive and transmit frequency in hertz, `Panel::Ptt(on)` sets the transmit flag, and `Panel::Signal(s)` sets the raw signal-strength byte (values above 255 saturate). Anything else returns an `Err` naming the knob a PRM8060 lacks. An empty vector is returned on success. Only the in-memory twin changes.

## `crates/radioctl-protocols/src/tentec.rs`

### `fn write_reply(&self) -> ReplyKind` [@ANCHOR: radioctl:radioctl_tentec_write_reply]

Tells the engine what to expect after a setting command on this dialect: `ReplyKind::Ack` for the Argonaut V, which answers writes with `G`, and `ReplyKind::None` for the Orion, Jupiter and Omni VII, which stay silent. It reads only the dialect and changes nothing. Setting commands in `encode` use it so the caller knows whether to wait for a reply.

### `fn slot(letter: u8) -> Option<usize>` [@ANCHOR: radioctl:radioctl_tentec_twin_slot]

Maps a VFO letter in a Ten-Tec Orion command to the index of the twin's frequency and mode arrays: `A` is 0 and `B` is 1. Any other byte returns `None`, and the twin then answers the command with the refusal `Z!`. An associated function with no state; pure.

## `crates/radioctl-core/src/description.rs`

### `pub fn from_name(name: &str) -> Option<VendorProtocol>` [@ANCHOR: radioctl:radioctl_core_vendorprotocol_from_name]

Maps the `vendor.protocol` name of a rig description file (for example `tentec`, `drake`, `alinco`, `racal`, `barrett`, `codan`, `prm80`) to a `VendorProtocol` variant; the table covers every variant. The match is exact and case sensitive and an unknown name returns `None`, which the rig-file loader reports as a bad description. The input is borrowed. Pure; the inverse of `name`.

### `pub fn name(self) -> &'static str` [@ANCHOR: radioctl:radioctl_core_vendorprotocol_name]

Returns the file-format name of a `VendorProtocol`, as written in a description's `vendor.protocol` field and accepted by `from_name`. The match is exhaustive, so it cannot fail. Takes the value by copy and returns a static string. Pure.

## `crates/radioctl-protocols/src/serial_vendor.rs`

### `pub fn push(&mut self, bytes: &[u8], now: Duration) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_vendor_rxbuffer_push]

Appends received bytes to the `RxBuffer` and applies the stale-partial rule. If the buffer already held bytes and the last append was more than `PARTIAL_FRAME_TIMEOUT` (500 ms) before `now`, the old bytes are taken out and returned as `Some(stale)`, so the engine can report them as unrecognised, and the new bytes start a fresh frame. The last-arrival time is then set to `now`. `now` is a monotonic time supplied by the caller, and a time earlier than the last one is treated as no elapsed time. Mutates the buffer; no I/O.

### `pub fn take_line(&mut self, terminator: u8) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_vendor_rxbuffer_take_line]

Removes and returns the first complete line from the buffer: everything before the first `terminator` byte, with the terminator removed from the output but also consumed from the buffer. Returns `None` and leaves the buffer untouched if no terminator has arrived. Callers loop on it until it returns `None`. Mutates the buffer.

### `pub fn take(&mut self, n: usize) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_vendor_rxbuffer_take]

Removes and returns exactly the first `n` bytes if at least that many have arrived, for fixed-length answers; otherwise returns `None` and leaves the buffer untouched. `n` of 0 returns an empty vector. Mutates the buffer.

### `pub fn peek(&self) -> &[u8]` [@ANCHOR: radioctl:radioctl_vendor_rxbuffer_peek]

Returns a borrowed view of all the bytes held so far without consuming them, for engines that need to look ahead, such as counting console prompts. Takes `&self`, no side effects, cannot fail.

### `pub fn take_all(&mut self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_vendor_rxbuffer_take_all]

Removes and returns everything held, leaving the buffer empty. Engines call it in `encode` to discard leftover bytes from an earlier exchange before a new request, and twins call it to consume a whole input. It does not reset the last-arrival time. Mutates the buffer.

### `pub fn parse_decimal(text: &[u8]) -> Option<u64>` [@ANCHOR: radioctl:radioctl_vendor_parse_decimal]

Parses a non-empty run of ASCII digits into a `u64`. It returns `None` for an empty slice, more than 18 digits, or any byte that is not 0 to 9 (so no sign, space or decimal point). The 18-digit cap and the checked arithmetic keep the result from overflowing. It is the one numeric parser every vendor engine uses for frequencies and codes. Pure; borrows the bytes.

### `pub fn vendor_of(desc: &RigDescription) -> Result<(VendorProtocol, &str), RigError>` [@ANCHOR: radioctl:radioctl_vendor_vendor_of]

Reads the vendor protocol and the dialect name from a description. It succeeds only when the description's family is `SerialVendor` and it has a vendor block, returning the `VendorProtocol` and a string borrowed from the description. Otherwise it returns an `Unsupported` error `not a vendor serial description`. Every vendor engine and twin constructor starts with this check, and the dialect string chooses the variant. Pure.

### `pub fn engine_for(desc: &RigDescription) -> Result<Box<dyn ProtocolEngine>, RigError>` [@ANCHOR: radioctl:radioctl_vendor_engine_for]

Builds the right client engine for a vendor serial description. It reads the protocol and dialect with `vendor_of` and constructs the matching engine, cloning the description; Ten-Tec `rx320` and `tt550` go to the RX-320 engine and other Ten-Tec dialects to the Ten-Tec engine, and Barrett `barrett4100` to its own engine. It returns the engine's own construction error, for instance an unknown dialect, or `Unsupported` if the description is not a vendor one. Some engines it returns can encode PTT or transmit-frequency commands; each checks capabilities itself and the caller owns the gate. No I/O.

### `pub fn twin_for(desc: &RigDescription) -> Result<Box<dyn VendorTwin>, RigError>` [@ANCHOR: radioctl:radioctl_vendor_twin_for]

Builds the virtual radio (a `VendorTwin`) matching a vendor serial description, with the same selection rules as `engine_for`, so the conformance harness can drive every family the same way. The twin starts in the description's default state. It returns the twin's own construction error or `Unsupported` for a description that is not a vendor one. It borrows the description and does no I/O.

## `crates/radioctl-protocols/src/barrett.rs`

### `pub fn frame(text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_barrett_frame]

Builds the bytes of one Barrett radio answer: XOFF (0x13), the text, CR LF, then XON (0x11). The twin uses it to answer, and the engine's `feed` expects this shape. The text is borrowed and copied into a new vector; pure; cannot fail.

### `pub fn unframe(raw: &[u8]) -> &[u8]` [@ANCHOR: radioctl:radioctl_barrett_unframe]

Extracts the text of a Barrett answer taken up to (not including) its XON: a leading XOFF is dropped, and a trailing LF and then CR are dropped. An answer with no XOFF is accepted as it is, and one with no line end is returned whole. The result borrows from the input. Pure; cannot fail.

### `pub fn band_offset(hz: Hz) -> Option<u32>` [@ANCHOR: radioctl:radioctl_barrett_band_offset]

Gives the index (0 to 9) of the band channel used for a frequency on the channel-tuned Barretts (950 and 4050). The ten band ends are 3.5, 5.3, 7.0, 10.1, 14.0, 18.068, 21.0, 24.89, 28.0 and 30.0 MHz. A frequency exactly on an edge belongs to the upper band, matching Hamlib's loop that keeps the last match. Above 30 MHz it returns `None`, and the engine refuses the tune. Pure.

### `fn from_name(name: &str) -> Option<Tuning>` [@ANCHOR: radioctl:radioctl_barrett_tuning_from_name]

Maps a Barrett dialect name to how the radio is tuned: `barrett` (the 2050) tunes directly with `TR` and `TC`, `barrett950` uses memory channels based at 0 and `barrett4050` uses channels based at 9990. Any other name returns `None`, and the engine and twin constructors turn that into an `Unsupported` error. Private, pure.

### `pub fn new(desc: RigDescription) -> Result<BarrettEngine, RigError>` [@ANCHOR: radioctl:radioctl_barrett_engine_new]

Builds the engine for a Barrett 950, 2050 or 4050 description, taking it by value. It requires a serial-vendor description with a known dialect (`barrett`, `barrett950` or `barrett4050`), otherwise `Unsupported`. The engine starts with no pending reply and an empty record of the channels it has written. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_barrett_engine_encode]

Builds the command text for an operation after refusing (`Unsupported`) any the description lacks. Reads: `IR` and `IT` (receive and transmit frequency), `IB` (mode), `IP` (PTT). Writes: set frequency (a value over eight digits is refused; the 2050 sends `TR<8 digits>` and `TC9999T<8 digits>`, where VFO B sets only the transmit frequency; the 950 and 4050 select a band channel with `XC<4 digits>` and, only if this engine has not already written that frequency there, program it with `PC...`, and refuse VFO B and frequencies above 30 MHz); set mode (`TB<letter>`); set PTT (`XP1` or `XP0`). A write expects one or two `OK` answers. `SetPtt` and the transmit-frequency writes can key or retune the transmitter; the gate is the caller's. It records what is pending and clears the receive buffer.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_barrett_engine_feed]

Processes bytes received at `now`. A stale partial frame is discarded and reported `Unrecognised`. Each answer ends at XON and is unframed. For a write, an `OK` counts down the number of answers expected and gives one `Ack` when the last arrives; any other text is a `Nak`, and the engine forgets what it recorded as written to that channel so it will write it again. Reads: a decimal gives a frequency, the second byte of the answer is the mode letter (checked against the description), and `0` or `1` is the PTT state. Anything else is `Unrecognised`. No I/O.

### `pub fn new(desc: &RigDescription) -> Result<BarrettTwin, RigError>` [@ANCHOR: radioctl:radioctl_barrett_twin_new]

Builds the virtual Barrett from a description (borrowed): the tuning style from the dialect name, receive and transmit frequency at the description's default, the default mode (or the first listed code, or `U`), PTT off, no channel selected and no channel contents. It returns `Unsupported` for a non-vendor description or an unknown dialect. No I/O.

### `fn answer(&mut self, line: &[u8]) -> Option<String>` [@ANCHOR: radioctl:radioctl_barrett_twin_answer]

Works out the answer text for one command line and applies settings. It handles `IR`, `IT`, `IB` (a `0` then the mode letter), `IP`, `IV` (a version text), `TR` and `TC9999T` (2050 only), `TB` (if the letter is a listed code) and `XP0` and `XP1`. For the channel-tuned models it handles `XC` (select, loading the channel's frequencies if programmed), `IDC` (channel, receive and transmit digits, or `E5` for an empty channel) and `PC` (program a channel). Settings answer `OK`. Returns `None` for an unknown or malformed command, which `feed` turns into `ERROR`. Mutates the twin.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_barrett_twin_snapshot]

Reports the twin's observable state for the oracle comparison harness: the receive frequency in hertz, the current mode code and the PTT flag. It always returns `Some`. Reads only the twin; no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_barrett_twin_feed]

The virtual radio's input. It buffers bytes received at `now` and, for each CR-terminated command, asks `answer` and sends the text framed with XOFF and XON (`frame`); an unknown command is answered with the text `ERROR`. An empty line, such as the doubled CR that Hamlib sends after a mode write, is skipped without an answer. Returns the bytes to send; mutates the twin; no I/O.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_barrett_twin_front_panel]

Applies a scripted front-panel action to the twin: `Panel::Frequency(hz)` sets both the receive and transmit frequency in hertz, and `Panel::Ptt(on)` sets the transmit flag. Anything else returns an `Err` naming the knob a Barrett lacks. An empty vector is returned on success, because the radio sends nothing unprompted. Only the in-memory twin changes.

## `crates/radioctl-protocols/src/codan.rs`

### `pub fn khz_text(hz: Hz) -> String` [@ANCHOR: radioctl:radioctl_codan_khz_text]

Formats a frequency as kilohertz text the way the CODAN console prints it: the whole kilohertz and, when there is a remainder, a point and up to three decimals with trailing zeros removed (14,074,000 Hz is `14074`, 14,074,500 Hz is `14074.5`). Integer arithmetic only, so exact to the hertz. Returns a new string; pure; cannot fail.

### `pub fn parse_khz(text: &str) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_codan_parse_khz]

Parses a kilohertz number with up to three decimals into integer hertz, after trimming blanks. `14074.5` is 14,074,500 Hz. It returns `None` for an empty whole part, more than three decimals, a non-numeric part or overflow. Pure; the inverse of `khz_text`.

### `pub fn new(desc: RigDescription) -> Result<CodanEngine, RigError>` [@ANCHOR: radioctl:radioctl_codan_engine_new]

Builds the engine for a CODAN Envoy or NGT description, taking it by value. It requires a serial-vendor description whose dialect is `codan`, otherwise `Unsupported`. It starts with nothing pending and no second line to drop. It does not log in or set up the console (`echo off`, the `admin` login and the free-transmit option), so the console must already be configured. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_codan_engine_encode]

Builds the console command for an operation after refusing (`Unsupported`) any the description lacks. Get frequency is `freq`, get mode `mode`, get PTT `connect tcvr rf ptt`; set frequency is `connect tcvr rf <Hz> <Hz>` (receive and transmit equal) plus `freq`, set mode is `mode USBW` or `mode LSBW` (only the description codes `U` and `L` are accepted), and set PTT is `connect tcvr rf ptt on` or `off` plus `ptt`. Frequency and PTT sets arrange to drop the second answer line. VFO B and everything else is `Unsupported`. `SetPtt` keys the transmitter and the gate is the caller's. It clears the receive buffer.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_codan_engine_feed]

Processes bytes received at `now`. A stale partial frame is discarded and reported `Unrecognised`. Each non-empty LF-ended line is trimmed and matched to the pending request. After a set, the first line is the `Ack`, whatever it says, and the second line that frequency and PTT sets cause is dropped. A frequency read needs `FREQ: <kHz>`; a mode read needs `MODE: <name>, ...` whose first name starts `USB` or `LSB` and is known to the description; a PTT read gives true unless the text from `Ptt` on is exactly `Ptt=Off`. Anything else is `Unrecognised`. No I/O.

# radioctl function anchors, batch 14

Function anchors for the `radioctl-protocols` crate (HiQSDR, Barrett 4100, WJ-8888 and AnyTone D578A vendor families), written to the ADR 0113 standard. Each anchor names a function in the crate source and is exercised by a test that cites it with `Tests [@ANCHOR: ...]`. Engines only convert operations to bytes and bytes to observations; the transmit lock, band check and transmit gate are applied by the actor in `radioctl-core` that calls them, not here. Entries that can put a key-down on the wire (Barrett `SRPTT1`) say so. Twins are virtual radios for tests and never touch hardware.

## `crates/radioctl-protocols/src/hiqsdr.rs`

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_hiqsdr_engine_feed]

Handles bytes that arrive from a HiQSDR. The radio sends nothing back on its control port, so any data is unexpected: an empty `bytes` slice gives an empty vector, and any non-empty slice is returned as one `Inbound::Unrecognised` holding a copy of the bytes. The `now` argument is unused because nothing is buffered, so no partial-frame timeout applies. No state changes, no errors, never blocks.

### `pub fn new(desc: &RigDescription) -> Result<HiqsdrTwin, RigError>` [@ANCHOR: radioctl:radioctl_hiqsdr_twin_new]

Builds a virtual HiQSDR for tests. `desc` is borrowed; the dialect must be `hiqsdr` or `Unsupported` is returned. The twin starts with the 22 byte `initial_frame()` (zero tuning, level 120, not keyed) and remembers the description's default frequency, which it reports until the first real tuning frame arrives. No I/O.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_hiqsdr_twin_snapshot]

Reports the twin's state for the Hamlib oracle batch. Always `Some`. `frequency` is the receive phase word of the last accepted frame converted back to hertz with `word_frequency`; until a frame with a non-zero receive word has been seen it is the description's default frequency, so a freshly opened twin does not report 0 Hz. `mode_code` is the mode byte reduced to its CW bit (0x01) and other bit (0x02); `ptt` is true when the keyed bit 0x08 is set. Immutable, no side effects.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_hiqsdr_twin_feed]

Receives one control datagram and keeps it. The radio treats one datagram as one frame: only a slice of exactly 22 bytes that starts with `St` is accepted and stored as the new control frame; anything else is ignored, as a real radio ignores junk. The twin also remembers that it has been tuned once a frame with a non-zero receive tuning word arrives (the flag stays set). It never replies, so the result is always an empty vector. The time argument is unused. No I/O.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_hiqsdr_twin_front_panel]

Always refuses. The HiQSDR is controlled only over its network port and has no front panel, so every `panel` returns `Err("a HiQSDR twin has no front panel: <panel>")` and the twin is unchanged. It exists to satisfy the `VendorTwin` trait.

## `crates/radioctl-protocols/src/barrett4100.rs`

### `pub fn command(text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_barrett4100_command]

Wraps a command string for the Barrett 4100 serial line: a line feed (0x0A), the ASCII text, then a carriage return (0x0D). `text` is borrowed and not validated; the caller supplies the radio's command words, for example `M:FF GRF`. Returns a new vector, pure, cannot fail.

### `pub fn new(desc: RigDescription) -> Result<Barrett4100Engine, RigError>` [@ANCHOR: radioctl:radioctl_barrett4100_engine_new]

Builds the client engine for a Barrett 4100 HF transceiver. `desc` is owned and kept. The dialect must be `barrett4100`, otherwise (or for a non-vendor description) `Unsupported`. The engine starts not yet in remote mode (`entered` false), with an empty buffer and an empty queue of expected answers. No I/O; the remote-mode command is sent with the first operation.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_barrett4100_engine_encode]

Builds the bytes for an operation and queues the answers that are expected. `op` is borrowed. `GetFrequency(A)` sends `M:FF GRF` and expects a frequency observation. `SetFrequency(A, hz)` must be 10 kHz to 30 MHz inclusive or `Unsupported` is returned; it sends two commands, set receive frequency (`SRF<hz> GRF`) and set transmit frequency (`STF<hz> GTF`), and expects two acknowledgements, so the radio is tuned for both receive and transmit. `SetPtt(on)` sends `M:FF SRPTT1 GRPTT` to key or `SRPTT0 GRPTT` to unkey and expects one acknowledgement; keying puts the transmitter on air, so the actor applies the transmit gate and the fail-closed unkey around it. Other operations or missing capabilities return `Unsupported`. The first command ever encoded is preceded by the remote-mode command `M:REMOTE SENTER2,1`, once, and the engine remembers it has done so (and it sets this state even if the radio later fails to answer).

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_barrett4100_engine_feed]

Matches the radio's answers, which are framed by XOFF at the start and XON (0x11) at the end, to the expectations queued by `encode`. A partial answer older than 500 ms is returned as `Inbound::Unrecognised`. For each complete answer the framing is removed, the text trimmed and the oldest expectation consumed. A session answer (reply to the remote-mode command) produces nothing. For a write expectation of n answers: text starting with `s` counts down and the last gives `Inbound::Ack`; any other text gives `Inbound::Nak` immediately and the remaining owed answers of that operation are dropped. For a frequency expectation `gRF<digits>` gives a VFO A frequency observation in hertz, else `Unrecognised`. An answer with nothing expected is `Unrecognised`.

### `fn link_restored(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_barrett4100_engine_link_restored]

Called by the actor after the serial link came back from a loss. The radio leaves remote mode when the link drops, so the engine discards any half-received answer and all expectations. If remote mode had ever been entered it returns one request, the remote-mode command `M:REMOTE SENTER2,1`, expects an answer for it (consumed silently) and the actor writes it once; if nothing had been sent before it returns an empty vector. No keying command is sent. Idempotent in the sense that calling it twice queues a second session expectation only if a second request is written.

### `pub fn new(desc: &RigDescription) -> Result<Barrett4100Twin, RigError>` [@ANCHOR: radioctl:radioctl_barrett4100_twin_new]

Builds a virtual Barrett 4100 for tests. `desc` is borrowed; the dialect must be `barrett4100` or `Unsupported` is returned. Receive and transmit frequencies start at the description's default in hertz, the radio is unkeyed and not in remote mode. No I/O.

### `fn answer(&mut self, text: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_barrett4100_twin_answer]

Builds the answer text for one command line (already stripped of the leading line feed), or `None` when the radio says nothing. `M:REMOTE SENTER2,1` enters remote mode and answers `s`; `M:REMOTE SENTER0` leaves it and answers `s`. Every other command must start with `M:FF ` and is ignored (no answer) while the twin is not in remote mode. `GRF` answers `gRF<receive hz>`; `GRPTT` answers `gRPTT0` or `gRPTT1`; `SRF<hz> GRF` sets the receive frequency and answers `s gRF<hz>`; `STF<hz> GTF` sets the transmit frequency and answers `s gTF<hz>`; `SRPTT<0|1> GRPTT` sets the modelled key state (non-zero keys) and answers `s gRPTT<state>`. Anything malformed returns `None`. Mutates the twin only.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_barrett4100_twin_snapshot]

Reports the twin's state for oracle comparison. Always `Some`: `frequency` is the receive frequency in hertz, `ptt` is the modelled key state and `mode_code` is `None` because this protocol carries no mode. The transmit frequency is not included. Immutable, no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_barrett4100_twin_feed]

Consumes controller bytes and returns replies. Bytes are buffered with monotonic time `now` (a partial command older than 500 ms is dropped). Each command ends in CR; a leading line feed is removed. The text goes to `answer`; when it returns an answer the answer is wrapped in the XOFF/XON frame and appended to the output, in order. Commands with no answer produce no bytes.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_barrett4100_twin_front_panel]

Simulates a knob on the radio. `panel` is owned. `Panel::Frequency(hz)` sets both receive and transmit frequency; `Panel::Ptt(on)` sets the modelled key state. Other controls return `Err("a Barrett 4100 twin has no <panel> knob")`. Success returns an empty vector. No range check. Nothing is transmitted.

## `crates/radioctl-protocols/src/wj.rs`

### `pub fn mode_code(mode: Mode) -> Option<u8>` [@ANCHOR: radioctl:radioctl_wj_mode_code]

Maps a shared `Mode` to the three-bit mode field of a WJ-8888 command frame: AM 0, FM 1, CW 2, LSB 5, USB 6. `mode` is passed by value. Any other mode (for example RTTY) returns `None`, which the engine reports as `Unsupported`. Pure.

### `pub fn bcd_digits(hz: Hz) -> [u8; 4]` [@ANCHOR: radioctl:radioctl_wj_bcd_digits]

Reproduces Hamlib's BCD routine for the WJ-8888 frequency. `hz` is in hertz and is divided by 10 (units of 10 Hz, anything below is dropped); the result is seven decimal digits, most significant first. The first six digits are packed two to a byte in the first three bytes, and the seventh sits in the high nibble of the fourth byte, whose low nibble is zero. A value of 100 MHz or more loses its high digits (only seven digits are kept). Pure, cannot fail.

### `pub fn command_frame(hz: Hz, mode: u8, width: u8) -> [u8; FRAME_LEN]` [@ANCHOR: radioctl:radioctl_wj_command_frame]

Builds the ten byte command frame the WJ-8888 receiver takes. `hz` is the frequency in hertz, `mode` is the three-bit mode code from `mode_code` and `width` is the three-bit IF bandwidth code (only the low three bits of each are used). Byte 0 is 0x48; bytes 1 to 4 carry the BCD digits of the frequency shifted into the frame's 6-bit groups (so the lowest bit of the 100 Hz digit is lost); byte 5 holds width and mode; bytes 6 to 8 carry the fixed BFO (no shift, 0x400) and full RF gain (0x7f), and the AGC (automatic gain control) is slow. Byte 9 is zero. Returns a fixed array; pure. This follows Hamlib's captured frames and is not verified on a radio.

### `pub fn decode_frame(buf: &[u8]) -> Option<(Hz, u8)>` [@ANCHOR: radioctl:radioctl_wj_decode_frame]

The inverse of `command_frame`, for the twin and the tests. `buf` must be exactly ten bytes and byte 0 must have bit 0x40 set (a command frame); otherwise `None`. It rebuilds the seven BCD digits from the packed bytes and returns the frequency in hertz (units of 10 Hz times 10), and the three-bit mode code from byte 5. The bit that `command_frame` loses from the 100 Hz digit reads as zero, so the decoded frequency can differ from the encoded one in that digit. Bytes beyond those it reads are ignored. Pure.

### `pub fn new(desc: RigDescription) -> Result<WjEngine, RigError>` [@ANCHOR: radioctl:radioctl_wj_engine_new]

Builds the client engine for a WJ-8888 receiver. `desc` is owned and kept. The dialect must be `wj8888`, else `Unsupported` (also for a non-vendor description). Like Hamlib the engine starts at 500 kHz, AM (mode 0), IF width code 4 (8 kHz). No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_wj_engine_encode]

Updates the engine's frequency or mode and returns the whole ten byte command frame as the request (no reply expected). `op` is borrowed. `SetFrequency(A, hz)` must be 5 kHz to 30 MHz inclusive or `Unsupported` is returned. `SetMode` must have a code in `mode_code` else `Unsupported`, and it also sets the width code to 2 (the width Hamlib's wire shows after a mode write). Other operations (there are no reads and no PTT; this is a receiver) or missing capabilities return `Unsupported`. State is changed only on success.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_wj_engine_feed]

Handles bytes from the receiver. Only the receiver's monitor frames answer and this engine sends no request that would produce one, so anything received is unexpected: an empty slice gives an empty vector, any other slice is returned as one `Inbound::Unrecognised` holding a copy. The time argument is unused as nothing is buffered. No state changes, no errors.

### `pub fn new(desc: &RigDescription) -> Result<WjTwin, RigError>` [@ANCHOR: radioctl:radioctl_wj_twin_new]

Builds a virtual WJ-8888 for tests. `desc` is borrowed; the dialect must be `wj8888` or `Unsupported` is returned. The frequency starts at the description's default in hertz and the mode at the default mode's three-bit code (0, AM, if the description has none). No I/O.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_wj_twin_snapshot]

Reports the twin's state for oracle comparison. Always `Some`: `frequency` is the held frequency in hertz and `mode_code` is the three-bit mode value (0 to 7), not an ASCII letter. `ptt` is always false because this is a receiver. Immutable, no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_wj_twin_feed]

Consumes controller bytes. They are buffered with monotonic time `now` (a partial frame older than 500 ms is dropped) and whenever ten bytes are held they are taken as a frame and decoded with `decode_frame`; a valid command frame sets the twin's frequency and mode, an invalid one is discarded. The twin never answers, so the result is always empty. Several frames in one write are applied in order.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_wj_twin_front_panel]

Simulates a knob on the receiver. `panel` is owned. `Panel::Frequency(hz)` sets the frequency; `Panel::Mode(mode)` sets the mode code through `mode_code` and returns `Err("no <mode> mode")` if the mode has none. Any other control returns `Err("a WJ twin has no <panel> knob")`. Success returns an empty vector. No range check on the frequency.

## `crates/radioctl-protocols/src/anytone.rs`

### `pub fn open_records() -> Vec<u8>` [@ANCHOR: radioctl:radioctl_anytone_open_records]

Returns the two records the engine sends when the link opens: the mode announcement `+ADATA:00,016` followed by the 16 data bytes `\x01D578UV COM MODE` and CR LF, then the `+ADATA:00,000` close record. The records are fixed text with no arguments. They only announce a PC control session; no key-down record is among them. Returns a new vector, pure.

### `pub fn read_frequency_record(vfo: Vfo) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_anytone_read_frequency_record]

Builds the 25 byte record that asks the radio for the frequency of a VFO: the header `+ADATA:00,006` and CR LF, then the data bytes `04`, `2C` (VFO A) or `2D` (VFO B), `07`, five zero bytes and CR LF. `vfo` is passed by value; anything other than B is treated as A. Pure, cannot fail.

### `pub fn answer_frequency(answer: &[u8]) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_anytone_answer_frequency]

Reads the frequency out of a radio answer. `answer` must be exactly 138 bytes and start with `+ADATA:`; otherwise `None`. Four bytes at offset 17 hold eight packed BCD digits, most significant first, in units of 10 Hz; if any nibble is above 9 the result is `None`. Otherwise the digits times 10 give hertz. The rest of the answer is ignored. Pure.

### `pub fn frequency_answer(vfo: Vfo, hz: Hz) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_anytone_frequency_answer]

Builds a 138 byte answer the way the virtual radio sends it: header `+ADATA:00,121` and CR LF, the bytes `04` and `2C` (VFO A) or `2D` (VFO B), four packed BCD bytes of `hz / 10` (eight digits, so values of 1 GHz or more lose their top digit), zero padding, and a final CR LF at the end. The payload length is a guess (documented in the source), because the real radio's answer is only known to have this length. `hz` is in hertz. Returns a new vector; pure.

### `pub fn new(desc: RigDescription) -> Result<AnytoneEngine, RigError>` [@ANCHOR: radioctl:radioctl_anytone_engine_new]

Builds the client engine for an AnyTone D578A. `desc` is owned and kept. The dialect must be `d578a`, else `Unsupported` (also for a non-vendor description). The engine starts with an empty buffer and no request outstanding. No I/O; the open records come from `open_requests`.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_anytone_engine_encode]

Encodes the only operation the engine supports, the frequency read of VFO A or B. `op` is borrowed. `GetFrequency(vfo)` records which VFO was asked (replacing an earlier unanswered one) and returns the 25 byte read record with a frequency observation expected. Every other operation, or a missing capability, returns `Unsupported`; the engine never sends a transmit or frequency-set command, so it cannot key the radio.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_anytone_engine_feed]

Collects 138 byte answers. Bytes are buffered with monotonic time `now`; a partial answer older than 500 ms is returned as `Inbound::Unrecognised` and dropped. In a loop: an echo of the `+ADATA:00,000` record at the front is removed and skipped; while the held bytes could still be the start of that echo, it waits for more; otherwise, when 138 bytes are available they are taken. If a read was asked and `answer_frequency` accepts the answer, a frequency observation for that VFO is returned and the request is cleared; any other 138 byte block (including one when nothing was asked) is returned as `Unrecognised`.

### `fn open_requests(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_anytone_engine_open_requests]

Returns the requests that make a freshly opened D578A ready to talk: a single request with the bytes of `open_records()` and no reply expected. The actor writes it once when it starts with the link up, before serving a command. It does not contain the record that may key the radio, so it is safe to send at any time. Takes `&mut self` as the trait requires but does not change state.

### `fn link_restored(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_anytone_engine_link_restored]

Called after the link came back from a loss. Discards any half-received answer from the buffer, forgets the outstanding frequency read, and returns the same open dialogue as `open_requests`, so the radio is put back in PC control mode. The actor writes it once after its fail-closed unkey. Nothing keys the transmitter.

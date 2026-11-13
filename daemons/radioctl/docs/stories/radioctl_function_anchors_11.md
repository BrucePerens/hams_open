# radioctl function anchors, batch 11

Function anchors for the `radioctl-protocols` crate (CODAN, Dorji, Guohe and Kachina vendor serial families), written to the ADR 0113 standard. Each anchor names a function in the crate source and is exercised by a test that cites it with `Tests [@ANCHOR: ...]`. Engines here only turn an operation into bytes and bytes into observations; the transmit lock, band check and transmit gate are applied by the actor that calls the engine (in `radioctl-core`), not by this code. Entries for `SetPtt`, `SetFrequency` and similar say what is put on the wire. Twins are virtual radios used by tests and never touch hardware.

## `crates/radioctl-protocols/src/codan.rs`

### `pub fn new(desc: &RigDescription) -> Result<CodanTwin, RigError>` [@ANCHOR: radioctl:radioctl_codan_twin_new]

Builds a virtual CODAN radio (the "twin") in the state the rig description names. It is called by the twin factory and by tests. `desc` is borrowed and read only: its vendor dialect must be `codan`, its `default_frequency` (hertz) becomes both the receive and transmit frequency, and its `default_mode` is turned into a mode code through `mode_code` (falling back to `U`, upper sideband, if the description has none). PTT (push-to-talk) starts off and the receive buffer starts empty. Returns `RigError` of kind `Unsupported` if the description is not a vendor serial one (from `vendor_of`) or the dialect is not `codan`. Pure construction: no I/O, no clock.

### `fn answer(&mut self, line: &str) -> String` [@ANCHOR: radioctl:radioctl_codan_twin_answer]

Produces the console reply for one trimmed command line, the way the CICS text console of a CODAN radio answers (facts taken from Hamlib 4.6.2, nothing verified on a radio). `line` is borrowed and compared whole. `freq` answers `FREQ: <kHz>` from the receive frequency; `mode` answers `MODE: <name>, <name>, 1500, 2400` with LSB (lower sideband) or USB; `ptt` (or `connect tcvr rf ptt`) answers `Ptt=On` or `Ptt=Off`. Setters change the twin and answer `OK`: `mode USBW`, `mode LSBW`, `connect tcvr rf ptt on` and `off`, and `connect tcvr rf <rx> <tx>` (two integer hertz values; if either does not parse the answer is `ERROR`). The console set-up lines `echo off`, `ver`, `login` and `login admin ''` answer `OK`; every other line answers `ERROR`. Mutates the twin, never blocks, no I/O. The PTT state it holds is only a model: nothing is keyed.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_codan_twin_snapshot]

Reports what the twin currently holds so a test can compare it with a Hamlib oracle or with the engine's cache. The result is always `Some` for this twin: `frequency` is the receive frequency in hertz, `mode_code` is the one-byte description code (`U` or `L`) and `ptt` is the modelled key state. The transmit frequency is not reported. Borrows the twin immutably, has no side effects and cannot fail.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_codan_twin_feed]

Consumes bytes the controller wrote and returns the bytes the radio sends back. `bytes` are appended to the twin's `RxBuffer` with the caller's monotonic time `now` (a partial command older than the buffer timeout, 500 ms, is dropped by the buffer). Every complete command line ends in LF; Hamlib joins two commands in one write with a CR between them, so each LF-terminated chunk is split at CR, trimmed, empty pieces are skipped, and each remaining line goes through `answer`. Each answer is returned followed by CR LF, in order, so two commands in one write give two answer lines. Incomplete input yields an empty vector and is held for the next call. Not thread safe by itself (needs `&mut`); never blocks.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_codan_twin_front_panel]

Simulates an operator turning a knob on the radio itself, so tests can check that the controller notices a change it did not make. `panel` is owned. `Panel::Frequency(hz)` sets both the receive and transmit frequency to `hz`; `Panel::Ptt(on)` sets the modelled key state. Any other panel control returns `Err` with the text `a CODAN twin has no <panel> knob`. On success the result is an empty byte vector, because this radio sends nothing unprompted. No range check is made on the frequency. Nothing is transmitted: the PTT value is a flag in the model.

## `crates/radioctl-protocols/src/dorji.rs`

### `pub fn from_name(name: &str) -> Option<Band>` [@ANCHOR: radioctl:radioctl_dorji_band_from_name]

Maps the dialect name of a Dorji module description to its band: `dra818u` gives `Band::Uhf` and `dra818v` gives `Band::Vhf`. The match is exact and case sensitive. Any other text returns `None`, which callers (`DorjiEngine::new`, `DorjiTwin::new`) turn into an `Unsupported` error. Pure, borrowed input, no side effects.

### `pub fn contains(self, hz: u64) -> bool` [@ANCHOR: radioctl:radioctl_dorji_band_contains]

Says whether a frequency is inside the range the module can tune. `hz` is in hertz. UHF accepts 400,000,000 to 480,000,000 inclusive and VHF (very high frequency, 30 to 300 MHz) accepts 134,000,000 to 174,000,000 inclusive. The check is on the value as given, so callers pass the already rounded channel frequency. It is a capability check of the module, not a licence or band-plan check, and it does not itself prevent a transmission; it is used by the engine to refuse out-of-range tuning and by the twin to answer a bad group command with a refusal. Pure.

### `pub fn channel(hz: Hz) -> Hz` [@ANCHOR: radioctl:radioctl_dorji_channel]

Rounds a frequency to the nearest 12.5 kHz channel, the module's tuning step. `hz` is a `Hz` value in hertz (any value); the result is `(hz + 6250) / 12500 * 12500` using integer arithmetic, so exact half steps round up (for example 146,006,250 becomes 146,012,500). It does not range check; callers test the result with `Band::contains`. Cannot fail, and large values near `u64::MAX` would overflow in the addition (unknown: no caller passes such values, check before using it on untrusted input).

### `pub fn mhz_text(hz: Hz) -> String` [@ANCHOR: radioctl:radioctl_dorji_mhz_text]

Formats a frequency the way the DRA818 `AT+DMOSETGROUP` command wants it: megahertz as three digits, a point, and four decimals (units of 100 Hz), for example `146.5200`. `hz` is in hertz; the whole megahertz part is zero padded to three digits and the fraction is the remainder divided by 100, so anything below 100 Hz is truncated. Values of 1000 MHz or more print more than three leading digits. Returns a new `String`, pure, cannot fail.

### `pub fn parse_mhz(text: &str) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_dorji_parse_mhz]

The inverse of `mhz_text`: reads `fff.ffff` megahertz text and returns the frequency in hertz. The text must be split by one `.` into exactly three characters before and four after, and both parts must parse as unsigned integers; otherwise it returns `None` (no point, wrong lengths, signs or letters). The fraction is in units of 100 Hz. No range check is made. Used by the twin to read the frequencies of a set-group command. Pure.

### `pub fn new(desc: RigDescription) -> Result<DorjiEngine, RigError>` [@ANCHOR: radioctl:radioctl_dorji_engine_new]

Builds the client engine for a Dorji DRA818 module from a vendor serial description. `desc` is owned and kept by the engine. The dialect must be `dra818u` or `dra818v`; the band is derived from it, the working frequency starts at the description's default frequency rounded to a channel, the bandwidth starts narrow (12.5 kHz) and no command is outstanding. Returns `Unsupported` if the description is not a vendor serial one or the dialect is unknown. No I/O is done here; the engine has no open sequence of its own.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_dorji_engine_encode]

Turns one operation into the module's `AT+DMOSETGROUP` command and the reply to expect (`ReplyKind::Ack`). `op` is borrowed. The module has only a set command, so only `SetFrequency` on VFO (variable frequency oscillator, the tuning control) A and `SetMode` are supported. A frequency is rounded to a 12.5 kHz channel and must be inside the band, otherwise `Unsupported` ("frequency outside the module's band"); a mode must exist in the description and code `1` selects wide bandwidth, anything else narrow. Every other operation, including `SetPtt` (the module keys from a hardware pin), returns `Unsupported`, as does an operation whose capability the description lacks. The bytes carry the same frequency for transmit and receive, tone codes 0000 and a fixed squelch of 4. On success the engine remembers it is waiting for an answer and clears its receive buffer. Mutates the engine; no I/O; the actor writes the bytes and applies any transmit gate before calling.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_dorji_engine_feed]

Decodes bytes received from the module. `bytes` are pushed with the caller's monotonic `now`; if a partial line older than 500 ms was held it is returned as `Inbound::Unrecognised` and discarded. Complete LF-terminated lines are trimmed and empty ones skipped. While a command is outstanding, `+DMOSETGROUP:0` becomes `Inbound::Ack` and `+DMOSETGROUP:1` becomes `Inbound::Nak`; any other line, or any line when nothing is outstanding, is returned as `Inbound::Unrecognised` with the raw bytes. The waiting flag is cleared by the first line after a request. Returns possibly several items in order; never blocks.

### `pub fn new(desc: &RigDescription) -> Result<DorjiTwin, RigError>` [@ANCHOR: radioctl:radioctl_dorji_twin_new]

Builds a virtual DRA818 module for tests. `desc` is borrowed and read: the dialect (`dra818u` or `dra818v`) chooses the band and the default frequency, rounded to a channel, becomes both the receive and transmit frequency; bandwidth starts narrow. Returns `Unsupported` for a non-vendor description or an unknown dialect. No I/O.

### `fn answer(&mut self, line: &str) -> Option<&'static str>` [@ANCHOR: radioctl:radioctl_dorji_twin_answer]

Answers one trimmed command line the way the module does, or returns `None` for a line it does not know (the twin then sends nothing). `AT+DMOCONNECT` answers `+DMOCONNECT:0`. `AT+DMOSETVOLUME=n` answers `+DMOSETVOLUME:0` for a value 1 to 8 and `+DMOSETVOLUME:1` otherwise. `AT+DMOSETGROUP=bw,tx,rx,txtone,squelch,rxtone` needs exactly six comma separated fields: bandwidth 0 or 1, transmit and receive in `fff.ffff` megahertz and inside the band, squelch 0 to 8. When valid it stores the bandwidth and both frequencies and answers `+DMOSETGROUP:0`; any malformed or out-of-band group answers `+DMOSETGROUP:1` and changes nothing. The tone fields are not checked. Mutates the twin only.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_dorji_twin_snapshot]

Reports the twin's state for oracle comparison. Always `Some`: `frequency` is the receive frequency in hertz, `mode_code` is `1` (wide) or `0` (narrow) as an ASCII byte, and `ptt` is always false because the module has no software PTT. Borrows immutably, no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_dorji_twin_feed]

Consumes controller bytes and returns the module's replies. Bytes are appended to the receive buffer with the monotonic time `now` (a partial line older than 500 ms is dropped). Each complete LF-terminated line is trimmed and given to `answer`; when `answer` returns a reply it is appended followed by CR LF, and lines the module does not know produce no bytes. Replies keep command order. Empty vector when no complete line is held. No I/O, never blocks, needs `&mut`.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_dorji_twin_front_panel]

Always refuses. A Dorji module has no knobs, so every `panel` value returns `Err` with the text `a Dorji module has no <panel> knob`; the twin is not changed and no bytes are produced. It exists to satisfy the `VendorTwin` trait so a test that tries to turn a knob gets a clear error.

## `crates/radioctl-protocols/src/guohe.rs`

### `pub fn crc16(data: &[u8]) -> u16` [@ANCHOR: radioctl:radioctl_guohe_crc16]

Computes the CRC-16/CCITT-FALSE checksum used by the PMR-171 frames: polynomial 0x1021, initial value 0xFFFF, bits processed most significant first, no reflection and no final xor. `data` is borrowed and may be empty (the result is then 0xFFFF). Pure and cheap, cannot fail. The caller supplies the length byte, command and data (not the header); `build_frame` and `parse_frame` use it for the trailing two bytes, sent big endian.

### `pub fn build_frame(command: u8, data: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_guohe_build_frame]

Builds a complete PMR-171 frame. The result is the four-byte header `A5 A5 A5 A5`, a length byte equal to data length plus 3 (command byte plus two CRC bytes; saturates to 255 if data is longer than 252 bytes, which would make an invalid frame), the `command` byte, `data`, then the CRC-16 of length, command and data, big endian. `command` is the protocol command (for example 0x0b status, 0x09 frequencies, 0x0a modes, 0x07 PTT). `data` is borrowed. Returns a new vector; pure, no errors.

### `pub fn parse_frame(frame: &[u8]) -> Option<(u8, &[u8])>` [@ANCHOR: radioctl:radioctl_guohe_parse_frame]

Validates one whole frame and splits it. `frame` must start with the `A5 A5 A5 A5` header, have a length byte of at least 3, contain exactly that many bytes after the length byte, and carry a CRC that matches. It returns `Some((command, data))` where `data` borrows from `frame` (and may be empty), or `None` for a bad header, short or long frame or CRC mismatch. It never panics on arbitrary bytes. Pure; used by both the engine's `feed` and the twin's.

### `fn take_frame(rx: &mut RxBuffer) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_guohe_take_frame]

Cuts one frame candidate from the front of the receive buffer `rx` (mutated). It looks for the `A5 A5 A5 A5` header; bytes before it are discarded, and if no header is found all but the last three bytes (a possible start of a header) are discarded. If fewer than five bytes (header plus length) are held it returns `None` and waits for more. Otherwise it takes header + 1 + length byte bytes if all have arrived, else `None`. The CRC is not checked here: the caller runs `parse_frame` on the result. Loops until it can decide, so garbage cannot stall it.

### `pub fn new(desc: RigDescription) -> Result<GuoheEngine, RigError>` [@ANCHOR: radioctl:radioctl_guohe_engine_new]

Builds the client engine for a Guohe PMR-171 from a vendor serial description. `desc` is owned and kept. The dialect must be `pmr171`, else `Unsupported` (also for a non-vendor description). The engine starts with VFO A at the description's default frequency and VFO B at 14,999,000 Hz, both VFOs in the default mode code (falling back to ASCII `0`), nothing pending and an empty buffer. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_guohe_engine_encode]

Builds the frame for one operation. `op` is borrowed. `GetFrequency`, `GetMode` and `GetPtt` all send the status command 0x0b and expect an observation of the field asked for. `SetFrequency(vfo, hz)` replaces that VFO in the engine's copy and sends both VFO frequencies (command 0x09, two 32-bit big endian values); a frequency over `u32::MAX` returns `Unsupported`. `SetMode` sends both mode digits (0x0a) after setting VFO A's mode from the description's code. `SetPtt(on)` sends command 0x07 with data 0 to key and 1 to unkey (inverted sense). Other operations, or one the description lacks the capability for, return `Unsupported`. The engine records what answer it expects and clears its buffer. Keying the radio is only a byte on the wire here; the actor applies the transmit gate and fail-closed unkey around it.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_guohe_engine_feed]

Decodes bytes from the radio into frames. A stale partial frame (older than 500 ms) is returned as `Inbound::Unrecognised` and dropped. Each frame cut by `take_frame` is checked by `parse_frame`; a bad frame becomes `Unrecognised`. While a status request is pending, a 0x0b frame of at least 11 data bytes (ptt, two mode digits, two 32-bit frequencies) updates the engine's cache and yields observations for VFO A and B frequency, the mode of A if the description knows the code, and PTT (true when the byte is 1). While an acknowledgement is pending any valid frame yields `Inbound::Ack`. Anything else is `Unrecognised`. The pending state is consumed by each frame. There is no Nak form on this radio.

### `pub fn new(desc: &RigDescription) -> Result<GuoheTwin, RigError>` [@ANCHOR: radioctl:radioctl_guohe_twin_new]

Builds a virtual PMR-171 for tests. `desc` is borrowed; the dialect must be `pmr171` or `Unsupported` is returned. VFO A takes the default frequency, VFO B starts at 14,999,000 Hz, both modes take the default mode code (fallback ASCII `0`), PTT is off and the buffer is empty. No I/O.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_guohe_twin_snapshot]

Returns the twin's state for oracle comparison. Always `Some`: `frequency` is VFO A in hertz, `mode_code` is VFO A's ASCII mode code and `ptt` is the modelled key flag. VFO B and its mode are not reported. Immutable, no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_guohe_twin_feed]

Consumes controller bytes and returns the radio's reply frames. Frames are cut with `take_frame` and checked with `parse_frame`; bad ones are ignored silently. Command 0x0b with no data answers a status frame (ptt byte, two mode digits, two frequencies as 32-bit big endian). Command 0x09 with eight bytes stores both frequencies and echoes the frame. Command 0x0a with two digits 0 to 9 stores both modes and echoes. Command 0x07 with 0 or 1 sets PTT (0 means on) and echoes. Any other command is ignored with no reply. Mutates the twin, no I/O. The PTT is a modelled flag; nothing is keyed.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_guohe_twin_front_panel]

Simulates an operator action at the radio. `panel` is owned. `Panel::Frequency(hz)` sets VFO A's frequency, `Panel::Ptt(on)` sets the modelled key flag. Any other control returns `Err` with `a Guohe twin has no <panel> knob`. Success returns an empty vector because the radio does not report front-panel changes unprompted. No range check. Nothing is transmitted.

## `crates/radioctl-protocols/src/kachina.rs`

### `pub fn dds_bytes(hz: Hz) -> Option<[u8; 4]>` [@ANCHOR: radioctl:radioctl_kachina_dds_bytes]

Converts a frequency to the four-byte DDS tuning word the Kachina 505DSP takes. `hz` is in hertz and must be 30,000 to 30,000,000 inclusive, else `None`. The word is `2.2369621333 * (75,000,000 + hz)` truncated to an integer, sent big endian; the top byte keeps only its low six bits and is or-ed with 0x40 (antenna port A). Pure floating-point arithmetic, exact because the product is below 2^30. Used by the engine for both receive and transmit tuning. The Hamlib source is the reference; not verified on a radio.

### `pub fn frequency_of(bytes: [u8; 4]) -> Hz` [@ANCHOR: radioctl:radioctl_kachina_frequency_of]

The inverse of `dds_bytes`: turns four DDS bytes into a frequency in hertz. The top two bits of the first byte (antenna bits) are ignored; the word is divided by 2.2369621333, 75,000,000 is subtracted, and the result is rounded to the nearest hertz and clamped at zero. `bytes` is owned (copied). Pure; it does not check the 30 kHz to 30 MHz range, so any word gives some value.

### `fn frame(letter: u8, data: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_kachina_frame]

Builds one Kachina command frame: STX (0x02), the ASCII command letter, the data bytes, ETX (0x03). `letter` is `R` (receive frequency), `T` (transmit frequency), `M` (mode) or `x` (PTT) as used by the engine; `data` is borrowed and may be empty. No length byte or checksum exists in this protocol. Returns a new vector; pure, no errors.

### `pub fn new(desc: RigDescription) -> Result<KachinaEngine, RigError>` [@ANCHOR: radioctl:radioctl_kachina_engine_new]

Builds the client engine for a Kachina 505DSP. `desc` is owned and kept. The dialect must be `505dsp`; otherwise (or for a non-vendor description) `Unsupported`. The engine starts with no acknowledgements owed. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_kachina_engine_encode]

Builds the command bytes for an operation. `op` is borrowed. `SetFrequency` on VFO A sends two frames back to back, `R` and `T`, both with the same DDS word, so receive and transmit are tuned together (frequency must be 30 kHz to 30 MHz, else `Unsupported`) and two acknowledgements are owed. `SetMode` sends frame `M` with the description's mode code minus ASCII `0` (mode must be in the description) and owes one. `SetPtt(on)` sends frame `x` with 1 to key and 0 to unkey and owes one. All other operations, or one whose capability is missing, return `Unsupported`; the radio has no read commands. The reply kind is `Ack`. The actor writes the bytes and applies the transmit gate before `SetPtt`; the engine only records how many answers are owed.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_kachina_engine_feed]

Decodes single status bytes from the radio. The time argument is unused (no framing, so no stale buffer). Byte 0xFF (good) while acknowledgements are owed decrements the count and, when it reaches zero, produces one `Inbound::Ack`, so a frequency command (two frames) is acknowledged only after both are accepted. Byte 0xFE (error) clears the count and produces `Inbound::Nak`. Any other byte, including a 0xFF when nothing is owed, becomes `Inbound::Unrecognised` with that one byte. Never blocks.

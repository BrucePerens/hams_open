# radioctl function anchors, batch 08

Function anchors for the Racal twin and the ADAT, CommRadio, GomSpace, PRM80, RS-HFIQ, RFT and DRT1 vendor engines and virtual twins in `radioctl-protocols`, written to the ADR 0113 standard. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
The RFT and DRT1 radios are receive-only. The RS-HFIQ engine can encode `SetPtt` (`*x1` and `*x0`) and the GomSpace twin models a transmit table; an engine only checks that the description lists the capability, and the lock, band check and transmit gate are applied by the caller that owns the engine (unknown from these files; check `radioctld`), never by an engine or a twin.

## `crates/radioctl-protocols/src/racal.rs`

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_racal_twin_feed]

The virtual Racal receiver's command parser. It buffers bytes received at `now` and handles each CR-terminated line that starts with the dialect's prefix (a line without it is silently skipped). The ask-frequency and ask-mode commands are answered in the dialect's form (`F<mhz or hz>`, `D<code>I3`, or `M<code>` on the RA3702, with the reply prefix `$0` on the RX-331 and LF on the RA3702), the RA3702 filter queries get fixed answers, and `F` and mode-set commands update the twin if the number parses and the mode code is one the description lists. Set commands get no reply. Returns the bytes to send; mutates the twin; no I/O.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_racal_twin_front_panel]

Applies a scripted front-panel action to the twin: only `Panel::Frequency(hz)` is accepted and sets the stored frequency in hertz. Any other action returns an `Err` string naming the knob a Racal twin lacks. On success it returns an empty byte vector because the receiver sends nothing by itself. Only the in-memory twin changes.

## `crates/radioctl-protocols/src/adat.rs`

### `fn value_of(line: &[u8]) -> Option<&[u8]>` [@ANCHOR: radioctl:radioctl_adat_value_of]

Extracts the value part of an ADAT reply line: whatever follows the first space, with leading and trailing blanks trimmed. A leading NUL byte, which the radio can emit at the start of an answer, is dropped first. This mirrors Hamlib, which does not examine what comes before the first space, so the exact echo is unknown. A line with no space returns `None`. The result borrows from the input. Pure.

## `crates/radioctl-protocols/src/commradio.rs`

### `fn push_escaped(out: &mut Vec<u8>, byte: u8)` [@ANCHOR: radioctl:radioctl_commradio_push_escaped]

Appends one byte to an outgoing CommRadio frame, escaping the three reserved values: the frame start `FE`, the frame end `FD` and the escape byte `FC` itself are written as `FC` followed by the byte XOR 0x14; every other byte is appended unchanged. `out` is mutated in place by push, and nothing is returned. It is applied to the message bytes and to both CRC bytes but not to the address or type. Pure, no errors.

## `crates/radioctl-protocols/src/gomspace.rs`

### `fn table_of(vfo: Vfo) -> u8` [@ANCHOR: radioctl:radioctl_gomspace_table_of]

Gives the GS100 configuration table number for a VFO: VFO A is table 1 (receive) and VFO B is table 5 (transmit). The engine sends `param mem <n>` with this number before each frequency get or set, so setting VFO B changes the transmit frequency table, not a receive one. Takes the VFO by value, exhaustive, cannot fail. Pure.

### `fn slot(&mut self) -> Option<&mut Hz>` [@ANCHOR: radioctl:radioctl_gomspace_twin_slot]

Returns a mutable reference to the frequency of the table the twin currently has selected: the receive frequency for table 1, the transmit frequency for table 5, and `None` for any other table number (a selection the twin does not model). The caller uses it to read or write the frequency; a `None` means a `param get freq` prints nothing and a set is ignored. Borrows the twin mutably. No I/O.

## `crates/radioctl-protocols/src/prm80.rs`

### `fn hex_byte(text: &[u8]) -> Option<u8>` [@ANCHOR: radioctl:radioctl_prm80_hex_byte]

Parses exactly two hexadecimal digits (either case) from the radio's `E` status answer into one byte. Text that is not valid UTF-8, has a length other than two, or contains a non-hex character returns `None`, so a damaged answer is refused and not misread. The status answer is eleven such bytes in a row. Borrows the slice; pure.

## `crates/radioctl-protocols/src/rshfiq.rs`

### `pub fn new(desc: RigDescription) -> Result<RshfiqEngine, RigError>` [@ANCHOR: radioctl:radioctl_rshfiq_engine_new]

Builds the engine for a HobbyPCB RS-HFIQ description, taking it by value. It must be a serial-vendor description whose dialect is `rshfiq`; otherwise it returns an `Unsupported` error. The engine starts with nothing pending and an empty receive buffer. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_rshfiq_engine_encode]

Builds the bytes for an operation. It refuses with `Unsupported` an op whose capability the description lacks. Supported: get frequency on VFO A (`*f?` CR, expects a number), set frequency on VFO A (`*f<Hz>` CR, no reply), set PTT (`*x1` or `*x0` CR, no reply) and get power (`*L` CR, expects the transmit power in percent). Everything else, including VFO B and any mode operation, is `Unsupported`. `SetPtt` keys the transmitter; the gate is applied by the caller. The engine records what is pending and clears the receive buffer, so one request at a time. No I/O.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_rshfiq_engine_feed]

Processes bytes received at `now`. A stale partial frame is discarded and reported `Unrecognised`. Each LF-ended line has a trailing CR removed; empty lines are skipped. The line is read as a decimal number: after a frequency read it becomes a frequency observation in hertz; after a power read it becomes a power observation if it is 0 to 110 (percent of nominal), else `Unrecognised`. With nothing pending, or a line that is not a number, it is `Unrecognised`. Pending state is cleared for every line. No I/O, never blocks.

### `pub fn new(desc: &RigDescription) -> Result<RshfiqTwin, RigError>` [@ANCHOR: radioctl:radioctl_rshfiq_twin_new]

Builds the virtual RS-HFIQ from a description (borrowed): the description's default frequency, PTT off and power 0 percent. It returns `Unsupported` if the description is not a serial-vendor one. No I/O.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_rshfiq_twin_feed]

The virtual radio's command parser. It buffers bytes received at `now` and handles each CR-terminated command: `*f?` answers the frequency digits and CR LF, `*L` answers the power percent, `*x1` sets the twin's PTT flag and power to 100, `*x0` clears both, and `*f<digits>` sets the frequency. Malformed digits and unknown commands are ignored without a reply. Returns the bytes to send; mutates the twin; no I/O. The power value is the twin's own simple model, not a measurement.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_rshfiq_twin_front_panel]

Applies a scripted front-panel action to the twin: `Panel::Frequency(hz)` sets the frequency in hertz and anything else returns an `Err` naming the knob an RS-HFIQ lacks. On success the result is an empty vector, since the radio sends nothing unprompted. Only the in-memory twin changes.

## `crates/radioctl-protocols/src/rft.rs`

### `pub fn format_khz(hz: Hz) -> String` [@ANCHOR: radioctl:radioctl_rft_format_khz]

Formats a frequency as kilohertz with exactly six decimals, for the EKD-500 `FRQ` command: the whole kilohertz, a point, the three remaining hertz digits and three zeros (14.2 MHz is `14200.000000`; 7,000,001 Hz is `7000.001000`). Integer arithmetic only, so there is no rounding, and it cannot fail. Returns a new string.

### `pub fn parse_khz(text: &[u8]) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_rft_parse_khz]

Parses a kilohertz decimal into integer hertz. The first three decimals are hertz digits (`.5` is 500 Hz, `.001` is 1 Hz); further decimals must be zero. Returns `None` for a non-digit anywhere, an empty whole part, a nonzero digit past the third decimal (a fraction of a hertz) or an overflow. A number with no point is whole kilohertz. Borrows the bytes; pure; the inverse of `format_khz`.

### `pub fn new(desc: RigDescription) -> Result<RftEngine, RigError>` [@ANCHOR: radioctl:radioctl_rft_engine_new]

Builds the engine for an RFT EKD-500 description, taking it by value. It requires a serial-vendor description with dialect `ekd500`, else it returns `Unsupported`. The engine has a description and a receive buffer only, since the receiver has no state to read back. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_rft_engine_encode]

Builds the only command the EKD-500 understands: set frequency on VFO A becomes `FRQ<kHz>` CR with six decimals (`format_khz`), with no reply expected. It refuses, with `Unsupported`, an op the description does not list and everything else (reads, modes, PTT, VFO B). The frequency is not range checked here. It clears the receive buffer. The receiver has no PTT, so nothing here can transmit.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_rft_engine_feed]

Processes bytes received at `now`. A stale partial frame is discarded and reported `Unrecognised`. Every CR-terminated line the receiver sends is reported as an `Ack`, whatever its content, because Hamlib waits for a line after a set and accepts a timeout, so the content is not examined. Silence produces no events. No I/O, never blocks.

### `pub fn new(desc: &RigDescription) -> Result<RftTwin, RigError>` [@ANCHOR: radioctl:radioctl_rft_twin_new]

Builds the virtual EKD-500 from a description (borrowed): it starts tuned to the description's default frequency with an empty buffer. It returns `Unsupported` if the description is not a serial-vendor one. No I/O.

### `pub fn frequency(&self) -> Hz` [@ANCHOR: radioctl:radioctl_rft_twin_frequency]

Returns the frequency the twin is tuned to, in hertz, copied out. Because the real receiver cannot be read back, tests use this accessor to see what the engine sent. Borrows the twin, cannot fail, no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_rft_twin_feed]

The virtual receiver's command parser. It buffers bytes received at `now` and, for each CR-terminated line that begins `FRQ` and carries a valid kilohertz decimal (`parse_khz`), sets its frequency. Anything else is ignored. It always returns an empty vector, because the twin says nothing. Mutates the twin; no I/O.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_rft_twin_front_panel]

Applies a scripted front-panel action to the twin: `Panel::Frequency(hz)` sets the frequency in hertz and anything else returns an `Err` naming the knob an EKD-500 lacks. On success it returns an empty vector. Only the in-memory twin changes.

## `crates/radioctl-protocols/src/drt1.rs`

The DRT1 has no byte stream: the host programs an AD9951 DDS by wiggling the serial port's RTS, DTR and break lines, so these functions build and decode line sequences (`LineStep`: a line, asserted or not, and a hold time).

### `pub fn cfr2_word(ref_mult: u32, pump_ua: u32) -> u32` [@ANCHOR: radioctl:radioctl_drt1_cfr2_word]

Computes the 3-byte CFR2 register value that sets the DDS clock multiplier and charge pump. `ref_mult` goes in bits 7 to 3 (masked to five bits), bit 2 (VCO gain) is always set, and the charge pump current in microamperes, valid 75 to 150 in steps of 25, becomes bits 1 and 0 as `(pump_ua - 75) / 25` (a value below 75 saturates to 0, one above 150 wraps by the mask). With the defaults, multiplier 8 and 150 uA, the result is 0x47. Pure.

### `pub fn tuning_word(hz: Hz) -> u32` [@ANCHOR: radioctl:radioctl_drt1_tuning_word]

Computes the 32-bit FTW0 tuning word for a receive frequency: `(f + 45 MHz IF) / (45.012 MHz oscillator * 8) * 2^32`, truncated like the DDS arithmetic. It uses floating point and the module's defaults for the oscillator, IF and multiplier. It does not range check; `encode_lines` limits the input to 50 kHz through 30 MHz first, and a value that makes the ratio exceed 1 would wrap. Pure.

### `pub fn word_frequency(ftw: u32, cfr2: u32) -> Hz` [@ANCHOR: radioctl:radioctl_drt1_word_frequency]

The inverse of `tuning_word`, used by the twin: from a tuning word and a CFR2 word it takes the multiplier from CFR2 bits 7 to 3, computes the oscillator frequency `ftw / 2^32 * 45.012 MHz * multiplier`, subtracts the 45 MHz IF, and rounds to the nearest hertz, never going below 0. Floating point; pure; cannot fail.

### `pub fn write_register(addr: u32, bytes: u32, data: u32) -> Vec<LineStep>` [@ANCHOR: radioctl:radioctl_drt1_write_register]

Builds the line steps of one DDS register write. It clears break, raises DTR, sends the instruction byte (the 5-bit `addr` with bit 7 zero for a write), then `bytes` data bytes of `data`, most significant bit first, then drops DTR, which latches the register. Each bit sets RTS (asserted for a 0, because the level converter inverts) and then sets and clears break as the clock. Every step is held for `STEP_HOLD` (20 microseconds). `bytes` is 3 for CFR2 and 4 for FTW0. Returns a new vector; pure.

### `pub fn tune_steps(hz: Hz) -> Vec<LineStep>` [@ANCHOR: radioctl:radioctl_drt1_tune_steps]

Builds the complete line sequence to tune the receiver: first the idle state (DTR down, RTS down, break clear), then a CFR2 write (address 1, three bytes, from the default multiplier and 150 uA) and an FTW0 write (address 4, four bytes) for `hz`. The caller executes the steps on the serial port's control lines, with the hold times, to program the DDS. Does no range check (see `encode_lines`). Pure; allocates.

### `pub fn new(desc: RigDescription) -> Result<Drt1Engine, RigError>` [@ANCHOR: radioctl:radioctl_drt1_engine_new]

Builds the DRT1 engine from a description, taking it by value. It requires a serial-vendor description whose dialect is `drt1`, else it returns `Unsupported`. The engine holds only the description, because there is no byte stream and no state to track. No I/O.

### `fn encode_lines(&mut self, op: &Op) -> Result<Option<Vec<LineStep>>, RigError>` [@ANCHOR: radioctl:radioctl_drt1_engine_encode_lines]

The engine's real command path: it turns an operation into a control-line sequence. It first refuses an op whose capability the description lacks (`Unsupported`). Setting the frequency on VFO A to 50 kHz through 30 MHz returns `Some(tune_steps(hz))`; a frequency outside that range returns a `Malformed` error saying the DRT1 receives 50 kHz to 30 MHz; every other op is `Unsupported`. The receiver is AM only with no read-back and no PTT, so nothing here can transmit. Pure.

### `fn encode(&mut self, _op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_drt1_engine_encode]

The byte-stream encoder required by the `ProtocolEngine` trait. The DRT1 is commanded only by control-line sequences, so this always returns an `Unsupported` error whatever the operation; callers must use `encode_lines`. The argument is ignored. Pure, no side effects.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_drt1_engine_feed]

Handles bytes received from the DRT1. The receiver sends none, so an empty slice gives no events and any bytes that do arrive (line noise, a wrong device) are returned as one `Unrecognised` event. The timestamp is ignored. Pure, no I/O.

### `pub fn new(desc: &RigDescription) -> Result<Drt1Twin, RigError>` [@ANCHOR: radioctl:radioctl_drt1_twin_new]

Builds the virtual DRT1, an AD9951 serial-port decoder, from a description (borrowed). Its shared state, behind an `Arc<Mutex>`, starts with all lines low, no bits clocked, no register latched and the description's default frequency. It returns `Unsupported` if the description is not a serial-vendor one. The twin is `Clone`: one handle goes to the actor as its control lines and another is kept to inspect the tuning. No I/O.

### `fn latch(&mut self)` [@ANCHOR: radioctl:radioctl_drt1_dds_latch]

Called when DTR drops, to decode the bits clocked in since DTR rose. The first eight bits are the instruction; a set bit 7 (a read) is ignored. A write to register 1 with exactly 24 data bits stores CFR2, and register 4 with exactly 32 bits stores FTW0; any other address or length is dropped. When both registers have been latched at least once, the twin's frequency is recomputed with `word_frequency`. Mutates only the twin state; shorter than eight bits returns without effect.

### `pub fn frequency(&self) -> Hz` [@ANCHOR: radioctl:radioctl_drt1_twin_frequency]

Returns the frequency the twin is tuned to, in hertz. It stays at the description's default until both CFR2 and FTW0 have been written and latched, then follows the last tune. Takes the shared lock briefly (a poisoned lock is recovered, not a panic). Used by tests to check what a tune sequence programmed.

### `pub fn registers(&self) -> (Option<u32>, Option<u32>)` [@ANCHOR: radioctl:radioctl_drt1_twin_registers]

Returns the CFR2 and FTW0 words last latched, as a tuple of options, with `None` for a register that has not been written. It lets tests verify the exact register values a tune produced. It takes the shared lock briefly (a poisoned lock is recovered), returns copies and has no side effects.

# radioctl function anchors, batch 09

Function anchors for the DRT1 twin, and the ADAT, GomSpace, CommRadio and PRM80 vendor helpers, engines and virtual twins in `radioctl-protocols`, written to the ADR 0113 standard. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
The ADAT engine can encode `SetPtt` (`$MOX>` and `$MOX<`); the PRM80 engine is read only, and the DRT1 and CommRadio CTX-10 have no PTT (push to talk). An engine only checks that the description lists the capability; the lock, band check and transmit gate are applied by the caller that owns the engine (unknown from these files; check `radioctld`), never by an engine or twin.

## `crates/radioctl-protocols/src/drt1.rs`

### `fn set_line(&self, _line: KeyLine, _asserted: bool) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_drt1_twin_set_line]

The `ControlLines` method for a transmit key line (the PTT or CW (continuous wave, Morse code) key). The DRT1 is a receiver and has no keying line, so the twin always returns an `Unsupported` error and ignores both arguments. This guarantees a DRT1 can never be keyed through the line interface. It takes `&self`, changes no state and does not lock.

### `fn set_control(&self, line: ControlLine, asserted: bool) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_drt1_twin_set_control]

Applies one serial control-line change to the virtual AD9951 serial port; it always returns `Ok`. It takes the shared state lock for the call. RTS (request to send) just records its level. A DTR (data terminal ready) rising edge clears the collected bits, and a DTR falling edge latches the register (`latch`) before the level is stored. A break rising edge while DTR is up clocks in one bit, whose value is the inverse of RTS (RTS is asserted for a 0). Changes that repeat the current level, and breaks while DTR is low, do nothing. This is how `write_register` sequences are decoded.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_drt1_twin_snapshot]

Reports the twin's observable state to the test and display harness: the current tuned frequency in hertz (from `frequency`), no mode code (the DRT1 is AM only and has no mode command) and PTT false. It always returns `Some`. It takes the shared lock briefly and changes nothing.

### `fn feed(&mut self, _bytes: &[u8], _now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_drt1_twin_feed]

The `VendorTwin` byte-stream input. The DRT1 has no serial data stream, so the twin ignores whatever it is given and always returns an empty vector. Both arguments are unused. It is required by the trait only; tuning arrives through `set_control`.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_drt1_twin_front_panel]

Applies a scripted front-panel action to the twin: `Panel::Frequency(hz)` sets the tuned frequency in hertz directly, bypassing the register decoding, and any other action returns an `Err` naming the knob a DRT1 lacks. It returns an empty vector on success. It takes the shared lock briefly (a poisoned lock is recovered). Only the in-memory twin changes.

## `crates/radioctl-protocols/src/adat.rs`

### `pub fn mode_code_of_name(name: &[u8]) -> Option<u8>` [@ANCHOR: radioctl:radioctl_adat_mode_code_of_name]

Looks up the numeric wire code of a mode name as the ADT-200A prints it in a `$MOD?` answer: `CW-R` is 0, `CW` 1, `LSB` 2, `USB` 3, `AM` 5, `AM-SL` 6, `AM-SU` 7 and `FM` 8 (code 4 is unused). The match is exact and case sensitive on the bytes. An unknown name returns `None`. Pure; borrows the slice.

### `pub fn parse_frequency(text: &[u8]) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_adat_parse_frequency]

Parses a number with a unit, as the ADT-200A prints it, into exact integer hertz: `14.2MHz`, `7100.5kHz`, `800Hz` or `1.296GHz`. The unit (`Hz`, `kHz`, `MHz` or `GHz`, case sensitive) starts at the first letter. The fraction is scaled exactly; digits that would be finer than one hertz must be zero or the result is `None`. An unknown unit, empty whole part, non-digit or overflow also returns `None`. Pure; borrows the bytes.

### `pub fn new(desc: RigDescription) -> Result<AdatEngine, RigError>` [@ANCHOR: radioctl:radioctl_adat_engine_new]

Builds the engine for an ADAT ADT-200A description, taking it by value. It must be a serial-vendor description with dialect `adt200a`; otherwise it returns `Unsupported`. The engine starts with nothing pending and an empty receive buffer. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_adat_engine_encode]

Builds the bytes for an operation after refusing (`Unsupported`) any whose capability the description lacks. Supported: get frequency on VFO (variable frequency oscillator, the tuning control) A (`$FRA?` CR) and get mode (`$MOD?`), both expecting an observation; set frequency on VFO A (`$FR1:<Hz>` CR) and set mode (`$MOD:<nn>`, the description's code as two digits, `Unsupported` if the mode has no code), neither answered; set PTT (`$MOX>` to key and `$MOX<` to unkey, not answered) and get PTT (`$MTR?`). Any other op is `Unsupported`. `SetPtt` can key the transmitter, and the caller owns the gate. It records the pending read and clears the receive buffer.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_adat_engine_feed]

Processes bytes received at `now`. A stale partial frame is discarded and reported `Unrecognised`. Each LF-ended line, with a trailing CR removed and empty lines skipped, is matched to the pending read, using only what follows the first space. A frequency answer `<vfo> <number><unit>` gives a frequency observation, but VFO 0 (no VFO active) is not a frequency and is `Unrecognised`. A mode name that the description can map gives a mode observation. A PTT answer is `0` or `1`. Anything else, or any line with nothing pending, is `Unrecognised`. The pending state is cleared for every line. No I/O.

### `pub fn new(desc: &RigDescription) -> Result<AdatTwin, RigError>` [@ANCHOR: radioctl:radioctl_adat_twin_new]

Builds the virtual ADT-200A from a description (borrowed): the description's default frequency, the code of the default mode (3, USB, if none is listed), PTT off and a copy of the list of mode codes. It returns `Unsupported` if the description is not a serial-vendor one. No I/O.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_adat_twin_feed]

The virtual radio's command parser. It buffers bytes received at `now` and handles each CR-terminated command. `$FRA?` answers `$FRA? 1 <Hz>Hz`, `$MOD?` the mode name (USB if the stored code has no name) and `$MTR?` 0 or 1, each ended by CR LF. `$MOX>` and `$MOX<` set and clear the PTT flag, and `$FR1:<digits>` and `$MOD:<nn>` set the frequency and, if the description lists the code, the mode; none of the settings is answered. Malformed or unknown commands are ignored. Mutates the twin; no I/O.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_adat_twin_front_panel]

Applies a scripted front-panel action to the twin: `Panel::Frequency(hz)` sets the frequency in hertz and `Panel::Ptt(on)` sets the transmit flag. Anything else returns an `Err` naming the knob an ADT-200A lacks. An empty vector is returned on success. Only the in-memory twin changes.

## `crates/radioctl-protocols/src/gomspace.rs`

### `pub fn count_prompts(bytes: &[u8]) -> usize` [@ANCHOR: radioctl:radioctl_gomspace_count_prompts]

Counts how many times the GS100 console prompt (`PROMPT`, which carries colour escape codes) appears in a byte slice, scanning with a sliding window so overlapping matches are counted. The engine uses it to know when both commands of a request have been answered (two prompts). Pure; borrows the slice; cannot fail.

### `pub fn lines_without_prompts(bytes: &[u8]) -> Vec<Vec<u8>>` [@ANCHOR: radioctl:radioctl_gomspace_lines_between_prompts]

Splits console output into its text lines with the prompts removed. Each prompt is replaced by a line break, the text is split at LF, each line has surrounding whitespace (including CR) trimmed, and empty lines are dropped. The remaining lines are the echoed commands and any answer such as `freq = 7074000`. It allocates new vectors and is pure.

### `pub fn new(desc: RigDescription) -> Result<GomspaceEngine, RigError>` [@ANCHOR: radioctl:radioctl_gomspace_engine_new]

Builds the engine for a GomSpace GS100 description, taking it by value. It must be a serial-vendor description with dialect `gs100`; otherwise `Unsupported`. The engine starts with nothing pending and an empty receive buffer. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_gomspace_engine_encode]

Builds the console commands for an operation, after refusing (`Unsupported`) any the description lacks. Only frequency get and set are supported, on either VFO: both are sent as two commands in one write, `param mem <table>` (1 for VFO A receive, 5 for VFO B transmit) and then `param get freq` or `param set freq <Hz>`, each ended by LF. A get expects an observation and a set an ack. It does not range check the frequency. It records what is pending and clears the receive buffer.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_gomspace_engine_feed]

Collects console output until both commands of the pending request have produced a prompt (two prompts), and returns zero or one event for it. A stale partial frame is first discarded and reported `Unrecognised`; with nothing pending or fewer than two prompts it waits. Then the lines without prompts are taken, the echoed `param ` lines are dropped, and what remains is the answer: for a set, an empty answer is `Ack` and anything else `Nak`; for a get, exactly one line of the form `freq = <Hz>` gives a frequency observation for the VFO asked, else `Unrecognised`. No I/O.

### `pub fn new(desc: &RigDescription) -> Result<GomspaceTwin, RigError>` [@ANCHOR: radioctl:radioctl_gomspace_twin_new]

Builds the virtual GS100 from a description (borrowed): table 1 (receive) selected, and both the receive and transmit frequencies set to the description's default. It returns `Unsupported` if the description is not a serial-vendor one. No I/O.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_gomspace_twin_feed]

The virtual console. For each LF-terminated line received at `now` it echoes the line with CR LF, then handles `param mem <n>` (select table n), `param get freq` (print `freq = <Hz>` for the selected table, if it is table 1 or 5) and `param set freq <Hz>` (store in the selected table). It ends every line with the colour prompt and an LF. Other commands only get echo and prompt. Returns the output bytes; mutates the twin; no I/O.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_gomspace_twin_front_panel]

Applies a scripted front-panel action to the twin: `Panel::Frequency(hz)` sets the receive-table frequency in hertz (the transmit table is not changed). Anything else returns an `Err` naming the knob a GS100 lacks. An empty vector is returned on success. Only the in-memory twin changes.

## `crates/radioctl-protocols/src/commradio.rs`

### `pub fn crc16(data: &[u8], mut crc: u16) -> u16` [@ANCHOR: radioctl:radioctl_commradio_crc16]

Computes the CRC-16/KERMIT (reflected polynomial 0x8408, as the CTX-10 frames use) of `data`, continuing from a starting value `crc`: pass 0 for a fresh checksum, or the previous result to extend it over more data. For the ASCII text `123456789` and a start of 0 it returns 0x2189. Pure; borrows the slice; never fails.

### `pub fn frame(address: u8, kind: u8, payload: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_commradio_frame]

Builds a complete CTX-10 frame: `FE`, the destination `address` (0x21 to the radio, 0x11 to the host), the message `kind`, the payload, the two CRC bytes high byte first, and `FD`. The CRC covers the address, kind and payload. The payload and CRC bytes (not the address or kind) are escaped so that `FC`, `FD` and `FE` become `FC` and the byte XOR 0x14. Returns a new vector; pure; cannot fail.

### `pub fn unframe(bytes: &[u8], address: u8) -> Option<(u8, Vec<u8>)>` [@ANCHOR: radioctl:radioctl_commradio_unframe]

Decodes one whole frame (from `FE` to `FD`) that must be addressed to `address`. It returns the message kind and the payload with escapes removed. It returns `None`, never panicking, when the start or end byte is missing, the address differs, an unescaped `FE` or `FD` or a dangling escape appears inside, the message is shorter than a kind plus two CRC bytes, or the CRC does not match. The reverse of `frame`. Pure; allocates the payload.

### `pub fn new(desc: RigDescription) -> Result<CommradioEngine, RigError>` [@ANCHOR: radioctl:radioctl_commradio_engine_new]

Builds the engine for a CommRadio CTX-10 description, taking it by value. It must be a serial-vendor description with dialect `ctx10`; otherwise `Unsupported`. The engine starts with nothing pending and an empty buffer. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_commradio_engine_encode]

Builds the frame for an operation, after refusing (`Unsupported`) any the description lacks. Supported: get frequency on VFO A (frame type 0x32, no payload, expects an observation) and set frequency on VFO A (type 0x30 with the frequency as four big-endian bytes, expects an acknowledgement). A frequency outside 150 kHz to 30 MHz returns a `Rejected` error; one that does not fit 32 bits is `Unsupported`. Everything else (modes, VFO B, PTT) is `Unsupported`. It remembers the pending operation, including the frequency to be confirmed, and clears the receive buffer. The radio has no PTT.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_commradio_engine_feed]

Processes bytes received at `now`. A stale partial frame is discarded and reported `Unrecognised`. Each frame ending in `FD` is decoded with `unframe` for the host address. After a set, a `31` or `34` frame carrying the frequency that was requested is an `Ack`, and a different frequency is a `Nak`. Any `33` or `34` frame carrying a four-byte frequency gives a frequency observation, which also covers the radio announcing a knob turn by itself. A frame that does not fit the pending operation is `Unrecognised` and leaves the pending operation in place. No I/O.

### `pub fn new(desc: &RigDescription) -> Result<CommradioTwin, RigError>` [@ANCHOR: radioctl:radioctl_commradio_twin_new]

Builds the virtual CTX-10 from a description (borrowed): the description's default frequency and an empty buffer. It returns `Unsupported` if the description is not a serial-vendor one. No I/O.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_commradio_twin_feed]

The virtual radio's frame handler. For each frame ended by `FD` received at `now` that decodes for the radio address, a type 0x32 request with no payload is answered with a 0x33 frame carrying the frequency, and a type 0x30 set stores the frequency if it is in 150 kHz to 30 MHz, then answers with a 0x31 frame carrying the frequency now held (so an out-of-range value is answered with the old one, which the engine reads as a refusal). Bad frames and other types are ignored. Returns the frames to send; mutates the twin; no I/O.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_commradio_twin_front_panel]

Applies a scripted front-panel action to the twin: `Panel::Frequency(hz)` sets the frequency and returns a type 0x34 frame with the new frequency, because the real radio announces a knob turn by itself. Any other action returns an `Err` naming the knob a CTX-10 lacks. The returned bytes are for the simulated wire. Only the in-memory twin changes.

## `crates/radioctl-protocols/src/prm80.rs`

### `pub fn rx_from_pll(word: u16) -> Hz` [@ANCHOR: radioctl:radioctl_prm80_rx_from_pll]

Converts a receive PLL word, counted in 12.5 kHz steps, to the receive frequency in hertz. The word is multiplied by 12,500 Hz; above 300 MHz the 21.4 MHz IF is added, and at or below it is subtracted (not below zero). Takes the 16-bit word by value; pure; cannot fail. The inverse for the same split is `pll_from_rx`.

### `pub fn pll_from_rx(hz: Hz) -> Option<u16>` [@ANCHOR: radioctl:radioctl_prm80_pll_from_rx]

Converts a receive frequency to the PLL word that tunes it: above 300 MHz the IF of 21.4 MHz is subtracted from it, otherwise added, and the result is divided by the 12.5 kHz step (truncating). It returns `None` if the subtraction underflows or the word does not fit in 16 bits. Pure. The engine in this crate does not set frequencies, so this is for the twin and tests.

### `pub fn decode_state(line: &[u8]) -> Option<State>` [@ANCHOR: radioctl:radioctl_prm80_decode_state]

Decodes the radio's 22-character answer to the `E` command: eleven bytes as hex digit pairs. Byte 0 is the mode, whose bit 3 means transmitting (`ptt`); bytes 6 and 7 are the receive PLL word, converted with `rx_from_pll`; bytes 8 and 9 are the transmit PLL word, multiplied by 12.5 kHz; byte 10 is the raw signal strength. Returns `None` when the length is not 22 or any pair is not hex. Pure.

### `pub fn new(desc: RigDescription) -> Result<Prm80Engine, RigError>` [@ANCHOR: radioctl:radioctl_prm80_engine_new]

Builds the read-only engine for a Philips PRM8060 description, taking it by value. It must be a serial-vendor description with dialect `prm8060`, else `Unsupported`. The engine starts with no read pending and an empty buffer. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_prm80_engine_encode]

Builds the request for a read, after refusing (`Unsupported`) an op whose capability the description lacks. The three reads supported, frequency (either VFO), PTT state and signal strength, all send the single byte `E`, which makes the radio print its whole state, and the engine remembers which field was asked. Setting commands are not implemented, because the radio asks for each argument at a prompt, so any set returns `Unsupported`. The receive buffer is cleared. This engine cannot transmit.

# radioctl function anchors, batch 02

Function anchors for the `radioctl-core` crate (the USB link and the libusb backend) and the `radioctl-protocols` crate (the Si570 USB kits, the FUNcube dongles and the Elektor SDR-USB), written to the ADR 0113 standard. Tests cite its anchor with `Tests [@ANCHOR: ...]`.
Only the Si570 engine can key a transmitter, through its PTT request (`Op::SetPtt`); that request is encoded only for descriptions that list the capability, and the daemon's transmit gate (lock, band check, operator opening) sits in front of the engine, not inside it. The FUNcube and Elektor boards are receive only.

## `crates/radioctl-core/src/usb_link.rs`

### `fn session(&self, device: &mut dyn UsbDevice) -> SessionEnd` [@ANCHOR: radioctl:radioctl_core_usb_session]

The loop of one open device inside the supervisor thread. It waits on the command channel with a 100 ms tick; each `Command::Write` adds its bytes to the `bytes_out` counter, appends them to a local `pending` buffer and calls `serve` to perform every complete request frame. It returns `SessionEnd::Lost(reason)` when `serve` reports the device gone, and `SessionEnd::Shutdown` when a `Shutdown` command arrives or the channel is disconnected (the actor and bridge are gone). The `device` is borrowed mutably for the whole session and used only from this thread, because libusb transfers block. A timeout tick does nothing. It never reopens anything itself: the caller `run` decides what to do with `Lost`. It takes no locks and keys nothing; whatever the frames ask for is performed as received.

### `fn serve(&self, device: &mut dyn UsbDevice, pending: &mut Vec<u8>) -> Option<String>` [@ANCHOR: radioctl:radioctl_core_usb_serve]

Turns the bytes collected in `pending` into transfers. Each pass decodes one request frame with `UsbTransfer::decode`; a complete frame is removed from `pending` and performed with `execute` using `UsbConfig::transfer_timeout` (blocking, one transfer at a time). An incomplete frame returns `None` and keeps the bytes for the next write. A malformed frame means the actor and link are out of step: all pending bytes are dropped and a reply with op 0 and status `Other` is sent so the engine answers `Nak` instead of waiting. Every reply is encoded, counted in `bytes_in` and sent to the actor with `blocking_send`, which blocks if the actor is slow. When a reply has status `NoDevice` the remaining bytes are cleared (a stale command must not be replayed into a device that comes back) and `Some("device is gone")` is returned. Timeouts and stalls are reported in the reply and do not end the loop.

### `pub async fn open_usb(runtime: &Handle, backend: Arc<dyn UsbBackend>, config: UsbConfig) -> Result<UsbLink, RigError>` [@ANCHOR: radioctl:radioctl_core_usb_open]

Starts a USB link: the one entry point callers use. `runtime` is the Tokio runtime handle on which two small bridge tasks are spawned (stream to supervisor, supervisor to stream); `backend` is shared (`Arc`) with the supervisor thread named `radioctl-usb`, which opens the device through it; `config` is moved in and holds the target, the transfer timeout, the backoff and `first_open_attempts`. It creates an in-memory duplex stream of 8192 bytes, a bounded write queue of 64 commands and a read queue of 64 replies, then waits until the first open has succeeded. Returns a `UsbLink` whose `transport` goes to `rig::spawn`. Errors: the error of the last open attempt (normally `LinkDown`, for example "not plugged in"), a `LinkDown` if the thread cannot be started, or if the thread ends before opening. After the first success, later losses are retried forever with backoff. It keys nothing. Dropping the stream closes the device. It is async but the device work is on its own thread.

## `crates/radioctl-core/src/usb_libusb.rs`

### `pub fn new() -> Result<LibusbBackend, RigError>` [@ANCHOR: radioctl:radioctl_core_libusb_backend_new]

Creates the direct libusb backend (cargo feature `usb`, off by default) by initialising a libusb context with `rusb::Context::new`. It takes no arguments and holds the context for the life of the backend; the backend is then shared as `Arc<dyn UsbBackend>` with `open_usb`. The only error is `ErrorKind::Unsupported` with detail "libusb: ..." when libusb cannot initialise (library missing or a platform failure). It is cheap, synchronous and does not touch any device; enumeration happens later in `open`. The relay's zero-privilege broker will provide a different `UsbBackend`, so callers should not depend on this type directly.

### `fn open(&self, target: &UsbTarget) -> Result<Box<dyn UsbDevice>, RigError>` [@ANCHOR: radioctl:radioctl_core_libusb_backend_open]

Finds and opens one device on the bus: part of the `UsbBackend` trait, called by the supervisor for the first open and every reopen. `target` is borrowed and read only. It lists the devices, keeps those whose vendor and product ids match, tries to open each, reads the manufacturer, product and serial strings and accepts the first for which `UsbTarget::matches` is true. It then asks libusb to detach the kernel driver automatically (failure ignored) and claims `target.interface` when one is set. Errors are all `LinkDown`: listing failed; the claim failed; or, with the device not found, a message that says "is not plugged in" when no candidate had the ids, "cannot be opened ... check the udev rule" when candidates existed but could not be opened, and "no ... has the expected strings or serial number" otherwise. A device that cannot be opened is skipped, not fatal. Blocking; it must be called from the supervisor thread, not from a runtime worker. It performs no transfers.

## `crates/radioctl-protocols/src/si570_usb.rs`

### `pub fn word_from_hz(hz: Hz, multiplier: u8) -> Option<u32>` [@ANCHOR: radioctl:radioctl_si570_usb_word_from_hz]

Converts a receive frequency to the 32-bit word the by-value set-frequency request (0x32) carries: the oscillator frequency in MHz as 11.21 fixed point, that is `hz * multiplier * 2^21 / 1_000_000`, rounded to the nearest unit (about 0.5 Hz at 7 MHz). `hz` is the receive frequency in hertz; `multiplier` is the board's oscillator-to-receive ratio (4 for quadrature boards, 2 for the KTH, or the FiFi-SDR's own factor). Returns `None` when the word does not fit in 32 bits, that is above 2047 MHz of oscillator. The arithmetic is in 128 bits so it cannot overflow. A multiplier of 0 gives `Some(0)`; the engine constructor refuses a zero multiplier, so this does not occur in use. Pure, no errors, no state.

### `pub fn registers_from_hz(hz: Hz, multiplier: u8) -> Option<[u8; 6]>` [@ANCHOR: radioctl:radioctl_si570_usb_registers_from_hz]

Solves the Si570 divider settings on the host, for DG8SAQ firmware older than 0x0F00 that has no by-value request. The output frequency is `hz * multiplier`. It tries every HS_DIV from 4, 5, 6, 7, 9, 11 with N1 equal to 1 or an even number up to 128, keeps those whose DCO (output times HS_DIV times N1) lies between 4850 and 5670 MHz, and takes the one with the lowest DCO (the first found on a tie). RFREQ is the DCO divided by the nominal 114.285 MHz crystal as 10.28 fixed point, rounded. The six bytes are registers 7 to 12: HS_DIV code (3 bits), N1 minus one (7 bits), RFREQ (38 bits), big-endian. Returns `None` for a zero output or when no setting puts the DCO in the window. Pure, 128-bit arithmetic, no errors. The real crystal of a board may differ from nominal, so the result is only as exact as the crystal; unknown, check on a board.

### `pub fn hz_from_registers(registers: &[u8; 6], multiplier: u8) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_si570_usb_hz_from_registers]

Reads a frequency back from the six Si570 register bytes returned by request 0x3F, the inverse of `registers_from_hz`. `registers` is borrowed, registers 7 to 12 in order, big-endian; `multiplier` is the board ratio. It splits the 48-bit word into the HS_DIV code, N1 minus one and the 38-bit RFREQ and computes `crystal * RFREQ / (2^28 * HS_DIV * N1 * multiplier)` with the nominal 114.285 MHz crystal, rounded to the nearest hertz. Returns `None` when `multiplier` is 0 or the HS_DIV code is not one of 0, 1, 2, 3, 5, 7 (codes 4 and 6 are unused), which the engine reports as an unrecognised reply. Pure, no side effects, 128-bit arithmetic.

### `fn fifi_answer(&mut self, read: FifiRead, reply: &UsbReply) -> Inbound` [@ANCHOR: radioctl:radioctl_si570_usb_fifi_answer]

Decodes the answer to a FiFi-SDR parameter read; `read` says which parameter the pending request asked for and `reply` is the decoded USB reply (borrowed). Mode: one byte, 0 to 3 mapped to LSB, USB, AM, FM, else unrecognised. Width: four little-endian bytes, hertz. Signal: four bytes, capped at 65535. Volume: two bytes, signed 16-bit, 0 to 100 (an out of range value, including negative, is reported as 0, as Hamlib does). Preamp: one byte, nonzero gives step 1. VCO factor: four bytes; a value of 1 to 255 replaces the engine's multiplier (zero or absurd values keep the description's, to avoid dividing by zero) and the result is `Inbound::Ack`, not an observation. Any answer of the wrong length becomes `Inbound::Unrecognised` with the re-encoded reply. It mutates `self.multiplier` only for the VCO factor. Pure otherwise; called from `feed`.

### `fn open_requests(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_si570_usb_engine_open_requests]

The rig actor's open hook for this engine, also reached through `link_restored` after the device was reopened. For a FiFi-SDR it returns one request, a control-in read (request 0xAB, wIndex 11, 4 bytes) of the VCO factor, with reply kind `Ack`; it clears the receive buffer and records the pending read so `feed` will apply the factor to the multiplier. For the SoftRock, KTH, Peaberry and FA-SDR boards, which have nothing to read at open, it returns an empty list. The result is owned by the caller. It mutates the engine state (`pending`, `rx`) and does no I/O; the transfer is performed by the USB link. It sends nothing that keys the transmitter. Calling it again simply repeats the read (idempotent).

### `pub fn hz_from_word(word: u32, multiplier: u8) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_si570_usb_hz_from_word]

The inverse of `word_from_hz`: converts the 11.21 fixed-point oscillator word read back with request 0x3A into a receive frequency. `word` is the little-endian value already decoded to a number; `multiplier` is the board ratio. The result is `word * 1_000_000 / (multiplier * 2^21)` rounded to the nearest hertz. Returns `None` when `multiplier` is 0 (a division by zero is avoided) or when the result does not fit in a `u64`, which cannot happen for a 32-bit word. Pure and allocation free; used by the engine's `feed` on a frequency reply. A round trip through `word_from_hz` returns the original frequency to within a hertz at the frequencies the boards cover.

### `pub fn new(desc: RigDescription) -> Result<Si570UsbEngine, RigError>` [@ANCHOR: radioctl:radioctl_si570_usb_engine_new]

Builds the Si570 USB client engine from a rig description, which is taken by value and kept. It reads the description's USB block and copies the protocol and the multiplier (the oscillator-to-receive ratio, for example 4 for a quadrature board). The engine starts with nothing pending and an empty receive buffer. Errors, all `ErrorKind::Unsupported`: the description has no USB block ("not a USB description"); the protocol is not `Si570`, `Si570Legacy` or `Fifisdr`; or the multiplier is 0. No I/O is done. The multiplier may later be replaced by the FiFi-SDR's own VCO factor read at open. Called by the engine factory when a USB rig is created.

### `fn set_value(&self) -> u16` [@ANCHOR: radioctl:radioctl_si570_usb_set_value]

Returns the wValue of the set-frequency control request for this board. On the Si570 kits (SoftRock, KTH, Peaberry, FA-SDR and the DG8SAQ legacy firmware) it is 0x700 plus the Si570's I2C address 0x55, that is 0x755; on the FiFi-SDR it is 0. It takes no arguments beyond the engine, never fails and has no side effects; `encode` uses it for both the by-value request 0x32 and the register request 0x30. The value comes from Hamlib `si570avrusb.c` and has not been checked against a board (unknown on hardware; check before relying on it for a new kit).

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_si570_usb_engine_encode]

The `ProtocolEngine` hook that turns one operation into one USB control transfer frame, returned as `Request { bytes, reply }` (the bytes are an encoded `UsbTransfer`, the reply kind says whether an `Ack` or a given observation is expected). `op` is borrowed. Supported: get and set frequency on VFO A (by value, or as six registers on legacy firmware); `SetPtt`; and, on the FiFi-SDR only, mode, filter width, signal, AF gain (clamped to 100) and preamp step 0 or 1. Errors, all `Unsupported`: the capability is not in the description; the frequency is out of the Si570 range; a mode other than LSB, USB, AM, FM; a width over 32 bits; or any other operation. It stores the expected answer kind in `pending` and clears the receive buffer, so only one request may be outstanding; call `feed` before encoding the next. Transmit: `SetPtt` is a control-in request 0x50 with wValue 1 or 0 and is only produced for descriptions that list `set-ptt`; the daemon's lock, band check and transmit gate apply before this is called, and nothing here opens them. Unverified on hardware.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_si570_usb_engine_feed]

Takes bytes arriving from the USB link and returns what they mean. `bytes` are appended to an internal buffer, then each complete `UsbReply` frame is decoded and matched with the one pending request (the `now` argument is unused; there are no timers). A status of `Timeout` or `NoDevice` produces nothing (silence, so the actor ends the command with its own typed timeout). `Ok` gives `Ack` after a set, a frequency observation after a read (the word or the six registers converted to hertz on VFO A), or the result of `fifi_answer`; `Stall` or `Other` gives `Nak`; a reply with nothing pending, or an unparsable payload, gives `Inbound::Unrecognised` with the reply bytes. A malformed frame empties the buffer into one `Unrecognised` and stops. The pending slot is cleared on every reply. Pure apart from the engine's own buffer; never blocks.

### `pub fn new(desc: &RigDescription) -> Result<Si570Twin, RigError>` [@ANCHOR: radioctl:radioctl_si570_usb_twin_new]

Builds the virtual Si570 board used for tests and the virtual rig, in the state the description gives. `desc` is borrowed. It computes the oscillator word and the six registers for `default_frequency` with the board's multiplier, and sets PTT off. For a FiFi-SDR the twin also holds the parameters: VCO factor equal to the multiplier, mode 1 (USB), width 2400 Hz, volume 50, preamp 0, signal 0. Errors: `Unsupported` when the description has no USB block, or when the default frequency does not fit the word (above 2047 MHz of oscillator). If no register setting exists the registers are zeros; the register answer is then meaningless, but only the legacy protocol reads them. No I/O. The twin implements `UsbDevice`, so it plugs into the same link as a real board.

### `pub fn ptt(&self) -> bool` [@ANCHOR: radioctl:radioctl_si570_usb_twin_ptt]

Returns whether the virtual board is currently keyed: true after the twin has received a PTT request (0x50) with wValue 1, false after wValue 0 or at construction. It is a plain read of a boolean with no side effects, no locking (the twin is used from one thread through `&mut self` transfers) and no failure. It exists for tests, which assert that a keying request was or was not sent through the engine and that the transmit gate held it back. It reflects the twin, not any real transmitter.

### `fn control_in(&mut self, request_type: u8, request: u8, value: u16, index: u16, length: u16, _timeout: Duration) -> Result<Vec<u8>, UsbFailure>` [@ANCHOR: radioctl:radioctl_si570_usb_twin_control]

The twin's answer to a control-in transfer (`UsbDevice::control_in`); the timeout is ignored. Arguments are the standard setup packet fields: type 0xC0 for the vendor requests, the request number, wValue, wIndex and the expected length in bytes. Answers, and only with exactly the length shown: on a FiFi-SDR twin, request 0xAB with wValue 0 reads the parameter at wIndex 11 (VCO factor, 4 bytes), 14 (volume, 2), 15 (mode, 1), 16 (width, 4), 17 (S-meter, 4) or 19 (preamp, 1), little-endian; request 0x3A (index 0, 4 bytes) returns the frequency word except on legacy firmware; request 0x3F with wValue 0x55 (index 0, 6 bytes) returns the registers on legacy firmware only; request 0x50 (index 0, 3 bytes) with wValue 0 or 1 sets the PTT flag and returns three zero bytes, on the Si570 protocols. Anything else returns `UsbFailure::Stall`. The PTT request mutates the twin, nothing else does.

## `crates/radioctl-protocols/src/funcube.rs`

### `pub fn command_report(command: u8, args: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_funcube_command_report]

Builds the 64-byte HID report the FUNcube dongle takes: byte 0 is `command` (for example 0x65 set frequency in hertz), bytes 1 onward are `args` copied in order, and the rest is zero. `args` is borrowed and read only; at most 63 bytes are used and any extra are silently ignored. The returned vector is always exactly 64 bytes (`REPORT`). It is used for requests by `exchange` and by the twin to build its answers (byte 1 then carries the success flag). Pure, no errors, no side effects.

### `pub fn new(desc: RigDescription) -> Result<FuncubeEngine, RigError>` [@ANCHOR: radioctl:radioctl_funcube_engine_new]

Builds the FUNcube client engine from a description, taken by value. It succeeds only when the description's USB protocol is `funcube`; it then records whether the USB product id is 0xFB31 (the Dongle Pro+, with LNA, mixer and IF gain controls) or anything else (the original Dongle, with a single LNA setting). The engine starts assuming current firmware that has the hertz commands (`legacy` false), with nothing pending and an empty buffer. Returns `ErrorKind::Unsupported` ("not a FUNcube description") for any other description or one without a USB block. No I/O. The engine is a receive-only device: nothing in it can key a transmitter.

### `fn gain_value(&self, level: Level, value: u16) -> Result<(u8, u8), RigError>` [@ANCHOR: radioctl:radioctl_funcube_gain_value]

Chooses the command byte and its one argument byte that set `level` to `value` on this dongle. For `Preamp` and `Attenuator` the value is first converted to a wire step through `attenuation::step_of` using the description (the attenuator value is decibels); other levels use the value as a step that must fit in a `u8`. Pro+: `Preamp` step 0 or 1 goes to the LNA command 0x6E as 0 or 1; `Attenuator` step 0 or 1 goes to the mixer command 0x72 as 1 when the step is 0 (mixer gain on, full gain) and 0 for step 1; `RfGain` goes to the IF command 0x75, clamped to 59. Original Dongle: `Preamp` steps 0 to 6 and `Attenuator` steps 0 to 2 map to LNA codes through two tables. Anything else, or a step outside a table, is `Unsupported` ("no such level or step on this dongle"). Pure.

### `fn gain_read(&self, level: Level) -> Result<(u8, Reading), RigError>` [@ANCHOR: radioctl:radioctl_funcube_gain_read]

Chooses the read command and the way the answer byte is decoded for `level`. Pro+: `Preamp` reads 0x96 (LNA), `Attenuator` reads 0x9A (mixer), `RfGain` reads 0x9D (IF gain). Original Dongle: `Preamp` and `Attenuator` both read 0x96 and differ only in how the code is decoded (different tables). The `Reading` value is stored in the pending state so that `feed` can convert the answer byte back to a level or step. Any other level returns `Unsupported` ("no such level on this dongle"); the original Dongle has no RF gain read and the Pro+ no signal read. Pure, no I/O, never touches the transmitter.

### `fn exchange(command: u8, args: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_funcube_exchange]

Builds the whole USB request frame for one dongle command: an encoded `UsbTransfer::InterruptExchange` that writes the 64-byte report from `command_report(command, args)` to interrupt OUT endpoint 0x02 and then reads 64 bytes from interrupt IN endpoint 0x82. `command` is the command byte (set, read, gain, RSSI) and `args` the argument bytes, borrowed. The returned bytes are owned and go straight into `Request.bytes`. It is an associated function with no engine state, pure and infallible. One frame means the link performs write and read as a single transfer, so no second request can interleave between them.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_funcube_engine_encode]

Turns one operation on VFO A into a dongle request. Supported: get frequency (0x66), set frequency in hertz (0x65, 32-bit little-endian), `SetLevel` and `GetLevel` for the gain controls (through `gain_value` and `gain_read`), and `GetSignal` on the original Dongle (RSSI 0x68). If an earlier set was refused (firmware flagged `legacy`), set frequency uses command 0x64 with 24 bits of kilohertz (frequency divided by 1000, rounded down) and get frequency is refused, since old firmware cannot read it back. Errors, all `Unsupported`: capability missing from the description; a frequency over 32 bits (or 24 bits of kHz); a gain error; the Pro+ signal read; any other operation. It sets `pending` and clears the buffer, so one request at a time. Receive only; there is no transmit path.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_funcube_engine_feed]

Decodes reply frames from the link (`bytes`; `_now` unused) into events. For each complete `UsbReply` it takes the pending request. A `Timeout` or `NoDevice` status is silence. A reply is accepted when the status is `Ok`, answer byte 0 repeats the command and byte 1 is 1 (`SUCCESS`); otherwise it is `Nak`. If the refused command was the hertz set (0x65), the engine marks the firmware legacy so the next set uses 0x64. An accepted set gives `Ack`; a frequency read gives hertz from bytes 2 to 5 little-endian; a gain read converts answer byte 2 to a step or dB through `attenuation::value_of` (an unknown step is `Unrecognised`) or to RF gain or the raw RSSI as a signal. A reply with nothing pending or a short payload is `Unrecognised`; a malformed frame empties the buffer. Pure, never blocks.

### `pub fn new(desc: &RigDescription, legacy: bool) -> Result<FuncubeTwin, RigError>` [@ANCHOR: radioctl:radioctl_funcube_twin_new]

Creates the virtual FUNcube dongle in the description's default state. `desc` is borrowed; `legacy` true makes the twin behave as an older dongle that refuses the hertz commands (0x65 and 0x66) and only accepts the kilohertz set (0x64). The frequency starts at the description's default frequency, which must fit in 32 bits, otherwise `Unsupported`. The Pro+ is recognised by product id 0xFB31. The LNA starts at 4 on the original Dongle (its off code) and 0 on the Pro+; mixer, IF gain and RSSI start at 0 (`set_rssi` changes the RSSI). Pure, no I/O. The twin answers interrupt writes and reads, so it can sit behind the same link as a real dongle in tests.

### `pub fn frequency(&self) -> Hz` [@ANCHOR: radioctl:radioctl_funcube_twin_frequency]

Returns the frequency, in hertz, to which the virtual dongle is currently tuned. It starts at the description's default and changes whenever the twin accepts a set-frequency command: 0x65 stores the hertz value, 0x64 stores the kilohertz value times 1000 (saturating). It is a read of one field with no side effects, no locking and no failure; tests use it to confirm what the engine actually sent, including that old firmware received the kilohertz form. It reports the twin's state, not a real dongle.

## `crates/radioctl-protocols/src/elektor507.rs`

### `pub fn frequency(self) -> f64` [@ANCHOR: radioctl:radioctl_elektor507_pll_frequency]

Computes the receive frequency, in hertz, that a set of PLL settings produces: pump `p` times the 10 MHz reference divided by the counter `q`, divided by divider `div`, divided by four (the quadrature detector divides the oscillator by four). `self` is the `Pll` copied by value. The result is a floating point number and is not rounded; for the power-on settings `Pll::SAFE` (p 8, q 2, div 8) it is 1.25 MHz. `q` and `div` are never zero for settings from `solve` or `SAFE`; zero would give infinity or NaN, so hand-built values must keep them positive. Pure and free of side effects; used by tests to check how close `solve` came to the requested frequency.

### `pub fn solve(self, hz: u64) -> Pll` [@ANCHOR: radioctl:radioctl_elektor507_pll_solve]

Finds the settings nearest to a wanted receive frequency. `hz` is in hertz (the engine only passes 30 kHz to 29.999 MHz); the target oscillator is four times that. For every reference counter `q` from 2 to 40 and every pump `p` that keeps the voltage-controlled oscillator between 100 and 400 MHz, it picks the divider (2 to 127) nearest to the target and keeps the candidate with the smallest absolute error. `self` is the starting best, so it is returned unchanged if nothing beats it; the engine passes its current settings. Cost: a few thousand candidates of floating point arithmetic per call, trivial. Pure; it never fails and returns the best effort even when the error is large. The reference is fixed at 10 MHz (Hamlib has a setting for it, not implemented here).

### `pub fn registers(self) -> [(u8, Vec<u8>); 3]` [@ANCHOR: radioctl:radioctl_elektor507_pll_registers]

Encodes PLL settings as three clock-chip register writes, each a (first register, values) pair. The first group, at 0x40, has three bytes: the pump range code (from `p`: 0 to 44, 45 to 479, 480 to 639, 640 to 799, larger) and the high bits of half the pump total less four, then the low byte of that, then the counter `q` minus two with the low bit of `p` in bit 7. The second, at 0x0C, is divider 1: 8 for a divider of 2, 6 for 3, otherwise the divider itself. The third, at 0x46, is the clock source low bits: 0x87 for a divider of 2, 0xC7 for 3, 0x47 otherwise. `self` is by value. Pure; arithmetic saturates rather than panics on out-of-range fields. The engine turns the groups into I2C pin samples; nothing is written here.

### `pub fn new(desc: RigDescription) -> Result<Elektor507Engine, RigError>` [@ANCHOR: radioctl:radioctl_elektor507_engine_new]

Builds the Elektor SDR-USB engine from a description, taken by value. It succeeds only when the USB protocol is `elektor507`; otherwise it returns `ErrorKind::Unsupported` ("not an Elektor SDR-USB description"). The new engine starts with both I2C pins high and the input multiplexer and attenuator at 0 (the pin byte `PORT_START`), the safe PLL settings (`Pll::SAFE`) and an empty receive buffer. No I/O happens until the rig actor calls `open_requests`. The board is receive only; the engine has no operation that keys a transmitter.

### `fn setup(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_elektor507_engine_setup]

Builds the open sequence of four requests, all expecting an `Ack`: three FTDI control transfers (reset, request 0; bit-bang mode with all eight pins outputs, request 0x0B with value 0x01FF; baud rate value 49230, request 3), then one bulk write to endpoint 0x02 of the pin samples that program the clock chip over I2C with the output enable (register 0x09), divider 1 (0x0C), clock source registers (0x44 to 0x46) and divider 2 source (0x47). It first resets the engine's pin byte and PLL settings to their start values, so it is idempotent and is also what a reconnect uses. The requests are returned owned. It does no I/O itself; the link performs them in order. The attenuator and multiplexer return to 0 dB and no antenna path until the next set. Values come from Hamlib and are unverified on a board.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_elektor507_engine_encode]

Encodes a set-frequency or set-attenuator operation as one bulk write of pin samples to endpoint 0x02, expecting an `Ack`. Frequency on VFO A (30 kHz to 29.999 MHz): picks the multiplexer (the low-pass path up to 1.6 MHz, the high-pass path above), solves new PLL settings from the current ones, and joins the register writes of the PLL into one transfer. Attenuator: the value is decibels and must be 0, 10 or 20 (two pins). Errors, all `Unsupported`: capability not in the description; frequency outside the range; any other attenuator value; reads of frequency or level ("the board cannot be read back", the daemon cache holds the last value); any other operation. It mutates the stored pin byte and PLL settings before the transfer is sent, so a transfer that later fails leaves the engine's view ahead of the board until the next set or reopen. Receive only.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_elektor507_engine_feed]

Interprets reply frames from the link (`bytes`; `_now` is unused). Frames are decoded from an internal buffer; for each, an `Ok` status gives `Inbound::Ack`, a `Stall` or `Other` status gives `Inbound::Nak`, and `Timeout` or `NoDevice` gives nothing (silence, so the actor ends the command with its own typed timeout). Unlike the other engines it keeps no pending request, because every request here (the setup transfers and the bulk writes) expects only an acknowledgement. A malformed frame places the whole buffer in one `Unrecognised` event and stops. Pure and non-blocking apart from the buffer.

### `fn open_requests(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_elektor507_engine_open_requests]

Called by the rig actor when the device is first opened. It returns the same four requests as `setup`: three FTDI control transfers (reset, bit-bang mode, baud rate) and the bulk write that programs the clock chip's output enable, divider 1, clock source and divider 2 source registers. The engine's pin byte and PLL settings are reset by this call. The requests are owned by the caller and performed in order by the USB link; each expects an `Ack`. It never fails, does no I/O itself and keys nothing. The board starts with its safe PLL settings, so a frequency must be set afterwards before reception is meaningful.

# radioctl function anchors, batch 03

Function anchors for the `radioctl-protocols` crate: the Elektor SDR-USB twin and engine tail, the FUNcube twin, and the Microtelecom Perseus firmware loader, front-end control and virtual board, written to the ADR 0113 standard. Tests cite each anchor with `Tests [@ANCHOR: ...]`.
Everything here is receive only. Nothing in these functions can key a transmitter, so the lock, band check and transmit gate do not apply. Behaviour is taken from Hamlib and libperseus-sdr facts and is not verified against a real board.

## `crates/radioctl-protocols/src/elektor507.rs`

### `fn link_restored(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_elektor507_engine_link_restored]

Called by the rig actor after the USB link reopened the device following a loss. The board forgot its state when it was unplugged or reset, so the whole open sequence is sent again: three FTDI control transfers (reset, bit-bang mode, baud) and the bulk write that reprograms the clock chip. It returns the same four requests as `open_requests`, by calling the same internal `setup`, which also resets the engine's pin byte and PLL settings to their start values. The requests are owned by the caller; each expects an `Ack`. No arguments, no errors, no I/O here. The frequency and attenuator must be set again afterwards; the daemon cache holds the last values, but this function does not replay them.

### `pub fn new(desc: &RigDescription) -> Result<Elektor507Twin, RigError>` [@ANCHOR: radioctl:radioctl_elektor507_twin_new]

Creates the virtual Elektor SDR-USB board for tests and the virtual rig. `desc` is borrowed and only checked: the USB protocol must be `elektor507`, otherwise the result is `ErrorKind::Unsupported` ("not an Elektor SDR-USB description"). The twin starts "not set up yet": all 256 clock-chip registers are zero, bit-bang mode is off, both I2C lines are high, the pin byte is the idle value and no PLL register has been written, so `frequency` returns `None`. It implements `UsbDevice` and decodes the bulk pin samples written by the engine back into I2C register writes. No I/O, no randomness.

### `pub fn register(&self, index: u8) -> u8` [@ANCHOR: radioctl:radioctl_elektor507_twin_register]

Reads one register of the virtual clock chip. `index` is the register number, 0 to 255; the twin keeps a 256-byte register file that is filled only by I2C writes addressed to the chip address 210. A register never written reads 0. The result is the value last written to that register. It is a plain read of `&self` with no side effects and cannot fail (an index outside the array would also give 0, but every `u8` is in range). Tests use it to check that the engine programmed, for example, register 0x0C (divider 1) and the pump registers 0x40 to 0x42 as intended.

### `pub fn frequency(&self) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_elektor507_twin_frequency]

Recovers the receive frequency the virtual clock chip's registers describe, in hertz. Returns `None` until at least one of the pump registers 0x40 to 0x42 has been written (`pll_written`). Otherwise it reads the pump range and total from registers 0x40 and 0x41 and the low pump bit and counter from 0x42, takes divider 1 from register 0x0C (or the fixed /2 and /3 selection in the top two bits of register 0x46), builds a `Pll` and returns its frequency rounded to the nearest hertz. The divider is forced to at least 1 so a zero register cannot divide by zero. Pure read, no side effects. It is the independent check that the engine's register encoding and the twin's decoding agree.

### `pub fn attenuation_db(&self) -> u16` [@ANCHOR: radioctl:radioctl_elektor507_twin_attenuation]

Returns the attenuation, in decibels, that the two attenuator pins of the virtual board currently select. The pins are bits 5 and 6 of the last pin byte seen on the bulk endpoint: 1 means 10 dB, 2 means 20 dB, and 0 or 3 mean 0 dB (code 3 is not used by the real board). It reads the stored pin byte with no side effects and cannot fail. Note that the pin byte changes with every sample, including the I2C clock and data samples of a register write, but the engine keeps bits 5 to 7 unchanged during those, so the value stays stable between attenuator operations.

### `pub fn multiplexer(&self) -> u8` [@ANCHOR: radioctl:radioctl_elektor507_twin_multiplexer]

Returns the setting of the input multiplexer pins of the virtual board, 0 to 7: bits 2 to 4 of the last pin byte. The engine sets 1 for frequencies up to 1.6 MHz (the low-pass path) and 2 above (the high-pass path); 0 is the power-on state. It is a plain read with no side effects, no locking and no failure. Used by tests to confirm the automatic antenna-path choice. As with the attenuator, it is the twin's view of the pins, not a measurement of a real board.

### `fn control_out(&mut self, request_type: u8, request: u8, value: u16, _index: u16, data: &[u8], _timeout: Duration) -> Result<usize, UsbFailure>` [@ANCHOR: radioctl:radioctl_elektor507_twin_control_out]

The virtual FT232R's answer to a vendor control transfer (`UsbDevice::control_out`). It accepts only request type 0x40 with an empty data stage; the index and timeout are ignored. Request 0 (reset) with value 0 clears the bit-bang flag; request 0x0B (bit-bang mode) with value 0x01FF sets it; request 3 (baud) with value 49230 is accepted and does nothing. Any other request, value, request type or non-empty data returns `UsbFailure::Stall`. On success it returns `Ok(0)`, the number of data bytes. The only state it changes is the bit-bang flag; the twin does not insist on bit-bang mode before accepting bulk samples, so the flag is for tests to read.

### `fn bulk_write(&mut self, endpoint: u8, data: &[u8], _timeout: Duration) -> Result<usize, UsbFailure>` [@ANCHOR: radioctl:radioctl_elektor507_twin_bulk_write]

The virtual board's receiver of pin samples. `endpoint` must be the bulk OUT endpoint 0x02, otherwise `UsbFailure::Stall`. Each byte of `data` is one sample of the eight pins; the twin feeds them in order to a small I2C decoder that watches pin 0 (data) and pin 1 (clock): a falling data line with the clock high is a start, a rising one a stop, and bits are taken on clock rising edges, nine clocks to a byte. At a stop, a transaction whose first byte is the chip address 210 writes its following bytes into consecutive registers beginning at the register named by the second byte. The attenuator and multiplexer pins are stored from every sample. Returns the number of bytes consumed. Side effects: register file, pin state and the "pump written" flag. Never blocks; the timeout is ignored.

## `crates/radioctl-protocols/src/funcube.rs`

### `fn interrupt_write(&mut self, endpoint: u8, data: &[u8], _timeout: Duration) -> Result<usize, UsbFailure>` [@ANCHOR: radioctl:radioctl_funcube_twin_interrupt]

The virtual FUNcube dongle's handling of a 64-byte HID report written to the interrupt OUT endpoint. `endpoint` must be 0x02 and `data` exactly 64 bytes, otherwise `UsbFailure::Stall`. Byte 0 is the command. It executes: 0x65 set hertz (bytes 1 to 4 little-endian) and 0x66 read hertz, both only on a non-legacy twin; 0x64 set kilohertz (3 bytes, times 1000); 0x6E and 0x96 set and read the LNA code; on the Pro+ only, 0x72/0x9A the mixer and 0x75/0x9D the IF gain; and on the original Dongle only 0x68 RSSI. Anything else, including a hertz command on a legacy twin, gets a refusal. In every case it prepares the answer report held for the next `interrupt_read`: command in byte 0, success flag 1 or 0 in byte 1, any value from byte 2. Returns the data length. Mutates the twin's state and the stored answer; a second write replaces an unread answer.

## `crates/radioctl-protocols/src/perseus.rs`

### `pub fn in_dir(dir: &Path) -> PerseusFiles` [@ANCHOR: radioctl:radioctl_perseus_files_in_dir]

Builds the pair of file paths the Perseus loader will read from a directory: `fx2.hex` (the Intel HEX image of the FX2 firmware) and `fpga.rbs` (a raw FPGA bitstream for the wanted sample rate). `dir` is borrowed and only joined with those two names; nothing is opened, checked or created, so the paths may not exist. The result is an owned `PerseusFiles` with the two fields `fx2_hex` and `fpga_image`. Pure, cannot fail. A missing file is detected later, by `open_ready`, which reports the path it looked at. radioctl does not ship the firmware or the images; the user supplies them.

### `pub fn default_dir(env_dir: Option<&str>, home: Option<&str>) -> Option<PathBuf>` [@ANCHOR: radioctl:radioctl_perseus_files_default_dir]

Chooses the directory to look in for the Perseus firmware files. `env_dir` is the value of the environment variable `RADIOCTL_PERSEUS_FIRMWARE_DIR`, `home` the value of `$HOME`; both are passed in so the function does not read the environment itself. A non-empty `env_dir` wins. Otherwise a non-empty `home` gives `<home>/.config/radioctl/firmware/perseus`. If neither is usable it returns `None`, and the caller must report that no firmware directory is known. An empty string counts as unset. Pure, no file system access, no errors.

### `fn usb_err(what: &str, failure: UsbFailure) -> RigError` [@ANCHOR: radioctl:radioctl_perseus_usb_err]

Turns a USB failure into the typed error the daemon reports. `what` is a short phrase naming the step ("FPGA reset", "writing FX2 RAM"); `failure` is the `UsbFailure` from the device. The kind is chosen by the failure: `NoDevice` gives `LinkDown` (the link will reopen the board), `Timeout` gives `Timeout`, and `Stall` or `Other` gives `Rejected`. The detail text is `perseus: <what>: <failure>`, so an operator can see which step failed. Pure, allocates the message, cannot fail. All loader and control steps use it so error kinds stay consistent.

### `fn read_file(path: &Path, what: &str) -> Result<Vec<u8>, RigError>` [@ANCHOR: radioctl:radioctl_perseus_read_file]

Reads a whole firmware file into memory (blocking file read). `path` is the file; `what` names it in the message ("FX2 firmware (Intel HEX)", "FPGA bitstream"). On any I/O error (missing, unreadable) it returns `ErrorKind::LinkDown` with a long message that gives the exact path, the operating system's reason, the directory where `fx2.hex` and `fpga.rbs` belong, the environment variable that can name another directory, and where the vendor publishes the files. The message is deliberately helpful because the board cannot start without the user's files. No size limit is applied. It is called by `open_ready`.

### `pub fn parse_intel_hex(text: &str) -> Result<Vec<HexRecord>, RigError>` [@ANCHOR: radioctl:radioctl_perseus_parse_intel_hex]

Parses the text of an Intel HEX file into data records for the FX2 RAM. `text` is borrowed; blank lines are ignored. Each line must start with a colon, have a length that matches its length byte, be hexadecimal and sum to zero modulo 256 (the checksum). Record type 00 (data) gives a `HexRecord` with a 16-bit address and the bytes; type 01 (end) marks the end; any other type (extended addressing and so on) is refused because the FX2 image does not need it. Errors, all `ErrorKind::Malformed` with the line number: no colon, bad length, not hexadecimal, bad checksum, unsupported type, data after the end record; also no end record, or no data records at all. Pure, no I/O.

### `pub fn load_fx2(dev: &mut dyn UsbDevice, records: &[HexRecord]) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_perseus_load_fx2]

Loads the FX2 firmware into the board's RAM over the blank board's USB device. `dev` is the open blank device (borrowed mutably); `records` are the parsed HEX records. It sends vendor request 0xA0 (the Cypress firmware load): value 0xE600 with byte 1 holds the CPU in reset, then each record's data is written at its address, then value 0xE600 with byte 0 releases the CPU. Each transfer has a one second timeout. A failure to hold the reset or to write RAM returns an error (`usb_err` kinds). The release is expected to fail because the board renumbers on its own as the firmware starts, so its result is ignored. Blocking. After success the caller must wait for the board to appear with its new product id.

### `pub fn configure_fpga(dev: &mut dyn UsbDevice, image: &[u8], wait: &mut dyn FnMut(Duration)) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_perseus_configure_fpga]

Loads the FPGA bitstream into the running board. `dev` is the running device (borrowed mutably), `image` the raw bitstream (not modified) and `wait` a callback that pauses for the given duration (a real caller sleeps; a test records it). Sequence: bulk write of the reset command; the image in pieces of 63 bytes, each prefixed with the config command byte; the check command; `wait(50 ms)`; then a 2-byte status read, whose second byte must be 1 (the DONE line). Errors: an empty image is `Malformed`; any transfer error is mapped with `usb_err`; a status byte other than 1 is `Rejected` and shows the status bytes. Nothing more is sent after the first failure. Blocking; transfers have a one second timeout.

### `pub fn is_pre_series(&self) -> bool` [@ANCHOR: radioctl:radioctl_perseus_product_id_is_pre_series]

Says whether the board is one of the few pre-production units. A production unit carries product code `0x8014` in its EEPROM record; any other product code counts as pre-series. The distinction matters because pre-series units drive the attenuator with inverted sense, and `set_attenuator_db` flips the two code bits for them. Takes `&self`, returns a boolean, has no side effects and cannot fail. Whether a given code really identifies a pre-series unit is taken from the vendor library; unknown on real boards.

### `pub fn read_product_id(dev: &mut dyn UsbDevice) -> Result<ProductId, RigError>` [@ANCHOR: radioctl:radioctl_perseus_read_product_id]

Reads the 12-byte product record the board keeps in its EEPROM, at address 8, using the board's command endpoints. `dev` is the running device (borrowed mutably). It writes the EEPROM read command (06, address little-endian, length 12) to bulk OUT 0x01 and reads 14 bytes from bulk IN 0x81. Byte 1 of the reply must be 1 or the read failed (`Rejected`, "the EEPROM read failed"). The record is parsed as serial number (u16 little-endian), product code (u16), hardware release (u8) and hardware version (u8). A reply too short gives `Malformed`. Transfer errors map through `usb_err`. Blocking, one second timeouts; read only, nothing on the board changes.

### `pub fn target(blank: bool) -> UsbTarget` [@ANCHOR: radioctl:radioctl_perseus_target]

Describes the USB device to open. `blank` true selects the board with no firmware (product id 0x8613) and false the board with the Perseus firmware running (0x325C); the vendor id is Cypress's 0x04B4. The target has no manufacturer, product or serial constraint (any matches) and asks for interface 0 to be claimed. The result is an owned `UsbTarget` that is handed to a `UsbBackend::open`. Pure, cannot fail. After the firmware loads, the board disappears and re-appears under the other product id, which is why the loader opens both.

### `pub fn preselector_for(frequency_hz: f64, enabled: bool) -> u8` [@ANCHOR: radioctl:radioctl_perseus_preselector_for]

Chooses the front-end preselector filter for a tuning frequency. `frequency_hz` is in hertz; `enabled` false means preselection is off. The board has ten band filters with upper cut-offs from 1.7 MHz to 32 MHz (table `PRESELECTOR_CUTOFFS_HZ`). The result, 0 to 9, is the index of the first filter whose cut-off is above the frequency; above the last one, or when `enabled` is false, it is 10 (`PRESELECTOR_WIDE`, the wide-band path). A negative or NaN frequency is below the first cut-off (negative) or compares false (NaN, which ends as 10). Pure, no errors. The result goes into the low nibble of the front-end port.

### `pub fn tuning_word(frequency_hz: f64) -> u32` [@ANCHOR: radioctl:radioctl_perseus_tuning_word]

Computes the 32-bit tuning word of the digital down-converter: `frequency / 80 MHz * 2^32`, with the nominal 80 MHz ADC clock, truncated toward zero (not rounded). `frequency_hz` is in hertz; valid input is 0 to 40 MHz (half the ADC clock), where the result is 0 to 2^31. A negative value saturates to 0 and a value above the range saturates at `u32::MAX` because of Rust's float-to-integer conversion, but `set_frequency` refuses such input before it gets here. Resolution is about 0.019 Hz. The real ADC clock may differ from nominal; unknown, check on a board. Pure.

### `pub fn open_ready(backend: &dyn UsbBackend, files: &PerseusFiles, wait: &mut dyn FnMut(Duration)) -> Result<PerseusControl, RigError>` [@ANCHOR: radioctl:radioctl_perseus_open_ready]

Brings a Perseus to a usable state; the real open path, called by the loader backend. `backend` opens USB devices (shared, read only); `files` names the user's firmware files; `wait` pauses for the given time (4 seconds for renumbering, 50 ms for the FPGA check). It first opens the running board. If that fails it opens the blank board, reads `fx2.hex`, parses it, loads it with `load_fx2`, drops the blank handle and then up to three times waits and tries the running board again; none found is `LinkDown` ("did not renumber"). Next it reads the EEPROM product record, reads `fpga.rbs` and configures the FPGA. A missing file is an error naming the path. Returns a `PerseusControl` with front-end, serial-IO and preselector state at zero. Blocking, takes seconds. Receive only.

### `pub fn product(&self) -> ProductId` [@ANCHOR: radioctl:radioctl_perseus_product]

Returns a copy of the product record (serial number, product code, hardware release and version) that was read from the board's EEPROM when it was opened. No board access happens; it is the value cached by `open_ready` or `from_device`. `ProductId` is `Copy`. It cannot fail and has no side effects. The serial number is what a service can use to tell two boards apart, and `is_pre_series` on the record says whether the attenuator code is inverted.

### `fn port_e(&mut self, value: u8) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_perseus_port_e]

Writes the front-end Port E byte: the attenuator code in the high nibble, the preselector filter in the low nibble. `value` is the whole byte. It sends the command 04 plus the byte on bulk OUT 0x01 with a one second timeout; no status is read back. Only after a successful write does it store the byte as the current front-end state, so a failure leaves the cached state unchanged. Errors come through `usb_err` as `LinkDown`, `Timeout` or `Rejected` with the text "front-end port write". Private; the public callers are `set_attenuator_db`, `set_frequency` and the shutdown path. Blocking.

### `fn sio(&mut self, control: u8, word: u32) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_perseus_sio]

Writes the five-byte serial-IO word to the FPGA: the SIO command 03, the control byte (bit 1 ADC dither, bit 2 ADC preamplifier) and the 32-bit tuning word little-endian, then reads back a status frame of the same length from bulk IN 0x81. `control` and `word` are the new values. A status of a different length is `Malformed` ("short serial-IO status"); transfer errors come through `usb_err`. Only after success are the cached control byte and word updated. One second timeouts; blocking. Private; used by `set_adc` and `set_frequency`, which each pass the part they are not changing.

### `pub fn set_attenuator_db(&mut self, db: u8) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_perseus_set_attenuator_db]

Sets the front-end input attenuator. `db` must be 0, 10, 20 or 30; anything else is `ErrorKind::Unsupported` ("no N dB attenuator step") and nothing is sent. The step index 0 to 3 is placed in the high nibble of the Port E byte; a pre-series unit takes the inverted code (index XOR 3). The low nibble (the preselector filter) is kept as it was. It then writes Port E through `port_e`, which can fail with link errors. Blocking, one bulk write. Side effect on hardware: changes the receive attenuation only, never a transmit path.

### `pub fn set_adc(&mut self, dither: bool, preamp: bool) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_perseus_set_adc]

Switches the ADC dither and the ADC preamplifier ("gain high") on or off. `dither` sets bit 1 and `preamp` bit 2 of the serial-IO control byte; the other bits of the cached control byte, and the current tuning word, are kept. It sends the combined word through `sio`, so the board answers with a status frame; errors are those of `sio` (`Malformed` for a short status, `LinkDown`, `Timeout` or `Rejected` for the transfer). Blocking, one bulk write and read. Calling it twice with the same values is harmless. It changes receive hardware only.

### `pub fn set_frequency(&mut self, frequency_hz: f64, preselector: bool) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_perseus_set_frequency]

Tunes the digital down-converter. `frequency_hz` must be from 0 to 40 MHz inclusive (half the 80 MHz ADC clock); a value outside the range, or NaN, is `Unsupported` ("outside 0 to 40 MHz") and nothing is sent. `preselector` true selects the matching band filter, false the wide-band path. It sends the tuning word with `sio`, keeping the control byte; then, only if the filter index differs from the last one written, rewrites Port E with the new filter in the low nibble and the attenuator code unchanged. Transfer errors propagate; if the filter write fails the new tuning word has already been written. Blocking, up to two bulk exchanges. Receive only: it tunes the receiver, not a transmitter.

### `pub fn shutdown(&mut self) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_perseus_shutdown]

Puts the board into its safe idle state by sending the vendor shutdown command (08) on bulk OUT 0x01: the board resets the FPGA, disconnects the preselector filters and sets 30 dB attenuation. No status is read. Errors come through `usb_err` ("shutdown"). The cached front-end and tuning state in `PerseusControl` is not changed by this call, so a caller that keeps using the control must set the frequency and attenuator again. Blocking, one bulk write. Useful before dropping the board or on an unrecoverable fault; it is receive-side hardware only.

### `pub fn new(running: bool, serial: u16, fpga_done: bool) -> VirtualPerseusBackend` [@ANCHOR: radioctl:radioctl_perseus_virtual_new]

Creates the virtual Perseus board on a virtual USB bus. `running` false makes it appear as the blank board (0x8613) until the FX2 firmware has been loaded, true as the running board (0x325C). `serial` is the serial number returned in the EEPROM record; the record is a production unit (product code 0x8014, hardware release 2, version 1). `fpga_done` decides whether the DONE line reads true after a non-empty bitstream. The result is clonable and all clones are the same board, protected by a mutex (a poisoned lock is recovered). No I/O.

### `pub fn state(&self) -> VirtualPerseusState` [@ANCHOR: radioctl:radioctl_perseus_virtual_state]

Returns a snapshot, taken under the board's lock, of what the virtual board has been told: whether the firmware is running, each FX2 RAM write as (address, data), the FPGA bitstream received since the last reset, the last Port E byte, the last serial-IO control byte and tuning word, whether shutdown was received, and every command frame in order. The returned value is an owned clone, so later activity does not change it, and cloning copies the bitstream and frames. It never fails and has no effect on the board. For tests.

### `fn open(&self, target: &UsbTarget) -> Result<Box<dyn UsbDevice>, RigError>` [@ANCHOR: radioctl:radioctl_perseus_virtual_open]

The `UsbBackend` entry of the virtual board. It succeeds only when the target's vendor id is 0x04B4 and the product id matches the board's present state: 0x8613 (blank) while no firmware has loaded, 0x325C (running) afterwards. Any other target, or the wrong one for the current state, returns `ErrorKind::LinkDown` with "usb:... is not on the bus". On success it returns a handle that shares the board's state (`Arc`); several handles can be open. This models the renumbering: the blank id vanishes once the firmware runs. No blocking.

### `fn control_out(&mut self, request_type: u8, request: u8, value: u16, _index: u16, data: &[u8], _timeout: Duration) -> Result<usize, UsbFailure>` [@ANCHOR: radioctl:radioctl_perseus_virtual_control_out]

The virtual board's FX2 firmware-load endpoint. Once the firmware is running it returns `UsbFailure::NoDevice` (the blank board has gone). Otherwise it accepts only request type 0x40, request 0xA0, else `Stall`. Value 0xE600 with data `[1]` holds the CPU in reset; with `[0]` it releases it, which sets `firmware_running` true but only if the CPU was held and at least one RAM write happened, otherwise `Stall`. Any other value is a RAM write, recorded as (address, bytes) and accepted only while held, else `Stall`. Returns the data length. The index and timeout are ignored. State change: the board's held flag, RAM writes and running flag.

### `fn bulk_write(&mut self, endpoint: u8, data: &[u8], _timeout: Duration) -> Result<usize, UsbFailure>` [@ANCHOR: radioctl:radioctl_perseus_virtual_bulk_write]

The virtual board's command endpoint. It stalls unless the firmware is running and `endpoint` is 0x01. Each frame is logged in the state, then acted on by its first byte: 00 appends to the FPGA bitstream, 01 clears it, 02 queues a status of `[02, done]` (done is the configured flag and a non-empty bitstream), 03 records the control byte and tuning word and queues the frame as its status, 04 records the Port E byte, 06 queues the EEPROM record (`[06, 01]` plus the first N bytes), 08 sets the shutdown flag. An unknown command, or a frame too short for it, is `Stall`. Returns the frame length. Status frames wait in a queue for `bulk_read`. Timeout ignored.

### `fn bulk_read(&mut self, endpoint: u8, _length: usize, _timeout: Duration) -> Result<Vec<u8>, UsbFailure>` [@ANCHOR: radioctl:radioctl_perseus_virtual_bulk_read]

The virtual board's status endpoint. `endpoint` must be 0x81. It removes and returns the oldest queued status frame, whole, whatever `length` the caller asked for (the length and timeout are ignored). When the queue is empty, or the endpoint is wrong, it returns `UsbFailure::Timeout`, which is how a real read would end after no answer. It modifies the queue, so one read consumes one status; the caller must read after each command that produces one (FPGA check, serial-IO, EEPROM read). Commands that produce no status (Port E, shutdown, FPGA data) queue nothing.

# radioctl function anchors, batch 57

Function anchors for helpers in `radioctl-protocols`, `radioctl-station` and `radioctld` that had no anchor: the Elektor SDR-USB bit-bang sample builder and its virtual twin's decoder, the FiFi-SDR mode code and link restore, the virtual Yaesu `CF` set, the Yaesu split read command, the pinned-certificate verifier of the Home Assistant link, and two helpers of the daemon configuration parser. Tests cite each anchor with `Tests [@ANCHOR: ...]`, and every test runs on fakes and twins only.
Transmit safety in this batch: nothing here keys a transmitter. The Elektor board and the FiFi-SDR are receive only, the `CF` set only moves the virtual rig's clarifier state, the split read is a query, and the configuration helpers only parse text. The transmit gate stays with the rig actor.

## `crates/radioctl-protocols/src/elektor507.rs`

### `Samples::sda(&mut self, high: bool)` [@ANCHOR: radioctl:radioctl_elektor507_samples_sda]

Drives the I2C data line in the pin byte the engine keeps and records the result. `high` true sets the data bit (`PIN_SDA`), false clears it; every other bit of the pin byte, the clock, the multiplexer and the attenuator pins, is left alone. After the change the whole pin byte is pushed to the output sample list, one byte per call, which is what the bit-bang bulk write carries to the FTDI chip. No errors and no I/O. Called only by the other `Samples` methods.

### `Samples::scl(&mut self, high: bool)` [@ANCHOR: radioctl:radioctl_elektor507_samples_scl]

The same as `sda` for the I2C clock line: `high` sets or clears `PIN_SCL` in the engine's pin byte, keeps all other pins as they were, and appends the new pin byte to the sample list. Because the attenuator and multiplexer bits ride along unchanged in every sample, a register write never disturbs the antenna path or the attenuation. Pure, no errors, no I/O, called by `byte` and `write`.

### `Samples::byte(&mut self, value: u8)` [@ANCHOR: radioctl:radioctl_elektor507_samples_byte]

Clocks one byte out on the I2C lines, most significant bit first: for each of the eight bits it sets the data line to the bit, raises and lowers the clock. It then clocks the acknowledge slot, releasing the data line (high) and pulsing the clock once, because the board has no read-back and the twin treats that ninth clock as the end of the byte. `value` is any byte. It produces 27 samples per call and cannot fail. Used by `write` for the chip address, the register number and each data byte.

### `Samples::write(&mut self, register: u8, values: &[u8])` [@ANCHOR: radioctl:radioctl_elektor507_samples_write]

Builds the pin samples of one I2C write transaction to the clock chip: a start condition (data falls while the clock is high), the chip address 210, the register number, each value in `values`, then a stop condition (data rises while the clock is high). `register` is the first register and the values go to consecutive registers. An empty `values` still yields the address and register bytes. No errors, no I/O; the samples are appended to the builder and later sent by the engine as a bulk write. The twin's `sample` and `finish` decode exactly this shape.

### `Elektor507Engine::samples(&mut self, writes: &[(u8, Vec<u8>)]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_elektor507_engine_samples]

Turns a list of register writes, each a register number with its values, into the single sample byte string for one bulk transfer. It starts a `Samples` builder on the engine's own pin byte so the attenuator and multiplexer settings carry through, writes each transaction in order, and returns the collected samples; the engine's pin byte ends as the last sample left it. An empty list gives an empty result. No errors and no I/O; used by `setup` and by tuning to build what the bulk write sends.

## `crates/radioctl-protocols/src/elektor507.rs`, virtual board

### `Elektor507Twin::sample(&mut self, byte: u8)` [@ANCHOR: radioctl:radioctl_elektor507_twin_sample]

Decodes one pin sample the way the real clock chip would watch its lines. It stores the byte as the new pin state, then compares the clock and data lines with the previous sample: data falling while the clock is high starts a transaction and clears the byte buffer, data rising while the clock is high ends it through `finish`, and a rising clock edge inside a transaction shifts in one data bit. After eight bits the next rising clock is the acknowledge slot and completes the byte, which is pushed to the buffer. No errors; state only, no I/O. Called by the twin's bulk write for every sample.

### `Elektor507Twin::finish(&mut self)` [@ANCHOR: radioctl:radioctl_elektor507_twin_finish]

Closes an I2C transaction seen by the twin. When the first byte received was the chip address 210, the next byte is the register number and every later byte is stored in consecutive registers of the 256-byte register file; writes beyond register 255 are dropped. Touching any of the pump registers 0x40 to 0x42 sets `pll_written`, which lets `frequency` answer. A transaction for another address, or one with no register byte, changes nothing. The in-transaction flag is cleared either way. No errors, no I/O.

## `crates/radioctl-protocols/src/si570_usb.rs`

### `fifi_mode_code(mode: Mode) -> Option<u8>` [@ANCHOR: radioctl:radioctl_si570_usb_fifi_mode_code]

Maps a radioctl mode to the FiFi-SDR's mode byte: LSB (lower sideband) 0, USB 1, AM 2, FM 3. Every other mode, CW (continuous wave, Morse code) and the digital modes among them, gives `None`, and the engine turns that into an unsupported-operation error before anything is sent. `fifi_mode_of` uses the same table in reverse. Pure, no side effects. The board only selects its demodulator, so there is no transmit effect.

### `Si570UsbEngine::link_restored(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_si570_usb_engine_link_restored]

Called by the rig actor after the USB link reopened the device. It forgets any request in flight and any half-received reply bytes, then returns the engine's `open_requests` again, so the board's start-up reads (the FiFi-SDR's VCO factor) are repeated. No arguments, no errors, no I/O here; the requests are owned by the caller and each expects the reply kind the open sequence names. Boards with no open sequence return an empty list.

## `crates/radioctl-protocols/src/virtual_rig.rs`

### `VirtualRig::yaesu_cf_set(&mut self, body: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_yaesu_cf_set]

Applies the eight characters after `CF` of a clarifier set on the virtual Yaesu. `body` is `000`, receive flag, transmit flag, `000` to switch the receive and transmit clarifiers, or `001`, a sign and four digits to set the offset in hertz (plus or minus, at most 9999). It updates the twin's RIT (receiver incremental tuning), XIT (transmitter incremental tuning) and offset state and returns `None`, since a Yaesu does not answer a set. Anything else, a bad flag, sign or digits, gives `Some("?;")`. Only twin state changes; there is no radio and no transmission.

## `crates/radioctl-protocols/src/yaesu.rs`

### `YaesuEngine::split_read(&self) -> String` [@ANCHOR: radioctl:radioctl_yaesu_engine_split_read]

The CAT (computer-aided transceiver control) command that reads the split state and the transmit VFO (variable frequency oscillator, the tuning control): `ST;` on models whose description says `yaesu.split = "st"`, `FT;` on the others. No arguments, no errors, no I/O. `GetSplit` and `GetTxVfo` both use it, and the answer is decoded by the matching field. A read only; it never changes split or keys the radio.

## `crates/radioctl-station/src/tls.rs`

### `PinVerifier::verify_server_cert(...)` [@ANCHOR: radioctl:radioctl_station_tls_verify_server_cert]

The rustls hook that decides whether a Home Assistant server certificate is trusted when a SHA-256 pin is configured instead of a CA. It hashes the DER bytes of the end-entity certificate (`end_entity`) and accepts only if the digest equals the pin; intermediates, server name, OCSP data and time are ignored by design. A different certificate returns `rustls::Error::General` saying it does not match the pinned fingerprint. The handshake signature is still checked separately, so the peer must hold the key. No I/O.

## `crates/radioctld/src/config.rs`

### `serial_target(path, usb) -> Result<PortTarget, String>` [@ANCHOR: radioctl:radioctld_config_serial_target]

Chooses the port of a serial transport from the two optional config keys. Exactly one must be given: a non-empty `path` gives `PortTarget::Path`, a `usb` table gives `PortTarget::Usb` with its vendor id, product id and optional serial number. Neither, both, or an empty path give the error "serial transport needs exactly one of path and usb". Pure, no I/O; the port is not opened here.

### `line_control(lines: Option<&str>) -> Result<LineControl, String>` [@ANCHOR: radioctl:radioctld_config_line_control]

Reads a serial transport's `lines` setting: absent or `"managed"` gives `LineControl::Managed`, the default in which the daemon may use the modem control lines, and `"unmanaged"` gives `LineControl::Unmanaged` for ports where the lines must be left alone. Any other text is an error naming the value. Pure, no I/O.

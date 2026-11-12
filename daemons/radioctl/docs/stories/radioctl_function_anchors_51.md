# radioctl function anchors, batch 51

Function anchors for the `radioctl-station` crate, the Modbus coil relay driver in `modbus.rs`: the `ModbusBoard` description and its parsing, the sans-IO framing functions (LRC, CRC-16, ASCII and RTU frames, request PDUs, reply validation) and the `ModbusRelay` channel driver that runs them over any async byte stream. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]` or is a private helper reached through those tests.
Transmit safety in this batch: `ModbusRelay` switches real relays on a serial board, and a relay can power or route a transmit path, so the safety rules live in this file. No command is sent on a link until the board's safe state has been written in one function 15 frame and acknowledged (`write_gated`, `ensure_safe_state`, `apply_safe`); the driver's coil record is changed only by acknowledged writes; any failed or garbled reply makes the coils unknown; and `force_safe` (the emergency all-off) ignores the retry delay and is never held back. For a board whose status is not readable, function 1 is never put on the wire (`exchange`), and the assurance is `Recorded`, not confirmed. The serial stream is behind one async mutex held across each whole request and reply, because the bus is half duplex. The station interlock and transmit gate are applied above this driver, in `station.rs`.

## `crates/radioctl-station/src/modbus.rs`

### `pub fn safe_bits(&self) -> Vec<bool>` [@ANCHOR: radioctl:radioctl_station_modbus_safe_bits]

Returns the safe level of every coil in coil order, as a new vector the caller owns. If the board's `safe_state` is empty the result is all off, one `false` per coil (`coils` entries); otherwise it is a copy of `safe_state`. Argument: `&self`, read only. It cannot fail or panic, does no I/O, takes no lock and is idempotent. Used by `apply_safe` for the safe-state frame and by `safe_line`.

### `pub fn sainsmart_16ch() -> ModbusBoard` [@ANCHOR: radioctl:radioctl_station_modbus_sainsmart]

Returns the built-in description of the SainSmart 16-channel 9-36 V USB relay module (sku 101-70-208): Modbus ASCII framing, slave 254 (0xFE), 9600 baud, 16 coils starting at coil 0, reply timeout 1000 ms, inter-frame pause 20 ms. It is marked unverified on hardware (`verified` false) and its status is assumed unreadable (`status_readable` false), so function 1 is never sent and the safe state is every relay off (16 entries). No arguments; cannot fail; no I/O or lock. Each call returns a fresh value the caller may adjust.

### `pub fn from_toml(text: &str) -> Result<ModbusBoard, StationError>` [@ANCHOR: radioctl:radioctl_station_modbus_from_toml]

Parses a board description from TOML text (borrowed) into a `ModbusBoard`. Fields without a default must be present; `coil_base`, `status_readable` (default true, alias `read_back`), `safe_state` and `verified` have defaults. Validation after parsing: the coil count must be 1 to 2000, the slave must not be 0 and must be at most 247 unless it is 0xFE or 0xFF, and a non-empty `safe_state` must have exactly one entry per coil. Errors, all `StationErrorKind::Config` with a message starting `modbus board:`: a TOML parse or type error, a bad slave or coil count, a wrong safe-state length. Pure: no I/O, no lock.

### `pub fn lrc(data: &[u8]) -> u8` [@ANCHOR: radioctl:radioctl_station_modbus_lrc]

Computes the Modbus ASCII checksum (LRC) of `data`, borrowed: the bytes are summed with wrapping arithmetic and the sum is negated (two's complement), giving the low byte. An empty slice gives 0. Returns the checksum byte. It cannot fail or panic, does no I/O and takes no lock. It is applied to the raw bytes (slave and PDU), not to the hex text. Used by `encode_ascii` and `decode_ascii`.

### `pub fn crc16(data: &[u8]) -> u16` [@ANCHOR: radioctl:radioctl_station_modbus_crc16]

Computes the Modbus RTU CRC-16 of `data` (borrowed): start value 0xFFFF, bitwise reflected algorithm with polynomial 0xA001, eight shifts per byte. Returns the 16-bit CRC, which `encode_rtu` appends low byte first. An empty slice gives 0xFFFF. It cannot fail or panic, does no I/O and takes no lock. Computed for every frame instead of using a table from a vendor.

### `fn hex_val(c: u8) -> Option<u8>` [@ANCHOR: radioctl:radioctl_station_modbus_hex_val]

Converts one ASCII hex digit byte `c` to its value 0 to 15. Accepts `0`-`9`, `A`-`F` and `a`-`f`; any other byte returns `None`. By value argument, pure, cannot fail or panic, no I/O, no lock. `decode_ascii` uses it and turns `None` into a non-hex-digit error, so a damaged frame fails closed.

### `pub fn encode_ascii(slave: u8, pdu: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_modbus_encode_ascii]

Builds a Modbus ASCII frame. Arguments: `slave`, the address byte, and `pdu`, the borrowed request bytes. The slave and the PDU, followed by their LRC, are written as two upper-case hex digits per byte after a leading `:` and followed by CR LF. Returns the frame as a new vector. Pure: no I/O, no lock, cannot fail. An empty PDU still gives a valid frame. The result is always uppercase and deterministic.

### `pub fn decode_ascii(frame: &[u8]) -> Result<(u8, Vec<u8>), StationError>` [@ANCHOR: radioctl:radioctl_station_modbus_decode_ascii]

Checks a received ASCII frame and returns the slave address and the PDU as a new vector. Argument: `frame`, borrowed, which must start with `:` and end with CR LF, have an even number of hex digits between them and at least 6 of them (slave, one PDU byte, checksum), contain only hex digits (either case) and carry a correct LRC. Errors, all `StationErrorKind::Driver`: not an ASCII frame, bad length, non-hex digit, checksum mismatch. Pure: no I/O, no lock, and it does not check that the slave is the expected one (the driver does that).

### `pub fn encode_rtu(slave: u8, pdu: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_modbus_encode_rtu]

Builds a Modbus RTU frame: the slave byte, the borrowed `pdu` bytes, then the CRC-16 of those bytes with the low byte first. Returns the binary frame as a new vector. Pure: no I/O, no lock, cannot fail. The inter-frame idle time that RTU needs is not produced here; the driver's pause before each request and its idle-gap read handle timing.

### `pub fn decode_rtu(frame: &[u8]) -> Result<(u8, Vec<u8>), StationError>` [@ANCHOR: radioctl:radioctl_station_modbus_decode_rtu]

Checks a received RTU frame and returns the slave address and the PDU as a new vector. Argument: `frame`, borrowed, at least 4 bytes (slave, one PDU byte, two CRC bytes); the CRC-16 of all but the last two bytes must equal the last two bytes, low byte first. Errors, both `StationErrorKind::Driver`: frame too short, RTU checksum mismatch. Pure: no I/O, no lock. The expected slave is not checked here.

### `pub fn pdu_write_coil(coil: u16, on: bool) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_modbus_pdu_write_coil]

Builds the request PDU for function 5 (write single coil, code 0x05): the function code, the coil address as two big-endian bytes, and the value `FF 00` for on or `00 00` for off. Arguments: `coil`, the protocol address (already including `coil_base`), and `on`. Returns a 5-byte vector. Pure: no I/O, no lock, cannot fail. A valid acknowledgement echoes the request exactly (see `parse_reply`).

### `pub fn pdu_write_coils(start: u16, bits: &[bool]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_modbus_pdu_write_coils]

Builds the request PDU for function 15 (write multiple coils, code 0x0F): the function code, the start address and the quantity (`bits.len()`) as big-endian 16-bit values, a byte count, then the bits packed eight to a byte with bit 0 of the first byte for the first coil. Arguments: `start` (protocol address) and `bits` (borrowed, levels in order). Returns the PDU. The quantity and byte count are truncating casts to `u16` and `u8`, so the caller must keep the list within the Modbus limit; `from_toml` limits boards to 2000 coils. Pure: no I/O, no lock, cannot fail.

### `pub fn pdu_read_coils(start: u16, count: u16) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_modbus_pdu_read_coils]

Builds the request PDU for function 1 (read coils, code 0x01): the function code, the start address and the coil count, each a big-endian 16-bit value, 5 bytes in all. Arguments: `start` (protocol address) and `count`. Returns the vector. Pure: no I/O, no lock, cannot fail. The driver refuses to send it for a board whose status is not readable (`exchange`); building the bytes has no such rule.

### `pub fn parse_reply(request: &[u8], reply: &[u8]) -> Result<Reply, StationError>` [@ANCHOR: radioctl:radioctl_station_modbus_parse_reply]

Validates a reply PDU against the request PDU it answers, both borrowed, and fails closed. Returns `Reply::Written` for a function 5 reply identical to the request, or a function 15 reply of 5 bytes whose function, start and quantity match the request; returns `Reply::Coils` with the levels in order for a function 1 reply whose byte count equals the ceiling of count over 8 and whose length is exactly 2 plus that. Errors, all `StationErrorKind::Driver`: an empty PDU, an exception reply (function with 0x80 set, the code is quoted), a different function, a write that is not echoed correctly, a wrong byte count, or any combination not listed. Pure: no I/O, no lock.

### `pub fn new(board: ModbusBoard, io: T) -> ModbusRelay<T>` [@ANCHOR: radioctl:radioctl_station_modbus_relay_new]

Creates a driver for `board` over the byte stream `io`, both moved in; `T` must be async read and write, `Unpin` and `Send`. Nothing is sent. Every coil is unknown, the safe state is marked not yet applied, the failure count is zero and no retry time is set. The first command, or `ensure_safe_state`, writes the safe state before anything else. Returns the driver; it cannot fail and does no I/O. The stream sits behind an async mutex and the record behind a standard mutex.

### `pub async fn reopen(&self, io: T)` [@ANCHOR: radioctl:radioctl_station_modbus_relay_reopen]

Replaces the byte stream after a reconnect, a USB drop, a power loss or a port restart. Argument: `io`, the new stream, moved in. The board may be in any relay state and has no watchdog, so every coil record becomes unknown, the safe state is required again, and the failure count and retry time are cleared. Nothing is sent here. Async: it waits for the stream lock, so it waits for any exchange in progress to finish. Returns nothing and cannot fail.

### `fn coil(&self, line: u32) -> Result<u16, StationError>` [@ANCHOR: radioctl:radioctl_station_modbus_relay_coil]

Maps a line number (zero based) to its protocol coil address, `coil_base` plus the line. Argument: `line`. Error: `StationErrorKind::Config` ("line is past the board's coils") when `line` is not below the board's coil count. Pure and bounds-checked; the `u16` addition cannot overflow for the validated board sizes. No I/O, no lock.

### `async fn read_frame(&self, io: &mut T) -> Result<Vec<u8>, StationError>` [@ANCHOR: radioctl:radioctl_station_modbus_relay_read_frame]

Reads one reply frame from `io` (mutably borrowed, the stream already locked by the caller) one byte at a time. ASCII framing: waits up to the reply timeout for each byte, discards bytes before a `:` (a new `:` restarts the frame) and returns at CR LF. RTU framing: waits the reply timeout for the first byte, then the inter-frame gap (at least 1 ms) between bytes, and a gap after at least one byte ends the frame successfully. A frame longer than 520 bytes is an error. Errors: `Timeout` ("no reply") if nothing arrives in time, `Driver` if the port closes (a zero-length read), a read error, or an over-long reply. Async; reads only.

### `async fn exchange(&self, io: &mut T, request: &[u8]) -> Result<Reply, StationError>` [@ANCHOR: radioctl:radioctl_station_modbus_relay_exchange]

Sends one request PDU on the locked stream and returns the validated reply. Arguments: `io`, the stream locked by the caller, and `request`, the borrowed PDU. First guard: for a board whose status is not readable a function 1 request is refused with a `Driver` error before anything is written, so it is never on the wire. Then it frames the request (ASCII or RTU) for the board's slave, waits the inter-frame time, writes and flushes, reads one frame, decodes it, requires the reply slave to equal the board's slave and validates it with `parse_reply`. Errors: `Driver` for write, flush, framing, checksum, wrong slave or reply mismatch, `Timeout` for no reply. Async and half duplex: the caller holds the stream lock for the whole exchange. A request is not retried.

### `async fn apply_safe(&self, io: &mut T, respect_backoff: bool) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_modbus_relay_apply_safe]

Writes the board's safe state to every coil in one function 15 frame and, on an acknowledgement (`Reply::Written`), records every coil at its safe level with the time, marks the safe state applied and clears the failure count and retry time. Arguments: `io`, the locked stream; `respect_backoff`, true for normal use and false for the emergency path. When true and a retry time is still in the future, it returns a `Driver` error without sending. On any failure (no reply, a bad reply or an unacknowledged write) every coil becomes unknown, the safe state stays not applied, the failure count increases and the next attempt waits 250 ms shifted left by the failure count (capped at six shifts) up to 8 seconds; the original error is returned. Async; one serial exchange; the record lock is taken only briefly.

### `pub async fn ensure_safe_state(&self) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_modbus_relay_ensure_safe_state]

Applies the safe state if it has not been applied on the current link. A caller that opens the link calls it first; every command path also calls it, so nothing else goes out before it succeeds. No arguments. Returns `Ok(())` at once, with no I/O, if the safe state is already done. Otherwise it calls `apply_safe` with the backoff respected; after a failure it returns an error without sending until the retry time passes. Errors are those of `apply_safe`. Async; holds the stream lock for the exchange.

### `async fn write_gated(&self, request: &[u8], affected: &[(usize, bool)]) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_modbus_relay_write_gated]

The one path for a command write. Arguments: `request`, the write PDU (borrowed) and `affected`, the coil indexes and levels it sets. It locks the stream, writes the safe state first if it is not yet applied (an error there is returned and nothing else is sent), then performs the exchange. On an acknowledgement the affected coils are recorded with the current time. On a non-write reply, a timeout, a garbled reply or an exception the affected coils become unknown and an error is returned; if the error text contains "port closed" or "failed:" the safe state is also marked not applied, so it is written again on the next command. Async; holds the stream lock across the whole exchange. Not idempotent as a bus operation; a retry is a new write.

### `pub async fn write_all(&self, on: bool) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_modbus_relay_write_all]

Sets every coil on or off in one function 15 frame, behind the safe-state gate (the safe state is written first if the link has not had it). Argument: `on`, the level for all coils. On an acknowledgement all coils are recorded at that level; on failure they become unknown. Returns the errors of `write_gated`. Async; one or two serial exchanges; a real hardware operation: switching all relays on is not checked against the interlock here, since that applies in the station above.

### `pub fn record(&self) -> Vec<Recorded>` [@ANCHOR: radioctl:radioctl_station_modbus_relay_record]

Returns the driver's record of every coil in order as an owned vector of `Recorded`: the level the board last acknowledged (`None` while unknown) and the age since that acknowledgement. Argument: `&self`. Takes the record lock briefly, does no I/O and cannot fail. It is a record of acknowledged writes, not a confirmation that the contact is in that state; the age grows between calls.

### `fn assurance(&self) -> Assurance` [@ANCHOR: radioctl:radioctl_station_modbus_relay_assurance]

Reports the driver's assurance level: `Assurance::HostSupervised` when the board's status is readable and `Assurance::Recorded` when it is not. Argument: `&self`. Returns the value without I/O or lock; it cannot fail. It depends only on `status_readable` from the board description and does not change during the life of the driver.

### `fn write_line(&self, line: u32, on: bool) -> BoxFuture<'_, Result<(), StationError>>` [@ANCHOR: radioctl:radioctl_station_modbus_relay_write_line]

Switches one relay with function 5 on the line's coil, behind the safe-state gate. Arguments: `line`, zero based, and `on`. Errors: `Config` if the line is past the board's coils (nothing sent) and any error of `write_gated`: the safe-state write failing, `Timeout`, or a `Driver` error for a garbled or unacknowledged reply. On success only that coil's record is updated. Async; one serial exchange (two if the safe state was pending); holds the stream lock for it. Real hardware switching; the interlock and transmit gate are applied above, in the station.

### `fn read_line(&self, line: u32) -> BoxFuture<'_, Result<bool, StationError>>` [@ANCHOR: radioctl:radioctl_station_modbus_relay_read_line]

Reads one line's state. Argument: `line`, zero based. Error `Config` if out of range. For a board whose status is not readable it sends nothing: it returns the driver's recorded level for the line, or a `Driver` error while that is unknown. For a readable board it locks the stream, writes the safe state first if needed, sends function 1 for one coil and returns the bit; a reply that is not exactly one coil gives a `Driver` error ("unexpected read reply"), and exchange errors pass through. Even then the bit is the commanded state, not a measurement of the contacts. Async; at most one or two serial exchanges.

### `fn can_read_back(&self) -> bool` [@ANCHOR: radioctl:radioctl_station_modbus_relay_can_read_back]

Reports whether the board's coil status can be read, which is the `status_readable` setting of the board description. False for a board whose status is not trusted, where function 1 is never sent. Argument: `&self`. Returns the flag; cannot fail, no I/O, no lock, constant for the life of the driver. The switchboard uses it to decide whether a write is confirmed by a read.

### `fn safe_line(&self, line: u32) -> bool` [@ANCHOR: radioctl:radioctl_station_modbus_relay_safe_line]

Returns the board's safe level for one line: the `safe_state` entry, or off when `safe_state` is empty. Argument: `line`, zero based. A line beyond the list also returns false (off) rather than an error. Computed from `safe_bits`, so no I/O and no lock; cannot fail or panic. The emergency all-off reads it to know the intended safe level.

### `fn force_safe(&self, line: u32) -> BoxFuture<'_, Result<(), StationError>>` [@ANCHOR: radioctl:radioctl_station_modbus_relay_force_safe]

The emergency path: writes the board's whole safe state in one function 15 frame, ignoring the retry delay and the minimum times. Argument: `line`, only checked for range (error `Config` if past the board's coils); the frame always covers every coil, so calling it for each line sends the same frame each time. It waits for the stream lock, so it waits for an exchange in progress but is never refused by the backoff. Errors: those of `apply_safe` (`Timeout`, or `Driver` for a bad or missing acknowledgement); on failure the coils become unknown and the delay is extended. Async; one serial exchange; real hardware.

### `fn recorded_line(&self, line: u32) -> Option<Recorded>` [@ANCHOR: radioctl:radioctl_station_modbus_relay_recorded_line]

Returns the record of one line: the level the board last acknowledged and its age, or an unknown value. Argument: `line`, zero based. Returns `None` if the line is out of range, otherwise a copy of that coil's `Recorded` from `record`. Recorded, not confirmed. Takes the record lock briefly (it copies the whole record to pick one entry), does no I/O and cannot fail.

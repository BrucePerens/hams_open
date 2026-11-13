# radioctl function anchors, batch 20

Function anchors for `radioctl-protocols` (the virtual rig's stop-Morse, Kenwood and CI-V reply helpers) and `radioctl-core` (reconnect backoff, USB port selection, the serial control-line keyer, TCP connect and the serial link). Tests cite each anchor with `Tests [@ANCHOR: ...]`. Transmit gate: the virtual rig never transmits (it only sets an in-memory key flag). The keyer in `keyer.rs` is the one place in this batch that can key a real transmitter: `LineKeyer::key` is refused unless the rig's transmit gate is open, `release` and the drop handler are always allowed. The serial control-line functions drive the electrical line without any gate; the gate is applied by `LineKeyer::key` above them, so they must not be used to key except through a keyer.

## `crates/radioctl-protocols/src/virtual_rig.rs`

### `fn stop_cw(&mut self)` [@ANCHOR: radioctl:radioctl_virtual_stop_cw]

Records that a stop-Morse command arrived at the virtual rig: it empties the record of Morse text received so far (`state.cw_sent`) and adds one to the stop counter (`state.cw_stops`, a saturating count that never wraps). It takes no arguments and returns nothing. Called by the CI-V `17 FF` handler, the Elecraft `KY <EOT>` stop and the Kenwood `KY` stop forms. Tests read the counter to prove a stop reached the radio. It does not change the key flag: a virtual rig held keyed by PTT (push to talk) stays keyed. No I/O.

### `fn kenwood_command(&mut self, text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_kenwood_command]

The virtual Kenwood command table: takes one tokenized command (`text`, no terminator) and returns the answer bytes, empty when the radio is silent. The TS-890S, TS-440 family and TS-990 models are handed first to their own tables. Otherwise it handles `FA` and `FB` (read or set 11-digit frequency), `MD`, `TX` and `RX` (which set only the in-memory key flag, whatever argument follows), `KY` via `kenwood_ky`, `ID`, `AI` (auto-information flag), `SM`, `PS`, `FV` (a fixed firmware string, our own value), `DA`, `IF`, `FR` and `FT` (VFO selection), `RT`, `XT`, `RC`, `RU` and `RD` (move the offset), the levels `AG RG MG ML PC KS`, and the attenuator and preamp `RA` and `PA`. An unknown command or malformed argument is answered `?;`. Mutates the state; no I/O, no RF.

### `pub(super) fn kenwood_ky(&mut self, arg: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_kenwood_ky]

Handles a Kenwood `KY` command (`arg` is the text after `KY`) as the description's `[kenwood.ky]` block says the model takes it. Every command is answered `?;` on a model whose manuals document no `KY` or give no stop. An empty argument is a read and answers `KY0;` (the buffer has room). The model's stop form (`0` for the `Ky0` stop, or a field of blanks of the model's width for the `Blank` stop) empties the recorded text and returns no reply. A message of the right form and width (a leading blank and exactly `width` characters for the padded form; a leading `2` and up to `width` characters for the variable form) is checked with `KyParams::encode_text`, trimmed of trailing blanks and appended to `cw_sent`, with no reply. Anything else is `?;`. Mutates the state only.

### `fn kenwood_id(&self) -> u16` [@ANCHOR: radioctl:radioctl_virtual_kenwood_id]

Returns the model identifier that the virtual rig reports for the Kenwood `ID` command: the `model_id` in the description's Kenwood section when the wire is Kenwood, and 0 for every other wire kind. The caller formats it as three digits (`ID019;`). Pure, never fails.

### `fn set_frequency(&mut self, digits: &str, vfo_a: bool) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_set_frequency]

Sets a VFO frequency from the digits of a Kenwood or Elecraft `FA` or `FB` command. `digits` must be exactly 11 decimal digits (hertz); `vfo_a` true sets VFO A and false VFO B. Returns `None` (no reply) on success and `Some("?;")` for a wrong length or non-digit text. There is no range check, so any 11-digit value is accepted. Mutates the state; no I/O.

### `fn set_kenwood_mode(&mut self, code: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_set_kenwood_mode]

Sets the mode from the one-character code of a Kenwood or Elecraft `MD` command. `code` must be exactly one byte that the description's mode table knows; otherwise the answer is `Some("?;")` and the state is unchanged. On success it stores the mode and returns `None` (no reply). Mutates the state; no I/O.

### `fn data_reply(cmd: u8, bcd: Option<Vec<u8>>, prefix: &[u8]) -> CivReply` [@ANCHOR: radioctl:radioctl_virtual_data_reply]

Builds a CI-V data reply from an already encoded BCD frequency field. `cmd` is the reply command byte, `prefix` is copied in front of the BCD bytes (for example the `25` selector), and `bcd` is the encoder result. When `bcd` is `None` (the frequency does not fit the model's field) the reply is `CivReply::Ng`; otherwise `CivReply::Data(cmd, prefix + bcd)`. Pure.

### `fn meter_reply(sub: u8, value: u16) -> CivReply` [@ANCHOR: radioctl:radioctl_virtual_meter_reply]

Builds the reply to a CI-V `15` read: a data reply for command `0x15` holding the sub-command byte `sub` and `value` as two BCD bytes, most significant pair first. `value` is the raw reading and is clamped to 9999 by the BCD encoder. Used for the S-meter, the power meter and the raw transmit meters. Pure, cannot fail.

## `crates/radioctl-core/src/backoff.rs`

### `pub fn delay(&self, failures: u32) -> Duration` [@ANCHOR: radioctl:radioctl_core_backoff_delay]

`Backoff::delay`: the wait before the next reconnect attempt after `failures` failures in a row, where 1 is the first failure. The wait starts at `initial` (never less than 1 ms), is multiplied by `factor` for each further failure (a `factor` of 0 acts as 1), and never exceeds `max` (and `max` is raised to `initial` if smaller). `failures` of 0 or 1 gives the initial wait. The result is always between 1 ms and the ceiling, so a retry loop is bounded and never spins. Pure; uses saturating arithmetic so large counts cannot overflow. Defaults are 250 ms, 8 s and 2.

## `crates/radioctl-core/src/usb.rs`

### `pub fn classify(vid: u16, pid: u16, cdc_acm: bool) -> Chip` [@ANCHOR: radioctl:radioctl_core_usb_classify]

Names the USB-to-serial chip family behind a port from its ids. `vid` and `pid` are USB vendor and product ids. Any FTDI vendor id (`0x0403`) is `Ftdi`; Silicon Labs (`0x10C4`) with a product id from `0xEA60` to `0xEA71` is `Cp210x`; WCH (`0x1A86`) with product `7522`, `7523`, `5523`, `55D4` or `55D3` (hex) is `Ch340`. Otherwise `cdc_acm` true (the kernel bound the CDC ACM driver) gives `CdcAcm`, and anything else `Unknown`. Pure, no errors. Unknown whether the lists are complete for every adapter in the field.

### `pub fn chip(&self) -> Chip` [@ANCHOR: radioctl:radioctl_core_usb_port_chip]

`PortInfo::chip`: the chip family of a port found by the listing. For a port with both a USB vendor and product id it calls `classify` with the port's `cdc_acm` flag; for a port with no USB ids (a plain serial port or a pty) it is `Chip::Unknown`. Borrows the port; pure, cannot fail.

### `pub fn select_port<'a>( matcher: &UsbMatcher, ports: &'a [PortInfo], ) -> Result<&'a PortInfo, RigError>` [@ANCHOR: radioctl:radioctl_core_usb_select_port]

Picks the one port that matches a `UsbMatcher` (vendor id, product id and an optional serial number) out of a list. Without a serial number in the matcher any serial matches. Returns a reference into `ports` (borrowed for the same lifetime). Errors: no match gives `ErrorKind::LinkDown` ("no serial port with USB id vvvv:pppp ...", the device may simply be unplugged, so a retry is reasonable); more than one match gives `ErrorKind::Unsupported` telling the caller to give the serial number. It never chooses between several matches, since picking the wrong one could mean sending commands to the wrong radio. Pure; no I/O.

### `fn read_trimmed(path: &Path) -> Option<String>` [@ANCHOR: radioctl:radioctl_core_usb_read_trimmed]

Reads a small sysfs attribute file and returns its text with surrounding whitespace removed, or `None` if the file cannot be read or is empty after trimming. Used for the vendor id, product id, serial and product strings. Blocking file read of a tiny sysfs file; every failure is swallowed into `None`, so a missing attribute looks the same as an empty one.

### `pub fn list_sysfs(sys_root: &Path, dev_root: &Path) -> Vec<PortInfo>` [@ANCHOR: radioctl:radioctl_core_usb_list_sysfs]

Lists the `ttyUSB*` and `ttyACM*` ports from a sysfs tree rooted at `sys_root` (normally `/sys`), with the device nodes put under `dev_root` (normally `/dev`); both are borrowed, and passing a fake tree makes it testable. For each port the USB device is the nearest ancestor of the tty's `device` link that has `idVendor` and `idProduct`; the ids are read as hexadecimal, and the serial and product strings are read when present. The `cdc_acm` flag is set when the device's driver link names `cdc_acm`. Entries that cannot be read are skipped, and an unreadable `class/tty` gives an empty list. The result is sorted by path. Blocking file I/O in small sysfs files; no errors are returned.

### `pub fn list_ports() -> Vec<PortInfo>` [@ANCHOR: radioctl:radioctl_core_usb_list_ports]

Lists the serial ports of this machine that can be told apart by USB id. On Linux it calls `list_sysfs` on `/sys` and `/dev` (no libudev needed). On other systems it asks the `serialport` crate and converts the USB entries (the `cdc_acm` flag is always false there). A failed enumeration gives an empty list rather than an error. Blocking file or system calls; call it from a blocking context, not from a latency-sensitive task. The listing is a snapshot, so a device plugged in a moment later is not in it.

## `crates/radioctl-core/src/keyer.rs`

### `pub fn new(lines: C, line: KeyLine, active_high: bool) -> Result<LineKeyer<C>, RigError>` [@ANCHOR: radioctl:radioctl_core_keyer_new]

`LineKeyer::new`: builds a keyer for one serial control line. `lines` is anything that implements `ControlLines` (owned and kept), `line` is `KeyLine::Rts` or `Dtr`, and `active_high` says whether keying asserts the line (true) or releases it (false). The line is put in its inactive level at once (the opposite of the keyed level), and a failure to do so is returned as the line's own error (for example `Unsupported` for RTS on a port with hardware flow control, `LinkDown` for a closed port) instead of being hidden, so no keyer exists for a line it cannot control. Does not key the transmitter.

### `pub fn key(&self, rig: &RigHandle) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_keyer_key]

Keys the transmitter by driving the control line to its active level. This is a transmit-capable operation: it first checks that the rig's transmit gate is open (`rig.transmit_permit()` is `Some`) and otherwise returns `ErrorKind::TransmitGateClosed` ("transmit gate is closed for this rig") without touching the line. `rig` is borrowed. The check reads the rig's gate flag and permit (atomic and mutex reads), not a queued command, so it is immediate; the line write is also immediate and does not wait behind a pending read. This function does not check the band or the transmit frequency; unknown whether the caller does so, check the caller. Errors from the line write (`LinkDown`, `Unsupported`) are returned as they are. Calling it again while keyed just asserts the line again, which is harmless.

### `pub fn release(&self) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_keyer_release]

Unkeys the transmitter by setting the line to its inactive level. Always allowed: it needs no gate and no rig handle, so an unkey can never be refused. It is immediate (not queued behind reads) and idempotent: releasing an already released line is harmless. Returns the line's own error if the write fails (for example the port has gone), which the caller must treat as "the transmitter may still be keyed". Called by the holder and by the drop handler.

### `fn drop(&mut self)` [@ANCHOR: radioctl:radioctl_core_keyer_drop]

`Drop` for `LineKeyer`: when a keyer goes away (its holder returns, panics or is dropped while keyed) it calls `release` so the line is made inactive and RF cannot stay on the air for want of a release call. Any error from the release is ignored because a destructor cannot report one, so after a failure on drop (for example the port was unplugged) nothing says the line was not released. Runs on whatever thread drops the keyer; a single immediate line write.

## `crates/radioctl-core/src/tcp.rs`

### `pub async fn connect_tcp<A: ToSocketAddrs + Copy>( addr: A, config: &TcpConfig, ) -> Result<TcpStream, RigError>` [@ANCHOR: radioctl:radioctl_core_tcp_connect]

Opens a TCP connection to a rig, a network serial server or `rigctld`. `addr` is anything tokio can resolve and is `Copy`; `config` is borrowed: `connect_timeout` bounds each attempt (default 3 s), `attempts` is the total number of tries (at least 1 even if 0 is given; default 3) and `backoff` supplies the wait between tries (`Backoff::delay(attempt)`, the sleep happens only between attempts). On success it switches off Nagle's algorithm (no-delay, because CAT frames are tiny) and returns the stream; a failure to set it is `LinkDown`. After the last failed attempt it returns `ErrorKind::LinkDown` saying how many attempts were made and the last cause (a connect error or "no answer within ..."). Async; the sleeps and the connect wait are the runtime's, so it never blocks a thread. The worst case is about `attempts * connect_timeout` plus the backoff waits. No retry beyond that: a hang is ruled out.

## `crates/radioctl-core/src/serial.rs`

### `pub fn new(target: PortTarget, baud: u32) -> SerialConfig` [@ANCHOR: radioctl:radioctl_core_serial_config_new]

`SerialConfig::new`: a configuration for a serial port with safe defaults. `target` is a device path or a USB matcher (owned), `baud` is the baud rate in bits per second. Defaults: 8 data bits, no parity, 1 stop bit, no flow control, control lines managed (`LineControl::Managed`) with DTR and RTS left inactive after every open, the default backoff (250 ms to 8 s), and one attempt for the first open (a later disconnect is retried without limit). Pure constructor, cannot fail; change fields afterwards for anything else.

### `pub fn with_description_line(mut self, description: &RigDescription) -> SerialConfig` [@ANCHOR: radioctl:radioctl_core_serial_config_with_description]

Takes the line settings from a rig description: data bits (7 or else 8), parity (`E`, `O`, else none), stop bits (2 or else 1) and flow control (`rtscts`, `xonxoff`, else none), from the receiver block or else the vendor block; the Yaesu legacy block gives only its stop bits. A description with none of these blocks leaves the 8N1 defaults. The baud rate, target and line handling are left as they are. `self` is consumed and returned (builder style); `description` is borrowed. Pure, cannot fail. Note that rtscts selects hardware flow control, which makes RTS unusable as a keying line.

### `fn set_line(&self, line: KeyLine, asserted: bool) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_serial_control_set_line]

`ControlLines::set_line` for `SerialControl`: drives RTS or DTR to the given level (`asserted` true is on) with an immediate call on a second handle of the port, never queued behind reads or writes. Checks happen in this order and each fails with its own error: RTS with hardware flow control (`Unsupported`, the driver owns RTS; this applies to the release too, so a keyer cannot be built on that line); an unmanaged port (`Unsupported`, "no modem control lines"); a port that is not open (`LinkDown`); and an operating-system error (`LinkDown`, "control line: ..."). It takes the port mutex briefly (a poisoned mutex is recovered). This is the electrical keying path: it applies no gate itself, the gate is applied by `LineKeyer::key`.

### `fn check_rts_free(&self, is_rts: bool) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_serial_control_check_rts_free]

Refuses a write of RTS when the port uses hardware flow control, because the driver owns RTS then; a write would do nothing or fight the flow control, and a keyer built on it would look keyed while the line is not under its control. `is_rts` is true when the line being written is RTS; DTR and break pass. Returns `Unsupported` ("RTS belongs to hardware flow control on this port...") or `Ok`. It is called first for every write of RTS, including the release, so a keyer on that line cannot even be built. Pure.

### `fn set_control(&self, line: ControlLine, asserted: bool) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_serial_control_set_control]

`ControlLines::set_control` for `SerialControl`: sets a control line that carries data for a rig tuned by its control lines (no byte stream): RTS or DTR on or off, or the transmit line in break (`asserted` true) or out of it. The same checks as `set_line` apply in the same order: RTS with hardware flow control and unmanaged ports are `Unsupported`, a closed port or an operating-system failure is `LinkDown`. The port mutex is held briefly. Immediate, not queued; it is data, never a keying line, and it does not apply the transmit gate.

### `fn set_control(&self, line: ControlLine, asserted: bool) -> Result<(), RigError>` [@ANCHOR: radioctl:radioctl_core_control_lines_set_control]

The default `ControlLines::set_control`: refuses with `ErrorKind::Unsupported` ("this transport cannot drive the control lines as data") so a transport that cannot do it fails the command instead of silently dropping it. `line` and `asserted` are ignored. A transport that can (the serial control above) overrides it. Pure, never touches hardware.

### `pub fn status(&self) -> LinkStatus` [@ANCHOR: radioctl:radioctl_core_serial_link_status]

`SerialLink::status`: a copy of the current link status: `Connected` (with the device path in use and how many times the port has been opened, starting at 1), `Reconnecting` (failed attempts since the last connection and why the last one failed) or `Closed` (the supervisor has stopped because the byte stream was dropped). Reads the latest value of a watch channel; does not block and cannot fail.

### `pub fn watch(&self) -> watch::Receiver<LinkStatus>` [@ANCHOR: radioctl:radioctl_core_serial_link_watch]

`SerialLink::watch`: a new tokio watch receiver that wakes on every link status change (connect, reconnect attempts, close), so a caller can await changes instead of polling `status`. The receiver is a clone and is independent of the link's own; keeping it does not keep the port open. Cannot fail.

### `pub fn stats(&self) -> SerialStats` [@ANCHOR: radioctl:radioctl_core_serial_link_stats]

`SerialLink::stats`: a snapshot of the link's counters: bytes read from the port, bytes written, bytes written while the port was down and therefore dropped, and successful reconnects (not counting the first open). The values are read from atomics without ordering guarantees between them, so they may be a moment apart. Cannot fail.

### `pub fn into_parts(self) -> (DuplexStream, SerialMonitor)` [@ANCHOR: radioctl:radioctl_core_serial_link_into_parts]

Consumes a `SerialLink` and splits it into the byte stream (for `rig::spawn`, owned by the actor) and a `SerialMonitor` that keeps the control lines, the status watch and the counters for the owner for the life of the rig. Dropping the monitor does not close the port; dropping the stream does, and that stops the supervisor. Cannot fail.

### `pub fn status(&self) -> LinkStatus` [@ANCHOR: radioctl:radioctl_core_serial_monitor_status]

`SerialMonitor::status`: the current link status, as `SerialLink::status` (`Connected`, `Reconnecting` or `Closed`), for the owner that handed the byte stream to the actor. Reads the watch value; does not block and cannot fail.

### `pub fn watch(&self) -> watch::Receiver<LinkStatus>` [@ANCHOR: radioctl:radioctl_core_serial_monitor_watch]

`SerialMonitor::watch`: a new watch receiver that wakes on every link status change, as `SerialLink::watch`, for the owner after `into_parts`. The new receiver is independent of the monitor's own, and holding it does not keep the port open. A caller can await it instead of polling `status`. Cannot fail and does not block.

### `pub fn stats(&self) -> SerialStats` [@ANCHOR: radioctl:radioctl_core_serial_monitor_stats]

`SerialMonitor::stats`: a snapshot of the link counters (bytes in, bytes out, bytes dropped while down, reconnects), as `SerialLink::stats`. Each counter is read separately from an atomic, so they may differ by a moment. Cannot fail.

### `pub fn resolve(target: &PortTarget) -> Result<PathBuf, RigError>` [@ANCHOR: radioctl:radioctl_core_serial_resolve]

Turns a `PortTarget` into the device path to open: a `Path` target is returned as is (cloned, not checked for existence); a `Usb` target is looked up in the current port list (`list_ports`) and the one matching port's path is returned. A USB target errors as `select_port` does: `LinkDown` when nothing matches (the device is unplugged) and `Unsupported` when several match. Because the device node usually changes after a reconnect, the supervisor calls it again at every reopen. Blocking sysfs reads for the USB case.

### `fn open_port(config: &SerialConfig) -> Result<(Box<dyn SerialPort>, PathBuf), RigError>` [@ANCHOR: radioctl:radioctl_core_serial_open_port]

Opens the serial port described by `config` and returns it with the path used. It resolves the target, opens it with the configured baud rate, data bits, parity, stop bits and flow control and a 50 ms read timeout, then, when the lines are `Managed`, immediately sets DTR to `idle_dtr` and (unless hardware flow control owns RTS) RTS to `idle_rts`; a failure to do that fails the open. Errors, all `LinkDown`: target resolution failure, the open itself, and the DTR or RTS write. The operating system asserts DTR and RTS when it opens a port, so a glitch of microseconds happens before they are made inactive; hardware that keys on RTS or DTR needs an interface that tolerates it. Blocking; it runs on the supervisor's thread, never on a runtime worker.

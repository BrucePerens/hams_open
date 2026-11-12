# radioctl function anchors, batch 01

Function anchors of the USB transport in `radioctl-core` (`usb_wire.rs`, `usb_device.rs`, `usb_link.rs`, `usb_libusb.rs`) and of the USB engines and virtual boards in `radioctl-protocols`, written to the ADR 0113 standard. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`. None of these functions keys a transmitter by itself: they move request and reply frames between a protocol engine and a USB device, and any transmit decision (the lock, band check and transmit gate) is taken by the rig actor above them before an engine encodes a keying request. The link never keys a device on its own, and after a reconnect the actor unkeys the rig before any other command.

## `crates/radioctl-core/src/usb_wire.rs`

The frame the USB transport carries between a protocol engine and a `UsbDevice`. A request frame is `op, a, b, value(2), index(2), len(2), data(len)` (nine header bytes, little-endian); a reply frame is `op, status, len(2), data(len)` (four header bytes). `MAX_DATA` (4096) bounds every data field. `op` is 1 control in, 2 control out, 3 bulk write, 4 bulk read, 5 interrupt write, 6 interrupt read, 7 interrupt exchange.

### `fn put_header(op: u8, a: u8, b: u8, value: u16, index: u16, len: u16, data: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_core_usb_put_header]

Private helper that lays out one request frame: the three single bytes `op`, `a`, `b`, then `value`, `index` and `len` each as two little-endian bytes (nine header bytes in all), then `data` appended unchanged. It is the only place the request header is written, so every `UsbTransfer::encode` arm agrees with `UsbTransfer::decode`. It does not check that `len` matches `data.len()` and does not cut `data`; the caller (`encode`) cuts to `MAX_DATA` and computes `len` first. Pure, allocates one `Vec`, never fails.

### `pub fn UsbStatus::code(self) -> u8` [@ANCHOR: radioctl:radioctl_core_usb_status_code]

The wire byte of a reply status: `Ok` is 0, `Timeout` 1, `NoDevice` 2, `Stall` 3 and `Other` 4. `UsbReply::encode` writes it as the second byte of the reply frame, and `UsbStatus::from_code` is its inverse. The values are the wire format between the engine side and the link thread and must not be renumbered. `self` is `Copy`; the function is pure and total.

### `pub fn UsbStatus::from_code(code: u8) -> UsbStatus` [@ANCHOR: radioctl:radioctl_core_usb_status_from_code]

Reads the status byte of a reply: 0 `Ok`, 1 `Timeout`, 2 `NoDevice`, 3 `Stall`, and every other value (4 and anything unknown) `Other`. Mapping unknown bytes to `Other` means a corrupted or future status is treated as a generic failure rather than as success or as a lost device. Used by `UsbReply::decode`. Pure and total, so it has no error return.

### `pub fn UsbTransfer::op(&self) -> u8` [@ANCHOR: radioctl:radioctl_core_usb_transfer_op]

The operation byte that identifies the variant on the wire: `ControlIn` 1, `ControlOut` 2, `BulkWrite` 3, `BulkRead` 4, `InterruptWrite` 5, `InterruptRead` 6, `InterruptExchange` 7. `encode` writes it first in the request frame, `decode` switches on it, and `execute` copies it into the reply so the engine can tell which request an answer belongs to. Borrows the transfer, pure and total.

### `pub fn UsbTransfer::encode(&self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_core_usb_transfer_encode]

Builds the request frame for one transfer, through `put_header`. The meaning of the `a`, `b`, `value`, `index` and `len` fields depends on the variant: for the control operations `a` is the request type, `b` the request, and `len` the bytes to read (`ControlIn`) or the data length (`ControlOut`); for bulk and interrupt operations `a` is the endpoint; for an `InterruptExchange` `a` is the OUT endpoint, `b` the IN endpoint, `value` the read length and `len` the data length. Data longer than `MAX_DATA` is cut to `MAX_DATA`, which `decode` then reads back as a shorter transfer; engines never build such a transfer. Returns the frame bytes; cannot fail. Pure, allocates.

### `pub fn UsbTransfer::decode(bytes: &[u8]) -> Result<(UsbTransfer, usize), FrameError>` [@ANCHOR: radioctl:radioctl_core_usb_transfer_decode]

Decodes the first request frame at the start of `bytes` (a stream buffer that may hold several frames or a partial one) and returns the transfer with the number of bytes it consumed, so the caller drains that many. Fewer than nine bytes, or a write whose data field is not all there yet, gives `FrameError::Incomplete` (read more and call again). An unknown `op`, or a `len` over `MAX_DATA`, gives `FrameError::Malformed` (the stream is out of step; the link discards its buffer). Only the sending operations (2, 3, 5 and 7) carry data after the header; a read frame is exactly nine bytes. An exchange reads its length from `value`. Pure; borrows `bytes` and copies the data out.

### `pub fn UsbReply::encode(&self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_core_usb_reply_encode]

Builds the reply frame `op, status code, len(2), data`. The data is cut to `MAX_DATA` (4096) so the length field always fits; the link never produces more because requests are bounded the same way. Used by the link thread to write each answer back into the byte stream for the engine. Returns the bytes; pure, no failure.

### `pub fn UsbReply::decode(bytes: &[u8]) -> Result<(UsbReply, usize), FrameError>` [@ANCHOR: radioctl:radioctl_core_usb_reply_decode]

Decodes the first reply frame in `bytes` and returns it with the number of bytes it used. Fewer than four header bytes, or a data field not yet fully present, gives `FrameError::Incomplete`; a length over `MAX_DATA` gives `FrameError::Malformed`. The status byte goes through `UsbStatus::from_code`, so an unknown status is `Other`. Called by the USB protocol engines in `feed` to turn bytes from the link into replies. Pure; copies the data out of `bytes`.

### `fn status_of(failure: &UsbFailure) -> UsbStatus` [@ANCHOR: radioctl:radioctl_core_usb_status_of]

Maps a device-level failure to the reply status the engine sees: `UsbFailure::Timeout` to `UsbStatus::Timeout`, `NoDevice` to `NoDevice`, `Stall` to `Stall`, and `Other(_)` to `Other` (the backend's wording is dropped, because the reply frame carries only a status byte). Private helper of `execute`. The `NoDevice` status is the one the link treats as a lost device and answers with a reconnect; the others leave the link up. Pure and total.

### `pub fn execute(device: &mut dyn UsbDevice, transfer: &UsbTransfer, timeout: Duration) -> UsbReply` [@ANCHOR: radioctl:radioctl_core_usb_execute]

Performs one decoded transfer on an open device and returns the reply frame's content. `device` is borrowed mutably for the call; `timeout` bounds each blocking transfer (an exchange applies it to both halves). A read returns its bytes with status `Ok`; a write returns `Ok` only when the device accepted exactly the bytes given, and a short write is `UsbStatus::Other`, so a half-sent command never reads as success. A failed transfer returns an empty data field and the status of `status_of`. For `InterruptExchange` the write goes first and the read only if the write succeeded, with no other transfer in between (the FUNcube command and answer). The reply carries the request's `op`. It blocks, so it must be called from the link's own thread, never from an async worker. It never panics and has no error return.

## `crates/radioctl-core/src/usb_device.rs`

The device-access seam: `UsbTarget` says which device, `UsbDevice` is one open device, `UsbBackend` opens devices, and `FakeUsbBackend` is the in-memory bus that every test and every virtual twin uses; no test touches real USB hardware.

### `pub fn UsbTarget::matches(&self, vid: u16, pid: u16, manufacturer: &str, product: &str, serial: &str) -> bool` [@ANCHOR: radioctl:radioctl_core_usb_target_matches]

Decides whether a device that reports these ids and strings is the one named by the target. Vendor id and product id must be equal; an empty `manufacturer` or `product` in the target matches anything, otherwise the strings must be equal; a target serial of `None` matches any serial, otherwise it must be equal. The serial matters when two identical boards (the V-USB kits all share 16C0:05DC and differ by strings) are plugged in. Called by `LibusbBackend::open` on each enumerated device. Pure, no errors.

### `pub fn UsbTarget::label(&self) -> String` [@ANCHOR: radioctl:radioctl_core_usb_target_label]

A short text for status and logs: `usb:VVVV:PPPP` in four lowercase hex digits each, with `:serial` appended when the target names a serial. It is also the `port` shown in `LinkStatus::Connected` for a USB link and appears in the "is not plugged in" error. It never includes the manufacturer or product strings. Pure, allocates.

### `pub fn FakeUsbBackend::new(device: Box<dyn UsbDevice>) -> FakeUsbBackend` [@ANCHOR: radioctl:radioctl_core_fake_usb_new]

Creates a virtual bus with `device` plugged in (present flag true, zero opens, empty transfer log). The device is owned by the shared state; cloning the backend shares the same bus. The device keeps its state across unplug and replug, as a real board with its own power does, so a test can check that the rig restored its settings after a reconnect. Used by the virtual Si570, FiFi-SDR, FUNcube and Perseus twins and by the tests. Never fails; touches no hardware.

### `pub fn FakeUsbBackend::unplug(&self)` [@ANCHOR: radioctl:radioctl_core_fake_usb_unplug]

Marks the device absent. Handles that are already open then fail every transfer with `UsbFailure::NoDevice` (which the link turns into a reconnect), and `open` fails with `LinkDown`. Thread-safe (an atomic store with `SeqCst`); idempotent. Used by the hot-plug tests to simulate a pulled cable.

### `pub fn FakeUsbBackend::plug(&self)` [@ANCHOR: radioctl:radioctl_core_fake_usb_plug]

Marks the device present again, so the next `open` (the link's reconnect attempt) succeeds and returns a handle. Handles opened before the unplug stay useless only in the sense that the link drops them and reopens; the device's internal state is unchanged. Thread-safe, idempotent, no return value.

### `pub fn FakeUsbBackend::opens(&self) -> u32` [@ANCHOR: radioctl:radioctl_core_fake_usb_opens]

How many times `open` has succeeded on this bus (a failed open because the device is unplugged does not count). Tests read it to assert that a link reopened exactly once after an unplug. Atomic load; never blocks.

### `pub fn FakeUsbBackend::log(&self) -> Vec<FakeTransfer>` [@ANCHOR: radioctl:radioctl_core_fake_usb_log]

A copy of every transfer the device has received since the bus was created, oldest first, as `FakeTransfer` values (control in and out with their fields, bulk and interrupt reads and writes). A transfer is logged when it reaches the device, so one refused because the device was unplugged is not in the log. Tests use it to prove what an engine sent (for example that no transmit request was sent). Takes the log mutex briefly (a poisoned lock is recovered) and clones the vector.

### `fn FakeUsbBackend::open(&self, target: &UsbTarget) -> Result<Box<dyn UsbDevice>, RigError>` [@ANCHOR: radioctl:radioctl_core_fake_usb_open]

The `UsbBackend` implementation of the fake bus. If the device is unplugged it returns `RigError` of kind `LinkDown` with the message "`<label>` is not plugged in"; otherwise it counts the open and returns a new handle that shares the bus. The `target` is used only for the message: the fake serves one device and does not match ids or strings. The link retries a failed open with backoff.

### `fn FakeHandle::enter(&self, transfer: FakeTransfer) -> Result<MutexGuard<'_, Box<dyn UsbDevice>>, UsbFailure>` [@ANCHOR: radioctl:radioctl_core_fake_usb_handle_enter]

Common first step of every transfer through a fake handle. If the bus is unplugged it returns `UsbFailure::NoDevice` and logs nothing; otherwise it appends `transfer` to the log and returns the lock guard of the inner device, which the caller uses to forward the real call. Holding the guard serialises transfers from every handle on the bus. A poisoned mutex is recovered rather than propagated. Private.

### `fn FakeHandle::control_in(&mut self, request_type: u8, request: u8, value: u16, index: u16, length: u16, timeout: Duration) -> Result<Vec<u8>, UsbFailure>` [@ANCHOR: radioctl:radioctl_core_fake_usb_handle_transfers]

The six `UsbDevice` methods of a fake handle (`control_in`, `control_out`, `bulk_write`, `bulk_read`, `interrupt_write`, `interrupt_read`) share one pattern: call `enter` with the matching `FakeTransfer` (so an unplugged bus fails with `NoDevice` and a plugged one logs the transfer) and forward the identical arguments, including `timeout`, to the scripted or virtual device inside. Returns whatever that device returns: read bytes, bytes accepted, or a `UsbFailure`. Blocking only for the brief device lock; the virtual devices never sleep.

## `crates/radioctl-core/src/usb_libusb.rs`

The libusb implementation of the backend (cargo feature `usb`, off by default). Real hardware only; the failure mapping and the buffer helper are plain functions the tests do cover.

### `pub fn failure(error: rusb::Error) -> UsbFailure` [@ANCHOR: radioctl:radioctl_core_libusb_failure]

Translates a libusb error into what the link needs to decide. `Timeout` becomes `UsbFailure::Timeout`; `NoDevice` and `Io` become `NoDevice` (the device vanished or its bus failed, so the link reopens it); `Pipe` (a stall) becomes `Stall`, a refusal that leaves the link up; anything else becomes `Other` carrying the error text. Pure and total.

### `pub fn read_into(length: usize, read: impl FnOnce(&mut [u8]) -> rusb::Result<usize>) -> Result<Vec<u8>, UsbFailure>` [@ANCHOR: radioctl:radioctl_core_libusb_read_into]

Allocates a zeroed buffer of `length` bytes, passes it to the `read` closure (a libusb control, bulk or interrupt read), and returns the buffer cut to the byte count the closure reports. A closure error goes through `failure`. `length` is the most that is accepted (callers pass the frame's length field, at most 4096). A short read returns fewer bytes with no error; zero bytes returns an empty vector. The closure may block for the transfer's timeout.

## `crates/radioctl-core/src/usb_link.rs`

The USB link: a supervisor thread owns the open device and presents it to the actor as a byte stream of request and reply frames, with serial-style reconnect (status `Reconnecting`, bytes written meanwhile dropped and counted, exponential backoff, higher generation once reopened).

### `pub fn UsbConfig::new(target: UsbTarget) -> UsbConfig` [@ANCHOR: radioctl:radioctl_core_usb_config_new]

Creates a configuration for `target` with the defaults: a one-second `transfer_timeout`, the default `Backoff`, and `first_open_attempts` of 1 (one try at the first open; a device that is absent then fails the open at once; later losses are retried without limit). The fields are public so a caller can change them before `open_usb`. Owns the target. Pure.

### `pub fn UsbLink::status(&self) -> LinkStatus` [@ANCHOR: radioctl:radioctl_core_usb_link_status]

The current link status, cloned from the watch channel: `Connected { port, generation }`, `Reconnecting { attempt, last_error }` or `Closed`. A link that has not opened yet reads `Reconnecting` with attempt 0 and last error "opening"; `open_usb` returns only after the first open, so a caller normally sees `Connected`. Non-blocking.

### `pub fn UsbLink::watch(&self) -> watch::Receiver<LinkStatus>` [@ANCHOR: radioctl:radioctl_core_usb_link_watch]

A clone of the status receiver, which wakes on every status change. The rig actor receives one through `RigOptions::link_watch`: it marks the rig down while the status is `Reconnecting` and, on seeing `Connected` with a higher generation, unkeys the rig before any other command and emits `Alarm::LinkRestored`. Cheap; each caller gets its own receiver.

### `pub fn UsbLink::stats(&self) -> UsbStats` [@ANCHOR: radioctl:radioctl_core_usb_link_stats]

A snapshot of the link counters as `SerialStats` (the same shape as a serial link's): `bytes_in` (reply bytes sent to the actor), `bytes_out` (request bytes received from it), `bytes_dropped` (written while the device was away, never replayed) and `reconnects`. Read with relaxed atomics, so the four values are not mutually consistent at one instant. Non-blocking.

### `pub fn UsbLink::into_parts(self) -> (DuplexStream, UsbMonitor)` [@ANCHOR: radioctl:radioctl_core_usb_link_into_parts]

Consumes the link and splits it into the byte stream (handed to `rig::spawn`, owned by the actor) and a `UsbMonitor` the owner keeps for status and statistics. Dropping the stream closes the device (the supervisor sees the end of input and shuts down); dropping the monitor does not. Pure move, no failure.

### `pub fn UsbMonitor::status(&self) -> LinkStatus` [@ANCHOR: radioctl:radioctl_core_usb_monitor_status]

Same as `UsbLink::status`, for the part of the link that stays with the owner after the stream went to the actor: the current `LinkStatus` cloned from the watch channel: `Connected { port, generation }`, `Reconnecting { attempt, last_error }` or `Closed`. Use it for a status line or an alarm; the actor itself learns of changes through the watch receiver it was given. Non-blocking, no error return.

### `pub fn UsbMonitor::watch(&self) -> watch::Receiver<LinkStatus>` [@ANCHOR: radioctl:radioctl_core_usb_monitor_watch]

A new receiver that wakes on every status change, for a supervisor that watches the link after the stream has moved to the actor (for example `radioctld`'s status line). Each call returns an independent receiver. Non-blocking.

### `pub fn UsbMonitor::stats(&self) -> UsbStats` [@ANCHOR: radioctl:radioctl_core_usb_monitor_stats]

A snapshot of the same counters as `UsbLink::stats` (`bytes_in`, `bytes_out`, `bytes_dropped`, `reconnects`), read from the shared atomics with relaxed ordering. The counters keep counting for as long as the supervisor thread runs. Non-blocking.

### `fn Supervisor::run(self, first: oneshot::Sender<Result<(), RigError>>)` [@ANCHOR: radioctl:radioctl_core_usb_supervisor_run]

The body of the `radioctl-usb` thread. It makes up to `first_open_attempts` (at least one) tries to open the device through the backend, waiting `backoff.delay(attempt)` between tries through `sleep_dropping`. If none succeeds it reports the last error on `first` and publishes `Closed`. Once open it publishes `Connected` with generation 1, signals `first` with `Ok`, and runs `session` until shutdown or loss. After a loss it drops the device, publishes `Reconnecting { attempt, last_error }` for each retry, waits with `sleep_dropping`, and reopens with no attempt limit; each success raises the generation and counts a reconnect. On shutdown it publishes `Closed` and returns. It is the only owner of the device, so transfers never overlap. It never keys a device.

### `fn Supervisor::sleep_dropping(&self, delay: Duration) -> bool` [@ANCHOR: radioctl:radioctl_core_usb_sleep_dropping]

Waits `delay` while draining the write queue, so the actor never blocks on a missing device and stale commands are never replayed into a device that comes back. Every `Command::Write` received during the wait is discarded and its length added to `bytes_dropped`. Returns `false` when a `Shutdown` command arrives or the actor's end of the queue is gone (stop reconnecting), and `true` when the full delay has elapsed. Runs on the supervisor thread and blocks it.

## `crates/radioctl-protocols/src/usb_family.rs`, `funcube.rs`, `si570_usb.rs`

The protocol engines that speak the frames above, and the virtual boards (`twins`) the tests and virtual rigs use in place of hardware.

### `fn usb_of(desc: &RigDescription) -> Result<&UsbParams, RigError>` [@ANCHOR: radioctl:radioctl_usb_family_usb_of]

Returns the USB parameters of a rig description (the `[usb]` table: protocol, ids, strings, endpoints) when its family is `Family::UsbDevice` and the table is present. Any other description (a different family, or a USB family with no table) is refused with `ErrorKind::Unsupported` and the message "not a USB device description". Used by `engine_for` and by the twin and target builders so a wrongly described rig fails at construction. Borrows the description, pure.

### `fn interrupt_read(&mut self, endpoint: u8, length: usize, _timeout: Duration) -> Result<Vec<u8>, UsbFailure>` [@ANCHOR: radioctl:radioctl_funcube_twin_interrupt_read]

The virtual FUNcube Dongle's HID input report. If the endpoint is not the input endpoint, or no answer is waiting (nothing was written first), it returns `UsbFailure::Timeout`, as a real dongle with no pending report would. Otherwise it takes the pending answer (the 64-byte report prepared by the last `interrupt_write`), truncates it to `length`, and returns it; the answer is consumed, so a second read times out. `timeout` is ignored. Part of the virtual twin; touches no hardware.

### `fn control_out(&mut self, request_type: u8, request: u8, value: u16, index: u16, data: &[u8], _timeout: Duration) -> Result<usize, UsbFailure>` [@ANCHOR: radioctl:radioctl_si570_usb_twin_control_out]

The virtual Si570 or FiFi-SDR board's control-out handler. For a FiFi-SDR it accepts the vendor write request with `value` 0 and sets volume (2 bytes), mode (1 byte, at most 3), width (4 bytes) or preamp (1 byte, 0 or 1) according to `index`; anything else stalls. For the older Si570 boards it accepts the register write (six bytes, legacy) or the set-frequency-by-value word (four bytes) only when `value` equals the expected I2C-address value (`0x700` plus the Si570 address, or the same for legacy). Returns the number of bytes accepted, or `UsbFailure::Stall` for any other request, wrong length or out-of-range value. Bulk and interrupt transfers always stall. Virtual only; no hardware.

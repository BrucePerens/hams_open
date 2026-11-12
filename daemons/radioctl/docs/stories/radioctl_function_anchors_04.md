# radioctl function anchors, batch 04

Function anchors for the Perseus engine, session and loader, the Digital World Traveller engine, the `usb-device` family dispatch (`radioctl-protocols`), the USB rig start-up in `radioctld`, and the VFO and transmit-state helpers of `radioctl-core`. Tests cite each anchor with `Tests [@ANCHOR: ...]`.
The Perseus and the Traveller are receive only and have no transmit path. `TxState` derives which frequency a rig would transmit on; it keys nothing, but its output is what the transmit gate and band check read, so a wrong answer there would defeat them.

## `crates/radioctl-protocols/src/perseus_engine.rs`

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_perseus_engine_encode]

Turns one operation into one control-out frame addressed to the Perseus session (not to the board itself). `op` is borrowed. Any transmitting operation or `SetPtt` is refused first with `Unsupported` ("a receiver and cannot transmit"), then a capability the description lacks. Accepted: set frequency on VFO A (8 bytes of hertz, little-endian, request 0x21; above 40 MHz is refused); attenuator in decibels (0, 10, 20, 30 as the description allows, request 0x22, checked with `attenuation::step_of`); preamp step (0x24, also checked); dither and preselector, 0 or 1 only (0x23, 0x25). Everything else, including every read, is `Unsupported`, because the board cannot be read back. It sets the engine to wait for one answer and clears the buffer; the reply kind is always `Ack`. Nothing is sent to the radio here.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_perseus_engine_feed]

Interprets reply frames returned by the Perseus session. `bytes` are appended to a buffer, then each complete `UsbReply` is decoded (`_now` is unused). If no request is waiting the reply is reported as `Inbound::Unrecognised`. Otherwise the wait is cleared and the status decides: `Ok` gives `Ack`, `Timeout` and `NoDevice` give nothing (silence; the actor ends the command in its own timeout), anything else (`Stall`, `Other`) gives `Nak`. A malformed frame moves the whole buffer into one `Unrecognised` and stops. Only one request is outstanding at a time. Pure; never blocks.

### `pub fn new(desc: RigDescription) -> Result<PerseusEngine, RigError>` [@ANCHOR: radioctl:radioctl_perseus_engine_new]

Builds the Perseus client engine from a description, which is taken by value and kept. It succeeds only when the USB protocol is `perseus`; otherwise it returns `ErrorKind::Unsupported` ("not a Perseus description"), including for a description with no USB block. The engine starts not waiting for any reply and with an empty buffer. No I/O is done. The firmware loader and front-end control are not in the engine; they live in the loader backend and the session that the USB link opens. Called by `usb_family::engine_for`.

### `fn transfer(request: u8, value: u16, data: Vec<u8>) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_perseus_engine_transfer]

Builds the encoded USB request frame for one session command: a `UsbTransfer::ControlOut` with the session request type 0x40, the given `request` number (0x21 tune, 0x22 attenuator, 0x23 dither, 0x24 preamp, 0x25 preselector), `value` (decibels or 0/1, unused for tune), wIndex 0 and `data` (owned; the 8-byte frequency for tune, empty otherwise). Returns the owned frame bytes for `Request.bytes`. An associated function with no engine state; pure and infallible. These are radioctl-internal requests: the session decodes them and turns them into the board's own bulk commands, so they are never sent to the board as they stand.

## `crates/radioctl-protocols/src/perseus.rs`

### `pub fn from_device(mut dev: Box<dyn UsbDevice>) -> Result<PerseusControl, RigError>` [@ANCHOR: radioctl:radioctl_perseus_from_device]

Builds a `PerseusControl` over a board that is already running firmware with its FPGA configured, without loading anything. `dev` is the open device, taken by value (owned by the control from then on). It reads the EEPROM product record with `read_product_id`, so it can fail with that function's errors (`LinkDown`, `Timeout`, `Rejected` or `Malformed`). The front-end byte, serial-IO control and word start at 0 and no preselector is recorded. Used by the virtual ready board; the real path is `open_ready`, which also loads firmware and configures the FPGA. Blocking, one command and a status read.

### `pub fn new(inner: Arc<dyn UsbBackend>, files: PerseusFiles, real_time: bool) -> PerseusLoaderBackend` [@ANCHOR: radioctl:radioctl_perseus_loader_new]

Creates the backend that opens a Perseus and loads the user's firmware when needed. `inner` is the backend that opens raw USB devices (shared, so the loader can open the blank and the running board); `files` gives the paths of `fx2.hex` and `fpga.rbs` (owned); `real_time` true makes the loader really sleep for the board to renumber (4 s per attempt) and for the FPGA check (50 ms), false skips the sleeping, for a virtual board that needs none. No I/O and no file is read until `open`. Cannot fail. The result is wrapped in an `Arc` by `usb_family::backend_for` and handed to the USB link, which calls it on each (re)open.

### `fn open(&self, _target: &UsbTarget) -> Result<Box<dyn UsbDevice>, RigError>` [@ANCHOR: radioctl:radioctl_perseus_loader_open]

The `UsbBackend` entry of the loader, called by the USB link on the first open and on every reopen. The target is ignored (the loader knows the two ids itself). It runs `PerseusControl::open_ready` with a wait function that sleeps only when `real_time` was set, so on a real board it can block the supervisor thread for several seconds (firmware load, renumbering, FPGA configuration); this is the reason the link drives devices from its own thread. It returns a boxed `PerseusSession` over the ready control. Errors are those of `open_ready`: a missing firmware file naming its path, no renumber, EEPROM or FPGA failures. After an unplug the link reopens through here, which reloads the firmware. Receive only.

### `pub fn board(&self) -> &VirtualPerseusBackend` [@ANCHOR: radioctl:radioctl_perseus_ready_virtual_board]

Returns a borrowed reference to the underlying virtual board of a `ReadyVirtualPerseus`, so a test can read what the board has been told through `VirtualPerseusBackend::state`: RAM writes, the FPGA image, the last front-end byte, serial-IO word and whether shutdown was received. The reference lives as long as the `ReadyVirtualPerseus`; since the board is shared behind a lock, state read this way is a snapshot. No side effects and no failure.

### `pub fn new(serial: u16) -> ReadyVirtualPerseus` [@ANCHOR: radioctl:radioctl_perseus_ready_virtual_new]

Creates a virtual Perseus whose firmware is already running and whose FPGA reports DONE, so it can be opened without any firmware files. `serial` is the serial number its EEPROM record reports; the unit is a production unit (product code 0x8014). Built from `VirtualPerseusBackend::new(true, serial, true)`. The value is `Clone`: all clones are the same board. No I/O, cannot fail. Used by `usb_family::twin_for` (serial 1) so a virtual Perseus rig works with no hardware.

### `fn open(&self, _target: &UsbTarget) -> Result<Box<dyn UsbDevice>, RigError>` [@ANCHOR: radioctl:radioctl_perseus_ready_virtual_open]

The `UsbBackend` entry of the ready virtual board. The target is ignored. It opens the running virtual board, reads its product record through `PerseusControl::from_device` and returns a `PerseusSession` over it, so the caller gets the same kind of device a real loader returns. Errors only if the virtual board cannot be opened as the running board or its record cannot be read (it can, as the board is created running). Each call makes a new session on the same shared board; dropping a session sends the board's shutdown command, which the state records.

### `fn control_out(&mut self, request_type: u8, request: u8, value: u16, _index: u16, data: &[u8], _timeout: Duration) -> Result<usize, UsbFailure>` [@ANCHOR: radioctl:radioctl_perseus_session_control_out]

Executes a radioctl-internal session request, the device side of the engine's frames. `request_type` must be 0x40, otherwise `Stall`. Requests: 0x21 tune, `data` exactly 8 bytes of hertz little-endian (else `Stall`), then `set_frequency` with the current preselector setting; 0x22 attenuator, `value` decibels up to 255, then `set_attenuator_db`; 0x23 dither and 0x24 preamp, `value` 0 or 1 (otherwise `Stall`), applied with `set_adc` together with the other flag; 0x25 preselector, `value` 0 or 1, retunes at the remembered frequency (or only records the setting before any tune). The session's remembered frequency, dither, preamp and preselector change only after the board accepted the command. Board errors map to `NoDevice` (link down), `Timeout`, or `Stall`. Success returns the data length. Blocking, one to two bulk exchanges.

### `fn drop(&mut self)` [@ANCHOR: radioctl:radioctl_perseus_session_drop]

Runs when a Perseus session is discarded: when the USB link closes the device, when the link reopens after a loss, or when the rig stops. It sends the board's shutdown command (reset the FPGA, disconnect the preselector filters and set 30 dB attenuation), the safe state, and ignores any error, because the device may already be gone and a destructor cannot report. Cannot panic on its own. Blocking for up to the one second transfer timeout. This keeps an abandoned receiver from being left with its front end in an arbitrary state; it does not affect any transmitter, as the board has none.

### `fn failure(error: &RigError) -> UsbFailure` [@ANCHOR: radioctl:radioctl_perseus_session_failure]

Maps a `RigError` from the control layer back to the `UsbFailure` the link understands, so that the session looks like an ordinary USB device. `ErrorKind::LinkDown` becomes `NoDevice` (the link then reopens the device and reloads the firmware), `Timeout` becomes `Timeout`, and every other kind (`Rejected`, `Unsupported`, `Malformed`) becomes `Stall`, which the engine reports as `Nak`. `error` is borrowed; pure; cannot fail. The opposite direction is `usb_err`.

### `pub fn new(control: PerseusControl) -> PerseusSession` [@ANCHOR: radioctl:radioctl_perseus_session_new]

Wraps a ready `PerseusControl` (taken by value) in a session, the `UsbDevice` that the link drives. The session starts with no remembered frequency, preselection on, ADC dither off and ADC preamplifier off; these are the values the session combines with each new request (for example it needs the preamp flag when dither changes, because both share one control byte). Nothing is sent to the board, so the board's actual state is what `open_ready` left. Cannot fail. When the session is dropped the board is shut down.

## `crates/radioctl-protocols/src/usb_family.rs`

### `pub fn backend_for(desc: &RigDescription, inner: Arc<dyn UsbBackend>, firmware_dir: Option<&Path>, env_dir: Option<&str>, home: Option<&str>, real_time: bool) -> Result<Arc<dyn UsbBackend>, RigError>` [@ANCHOR: radioctl:radioctl_usb_family_backend_for]

Chooses the backend through which a description's device is opened. For every board but the Perseus it returns `inner` unchanged. For a Perseus it returns a `PerseusLoaderBackend` over `inner` that loads the user's firmware. The firmware directory is `firmware_dir` (from the rig's transport configuration) if given, else the directory from `RADIOCTL_PERSEUS_FIRMWARE_DIR` (`env_dir`), else `<home>/.config/radioctl/firmware/perseus`. The environment values are passed in rather than read here. Errors: `Unsupported` if the description is not a USB device, or for a Perseus when none of the three gives a directory. `real_time` false skips the loader's pauses, for a virtual board. No file is read here.

### `pub fn engine_for(desc: &RigDescription) -> Result<Box<dyn ProtocolEngine>, RigError>` [@ANCHOR: radioctl:radioctl_usb_family_engine_for]

Picks the protocol engine for a USB-device description. `desc` is borrowed and cloned into the engine. The USB protocol selects: `si570`, `si570-legacy` and `fifisdr` give the Si570 engine; `funcube` the FUNcube engine; `elektor507`, `dwt` and `perseus` their engines. Errors: `Unsupported` ("not a USB device description") when the family is not `usb-device` or the USB block is missing, and any error of the chosen engine constructor. Pure and cheap; no I/O. Called when a rig is created from a description.

### `pub fn twin_for(desc: &RigDescription) -> Result<Box<dyn UsbDevice>, RigError>` [@ANCHOR: radioctl:radioctl_usb_family_twin_for]

Builds the virtual device for a USB-device description, to run behind the fake USB backend for virtual rigs and tests. `desc` is borrowed. Si570 protocols give an `Si570Twin`, `funcube` a current-firmware `FuncubeTwin`, `dwt` a `DwtTwin`, `elektor507` an `Elektor507Twin`, and `perseus` a ready virtual Perseus (serial 1) opened as a session. Errors: `Unsupported` for a description that is not a USB device, plus the constructor's own errors (for instance a default frequency out of range). No hardware is touched. The twin answers the same frames as the real board, so the same engine and link run over it.

### `pub fn target_for(desc: &RigDescription) -> Result<UsbTarget, RigError>` [@ANCHOR: radioctl:radioctl_usb_family_target_for]

Describes the device to look for on the USB bus. It copies the vendor id, product id, manufacturer string and product string from the description's USB block, with no serial number, so any matching board is accepted; the daemon's configuration (`vid`, `pid`, `serial` in the rig's usb transport) then narrows it. The interface to claim is 2 for the FUNcube (its HID interface) and none for the others. `desc` is borrowed; the result is an owned `UsbTarget`. Error: `Unsupported` if the description is not a USB device. Pure.

### `pub fn new(desc: &RigDescription) -> Result<UsbFrameStandIn, RigError>` [@ANCHOR: radioctl:radioctl_usb_family_stand_in_new]

Creates a byte-in, byte-out stand-in for a USB board: a virtual device (from `twin_for`) with an empty buffer for partial frames. `desc` is borrowed. Its purpose is the conformance harness, which replays transcripts of request frames, the very bytes the engine writes, and compares the reply frames. Errors are those of `twin_for` (`Unsupported` for a non-USB description or a bad default). No hardware, no I/O.

### `pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_usb_family_stand_in_feed]

Gives the stand-in some request bytes and returns the reply bytes. `bytes` are appended to the stand-in's pending buffer, then every complete request frame is decoded and performed on the virtual device with a one second timeout, its encoded reply appended to the result. An incomplete frame is kept for the next call. A malformed frame clears the pending bytes and adds a reply with op 0 and status `Other`, then stops (the remaining bytes are lost). Never returns an error: a failed transfer is reported in its reply status. The result may be empty. It mutates the twin. Single threaded; the same logic as the link's `serve`.

## `crates/radioctld/src/rigs.rs`

### `async fn start_usb_rig(runtime: &Handle, engine: BoxedEngine, backend: Arc<dyn UsbBackend>, usb: UsbConfig, mut options: RigOptions) -> Result<(RigHandle, UsbMonitor), RigError>` [@ANCHOR: radioctl:radioctld_rigs_start_usb_rig]

Opens the USB link and starts the rig actor over it; the daemon calls it for a virtual USB board (with the fake backend) and for a real one. `runtime` is the Tokio handle; `engine` is the protocol engine (owned); `backend` opens the device (shared); `usb` holds the target, timeout and backoff; `options` is taken by value and has `link_watch` set to the link's status receiver so the actor refuses commands while the link is down and unkeys the rig when it returns. It waits for `open_usb` (first open) then calls `rig::spawn`. Returns the rig handle and the `UsbMonitor` the daemon keeps for status and counters. Errors: the first open's error (no board, no permission, a missing firmware file) or a spawn error; a failure there stops the daemon from starting. Async; the device work runs on the link thread.

### `fn real_usb_backend() -> Result<Arc<dyn UsbBackend>, RigError>` [@ANCHOR: radioctl:radioctld_rigs_real_usb_backend]

The backend for a real USB device in a build with the cargo feature `usb`: it creates a `LibusbBackend`, which initialises libusb. It takes no arguments and returns a shared backend. The only error is the libusb initialisation failure (`Unsupported`, "libusb: ..."). Called once when a rig with a real `usb` transport is started, then handed to `usb_family::backend_for` and to the link. There is a twin with the same name for builds without the feature (see `real_usb_backend` for builds without USB support). No device is opened here.

### `fn is_line_driven(description: &RigDescription) -> bool` [@ANCHOR: radioctl:radioctld_rigs_is_line_driven]

Tells whether a rig is driven by control-line sequences instead of bytes. It is true when the description has a vendor block whose protocol is `Drt1` (the SAT-Schneider DRT1 receiver); false for all others. `description` is borrowed. Pure and cannot fail. It is used when a virtual rig is started, to choose a line-driven twin whose "transport" is the control lines the actor drives, with an idle byte stream that nothing writes to; a rig that is not line driven gets the ordinary byte-stream twin.

### `fn real_usb_backend() -> Result<Arc<dyn UsbBackend>, RigError>` [@ANCHOR: radioctl:radioctld_rigs_real_usb_backend_missing]

The other variant of `real_usb_backend`, compiled when the `usb` feature is off. It opens nothing and always fails at once with `ErrorKind::Unsupported` and a message saying that this radioctld was built without USB support (cargo feature `usb`, which needs libusb-1.0). The point is to give a clear error when the daemon is started with a `usb` transport, rather than fail later with a vague message. No arguments, no side effects. Virtual USB rigs do not use it, since they run over the fake backend.

## `crates/radioctl-protocols/src/dwt.rs`

### `pub fn new(desc: RigDescription) -> Result<DwtEngine, RigError>` [@ANCHOR: radioctl:radioctl_dwt_engine_new]

Builds the engine for the Digital World Traveller DRM receiver (USB 1539:1730) from a description taken by value. It succeeds only when the USB protocol is `dwt`; otherwise `ErrorKind::Unsupported` ("not a Digital World Traveller description"). The engine starts not waiting for a reply and with an empty buffer. No I/O. The receiver can only be tuned; there is no read-back and no transmit.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_dwt_engine_encode]

Encodes a set-frequency operation as one control-out transfer. `op` is borrowed. The request has type 0, request 0, wValue 0, wIndex 0 and nine data bytes: the fixed prefix `4A 00 03 00 FF FF 32` and the frequency in whole kilohertz as 16 bits, high byte first (the hertz value is divided by 1000, truncated). Errors, all `Unsupported`: capability missing from the description; any operation other than a set frequency on VFO A; a frequency over 65535 kHz (Hamlib would wrap it; this engine refuses). Sets the engine to wait for one `Ack`. Receive only. Not verified on a receiver.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_dwt_engine_feed]

Interprets reply frames from the link. `bytes` are buffered and each complete `UsbReply` decoded (`_now` unused). With no request waiting the reply is `Inbound::Unrecognised`. Otherwise `Ok` gives `Ack`; `Timeout` and `NoDevice` give nothing, so the actor ends the command in its own timeout; `Stall` or `Other` give `Nak`. A malformed frame puts the buffer into one `Unrecognised` and stops. The receiver has no read, so only acknowledgements are produced. Pure; never blocks.

### `pub fn khz(&self) -> Option<u16>` [@ANCHOR: radioctl:radioctl_dwt_twin_khz]

Returns the frequency, in whole kilohertz, to which the virtual receiver was last tuned, or `None` if it has never received a tune request. The value is the 16-bit number the twin decoded from the last accepted transfer, high byte first. A plain read with no side effects, no locking and no failure. Tests use it to confirm what the engine put on the wire, for example that 7 MHz arrives as 7000.

### `fn control_out(&mut self, request_type: u8, request: u8, value: u16, index: u16, data: &[u8], _timeout: Duration) -> Result<usize, UsbFailure>` [@ANCHOR: radioctl:radioctl_dwt_twin_control_out]

The virtual Traveller's reception of a control transfer. It accepts only request type 0, request 0, wValue 0, wIndex 0 and exactly nine data bytes whose first seven equal the fixed prefix `4A 00 03 00 FF FF 32`; the last two are stored as the tuned kilohertz, high byte first, and the data length is returned. Anything else, including a wrong length or prefix, returns `UsbFailure::Stall`, which the engine reports as `Nak`. The timeout is ignored. The only state it changes is the stored kilohertz.

## `crates/radioctl-core/src/units.rs`

### `pub fn other(self) -> Vfo` [@ANCHOR: radioctl:radioctl_core_vfo_other]

Returns the opposite VFO: VFO B for VFO A and VFO A for VFO B. `self` is the `Vfo` by value (it is `Copy`). It is used where a rig that has split on but no reported transmit VFO is assumed to transmit on the VFO it is not receiving on (see `TxState::tx_vfo`), and when swapping or copying VFOs. Pure, no errors, applying it twice gives the original.

## `crates/radioctl-core/src/txstate.rs`

### `pub fn from_cache(get: impl Fn(Field) -> Option<Observation>) -> TxState` [@ANCHOR: radioctl:radioctl_core_txstate_from_cache]

Collects the inputs of the transmit-frequency derivation from the actor's cache. `get` is a closure the caller supplies; given a `Field` it returns the cached `Observation` or `None`. The function asks for the receive VFO, the transmit VFO, the split flag and the frequencies of VFO A and VFO B, and keeps each only when the observation has the matching kind (a wrong kind counts as unknown). The result is a `Copy` `TxState` in which every field is an `Option`. Pure, no I/O, no locks beyond what `get` takes. A value the rig has not reported stays `None`, which later functions treat conservatively.

### `pub fn rx_vfo(&self) -> Vfo` [@ANCHOR: radioctl:radioctl_core_txstate_rx_vfo]

Returns the VFO the rig receives on: the reported receive VFO, or VFO A when none has been reported (the selected VFO on CI-V rigs, where the unselected one is VFO B). Pure, no arguments, cannot fail. It always returns an answer, even when the cache is empty, so callers must not treat it as proof that the rig reported anything. Used by `tx_vfo` and `rx_frequency`.

### `pub fn tx_vfo(&self) -> Vfo` [@ANCHOR: radioctl:radioctl_core_txstate_tx_vfo]

Returns the VFO the rig transmits on. If split is known to be off, it is the receive VFO. Otherwise, if the rig reported a transmit VFO, that one is used (even when split is not yet known). With split known to be on and no reported transmit VFO, it is the other VFO. With nothing known it is the receive VFO. Pure and total. Because the unknown-split case assumes "not split", this answer alone must not be used as a gate for keying; `known_for_transmit` insists that the split state has been read for a rig that can split.

### `pub fn rx_frequency(&self) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_core_txstate_rx_frequency]

Returns the receive frequency in hertz: the cached frequency of the receive VFO (`rx_vfo`), or `None` until it has been read. No arguments; pure; it does not consider RIT, which would move the actual receive frequency by a few kilohertz. A follower that wants the VFO frequency (a band decoder, for example) can rely on this.

### `pub fn tx_frequency(&self) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_core_txstate_tx_frequency]

Returns the frequency of the transmit VFO (`tx_vfo`) in hertz, or `None` when that VFO's frequency has not been read. It equals the receive frequency when the rig is not split. XIT is not folded in. Pure, cannot fail. This is the value the derivation publishes as the active transmit frequency, but because it assumes "not split" when split is unread, the transmit gate uses `known_for_transmit` instead.

# radioctl function anchors, batch 19

Function anchors for `radioctl-protocols`: the OpenHPSDR bridge (`hpsdr_io.rs`), the virtual rig (`virtual_rig.rs`, the Elecraft and CI-V halves and the rig-level API), and the CI-V frame decoder and Kenwood decoder and dialect encoders that the engines use. Tests cite each anchor with `Tests [@ANCHOR: ...]`. Transmit gate: the virtual rigs never transmit (keying only sets an in-memory flag that tests read). In the HPSDR bridge a `P 1` line is the one thing that can key a radio; the rig actor sends it only with its own transmit gate open, and the bridge then opens the `hpsdr` crate's gate for a bounded lease. The Kenwood encoders here build `TX;` and `KY` bytes but the actor, not these functions, applies the gate.

## `crates/radioctl-protocols/src/hpsdr_io.rs`

### `fn default() -> Self` [@ANCHOR: radioctl:radioctl_hpsdr_options_default]

`Default` for `HpsdrOptions`, the bridge's settings. `ptt_lease` is 3 seconds: how long one `P 1` keys the radio before the bridge unkeys by itself. `drive` is 0, the drive level (0 to 255) sent while keyed, so a default bridge keys with no drive. `status_every` is 250 ms, the interval of the unsolicited power and SWR lines. `power_constant` is 0.095, the SWR power constant for an ANAN-100 (0.09 is the ANAN-10 value). Pure, cannot fail. A caller that wants drive must set it explicitly.

### `pub fn set_attenuator(&self, db: u8) -> Result<(), hpsdr::error::HpsdrError>` [@ANCHOR: radioctl:radioctl_hpsdr_control_set_attenuator]

`HpsdrControl::set_attenuator`: sets the step attenuator of the served session directly, bypassing the line protocol (the actor reaches the same thing as `Level::Attenuator`). `db` is whole decibels, 0 to 31. It takes the bridge's link mutex (a poisoned lock is recovered) for the call and returns the session's own error for a value the session refuses. When the bridge task has already stopped the link is gone and the result is `Err(HpsdrError::Stopped)`. It does not record the value in the bridge's last-attenuation field, so a later `GA` can differ from this setting. No RF is involved.

### `pub fn status(&self) -> HardwareStatus` [@ANCHOR: radioctl:radioctl_hpsdr_control_status]

`HpsdrControl::status`: returns the latest `HardwareStatus` (forward and reverse power counts and the other counters the `hpsdr` crate keeps) of the served session, asking the Protocol 1 or Protocol 2 session as appropriate. Takes the link mutex briefly. When the bridge has stopped it returns the default (all zero) status rather than an error, so a caller cannot tell a stopped bridge from a silent radio. No I/O of its own.

### `pub fn take_streams(&self) -> Vec<hpsdr::iq::ChannelStream>` [@ANCHOR: radioctl:radioctl_hpsdr_control_take_streams]

`HpsdrControl::take_streams`: hands over the session's receive I/Q streams (the data plane) to the caller. The streams are moved out of the session, so only the first call gets them and later calls return an empty vector; it also returns an empty vector when the bridge has stopped. Takes the link mutex mutably for the call. This is receive data only; nothing here transmits.

### `fn set_attenuator(link: &HpsdrLink, db: u8) -> Result<(), hpsdr::error::HpsdrError>` [@ANCHOR: radioctl:radioctl_hpsdr_set_attenuator]

Sets the step attenuator of either session type, in whole decibels (the doc says the session refuses more than 31). On Protocol 1 it sets the attenuation and enables the attenuator in the session's configuration; on Protocol 2 it sets the first ADC's attenuation. `link` is borrowed, and the update goes through the session's `update` method, so its result is the session's own error (for example when the value is refused or the session has stopped). Used by the `A <db>` line and by `HpsdrControl::set_attenuator`. No transmit effect.

### `fn set_frequency(link: &HpsdrLink, hz: u32) -> Result<(), hpsdr::error::HpsdrError>` [@ANCHOR: radioctl:radioctl_hpsdr_set_frequency]

Retunes the first receiver (DDC0) of either session to `hz` hertz (a `u32`), and sets the transmit frequency to the same value, so the two follow each other. `link` is borrowed; the change is sent through the session's `update`, whose error (for example a stopped session) is returned. Protocol 1 and Protocol 2 use the same two fields (`rx_hz[0]` and `tx_hz`). No band check is made here: the engine range-limits the value to 32 bits, and any transmit-band policy sits above this layer. Used by the `F <hz>` line.

### `fn request_ptt(link: &HpsdrLink, on: bool, drive: u8) -> Result<(), hpsdr::error::HpsdrError>` [@ANCHOR: radioctl:radioctl_hpsdr_request_ptt]

Keys (`on` true) or unkeys the radio through the `hpsdr` session, whichever protocol it is. `drive` is the drive level 0 to 255 sent while keyed. The session re-checks its own transmit gate before every packet, so keying fails (an `HpsdrError`) if the gate is closed. The caller, `handle_line`, opens the gate for a lease first and closes it again on failure or unkey. This is the function that can put RF on the air through an HPSDR radio; it does not check the gate, band or lock itself.

### `fn handle_line(line: &str, shared: &Shared, st: &mut State, opts: &HpsdrOptions) -> String` [@ANCHOR: radioctl:radioctl_hpsdr_handle_line]

Answers one trimmed request line from the engine and returns the reply text (LF terminated). Holds the link mutex for the whole call; replies `ERR stopped` when the link has gone. `GF`, `GP`, `GA` read the bridge's remembered frequency, key state and attenuation; `GW` is the forward power count; `GR` is the SWR in tenths via `swr_tenths`. `A <db>` sets the attenuator (reply `ERR attenuator` for a value above 255 or one the session refuses). `F <hz>` retunes (`ERR frequency` above 32 bits or on session failure). `P 1` opens the `hpsdr` transmit gate until now plus `ptt_lease`, then requests PTT (push to talk) with the configured drive: `OK` on success, otherwise the gate is closed again and `ERR <reason>`. `P 0` unkeys, always closes the gate and clears the key state, replying `OK` or `ERR unkey`. Anything else is `ERR command`. The rig actor sends `P 1` only with its own gate open.

### `fn status_of(link: &HpsdrLink) -> HardwareStatus` [@ANCHOR: radioctl:radioctl_hpsdr_status_of]

Returns the latest hardware status of either session type by asking whichever the `link` is (Protocol 1 or Protocol 2). `link` is borrowed. Pure apart from reading the session's status snapshot; no I/O, cannot fail. Used for the `GW` and `GR` replies and the unsolicited status lines.

### `fn swr_tenths(st: &HardwareStatus, opts: &HpsdrOptions) -> u16` [@ANCHOR: radioctl:radioctl_hpsdr_swr_tenths]

Converts the forward and reverse power counts of a hardware status into SWR in tenths of a unit (10 is 1.0:1), using the bridge's `power_constant`. The result is rounded and clamped to 10 through 999. When the status gives no SWR (no forward power), the result is 10. The formula itself lives in the `hpsdr` crate's `HardwareStatus::swr`; unknown from this crate whether a given radio's constant matches the default. Pure.

### `pub fn serve_hpsdr( runtime: &Handle, link: HpsdrLink, gate: TxGate, freq: u32, opts: HpsdrOptions, ) -> (DuplexStream, HpsdrControl)` [@ANCHOR: radioctl:radioctl_hpsdr_serve]

Starts the bridge task on `runtime` and returns an in-memory duplex stream (the transport the rig actor reads and writes) and a cloneable `HpsdrControl`. `link` (a running session) and `gate` (the session's closed `TxGate`) are moved in; `freq` is the starting frequency the bridge reports; `opts` supplies the lease, drive and status interval. The task reads LF-terminated request lines (longer than 128 bytes are truncated) from the stream, answers each through `handle_line`, and every `status_every` writes unsolicited `W`, `R` and `P` lines. If the gate's lease has run out while keyed it unkeys the radio by itself and reports `P 0`. When the actor drops its end, the task closes the gate and stops the session. The duplex buffer is 4096 bytes. Locks: the link mutex is held only during one line or one status tick.

### `pub async fn start_session( runtime: &Handle, radio: &hpsdr::discovery::Discovered, protocol2_ports: Option<hpsdr::p2::RadioPorts>, freq: u32, ) -> Result<(HpsdrLink, TxGate), hpsdr::error::HpsdrError>` [@ANCHOR: radioctl:radioctl_hpsdr_virtual_anan]

Starts an `hpsdr` session against a radio the caller already found (in tests, a loopback emulator, which is why the anchor is called the virtual ANAN) and returns the link and its transmit gate. `radio` is the discovered radio; `protocol2_ports` selects the protocol (`None` starts Protocol 1, `Some(ports)` Protocol 2); `freq` sets all receiver frequencies and the transmit frequency. The gate is a fresh, closed `TxGate` that the session and the bridge share, so nothing can transmit until the bridge opens it. Async; it awaits the session start and returns the session's `HpsdrError` if that fails. Uses the session's default options.

## `crates/radioctl-protocols/src/virtual_rig.rs`

### `fn elecraft_command(&mut self, text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_elecraft_command]

The virtual Elecraft rig's command table: given one tokenized command (terminator removed, case ignored) it changes the rig state and returns the answer bytes, empty for settings and for commands that send no reply. It handles frequency `FA`/`FB`, mode `MD`, `TQ`, `TX`/`RX` (which only set the in-memory key flag), `SM`, `PO`, split `FT`/`FR`, `RT`, `XT`, `RO`, `RC`, `BW`, `SW`, `ID`, `IF`, revisions `RV`, `AI` (auto-information, whose value 1 also answers with an `IF`), `PS`, `OM`, `K2`/`K3`, `KY` (text is recorded, the EOT stop empties the record) and the attenuator, preamp and numeric levels. An unknown command or a malformed value is answered `?;`. Mutates the state; no I/O, no RF.

### `fn set_meta(&mut self, digit: &str, max: u8, k2: bool) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_elecraft_set_meta]

Stores a `K2n` or `K3n` meta-command value. `digit` must be one decimal digit no larger than `max` (3 for `K2`, 1 for `K3`); `k2` chooses which field is stored. Returns `None` (no reply) on success and `Some("?;")` for anything else. The value is only remembered: the virtual rig keeps answering in the basic formats whatever is stored, so the extended formats are not emulated.

### `fn elecraft_level(&mut self, level: Level, data: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_elecraft_level]

Reads or sets one numeric level (AF, RF gain, mic, monitor, keyer speed, RF power) of the virtual Elecraft rig. With empty `data` it answers `<prefix><value:03>;` from the stored level, or the model's minimum if none is stored. With three digits it stores the value clamped to the model's range (`level_wire_for`) and returns `None`; anything that is not exactly three digits or does not fit is answered `?;`. So a set outside the range is accepted but stored at the limit. Mutates the state; no RF.

### `fn elecraft_if(&self) -> String` [@ANCHOR: radioctl:radioctl_virtual_elecraft_if]

Builds the `IF` answer for the current state: 11-digit VFO A frequency, five blanks, the RIT/XIT offset (sign and four digits), RIT (receiver incremental tuning) and XIT (transmitter incremental tuning) flags, a fixed ` 00`, the transmit flag, the mode code, a fixed `00`, the split flag and a constant `001 ;` tail. The mode is `0` if the description has no code for the mode. Pure, no errors. It is the inverse of `decode_if` in the Elecraft engine and is also pushed by auto-information.

### `fn elecraft_panel(&self, change: PanelChange) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_elecraft_panel]

Builds what a virtual Elecraft rig sends unasked after a front-panel change, depending on its auto-information level. Level 0: nothing. Level 1: an `IF` answer for any change but PTT and signal. Levels 2 and 3: `FA<hz>;` for a frequency change, `MD<code>;` for a mode change (nothing if the mode has no code), and `IF` for split and offset changes. PTT and signal changes never produce output. Pure; called after the state has already been updated.

### `fn elecraft_power(&self) -> u16` [@ANCHOR: radioctl:radioctl_virtual_elecraft_power]

The virtual rig's `PO` reading: zero unless keyed; while keyed, the stored RF power level (taken as watts) times 10, so the unit is tenths of a watt, capped at 999. Uses saturating arithmetic and a missing stored level counts as 0. Pure, no errors.

### `fn elecraft_id(&self) -> u16` [@ANCHOR: radioctl:radioctl_virtual_elecraft_id]

The model identifier a virtual Elecraft rig reports for `ID`: the `model_id` from the description's Elecraft section, or 0 if the rig is not an Elecraft wire. Pure. The Elecraft command table formats it as three digits.

### `pub fn from_description(desc: &RigDescription) -> Result<VirtualRig, RigError>` [@ANCHOR: radioctl:radioctl_virtual_from_description]

Builds a virtual rig of the described model in its default state. `desc` is borrowed and cloned. The wire protocol follows the description's family (CI-V, Kenwood, Elecraft, Kenwood handheld, Yaesu legacy, receiver, Yaesu newcat) and its family block; a description lacking the block for its family gives `Unsupported` ("description lacks its family block"), and a receiver description can fail with that module's own error. Initial state: both VFOs at the default frequency and mode, not keyed, signal 30, no CW sent, transceive on only for CI-V, filter width 500 Hz, default levels (Kenwood models start with their own monitor, keyer speed and RF power), SWR 10, auto-information 0. Nothing here touches the transmitter.

### `pub fn state(&self) -> &RigState` [@ANCHOR: radioctl:radioctl_virtual_state]

Borrows the virtual rig's current `RigState` so a test can assert on it (frequencies, mode, key flag, CW text, levels). Read-only, cannot fail, no locking (a `VirtualRig` is owned by one caller and is not `Sync`-shared).

### `pub fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_feed]

Consumes the bytes a controller wrote at time `now` (a monotonic duration) and returns the bytes the radio sends back. It dispatches on the wire protocol: CI-V frames go through the frame parser and `civ_frame` (junk is ignored); Kenwood, Elecraft and Yaesu commands go through the tokenizer (junk, and a client command it cannot parse, is answered `?;`, except the Elecraft stop `KY <space><EOT>;`, which is accepted); Yaesu legacy collects five-byte blocks and drops a partial block after the partial-frame timeout; Kenwood handheld and receiver rigs go to their own simulators. Returns an empty vector when nothing is due. Mutates the state; no I/O, no timers. It cannot transmit; keying sets `state.ptt`.

### `pub fn front_panel(&mut self, change: PanelChange) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_front_panel]

Simulates the operator using the radio's own controls: tuning VFO A, changing mode, pressing PTT, changing signal strength, switching split, or changing the RIT/XIT offset. The state is changed first. Elecraft rigs then answer with `elecraft_panel` according to their auto-information level. Other rigs return bytes only when transceive is on: CI-V sends a broadcast frequency or mode frame, Kenwood and Yaesu send `FA` or `MD` for those two changes; everything else, and every change with transceive off, returns an empty vector. `PanelChange::Ptt(true)` only sets the in-memory key flag.

### `fn drop_leading_junk(&mut self, out: &mut Vec<Parsed>)` [@ANCHOR: radioctl:radioctl_civ_parser_drop_leading_junk]

Used by the CI-V frame parser when no `FE FE` preamble is in the buffer: everything pending is moved into `out` as one `Parsed::Junk`, except a trailing single `FE`, which is kept because the second half of the preamble may arrive in the next read. Nothing is dropped when the buffer is empty or is only a lone `FE`. `out` is appended to (borrowed mutably); the parser's own buffer is shortened. No errors, no I/O.

### `fn frame_or_junk(raw: Vec<u8>) -> Parsed` [@ANCHOR: radioctl:radioctl_civ_frame_or_junk]

Turns a byte run that begins with `FE FE` and ends with `FD` into a `Parsed::Frame`, or into `Parsed::Junk` if it is too short. If a later `FE FE` appears inside the run, only the part from the last such preamble is considered (a collision on the bus restarts the frame). The frame needs at least destination, source and command bytes; those, and the rest as data, fill the `Frame`. `raw` is owned. Pure, never panics.

### `fn decode(&mut self, frame: &Frame) -> Option<Inbound>` [@ANCHOR: radioctl:radioctl_civ_engine_decode]

Decodes one CI-V frame the rig sent. It returns `None` (no event) for our own echoed frames (source is the controller), for frames not addressed to the controller or broadcast, for frames not from the configured rig address, and for scope frames that do not complete a sweep (command 27 goes to the scope assembler). A PTT echo `1C 00 <state>` counts as the acknowledgement while a PTT set is awaited. `FB` is `Ack` and `FA` is `Nak`. Recognised replies become observations: frequency (`00`, `03`, `25`), mode (`01`, `04`, `26 00`, `1A 34`), split, RIT offset, RIT and XIT, levels (`14`, with keyer speed converted to words per minute; the attenuator `11` and preamp `16 02` through the description's steps), filter width, model id, PTT, and S-meter, power and other raw meters (values beyond the engine's limits are rejected). Any other frame, or one whose data does not parse, becomes `Unrecognised` with the frame bytes. Mutates only the awaiting-PTT flag and the scope assembler.

## `crates/radioctl-protocols/src/kenwood.rs`

### `fn decode(&self, text: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_kenwood_engine_decode]

Decodes one tokenized Kenwood reply (terminator removed). `?` is `Nak`. On the older dialects (TS-440, TS-450, R-5000) and the TS-990 an answer in a form that rig does not give (`MD`, `FR`, `FT`, `SM`, level names and so on) is `Unrecognised`; the TS-990 reads its mode from `OM0<m>`. An `IF` answer goes to `decode_if`. Otherwise: `FA` and `FB` with exactly 11 digits (frequency), `MD` (one mode code), `SM0` with a four-digit reading, `ID` (three digits), `FR` and `FT` (receive and transmit VFO), `RT` and `XT` (flags), `RA` and `PA` through `decode_front_end`, and numeric levels through `decode_level`. A reply that does not fit becomes `Unrecognised` with the text plus `;`. Pure; never panics.

### `fn encode_ts440(&mut self, op: &Op) -> Option<Result<Request, RigError>>` [@ANCHOR: radioctl:radioctl_kenwood_engine_encode_ts440]

The encoder for the operations whose wire form differs on the older dialects (TS-440S, TS-450S, TS-690S, R-5000). Returns `None` when the classic Kenwood form applies, so the caller falls through. Differences: `GetMode` and `GetTxVfo` are read from `IF;` (those commands have no read form); `SetSplit` is `FN0;SP<n>;` on the TS-440S and R-5000 and `FR0;FT<n>;` on the TS-450S and TS-690S; `SetTransceive` is `AI<0|1>;` and is remembered for reconnection. `SetRitOffset`, `GetSignal`, level reads and writes, and `StopCw` are `Unsupported` ("not in the TS-440S command set"). Mutates only the remembered transceive flag; no I/O.

### `fn encode_ts990(&self, op: &Op) -> Option<Result<Request, RigError>>` [@ANCHOR: radioctl:radioctl_kenwood_engine_encode_ts990]

The encoder for the operations that differ on the TS-990S. Returns `None` when the classic form applies. `GetMode` is `OM0;` and `SetMode` is `OM0<code>;` (`Unsupported` if the description has no code for the mode); there is no `MD`, `FR` or `FT`. `GetTxVfo`, `SetSplit`, `GetSplit`, `GetPtt` and all level reads and writes are `Unsupported` ("not in the TS-990S command set") because the sub band is a second receiver and the transmit state is not in `IF`. `KY` still follows the description's `[kenwood.ky]` block through the classic path. Pure.

### `fn decode_front_end(&self, name: &str, digits: &str) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_kenwood_engine_decode_front_end]

Decodes an `RA` (attenuator) or `PA` (preamplifier) answer into a level observation. The attenuator carries two digits and then an always-zero two-digit pair; the preamp one digit and an always-zero second digit; either the short or the doubled form is accepted. The step number is mapped to a level value through the description's front-end table (`attenuation::value_of`). `None` for a wrong length, non-digit text, or a step the description does not have, so the reply is reported as unrecognised rather than guessed at. Pure.

### `fn front_end_set(&self, level: Level, value: u16) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_kenwood_engine_front_end_set]

Builds the set command for the attenuator (`RA<nn>;`, two digits) or the preamplifier (`PA<n>;`). `value` is the level value in the description's units (decibels for the attenuator); it is converted to the rig's step number through the description, and a value the model has no step for, or a model without the control, returns `Unsupported` (from `attenuation::step_of`). The request expects no reply. Pure, no I/O.

## `crates/radioctl-protocols/src/virtual_rig.rs` (continued)

### `pub fn power(&self) -> u16` [@ANCHOR: radioctl:radioctl_virtual_state_power]

`RigState::power`: the raw power-meter reading of the virtual rig. It is 128 while the rig is keyed and 0 otherwise. The numbers are the Python simulator's raw values, not a model of a real radio's meter. Pure, no errors.

### `fn civ_frame(&mut self, frame: &Frame) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_civ_frame]

Handles one CI-V frame sent to the virtual rig and returns the wire bytes it answers with. Frames are ignored (empty output) when the rig is not a CI-V one, when they are neither for the rig's address nor for broadcast, and when they come from neither the controller nor the broadcast address and are not addressed to the rig. If the description says the bus echoes (`echo`), the received frame is returned first. The answer goes to the sender: `FB` for OK, `FA` for NG, or a data frame, as `civ_dispatch` decided. Mutates the state; no I/O.

### `fn civ_dispatch(&mut self, frame: &Frame, params: &CivParams) -> CivReply` [@ANCHOR: radioctl:radioctl_virtual_civ_dispatch]

The virtual CI-V command table: matches the command and sub-command bytes of a frame, performs the change on the virtual rig's state and returns `CivReply::Ok`, `Ng` or `Data`. It covers frequency reads and sets for the selected and the other VFO (`03`, `05`, `25`), VFO select, copy and swap (`07`), split (`0F`), RIT/XIT (`21`), attenuator and preamp (`11`, `16 02`), levels (`14`), filter (`1A 03`), the scope (command 27, `Ng` if the model has none), mode reads and sets (`04`, `06`, `26`, `1A 06`), PTT (`1C 00`, only the in-memory key flag), CW text and stop (`17`), the model id (`19`), and the S-meter, power and raw meters (`15`). Models with the `F8101` command set also take its frequency and mode forms. Anything unrecognised, malformed or out of range is `Ng`. No I/O; it cannot transmit.

### `fn civ_mode_reply(&self, cmd: u8, prefix: &[u8]) -> CivReply` [@ANCHOR: radioctl:radioctl_virtual_civ_mode_reply]

Builds the data reply of a mode read. `cmd` is `0x04` (answers `code filter`) or `0x26` (answers `selector code data filter`, with `prefix` carrying the selector byte). A data mode is shown as its sideband's code with the DATA byte set; `04` has no data flag so it shows the plain sideband. The filter number is always 1. If the description has no code for the mode's base mode the result is `CivReply::Ng`. Pure. Hamlib reads the filter byte as the DATA flag when the flag byte is missing (found by the rigctld oracle test).
